//! 插件驱动行（`drivers` 表的 `driver_kind='sidecar'` 子集）的落库。
//!
//! 为什么存在：连接对话框的驱动目录读的是 `drivers` 表（`get_all_drivers`），而插件
//! 激活只注册进运行时 `DriverRegistry`——两个世界没桥，插件驱动在新建连接界面不可见
//! （2026-11-10 核查结论）。本模块是桥的 SQL 侧：
//!
//! - **归属标记 = `driver_kind = 'sidecar'`**：内置驱动声明（`declaration.rs`）从不写
//!   这个值，所以「sidecar 行的集合」就是「插件行的集合」，不需要归属列；
//! - 行内容（config_schema / url_template / default_port …）由插件侧派生
//!   （`crates/plugin/src/driver_decl.rs`），本层只认 `PluginDriverRow`；
//! - `enabled` 的所有权与内置声明不同：内置 upsert **不**碰冲突行的 `enabled`（归库/用户），
//!   插件行的 `enabled` 跟随插件启停（激活=1，停用=0），所以这里冲突时**要**更新它；
//! - 陈旧行按全量重算清理：`driver_kind='sidecar'` 且不在当前存活集合里的行删除
//!   （插件卸载 / 目录消失后，下拉里不再留幽灵行）。
//!
//! 与 `declaration.rs` 同一套 upsert 列序，列含义见迁移 008。

use rusqlite::{Connection, params};
use shared::error::{CoreError, StorageError};

fn storage_err(operation: &str, reason: String) -> CoreError {
    CoreError::Storage(StorageError::Persistence {
        store: "plugin_driver_store".to_string(),
        operation: operation.to_string(),
        reason,
    })
}

/// 一行插件驱动（upsert 拥有的全部业务列；`driver_kind` 固定 `'sidecar'`、
/// `is_file = 0`、`driver_properties = '{}'` 由 SQL 侧写死）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDriverRow {
    /// 驱动 id（= 运行时 `DriverRegistry` 的注册键 = `contributes.drivers[].id`）
    pub id: String,
    /// 数据库族 id（`data_source_types.id` 外键；族不存在时先 [`ensure_plugin_family`]）
    pub type_id: String,
    pub name: String,
    pub default_port: Option<i64>,
    pub url_template: Option<String>,
    pub version: String,
    /// 连接表单字段表（JSON：`{"fields":[{key,label,type,required,default?,placeholder?}],"options":[]}`，
    /// 与内置声明 `config_schema_json` 同构）
    pub config_schema: String,
    /// JSON 数组字符串（如 `["password"]`；无认证面给 `[]`）
    pub supported_auth_types: String,
    /// JSON 数组字符串（清单 `features`）
    pub capabilities: String,
    /// 跟随插件启停：激活 1 / 停用 0（对话框据此置灰或隐藏）
    pub enabled: bool,
}

/// 幂等 upsert 一行插件驱动。
///
/// 与内置声明的 upsert 有一点刻意不同（见模块头）：冲突时**更新 `enabled`**
/// （插件启停是这一行的真实状态）。族必须先在 `data_source_types` 里——
/// 调用方先 [`ensure_plugin_family`]，这里不替它猜。
pub fn upsert_plugin_driver_row(conn: &Connection, row: &PluginDriverRow) -> Result<(), CoreError> {
    conn.execute(
        "INSERT INTO drivers
             (id, type_id, name, driver_kind, is_file, default_port, url_template,
              version, config_schema, supported_auth_types, capabilities,
              driver_properties, enabled)
         VALUES (?1, ?2, ?3, 'sidecar', 0, ?4, ?5, ?6, ?7, ?8, ?9, '{}', ?10)
         ON CONFLICT(id) DO UPDATE SET
             type_id = excluded.type_id,
             name = excluded.name,
             driver_kind = excluded.driver_kind,
             is_file = excluded.is_file,
             default_port = excluded.default_port,
             url_template = excluded.url_template,
             version = excluded.version,
             config_schema = excluded.config_schema,
             supported_auth_types = excluded.supported_auth_types,
             capabilities = excluded.capabilities,
             driver_properties = excluded.driver_properties,
             enabled = excluded.enabled",
        params![
            row.id,
            row.type_id,
            row.name,
            row.default_port,
            row.url_template,
            row.version,
            row.config_schema,
            row.supported_auth_types,
            row.capabilities,
            row.enabled as i64,
        ],
    )
    .map_err(|e| storage_err("upsert_plugin_driver_row", e.to_string()))?;
    Ok(())
}

/// 确保数据库族行存在（`INSERT OR IGNORE`：已有行——包括内置声明播种的——零改动）。
///
/// 插件驱动常对应既有族（oracle / postgresql…）；没有对应族时按插件声明的
/// `category`（默认 relational）建一行，图标留空（插件图标是资源路径，这里是
/// emoji 字符位，塞路径会显示文件名）。
pub fn ensure_plugin_family(
    conn: &Connection,
    family_id: &str,
    name: &str,
    category: &str,
) -> Result<(), CoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO data_source_types (id, name, category, icon, enabled)
         VALUES (?1, ?2, ?3, NULL, 1)",
        params![family_id, name, category],
    )
    .map_err(|e| storage_err("ensure_plugin_family", e.to_string()))?;
    Ok(())
}

/// 清理陈旧的插件驱动行：`driver_kind='sidecar'` 且 id 不在存活集合里的删除。
///
/// 只动 sidecar 行——内置声明（native/jdbc…）永不匹配条件。存活集合为空时
/// 删光全部 sidecar 行（没有任何插件驱动的合法状态）。返回删除行数（日志/测试断言用）。
pub fn delete_stale_plugin_driver_rows(
    conn: &Connection,
    keep_ids: &[String],
) -> Result<usize, CoreError> {
    let affected = if keep_ids.is_empty() {
        conn.execute("DELETE FROM drivers WHERE driver_kind = 'sidecar'", [])
    } else {
        let placeholders = keep_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "DELETE FROM drivers WHERE driver_kind = 'sidecar' AND id NOT IN ({placeholders})"
        );
        conn.execute(&sql, rusqlite::params_from_iter(keep_ids.iter()))
    }
    .map_err(|e| storage_err("delete_stale_plugin_driver_rows", e.to_string()))?;
    Ok(affected)
}

/// `drivers` 表里现有的插件驱动 id 集合（诊断与测试用）。
pub fn list_plugin_driver_ids(conn: &Connection) -> Result<Vec<String>, CoreError> {
    let mut stmt = conn
        .prepare("SELECT id FROM drivers WHERE driver_kind = 'sidecar' ORDER BY id")
        .map_err(|e| storage_err("list_plugin_driver_ids", e.to_string()))?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| storage_err("list_plugin_driver_ids", e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("list_plugin_driver_ids", e.to_string()))?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_with_tables() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        conn.execute_batch(
            "CREATE TABLE data_source_types (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT NOT NULL,
                icon TEXT, enabled BOOLEAN DEFAULT 1,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE drivers (
                id TEXT PRIMARY KEY,
                type_id TEXT NOT NULL REFERENCES data_source_types(id),
                name TEXT NOT NULL, driver_kind TEXT DEFAULT 'native',
                is_file BOOLEAN DEFAULT 0, default_port INTEGER, url_template TEXT,
                download_url TEXT, download_checksum TEXT, version TEXT,
                config_schema TEXT NOT NULL, supported_auth_types TEXT,
                capabilities TEXT, driver_properties TEXT,
                enabled BOOLEAN DEFAULT 1,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);",
        )
        .expect("create tables");
        conn
    }

    fn sample_row(id: &str, enabled: bool) -> PluginDriverRow {
        PluginDriverRow {
            id: id.to_string(),
            type_id: "oracle".to_string(),
            name: "Oracle Database".to_string(),
            default_port: Some(1521),
            url_template: Some("jdbc:oracle:thin:@//{host}:{port}/{database}".into()),
            version: "0.1.0".to_string(),
            config_schema: r#"{"fields":[],"options":[]}"#.to_string(),
            supported_auth_types: r#"["password"]"#.to_string(),
            capabilities: r#"["transactions","cancel"]"#.to_string(),
            enabled,
        }
    }

    #[test]
    fn upsert_requires_family_and_is_idempotent() {
        let conn = conn_with_tables();
        ensure_plugin_family(&conn, "oracle", "Oracle", "relational").unwrap();
        upsert_plugin_driver_row(&conn, &sample_row("oracle", true)).unwrap();
        // 幂等：同一行再 upsert 不报错、不翻倍
        upsert_plugin_driver_row(&conn, &sample_row("oracle", true)).unwrap();
        assert_eq!(list_plugin_driver_ids(&conn).unwrap(), vec!["oracle"]);
    }

    #[test]
    fn enabled_follows_plugin_state_on_conflict() {
        let conn = conn_with_tables();
        ensure_plugin_family(&conn, "oracle", "Oracle", "relational").unwrap();
        upsert_plugin_driver_row(&conn, &sample_row("oracle", true)).unwrap();
        // 停用插件 → 同 id 重 upsert，enabled 翻 0
        upsert_plugin_driver_row(&conn, &sample_row("oracle", false)).unwrap();
        let enabled: i64 = conn
            .query_row("SELECT enabled FROM drivers WHERE id='oracle'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(enabled, 0);
    }

    #[test]
    fn stale_delete_only_touches_sidecar_rows() {
        let conn = conn_with_tables();
        ensure_plugin_family(&conn, "oracle", "Oracle", "relational").unwrap();
        ensure_plugin_family(&conn, "mysql", "MySQL", "relational").unwrap();
        // 内置 native 行 + 两个插件行
        conn.execute(
            "INSERT INTO drivers (id, type_id, name, driver_kind, config_schema)
             VALUES ('mysql', 'mysql', 'MySQL (sqlx)', 'native', '{}')",
            [],
        )
        .unwrap();
        upsert_plugin_driver_row(&conn, &sample_row("oracle", true)).unwrap();
        upsert_plugin_driver_row(&conn, &PluginDriverRow {
            id: "legacy".into(),
            type_id: "oracle".into(),
            ..sample_row("oracle", true)
        })
        .unwrap();

        // 只保留 oracle：legacy 行删掉，native 行不动
        let deleted = delete_stale_plugin_driver_rows(&conn, &["oracle".to_string()]).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(list_plugin_driver_ids(&conn).unwrap(), vec!["oracle"]);
        let native_still_there: i64 = conn
            .query_row("SELECT COUNT(*) FROM drivers WHERE id='mysql'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(native_still_there, 1);

        // 存活集合为空 → sidecar 行删光，native 仍在
        let deleted = delete_stale_plugin_driver_rows(&conn, &[]).unwrap();
        assert_eq!(deleted, 1);
        assert!(list_plugin_driver_ids(&conn).unwrap().is_empty());
        let native_still_there: i64 = conn
            .query_row("SELECT COUNT(*) FROM drivers WHERE id='mysql'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(native_still_there, 1);
    }

    #[test]
    fn ensure_family_ignores_existing_rows() {
        let conn = conn_with_tables();
        ensure_plugin_family(&conn, "oracle", "Oracle", "relational").unwrap();
        ensure_plugin_family(&conn, "oracle", "别的名字也不覆盖", "nosql").unwrap();
        let (name, category): (String, String) = conn
            .query_row(
                "SELECT name, category FROM data_source_types WHERE id='oracle'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "Oracle");
        assert_eq!(category, "relational");
    }
}
