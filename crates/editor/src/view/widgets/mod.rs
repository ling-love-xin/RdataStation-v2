//! 视图内部件（状态栏 / 结果区 / 结果集标签条）
//!
//! 查找 / 替换**不在这里**：那是编辑器内核的能力（`Ctrl+F` → 内核 `Search`，
//! 由组件库的查找面板渲染），应用层不重复造一个。
//!
//! 结果区按原型 §2.4 拆三块：**结果集标签条**（⑤ · `result_sets`）、
//! **工具栏与网格**（⑥ · `result_grid`）、**错误卡片**（`error_card`）；
//! 结果状态行（⑦）是工具栏那行下面的第二行，同样在 `result_grid` 里。

pub mod error_card;
pub mod result_grid;
pub mod result_sets;
pub mod status_bar;
