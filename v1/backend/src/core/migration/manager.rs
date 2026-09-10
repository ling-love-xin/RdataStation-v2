use include_dir::{include_dir, Dir};
use rusqlite::Connection;
/**
 * 迁移管理器模块
 *
 * 核心迁移调度器，负责：
 * - 加载嵌入的 SQL 迁移文件
 * - 对比当前版本，找出待执行的迁移
 * - 按顺序执行迁移
 * - 支持多种迁移类型（global/project_meta/project_analysis/connection_metadata）
 */
use std::path::Path;

use crate::core::error::{CommonError, CoreError};
use crate::core::migration::executor::{Migration, MigrationExecutor};
use crate::core::migration::schema::SchemaTracker;

/// 编译时嵌入 migrations 目录
pub const MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

/// 迁移类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MigrationType {
    /// 全局系统库
    Global,
    /// 项目级元数据库
    ProjectMeta,
    /// 项目级分析引擎（DuckDB）
    ProjectAnalysis,
    /// 连接级元数据库
    ConnectionMetadata,
}

impl MigrationType {
    /// 获取迁移目录名称
    pub fn dir_name(&self) -> &'static str {
        match self {
            MigrationType::Global => "global",
            MigrationType::ProjectMeta => "project_meta",
            MigrationType::ProjectAnalysis => "project_analysis",
            MigrationType::ConnectionMetadata => "connection_metadata",
        }
    }
}

/// 迁移管理器
pub struct MigrationManager;

impl MigrationManager {
    /// 创建新的迁移管理器
    pub fn new() -> Self {
        Self
    }

    /// 执行指定类型的迁移
    pub fn migrate(
        &self,
        db_path: &Path,
        migration_type: MigrationType,
    ) -> Result<Vec<Migration>, CoreError> {
        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to create directory {:?}: {}",
                    parent, e
                )))
            })?;
        }

        // 连接数据库
        let conn = Connection::open(db_path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to open database {:?}: {}",
                db_path, e
            )))
        })?;

        // 确保版本追踪表存在
        SchemaTracker::ensure_table(&conn)?;

        // 获取当前版本
        let current_version = SchemaTracker::get_current_version(&conn)?;

        // 加载待执行的迁移
        let migrations = self.load_pending_migrations(migration_type, current_version)?;

        // 按顺序执行迁移
        let mut applied = Vec::new();
        for migration in &migrations {
            MigrationExecutor::execute(&conn, migration)?;
            applied.push(migration.clone());
        }

        if !applied.is_empty() {
            tracing::info!(
                "Applied {} migrations for {:?} to {:?}",
                applied.len(),
                migration_type,
                db_path
            );
        }

        Ok(applied)
    }

    /// 加载待执行的迁移文件
    fn load_pending_migrations(
        &self,
        migration_type: MigrationType,
        current_version: u32,
    ) -> Result<Vec<Migration>, CoreError> {
        let dir = MIGRATIONS_DIR
            .get_dir(migration_type.dir_name())
            .ok_or_else(|| {
                CoreError::common(CommonError::General(format!(
                    "Migration directory '{}' not found",
                    migration_type.dir_name()
                )))
            })?;

        let mut migrations: Vec<Migration> = dir
            .files()
            .filter_map(|f| {
                let path = f.path();
                let filename = path.file_name()?.to_str()?;

                // 只处理 .sql 文件
                if !filename.ends_with(".sql") {
                    return None;
                }

                // 解析版本号
                let (version, name) = MigrationExecutor::parse_filename(filename)?;

                // 只加载未应用的版本
                if version <= current_version {
                    return None;
                }

                // 读取 SQL 内容
                let sql = f.contents_utf8()?;

                Some(Migration {
                    version,
                    name,
                    sql: sql.to_string(),
                })
            })
            .collect();

        // 按版本号排序
        migrations.sort_by_key(|m| m.version);

        Ok(migrations)
    }

    /// 验证迁移文件语法（不实际执行）
    pub fn validate(&self, migration_type: MigrationType) -> Result<Vec<Migration>, CoreError> {
        self.load_pending_migrations(migration_type, 0)
    }

    /// 获取指定类型的所有迁移（包括已应用的）
    pub fn get_all_migrations(
        &self,
        migration_type: MigrationType,
    ) -> Result<Vec<Migration>, CoreError> {
        let dir = MIGRATIONS_DIR
            .get_dir(migration_type.dir_name())
            .ok_or_else(|| {
                CoreError::common(CommonError::General(format!(
                    "Migration directory '{}' not found",
                    migration_type.dir_name()
                )))
            })?;

        let migrations: Vec<Migration> = dir
            .files()
            .filter_map(|f| {
                let filename = f.path().file_name()?.to_str()?;
                if !filename.ends_with(".sql") {
                    return None;
                }
                let (version, name) = MigrationExecutor::parse_filename(filename)?;
                let sql = f.contents_utf8()?.to_string();
                Some(Migration { version, name, sql })
            })
            .collect();

        Ok(migrations)
    }

    /// 获取数据库当前版本
    pub fn get_current_version(&self, db_path: &Path) -> Result<u32, CoreError> {
        let conn = Connection::open(db_path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to open database {:?}: {}",
                db_path, e
            )))
        })?;
        SchemaTracker::ensure_table(&conn)?;
        SchemaTracker::get_current_version(&conn)
    }

    /// 获取已应用的版本列表
    pub fn get_applied_versions(
        &self,
        db_path: &Path,
    ) -> Result<Vec<crate::core::migration::SchemaVersion>, CoreError> {
        let conn = Connection::open(db_path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to open database {:?}: {}",
                db_path, e
            )))
        })?;
        SchemaTracker::ensure_table(&conn)?;
        SchemaTracker::get_applied_versions(&conn)
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new()
    }
}
