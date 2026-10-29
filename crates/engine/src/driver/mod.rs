//! 数据库连接抽象层（rds-driver 抽取后的命名空间保真薄壳）
//!
//! 实现已迁至 `crates/driver`（rds-driver），本模块仅做重导出，
//! `engine::driver::*` 路径对外零改动（12 个依赖方与内部 services/duckdb/cache 均不受影响）。

pub use rds_driver::*;
