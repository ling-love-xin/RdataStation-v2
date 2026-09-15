//! 视图内部件（状态栏 / 结果网格）
//!
//! 查找 / 替换**不在这里**：那是编辑器内核的能力（`Ctrl+F` → 内核 `Search`，
//! 由组件库的查找面板渲染），应用层不重复造一个。

pub mod result_grid;
pub mod status_bar;
