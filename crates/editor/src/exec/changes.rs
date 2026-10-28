//! 结果集单元格变更集（T2a-3）——2026-10-23 下移 `rds-result-panel`
//!
//! 重导出层：`ChangeSet` / `CellChange` / `CellChangeStatus` 随结果面板独立成 crate，
//! editor 内部 `crate::changes::…` 路径保持不变。全部实现见
//! `crates/result_panel/src/changes.rs`。

pub use result_panel::changes::*;
