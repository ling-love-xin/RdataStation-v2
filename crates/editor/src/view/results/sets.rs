//! 结果集标签条（B2）——2026-10-23 下移 `rds-result-panel`
//!
//! 重导出层：`ResultSetTab` / `tabs` 随结果面板独立成 crate，editor 内部
//! `crate::view::results::sets::…` 路径保持不变。全部实现见
//! `crates/result_panel/src/sets.rs`。

pub use result_panel::sets::*;
