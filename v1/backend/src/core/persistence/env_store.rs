use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::error::{CoreError, StorageError};

/// 环境定义（生产环境、预发布环境、开发环境等）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub sort_order: i32,
    pub origin: Option<String>,
    pub source_id: Option<String>,
    pub snapshot_at: Option<String>,
    pub created_at: String,
}

/// 环境策略（只读模式、导航过滤、查询超时、行数限制、DDL/DML 阻断等）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EnvironmentPolicy {
    pub id: String,
    pub environment_id: String,
    pub policy_type: String,
    pub policy_config: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "env_store".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 确保 environments 表存在（回退修复：兼容迁移 010 可能因 ALTER TABLE 冲突而失败）
fn ensure_table(conn: &Connection) -> Result<(), CoreError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS environments (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            description TEXT,
            color       TEXT,
            sort_order  INTEGER DEFAULT 0,
            origin      TEXT,
            source_id   TEXT,
            snapshot_at TEXT,
            created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| storage_err("ensure_table", e.to_string()))?;

    // 补充：确保迁移可能遗漏的列（ALTER TABLE ADD COLUMN 不支持 IF NOT EXISTS，忽略重复错误）
    let extra_cols = ["origin TEXT", "source_id TEXT", "snapshot_at TEXT"];
    for col_def in &extra_cols {
        let _ = conn.execute(
            &format!("ALTER TABLE environments ADD COLUMN {}", col_def),
            [],
        );
    }
    Ok(())
}

/// 创建新环境
pub fn create_environment(conn: &Connection, env: &Environment) -> Result<(), CoreError> {
    // 确保表存在
    let _ = ensure_table(conn);
    conn.execute(
        "INSERT INTO environments (id, name, description, color, sort_order, origin, source_id, snapshot_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            env.id,
            env.name,
            env.description,
            env.color,
            env.sort_order,
            env.origin,
            env.source_id,
            env.snapshot_at,
            env.created_at
        ],
    )
    .map_err(|e| storage_err("create_environment", e.to_string()))?;
    Ok(())
}

/// 列出所有环境，按排序字段和名称排序
pub fn list_environments(conn: &Connection) -> Result<Vec<Environment>, CoreError> {
    // 尝试查询，如果表不存在则先创建
    let result = query_environments(conn);
    if result.is_err() {
        // 表可能不存在，尝试创建并重试
        ensure_table(conn)?;
        return query_environments(conn);
    }
    result
}

fn query_environments(conn: &Connection) -> Result<Vec<Environment>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, color, sort_order, origin, source_id, snapshot_at, created_at
             FROM environments ORDER BY sort_order, name",
        )
        .map_err(|e| storage_err("prepare_list_environments", e.to_string()))?;

    let items = stmt
        .query_map([], |row| {
            Ok(Environment {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                color: row.get(3)?,
                sort_order: row.get(4)?,
                origin: row.get(5)?,
                source_id: row.get(6)?,
                snapshot_at: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|e| storage_err("query_environments", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(items)
}

/// 更新环境信息，若环境不存在则返回错误
pub fn update_environment(conn: &Connection, env: &Environment) -> Result<(), CoreError> {
    let rows = conn
        .execute(
            "UPDATE environments SET name = ?1, description = ?2, color = ?3, sort_order = ?4 WHERE id = ?5",
            params![env.name, env.description, env.color, env.sort_order, env.id],
        )
        .map_err(|e| storage_err("update_environment", e.to_string()))?;

    if rows == 0 {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "env_store".to_string(),
            operation: "update_environment".to_string(),
            reason: format!("environment not found: {}", env.id),
        }));
    }
    Ok(())
}

/// 删除环境（关联的策略会级联删除）
pub fn delete_environment(conn: &Connection, id: &str) -> Result<(), CoreError> {
    conn.execute("DELETE FROM environments WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_environment", e.to_string()))?;
    Ok(())
}

/// 根据 ID 获取单个环境
pub fn get_environment(conn: &Connection, id: &str) -> Result<Option<Environment>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, description, color, sort_order, origin, source_id, snapshot_at, created_at
             FROM environments WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_environment", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(Environment {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            color: row.get(3)?,
            sort_order: row.get(4)?,
            origin: row.get(5)?,
            source_id: row.get(6)?,
            snapshot_at: row.get(7)?,
            created_at: row.get(8)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_environment", e.to_string()))
}

/// 为指定环境创建策略
pub fn create_policy(conn: &Connection, policy: &EnvironmentPolicy) -> Result<(), CoreError> {
    conn.execute(
        "INSERT INTO environment_policies (id, environment_id, policy_type, policy_config, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            policy.id,
            policy.environment_id,
            policy.policy_type,
            policy.policy_config,
            policy.enabled as i32,
            policy.created_at,
        ],
    )
    .map_err(|e| storage_err("create_policy", e.to_string()))?;
    Ok(())
}

/// 列出指定环境的所有策略
pub fn list_policies(
    conn: &Connection,
    environment_id: &str,
) -> Result<Vec<EnvironmentPolicy>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, environment_id, policy_type, policy_config, enabled, created_at
             FROM environment_policies WHERE environment_id = ?1 ORDER BY policy_type",
        )
        .map_err(|e| storage_err("prepare_list_policies", e.to_string()))?;

    let items = stmt
        .query_map(params![environment_id], |row| {
            Ok(EnvironmentPolicy {
                id: row.get(0)?,
                environment_id: row.get(1)?,
                policy_type: row.get(2)?,
                policy_config: row.get(3)?,
                enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| storage_err("query_policies", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(items)
}

/// 更新策略，若策略不存在则返回错误
pub fn update_policy(conn: &Connection, policy: &EnvironmentPolicy) -> Result<(), CoreError> {
    let rows = conn
        .execute(
            "UPDATE environment_policies SET policy_type = ?1, policy_config = ?2, enabled = ?3 WHERE id = ?4",
            params![policy.policy_type, policy.policy_config, policy.enabled as i32, policy.id],
        )
        .map_err(|e| storage_err("update_policy", e.to_string()))?;

    if rows == 0 {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "env_store".to_string(),
            operation: "update_policy".to_string(),
            reason: format!("policy not found: {}", policy.id),
        }));
    }
    Ok(())
}

/// 删除策略
pub fn delete_policy(conn: &Connection, id: &str) -> Result<(), CoreError> {
    conn.execute(
        "DELETE FROM environment_policies WHERE id = ?1",
        params![id],
    )
    .map_err(|e| storage_err("delete_policy", e.to_string()))?;
    Ok(())
}
