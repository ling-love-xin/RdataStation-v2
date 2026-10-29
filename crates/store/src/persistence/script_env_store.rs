//! 脚本执行环境配置表（T2b-3 管理环境）
//!
//! 通用模式的执行环境（Python / R / Rust / 自定义解释器）是**机器级配置**：一个解释器
//! 的路径在哪里、带什么参数，跟哪条数据库连接、哪个项目都无关。所以它落全局库，与
//! 认证配置、网络配置同一层。
//!
//! 注意与 [`super::env_store`] 区分：那边是**数据库环境**（生产 / 预发布 / 开发，带策略），
//! 这边是**脚本解释器**（编辑器通用模式跑脚本用的命令）。表名 `script_environments` 防混。
//!
//! ## 行形状
//!
//! `args` 以 JSON 数组存（`["-u", "--no-site"]`）：参数可以含空格，逗号分隔反而说不清；
//! `kind` 存 `EnvKind` 的持久化键（"python"/"rscript"/"rustc"/"custom"，见编辑器 env 模块的
//! `as_key`）——展示文案可改，存库的值不能。

use std::sync::Arc;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::global_db::GlobalSqlitePool;
use shared::error::{CoreError, StorageError};

/// 一条脚本执行环境配置（引擎侧行形状；编辑器侧 [`editor::env::Environment`] 由宿主转换）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ScriptEnvRow {
    /// 稳定 id（跨会话引用；自定义环境由宿主生成，如 `custom-<uuid>`）
    pub id: String,
    /// 展示名（如「Python 3.12」）
    pub name: String,
    /// 解释器类型（`EnvKind::as_key`：python / rscript / rustc / custom）
    pub kind: String,
    /// 可执行命令（`python` / `Rscript` / `rustc` / 绝对路径）
    pub command: String,
    /// 附加参数（如 `-u`；JSON 数组落库）
    pub args: Vec<String>,
    /// 创建时间（ISO 或毫秒串；仅展示用）
    pub created_at: String,
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "script_env_store".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 脚本执行环境配置存储（与 [`super::WorkbenchContextStore`] 同款：结构体包池 + 懒建表）
pub struct ScriptEnvStore {
    pool: Arc<GlobalSqlitePool>,
}

impl ScriptEnvStore {
    /// 创建存储服务（幂等建表：老库没有这张表也会补上）
    pub fn new(pool: Arc<GlobalSqlitePool>) -> Result<Self, CoreError> {
        let store = Self { pool };
        store.init_tables()?;
        Ok(store)
    }

    /// 取连接（操作完成后自动归还）
    fn with_connection<T, F: FnOnce(&Connection) -> Result<T, CoreError>>(
        &self,
        f: F,
    ) -> Result<T, CoreError> {
        let conn = self.pool.acquire_sync()?;
        f(conn.inner()?)
    }

    fn init_tables(&self) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS script_environments (
                    id          TEXT PRIMARY KEY,
                    name        TEXT NOT NULL,
                    kind        TEXT NOT NULL,
                    command     TEXT NOT NULL,
                    args        TEXT NOT NULL DEFAULT '[]',
                    created_at  TEXT NOT NULL DEFAULT ''
                )",
                [],
            )
            .map_err(|e| storage_err("init_tables", e.to_string()))?;
            Ok(())
        })
    }

    /// 新增一条环境配置（id 冲突 = 报错，不悄悄覆盖）
    pub fn create(&self, row: &ScriptEnvRow) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO script_environments (id, name, kind, command, args, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    row.id,
                    row.name,
                    row.kind,
                    row.command,
                    serde_json::to_string(&row.args)
                        .map_err(|e| storage_err("create_args_encode", e.to_string()))?,
                    row.created_at,
                ],
            )
            .map_err(|e| storage_err("create", e.to_string()))?;
            Ok(())
        })
    }

    /// 列出全部环境配置（按名称排序，界面顺序稳定）
    pub fn list(&self) -> Result<Vec<ScriptEnvRow>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, kind, command, args, created_at
                     FROM script_environments ORDER BY name",
                )
                .map_err(|e| storage_err("prepare_list", e.to_string()))?;
            let items = stmt
                .query_map([], |row| {
                    let args_json: String = row.get(4)?;
                    let args: Vec<String> =
                        serde_json::from_str(&args_json).unwrap_or_default();
                    Ok(ScriptEnvRow {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        kind: row.get(2)?,
                        command: row.get(3)?,
                        args,
                        created_at: row.get(5)?,
                    })
                })
                .map_err(|e| storage_err("query_list", e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(items)
        })
    }

    /// 更新一条环境配置（不存在 = 报错，不静默）
    pub fn update(&self, row: &ScriptEnvRow) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            let rows = conn
                .execute(
                    "UPDATE script_environments SET name = ?1, kind = ?2, command = ?3, args = ?4
                     WHERE id = ?5",
                    params![
                        row.name,
                        row.kind,
                        row.command,
                        serde_json::to_string(&row.args)
                            .map_err(|e| storage_err("update_args_encode", e.to_string()))?,
                        row.id,
                    ],
                )
                .map_err(|e| storage_err("update", e.to_string()))?;
            if rows == 0 {
                return Err(storage_err(
                    "update",
                    format!("environment not found: {}", row.id),
                ));
            }
            Ok(())
        })
    }

    /// 删除一条环境配置（不存在 = Ok，删除本就幂等）
    pub fn delete(&self, id: &str) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM script_environments WHERE id = ?1",
                params![id],
            )
            .map_err(|e| storage_err("delete", e.to_string()))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（本 crate 顶层可能有与 gpui 相关的符号）
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::ScriptEnvRow;
    use crate::persistence::global_db::GlobalSqlitePool;

    fn temp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_script_env_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("global.db")
    }

    /// 建池 + 开店（建池在自建 runtime 里，开店在离开 async 上下文之后——与
    /// `WorkbenchContextStore` 测试同一套纪律：存储层 API 是同步的，不能在 runtime 驱动线程上调）
    fn open_store(db: &PathBuf) -> super::ScriptEnvStore {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let pool = runtime.block_on(async {
            Arc::new(GlobalSqlitePool::new(db.clone(), 1).await.expect("create pool"))
        });
        drop(runtime);
        super::ScriptEnvStore::new(pool).expect("open store")
    }

    fn row(id: &str, name: &str) -> ScriptEnvRow {
        ScriptEnvRow {
            id: id.to_string(),
            name: name.to_string(),
            kind: "custom".to_string(),
            command: "/usr/bin/python3".to_string(),
            args: vec!["-u".to_string(), "--no-site".to_string()],
            created_at: "1700000000000".to_string(),
        }
    }

    /// 核心往返：create → list 原样回来（args 经 JSON 落库再读回，不丢内容）
    #[test]
    fn a_row_round_trips_through_the_table() {
        let (db,) = (temp_db("roundtrip"),);
        let store = open_store(&db);
        store.create(&row("env-1", "Python 3.12")).expect("create");

        let list = store.list().expect("list");
        assert_eq!(list.len(), 1);
        let back = &list[0];
        assert_eq!(back.id, "env-1");
        assert_eq!(back.name, "Python 3.12");
        assert_eq!(back.kind, "custom");
        assert_eq!(back.command, "/usr/bin/python3");
        // args 含空格也能经 JSON 完整往返（逗号分隔就说不清了）
        assert_eq!(back.args, vec!["-u", "--no-site"]);
    }

    /// update 真改行；改不存在的行 = 报错（不静默）
    #[test]
    fn update_changes_a_row_and_rejects_unknown_ids() {
        let db = temp_db("update");
        let store = open_store(&db);
        store.create(&row("env-1", "Python 3.12")).expect("create");

        let mut edited = row("env-1", "Python 3.12 venv");
        edited.command = "C:/venv/python.exe".to_string();
        edited.args = vec!["-u".to_string()];
        store.update(&edited).expect("update");

        let list = store.list().expect("list");
        assert_eq!(list[0].name, "Python 3.12 venv");
        assert_eq!(list[0].command, "C:/venv/python.exe");
        assert_eq!(list[0].args, vec!["-u"]);

        let missing = store.update(&row("env-404", "nope"));
        assert!(missing.is_err(), "改不存在的行必须报错");
    }

    /// delete 幂等：删存在的行之后列表为空；删不存在的行也 Ok
    #[test]
    fn delete_removes_a_row_and_is_idempotent() {
        let db = temp_db("delete");
        let store = open_store(&db);
        store.create(&row("env-1", "Python 3.12")).expect("create");
        store.delete("env-1").expect("delete");
        assert!(store.list().expect("list").is_empty());
        store.delete("env-1").expect("delete again is fine");
    }

    /// 同 id 重复 create = 报错（id 是主键：宿主生成 id 时保证唯一，这里挡住意外撞车）
    #[test]
    fn duplicate_id_is_rejected() {
        let db = temp_db("duplicate");
        let store = open_store(&db);
        store.create(&row("env-1", "Python 3.12")).expect("create");
        assert!(store.create(&row("env-1", "Another")).is_err());
    }
}
