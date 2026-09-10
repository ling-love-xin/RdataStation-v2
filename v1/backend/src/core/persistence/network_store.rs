use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::error::{CoreError, StorageError};

/// 网络配置（SSH 隧道、HTTP 代理、SSL 证书等）
/// config 列保留完整配置信息（含 host/port/forwarding + auth 冗余）
/// auth_config_id 引用 auth_configs.id，指向独立存储的认证凭据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NetworkConfig {
    pub id: String,
    pub name: Option<String>,
    pub network_type: String,
    pub config: String,
    pub auth_config_id: Option<String>,
    pub origin: Option<String>,
    pub source_id: Option<String>,
    pub snapshot_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "network_store".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 确保 network_configs 表存在（回退修复）
fn ensure_table(conn: &Connection) -> Result<(), CoreError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS network_configs (
            id            TEXT PRIMARY KEY,
            name          TEXT,
            network_type  TEXT NOT NULL,
            config        TEXT NOT NULL,
            auth_config_id TEXT,
            origin        TEXT,
            source_id     TEXT,
            snapshot_at   TEXT,
            created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| storage_err("ensure_table", e.to_string()))?;

    // 补充：确保迁移可能遗漏的列（ALTER TABLE ADD COLUMN 不支持 IF NOT EXISTS，忽略重复错误）
    let extra_cols = [
        "auth_config_id TEXT",
        "origin TEXT",
        "source_id TEXT",
        "snapshot_at TEXT",
    ];
    for col_def in &extra_cols {
        let _ = conn.execute(
            &format!("ALTER TABLE network_configs ADD COLUMN {}", col_def),
            [],
        );
    }
    Ok(())
}

/// 创建网络配置（项目库，含快照溯源字段）
pub fn create_network_config(conn: &Connection, nc: &NetworkConfig) -> Result<(), CoreError> {
    let _ = ensure_table(conn);
    conn.execute(
        "INSERT OR REPLACE INTO network_configs (id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            nc.id,
            nc.name,
            nc.network_type,
            nc.config,
            nc.auth_config_id,
            nc.origin,
            nc.source_id,
            nc.snapshot_at,
            nc.created_at,
            nc.updated_at
        ],
    )
    .map_err(|e| storage_err("create_network_config", e.to_string()))?;
    Ok(())
}

/// 列出网络配置，可按网络类型过滤（项目库，含快照溯源字段）
pub fn list_network_configs(
    conn: &Connection,
    network_type: Option<&str>,
) -> Result<Vec<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let (sql, param): (String, Option<String>) = if let Some(t) = network_type {
        (
            "SELECT id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at
             FROM network_configs WHERE network_type = ?1 ORDER BY name"
                .to_string(),
            Some(t.to_string()),
        )
    } else {
        (
            "SELECT id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at
             FROM network_configs ORDER BY network_type, name"
                .to_string(),
            None,
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_network_configs", e.to_string()))?;

    let items = if let Some(ref p) = param {
        stmt.query_map(params![p], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: row.get(5)?,
                source_id: row.get(6)?,
                snapshot_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| storage_err("query_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: row.get(5)?,
                source_id: row.get(6)?,
                snapshot_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| storage_err("query_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(items)
}

/// 根据 ID 获取网络配置（项目库，含快照溯源字段）
pub fn get_network_config(conn: &Connection, id: &str) -> Result<Option<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at
             FROM network_configs WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_network_config", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(NetworkConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            network_type: row.get(2)?,
            config: row.get(3)?,
            auth_config_id: row.get(4)?,
            origin: row.get(5)?,
            source_id: row.get(6)?,
            snapshot_at: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_network_config", e.to_string()))
}

/// 更新网络配置，若配置不存在则返回错误
pub fn update_network_config(conn: &Connection, nc: &NetworkConfig) -> Result<(), CoreError> {
    let rows = conn
        .execute(
            "UPDATE network_configs SET name = ?1, network_type = ?2, config = ?3, auth_config_id = ?4, updated_at = ?5 WHERE id = ?6",
            params![nc.name, nc.network_type, nc.config, nc.auth_config_id, nc.updated_at, nc.id],
        )
        .map_err(|e| storage_err("update_network_config", e.to_string()))?;

    if rows == 0 {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "network_store".to_string(),
            operation: "update_network_config".to_string(),
            reason: format!("network config not found: {}", nc.id),
        }));
    }
    Ok(())
}

/// 删除网络配置
pub fn delete_network_config(conn: &Connection, id: &str) -> Result<(), CoreError> {
    conn.execute("DELETE FROM network_configs WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_network_config", e.to_string()))?;
    Ok(())
}

// ===========================================================================
// ======================== 全局库专用函数（无 origin/source_id/snapshot_at）==
// ===========================================================================
// 全局 network_configs 表不需要快照溯源字段（全局和项目物理隔离）
// 项目 network_configs 表（有 origin 列）使用上面的通用函数

/// 全局库：创建网络配置（不含快照溯源字段，含 auth_config_id）
pub fn create_global_network_config(
    conn: &Connection,
    nc: &NetworkConfig,
) -> Result<(), CoreError> {
    let _ = ensure_table(conn);
    conn.execute(
        "INSERT INTO network_configs (id, name, network_type, config, auth_config_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            nc.id,
            nc.name,
            nc.network_type,
            nc.config,
            nc.auth_config_id,
            nc.created_at,
            nc.updated_at
        ],
    )
    .map_err(|e| storage_err("create_global_network_config", e.to_string()))?;
    Ok(())
}

/// 全局库：列出网络配置（不含快照溯源字段）
pub fn list_global_network_configs(
    conn: &Connection,
    network_type: Option<&str>,
) -> Result<Vec<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let (sql, param): (String, Option<String>) = if let Some(t) = network_type {
        (
            "SELECT id, name, network_type, config, auth_config_id, created_at, updated_at
             FROM network_configs WHERE network_type = ?1 ORDER BY name"
                .to_string(),
            Some(t.to_string()),
        )
    } else {
        (
            "SELECT id, name, network_type, config, auth_config_id, created_at, updated_at
             FROM network_configs ORDER BY network_type, name"
                .to_string(),
            None,
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_global_network_configs", e.to_string()))?;

    let items = if let Some(ref p) = param {
        stmt.query_map(params![p], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: Some("global".to_string()),
                source_id: None,
                snapshot_at: None,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| storage_err("query_global_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: Some("global".to_string()),
                source_id: None,
                snapshot_at: None,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| storage_err("query_global_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(items)
}

/// 全局库：根据 ID 获取网络配置（不含快照溯源字段）
pub fn get_global_network_config(
    conn: &Connection,
    id: &str,
) -> Result<Option<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, network_type, config, auth_config_id, created_at, updated_at
             FROM network_configs WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_global_network_config", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(NetworkConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            network_type: row.get(2)?,
            config: row.get(3)?,
            auth_config_id: row.get(4)?,
            origin: Some("global".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_global_network_config", e.to_string()))
}
