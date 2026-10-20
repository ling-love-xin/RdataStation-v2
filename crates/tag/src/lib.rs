//! rds-tag — 产品级统一标签能力（M4 导航连接 / M6 分析资源 / M5 草稿箱 共用）。
//!
//! 设计：`docs/architecture/tag/tag-system-spec.md`（正式规格，2026-10-20 定稿）。
//! 骨架范围（执行路径第 2 步）：统一模型 + 定义 CRUD + 软删口径 + 命名工具；
//! 三模块的关联实现、UI 接入与存量迁移在后续步骤（分析资源 → 草稿箱 → 导航）。

pub mod model;
pub mod store;

pub use model::{Tag, SCOPE_GLOBAL, SCOPE_PROJECT, normalize_name, parse_key_value};
pub use store::{TagDefinitions, TagStore, TagTable};
