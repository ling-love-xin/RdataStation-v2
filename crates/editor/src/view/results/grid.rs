//! 结果网格（delegate + 渲染）——2026-10-23 下移 `rds-result-panel`
//!
//! 重导出层：`ResultGridDelegate` / `ResultToolbar` / `ResultStatus` / `ResultControls` /
//! `ResultPane` / `result_grid::render` 随结果面板独立成 crate，editor 内部
//! `crate::view::results::grid::…` 路径保持不变。全部实现见
//! `crates/result_panel/src/grid.rs`。

pub use result_panel::grid::*;
