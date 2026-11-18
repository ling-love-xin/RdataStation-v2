//! 页签域（`panel/` 第六块）：值详情 / 统计 / 分组 / 引用 / 执行 SQL 各右栏页签。

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::table::TableDelegate;
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::tree::tree;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::ResultPanel;
use super::events::{panel_view_item_label, panel_view_menu_items, PanelViewItem};
use crate::grid::cell::{
    HEX_DUMP_BYTES_PER_LINE, ViewerMode, classify_value, hex_lines, hex_to_bytes,
};
use crate::grid::stats::{ColumnStats, fmt_grouped, text_minmax};
use crate::ports::{
    GroupSort, ResultActionQueue, ResultRequest, RightTab, StatsItem, StatsMode, ValuePayload,
};
use crate::store::ColumnKind;
use crate::ui;

/// 【P2-1】hex 转储每页行数（16 行 × 16 字节 = 256 字节 / 页）
const HEX_LINES_PER_PAGE: usize = 16;

/// 【R3 分组页】「移除分组列」是否可用（纯函数）。
///
/// 对齐 DBeaver `GroupingColumnsContainer.canColumnBeRemoved`（`:92-94`）：只有
/// 「选中列在分组键里」且「移除后仍至少剩一列」时才可用——**删到零是「清空分组」
/// 的职责**，逐列删除不许把分组键删空。没显式分组键（走选中列兜底）时也没得删。
pub(crate) fn can_remove_group_column(group_columns: &[usize], selected: Option<usize>) -> bool {
    match selected {
        Some(selected) => group_columns.len() > 1 && group_columns.contains(&selected),
        None => false,
    }
}

/// 【计算页】「复制全部」文本（纯函数）。
///
/// 对齐 DBeaver `AggregateColumnsPanel.CopyAllAction`（`:471-495`）的**非分组口径**：
/// 逐行 `函数=值`、`\n` 分隔（分组口径是树缩进，本仓没有分组聚合，见 §8.87）。
/// 空集合返回空串——调用点按 DBeaver 同款「空文本不写剪贴板」处理。
pub(crate) fn stats_copy_all_text(entries: &[(StatsItem, String)]) -> String {
    entries
        .iter()
        .map(|(item, value)| format!("{}={}", item.label(), value))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 【保存值】按值形态给出待落盘的字节与默认文件名（纯函数；测试直接钉它）。
///
/// 三条口径（与值查看器同源，宿主不必再判一次值形态）：
/// - 文本 / JSON：写展示文本原文（JSON 存下来就是合法的 `.json`）；
/// - 二进制（hex 转储 / 图片）：写 `hex_to_bytes` 还原出的**真字节**——图片存下来
///   就是可用的 png/jpg/gif/webp，不是一份 hex 文本。
///
/// 默认文件名只给「行列号 + 扩展名」（1 基行号与列号），不带列名：列名可能含文件系统
/// 非法字符（`/` `:` `*` 等）与超长中文，清洗规则会引出一整套平台差异，不如不拼。
pub(crate) fn value_payload(
    kind: ColumnKind,
    value: &str,
    row: usize,
    col: usize,
) -> ValuePayload {
    let (bytes, ext) = match classify_value(kind, value) {
        // 图片格式名（PNG / JPEG / GIF / WebP）直接当扩展名（小写），与字节内容一致
        ViewerMode::Image { format, .. } => (hex_to_bytes(value), format.to_ascii_lowercase()),
        ViewerMode::Hex => (hex_to_bytes(value), "bin".to_string()),
        ViewerMode::Json => (value.as_bytes().to_vec(), "json".to_string()),
        ViewerMode::Text => (value.as_bytes().to_vec(), "txt".to_string()),
    };
    ValuePayload {
        bytes,
        default_name: format!("value_r{}_c{}.{ext}", row + 1, col),
    }
}

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
            // 空态也保留页内工具条（六页同形，见 `empty_page`）
            return self.empty_page("editor-result-value-toolbar", "值", "未选中单元格", cx);
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
        // 【R1】页内工具条：左「列名 · 类型」，右「位置标记 + 复制当前值 + 保存值」
        // （DBeaver 值页动作来自 `valueManager.contributeActions`——「保存至文件…」在
        // `ContentValueManager.contributeContentActions` 里贡献——与 `SaveValueAction`
        // （把值**写回**结果集，本仓只读预览不做）；本仓动作集 = 复制 + 保存至文件）
        let marker = div()
            .text_xs()
            .text_color(muted)
            .child(SharedString::from(row_marker))
            .into_any_element();
        let copy = {
            let queue = self.queue.clone();
            Button::new("editor-result-value-copy")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-value-copy".to_string())
                .icon(IconName::Copy)
                .tooltip("复制当前单元格的值")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CopyValue, app);
                })
                .into_any_element()
        };
        // 【保存值】对齐 DBeaver「保存至文件…」（`ContentValueManager.contributeContentActions`
        // `:86-99` → `saveToFile` `:291-334`）：落盘动作在宿主（选路径 + 写盘），
        // 面板只交字节与默认文件名。
        //
        // 【动作可用性】DBeaver 那份动作是**条件贡献**的：`if (!content.isNull())` 才加
        // `saveToFile`（`:92-99`）。本仓按同一口径置灰——NULL 格 / 行号槽没有内容可写。
        let savable = value_can_save_to_file(col, delegate.row_cell_text(row, col));
        let save = {
            let queue = self.queue.clone();
            Button::new("editor-result-value-save")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-value-save".to_string())
                .icon(IconName::Download)
                .disabled(!savable)
                .tooltip(if savable {
                    "把当前值保存到文件"
                } else {
                    "该格没有内容可保存（NULL / 行号槽）"
                })
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SaveValue, app);
                })
                .into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-value-tab".to_string())
            .child(self.panel_toolbar(
                "editor-result-value-toolbar",
                SharedString::from(title),
                vec![marker, copy, save],
                cx,
            ))
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

    /// 【R1】当前选中单元格的显示文本（值详情页「复制」用；事件路径调用，render 不读）
    pub fn selected_value_text(&self, cx: &App) -> Option<String> {
        let (row, col) = self.selection_pos?;
        let grid = self.grid.read(cx);
        Some(grid.delegate().cell_text(row, col, cx))
    }

    /// 【保存值】当前选中格的待落盘内容（对齐 DBeaver「保存至文件…」
    /// `ContentValueManager.saveToFile`；事件路径调用）。
    ///
    /// 字节与默认名全部由 [`value_payload`] 按值形态算，这里只补「选中格 → 列类型」的一跳；
    /// 行号槽（`col == 0`）给不出列类型，按文本处理（与值查看器同口径）。
    pub fn selected_value_payload(&self, cx: &App) -> Option<ValuePayload> {
        let (row, col) = self.selection_pos?;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        // 【动作可用性】与页内「保存值」钮同一个判据：NULL 格 / 行号槽没有内容可写，
        // 给空载荷就是让宿主写一个内容为 `NULL` 的文件（DBeaver 干脆不贡献该动作）。
        if !value_can_save_to_file(col, delegate.row_cell_text(row, col)) {
            return None;
        }
        let value = delegate.cell_text(row, col, cx);
        let kind = if col == 0 {
            ColumnKind::Unknown
        } else {
            delegate.column_kind(col)
        };
        Some(value_payload(kind, &value, row, col))
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
            // 空态也保留页内工具条（六页同形，见 `empty_page`）
            return self.empty_page(
                "editor-result-stats-toolbar",
                "计算",
                "选中一个数据列查看统计",
                cx,
            );
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
        // 【函数=值】启用项 + 当前值先算成一对表：行渲染与「复制全部」共用同一份快照
        // （复制全部必须拷「此刻看到的这些」，不能在菜单回调里再算一遍）
        let entries: Vec<(StatsItem, String)> = self
            .stats_items
            .iter()
            .map(|item| {
                let value = stat_value(*item, &stats, text_extremes.as_ref(), self.stats_mode);
                (*item, value)
            })
            .collect();
        let copy_all = stats_copy_all_text(&entries);
        let rows: Vec<AnyElement> = entries
            .iter()
            .map(|(item, value)| self.stat_row(*item, value, copy_all.clone(), cx))
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
                    .debug_selector(|| "editor-result-stats-header".to_string())
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
    /// 【R1】右栏各页共用的页内工具条：对齐 DBeaver 每个面板内部的一条
    /// `ToolBarManager(SWT.HORIZONTAL | SWT.RIGHT | SWT.FLAT)`（`ResultSetViewer.java:1337-1374`，
    /// 动作由各面板 `contributeActions` 贡献）。
    /// 形态：高 28px（面板头档 = vendor `Toolbar` XSmall 自带的 `min_h_7`，容器同高才不溢出）、
    /// 左标题 / 右动作组、底边一条 `HAIRLINE` 分隔线。
    /// **动作只给本仓已有能力**——不造没有底层实现的按钮：DBeaver 的引用页
    /// `contributeActions` 是空实现（`ReferencesPanel.java:156-158`），该页保持空态。
    pub(crate) fn panel_toolbar(
        &self,
        debug_id: &'static str,
        title: SharedString,
        actions: Vec<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_1()
            .px_2()
            .h(rems(ui::RESULT_PANEL_TOOLBAR_HEIGHT))
            .flex_none()
            .text_xs()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .debug_selector(move || debug_id.to_string())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.colors.muted_foreground)
                    .child(title),
            )
            .child(
                Toolbar::new(format!("{debug_id}-actions"))
                    .xsmall()
                    .p_0()
                    .contents(actions),
            )
            .child(self.panel_view_menu(debug_id, cx))
            .into_any_element()
    }

    /// 【右栏视图菜单】每页条右端那颗「视图菜单」（对齐 DBeaver `addDefaultPanelActions`，
    /// `ResultSetViewer.java:1590-1614`：那边 `addPanel` 给**每个面板**的面板工具条右端都挂
    /// 一颗 `IMG_LCL_VIEW_MENU` 按钮，点开是 `fillPanelsMenu()`（`:1559-1588`）的菜单）。
    ///
    /// 菜单 = 六页勾选项（`STYLE_CHECK`，当前页为勾选态）→ 分隔 → 面板最大化 → 分隔 →
    /// 面板总开关 → 布局方向（段位照 `fillPanelsMenu:1559-1588`；`CMD_ACTIVATE_PANELS`
    /// 本仓无端口、按「能力没有就不摆」不列）。条目清单与文案分别由纯函数
    /// `panel_view_menu_items` / `panel_view_item_label` 定
    /// （`PopupMenu` 内部不可读，测试钉在纯函数上）。
    fn panel_view_menu(&self, debug_id: &'static str, cx: &App) -> AnyElement {
        let _ = cx;
        let queue = self.queue.clone();
        let current: RightTab = self.right_tab;
        let panels_visible = self.panels_visible();
        let vertical_layout = self.vertical_layout;
        let panel_maximized = self.panel_maximized;
        let items = panel_view_menu_items();
        let menu_id = format!("{debug_id}-view-menu");
        let button = Button::new(menu_id.clone())
            .ghost()
            .xsmall()
            .icon(IconName::Menu)
            .tooltip("视图菜单（切换面板 / 显示与布局）")
            .debug_selector(move || menu_id.clone());
        DropdownButton::new(format!("{debug_id}-view-menu-anchor"))
            .button(button)
            .menu_accessibility_label("视图菜单")
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                let mut menu = menu;
                for item in items.iter().copied() {
                    match item {
                        PanelViewItem::Separator => {
                            menu = menu.item(PopupMenuItem::separator());
                        }
                        PanelViewItem::Page(tab) => {
                            let queue = queue.clone();
                            menu = menu.item(
                                PopupMenuItem::new(panel_view_item_label(
                                    item,
                                    panels_visible,
                                    vertical_layout,
                                    panel_maximized,
                                ))
                                .checked(tab == current)
                                .on_click(move |_, _window, app| {
                                    queue.request(ResultRequest::SetRightTab(tab), app);
                                }),
                            );
                        }
                        PanelViewItem::PanelsToggle => {
                            // 菜单项报「点了会做什么」：当前可见 → 收起，反之 → 展开
                            let target = !panels_visible;
                            let queue = queue.clone();
                            menu = menu.item(
                                PopupMenuItem::new(panel_view_item_label(
                                    item,
                                    panels_visible,
                                    vertical_layout,
                                    panel_maximized,
                                ))
                                .checked(panels_visible)
                                .on_click(move |_, _window, app| {
                                    queue.request(ResultRequest::SetPanelsVisible(target), app);
                                }),
                            );
                        }
                        PanelViewItem::LayoutToggle => {
                            let queue = queue.clone();
                            menu = menu.item(
                                PopupMenuItem::new(panel_view_item_label(
                                    item,
                                    panels_visible,
                                    vertical_layout,
                                    panel_maximized,
                                ))
                                .on_click(move |_, _window, app| {
                                    queue.request(ResultRequest::ToggleVerticalLayout, app);
                                }),
                            );
                        }
                        PanelViewItem::MaximizeToggle => {
                            let queue = queue.clone();
                            menu = menu.item(
                                PopupMenuItem::new(panel_view_item_label(
                                    item,
                                    panels_visible,
                                    vertical_layout,
                                    panel_maximized,
                                ))
                                .checked(panel_maximized)
                                .on_click(move |_, _window, app| {
                                    queue.request(ResultRequest::TogglePanelMaximize, app);
                                }),
                            );
                        }
                    }
                }
                menu
            })
            .into_any_element()
    }

    /// 对齐 DBeaver Calc 的 `fillToolBar`（`AggregateColumnsPanel.java:322-331`：
    /// ＋ / − / Reset / 分隔 / 分组 / 口径）。本仓「−」在统计行行尾，这里只摆三件。
    fn stats_toolbar(&self, column_name: &str, cx: &App) -> AnyElement {
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
                .tooltip("恢复默认统计项（对齐 DBeaver 默认集：只有「非空」）")
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
        self.panel_toolbar(
            "editor-result-stats-toolbar",
            SharedString::from(format!("「{column_name}」统计")),
            vec![add, reset, mode_button],
            cx,
        )
    }

    /// 【R2】计算页一行：函数名（左）+ 值（右）+ 行尾停用钮（替代 DBeaver 的「−」）。
    ///
    /// 【R7 对齐右键菜单】行上挂 `AggregateColumnsPanel` 的右键菜单（`:106-116`）：
    /// 复制值 / 复制全部 / 分隔 / 与工具条同款动作（本仓只摆「恢复默认统计项」，
    /// 添加在工具条 ＋ 里、分组聚合不适用，见 §8.87）。
    fn stat_row(
        &self,
        item: StatsItem,
        value: &str,
        copy_all: String,
        cx: &App,
    ) -> AnyElement {
        let muted = cx.theme().colors.muted_foreground;
        let queue = self.queue.clone();
        let row_id = format!("editor-result-stat-row-{:?}", item);
        let remove_id = format!("editor-result-stat-remove-{:?}", item);
        // 菜单里的两份文本在渲染期定稿：复制值 = 本行值，复制全部 = 上面的整表快照
        let copy_value = value.to_string();
        let reset_queue = self.queue.clone();
        div()
            .id(("editor-result-stat-menu", item as usize))
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .h_5()
            .text_xs()
            .debug_selector(move || row_id.clone())
            .context_menu(move |mut menu: PopupMenu, _window, _cx| {
                // 空文本不写剪贴板（DBeaver `CopyAction` 靠 SWT 自己早退，本仓显式判空）
                let one = copy_value.clone();
                menu = menu.item(PopupMenuItem::new("复制值").on_click(
                    move |_, _window, app| {
                        if one.is_empty() {
                            return;
                        }
                        app.write_to_clipboard(ClipboardItem::new_string(one.clone()));
                    },
                ));
                let all = copy_all.clone();
                menu = menu.item(PopupMenuItem::new("复制全部").on_click(
                    move |_, _window, app| {
                        if all.is_empty() {
                            return;
                        }
                        app.write_to_clipboard(ClipboardItem::new_string(all.clone()));
                    },
                ));
                let reset_queue = reset_queue.clone();
                menu.separator().item(
                    PopupMenuItem::new("恢复默认统计项（默认只有「非空」）").on_click(
                        move |_, _window, app| {
                            reset_queue.request(ResultRequest::ResetStatsItems, app);
                        },
                    ),
                )
            })
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
                    .debug_selector(move || remove_id.clone())
                    .icon(IconName::X)
                    .tooltip(format!("停用「{}」", item.label()))
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::RemoveStatsItem(item), app);
                    }),
            )
            .into_any_element()
    }

    /// 【右栏】分组页（DBeaver「分组」）：按一组列本地 groupby 计数。
    ///
    /// 【R3】分组键列 = 工具条下拉里勾选的列集合（空 = 网格选中格优先、清单选中列兜底）；
    /// 支持多列组合、排序口径（按值 / 按计数）与「仅重复」筛选。大表口径是下发
    /// GROUP BY 重查（见 §8.8），本地页只服务中小结果集的即时分组。
    pub(crate) fn render_group_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();

        // 【R3】分组键列：显式选择的列集合优先（顺序即拼接顺序）；集合空 = 退回当前选中列
        //
        // 【2026-11-17 对齐】没有分组键时不再整页早退回空态——页内工具条（尤其分组列
        // 下拉）必须常驻，否则这一页在「还没选列」时连选列的入口都没有了。
        // 空态只替换正文，与 DBeaver 的 `GroupingPanel` 常驻工具条同一口径
        // （`GroupingPanel.fillToolBar:164-176`）。
        let cols: Vec<usize> = if self.group_columns.is_empty() {
            self.inspect_column(delegate).into_iter().collect()
        } else {
            self.group_columns
                .iter()
                .copied()
                .filter(|column| *column < delegate.columns().len())
                .collect()
        };
        let mut groups = if cols.is_empty() {
            Vec::new()
        } else {
            delegate.group_by_columns_for(&cols)
        };
        arrange_groups(&mut groups, self.group_sort, self.group_duplicates_only);
        let names: Vec<String> = cols
            .iter()
            .map(|column| {
                delegate
                    .columns()
                    .get(*column)
                    .cloned()
                    .unwrap_or_else(|| "列".to_string())
            })
            .collect();
        let title = if cols.is_empty() {
            "分组".to_string()
        } else {
            format!("按 {} 分组 · {} 组", names.join(" + "), groups.len())
        };

        // 动作一：分组列下拉（勾选式多选；隐藏列不进分组键）
        let all_names = delegate.columns().to_vec();
        let hidden: Vec<bool> = (0..all_names.len())
            .map(|column| delegate.is_hidden_column(column))
            .collect();
        let columns_menu = {
            let queue = self.queue.clone();
            let selected = cols.clone();
            DropdownButton::new("editor-result-group-columns")
                .xsmall()
                .button(
                    Button::new("editor-result-group-columns-btn")
                        .ghost()
                        .xsmall()
                        .debug_selector(|| "editor-result-group-columns".to_string())
                        .icon(IconName::Table2)
                        .label("列"),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    for (column, name) in all_names.iter().enumerate() {
                        if hidden.get(column).copied().unwrap_or(false) {
                            continue;
                        }
                        let queue = queue.clone();
                        let checked = selected.contains(&column);
                        menu = menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(checked)
                                .on_click(move |_, _window, app| {
                                    queue.request(ResultRequest::ToggleGroupColumn(column), app);
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        };
        // 动作二：排序口径 toggle（按值 / 按计数）
        let sort_button = {
            let queue = self.queue.clone();
            let next = self.group_sort.toggled();
            Button::new("editor-result-group-sort")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-group-sort".to_string())
                .label(self.group_sort.label())
                .tooltip("切换分组结果的排序口径".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SetGroupSort(next), app);
                })
                .into_any_element()
        };
        // 动作三：仅重复（只留行数 > 1 的分组）
        let duplicates_on = self.group_duplicates_only;
        let duplicates_button = {
            let queue = self.queue.clone();
            Button::new("editor-result-group-dup")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-group-dup".to_string())
                .label("仅重复")
                .toggled(duplicates_on)
                // DBeaver `controls_resultset_grouping_show_duplicates_only`
                // = "Show duplicates only (COUNT > 1)"
                .tooltip("仅显示重复分组（COUNT > 1）".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SetGroupDuplicatesOnly(!duplicates_on), app);
                })
                .into_any_element()
        };

        // 动作四：移除分组列（对齐 DBeaver `DeleteColumnAction` +
        // `controls_resultset_grouping_remove_column`：作用于**当前选中列**，
        // 选中列不在分组键里 / 只剩一列时不可用）
        let remove_button = {
            let queue = self.queue.clone();
            let enabled = can_remove_group_column(&self.group_columns, self.selected_col);
            let column = self.selected_col.unwrap_or(0);
            Button::new("editor-result-group-remove")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-group-remove".to_string())
                .icon(IconName::CircleMinus)
                .disabled(!enabled)
                .tooltip("移除分组列".to_string())
                .on_click(move |_, _window, app| {
                    // 复用勾选语义：已在集合里 → 移除（与下拉取消勾选同一路径）
                    queue.request(ResultRequest::ToggleGroupColumn(column), app);
                })
                .into_any_element()
        };

        // 动作五：清空分组列（对齐 DBeaver `ClearGroupingAction`：图标动作，
        // 只在有显式分组列时可用；清空后回到「选中列兜底」）
        let clear_button = {
            let queue = self.queue.clone();
            let enabled = !self.group_columns.is_empty();
            Button::new("editor-result-group-clear")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-group-clear".to_string())
                .icon(IconName::CircleX)
                .disabled(!enabled)
                .tooltip("清空分组列".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ClearGroupColumns, app);
                })
                .into_any_element()
        };

        let rows_el: Vec<AnyElement> = groups
            .iter()
            .map(|(values, count)| {
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
                            .child(SharedString::from(values.join(" | "))),
                    )
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .into_any_element()
            })
            .collect();
        // 正文：有分组键时是分组行清单，否则是「还没选列」空态（工具条常驻，见上）
        let body: AnyElement = if cols.is_empty() {
            self.sidebar_empty("选中一个数据列查看分组", cx)
        } else {
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .children(rows_el)
                .into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-group-tab".to_string())
            .child(self.panel_toolbar(
                "editor-result-group-toolbar",
                SharedString::from(title),
                // 顺序与分隔照 `GroupingPanel.fillToolBar:164-176`：
                // `DefaultSorting` → `DuplicatesOnly` → SEP → `EditColumns` → `DeleteColumn`
                // → SEP → `ClearGrouping` → SEP → 扩展动作（本仓无扩展点，不摆）。
                // 「编辑列」在那边是 `GroupingConfigDialog`（多列编辑表格），本仓用勾选式下拉
                // 承载同一件事（列清单 + 勾选状态，能力等价、形态更轻）。
                vec![
                    sort_button,
                    duplicates_button,
                    Separator::vertical().into_any_element(),
                    columns_menu,
                    remove_button,
                    Separator::vertical().into_any_element(),
                    clear_button,
                ],
                cx,
            ))
            .child(body)
            .into_any_element()
    }

    /// 【引用】外键引用查看页签（对齐 DBeaver `ReferencesPanel` + `ReferencesResultsContainer`）
    ///
    /// 形：28px 页内工具条（标题「引用」，`contributeActions` 那边是空实现 ⇒ 本页也无动作）
    /// + 正文。正文按清单状态分四态：
    /// - 未读 / 读取中：空态文案（取数由宿主在切页签时分派，见 `ResultPanel::begin_references_load`）；
    /// - 读失败（宿主自身原因，如未绑定连接）：显示原因；
    /// - 读到但为空：「无引用」（照 DBeaver `ResultSetMessages.refs_no_refs_text`）；
    /// - 读到非空：**只读键下拉**（DBeaver `CSmartCombo`）+「打开目标对象」+ 选中键的列映射。
    ///
    /// 与 DBeaver 的**已知差异**：那边下拉下面嵌一整份 `ResultSetViewer` 显示引用行，
    /// 本仓首版只到「键 + 列映射」（引用行按 `result-panel-references-plan.md` 的
    /// `B′` 方案另开结果集，尚未接线）。
    pub(crate) fn render_reference_tab(&self, cx: &App) -> AnyElement {
        let body: AnyElement = match &self.references {
            crate::references::ReferencesState::NotLoaded => {
                self.sidebar_empty("切到本页时读取外键引用", cx)
            }
            crate::references::ReferencesState::Loading => {
                self.sidebar_empty(crate::references::ReferencesState::LOADING, cx)
            }
            crate::references::ReferencesState::Failed(message) => {
                self.sidebar_empty(message, cx)
            }
            crate::references::ReferencesState::Loaded(keys) if keys.is_empty() => {
                self.sidebar_empty(crate::references::ReferencesState::NO_REFERENCES, cx)
            }
            crate::references::ReferencesState::Loaded(keys) => {
                self.render_reference_keys(keys, cx)
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-reference-tab".to_string())
            .child(self.panel_toolbar(
                "editor-result-reference-toolbar",
                SharedString::from("引用"),
                Vec::new(),
                cx,
            ))
            .child(body)
            .into_any_element()
    }

    /// 【引用页】有键时的正文：只读键下拉 + 「打开目标对象」+ 选中键的列映射。
    ///
    /// 下拉用 kit `DropdownButton` 承载只读选择器（DBeaver 用 `SWT.READ_ONLY` 的
    /// `CSmartCombo`；本仓没有独立 combo 组件，用「按钮 + 勾选式菜单」表达同一件事：
    /// 当前项打勾、选另一项即切换）。
    fn render_reference_keys(
        &self,
        keys: &[crate::references::ReferenceKey],
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let active_index = self.active_reference.filter(|index| *index < keys.len());
        let active = active_index.and_then(|index| keys.get(index));
        let queue = self.queue.clone();

        let label: SharedString = match active {
            Some(key) => SharedString::from(crate::references::key_label(key)),
            None => SharedString::from(crate::references::ReferencesState::NO_REFERENCES),
        };
        let items: Vec<(usize, String, bool)> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| {
                (
                    index,
                    crate::references::key_label(key),
                    active_index == Some(index),
                )
            })
            .collect();
        let chooser = DropdownButton::new("editor-result-reference-keys-anchor")
            .button(
                // 没有该页签该有的宽度弹性：长约束名要能截断，否则会把「打开目标对象」挤出去
                Button::new("editor-result-reference-keys-button")
                    .ghost()
                    .xsmall()
                    .label(label)
                    .tooltip("选择引用键（本表外键）".to_string())
                    .debug_selector(|| "editor-result-reference-keys".to_string()),
            )
            .menu_accessibility_label("引用键")
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                let mut menu = menu;
                for (index, text, checked) in items.iter() {
                    let queue = queue.clone();
                    let index = *index;
                    let checked = *checked;
                    let text = text.clone();
                    menu = menu.item(
                        PopupMenuItem::new(text)
                            .checked(checked)
                            .on_click(move |_, _window, app| {
                                queue.request(
                                    ResultRequest::SetActiveReference(Some(index)),
                                    app,
                                );
                            }),
                    );
                }
                menu
            });

        // 「查看引用行」：按当前行的外键值查目标表，交给编辑器**另开一份结果集**
        // （方案 B′；DBeaver 是在本面板内嵌一份结果视图，见 result-panel-references-plan.md）。
        // 取不到「选中键 + 当前行 + 可见列有值」就不摆这张票——不生成半条 SQL。
        let rows_sql = self.reference_rows_sql(cx);
        let rows_queue = self.queue.clone();
        let rows = Button::new("editor-result-reference-rows")
            .ghost()
            .xsmall()
            .icon(IconName::Rows3)
            .label("查看引用行")
            .tooltip("按当前行的外键值查询目标表（另开一份结果集）".to_string())
            .debug_selector(|| "editor-result-reference-rows".to_string())
            .disabled(rows_sql.is_none())
            .on_click(move |_, _window, app| {
                let Some(sql) = rows_sql.clone() else {
                    return;
                };
                rows_queue.request(ResultRequest::OpenReferenceRows { sql }, app);
            });

        // 「打开目标对象」：目标表在当前连接里定位（DBeaver `refs_open_target`）
        let schema = self
            .reference_source(cx)
            .map(|(schema, _, _)| schema)
            .unwrap_or_default();
        let target = active.map(|key| key.table.clone());
        let open_queue = self.queue.clone();
        let open = Button::new("editor-result-reference-open")
            .ghost()
            .xsmall()
            .icon(IconName::Table)
            .label("打开目标对象")
            .tooltip("在导航树里定位该外键的目标表".to_string())
            .debug_selector(|| "editor-result-reference-open".to_string())
            .disabled(target.is_none())
            .on_click(move |_, _window, app| {
                let Some(table) = target.clone() else {
                    return;
                };
                open_queue.request(
                    ResultRequest::OpenReferenceTarget {
                        schema: schema.clone(),
                        table,
                    },
                    app,
                );
            });

        let mapping: AnyElement = match active {
            Some(key) => div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(SharedString::from(crate::references::mapping_text(key)))
                .into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b(ui::HAIRLINE)
                    .child(div().flex_1().min_w_0().text_ellipsis().child(chooser))
                    .child(rows)
                    .child(open),
            )
            .child(div().px_2().py_1().child(mapping))
            .into_any_element()
    }

    /// 【1.2】「执行SQL」页签：筛选框运行后生成的 wrapped SQL 原文（只读 + 复制）。
    ///
    /// 【2026-11-17 对齐】空态也保留页内工具条（此前空态直接返回居中文字，
    /// 切页时正文起点比其它页高 28px），只是没有可复制的文本时不摆复制钮。
    pub(crate) fn render_exec_sql_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        // 【R1】页内工具条：左「生成的 SQL」、右复制（原先是在 `p_2` 里自绘的一行标题）
        let copy: Option<AnyElement> = self.exec_sql.as_ref().map(|_| {
            let copy_queue = self.queue.clone();
            Button::new("editor-result-exec-sql-copy")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-exec-sql-copy".to_string())
                .icon(IconName::Copy)
                .label("复制")
                .tooltip("复制生成的 SQL".to_string())
                .on_click(move |_, _window, app| {
                    copy_queue.request(ResultRequest::CopyExecSql, app);
                })
                .into_any_element()
        });
        let body: AnyElement = match self.exec_sql.clone() {
            Some(sql) => div()
                .flex_1()
                .min_h_0()
                .m_2()
                .overflow_y_scrollbar()
                .bg(theme.colors.background)
                .rounded_md()
                .p_2()
                .text_xs()
                .text_color(theme.colors.foreground)
                .child(SharedString::from(sql))
                .into_any_element(),
            None => {
                self.sidebar_empty("在工具栏筛选框输入词并点 ▶ 运行，生成 SQL 会显示在这里", cx)
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-exec-sql-tab".to_string())
            .child(self.panel_toolbar(
                "editor-result-exec-sql-toolbar",
                SharedString::from("生成的 SQL"),
                copy.into_iter().collect(),
                cx,
            ))
            .child(body)
            .into_any_element()
    }

    /// 【侧栏】「28px 页内工具条 + 空态正文」的整页骨架。
    ///
    /// 右栏六页统一形态：页签条 24px + 页内工具条 28px + 正文吃满剩余高。
    /// 空态也保留工具条——否则切页时正文起点会比其它页高 28px（2026-11-17 对齐）。
    fn empty_page(
        &self,
        toolbar_id: &'static str,
        title: &str,
        body: &str,
        cx: &App,
    ) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .child(self.panel_toolbar(
                toolbar_id,
                SharedString::from(title),
                Vec::new(),
                cx,
            ))
            .child(self.sidebar_empty(body, cx))
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

/// 【值页动作可用性】「保存值到文件」什么时候可用：DBeaver 的 `saveToFile` 是
/// **条件贡献**的（`ContentValueManager.contributeContentActions`：`if (!content.isNull())`
/// 才加，`:92-99`），NULL 值不给保存入口。本仓再加一条：`#` 行号槽不是数据格
/// （`row_cell_text` 对它返回 `None`），显示的数字是视图行号，也按「没有内容」处理。
///
/// 判据统一走 [`crate::export::is_null_cell`]（`None` / 字面 `NULL` 都算），
/// 不另造一套"是不是空值"的规则。
pub(crate) fn value_can_save_to_file(col_ix: usize, raw: Option<&str>) -> bool {
    col_ix != 0 && !crate::export::is_null_cell(raw)
}

/// 【R3 分组页】分组结果排序 + 「仅重复」筛选（纯函数，便于单测）。
///
/// 先过滤再排序（避免为要丢掉的组白排）；`CountDesc` 同数按值序兜底，
/// 保证同一份数据每次渲染顺序一致（否则 UI 会随统计重算跳动）。
pub(crate) fn arrange_groups(
    groups: &mut Vec<(Vec<String>, usize)>,
    sort: GroupSort,
    duplicates_only: bool,
) {
    if duplicates_only {
        groups.retain(|(_, count)| *count > 1);
    }
    match sort {
        GroupSort::Value => groups.sort_by(|a, b| a.0.cmp(&b.0)),
        GroupSort::CountDesc => groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))),
    }
}

/// 【右栏】页签下标 → 枚举（TabBar 回调用；与 `RightTab` 定义顺序一致——
/// Browser=0 / Value=1 / Stats=2 / Group=3 / Reference=4 / ExecSql=5，
/// 对齐 DBeaver 右侧页签顺序）
///
/// 【2026-11-18】顺序与映射已收成单一来源 `events::right_tabs`（页签条 / 视图菜单 /
/// 竖条共用），本文件不再各写一份 match。

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
