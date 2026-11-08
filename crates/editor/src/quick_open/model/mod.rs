//! Quick Open 的**纯逻辑**：查询解析（模式前缀）、行匹配与评分、命中区间、结果分组与上限。
//!
//! 不依赖 GPUI / `Shared`，输入输出都是纯数据，因此可直接单测
//! （P2-c 下沉后：`cargo test -p rds-editor --lib quick_open`）。
//! 渲染与副作用在 workbench（`quick_open/delegate.rs` 与 `view.rs`——宿主 render / 事件路径）；
//! 命令目录在 [`crate::quick_open::commands`]（本模块只消费它的 `command_rows()`）。
//!
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`（§5 模式 / §6 元数据两档 / §8 匹配排序）。

mod groups;
mod scoring;
mod sources;
mod types;

#[cfg(test)]
mod tests;

pub use groups::*;
pub use scoring::*;
pub use sources::*;
pub use types::*;

// `LIKE` 通配符转义与 FTS5 `MATCH` 词清洗**不在这里**：
// - `LIKE` 的转义已在写侧（`engine::persistence::MetadataCacheOps::like_escape`，带单测）；
// - FTS 的词清洗归 Phase 1 接线的 `search_fts`（现状是 `format!("{}*", q)` 裸拼，
//   会被用户输入里的 `"` / `*` / `NEAR` 破坏语法）——修在它自己的 crate，避免两处各写一套。
