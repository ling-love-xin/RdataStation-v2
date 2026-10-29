//! 全局数据库路径助手
//!
//! 自 engine `migration::global_init` 下沉（2026-10-29，engine 瘦身第二刀）：
//! 全局系统库文件位置（数据根 / 系统目录 / Secrets / SQLite / DuckDB / 元数据目录）
//! 属于持久化域，store 各 store 与启动编排（engine global_init）两侧共用。
//! engine 经命名空间薄壳重导出，`engine::migration::get_*` 路径零改动。

use std::path::PathBuf;

use shared::error::{CommonError, CoreError};

/// 系统目录名称
const SYSTEM_DIR_NAME: &str = "system";

/// 全局 SQLite 数据库文件名
const GLOBAL_SQLITE_NAME: &str = "global.db";

/// 全局 DuckDB 数据库文件名
const GLOBAL_DUCKDB_NAME: &str = "analytics.duckdb";

/// Secret 存储目录名（`system/secrets`）
const SECRETS_DIR_NAME: &str = "secrets";

/// 获取全局数据目录路径
///
/// 位置由 `paths::data_dir()` 解析：`<RDS_HOME>/data`（默认 RDS_HOME = 可执行文件所在目录）。
pub fn get_global_data_dir() -> Result<PathBuf, CoreError> {
    let app_dir = paths::data_dir();
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

/// DuckDB Secret 存储目录（`<RDS_HOME>/data/system/secrets`，不存在则建）
///
/// **两处必须用同一个目录**：凭据注册（`workbench::services::secret_integration`）与
/// 会话读取（`engine::duckdb::manager::DuckDBManager::configure_connection` 的
/// `secret_directory`）。否则会出现“注册成功、挂载却说找不到凭据”，在真机上表现为
/// 网络源认证失败（`ATTACH` 里的口令是脱敏的 `******`，只能靠 Secret 补）。
pub fn get_secrets_dir() -> Result<PathBuf, CoreError> {
    let dir = get_system_dir()?.join(SECRETS_DIR_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "Failed to create secrets directory: {}",
            e
        )))
    })?;
    Ok(dir)
}

/// 获取全局 SQLite 数据库路径
///
/// 路径：`<RDS_HOME>/data/system/global.db`
pub fn get_global_db_path() -> Result<PathBuf, CoreError> {
    let system_dir = get_system_dir()?;
    Ok(system_dir.join(GLOBAL_SQLITE_NAME))
}

/// 获取全局 DuckDB 数据库路径
///
/// 路径：`<RDS_HOME>/data/system/analytics.duckdb`
pub fn get_global_duckdb_path() -> Result<PathBuf, CoreError> {
    let system_dir = get_system_dir()?;
    Ok(system_dir.join(GLOBAL_DUCKDB_NAME))
}

/// 获取全局元数据目录路径
///
/// 路径：`<RDS_HOME>/data/metadata/global/`
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
