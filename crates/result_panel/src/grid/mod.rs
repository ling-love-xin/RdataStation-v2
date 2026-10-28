//! 结果区（原型 §2.4 的 ⑤⑥⑦ + 网格）
//!
//! 自上而下：**结果集标签条**（由宿主给）→ **工具栏**（[`ResultToolbar`]：`行数 N` │
//! `耗时 1.2s` │ `连接名` + 动作）→ **网格 或 错误卡片** → **结果状态行**（[`ResultStatus`]：
//! `共 N 行` │ `已选第 M 行` + 截断提示）。
//!
//! 网格本身用组件库的 `DataTable` + `TableState`（虚拟滚动、列宽拖拽、行选择都是现成的，
//! 不手搓）。首列是固定的 `#` 行号槽（原型 §2.4），`NULL` 用 `muted_foreground` 斜体呈现
//! ——它是 SQL 里的一个真实值，与字符串 `"NULL"` 区分开。
//!
//! 单元格与表头都**截断**（结果里的长文本 / 长 JSON / 长列名比列宽宽是常态），截掉的部分
//! 靠**悬停全文**看（自己画 `render_th` / `render_td` 就是为了挂它）。右键菜单除了按值筛选
//! 与冻结，还给三种复制：此值 · 此列 · 整行（TSV，走 [`::shared::string::tsv_cell`] 同一份转义）。
//!
//! 原型 §2.4 的工具栏还包含**筛选 · 下发开关 · 分析 · 导出**——它们各自属 B15 / B14 / B7，
//! **没实现就不摆按钮**；分页/取下一段属 B5b，位置留在 ⑦ 那一行。
//!
//! 数据源是 [`crate::store::ResultStore`]（结果唯一权威）。网格**不持有真值**——
//! delegate 里的行是从权威那里拷来的投影，`set_data` 是唯一的写入点。
//!
//! ## 类型感知渲染
//!
//! 列类型（[`crate::store::ColumnKind`]）只影响**怎么画**：数字右对齐、布尔 / 时间族用等宽字体
//! （主题的 `mono_font_family`，不引新依赖）、表头在列名右侧挂一个类型小标签（次级色）。
//! **类型缺失时一个像素都不变**：对齐、字体、表头都回到今天的样子——驱动填上列类型之前，
//! 所有真实结果走的都是这一档。
//!
//! ## 按需取值
//!
//! 网格不预计算任何展示文本：组件的虚拟滚动只问可见窗口，delegate 就只回答那一格的取值。
//! 于是三条本来会按**整表**算的事都推到了真要的时候：
//!
//! - 视图行序：没有筛选也没有排序时**不物化**那份映射（`view_rows = None` 即恒等）；
//! - 筛选匹配：两边都是 ASCII 时零分配（回落 `to_lowercase` 只是为了保住口径）；
//! - 类型解析：入库时**每列一次**，不是每格一次。
//!
//! 需要**整表真值**的出口仍然是整表，但它们都是用户动作触发的（复制整列 / 整行 · 导出 ·
//! 筛选 · 排序），不是入库时预支的。
//!
//! ## 目录拆分（2026-10-29）
//!
//! 原单文件 `grid.rs`（6556 行）按域拆为子目录，**全部是移动 + 可见性调整，零逻辑改动**：
//!
//! | 文件 | 域 |
//! | --- | --- |
//! | [`delegate`] | `ResultGridDelegate` 结构体 + 数据同步 + 钩子注入 |
//! | [`view_rows`] | 隐藏列 / 列换算 / 转置 / 冻结 / 筛选 / 排序 / 视图行序 / 类型标签 |
//! | [`edit`] | 编辑态生命周期 + 提交值校验 |
//! | [`text`] | 取值 / 复制 / 查找 / 选择文本 / 悬停全文 / 菜单预览 |
//! | [`menu`] | 三组右键菜单（纯函数）+ 列去重 + `context_menu` |
//! | [`stats`] | 列统计 / 分组计数 / 统计文本 |
//! | [`cell`] | 单元格渲染（样式 / 裁剪 / JSON / BLOB + `render_td`） |
//! | [`header`] | 表头渲染（截断 / 类型标签 / 排序箭头 + `render_th`） |
//! | [`table_impl`] | `TableDelegate` 其余方法（列数 / 排序 / 拖列 / 取下一段 / 空态） |
//! | [`status`] | 工具栏 / 状态行 / 提示文案 / 渲染家族 |
//! | [`tests`] | 全量单元测试（原 `mod tests` 整体迁入） |
//!
//! 同一结构体的多个 `impl` 块分布在 `delegate` / `view_rows` / `edit` / `text`（Rust 合法）；
//! 同一 trait（`TableDelegate`）**不可拆多个 impl 块**（E0119），故 `render_th` / `render_td` /
//! `context_menu` 的逻辑在 [`header`] / [`cell`] / [`menu`] 的顶层自由函数中，
//! [`table_impl`] 只做薄转发。字段与方法放宽到 `pub(crate)` 供同 crate 共享，
//! **对外 pub 面不变**（editor 侧 `pub use result_panel::grid::*` 通配重导出不受影响）。

pub(crate) mod cell;
pub(crate) mod delegate;
pub(crate) mod edit;
pub(crate) mod header;
pub(crate) mod menu;
pub(crate) mod stats;
pub(crate) mod status;
pub(crate) mod table_impl;
pub(crate) mod text;
pub(crate) mod view_rows;
#[cfg(test)]
mod tests;

pub use delegate::{
    CELL_DISPLAY_LIMIT, COLUMN_FILTER_VALUE_LIMIT, RESULT_GRID_CONTEXT, CellEdit,
    ColumnFilterHook, ColumnStatsHook, CommitEditHook, CommitLocalEditHook, FilterValueHook,
    InsightColumnHook, LoadMoreHook, PreviewValueHook, ResultGridDelegate, RowActionHook,
    SortDownHook,
};
pub use menu::{
    ContextAction, ContextMenuItem, ContextTarget, column_browser_menu_items,
    column_distinct_values, column_menu_items, context_menu_items,
};
pub use text::{GridSelection, ResultMatch};
pub use cell::format_cell_text;
pub use stats::{ColumnStats, column_stats, column_stats_from_values, group_by_values, stats_from_fact, stats_lines};
pub use edit::validate_edit_value;
pub use status::{
    MAX_SINGLE_SEGMENT_ROWS, WINDOW_FETCH_NOTICE,
    ResultControls, ResultPane, ResultStatus, ResultToolbar, bytes_text, duration_text,
    fetch_more_hint, new_table_state, render, render_status_row,
    render_toolbar, status_action_placeholders, status_segments, thousands, truncated_hint,
    truncation_notice, window_fetch_active,
};