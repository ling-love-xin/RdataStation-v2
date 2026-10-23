//! 结果区错误卡片——2026-10-23 下移 `rds-result-panel`
//!
//! 重导出层：`ErrorCard` / `render` 随结果面板独立成 crate，editor 内部
//! `crate::view::results::error_card::…` 路径保持不变。全部实现见
//! `crates/result_panel/src/error_card.rs`。

pub use result_panel::error_card::*;
