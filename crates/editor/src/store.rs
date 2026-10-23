//! 结果存储（**结果集的唯一权威**）——2026-10-23 下移 `rds-result-panel`
//!
//! 本文件保留为**重导出层**：结果域（`ResultStore` / `ResultEntry` / `ColumnKind` /
//! `ColumnType` / `ResultPlacement` / `Pushdown`）随结果面板独立成 crate，editor 内部
//! 的 `crate::store::…` 路径保持不变（依赖方向：editor → result_panel，不反向）。
//! 全部实现见 `crates/result_panel/src/store.rs`。

pub use result_panel::store::*;
