use crate::core::error::{CommonError, CoreError};
use crate::core::migration::schema::SchemaTracker;
/**
 * 迁移执行器模块
 *
 * 负责执行单个迁移脚本，事务包裹确保原子性
 */
use rusqlite::Connection;

/// 单个迁移脚本
#[derive(Debug, Clone)]
pub struct Migration {
    /// 版本号
    pub version: u32,
    /// 迁移名称（不含版本号前缀）
    pub name: String,
    /// SQL 内容
    pub sql: String,
}

/// 迁移执行器
pub struct MigrationExecutor;

impl MigrationExecutor {
    /// 执行单个迁移（事务包裹）
    pub fn execute(conn: &Connection, migration: &Migration) -> Result<(), CoreError> {
        let tx = conn.unchecked_transaction().map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to begin transaction: {}",
                e
            )))
        })?;

        // 执行 SQL
        tx.execute_batch(&migration.sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to execute migration {}: {}",
                migration.name, e
            )))
        })?;

        // 记录版本
        SchemaTracker::record_version(&tx, migration.version, &migration.name)?;

        // 提交事务
        tx.commit().map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to commit migration {}: {}",
                migration.name, e
            )))
        })?;

        tracing::info!(
            "Applied migration {} (version {})",
            migration.name,
            migration.version
        );
        Ok(())
    }

    /// 解析迁移文件名
    /// 格式：001_init.sql -> version=1, name="init"
    pub fn parse_filename(filename: &str) -> Option<(u32, String)> {
        let stem = filename.strip_suffix(".sql")?;
        let parts: Vec<&str> = stem.splitn(2, '_').collect();

        if parts.len() != 2 {
            return None;
        }

        let version = parts[0].parse::<u32>().ok()?;
        let name = parts[1].to_string();

        Some((version, name))
    }
}
