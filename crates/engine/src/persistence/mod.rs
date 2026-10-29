//! 持久化存储模块（rds-store 抽取后的命名空间保真薄壳）
//!
//! 实现已迁至 `crates/store`（rds-store），本模块仅做重导出，
//! `engine::persistence::*` 路径对外零改动。

pub use rds_store::persistence::*;
