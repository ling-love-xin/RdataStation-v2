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

use shared::error::{CommonError, CoreError};
use crate::migration::executor::{Migration, MigrationExecutor};
use crate::migration::schema::SchemaTracker;

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
    ///
    /// `ProjectAnalysis` 会被拒绝——它的载体是 DuckDB 文件，而本管理器基于 rusqlite：
    /// 用它建 `analytics.duckdb` 会写出 **SQLite 格式**的文件。DuckDB 内置 SQLite 存储
    /// 后端仍能打开这种文件（`duckdb_databases().type = 'sqlite'`），所以既不会报错、
    /// 也无法从「open 成功」的断言里发现，只会让存储格式与迁移账本悄悄分裂。
    /// DuckDB 侧请走 `crate::migration::duckdb::{migrate_at_path, apply_migrations}`。
    pub fn migrate(
        &self,
        db_path: &Path,
        migration_type: MigrationType,
    ) -> Result<Vec<Migration>, CoreError> {
        if migration_type == MigrationType::ProjectAnalysis {
            return Err(CoreError::common(CommonError::General(
                "MigrationManager 不支持 ProjectAnalysis（目标为 DuckDB 文件）：请改用 \
                 engine::migration::duckdb::migrate_at_path / apply_migrations"
                    .to_string(),
            )));
        }

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
    ) -> Result<Vec<crate::migration::SchemaVersion>, CoreError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_mgr_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("db")
    }

    /// 防线：rusqlite 迁移器必须拒绝 DuckDB 类型，且**不得**留下半成品文件。
    #[test]
    fn rejects_project_analysis() {
        let db_path = temp_db_path("guard");

        let err = MigrationManager::new()
            .migrate(&db_path, MigrationType::ProjectAnalysis)
            .expect_err("ProjectAnalysis 必须被拒绝");
        assert!(
            err.to_string().contains("duckdb"),
            "错误信息应指向 DuckDB 迁移器：{err}"
        );
        assert!(!db_path.exists(), "被拒绝时不应创建任何文件");
    }

    /// 对照组：SQLite 类型照常执行（确认防线没有误伤）。
    #[test]
    fn allows_sqlite_types() {
        let db_path = temp_db_path("ok");

        let applied = MigrationManager::new()
            .migrate(&db_path, MigrationType::ProjectMeta)
            .expect("ProjectMeta 应正常执行");
        assert!(!applied.is_empty(), "首次应至少应用 1 条迁移");
        assert!(db_path.exists(), "project.db 应被创建");
    }
}
