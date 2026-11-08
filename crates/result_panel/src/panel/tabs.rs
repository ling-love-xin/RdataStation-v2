//! 页签域（`panel/` 第六块）：值详情 / 统计 / 分组 / 引用 / 执行 SQL 各右栏页签。

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::table::TableDelegate;
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::tree::tree;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::ResultPanel;
use crate::grid::cell::{
    HEX_DUMP_BYTES_PER_LINE, ViewerMode, classify_value, hex_lines, hex_to_bytes,
};
use crate::grid::stats::{ColumnStats, fmt_grouped, text_minmax};
use crate::ports::{ResultActionQueue, ResultRequest, StatsItem, StatsMode};
#[cfg(test)]
use crate::ports::RightTab;
use crate::store::ColumnKind;
use crate::ui;

/// 【P2-1】hex 转储每页行数（16 行 × 16 字节 = 256 字节 / 页）
const HEX_LINES_PER_PAGE: usize = 16;

/// 【P2-1】hex 页号与总页数：空值也算 1 页（好显示「第 1/1 页」），页号夹到合法范围
pub(crate) fn hex_page_clamp(total_bytes: usize, page: usize) -> (usize, usize) {
    let page_bytes = HEX_LINES_PER_PAGE * HEX_DUMP_BYTES_PER_LINE;
    let total_pages = total_bytes.div_ceil(page_bytes).max(1);
    (page.min(total_pages - 1), total_pages)
}

/// 【P2-1】hex 翻页按钮：到边界直接禁用（不靠"点了页号不动"兜底）
fn hex_page_button(
    id: &'static str,
    icon: IconName,
    tooltip: &'static str,
    target_page: usize,
    enabled: bool,
    queue: ResultActionQueue,
) -> AnyElement {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon)
        .tooltip(tooltip)
        .debug_selector(move || id.to_string())
        .disabled(!enabled)
        .on_click(move |_, _window, app| {
            queue.request(ResultRequest::HexPage(target_page), app);
        })
        .into_any_element()
}

impl ResultPanel {
    pub(crate) fn render_value_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let Some((row, col)) = self.selection_pos else {
            return self.sidebar_empty("未选中单元格", cx);
        };
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let value = delegate.cell_text(row, col, cx);
        // 列名 / 类型：普通视图 = columns[col - 1]（col 含行号槽）；行号槽 / 转置
        // 给不出语义列名，标题只用位置
        let (name, type_name) = if col == 0 || self.transposed {
            (None, None)
        } else {
            let data_col = col - 1;
            (
                delegate.columns().get(data_col).cloned(),
                delegate
                    .column_types()
                    .get(data_col)
                    .and_then(|t| t.as_ref())
                    .map(|t| t.name.clone()),
            )
        };
        let title = match (&name, &type_name) {
            (Some(name), Some(type_name)) => format!("{name} · {type_name}"),
            (Some(name), None) => name.clone(),
            _ => "值".to_string(),
        };
        // 【#28-B4】位置标注：本地新增 / 复制行标「本地」，被删底层行标「已删除」；
        // 行号仍是视图行号（D6：不另造本地序号体系，身份靠标注与网格底色 / 删除线区分）。
        // 【2026-10-28 微调】第一行 = 值位置信息：行 + 列号（col 含行号槽，col==0 是行号
        // 槽本身，无列号可标；普通列标「第 N 列」）。
        let row_marker = {
            let base = if col == 0 {
                format!("第 {} 行", row + 1)
            } else {
                format!("第 {} 行 · 第 {} 列", row + 1, col)
            };
            if delegate.row_is_local(row) {
                format!("{base} · 本地")
            } else if delegate.is_deleted_row(row) {
                format!("{base} · 已删除")
            } else {
                base
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-value-tab".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_2()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(title))
                    .child(SharedString::from(row_marker)),
            )
            .child({
                // 【值查看器分派】按列类型 + 值形态选内容形态：文本块 / JSON 树 / hex 转储 /
                // 图片占位（纯 render 层；JSON 树状态由事件路径 `rebuild_value_tree` 重建）
                let kind = if col == 0 {
                    ColumnKind::Unknown
                } else {
                    delegate.column_kind(col)
                };
                self.render_value_body(kind, &value, cx)
            })
            .into_any_element()
    }

    /// 【值查看器】内容区按分派模式渲染（纯 render 层；JSON 树解析在事件路径）
    fn render_value_body(&self, kind: ColumnKind, value: &str, cx: &App) -> AnyElement {
        match classify_value(kind, value) {
            ViewerMode::Text => div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .px_2()
                .py_1()
                .text_xs()
                .whitespace_normal()
                .child(SharedString::from(value))
                .into_any_element(),
            ViewerMode::Json => self.render_json_tree(cx),
            ViewerMode::Hex => self.render_hex_dump(value, cx),
            ViewerMode::Image { format, size } => {
                let muted = cx.theme().colors.muted_foreground;
                // 【P2-2】事件路径已把 hex 解成 `Arc<Image>`（`rebuild_value_tree`），
                // 这里只摆 `img`：元素在 layout/paint 期按内容 id 走资产缓存解码。
                // 真解码失败由 `with_fallback` 回落文本（不静默空白）。
                if let Some(image) = &self.value_image {
                    let fallback = format;
                    let fallback_size = size;
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_2()
                        .child(
                            img(ImageSource::Image(image.clone()))
                                .w_full()
                                .max_h_full()
                                .with_fallback(move || {
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(SharedString::from(format!(
                                            "图片解码失败（{fallback} · {fallback_size} B）"
                                        )))
                                        .into_any_element()
                                }),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(muted)
                        .child(SharedString::from(format!(
                            "图片预览不可用：{format} · {size} B（格式不在支持集，或值不是合法 hex）"
                        )))
                        .into_any_element()
                }
            }
        }
    }

    /// 【值查看器】JSON 展开树（`tree::Tree` 组件；状态由事件路径重建，这里只摆）
    fn render_json_tree(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let state = self.json_tree.clone();
        tree(&state, move |_ix, entry, selected, _window, _app| {
            // 行内结构：depth 缩进 + 展开箭头（folder 才画）+ 键值标签。
            // 展开 / 收起由 Tree 组件自身处理（点击 folder 条目切换）。
            let indent = rems(ui::JSON_TREE_INDENT_STEP * entry.depth() as f32);
            let arrow = if entry.is_folder() {
                if entry.is_expanded() { "▾" } else { "▸" }
            } else {
                ""
            };
            let item = entry.item();
            ListItem::new(ElementId::Name(item.id.clone()))
                .selected(selected)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .pl(indent)
                        .child(
                            div()
                                .w(rems(ui::JSON_TREE_ARROW_WIDTH))
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(SharedString::from(arrow)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .child(SharedString::from(item.label.clone())),
                        ),
                )
        })
        .into_any_element()
    }

    /// 【值查看器 P2-1】hex 转储（每页 16 行 = 256 字节，可翻页）
    ///
    /// 分页是**可读性**而非性能优化：一屏只摆 16 行（offset · hex · ASCII 三栏），
    /// 大 BLOB 不必把上千行拼成字符串塞进元素树。页号是面板自持的纯视图态，按钮经
    /// `ResultRequest::HexPage` 回到 [`ResultPanel::set_hex_page`] 改它（再由宿主 drain
    /// 回来），所以 render 仍是纯读路径。
    fn render_hex_dump(&self, hex_text: &str, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let bytes = hex_to_bytes(hex_text);
        let total_bytes = bytes.len();
        // 页码兜底：值变短 / 清空后旧页号可能越界，渲染期夹住（不写回状态）
        let (page, total_pages) = hex_page_clamp(total_bytes, self.hex_page);
        let lines = hex_lines(
            &bytes,
            page * HEX_LINES_PER_PAGE * HEX_DUMP_BYTES_PER_LINE,
            HEX_LINES_PER_PAGE,
        );

        let pager = div()
            .h_flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_1()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(muted)
            .child(SharedString::from(format!(
                "第 {}/{} 页 · 共 {total_bytes} 字节",
                page + 1,
                total_pages
            )))
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .child(hex_page_button(
                        "editor-result-hex-prev",
                        IconName::ChevronLeft,
                        "上一页（前 256 字节）",
                        page.saturating_sub(1),
                        page > 0,
                        self.queue.clone(),
                    ))
                    .child(hex_page_button(
                        "editor-result-hex-next",
                        IconName::ChevronRight,
                        "下一页（后 256 字节）",
                        page + 1,
                        page + 1 < total_pages,
                        self.queue.clone(),
                    )),
            );

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .py_1()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .children(
                        lines
                            .into_iter()
                            .map(|line| div().child(SharedString::from(line))),
                    ),
            )
            .child(pager)
            .into_any_element()
    }

    /// 【右栏】计算页（对齐 DBeaver Calc `AggregateColumnsPanel`）：
    /// 列级聚合 + **可配置函数集**（＋ 添加 / 行尾 × 停用 / Reset 恢复）+ **聚合口径**
    /// 切换（数值 / 字符串）+ 两列表头（Function / Value）+ 千分位数字。
    ///
    /// 作用列 = 网格选中格优先（`selection_pos`），列浏览器清单选中列（`selected_col`）兜底；
    /// 转置视图不给语义列。数据 = 列级访问器 `column_stats_for`（数值口径走事实档列式
    /// 原生聚合，不物化整行矩阵）；字符串口径才取整列取值算字典序极值。
    ///
    /// **与 DBeaver 的两处有意偏离**（登记在册，不是遗漏）：
    /// 1. DBeaver 的「−」删的是 Tree 里选中的函数行；本仓统计行是普通元素、没有行选中
    ///    模型，改为每行行尾一个 ×（避免为对齐一个按钮去引整棵 Tree）。
    /// 2. DBeaver 的「按列分组聚合」（`GroupByColumnsAction`）以**多单元格选区**为输入；
    ///    本仓网格只有单格选择（`selection_pos: Option<(usize, usize)>`），没有多选模型，
    ///    故不实现——要做需先给 DataTable 引入单元格选区，属独立工程。
    pub(crate) fn render_stats_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let Some(data_col) = self.inspect_column(delegate) else {
            return self.sidebar_empty("选中一个数据列查看统计", cx);
        };
        let name = delegate
            .columns()
            .get(data_col)
            .cloned()
            .unwrap_or_else(|| "列".to_string());
        let stats = delegate.column_stats_for(data_col);
        // 字符串口径才需要整列取值（数值口径走事实档列式原生聚合，不物化矩阵）
        let text_extremes = matches!(self.stats_mode, StatsMode::Text)
            .then(|| text_minmax(&delegate.column_values(data_col)));
        let rows: Vec<AnyElement> = self
            .stats_items
            .iter()
            .map(|item| {
                let value = stat_value(*item, &stats, text_extremes.as_ref(), self.stats_mode);
                self.stat_row(*item, &value, cx)
            })
            .collect();
        // 全停用时的空态提示（不引 `when`：本 crate 的 styled 扩展没带这个便捷方法）
        let empty_hint = rows.is_empty().then(|| {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(SharedString::from("没有启用任何统计项：点 ＋ 添加"))
                .into_any_element()
        });
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-stats-tab".to_string())
            // 【R1 对齐】面板内工具条：DBeaver 每个侧面板自带一行靠右工具条
            // （`ResultSetViewer.java:1337-1374` 的 `ToolBarManager(SWT.RIGHT)`），
            // 本页把「＋ / Reset / 口径」收进这里（标题在左，动作在右，两列布局）
            .child(self.stats_toolbar(&name, cx))
            // 表头：对齐 DBeaver Calc 的 Tree 两个列头（Function / Value）
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_5()
                    .flex_none()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .text_xs()
                    .text_color(muted)
                    .child(
                        div()
                            .w(rems(ui::RESULT_SIDEBAR_KV_LABEL_WIDTH))
                            .flex_none()
                            .child(SharedString::from("函数")),
                    )
                    .child(div().flex_1().child(SharedString::from("值")))
                    // 行尾删除钮那一列的表头留白（与统计行的 × 列同宽）
                    .child(div().w(rems(1.25)).flex_none()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .children(empty_hint)
                    .children(rows)
                    .overflow_y_scrollbar(),
            )
            .into_any_element()
    }

    /// 【R2】计算页工具条：左「列名」标题 + 右动作（＋ 添加 / Reset / 口径 toggle），
    /// 对齐 DBeaver Calc 的 `fillToolBar`（`AggregateColumnsPanel.java:322-331`：
    /// ＋ / − / Reset / 分隔 / 分组 / 口径）。本仓「−」在统计行行尾，这里只摆三件。
    fn stats_toolbar(&self, column_name: &str, cx: &App) -> AnyElement {
        let muted = cx.theme().colors.muted_foreground;
        let missing: Vec<StatsItem> = StatsItem::ALL
            .into_iter()
            .filter(|item| !self.stats_items.contains(item))
            .collect();
        // ＋：还有未启用项才是下拉菜单；全启用时禁用（DBeaver 此时菜单为空、点了没反应，
        // 本仓直接给禁用态，避免"点了没反应"的假按钮）
        let add: AnyElement = if missing.is_empty() {
            Button::new("editor-result-stat-add")
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .tooltip("六项统计都已启用")
                .disabled(true)
                .into_any_element()
        } else {
            let queue = self.queue.clone();
            DropdownButton::new("editor-result-stat-add")
                .xsmall()
                .button(
                    Button::new("editor-result-stat-add-btn")
                        .ghost()
                        .xsmall()
                        .debug_selector(|| "editor-result-stat-add".to_string())
                        .icon(IconName::Plus)
                        .tooltip("添加统计项"),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    for item in missing.iter().copied() {
                        let queue = queue.clone();
                        menu = menu.item(PopupMenuItem::new(item.label().to_string()).on_click(
                            move |_, _window, app| {
                                queue.request(ResultRequest::AddStatsItem(item), app);
                            },
                        ));
                    }
                    menu
                })
                .into_any_element()
        };
        let reset = {
            let queue = self.queue.clone();
            Button::new("editor-result-stat-reset")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-stat-reset".to_string())
                .icon(IconName::RotateCcw)
                .tooltip("恢复默认统计项（六项全开）")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ResetStatsItems, app);
                })
                .into_any_element()
        };
        // 口径 toggle：数值 / 字符串（DBeaver 用 TYPE_NUMBER / TYPE_STRING 图标 CHECK 表达；
        // 本仓给文字标签，窄图标位放不下两个语义）
        let mode_button = {
            let queue = self.queue.clone();
            let text_mode = matches!(self.stats_mode, StatsMode::Text);
            Button::new("editor-result-stat-mode")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-stat-mode".to_string())
                .label(if text_mode { "字符串" } else { "数值" })
                .toggled(text_mode)
                .tooltip("聚合口径：数值时最小/最大值按数值比较；字符串时按字典序（文本列也给极值）")
                .on_click(move |_, _window, app| {
                    queue.request(
                        ResultRequest::SetStatsMode(if text_mode {
                            StatsMode::Numeric
                        } else {
                            StatsMode::Text
                        }),
                        app,
                    );
                })
                .into_any_element()
        };
        div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_1()
            .px_2()
            .h(rems(ui::RESULT_PANEL_TOOLBAR_HEIGHT))
            .flex_none()
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(muted)
                    .child(SharedString::from(format!("「{column_name}」统计"))),
            )
            .child(
                Toolbar::new("editor-result-stats-actions")
                    .xsmall()
                    .p_0()
                    .contents(vec![add, reset, mode_button]),
            )
            .into_any_element()
    }

    /// 【R2】计算页一行：函数名（左）+ 值（右）+ 行尾停用钮（替代 DBeaver 的「−」）
    fn stat_row(&self, item: StatsItem, value: &str, cx: &App) -> AnyElement {
        let muted = cx.theme().colors.muted_foreground;
        let queue = self.queue.clone();
        let debug_id = format!("editor-result-stat-{:?}", item);
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .h_5()
            .text_xs()
            .child(
                div()
                    .w(rems(ui::RESULT_SIDEBAR_KV_LABEL_WIDTH))
                    .flex_none()
                    .text_color(muted)
                    .child(SharedString::from(item.label())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .child(SharedString::from(value.to_string())),
            )
            .child(
                Button::new(format!("editor-result-stat-remove-{:?}", item))
                    .ghost()
                    .xsmall()
                    .debug_selector(move || debug_id.clone())
                    .icon(IconName::X)
                    .tooltip(format!("停用「{}」", item.label()))
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::RemoveStatsItem(item), app);
                    }),
            )
            .into_any_element()
    }

    /// 【右栏】分组页（DBeaver「分组」）：按选中列本地 groupby 计数。
    ///
    /// 作用列口径同计算页：网格选中格优先，清单选中列兜底。
    /// 纯函数 `group_by_count`（保留首现顺序，NULL 独立成组）；大表口径是
    /// 下发 GROUP BY 重查（见 §8.8），本地页只服务中小结果集的即时分组。
    pub(crate) fn render_group_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let Some(data_col) = self.inspect_column(delegate) else {
            return self.sidebar_empty("选中一个数据列查看分组", cx);
        };
        // 【P2-2】列级访问器：只取目标列取值，不再 `visible_rows()` 物化整行矩阵
        let groups = delegate.group_by_for(data_col);
        let name = delegate
            .columns()
            .get(data_col)
            .cloned()
            .unwrap_or_else(|| "列".to_string());
        let rows_el: Vec<AnyElement> = groups
            .iter()
            .map(|(value, count)| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_6()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(SharedString::from(value.clone())),
                    )
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .into_any_element()
            })
            .collect();
        div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-result-group-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!(
                        "按「{name}」分组 · {} 组",
                        groups.len()
                    ))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows_el),
            )
            .into_any_element()
    }

    /// 【引用】外键引用查看页签（对齐 DBeaver 引用页签；backlog 空态）
    pub(crate) fn render_reference_tab(&self, cx: &App) -> AnyElement {
        self.sidebar_empty("选中一个列查看外键引用", cx)
    }

    /// 【1.2】「执行SQL」页签：筛选框运行后生成的 wrapped SQL 原文（只读 + 复制）。
    pub(crate) fn render_exec_sql_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let Some(sql) = self.exec_sql.clone() else {
            return self
                .sidebar_empty("在工具栏筛选框输入词并点 ▶ 运行，生成 SQL 会显示在这里", cx);
        };
        let copy_queue = self.queue.clone();
        div()
            .v_flex()
            .w_full()
            .h_full()
            .gap_2()
            .p_2()
            .debug_selector(|| "editor-result-exec-sql-tab".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(SharedString::from("生成的 SQL")),
                    )
                    .child(
                        Button::new("editor-result-exec-sql-copy")
                            .ghost()
                            .small()
                            .icon(IconName::Copy)
                            .label("复制")
                            .tooltip("复制生成的 SQL".to_string())
                            .on_click(move |_, _window, app| {
                                copy_queue.request(ResultRequest::CopyExecSql, app);
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .bg(theme.colors.background)
                    .p_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(SharedString::from(sql)),
            )
            .into_any_element()
    }

    /// 【侧栏】右栏各页共用的空态（居中灰字；页内容区 `flex_1`）
    pub(crate) fn sidebar_empty(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_1()
            .px_2()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(SharedString::from(text))
            .into_any_element()
    }
}

/// 【右栏】页签下标 → 枚举（TabBar 回调用；与 `RightTab` 定义顺序一致——
/// Browser=0 / Value=1 / Stats=2 / Group=3 / Reference=4 / ExecSql=5，
/// 对齐 DBeaver 右侧页签顺序）
/// 【2026-10-28 微调】生产横 tab 栏只显激活页签后，索引映射仅供测试用
#[cfg(test)]
pub(crate) fn right_tab_from_index(index: usize) -> RightTab {
    match index {
        1 => RightTab::Value,
        2 => RightTab::Stats,
        3 => RightTab::Group,
        4 => RightTab::Reference,
        5 => RightTab::ExecSql,
        _ => RightTab::Browser,
    }
}

/// 【右栏】键值行（计算页 / 列浏览器明细共用）：左标签右值，值超长截断防挤
pub(crate) fn kv_row(label: &str, value: &str, cx: &App) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .px_2()
        .h_5()
        .text_xs()
        .child(
            div()
                .w(rems(ui::RESULT_SIDEBAR_KV_LABEL_WIDTH))
                .flex_none()
                .text_color(muted)
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(SharedString::from(value)),
        )
        .into_any_element()
}

/// 【右栏】统计值格式化：无数值样本 = —；有值走千分位（对齐 DBeaver Calc 的
/// `###,###,###,###,###,##0.###`，见 [`fmt_grouped`]）
pub(crate) fn fmt_stat(value: Option<f64>) -> String {
    value.map(fmt_grouped).unwrap_or_else(|| "—".to_string())
}

/// 【R2 计算页】单项取值（纯函数，便于单测）：
/// - 计数类（非空 / NULL）不受口径影响，走千分位整数
/// - 数值口径：Min/Max/Avg/Sum 走千分位浮点，无样本 = —
/// - 字符串口径：Min/Max 走字典序（文本列也给极值）；Avg/Sum 不出值（DBeaver 的
///   SUM/AVG 对字符串累计不成立）
pub(crate) fn stat_value(
    item: StatsItem,
    stats: &ColumnStats,
    text_extremes: Option<&(Option<String>, Option<String>)>,
    mode: StatsMode,
) -> String {
    match item {
        StatsItem::Count => fmt_grouped(stats.count as f64),
        StatsItem::Nulls => fmt_grouped(stats.nulls as f64),
        StatsItem::Min | StatsItem::Max => {
            if matches!(mode, StatsMode::Text) {
                let pick = text_extremes.and_then(|(min, max)| {
                    if matches!(item, StatsItem::Min) {
                        min.clone()
                    } else {
                        max.clone()
                    }
                });
                pick.unwrap_or_else(|| "—".to_string())
            } else if matches!(item, StatsItem::Min) {
                fmt_stat(stats.min)
            } else {
                fmt_stat(stats.max)
            }
        }
        StatsItem::Avg | StatsItem::Sum => {
            if matches!(mode, StatsMode::Text) {
                "—".to_string()
            } else if matches!(item, StatsItem::Avg) {
                fmt_stat(stats.avg)
            } else {
                fmt_stat(stats.sum)
            }
        }
    }
}
