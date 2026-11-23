//! 列浏览器清单（右栏「列浏览器」页签的清单部分）——gpui-kit `list` 组件承载
//!
//! 迁移自 panel.rs 手搓清单行（`div().cursor_pointer().on_click(...)`），换用
//! [`ListState`] / [`ListDelegate`] 后拿到：hover / 键盘上下 + Enter 确认 / 焦点管理 /
//! 搜索框（`searchable`，自带 cleanable）/ 滚动条 / a11y。
//!
//! ## 交互口径（对齐 panel.rs 原手搓，见 result-panel.md §8.12）
//!
//! - 单击 / Enter：选中列（`SelectColumn`）——List 内部先 `set_selected_index` 再
//!   `confirm`，本 delegate 在 `confirm` 里入队（避免与双击路径重复入队）。
//! - 双击：定位到该列首行（`LocateColumn`）——`ListItem` 无 `on_double_click`，
//!   在 `on_click` 里按 `ClickEvent::click_count >= 2` 判定（单击不做事，交给 confirm）。
//! - 眼睛按钮：切换该列隐藏 / 显示（`stop_propagation` 防冒泡触发行选中）。
//! - 右键菜单：冻结 / 排序 / 隐藏 / 列统计 / 复制列名 / 显示全部（复用
//!   [`crate::grid::column_browser_menu_items`]）。
//! - 搜索：列名 / 类型名 / 注释忽略大小写包含匹配；隐藏列保留在清单（置灰斜体）。
//!
//! ## 多选（P2-3）
//!
//! `List` 只维护**单个** `selected_index`（每次点击强制覆盖，`confirm` 只把
//! `secondary` 修饰键透传过来），没有集合语义。所以多选集合由本 delegate 自持
//! （[`Self::selected_columns`]），`confirm` 里按「Ctrl 点击增删 / 普通点击单选替换」
//! 算出新集合，经 `ResultRequest::ColumnSelectionChanged` 回传面板镜像；行内容的
//! 集合底色也在 `render_item` 里按集合自绘（List 的 `selected` 高亮仍表示"焦点行"，
//! 两者可分离：焦点行可以不在集合里）。
//!
//! ## 选中归属
//!
//! 清单行的选中**高亮**由 List 内部 `selected_index` 统一管理（`render_list_item`
//! 会覆盖 item 的 `.selected()`），面板侧 `selected_col` 只作为「选中了哪个数据列」的
//! 语义权威（明细 / 计算分组兜底 / 网格反推），两处通过 host 事件路径同步。

use shared::i18n::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::IndexPath;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::table::TableState;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::grid::{self as result_grid, ResultGridDelegate};
use crate::ports::{ResultActionQueue, ResultRequest};

/// 列浏览器 List delegate：一份纯投影（列名 / 类型名 / 隐藏集合 / 搜索词），
/// 数据由面板在 `sync`（事件路径）经 [`Self::set_data`] 灌入。
pub struct ColumnBrowserDelegate {
    /// 列名（数据列序，与网格 delegate `columns()` 一致）
    columns: Vec<String>,
    /// 类型名（数据列序；空串 = 无类型）
    type_names: Vec<String>,
    /// 隐藏集合（数据列序；隐藏列保留在清单，置灰斜体）
    hidden_columns: Vec<bool>,
    /// 列注释（数据列序；参与搜索匹配）
    comments: Vec<Option<String>>,
    /// 搜索词（List 内置搜索框经 `perform_search` 写入）
    query: String,
    /// 最近一次选中的数据列序（`set_selected_index` 记录，`confirm` 消费）
    last_selected: Option<usize>,
    /// 【P2-3】多选集合（数据列序，升序去重）。`List` 本身只给单个 `selected_index`，
    /// 集合语义只能由 delegate 自持：`confirm(secondary)` 增删、普通点击替换成单元素。
    selected_set: Vec<usize>,
    /// 动作队列（单击选中 / 双击定位 / 眼睛切显隐 / 右键动作都经它交宿主分派）
    queue: ResultActionQueue,
    /// 网格实体（右键动作直接 update 网格 delegate：冻结 / 隐藏 / 显示全部 / 清排序）
    grid: Entity<TableState<ResultGridDelegate>>,
}

impl ColumnBrowserDelegate {
    /// 新建（空投影；面板构造面板实体时创建，`sync` 后才有数据）
    pub fn new(queue: ResultActionQueue, grid: Entity<TableState<ResultGridDelegate>>) -> Self {
        Self {
            columns: Vec::new(),
            type_names: Vec::new(),
            hidden_columns: Vec::new(),
            comments: Vec::new(),
            query: String::new(),
            last_selected: None,
            selected_set: Vec::new(),
            queue,
            grid,
        }
    }

    /// 面板 `sync` 灌入列投影（每次换结果集 / 刷新都会重灌；`set_data` 后清
    /// `last_selected`，避免旧结果集的列序号被 `confirm` 误用）。
    ///
    /// **投影没变就什么都不做**：对齐 DBeaver `MetaDataPanel.refresh:184-196`
    /// （属性清单逐项相同就直接 `return`，注释原文 "No changes"）。本仓这条短路还有
    /// 实际收益——`set_data` 会清掉 `last_selected` 与多选集合，而 `sync` 在「刷新到同一份
    /// 结果」这种无实质变化时也会被调用，不短路会把用户的多选和 List 滚动位置一起打掉。
    pub fn set_data(
        &mut self,
        columns: Vec<String>,
        type_names: Vec<String>,
        hidden_columns: Vec<bool>,
        comments: Vec<Option<String>>,
    ) {
        let unchanged = self.columns == columns
            && self.type_names == type_names
            && self.hidden_columns == hidden_columns
            && self.comments == comments;
        if unchanged {
            return;
        }
        self.columns = columns;
        self.type_names = type_names;
        self.hidden_columns = hidden_columns;
        self.comments = comments;
        self.last_selected = None;
        // 换结果集后旧列序号全部失效，集合一并清空（否则批量动作会打到同号的另一列）
        self.selected_set.clear();
    }

    /// 【P2-3】当前多选集合（数据列序，升序）
    pub fn selected_columns(&self) -> &[usize] {
        &self.selected_set
    }

    /// 【P2-3】面板镜像回灌选集（「清空选择」按钮走这条路；与现集合相同则不动，
    /// 避免每帧 `set_*` 抖动）
    pub fn set_selected_columns(&mut self, columns: Vec<usize>) {
        if self.selected_set == columns {
            return;
        }
        self.selected_set = columns;
    }

    /// 面板 `sync` 同步真实网格实体：构造期传入的是占位网格（面板尚未建好表状态），
    /// 右键动作（冻结 / 隐藏 / 显示全部 / 清排序）直接 update 网格 delegate，
    /// 不换真实网格会落在空数据上（功能回归）。每次换结果集都要先 `set_grid` 再
    /// `set_data`。
    pub fn set_grid(&mut self, grid: Entity<TableState<ResultGridDelegate>>) {
        self.grid = grid;
    }

    /// 当前搜索词（测试断言）
    pub fn query(&self) -> &str {
        &self.query
    }

    /// 过滤后的数据列序（`items_count` / `render_item` 用它把 List 行号映射回数据列；
    /// 列数小，每次现算可接受；面板 `set_selection_pos` 也经它换算网格反推的选中行号）
    pub(crate) fn matched(&self) -> Vec<usize> {
        filter_browser_columns(&self.columns, &self.type_names, &self.comments, &self.query)
    }
}

impl ListDelegate for ColumnBrowserDelegate {
    type Item = ListItem;

    /// List 内置搜索框每敲一键都会触发；同步更新搜索词并请求重渲染
    /// （过滤结果随后由 `items_count` / `render_item` 现算）。
    fn perform_search(
        &mut self,
        query: &str,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.trim().to_string();
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.matched().len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let data_col = *self.matched().get(ix.row)?;
        let name = self.columns.get(data_col)?.clone();
        let type_name = self.type_names.get(data_col).cloned().unwrap_or_default();
        // 【DBeaver 对齐】列注释（`MetaDataPanel.addExtraColumns` 的 `Description` 列，
        // 值 = `entityAttribute.getDescription()`）。窄侧栏摆不下第三列，改成列名下的
        // 第二行（同一份数据，换形态）。
        let comment = browser_comment_text(self.comments.get(data_col).unwrap_or(&None));
        let hidden = self.hidden_columns.get(data_col).copied().unwrap_or(false);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let selected_bg = theme.list_active;
        let in_set = self.selected_set.contains(&data_col);

        // 眼睛：只切显隐，不冒泡触发行选中（与手搓版同口径）
        let eye = {
            let queue = self.queue.clone();
            Button::new(("editor-result-col-browser-eye", data_col))
                .ghost()
                .small()
                .icon(if hidden {
                    IconName::EyeOff
                } else {
                    IconName::Eye
                })
                .tooltip(if hidden {
                    "显示此列"
                } else {
                    "隐藏此列"
                })
                .on_click(move |_, _window, app| {
                    app.stop_propagation();
                    queue.request(ResultRequest::ToggleColumnVisibility(data_col), app);
                })
        };

        // 右键菜单素材在 render 期一次取好（与列头菜单同口径；闭包活到比本帧长，
        // 「当时这一列是什么」只有现在知道）
        let grid_entity = self.grid.clone();
        // 【列布局落盘】清单右键的隐藏 / 冻结也改 delegate，改完从这里交回「该存了」
        let layout_queue = self.queue.clone();
        let delegate = self.grid.read(&*cx).delegate();
        let frozen = delegate.is_frozen(data_col);
        let sort_active = delegate
            .sort_state()
            .is_some_and(|(sort_col, _)| sort_col == data_col);
        let target = result_grid::ContextTarget {
            value: String::new(),
            column_name: name.clone(),
            column: data_col,
            row: 0,
            rows: delegate.view_len(),
            frozen,
            hidden_columns: delegate.hidden_count(),
            insight: false,
            // 列浏览器清单只做列级动作（整表入口不在这一处）
            analyze: false,
            persist: false,
            table: None,
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active,
            // 列浏览器菜单没有行身份：行级三项只出现在单元格右键菜单
            row_ref: crate::rowset::RowRef::default(),
            editable: false,
            is_deleted: false,
            // 【第 67 批】列浏览器没有「这一格」：重置单元格只在单元格右键
            cell_changed: false,
            // 列浏览器菜单不摆选中统计三颗（它们只在单元格右键的 layout 段）
            selection_stat_rows: false,
            selection_stat_columns: false,
            selection_stat_cells: false,
            // 列浏览器菜单同样不摆布局段（呈现方式组只在单元格右键）
            presentation: delegate.presentation_view(),
            record_mode: delegate.transposed,
            // 同上：筛选条显隐项也不在列浏览器菜单里
            show_filter_panel: delegate.show_filter_panel(),
        };
        let menu_items = result_grid::column_browser_menu_items(&target);
        // 【多选批量动作】对齐 DBeaver：多选动作挂在**清单右键菜单**上
        // （`MetaDataPanel.fillCustomActions:234-252`），页内工具条只留「生成 DDL」。
        // 只有右键落在**多选集合内**时才出现（语义与「复制列文本」一致：
        // 操作对象是清单的结构化选中项，而不是单行）。
        let batch = batch_menu_items(&self.selected_set, data_col);
        let batch_queue = self.queue.clone();
        let selected: Vec<usize> = self.selected_set.clone();
        let copy_text = delegate.column_text(data_col);
        let stats_payload = delegate.column_stats_payload(data_col);
        // 【复制列名】要拷哪些列：右键这一列在**多选集合**里 → 拷整个集合（升序），
        // 否则只拷这一列（对齐 DBeaver：拷的是清单当前的结构化选中项，多选才有多行）
        let copy_names = {
            let columns = delegate.columns();
            column_names_to_copy(columns, &self.selected_set, data_col)
        };

        // 双击判定：ListItem 无 on_double_click，单击（click_count == 1）留给 List
        // 内部 confirm（SelectColumn），双击在第二次点击时入队 LocateColumn
        let queue_double = self.queue.clone();

        // 行内容（列名 flex_1 撑开 + 类型名 + 眼睛；隐藏列置灰斜体）。右键菜单挂在
        // 内容 div 而非 ListItem 本体：`context_menu` 会把元素包装成 `ContextMenu<Self>`，
        // 而 List 要求 `render_item` 返回 `ListItem`（List 内部要覆盖 `.selected()` /
        // `.secondary_selected()`，Selectable 是硬约束）。内容 div 带业务键 id——
        // 同一调用点渲染多行时菜单状态按 id 区分（见 ContextMenu 的 RowsRoot 用例）；
        // 内容 div `w_full()`，右键命中区覆盖整行，与手搓版一致。
        let content = div()
            .id(("editor-result-col-browser-menu", data_col))
            .h_flex()
            .w_full()
            .items_center()
            .gap_1()
            .min_w_0()
            .when(hidden, |r| r.text_color(muted).italic())
            // 多选集合底色：List 只画单个 `selected`（焦点行），集合高亮只能由内容层
            // 自绘；两者可分离（焦点行可以不在集合里，例如 Ctrl 点掉了它）
            .when(in_set, |r| r.bg(selected_bg))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .v_flex()
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .child(SharedString::from(name.clone())),
                    )
                    // 注释第二行：只在有注释时出现（DBeaver 的 Description 列）
                    .children(comment.map(|comment| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(muted)
                            .debug_selector(|| "editor-result-col-browser-comment".to_string())
                            .child(SharedString::from(comment))
                    })),
            )
            .when(!type_name.is_empty(), |r| {
                r.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(SharedString::from(type_name)),
                )
            })
            .child(eye)
            .context_menu(move |mut menu: PopupMenu, _window, _cx| {
                for item in menu_items.clone() {
                    match item.action {
                        result_grid::ContextAction::CopyColumnNames => {
                            // 文本在渲染期算好（多选集合 / 列名都只有此刻拿得到）
                            let text = copy_names.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    // 空文本不写剪贴板（DBeaver 同口径：`text.isEmpty()` 直接返回）
                                    if text.is_empty() {
                                        return;
                                    }
                                    app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                },
                            ));
                        }
                        result_grid::ContextAction::CopyColumn(_) => {
                            let text = copy_text.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                },
                            ));
                        }
                        result_grid::ContextAction::ToggleFreeze(column) => {
                            let grid_entity = grid_entity.clone();
                            let queue = layout_queue.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    grid_entity.update(app, |state, cx| {
                                        state.delegate_mut().toggle_freeze(column);
                                        state.refresh(cx);
                                    });
                                    queue.request(ResultRequest::ColumnLayoutChanged, app);
                                },
                            ));
                        }
                        result_grid::ContextAction::HideColumn(column) => {
                            let grid_entity = grid_entity.clone();
                            let queue = layout_queue.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    grid_entity.update(app, |state, cx| {
                                        state.delegate_mut().hide_column(column);
                                        state.refresh(cx);
                                    });
                                    queue.request(ResultRequest::ColumnLayoutChanged, app);
                                },
                            ));
                        }
                        result_grid::ContextAction::ShowAllColumns => {
                            let grid_entity = grid_entity.clone();
                            let queue = layout_queue.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    grid_entity.update(app, |state, cx| {
                                        state.delegate_mut().show_all_columns();
                                        state.refresh(cx);
                                    });
                                    queue.request(ResultRequest::ColumnLayoutChanged, app);
                                },
                            ));
                        }
                        result_grid::ContextAction::ColumnStats { column: _ } => {
                            if let Some((hook, name, text)) = stats_payload.clone() {
                                menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                    move |_, _window, app| {
                                        hook(name.clone(), text.clone(), app);
                                    },
                                ));
                            }
                        }
                        result_grid::ContextAction::ClearSort => {
                            let grid_entity = grid_entity.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    grid_entity.update(app, |state, cx| {
                                        state.delegate_mut().clear_sort();
                                        state.refresh(cx);
                                    });
                                },
                            ));
                        }
                        _ => {
                            // 排序下发 / 筛选 / 洞察 / 值与行动作属于列头与单元格，
                            // 清单入口不生成（`column_browser_menu_items` 也不摆这些项）
                        }
                    }
                }
                // 多选批量动作接在菜单尾部（前置一条分隔）：只在右键落在多选集合内时出现
                if !batch.is_empty() {
                    menu = menu.item(PopupMenuItem::separator());
                }
                for (action, label) in batch.iter() {
                    let queue = batch_queue.clone();
                    let action = *action;
                    let columns = selected.clone();
                    menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                        move |_, _window, app| {
                            app.stop_propagation();
                            match action {
                                ColumnBatchAction::HideSelected => queue.request(
                                    ResultRequest::SetColumnsHidden {
                                        columns: columns.clone(),
                                        hidden: true,
                                    },
                                    app,
                                ),
                                ColumnBatchAction::ShowSelected => queue.request(
                                    ResultRequest::SetColumnsHidden {
                                        columns: columns.clone(),
                                        hidden: false,
                                    },
                                    app,
                                ),
                                ColumnBatchAction::ClearSelection => {
                                    queue.request(ResultRequest::ClearColumnSelection, app)
                                }
                            }
                        },
                    ));
                }
                menu
            });

        let item = ListItem::new(("editor-result-col-browser-row", data_col))
            .px_2()
            .text_xs()
            .child(content)
            .on_click(move |e: &ClickEvent, _window, app| {
                if e.click_count() >= 2 {
                    queue_double.request(ResultRequest::LocateColumn(data_col), app);
                }
            });

        Some(item)
    }

    /// List 选中变化（单击 / 键盘上下 / 搜索自动选中首项）：记录数据列序供
    /// `confirm` 消费；越界防御（搜索词把匹配行过滤掉时旧行号可能出界）。
    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.last_selected = ix.and_then(|ix| self.matched().get(ix.row).copied());
        cx.notify();
    }

    /// 单击 / Enter 确认：入队选中该列（网格列高亮联动由宿主分派）。
    /// `secondary` = 点击时按了 Ctrl/Cmd（`List` 的 `ClickEvent::modifiers().secondary`）：
    /// 按下 → 该列在集合里增删（多选），否则 → 集合替换成这一列（单选）。
    /// 集合一变就回传面板（`ColumnSelectionChanged`），面板据此渲染"已选 N 列"。
    fn confirm(
        &mut self,
        secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        if let Some(data_col) = self.last_selected {
            self.queue.push(ResultRequest::SelectColumn(data_col));
            let next = next_multi_selection(&self.selected_set, data_col, secondary);
            if next != self.selected_set {
                self.selected_set = next.clone();
                self.queue
                    .push(ResultRequest::ColumnSelectionChanged(next));
            }
        }
        cx.notify();
    }

    /// 空清单（无列 / 搜索无匹配）：「无匹配列」居中（与手搓版同文案）
    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .px_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(tr("无匹配列"))
            .into_any_element()
    }
}

/// 【多选批量动作】列浏览器行右键菜单里的批量项（纯函数）：操作对象 = 清单的结构化
/// 选中项，所以只有「右键的那一列在多选集合里」时才出现（对齐 DBeaver
/// `MetaDataPanel.fillCustomActions:234-252` 的 `getStructuredSelection()` 口径）。
///
/// 集合为空、或右键列不在集合里 → 不生成任何批量项（此时右键菜单就是单列菜单）。
pub(crate) fn batch_menu_items(
    selected: &[usize],
    clicked: usize,
) -> Vec<(ColumnBatchAction, String)> {
    if selected.is_empty() || !selected.contains(&clicked) {
        return Vec::new();
    }
    let n = selected.len();
    vec![
        (ColumnBatchAction::HideSelected, format!("隐藏所选（{n} 列）")),
        (ColumnBatchAction::ShowSelected, format!("显示所选（{n} 列）")),
        (ColumnBatchAction::ClearSelection, format!("清空选择（{n} 列）")),
    ]
}

/// 批量动作种类（右键菜单里的三条；渲染层按它入队对应 `ResultRequest`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnBatchAction {
    HideSelected,
    ShowSelected,
    ClearSelection,
}

/// 多选集合推进（纯函数）：`additive`（Ctrl/Cmd 点击）= 有则删、无则加；
/// 非 additive = 替换成单元素。返回值升序去重（面板/批量动作都按数据列序处理）。
pub(crate) fn next_multi_selection(
    current: &[usize],
    clicked: usize,
    additive: bool,
) -> Vec<usize> {
    if !additive {
        return vec![clicked];
    }
    if current.contains(&clicked) {
        current.iter().copied().filter(|c| *c != clicked).collect()
    } else {
        let mut next: Vec<usize> = current.to_vec();
        next.push(clicked);
        next.sort_unstable();
        next.dedup();
        next
    }
}

/// 【列注释】清单第二行要显示的注释文本（纯函数）：`None` / 纯空白 → 不显示；
/// 有内容则去首尾空白后原样返回（对齐 DBeaver `addExtraColumns` 的 `Description` 列，
/// 空注释不占第二行）。
pub(crate) fn browser_comment_text(comment: &Option<String>) -> Option<String> {
    comment
        .as_ref()
        .map(|comment| comment.trim())
        .filter(|comment| !comment.is_empty())
        .map(|comment| comment.to_string())
}

/// 【复制列名】要写进剪贴板的文本（纯函数；对齐 DBeaver
/// `meta_data_panel_action_copy_column_text`：选中几行就拷几行，换行分隔）。
///
/// 取法：右键那一列在**多选集合**里 → 拷整个集合（按数据列序）；否则只拷它自己。
/// 集合里的下标越界一律跳过（搜索词把行过滤掉后旧下标可能失效）。
pub(crate) fn column_names_to_copy(
    columns: &[String],
    selected: &[usize],
    clicked: usize,
) -> String {
    if selected.contains(&clicked) {
        selected
            .iter()
            .filter_map(|column| columns.get(*column))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        columns.get(clicked).cloned().unwrap_or_default()
    }
}

/// 列浏览器搜索过滤（纯函数）：忽略大小写包含匹配列名 / 类型名 / 注释；
/// 空词 = 全量原序；隐藏列保留在清单（置灰斜体由渲染层处理，过滤不剔隐藏列）。
pub(crate) fn filter_browser_columns(
    names: &[String],
    type_names: &[String],
    comments: &[Option<String>],
    needle: &str,
) -> Vec<usize> {
    let needle = needle.trim().to_lowercase();
    (0..names.len())
        .filter(|&i| {
            if needle.is_empty() {
                return true;
            }
            names[i].to_lowercase().contains(&needle)
                || type_names
                    .get(i)
                    .map(|t| t.to_lowercase().contains(&needle))
                    .unwrap_or(false)
                || comments
                    .get(i)
                    .and_then(|c| c.as_ref())
                    .map(|c| c.to_lowercase().contains(&needle))
                    .unwrap_or(false)
        })
        .collect()
}
