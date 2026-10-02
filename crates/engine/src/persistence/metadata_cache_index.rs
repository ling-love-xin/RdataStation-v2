//! 元数据缓存索引表（**落盘**，§14 #19 观察项 1 收敛）。
//!
//! 背景：`metadata_identity_registry` 是**进程内** `conn_id → 指纹` 注册表（不落盘），
//! 重启即失；缓存管理需要跨会话知道"哪些指纹缓存文件被哪些连接引用 / 哪些是孤儿 /
//! 指向什么"——本模块提供持久化索引：
//!
//! - 存储：独立 SQLite 文件 `{system}/metadata_cache_index.sqlite`（全局单例；
//!   记录含 `scope`，全局与项目域各自独立计数、互不合并）；
//! - 表：按 `fingerprint` 聚合一行的引用计数（`ref_conn_ids` 逗号分隔）；
//! - 生命周期：连接登记指纹时 `record`（引用 +1 / 更新描述与最后使用）；
//!   连接删除时 `release`（引用 -1）；`ref_conn_ids` 为空 → 孤儿，可 `prune`；
//! - 失败降级：索引写盘失败只 warn（索引是诊断 / 管理用途，不影响缓存路径）。
//!
//! 注：`project_path` 由 registry 层推导不到（索引层无项目上下文），本轮留空；
//! 缓存管理 UI 轮再按 `conn_id` 反查补全。

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use shared::error::{CommonError, CoreError};

/// 索引库路径覆盖（仅测试用：**线程局部**——并行测试各自隔离，互不打断；
/// 生产路径固定 `{system}/metadata_cache_index.sqlite`）。
thread_local! {
    static INDEX_PATH_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn index_path_override() -> Option<PathBuf> {
    INDEX_PATH_OVERRIDE.with(|c| c.borrow().clone())
}

#[cfg(test)]
fn set_index_path_override_for_tests(path: PathBuf) {
    INDEX_PATH_OVERRIDE.with(|c| *c.borrow_mut() = Some(path));
}

/// 缓存作用域域（与 `id_prefix::uses_project_storage` 同源判定，域内共享）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheScope {
    /// 全局域（`G_` / `GP_` 全局侧）
    Global,
    /// 项目域（`P_` / `GP_` 项目侧）
    Project,
}

/// 索引条目（缓存管理 / 诊断展示用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIndexEntry {
    /// 身份指纹（缓存文件名 `meta_{fp}.sqlite`）
    pub fingerprint: String,
    /// 作用域域
    pub scope: CacheScope,
    /// 项目路径（registry 层推导不到时为空）
    pub project_path: Option<String>,
    /// 可读描述（最后登记连接的规范化串）
    pub description: String,
    /// 引用该缓存的连接 id 列表
    pub ref_conn_ids: Vec<String>,
    /// 最后使用时间（unix 秒）
    pub last_used_at: i64,
    /// 创建时间（unix 秒）
    pub created_at: i64,
}

/// 索引库文件路径：`{system}/metadata_cache_index.sqlite`（测试覆盖时用覆盖路径）。
pub fn index_path() -> Result<PathBuf, CoreError> {
    if let Some(p) = index_path_override() {
        return Ok(p);
    }
    Ok(crate::migration::get_system_dir()?.join("metadata_cache_index.sqlite"))
}

/// 打开（或创建）索引库；WAL + busy_timeout（与元数据缓存连接风格一致）。
fn open() -> Result<Connection, CoreError> {
    let path = index_path()?;
    let conn = Connection::open(&path).map_err(|e| {
        CoreError::storage(shared::error::StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "open_metadata_cache_index".to_string(),
            reason: e.to_string(),
        })
    })?;
    let _ = conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA busy_timeout=5000; \
         PRAGMA synchronous=NORMAL; \
         PRAGMA foreign_keys=ON;",
    );
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata_cache_index (
            fingerprint TEXT PRIMARY KEY,
            scope TEXT NOT NULL,
            project_path TEXT,
            description TEXT NOT NULL DEFAULT '',
            ref_conn_ids TEXT NOT NULL DEFAULT '',
            last_used_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        );",
    )
    .map_err(|e| {
        CoreError::storage(shared::error::StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "create_metadata_cache_index".to_string(),
            reason: e.to_string(),
        })
    })?;
    Ok(conn)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn scope_of(conn_id: &str) -> CacheScope {
    if crate::persistence::id_prefix::uses_project_storage(conn_id) {
        CacheScope::Project
    } else {
        CacheScope::Global
    }
}

fn scope_str(scope: CacheScope) -> &'static str {
    match scope {
        CacheScope::Global => "global",
        CacheScope::Project => "project",
    }
}

fn parse_scope(s: &str) -> CacheScope {
    if s == "project" {
        CacheScope::Project
    } else {
        CacheScope::Global
    }
}

/// 登记引用：指纹缓存被 `conn_id` 引用（幂等：同 conn_id 重复登记不叠加计数）。
///
/// - 已存在同指纹行 → 追加 conn_id（去重）、更新描述与最后使用；
/// - 不存在 → 新建行（ref_count 隐含于 `ref_conn_ids`）。
pub fn record(fp: &str, conn_id: &str, description: &str) -> Result<(), CoreError> {
    let conn = open()?;
    let scope = scope_of(conn_id);
    let now = now_secs();

    let existing: Option<String> = conn
        .query_row(
            "SELECT ref_conn_ids FROM metadata_cache_index WHERE fingerprint = ?1",
            [fp],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| io_err(e))?;

    let new_refs = match existing {
        Some(refs) => {
            let mut list: Vec<String> = refs
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if !list.iter().any(|c| c == conn_id) {
                list.push(conn_id.to_string());
            }
            list
        }
        None => vec![conn_id.to_string()],
    };

    conn.execute(
        "INSERT INTO metadata_cache_index (fingerprint, scope, project_path, description, ref_conn_ids, last_used_at, created_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?5)
         ON CONFLICT(fingerprint) DO UPDATE SET
           scope = ?2,
           description = ?3,
           ref_conn_ids = ?4,
           last_used_at = ?5",
        params![fp, scope_str(scope), description, new_refs.join(","), now],
    )
    .map_err(|e| io_err(e))?;
    Ok(())
}

/// 释放引用：`conn_id` 从所有指纹行的引用列表中移除（连接删除时调用）。
/// 引用列表变空 → 该指纹成为孤儿（`orphans()` 可见，`prune_orphans()` 可清理）。
pub fn release(conn_id: &str) -> Result<(), CoreError> {
    let conn = open()?;
    let rows: Vec<(String, String)> = conn
        .prepare(
            "SELECT fingerprint, ref_conn_ids FROM metadata_cache_index WHERE ref_conn_ids != ''",
        )
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .map_err(|e| io_err(e))?;

    for (fp, refs) in rows {
        let list: Vec<String> = refs
            .split(',')
            .filter(|s| !s.is_empty() && *s != conn_id)
            .map(|s| s.to_string())
            .collect();
        conn.execute(
            "UPDATE metadata_cache_index SET ref_conn_ids = ?1 WHERE fingerprint = ?2",
            params![list.join(","), fp],
        )
        .map_err(|e| io_err(e))?;
    }
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<CacheIndexEntry> {
    Ok(CacheIndexEntry {
        fingerprint: row.get(0)?,
        scope: parse_scope(&row.get::<_, String>(1)?),
        project_path: row.get(2)?,
        description: row.get(3)?,
        ref_conn_ids: row
            .get::<_, String>(4)?
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
        last_used_at: row.get(5)?,
        created_at: row.get(6)?,
    })
}

/// 全部索引条目（含孤儿；按最后使用倒序）。
pub fn list() -> Result<Vec<CacheIndexEntry>, CoreError> {
    let conn = open()?;
    let mut stmt = conn
        .prepare(
            "SELECT fingerprint, scope, project_path, description, ref_conn_ids, last_used_at, created_at
             FROM metadata_cache_index ORDER BY last_used_at DESC",
        )
        .map_err(|e| io_err(e))?;
    let rows = stmt
        .query_map([], row_to_entry)
        .map_err(|e| io_err(e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io_err(e))?;
    Ok(rows)
}

/// 孤儿条目：引用列表为空的指纹（连接已删除 / 释放但缓存文件未清）。
pub fn orphans() -> Result<Vec<CacheIndexEntry>, CoreError> {
    Ok(list()?.into_iter().filter(|e| e.ref_conn_ids.is_empty()).collect())
}

/// 清理孤儿索引行（**不删除缓存文件**——文件清理由缓存管理轮决定）。
/// 返回清理行数。
pub fn prune_orphans() -> Result<usize, CoreError> {
    let conn = open()?;
    let n = conn
        .execute("DELETE FROM metadata_cache_index WHERE ref_conn_ids = ''", [])
        .map_err(|e| io_err(e))?;
    Ok(n)
}

fn io_err(e: rusqlite::Error) -> CoreError {
    CoreError::storage(shared::error::StorageError::Persistence {
        store: "sqlite".to_string(),
        operation: "metadata_cache_index".to_string(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试使用独立索引库路径（并行互不污染）。
    /// 注意：覆盖路径是全局的，并行测试可能写入其中——断言一律按指纹过滤，
    /// 不依赖"库里只有自己"；conn_id 使用唯一前缀避免与他模块测试冲突。
    fn isolated(f: impl FnOnce()) {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rds_idx_test_{}_{}_{}",
            std::process::id(),
            now_secs(),
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        set_index_path_override_for_tests(dir.join("metadata_cache_index.sqlite"));
        f();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn by_fp<'a>(fp: &str, entries: &'a [CacheIndexEntry]) -> Option<&'a CacheIndexEntry> {
        entries.iter().find(|e| e.fingerprint == fp)
    }

    #[test]
    fn record_release_roundtrip() {
        isolated(|| {
            let fp = "fp_idx_a";
            record(fp, "G_conn_idx_1", "v1|mysql|net:db.internal/orders|user:app").unwrap();
            record(fp, "G_conn_idx_2", "v1|mysql|net:db.internal/orders|user:app").unwrap();

            let all = list().unwrap();
            let e = by_fp(fp, &all).expect("同指纹应聚合一行为一条");
            assert_eq!(e.scope, CacheScope::Global);
            assert_eq!(e.ref_conn_ids, vec!["G_conn_idx_1", "G_conn_idx_2"]);
            assert!(e.description.contains("db.internal"));

            // 幂等：同 conn_id 重复登记不叠加
            record(fp, "G_conn_idx_1", "v1|mysql|net:db.internal/orders|user:app").unwrap();
            let all = list().unwrap();
            assert_eq!(by_fp(fp, &all).unwrap().ref_conn_ids.len(), 2);

            // 释放一个 → 剩一个
            release("G_conn_idx_1").unwrap();
            let all = list().unwrap();
            assert_eq!(by_fp(fp, &all).unwrap().ref_conn_ids, vec!["G_conn_idx_2"]);

            // 全释放 → 孤儿；prune 后从索引消失
            release("G_conn_idx_2").unwrap();
            let orphans = orphans().unwrap();
            assert!(by_fp(fp, &orphans).is_some());
            assert!(prune_orphans().unwrap() >= 1);
            let all = list().unwrap();
            assert!(by_fp(fp, &all).is_none());
        });
    }

    #[test]
    fn scope_is_derived_from_conn_id() {
        isolated(|| {
            record("fp_idx_p", "P_conn_idx_x", "项目连接").unwrap();
            record("fp_idx_g", "G_conn_idx_x", "全局连接").unwrap();
            record("fp_idx_gp", "GP_conn_idx_y", "GP 项目侧").unwrap();

            let all = list().unwrap();
            assert_eq!(by_fp("fp_idx_p", &all).unwrap().scope, CacheScope::Project);
            assert_eq!(by_fp("fp_idx_g", &all).unwrap().scope, CacheScope::Global);
            assert_eq!(
                by_fp("fp_idx_gp", &all).unwrap().scope,
                CacheScope::Project,
                "GP_ 项目侧归项目域"
            );
        });
    }

    #[test]
    fn distinct_fingerprints_are_separate_rows() {
        isolated(|| {
            record("fp_idx_a", "G_conn_idx_1", "库 A").unwrap();
            record("fp_idx_b", "G_conn_idx_2", "库 B").unwrap();
            let all = list().unwrap();
            assert!(by_fp("fp_idx_a", &all).is_some());
            assert!(by_fp("fp_idx_b", &all).is_some());
            assert_ne!(
                by_fp("fp_idx_a", &all).unwrap().fingerprint,
                by_fp("fp_idx_b", &all).unwrap().fingerprint
            );
        });
    }
}
