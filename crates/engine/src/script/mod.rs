//! 脚本语言词法高亮（重导出自 rds-sql::script）
//!
//! 2026-10-29 随 SQL 语言服务一并迁至 `crates/sql`（见
//! `docs/architecture/layout/crate-slimming-engine.md` 第一刀），本模块仅作
//! 命名空间保真薄壳：外部 `engine::script::*` 路径零改动。

pub use rds_sql::script::*;
