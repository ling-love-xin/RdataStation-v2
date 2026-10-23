//! 结果区的脚本输出卡片（2b 通用模式）——2026-10-23 下移 `rds-result-panel`
//!
//! 重导出层：`ScriptCard` / `ScriptOutcomeView` / `render` 随结果面板独立成 crate，
//! editor 内部 `crate::view::results::script_card::…` 路径保持不变。全部实现见
//! `crates/result_panel/src/script_card.rs`。

pub use result_panel::script_card::*;
