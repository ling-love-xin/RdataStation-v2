//! SQL 语言服务（重导出自 rds-sql）
//!
//! 2026-10-29 起 SQL 处理能力独立为 `crates/sql`（rds-sql，见
//! `docs/architecture/layout/crate-slimming-engine.md` 第一刀），本模块仅作
//! 命名空间保真薄壳：外部 `engine::sql::*` 路径零改动。
//!
//! 注意：`engine::sql::script` 随 `rds_sql::*` 一并可见（脚本词法器模块），
//! 与 `engine::script`（同一模块的另一薄壳）指向相同代码。

pub use rds_sql::*;
