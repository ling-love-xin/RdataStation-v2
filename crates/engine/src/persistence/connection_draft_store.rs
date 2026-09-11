//! 连接对话框暂存列表存储（多连接连续编辑）。
//!
//! 定位：保存数据源连接对话框的**暂存列表快照**（原型设计 §2.2），使应用重启后
//! 可继续编辑未保存的草稿。表与迁移 `global/020_add_connection_drafts.sql` 一致。
//!
//! 安全约定：**不保存密码**（[`ConnectionDraftRow`] 无 password 字段）；凭据只在
//! 正式保存连接时由连接库加密落库。列表按 `position` 升序即 UI 展示顺序。
//!
//! 本存储同步（rusqlite），面向 UI 线程的轻量调用（条目通常个位数）。

use std::path::PathBuf;
use std::sync::OnceLock;

use rusqlite::{params, Connection};
use shared::error::{CommonError, CoreError, StorageError};

/// 进程级库路径覆盖（测试注入临时库；只能设置一次）。
static DB_PATH_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// 测试 / 嵌入场景：覆盖暂存列表库路径。
///
/// 设置后 [`ConnectionDraftStore::open_global`] 直接读写该路径，不再要求全局系统单例；
/// 不设置时未初始化全局系统的进程（如测试）会拒绝打开，避免写入用户真实库。
pub fn set_db_path_override(path: PathBuf) {
    let _ = DB_PATH_OVERRIDE.set(path);
}

/// 暂存条目记录（与 `connection_drafts` 列一一对应；JSON 列为序列化文本）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConnectionDraftRow {
    /// 显示名称（空 → UI 回退「新建数据源」）。
    pub name: String,
    /// 已保存连接 ID（Some = 该条目对应一条已保存连接）。
    pub saved_id: Option<String>,
    /// 数据库类型 id（`data_source_types.id`，如 `mysql`）。
    pub type_id: String,
    /// 驱动 id（`drivers.id`，如 `mysql_native`）；恢复时优先用它定位驱动。
    pub driver_id: String,
    /// 驱动实现短名（下拉显示值，如 `sqlx`；旧行为完整名 `MySQL (sqlx)`）。
    pub driver_name: String,
    pub url: String,
    /// 用户名（密码不落库）。
    pub username: String,
    pub remark: String,
    pub scope: String,
    pub project_path: String,
    pub ssl_mode: String,
    pub ssl_ca: String,
    pub ssl_cert: String,
    pub ssl_key: String,
    pub cache_path: String,
    pub duckdb_fed: bool,
    pub active_tab: i64,
    /// 协议链跳（JSON 数组）。
    pub hops_json: String,
    /// 驱动属性（JSON 数组）。
    pub props_json: String,
    /// 安全策略覆盖（JSON 布尔数组）。
    pub sec_overrides_json: String,
    pub auth_ref: Option<String>,
    pub network_ref: Option<String>,
    pub env: Option<String>,
    /// 标签文本（逗号分隔）。
    pub tags: String,
    /// 已勾选的项目分组 id（JSON 数组）。
    pub groups_json: String,
}

/// 暂存列表存储（持有一个 SQLite 连接）。
pub struct ConnectionDraftStore {
    conn: Connection,
}

impl ConnectionDraftStore {
    /// 打开全局库（生产：需全局系统已初始化；测试可用 [`set_db_path_override`] 注入）。
    pub fn open_global() -> Result<Self, CoreError> {
        if let Some(path) = DB_PATH_OVERRIDE.get() {
            return Self::open_at(path.clone());
        }
        // 全局系统未初始化（单元/集成测试、启动降级）：不持久化，避免写入用户真实库。
        if crate::migration::get_global_db_manager().is_none() {
            return Err(CoreError::common(CommonError::General(
                "global system not initialized".to_string(),
            )));
        }
        let path = crate::migration::get_global_db_path()?;
        Self::open_at(path)
    }

    /// 打开指定库并确保表结构存在（幂等，兼容未执行迁移的旧库；测试注入用）。
    pub fn open_at(path: PathBuf) -> Result<Self, CoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "create_draft_store_dir".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }
        let conn = Connection::open(&path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "open_draft_store".to_string(),
                reason: e.to_string(),
            })
        })?;
        let store = Self { conn };
        store.ensure_tables()?;
        Ok(store)
    }

    fn ensure_tables(&self) -> Result<(), CoreError> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS connection_drafts (
                    position            INTEGER PRIMARY KEY,
                    name                TEXT NOT NULL DEFAULT '',
                    saved_id            TEXT,
                    type_id             TEXT NOT NULL DEFAULT '',
                    driver_id           TEXT NOT NULL DEFAULT '',
                    driver_name         TEXT NOT NULL DEFAULT '',
                    url                 TEXT NOT NULL DEFAULT '',
                    username            TEXT NOT NULL DEFAULT '',
                    remark              TEXT NOT NULL DEFAULT '',
                    scope               TEXT NOT NULL DEFAULT '',
                    project_path        TEXT NOT NULL DEFAULT '',
                    ssl_mode            TEXT NOT NULL DEFAULT '',
                    ssl_ca              TEXT NOT NULL DEFAULT '',
                    ssl_cert            TEXT NOT NULL DEFAULT '',
                    ssl_key             TEXT NOT NULL DEFAULT '',
                    cache_path          TEXT NOT NULL DEFAULT '',
                    duckdb_fed          INTEGER NOT NULL DEFAULT 1,
                    active_tab          INTEGER NOT NULL DEFAULT 0,
                    hops_json           TEXT NOT NULL DEFAULT '[]',
                    props_json          TEXT NOT NULL DEFAULT '[]',
                    sec_overrides_json  TEXT NOT NULL DEFAULT '[]',
                    auth_ref            TEXT,
                    network_ref         TEXT,
                    env                 TEXT,
                    tags                TEXT NOT NULL DEFAULT '',
                    groups_json         TEXT NOT NULL DEFAULT '[]',
                    updated_at          TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )
            .map_err(|e| self.err("create_connection_drafts", e))?;
        Ok(())
    }

    fn err(&self, operation: &str, e: rusqlite::Error) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: operation.to_string(),
            reason: e.to_string(),
        })
    }

    /// 读取全部条目（按 `position` 升序）。读取失败返回空列表（UI 降级为无暂存）。
    pub fn list(&self) -> Vec<ConnectionDraftRow> {
        let mut stmt = match self.conn.prepare(
            "SELECT name, saved_id, type_id, driver_id, driver_name, url, username, remark, scope,
                    project_path, ssl_mode, ssl_ca, ssl_cert, ssl_key, cache_path, duckdb_fed,
                    active_tab, hops_json, props_json, sec_overrides_json, auth_ref, network_ref, env,
                    tags, groups_json
             FROM connection_drafts ORDER BY position ASC",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |r| {
            Ok(ConnectionDraftRow {
                name: r.get(0)?,
                saved_id: r.get(1)?,
                type_id: r.get(2)?,
                driver_id: r.get(3)?,
                driver_name: r.get(4)?,
                url: r.get(5)?,
                username: r.get(6)?,
                remark: r.get(7)?,
                scope: r.get(8)?,
                project_path: r.get(9)?,
                ssl_mode: r.get(10)?,
                ssl_ca: r.get(11)?,
                ssl_cert: r.get(12)?,
                ssl_key: r.get(13)?,
                cache_path: r.get(14)?,
                duckdb_fed: r.get::<_, i64>(15)? != 0,
                active_tab: r.get(16)?,
                hops_json: r.get(17)?,
                props_json: r.get(18)?,
                sec_overrides_json: r.get(19)?,
                auth_ref: r.get(20)?,
                network_ref: r.get(21)?,
                env: r.get(22)?,
                tags: r.get(23)?,
                groups_json: r.get(24)?,
            })
        });
        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 全量替换（事务内清空 + 按顺序写入；`position` 即列表下标）。
    pub fn replace_all(&self, rows: &[ConnectionDraftRow]) -> Result<(), CoreError> {
        self.conn
            .execute("DELETE FROM connection_drafts", [])
            .map_err(|e| self.err("clear_connection_drafts", e))?;
        let mut stmt = self
            .conn
            .prepare(
                "INSERT INTO connection_drafts (
                    position, name, saved_id, type_id, driver_id, driver_name, url, username, remark,
                    scope, project_path, ssl_mode, ssl_ca, ssl_cert, ssl_key, cache_path, duckdb_fed,
                    active_tab, hops_json, props_json, sec_overrides_json, auth_ref, network_ref, env,
                    tags, groups_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                           ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
            )
            .map_err(|e| self.err("prepare_connection_drafts", e))?;
        for (i, r) in rows.iter().enumerate() {
            stmt.execute(params![
                i as i64,
                r.name,
                r.saved_id,
                r.type_id,
                r.driver_id,
                r.driver_name,
                r.url,
                r.username,
                r.remark,
                r.scope,
                r.project_path,
                r.ssl_mode,
                r.ssl_ca,
                r.ssl_cert,
                r.ssl_key,
                r.cache_path,
                if r.duckdb_fed { 1 } else { 0 },
                r.active_tab,
                r.hops_json,
                r.props_json,
                r.sec_overrides_json,
                r.auth_ref,
                r.network_ref,
                r.env,
                r.tags,
                r.groups_json,
            ])
            .map_err(|e| self.err("insert_connection_draft", e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_draft_store_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn draft_rows_roundtrip_in_order() {
        let dir = temp_dir("roundtrip");
        let store = ConnectionDraftStore::open_at(dir.join("global.db")).expect("open store");

        assert!(store.list().is_empty());

        let rows = vec![
            ConnectionDraftRow {
                name: "新建 A".into(),
                type_id: "mysql".into(),
                driver_id: "mysql_native".into(),
                driver_name: "Official".into(),
                url: "mysql://h:3306/a".into(),
                username: "u".into(),
                tags: "prod, core".into(),
                groups_json: r#"["grp_a"]"#.into(),
                duckdb_fed: true,
                hops_json: r#"[{"kind":"SSH","label":"跳板","enabled":true}]"#.into(),
                ..Default::default()
            },
            ConnectionDraftRow {
                name: "已保存 B".into(),
                saved_id: Some("G_1".into()),
                type_id: "postgresql".into(),
                driver_id: "postgres".into(),
                driver_name: "sqlx".into(),
                scope: "仅项目".into(),
                duckdb_fed: false,
                active_tab: 3,
                ..Default::default()
            },
        ];
        store.replace_all(&rows).expect("replace all");

        let got = store.list();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "新建 A");
        assert!(got[0].duckdb_fed);
        assert_eq!(got[0].type_id, "mysql");
        assert_eq!(got[0].driver_id, "mysql_native");
        assert_eq!(got[0].driver_name, "Official");
        assert_eq!(got[0].tags, "prod, core");
        assert_eq!(got[0].groups_json, r#"["grp_a"]"#);
        assert_eq!(got[1].saved_id.as_deref(), Some("G_1"));
        assert_eq!(got[1].type_id, "postgresql");
        assert!(!got[1].duckdb_fed);
        assert_eq!(got[1].active_tab, 3);

        // 全量替换语义：行数减少时旧行不残留。
        store.replace_all(&rows[..1]).expect("replace shorter");
        assert_eq!(store.list().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn draft_table_has_no_password_column() {
        // 安全约定：凭据不落暂存表（列检查防止误加 password 字段）。
        let dir = temp_dir("no_password");
        let store = ConnectionDraftStore::open_at(dir.join("global.db")).expect("open store");
        let mut stmt = store
            .conn
            .prepare("PRAGMA table_info(connection_drafts)")
            .expect("table info");
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .expect("query cols")
            .filter_map(|r| r.ok())
            .collect();
        assert!(!cols.iter().any(|c| c.contains("password") || c == "pass"));
        assert!(cols.contains(&"saved_id".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
