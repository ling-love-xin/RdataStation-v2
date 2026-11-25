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
//! | [`array_cell`] | 数组单元格显示拆分（收起 `▶ 首值 [+N-1]` / 展开 `▼ [N]`，刀2） |
//! | [`header`] | 表头渲染（截断 / 类型标签 / 排序箭头 + `render_th`） |
//! | [`fit`] | 列宽自适应（双击列头分隔线：按内容算宽 + `fit_column`） |
//! | [`table_impl`] | `TableDelegate` 其余方法（列数 / 排序 / 拖列 / 取下一段 / 空态） |
//! | [`status`] | 工具栏 / 状态行 / 提示文案 / 渲染家族 |
//! | [`tests`] | 全量单元测试（原 `mod tests` 整体迁入） |
//!
//! 同一结构体的多个 `impl` 块分布在 `delegate` / `view_rows` / `edit` / `text`（Rust 合法）；
//! 同一 trait（`TableDelegate`）**不可拆多个 impl 块**（E0119），故 `render_th` / `render_td` /
//! `context_menu` 的逻辑在 [`header`] / [`cell`] / [`menu`] 的顶层自由函数中，
//! [`table_impl`] 只做薄转发。字段与方法放宽到 `pub(crate)` 供同 crate 共享，
//! **对外 pub 面不变**（editor 侧 `pub use result_panel::grid::*` 通配重导出不受影响）。

pub(crate) mod array_cell;
pub(crate) mod cell;
pub(crate) mod delegate;
pub(crate) mod edit;
pub(crate) mod fit;
pub(crate) mod header;
pub(crate) mod menu;
pub(crate) mod selection;
pub(crate) mod stats;
pub(crate) mod status;
pub(crate) mod table_impl;
#[cfg(test)]
mod tests;
pub(crate) mod text;
pub(crate) mod view_rows;

// 【第 59 批】结果网格的键盘动作
//
// 键位在 `crates/app` 注册，context = [`RESULT_GRID_CONTEXT`]（只挂网格那层，
// 编辑区 / SQL 编辑器不受影响）。对应 DBeaver 的
// `org.jkiss.dbeaver.ui.context.resultset.focused`：
// `plugin.xml:1071/1072` 的 `SHIFT+ENTER`（外部值编辑器）/ `ENTER`（内嵌编辑）、
// `:1059` 的 `Ctrl+反引号`（切换表示）、`:1082` 的 `Ctrl+S`（提交变更）、
// `:1083` 的 `Ctrl+R`（撤销未提交变更）、`:1090` 的 `Ctrl+Shift+G`（跳转到列）、
// `:1088` / `:1089` 的 `Ctrl+Alt+C` / `Ctrl+Alt+R`（选列 / 选行）、
// `:1058` 的 `Ctrl+Alt+Shift+T`（焦点在筛选框与结果呈现之间双向切换；
// `:1096` / `:1097` 的 `Alt+Shift+H` / `Alt+Shift+T`（隐藏当前列 / 显示全部隐藏列）、
// `:1095` 的 `Alt+Shift+C`（复制列名）、
// `:1094` 的 `Alt+Shift+F`（筛选设置）。
// 后三条在上游都挂 `...ui.context.resultset`（整块面板）而不是 `...resultset.focused`，
// 所以与上面几条不同：键位挂 `editor-result-panel` 上下文。
//
// 【第 81 批】另有**一条不是上游键位**的：`ESC` 关结果集浮层。上游 `ESC` 是
// `cell.reset`（`plugin.xml:1081`），本仓没有那条语义（网格的 `ESC` 留给组件），
// 浮层则按模态惯例一律 ESC 收起。它**不在这里的 `actions!` 里、也不是 keymap 键位**：
// 焦点一旦落进浮层里的 `InputState`，`Window::context_stack()` 会退化成空（实测 `[]`），
// 带 context 的 binding 一条都匹配不到（连 `Ctrl+Shift+G` 都按不出来），所以入口放在
// app 级 keystroke interceptor（`ResultPanel::new` 的 `intercept_keystrokes`）；
// 关哪个浮层由面板按当前打开状态自己判（`ResultPanel::dismiss_overlay`）。
use gpui_kit::actions;

actions!(
    result_panel,
    [
        /// `ENTER`：进入当前选中格的行内编辑（`CMD_ROW_EDIT_INLINE` → `openValueEditor(true)`）
        EnterCellEdit,
        /// `Ctrl+反引号`：切到下一个呈现（`CMD_SWITCH_PRESENTATION`，`plugin.xml:1059`）
        CycleResultPresentation,
        /// `Ctrl+S`：提交当前结果集的待提交变更（`CMD_APPLY_CHANGES`，`plugin.xml:1082`）。
        ///
        /// 同一个 `Ctrl+S` 在 `editor` context 是保存文档、在 `database-nav` 是提交暂存
        /// （`crates/app/src/main.rs:407` 有先例）：焦点在结果网格时这一层更深、先命中。
        CommitResultChanges,
        /// 【第 69 批】`ALT+INSERT`：在选中行之后新增一条空行（`CMD_ROW_ADD`，`plugin.xml:1073`）
        InsertResultRow,
        /// 【第 69 批】`CTRL+ALT+INSERT`：复制选中行为一条新行（`CMD_ROW_COPY`，`:1075`）
        DuplicateResultRow,
        /// 【第 69 批】`ALT+DELETE`：删除选中行（`CMD_ROW_DELETE`，`:1079`）
        DeleteResultRow,
        /// 【第 69 批】`CTRL+D`：把上一行的值填进当前行（`CMD_ROW_COPY_FROM_ABOVE`，`:1077`）
        CopyRowValuesFromAbove,
        /// 【第 69 批】`CTRL+ALT+D`：把下一行的值填进当前行（`CMD_ROW_COPY_FROM_BELOW`，`:1078`）
        CopyRowValuesFromBelow,
        /// 【第 70 批】`CTRL+ALT+N`：取下一段（`CMD_FETCH_PAGE` → `readNextSegment()`，
        /// `plugin.xml:1069`）。与状态栏「获取数据」钮同一条路（`ResultRequest::FetchMore`）。
        FetchNextSegment,
        /// 【第 70 批】`CTRL+G`：行号跳转（`CMD_GO_TO_ROW` → `GotoLineDialog`，`:1091`）。
        ///
        /// DBeaver 那边是弹输入框；本仓状态栏本来就有一口常驻行号输入框
        /// （`row_count_input`，`InputEvent::Change` 即时跳），所以键位的职责是
        /// **把那口框准备好并聚焦**，不另造一个弹窗。
        FocusRowJump,
        /// 【第 72 批】`CTRL+SHIFT+=`：取全部行（`CMD_FETCH_ALL` → `readAllData()`，
        /// `plugin.xml:1070`）。
        ///
        /// 键名写的是物理键 `=`（shift 下打出 `+`，但 gpui 的 `Keystroke` 比的是
        /// 规范化后的键名 + 修饰位，`platform/keystroke.rs:112`）。
        /// 与状态栏「获取全部数据」钮同一条入口：只送 `ResultRequest::AskFetchAll`
        /// （先确认），判定与真执行都在宿主侧。
        FetchAllData,
        /// 【第 74 批】`CTRL+R`：撤销全部未提交变更（`CMD_REJECT_CHANGES`
        /// → `rejectChanges()`，`plugin.xml:1083`）。
        ///
        /// 与工具栏那颗「回滚」钮同一条路（`ResultRequest::RollbackPending`）：
        /// 钮只在有待提交时摆（`panel/render.rs` 的 `pending_changes > 0`），
        /// 无待提交时键位留一句提示（第 69 批口径，DBeaver 那边是静默 return）。
        RejectPendingChanges,
        /// 【第 75 批】`CTRL+SHIFT+G`：跳转到列（`CMD_GO_TO_COLUMN` →
        /// `askForColumnToGo`，`plugin.xml:1090`）。
        ///
        /// DBeaver 弹 `InputDialog`（名字 / 1 基序号二选一）；本仓做成面板内浮层
        /// （`goto_column_open`）——只送 `ResultRequest::AskGotoColumn`，
        /// 开不开由宿主判（没有列可跳时留提示）。
        GotoColumn,
        /// 【第 76 批】`CTRL+ALT+C`：把当前格所在的**整列**选中
        /// （`CMD_SELECT_COLUMNS` → `SpreadsheetCommandHandler:106-118`）。
        SelectCurrentColumn,
        /// 【第 76 批】`CTRL+ALT+R`：把当前格所在的**整行**选中
        /// （`CMD_SELECT_ROWS` → `SpreadsheetCommandHandler:119-131`）。
        SelectCurrentRow,
        /// 【第 77 批】`CTRL+ALT+SHIFT+T`：焦点在「筛选框」与「结果呈现」之间来回切
        /// （`CMD_FOCUS_FILTER` → `ResultSetViewer.switchFilterFocus()`，`plugin.xml:1058`）。
        ///
        /// 这是**面板级**命令：键位挂 [`RESULT_PANEL_CONTEXT`](crate::panel::RESULT_PANEL_CONTEXT)
        /// （上游同一条也绑 `...context.resultset`，不是 `...resultset.focused`），
        /// 否则焦点已经在筛选框里时按不到「切回来」那一半。
        SwitchFilterFocus,
        /// 【第 78 批】`ALT+SHIFT+H`：隐藏当前格所在的列
        /// （`CMD_HIDE_COLUMNS` → `SpreadsheetCommandHandler:174-194`，`plugin.xml:1096`）。
        ///
        /// 面板级命令（上游同样挂 `...context.resultset`）：焦点在筛选框 / 侧栏里也能按到，
        /// 判据只看「当前格在哪一列」。
        HideCurrentColumn,
        /// 【第 78 批】`ALT+SHIFT+T`：把所有隐藏列显示回来
        /// （`CMD_SHOW_COLUMNS` → `SpreadsheetCommandHandler:195-212`，`plugin.xml:1097`）。
        ShowHiddenColumns,
        /// 【第 79 批】`ALT+SHIFT+C`：复制列名（`CMD_COPY_COLUMN_NAMES`，`plugin.xml:1095`；
        /// `ResultSetHandlerMain:333-375`）。
        ///
        /// 面板级命令（上游 contextId 同样是整块 `...ui.context.resultset`）。
        CopyColumnNames,
        /// 【第 80 批】`ALT+SHIFT+F`：打开筛选设置（`CMD_FILTER_EDIT_SETTINGS`，
        /// `plugin.xml:1094`；handler `:570` → `ResultSetHandlerMain:546-549` →
        /// `ResultSetViewer.showFilterSettingsDialog()`，`:644-646` = 直接开对话框、无前置判断）。
        ///
        /// 面板级命令（上游 contextId 同样是整块 `...ui.context.resultset`）。浮层本身
        /// 与它那三个鼠标入口早已就位（工具栏菜单「编辑筛选设置」/ 列头右键 / 筛选条
        /// 自定义钮），这条只是键盘入口。
        OpenFilterSettings,
        /// 【第 82 批】`F7` / `CTRL+7`：右侧辅助栏整组收起 / 展开
        /// （`CMD_TOGGLE_PANELS`，`plugin.xml:1084/1085`；handler `:167-169` →
        /// `ResultSetViewer.showPanels(!isPanelsVisible(), true, true)`，`:1478-1518`）。
        ///
        /// 面板级命令（上游 contextId 同样是整块 `...ui.context.resultset`，不是
        /// `...resultset.focused`）：焦点在网格或筛选框里都按得到。本仓等价鼠标入口是
        /// 右缘竖条顶部那颗 `panelsButton`（`editor-result-edge-panels` →
        /// `ResultRequest::SetPanelsVisible(!panels_visible)`），这条键位落同一份状态。
        TogglePanels,
        /// 【第 83 批】`CTRL+ALT+F2`：显示 / 隐藏「值详情」页
        /// （`CMD_TOGGLE_PANEL` + `panelId=value-view`，`plugin.xml:1099-1101`；
        /// handler `ResultSetHandlerTogglePanel:64-81`：可见则 `closePanel(id)`，
        /// 否则 `activatePanel(id, true, true)`）。
        ///
        /// 面板级命令（上游 contextId 是整块 `...ui.context.resultset`）。面板侧
        /// `toggle_right_tab` 早已实现（第 24 批，竖条 / 视图菜单共用），这条只是键盘入口，
        /// 走 `ResultRequest::ToggleRightTab`，连页集合落盘与引用页刷新一并复用。
        ToggleValuePanel,
        /// 【第 83 批】`CTRL+ALT+F3`：显示 / 隐藏「引用」页（`panelId=results-references`，
        /// `plugin.xml:1102-1104`）
        ToggleReferencePanel,
        /// 【第 83 批】`CTRL+ALT+F4`：显示 / 隐藏「列浏览器」页——上游这一页叫
        /// `results-metadata`（`plugin.xml:1105-1107`），本仓列浏览器即 metadata 面板
        /// （`ports.rs:846` 的注释：「列浏览器：搜索框 + 列清单 + 底部元数据明细……
        /// 对齐 DBeaver metadata 面板」；`events.rs:3002` 也把 `results-metadata` 记为
        /// 「ResultSet metadata description」）
        ToggleBrowserPanel,
        /// 【第 83 批】`CTRL+ALT+F5`：显示 / 隐藏「分组」页（`panelId=results-grouping`，
        /// `plugin.xml:1108-1110`）
        ToggleGroupPanel,
        /// 【第 83 批】`CTRL+ALT+F6`：显示 / 隐藏「计算」页（上游 `panelId=column-aggregate`，
        /// `plugin.xml:1111-1113`）
        ToggleStatsPanel,
    ]
);

/// 单元格右键「面板」子菜单的构造器（面板注入，见 [`menu::append_panels_submenu`]）。
///
/// 真值第三挂载点：`ResultSetViewer.java:3231-3233` 把 `fillPanelsMenu()` 的同一批条目
/// 挂成单元格右键的「面板」子菜单（`:3226-3229`）。本仓网格拿不到面板状态（页签集合 /
/// 可见性 / 方向 / 最大化都在 `ResultPanel` 上），所以由面板注入闭包：右键瞬间读真值，
/// 点下去仍走同一条 `ResultActionQueue`——与 `action_queue` 同一条通道。
///
/// 签名对齐 vendor `PopupMenu::build` + `PopupMenuItem::submenu`：拿到一个已构建的子菜单
/// 实体。`TableDelegate::context_menu` 只有菜单值、没有 `Context<PopupMenu>`，vendor 在
/// `PopupMenu::render:1574-1590` 对这条路径补齐 `parent_menu`（注释点名的正是
/// "a table delegate's `context_menu`"），所以这里不必自己接父菜单。
pub type PanelsMenuBuilder = std::rc::Rc<
    dyn Fn(
        gpui_kit::component::menu::PopupMenu,
        &mut gpui_kit::Window,
        &mut gpui_kit::Context<gpui_kit::component::menu::PopupMenu>,
    ) -> gpui_kit::component::menu::PopupMenu,
>;

pub use cell::format_cell_text;
pub use selection::{page_target, selection_counts, CellRange, GridKeyStep, PageDir};
pub use delegate::{
    CELL_DISPLAY_LIMIT, COLUMN_FILTER_VALUE_LIMIT, CellEdit, ColumnFilterHook, ColumnStatsHook,
    CommitEditHook, CommitLocalEditHook, FilterValueHook, InsightColumnHook, LoadMoreHook,
    PreviewValueHook, RESULT_GRID_CONTEXT, ResultGridDelegate, RowActionHook, SortDownHook,
};
pub use edit::{can_edit_cell, validate_edit_value};
pub use menu::{
    ContextAction, ContextMenuItem, ContextTarget, RowActionState, RowTargetState, SelectionStatKind,
    column_browser_menu_items, column_distinct_values, column_menu_items, context_menu_items,
    filter_panel_menu_item, filter_settings_menu_items, filters_menu_items,
    layout_presentation_menu_items, reset_cell_applicable, row_action_state, row_number_menu_items,
    row_target_state, selection_stat_menu_items,
};
pub use stats::{
    ColumnStats, column_stats, column_stats_from_values, group_by_values, stats_from_fact,
    stats_lines,
};
pub use status::{
    MAX_SINGLE_SEGMENT_ROWS, ResultControls, ResultPane, ResultStatus, ResultToolbar,
    SelectionStatPrefs, WINDOW_FETCH_NOTICE, bytes_text, duration_text, fetch_more_hint,
    new_table_state, render, render_status_row, render_toolbar, selection_statistics,
    status_action_placeholders, status_config_items, status_segments, thousands, truncated_hint,
    truncation_notice, window_fetch_active,
};
pub use text::{GridSelection, ResultMatch};
