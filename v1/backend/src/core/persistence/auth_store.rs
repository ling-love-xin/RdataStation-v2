use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::error::{CoreError, StorageError};

/// 认证配置（用户名/密码、密钥文件、Kerberos、OAuth2 等），密码字段已 AES-256-GCM 加密
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuthConfig {
    pub id: String,
    pub name: Option<String>,
    pub auth_type: String,
    pub auth_data: String,
    pub origin: Option<String>,
    pub source_id: Option<String>,
    pub snapshot_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "auth_store".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 确保 auth_configs 表存在（回退修复）
fn ensure_table(conn: &Connection) -> Result<(), CoreError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS auth_configs (
            id          TEXT PRIMARY KEY,
            name        TEXT,
            auth_type   TEXT NOT NULL,
            auth_data   TEXT NOT NULL,
            origin      TEXT,
            source_id   TEXT,
            snapshot_at TEXT,
            created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| storage_err("ensure_table", e.to_string()))?;

    // 补充：确保迁移可能遗漏的列（ALTER TABLE ADD COLUMN 不支持 IF NOT EXISTS，忽略重复错误）
    let extra_cols = ["origin TEXT", "source_id TEXT", "snapshot_at TEXT"];
    for col_def in &extra_cols {
        let _ = conn.execute(
            &format!("ALTER TABLE auth_configs ADD COLUMN {}", col_def),
            [],
        );
    }
    Ok(())
}

/// 加密 auth_data JSON 中的敏感字段（password、passphrase、clientSecret）
/// 调用方在写入 DB 前调用此函数，确保敏感字段以密文存储
pub fn encrypt_auth_data(auth_data: &str) -> Result<String, CoreError> {
    if auth_data.is_empty() {
        return Ok(auth_data.to_string());
    }

    let mut data: serde_json::Value = serde_json::from_str(auth_data)
        .map_err(|e| storage_err("parse_auth_data", format!("JSON 解析失败: {}", e)))?;

    let obj = data
        .as_object_mut()
        .ok_or_else(|| storage_err("encrypt_auth_data", "auth_data 不是 JSON 对象".to_string()))?;

    // 加密 password 字段
    if let Some(pwd) = obj.get("password").and_then(|v| v.as_str()) {
        if !pwd.is_empty() && !pwd.starts_with("AES:") {
            let encrypted = crate::core::crypto::encrypt_password(pwd)?;
            obj.insert(
                "password".to_string(),
                serde_json::Value::String(format!("AES:{}", encrypted)),
            );
        }
    }

    // 加密 passphrase 字段（SSH 密钥密码）
    if let Some(pp) = obj.get("passphrase").and_then(|v| v.as_str()) {
        if !pp.is_empty() && !pp.starts_with("AES:") {
            let encrypted = crate::core::crypto::encrypt_password(pp)?;
            obj.insert(
                "passphrase".to_string(),
                serde_json::Value::String(format!("AES:{}", encrypted)),
            );
        }
    }

    // 加密 clientSecret 字段（OAuth2）
    if let Some(cs) = obj.get("clientSecret").and_then(|v| v.as_str()) {
        if !cs.is_empty() && !cs.starts_with("AES:") {
            let encrypted = crate::core::crypto::encrypt_password(cs)?;
            obj.insert(
                "clientSecret".to_string(),
                serde_json::Value::String(format!("AES:{}", encrypted)),
            );
        }
    }

    serde_json::to_string(&data)
        .map_err(|e| storage_err("serialize_auth_data", format!("JSON 序列化失败: {}", e)))
}

/// 解密 auth_data JSON 中的敏感字段（用于读取时前端展示）
/// 以 "AES:" 前缀标识的字段会被解密
pub fn decrypt_auth_data(auth_data: &str) -> Result<String, CoreError> {
    if auth_data.is_empty() {
        return Ok(auth_data.to_string());
    }

    let mut data: serde_json::Value = serde_json::from_str(auth_data)
        .map_err(|e| storage_err("parse_auth_data", format!("JSON 解析失败: {}", e)))?;

    let obj = match data.as_object_mut() {
        Some(o) => o,
        None => return Ok(auth_data.to_string()),
    };

    for field in &["password", "passphrase", "clientSecret"] {
        if let Some(val) = obj.get(*field).and_then(|v| v.as_str()) {
            if let Some(enc) = val.strip_prefix("AES:") {
                if let Ok(decrypted) = crate::core::crypto::decrypt_password(enc) {
                    obj.insert(field.to_string(), serde_json::Value::String(decrypted));
                }
                // 解密失败 → 保持原样（防止破坏数据）
            }
        }
    }

    serde_json::to_string(&data)
        .map_err(|e| storage_err("serialize_auth_data", format!("JSON 序列化失败: {}", e)))
}

/// 创建认证配置（项目库，含快照溯源字段）
pub fn create_auth_config(conn: &Connection, ac: &AuthConfig) -> Result<(), CoreError> {
    let _ = ensure_table(conn);
    let encrypted_data = encrypt_auth_data(&ac.auth_data)?;
    conn.execute(
        "INSERT OR REPLACE INTO auth_configs (id, name, auth_type, auth_data, origin, source_id, snapshot_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            ac.id,
            ac.name,
            ac.auth_type,
            encrypted_data,
            ac.origin,
            ac.source_id,
            ac.snapshot_at,
            ac.created_at,
            ac.updated_at
        ],
    )
    .map_err(|e| storage_err("create_auth_config", e.to_string()))?;
    Ok(())
}

/// 列出认证配置，可按认证类型过滤（返回解密后的 auth_data）
pub fn list_auth_configs(
    conn: &Connection,
    auth_type: Option<&str>,
) -> Result<Vec<AuthConfig>, CoreError> {
    let _ = ensure_table(conn);
    let (sql, param): (String, Option<String>) = if let Some(t) = auth_type {
        (
            "SELECT id, name, auth_type, auth_data, origin, source_id, snapshot_at, created_at, updated_at
             FROM auth_configs WHERE auth_type = ?1 ORDER BY name"
                .to_string(),
            Some(t.to_string()),
        )
    } else {
        (
            "SELECT id, name, auth_type, auth_data, origin, source_id, snapshot_at, created_at, updated_at
             FROM auth_configs ORDER BY auth_type, name"
                .to_string(),
            None,
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_auth_configs", e.to_string()))?;

    let items = if let Some(ref p) = param {
        stmt.query_map(params![p], |row| {
            let raw_auth_data: String = row.get(3)?;
            let auth_data = decrypt_auth_data(&raw_auth_data).unwrap_or(raw_auth_data);
            Ok(AuthConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                auth_type: row.get(2)?,
                auth_data,
                origin: row.get(4)?,
                source_id: row.get(5)?,
                snapshot_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| storage_err("query_auth_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            let raw_auth_data: String = row.get(3)?;
            let auth_data = decrypt_auth_data(&raw_auth_data).unwrap_or(raw_auth_data);
            Ok(AuthConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                auth_type: row.get(2)?,
                auth_data,
                origin: row.get(4)?,
                source_id: row.get(5)?,
                snapshot_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| storage_err("query_auth_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(items)
}

/// 根据 ID 获取认证配置（返回解密后的 auth_data）
pub fn get_auth_config(conn: &Connection, id: &str) -> Result<Option<AuthConfig>, CoreError> {
    let _ = ensure_table(conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, auth_type, auth_data, origin, source_id, snapshot_at, created_at, updated_at
             FROM auth_configs WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_auth_config", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        let raw_auth_data: String = row.get(3)?;
        let auth_data = decrypt_auth_data(&raw_auth_data).unwrap_or(raw_auth_data);
        Ok(AuthConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            auth_type: row.get(2)?,
            auth_data,
            origin: row.get(4)?,
            source_id: row.get(5)?,
            snapshot_at: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_auth_config", e.to_string()))
}

/// 删除认证配置
pub fn delete_auth_config(conn: &Connection, id: &str) -> Result<(), CoreError> {
    conn.execute("DELETE FROM auth_configs WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_auth_config", e.to_string()))?;
    Ok(())
}

/// 更新认证配置的名称和认证数据（加密敏感字段后存储）
pub fn update_auth_config(conn: &Connection, ac: &AuthConfig) -> Result<(), CoreError> {
    let encrypted_data = encrypt_auth_data(&ac.auth_data)?;
    let rows = conn
        .execute(
            "UPDATE auth_configs SET name = ?1, auth_type = ?2, auth_data = ?3, updated_at = ?4 WHERE id = ?5",
            params![ac.name, ac.auth_type, encrypted_data, ac.updated_at, ac.id],
        )
        .map_err(|e| storage_err("update_auth_config", e.to_string()))?;

    if rows == 0 {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "auth_store".to_string(),
            operation: "update_auth_config".to_string(),
            reason: format!("auth_config not found: {}", ac.id),
        }));
    }
    Ok(())
}

// ===========================================================================
// ======================== 全局库专用函数（无 origin/source_id/snapshot_at）==
// ===========================================================================
// 全局 auth_configs 表不需要快照溯源字段（全局和项目物理隔离）
// 项目 auth_configs 表（有 origin 列）使用上面的通用函数

/// 全局库：创建认证配置（不含快照溯源字段，加密敏感字段）
pub fn create_global_auth_config(conn: &Connection, ac: &AuthConfig) -> Result<(), CoreError> {
    let _ = ensure_table(conn);
    let encrypted_data = encrypt_auth_data(&ac.auth_data)?;
    conn.execute(
        "INSERT INTO auth_configs (id, name, auth_type, auth_data, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            ac.id,
            ac.name,
            ac.auth_type,
            encrypted_data,
            ac.created_at,
            ac.updated_at
        ],
    )
    .map_err(|e| storage_err("create_global_auth_config", e.to_string()))?;
    Ok(())
}

/// 全局库：列出认证配置（不含快照溯源字段，返回解密后的 auth_data）
pub fn list_global_auth_configs(
    conn: &Connection,
    auth_type: Option<&str>,
) -> Result<Vec<AuthConfig>, CoreError> {
    let _ = ensure_table(conn);
    let (sql, param): (String, Option<String>) = if let Some(t) = auth_type {
        (
            "SELECT id, name, auth_type, auth_data, created_at, updated_at
             FROM auth_configs WHERE auth_type = ?1 ORDER BY name"
                .to_string(),
            Some(t.to_string()),
        )
    } else {
        (
            "SELECT id, name, auth_type, auth_data, created_at, updated_at
             FROM auth_configs ORDER BY auth_type, name"
                .to_string(),
            None,
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_global_auth_configs", e.to_string()))?;

    let items = if let Some(ref p) = param {
        stmt.query_map(params![p], |row| {
            let raw_auth_data: String = row.get(3)?;
            let auth_data = decrypt_auth_data(&raw_auth_data).unwrap_or(raw_auth_data);
            Ok(AuthConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                auth_type: row.get(2)?,
                auth_data,
                origin: Some("global".to_string()),
                source_id: None,
                snapshot_at: None,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| storage_err("query_global_auth_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            let raw_auth_data: String = row.get(3)?;
            let auth_data = decrypt_auth_data(&raw_auth_data).unwrap_or(raw_auth_data);
            Ok(AuthConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                auth_type: row.get(2)?,
                auth_data,
                origin: Some("global".to_string()),
                source_id: None,
                snapshot_at: None,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| storage_err("query_global_auth_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(items)
}

/// 全局库：根据 ID 获取认证配置（不含快照溯源字段，返回解密后的 auth_data）
pub fn get_global_auth_config(
    conn: &Connection,
    id: &str,
) -> Result<Option<AuthConfig>, CoreError> {
    let _ = ensure_table(conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, auth_type, auth_data, created_at, updated_at
             FROM auth_configs WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_global_auth_config", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        let raw_auth_data: String = row.get(3)?;
        let auth_data = decrypt_auth_data(&raw_auth_data).unwrap_or(raw_auth_data);
        Ok(AuthConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            auth_type: row.get(2)?,
            auth_data,
            origin: Some("global".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_global_auth_config", e.to_string()))
}
