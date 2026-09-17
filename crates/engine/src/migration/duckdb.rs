//! DuckDB 迁移执行器
//!
//! `MigrationManager` 基于 rusqlite（SQLite），无法打开 DuckDB 文件；
//! DuckDB 侧迁移统一走本模块：传入**已打开的** DuckDB 连接执行，
//! 避免对同一文件二次 `open`（Windows 上会因文件锁冲突报
//! "unable to open database file"；项目库初始化曾因此失败）。
//!
//! 迁移版本记录在 DuckDB 内的 `schema_version` 表，SQL 文件来自
//! `migrations/<migration_type.dir_name>/`（编译期嵌入）。

use std::path::Path;

use duckdb::Connection;

use shared::error::{CoreError, StorageError};

use super::MigrationType;

/// 用已打开的 DuckDB 连接执行指定类型的迁移，返回本次应用的迁移数。
pub fn apply_migrations(
    conn: &Connection,
    migration_type: MigrationType,
) -> Result<usize, CoreError> {
    warn_if_not_native_backend(conn);

    // 确保迁移版本表存在
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version     INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            applied_at  INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| persistence_error("create_schema_version", e))?;

    // 获取当前版本
    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .map_err(|e| persistence_error("get_current_version", e))?;

    // 加载待执行迁移
    use include_dir::include_dir;
    const MIGRATIONS_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

    let Some(dir) = MIGRATIONS_DIR.get_dir(migration_type.dir_name()) else {
        return Ok(0);
    };

    let mut migrations: Vec<(u32, String, String)> = dir
        .files()
        .filter_map(|f| {
            let filename = f.path().file_name()?.to_str()?;
            if !filename.ends_with(".sql") {
                return None;
            }
            let stem = filename.strip_suffix(".sql")?;
            let mut parts = stem.splitn(2, '_');
            let version = parts.next()?.parse::<u32>().ok()?;
            let name = parts.next()?.to_string();
            if version <= current_version {
                return None;
            }
            let sql = f.contents_utf8()?.to_string();
            Some((version, name, sql))
        })
        .collect();
    migrations.sort_by_key(|m| m.0);

    // 按版本顺序执行并记录
    let mut applied = 0usize;
    for (version, name, sql) in migrations {
        conn.execute_batch(&sql)
            .map_err(|e| persistence_error(&format!("migrate_{name}"), e))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (?1, ?2, ?3)",
            [
                &version as &dyn duckdb::ToSql,
                &name as &dyn duckdb::ToSql,
                &now as &dyn duckdb::ToSql,
            ],
        )
        .map_err(|e| persistence_error("record_version", e))?;

        tracing::info!("Applied DuckDB migration {} (version {})", name, version);
        applied += 1;
    }

    Ok(applied)
}

/// 按路径创建 / 打开 DuckDB 文件并执行指定类型的迁移，返回本次应用条数。
///
/// 与 [`apply_migrations`] 的分工：本函数负责「自开连接」，供**手上还没有 DuckDB
/// 连接**的调用方使用（如 M1 新建项目时建分析库）；项目打开流程已经有连接，
/// 继续直接用 [`apply_migrations`]，避免二次 `open` 触发 Windows 文件锁。
///
/// 为什么必须有它：`MigrationManager`（rusqlite）也能"建出"一个 `.duckdb` 文件，
/// 但那是一个 **SQLite 格式**的文件。这种文件不会报错——DuckDB 内置 SQLite 存储
/// 后端，会以 `duckdb_databases().type = 'sqlite'` 正常读写它，因此「`Connection::open`
/// 成功」这类断言会假通过。真正的代价是：同一个分析库的存储格式取决于它是哪条
/// 代码路径建的、DuckDB 与 SQLite 两侧共用同一张 `schema_version` 迁移账本
/// （编号分叉即静默跳过）、且标准 DuckDB 构建与第三方工具打开该文件会报
/// "not a valid DuckDB database file"。
pub fn migrate_at_path(db_path: &Path, migration_type: MigrationType) -> Result<usize, CoreError> {
    if let Some(parent) = db_path.parent() {
        if parent.as_os_str() != "" {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "duckdb".to_string(),
                    operation: "create_dir".to_string(),
                    reason: format!("{}: {e}", parent.display()),
                })
            })?;
        }
    }

    let conn = Connection::open(db_path).map_err(|e| persistence_error("open", e))?;
    apply_migrations(&conn, migration_type)
}

/// 查询文件的存储后端：`duckdb` = 原生；`sqlite` = 旧格式（rusqlite 建的）；
/// `None` = 查询不可用（旧版本无 `duckdb_databases().type`，或连接已失效）。
pub(crate) fn storage_backend(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT type FROM duckdb_databases() WHERE database_name = current_database()",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// 诊断：该文件的存储后端是否为 DuckDB 原生（best-effort，**绝不阻断迁移**）。
///
/// SQLite 格式的 `.duckdb` 文件 DuckDB 能正常读写，所以问题既不报错也不易察觉，
/// 只会在迁移账本上悄悄串台；这里把它显式暴露到日志，重建与否交给上层决定。
fn warn_if_not_native_backend(conn: &Connection) {
    if let Some(backend) = storage_backend(conn) {
        if backend != "duckdb" {
            tracing::warn!(
                backend = %backend,
                "分析库不是 DuckDB 原生存储后端（疑似 SQLite 格式）：DuckDB 虽能读写，但迁移账本会与 SQLite 侧共用、且第三方 DuckDB 工具打不开 —— 建议重建该文件"
            );
        }
    }
}

/// 统一的持久化错误包装（store = duckdb）。
fn persistence_error(operation: &str, e: duckdb::Error) -> CoreError {
    CoreError::Storage(StorageError::Persistence {
        store: "duckdb".to_string(),
        operation: operation.to_string(),
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立的临时库路径（每次用前清空同名目录）。
    fn temp_db_path(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rds_duckdb_mig_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("analytics.duckdb")
    }

    /// 建库产物必须是 DuckDB **原生**格式。
    ///
    /// 反例即本函数的由来：rusqlite 建出的同扩展名文件是 SQLite 格式，而 DuckDB
    /// 内置 SQLite 存储后端仍能打开它，所以只断言「能 open」测不出这个缺陷。
    #[test]
    fn migrate_at_path_creates_native_format() {
        let path = temp_db_path("native");
        let applied = migrate_at_path(&path, MigrationType::ProjectAnalysis).expect("迁移失败");
        assert!(applied >= 1, "首次至少应用 1 条迁移，实际 {applied}");

        let head = std::fs::read(&path).expect("读分析库");
        assert!(head.len() >= 12, "文件过小：{} 字节", head.len());
        // DuckDB 存储头：前 8 字节校验和，随后 4 字节魔数 "DUCK"。
        assert_eq!(&head[8..12], b"DUCK", "分析库必须是 DuckDB 原生格式");
    }

    /// 存储后端自报 duckdb —— 与上面的魔数断言互为佐证。
    #[test]
    fn backend_reports_native() {
        let path = temp_db_path("backend");
        migrate_at_path(&path, MigrationType::ProjectAnalysis).expect("迁移失败");

        let conn = Connection::open(&path).expect("打开分析库");
        let backend: String = conn
            .query_row(
                "SELECT type FROM duckdb_databases() WHERE database_name = current_database()",
                [],
                |row| row.get(0),
            )
            .expect("查询存储后端");
        assert_eq!(backend, "duckdb");
    }

    /// 重跑必须幂等（账本落在 DuckDB 自己的 schema_version 表里）。
    #[test]
    fn migrate_at_path_is_idempotent() {
        let path = temp_db_path("idempotent");
        migrate_at_path(&path, MigrationType::ProjectAnalysis).expect("首次迁移失败");
        let again = migrate_at_path(&path, MigrationType::ProjectAnalysis).expect("二次迁移失败");
        assert_eq!(again, 0, "已应用的迁移不应重复执行");
    }

    /// 旧格式（rusqlite 建出的 SQLite 格式）的分析库：
    /// ① 迁移不应被阻断（存量项目仍可用）；② 存储后端必须能被识别出来。
    ///
    /// 这条测试固定的是「检测能力」本身——它是该格式分裂缺陷唯一能被自动化发现的地方。
    #[test]
    fn detects_legacy_sqlite_backend() {
        let path = temp_db_path("legacy");
        std::fs::create_dir_all(path.parent().expect("父目录")).expect("建目录");

        // 故意用 rusqlite 造一个 SQLite 格式的 `.duckdb`（旧实现的产物）。
        {
            let conn = rusqlite::Connection::open(&path).expect("建 SQLite 文件");
            conn.execute_batch(
                "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at INTEGER NOT NULL);",
            )
            .expect("建 schema_version");
        }
        let head = std::fs::read(&path).expect("读文件");
        assert_eq!(&head[..6], b"SQLite", "前置条件：该文件应为 SQLite 格式");

        let applied =
            migrate_at_path(&path, MigrationType::ProjectAnalysis).expect("旧格式不应阻断迁移");
        assert_eq!(applied, 2, "SQLite 侧账本为空，两条 DuckDB 迁移应全部应用");

        let conn = Connection::open(&path).expect("打开分析库");
        assert_eq!(
            storage_backend(&conn).as_deref(),
            Some("sqlite"),
            "应识别出 SQLite 存储后端（告警路径据此触发）"
        );
    }
}
