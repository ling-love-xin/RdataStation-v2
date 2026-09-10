use crate::core::error::{CommonError, CoreError};
/**
 * 迁移版本追踪模块
 *
 * 负责记录和管理已应用的迁移版本
 */
use rusqlite::Connection;

/// 迁移版本记录
#[derive(Debug, Clone)]
pub struct SchemaVersion {
    /// 版本号
    pub version: u32,
    /// 迁移名称
    pub name: String,
    /// 应用时间戳
    pub applied_at: i64,
}

/// 版本追踪器
pub struct SchemaTracker;

impl SchemaTracker {
    /// 确保 schema_version 表存在
    pub fn ensure_table(conn: &Connection) -> Result<(), CoreError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version     INTEGER PRIMARY KEY,
                name        TEXT NOT NULL,
                applied_at  INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to create schema_version table: {}",
                e
            )))
        })?;
        Ok(())
    }

    /// 获取当前最高版本
    pub fn get_current_version(conn: &Connection) -> Result<u32, CoreError> {
        let result = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get::<_, u32>(0),
            )
            .map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to get current version: {}",
                    e
                )))
            })?;
        Ok(result)
    }

    /// 获取所有已应用的版本
    pub fn get_applied_versions(conn: &Connection) -> Result<Vec<SchemaVersion>, CoreError> {
        let mut stmt = conn
            .prepare("SELECT version, name, applied_at FROM schema_version ORDER BY version")
            .map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to prepare statement: {}",
                    e
                )))
            })?;

        let versions = stmt
            .query_map([], |row| {
                Ok(SchemaVersion {
                    version: row.get(0)?,
                    name: row.get(1)?,
                    applied_at: row.get(2)?,
                })
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to query versions: {}",
                    e
                )))
            })?;

        versions.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to collect versions: {}",
                e
            )))
        })
    }

    /// 记录已应用的版本
    pub fn record_version(conn: &Connection, version: u32, name: &str) -> Result<(), CoreError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![version, name, now],
        )
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to record version: {}",
                e
            )))
        })?;
        Ok(())
    }

    /// 检查指定版本是否已应用
    pub fn is_version_applied(conn: &Connection, version: u32) -> Result<bool, CoreError> {
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_version WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to check version: {}",
                    e
                )))
            })?;
        Ok(count > 0)
    }
}
