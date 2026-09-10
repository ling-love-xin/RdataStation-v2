//! 持久化存储模块
//!
//! 负责系统数据的持久化存储，包括：
//! - 最近使用的数据库连接信息
//! - SQL 执行历史记录
//! - 项目级数据（SQLite + DuckDB）
//!
//! ## SQL 安全性保障
//! 本模块所有 SQL 查询遵循以下安全模式：
//! - 系统内部查询（migration/schema）: 使用 PRAGMA / 参数化 DDL，无用户输入
//! - 运行时查询（log_store/history_store）: 参数通过 rusqlite `?N` 占位符绑定，禁止拼接
//! - 标识符拼接（global_db repair）：使用 `quote_identifier()` 安全包裹
//! - 全文搜索（metadata_cache）：FTS5 MATCH 模式通过参数绑定传递
//!
//! 所有 IO 错误都在此模块转换为 CoreError，确保上下文不丢失。

pub mod analytics_resource_store;
pub mod auth_store;
pub mod cache_version_migration;
pub mod connection_store;
pub mod driver_store;
pub mod env_store;
pub mod global_db;
pub mod history_store;
pub mod id_prefix;
pub mod insight_meta_store;
pub mod insight_store;
pub mod log_store;
pub mod metadata_cache;
pub mod metadata_cache_pool;
pub mod network_store;
pub mod plugin_store;
pub mod project_connection_store;
pub mod project_db;
pub mod project_store;
pub mod sql_template_store;
pub mod workbench_context_store;

pub use analytics_resource_store::{
    AnalyticsFolder, AnalyticsRecycleItem, AnalyticsResource, AnalyticsResourceStore, AnalyticsTag,
    CreateFolderRequest, CreateResourceRequest, CreateTagRequest, ListResourcesOutput,
    ResourceVersion,
};
pub use cache_version_migration::{CacheVersionManager, CURRENT_CACHE_VERSION};
pub use global_db::{
    GlobalDatabaseManager, GlobalDuckdbConnection, GlobalPooledConnection, GlobalSqlitePool,
};
pub use insight_meta_store::InsightMetaStore;
pub use insight_store::{
    InsightColumnStore, InsightSchemaReportStore, InsightStorage, InsightStorageStats,
    InsightTableReportStore, InsightVersionEntry,
};
pub use metadata_cache::{ConnectionType, MetadataCacheManager, MetadataCacheOps};
pub use metadata_cache_pool::{MetadataCachePool, PooledMetadataConnection};
pub use project_db::{ProjectDatabaseManager, ProjectDuckdbConnection, ProjectSqlitePool};
pub use sql_template_store::{SqlTemplate, SqlTemplateStore};
pub use workbench_context_store::{EditorContext, WorkbenchContextStore, WorkbenchLayout};

pub use log_store::LogStore;

use crate::core::error::{CoreError, StorageError};
use std::path::Path;

/// 将 IO 错误转换为 CoreError（显式转换，保留完整上下文）
///
/// # Arguments
///
/// * `err` - IO 错误
/// * `path` - 操作路径
/// * `operation` - 操作描述（如 "read", "write", "create"）
///
/// # Returns
///
/// 返回包含完整上下文的 CoreError
pub fn io_to_core_error(err: std::io::Error, path: &Path, operation: &str) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation.to_string(),
        err.to_string(),
    ))
}

/// 将序列化错误转换为 CoreError
pub fn serialize_to_core_error(format: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Serialization {
        format: format.to_string(),
        reason: reason.to_string(),
    })
}

/// 将反序列化错误转换为 CoreError
pub fn deserialize_to_core_error(format: &str, data: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Deserialization {
        format: format.to_string(),
        data: data.to_string(),
        reason: reason.to_string(),
    })
}

/// 将持久化错误转换为 CoreError
pub fn persistence_to_core_error(store: &str, operation: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: store.to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}
