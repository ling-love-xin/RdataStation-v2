//! Quick Open 的**纯逻辑层**（2026-10-29 自 workbench 下沉）。
//!
//! - [`model`]：查询解析（模式前缀）、行匹配与评分、命中区间、结果分组与上限——
//!   不依赖 GPUI / Shared，输入输出都是纯数据，可直接单测；
//! - [`commands`]：命令目录（稳定 id + 关键词 + 动作），同样是纯数据。
//!
//! 渲染与副作用留在 workbench（`quick_open/delegate.rs` 与 `view.rs`）；
//! 宿主动作端口 `QuickOpenHost` 也留在 workbench（执行一条结果要什么副作用只有宿主知道）。

pub mod commands;
pub mod model;
