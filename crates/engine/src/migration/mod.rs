//! 迁移系统模块（rds-store 抽取后的命名空间保真薄壳）
//!
//! 实现已迁至 `crates/store`（rds-store）：`pub use rds_store::migration::*` 保真迁移系统公共面；
//! `global_init`（全局系统启动编排，依赖驱动声明同步）留在本 crate。
//! 全局数据库管理器注册表下沉至 `rds_store::persistence`，此处重导出保持路径不变。

pub use rds_store::migration::*;

/// 全局系统初始化（依赖驱动声明同步，保留在 engine）
pub mod global_init;

// 全局数据库路径助手 / 管理器注册表：自 global_init 下沉 rds_store::persistence，
// 此处重导出保持 `engine::migration::get_*` 路径不变。
pub use rds_store::persistence::{
    get_global_data_dir, get_global_db_path, get_global_duckdb_path, get_global_metadata_dir,
    get_global_db_manager, get_secrets_dir, get_system_dir, install_global_db_manager,
};

pub use global_init::{initialize_global_system, shutdown_global_system};
