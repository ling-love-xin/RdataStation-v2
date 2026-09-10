//! 快照服务模块
//!
//! 提供全局配置到项目本地快照的创建、同步检测和执行功能。
//!
//! ## 核心能力
//!
//! - `snapshot_to_project()` — 将全局实体复制到项目本地，ID 从 G_xxx 变为 GP_xxx
//! - `check_sync_status()` — 检测快照是否过期（比较 snapshot_at vs updated_at）
//! - `sync_snapshot()` — 将过期快照更新为全局最新版本
//!
//! ## 使用场景
//!
//! 1. 用户在项目连接中选择全局环境/网络配置时，自动创建 GP_ 快照
//! 2. UI 显示"全局已更新"提示，用户点击同步按钮触发 sync_snapshot()
//! 3. 项目迁移到另一台机器时，GP_ 快照随项目走，不依赖全局配置

use rusqlite::{params, Connection};

use crate::core::error::{CoreError, StorageError};
use crate::core::persistence::id_prefix;

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "snapshot_service".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 快照状态
#[derive(Debug, Clone)]
pub struct SnapshotStatus {
    /// 快照 ID（GP_xxx）
    pub snapshot_id: String,
    /// 源全局 ID（G_xxx）
    pub source_id: String,
    /// 快照创建时间
    pub snapshot_at: String,
    /// 全局源是否已被更新
    pub outdated: bool,
    /// 全局源最近更新时间
    pub global_updated_at: Option<String>,
}

/// 将全局环境快照到项目本地
///
/// 读取 global DB 中 G_env_xxx 的完整数据（含策略），
/// 以 GP_env_xxx ID 写入 project DB。
pub fn snapshot_environment(
    global_conn: &Connection,
    project_conn: &Connection,
    global_env_id: &str,
) -> Result<String, CoreError> {
    let snapshot_id = id_prefix::to_snapshot_id(global_env_id).ok_or_else(|| {
        storage_err(
            "snapshot_environment",
            format!("无效的全局环境 ID: {}", global_env_id),
        )
    })?;

    let now = chrono::Utc::now().to_rfc3339();

    let env: (String, Option<String>, Option<String>, i32) = global_conn
        .query_row(
            "SELECT name, description, color, sort_order FROM environments WHERE id = ?1",
            params![global_env_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|e| storage_err("snapshot_environment", format!("读取全局环境失败: {}", e)))?;

    project_conn
        .execute(
            "INSERT OR REPLACE INTO environments (id, name, description, color, sort_order, origin, source_id, snapshot_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'global_snapshot', ?6, ?7, ?7)",
            params![snapshot_id, env.0, env.1, env.2, env.3, global_env_id, now],
        )
        .map_err(|e| {
            storage_err(
                "snapshot_environment",
                format!("写入项目环境快照失败: {}", e),
            )
        })?;

    let mut stmt = global_conn
        .prepare(
            "SELECT policy_type, policy_config, enabled FROM environment_policies WHERE environment_id = ?1 ORDER BY policy_type",
        )
        .map_err(|e| storage_err("snapshot_environment_policies", e.to_string()))?;

    let policies: Vec<(String, Option<String>, bool)> = stmt
        .query_map(params![global_env_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, i32>(2)? != 0))
        })
        .map_err(|e| storage_err("snapshot_environment_read_policies", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    for (policy_type, policy_config, enabled) in &policies {
        let policy_id =
            id_prefix::gen_project_id("ep", &uuid::Uuid::new_v4().to_string().replace('-', "_"));
        project_conn
            .execute(
                "INSERT OR REPLACE INTO environment_policies (id, environment_id, policy_type, policy_config, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![policy_id, snapshot_id, policy_type, policy_config, *enabled as i32, now],
            )
            .map_err(|e| {
                storage_err(
                    "snapshot_environment_policy",
                    format!("写入项目环境策略快照失败: {}", e),
                )
            })?;
    }

    tracing::info!(
        target = "snapshot",
        global_id = %global_env_id,
        snapshot_id = %snapshot_id,
        policies = policies.len(),
        "环境快照已创建"
    );

    Ok(snapshot_id)
}

/// 将全局网络配置快照到项目本地
pub fn snapshot_network_config(
    global_conn: &Connection,
    project_conn: &Connection,
    global_net_id: &str,
) -> Result<String, CoreError> {
    let snapshot_id = id_prefix::to_snapshot_id(global_net_id).ok_or_else(|| {
        storage_err(
            "snapshot_network_config",
            format!("无效的全局网络配置 ID: {}", global_net_id),
        )
    })?;

    let now = chrono::Utc::now().to_rfc3339();

    let nc: (Option<String>, String, String, String, String) = global_conn
        .query_row(
            "SELECT name, network_type, config, created_at, updated_at FROM network_configs WHERE id = ?1",
            params![global_net_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|e| {
            storage_err(
                "snapshot_network_config",
                format!("读取全局网络配置失败: {}", e),
            )
        })?;

    project_conn
        .execute(
            "INSERT OR REPLACE INTO network_configs (id, name, network_type, config, origin, source_id, snapshot_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'global_snapshot', ?5, ?6, ?7, ?6)",
            params![snapshot_id, nc.0, nc.1, nc.2, global_net_id, now, nc.3],
        )
        .map_err(|e| {
            storage_err(
                "snapshot_network_config",
                format!("写入项目网络配置快照失败: {}", e),
            )
        })?;

    tracing::info!(
        target = "snapshot",
        global_id = %global_net_id,
        snapshot_id = %snapshot_id,
        "网络配置快照已创建"
    );

    Ok(snapshot_id)
}

/// 将全局认证配置快照到项目本地
pub fn snapshot_auth_config(
    global_conn: &Connection,
    project_conn: &Connection,
    global_auth_id: &str,
) -> Result<String, CoreError> {
    let snapshot_id = id_prefix::to_snapshot_id(global_auth_id).ok_or_else(|| {
        storage_err(
            "snapshot_auth_config",
            format!("无效的全局认证配置 ID: {}", global_auth_id),
        )
    })?;

    let now = chrono::Utc::now().to_rfc3339();

    let ac: (Option<String>, String, String, String, String) = global_conn
        .query_row(
            "SELECT name, auth_type, auth_data, created_at, updated_at FROM auth_configs WHERE id = ?1",
            params![global_auth_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|e| {
            storage_err(
                "snapshot_auth_config",
                format!("读取全局认证配置失败: {}", e),
            )
        })?;

    project_conn
        .execute(
            "INSERT OR REPLACE INTO auth_configs (id, name, auth_type, auth_data, origin, source_id, snapshot_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'global_snapshot', ?5, ?6, ?7, ?6)",
            params![snapshot_id, ac.0, ac.1, ac.2, global_auth_id, now, ac.3],
        )
        .map_err(|e| {
            storage_err(
                "snapshot_auth_config",
                format!("写入项目认证配置快照失败: {}", e),
            )
        })?;

    tracing::info!(
        target = "snapshot",
        global_id = %global_auth_id,
        snapshot_id = %snapshot_id,
        "认证配置快照已创建"
    );

    Ok(snapshot_id)
}

/// 检测快照是否过期
///
/// 比较 project DB 中的 snapshot_at 与 global DB 中的 updated_at。
/// 若全局源已被更新，返回 SnapshotStatus { outdated: true }。
pub fn check_sync_status(
    global_conn: &Connection,
    project_conn: &Connection,
    table: &str,
    snapshot_id: &str,
) -> Result<SnapshotStatus, CoreError> {
    let source_id = id_prefix::source_global_id(snapshot_id).ok_or_else(|| {
        storage_err(
            "check_sync_status",
            format!("无法从快照 ID 反查全局源 ID: {}", snapshot_id),
        )
    })?;

    let snapshot_at: String = project_conn
        .query_row(
            &format!(
                "SELECT snapshot_at FROM {} WHERE id = ?1 AND origin = 'global_snapshot'",
                table
            ),
            params![snapshot_id],
            |row| row.get(0),
        )
        .map_err(|e| storage_err("check_sync_status", format!("读取项目快照时间失败: {}", e)))?;

    let global_updated_at: Option<String> = global_conn
        .query_row(
            &format!("SELECT updated_at FROM {} WHERE id = ?1", table),
            params![source_id],
            |row| row.get(0),
        )
        .ok();

    let outdated = match &global_updated_at {
        Some(gu) => gu > &snapshot_at,
        None => false,
    };

    Ok(SnapshotStatus {
        snapshot_id: snapshot_id.to_string(),
        source_id,
        snapshot_at,
        outdated,
        global_updated_at,
    })
}

/// 同步快照：用全局最新数据更新项目快照
///
/// 根据表名自动路由到对应的 snapshot 函数。
pub fn sync_snapshot(
    global_conn: &Connection,
    project_conn: &Connection,
    table: &str,
    snapshot_id: &str,
) -> Result<(), CoreError> {
    let source_id = id_prefix::source_global_id(snapshot_id).ok_or_else(|| {
        storage_err(
            "sync_snapshot",
            format!("无法从快照 ID 反查全局源 ID: {}", snapshot_id),
        )
    })?;

    match table {
        "environments" => {
            snapshot_environment(global_conn, project_conn, &source_id)?;
        }
        "network_configs" => {
            snapshot_network_config(global_conn, project_conn, &source_id)?;
        }
        "auth_configs" => {
            snapshot_auth_config(global_conn, project_conn, &source_id)?;
        }
        _ => {
            return Err(storage_err(
                "sync_snapshot",
                format!("不支持的表类型: {}", table),
            ));
        }
    }

    tracing::info!(
        target = "snapshot",
        table = %table,
        snapshot_id = %snapshot_id,
        source_id = %source_id,
        "快照已同步"
    );

    Ok(())
}
