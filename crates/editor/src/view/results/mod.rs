//! 结果集域（结果区与它的衍生功能）
//!
//! 结果集是编辑器文档状态的一部分（`crate::store::ResultStore` 按 `DocumentId` 存），
//! 所以它的视图也留在本 crate；但**刻意不摊在 `widgets/` 里**：结果区是会长的一块
//! （导出 / 值预览 / 值编辑 / 洞察入口 / 过滤 / 排序 / 筛选 / 冻结列……），各自有状态与测试，
//! 摊平放会很快变成第二个 `host.rs`。
//!
//! ## 竖向结构（原型 §2.4）与文件对应
//!
//! ```text
//! ⑤ 结果集标签条   → sets.rs
//! ⑥ 结果工具栏     → grid.rs（`ResultToolbar` / `ResultControls`）
//!    网格          → grid.rs（`ResultGridDelegate` + DataTable）
//! ⑦ 结果状态行     → grid.rs（`ResultStatus` / `status_segments`）
//!    错误呈现      → error_card.rs
//! ```
//!
//! ## 能用组件库的，不自己写（0.6.1 已核实的原生能力）
//!
//! | 需求 | 原生 API |
//! | --- | --- |
//! | 排序 | `Column.sort = Some(ColumnSort)` + `TableState::sortable` + delegate `perform_sort` |
//! | 冻结列 | `Column.fixed = Some(ColumnFixed…)`（横向滚动钉左） |
//! | 列宽 / 列序 | `Column.resizable` / `Column.movable` + `TableState::col_resizable` / `col_movable` |
//! | 选择 | `row_selectable` / `col_selectable` / `cell_selectable` |
//! | 滚动到底加载更多 | delegate `has_more` / `load_more` / `load_more_threshold`（自动触发） |
//! | 单元格 / 行 / 表头 | delegate `render_td` / `render_tr` / `render_th` / `render_group_th` |
//! | 右键菜单 | delegate `context_menu` |
//! | 取值导出口 | `TableState::dump` / `dump_range` / delegate `cell_text` |
//!
//! 没有原生、要业务侧实现的只有三件：**筛选**（没有 filter API，在数据层或 `rows_count` /
//! `render_td` 里过滤）、**值预览 / 值编辑**（弹层与写回库都是业务语义）、**洞察入口**（业务动作）。
//! 它们各有对应的计划步骤（B15 / B14），落地时加**本目录下的新文件**，不要往 `grid.rs` 里堆。

pub mod error_card;
pub mod grid;
pub mod sets;
