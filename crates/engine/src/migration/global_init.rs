/**
 * 全局系统初始化模块
 *
 * 负责应用启动时的全局初始化：
 * - 创建全局数据目录
 * - 初始化全局系统数据库（SQLite 连接池 + DuckDB 长连接）
 * - 执行全局系统库迁移
 * - 初始化全局配置
 */
use std::path::PathBuf;
use std::sync::OnceLock;

use shared::error::{CommonError, CoreError};
use crate::migration::{MigrationManager, MigrationType};
use crate::persistence::GlobalDatabaseManager;

/// 全局数据目录名称
const GLOBAL_DATA_DIR_NAME: &str = "RdataStation";

/// 系统目录名称
const SYSTEM_DIR_NAME: &str = "system";

/// 全局 SQLite 数据库文件名
const GLOBAL_SQLITE_NAME: &str = "global.db";

/// 全局 DuckDB 数据库文件名
const GLOBAL_DUCKDB_NAME: &str = "analytics.duckdb";

/// 全局系统数据库管理器实例
///
/// 使用 OnceLock 确保只初始化一次
/// 应用启动时创建，应用关闭时销毁
static GLOBAL_DB_MANAGER: OnceLock<GlobalDatabaseManager> = OnceLock::new();

/// 获取全局数据目录路径
pub fn get_global_data_dir() -> Result<PathBuf, CoreError> {
    let app_dir = dirs::data_dir()
        .ok_or_else(|| {
            CoreError::common(CommonError::General(
                "Failed to get system data directory".to_string(),
            ))
        })?
        .join(GLOBAL_DATA_DIR_NAME);

    std::fs::create_dir_all(&app_dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create global data directory: {}",
            e
        )))
    })?;

    Ok(app_dir)
}

/// 获取全局系统目录路径
///
/// 系统目录包含全局 SQLite 和 DuckDB 数据库
pub fn get_system_dir() -> Result<PathBuf, CoreError> {
    let data_dir = get_global_data_dir()?;
    let system_dir = data_dir.join(SYSTEM_DIR_NAME);

    std::fs::create_dir_all(&system_dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create system directory: {}",
            e
        )))
    })?;

    Ok(system_dir)
}

/// 获取全局 SQLite 数据库路径
///
/// 新路径：{data_dir}/RdataStation/system/global.db
pub fn get_global_db_path() -> Result<PathBuf, CoreError> {
    let system_dir = get_system_dir()?;
    Ok(system_dir.join(GLOBAL_SQLITE_NAME))
}

/// 获取全局 DuckDB 数据库路径
///
/// 新路径：{data_dir}/RdataStation/system/analytics.duckdb
pub fn get_global_duckdb_path() -> Result<PathBuf, CoreError> {
    let system_dir = get_system_dir()?;
    Ok(system_dir.join(GLOBAL_DUCKDB_NAME))
}

/// 获取全局元数据目录路径
///
/// 路径：{data_dir}/RdataStation/metadata/global/
pub fn get_global_metadata_dir() -> Result<PathBuf, CoreError> {
    let data_dir = get_global_data_dir()?;
    let metadata_dir = data_dir.join("metadata/global");

    std::fs::create_dir_all(&metadata_dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create global metadata directory: {}",
            e
        )))
    })?;

    Ok(metadata_dir)
}

/// 初始化全局系统数据库管理器
///
/// 必须在应用启动时调用，且只能调用一次。
/// 创建 SQLite 连接池和 DuckDB 长连接。
pub async fn initialize_global_system() -> Result<(), CoreError> {
    // 内置驱动注册（DriverRegistry）：测试连接 / 连接路由都从这里取工厂。
    // 必须在任何连接尝试前完成——曾漏掉这一步，导致真机「测试连接」报
    // `CONN_DRIVER_NOT_FOUND: Driver 'sqlite' not found in registry`（注册表为空）。
    // `register_by_factory` 是幂等写入（HashMap insert），重复调用安全。
    crate::driver::AutoDriverRegistrar::auto_register();

    let sqlite_path = get_global_db_path()?;
    let duckdb_path = get_global_duckdb_path()?;

    // 执行 SQLite 迁移
    let migration_manager = MigrationManager::new();
    let applied = migration_manager.migrate(&sqlite_path, MigrationType::Global)?;

    if !applied.is_empty() {
        tracing::info!(
            "Global system SQLite initialized with {} migrations",
            applied.len()
        );
    } else {
        tracing::debug!("Global system SQLite is up to date");
    }

    // 创建全局数据库管理器（连接池）
    let manager = GlobalDatabaseManager::new(
        sqlite_path,
        duckdb_path,
        10, // SQLite 连接池大小（增加到 10 以支持更高并发）
    )
    .await?;

    // 存量网络档案的凭据加密迁移（一次性、幂等）：升级前写入的明文 `config` 在这里转密文。
    // 覆盖全局库 + 已登记项目库（名册）；失败仅告警——迁移不应阻断启动
    // （读路径对明文仍兼容，下次编辑保存也会自动加密）。
    let mut migrated = 0usize;
    match manager.sqlite_pool().acquire().await {
        Ok(sqlite) => match sqlite.inner() {
            Ok(conn) => {
                match crate::persistence::network_store::reencrypt_all_network_configs(conn) {
                    Ok(n) => migrated += n,
                    Err(e) => tracing::warn!(
                        error = %e,
                        "全局库网络档案凭据加密迁移失败（读路径仍兼容明文）"
                    ),
                }
            }
            Err(e) => tracing::warn!(error = %e, "获取全局库连接失败，跳过网络档案加密迁移"),
        },
        Err(e) => tracing::warn!(error = %e, "获取全局库连接失败，跳过网络档案加密迁移"),
    }
    // 项目库：名册里的每个项目根（不存在 / 无 network_configs 表自动跳过，读路径无副作用）
    match manager.get_all_projects().await {
        Ok(projects) => {
            for p in projects {
                if p.path.trim().is_empty() {
                    continue;
                }
                match crate::persistence::network_store::reencrypt_project_network_configs(
                    std::path::Path::new(&p.path),
                ) {
                    Ok(n) => migrated += n,
                    Err(e) => tracing::warn!(
                        project = %p.path,
                        error = %e,
                        "项目库网络档案加密迁移失败（读路径仍兼容明文）"
                    ),
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "读取项目名册失败，跳过项目库网络档案加密迁移"),
    }
    if migrated > 0 {
        tracing::info!(count = migrated, "网络档案明文凭据已加密（一次性迁移）");
    }

    // 存储到全局实例
    install_global_db_manager(manager)?;

    tracing::info!("Global database manager initialized successfully");

    Ok(())
}

/// 注入全局库管理器（测试 / 嵌入场景；只能设置一次）。
///
/// 与 `initialize_global_system` 互斥：后到者返回错误。
/// 集成测试可用临时目录构造管理器后注入，从而覆盖
/// `DataSourceService::global()` 等生产单例路径。
pub fn install_global_db_manager(manager: GlobalDatabaseManager) -> Result<(), CoreError> {
    GLOBAL_DB_MANAGER.set(manager).map_err(|_| {
        CoreError::common(CommonError::General(
            "Global database manager already initialized".to_string(),
        ))
    })
}

/// 获取全局系统数据库管理器实例
///
/// 如果尚未初始化，将返回 None
pub fn get_global_db_manager() -> Option<&'static GlobalDatabaseManager> {
    GLOBAL_DB_MANAGER.get()
}

/// 关闭全局系统数据库管理器
///
/// 在应用退出时调用
pub async fn shutdown_global_system() -> Result<(), CoreError> {
    if let Some(manager) = GLOBAL_DB_MANAGER.get() {
        manager.close().await?;
        tracing::info!("Global database manager shut down successfully");
    }
    Ok(())
}
