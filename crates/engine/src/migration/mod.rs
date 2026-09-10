pub mod executor;
pub mod global_init;
/**
 * 迁移系统模块
 *
 * 负责数据库结构的版本管理和迁移执行：
 * - 从嵌入的 SQL 文件加载迁移脚本
 * - 按版本号顺序执行未应用的迁移
 * - 追踪已应用的迁移版本
 * - 支持 SQLite 和 DuckDB 两种数据库类型
 */
pub mod manager;
pub mod schema;

pub use executor::MigrationExecutor;
pub use global_init::{
    get_global_data_dir, get_global_db_manager, get_global_db_path, get_global_duckdb_path,
    get_global_metadata_dir, get_system_dir, initialize_global_system, shutdown_global_system,
};
pub use manager::{MigrationManager, MigrationType};
pub use schema::{SchemaTracker, SchemaVersion};
