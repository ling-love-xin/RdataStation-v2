pub mod duckdb;
/**
 * 迁移系统模块
 *
 * 负责数据库结构的版本管理和迁移执行：
 * - 从嵌入的 SQL 文件加载迁移脚本
 * - 按版本号顺序执行未应用的迁移
 * - 追踪已应用的迁移版本
 * - 支持 SQLite 和 DuckDB 两种数据库类型
 *
 * 注：启动编排 `global_init`（依赖驱动声明同步）保留在 engine，
 * 全局数据库管理器注册表在 `super::persistence::global_registry`。
 */
pub mod executor;
pub mod manager;
pub mod schema;

pub use executor::MigrationExecutor;
pub use manager::{MigrationManager, MigrationType};
pub use schema::{SchemaTracker, SchemaVersion};
