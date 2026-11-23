//! 页签域（`panel/` 第六块）：值详情 / 统计 / 分组 / 引用 / 执行 SQL 各右栏页签。

use shared::i18n::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::Selectable as _;
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
use super::events::{
    page_needs_separator, panel_view_item_label, panel_view_menu_items, PanelViewItem,
};
use crate::grid::cell::{
    HEX_DUMP_BYTES_PER_LINE, ViewerMode, classify_value, hex_lines, hex_to_bytes,
};
use crate::grid::stats::{ColumnStats, distinct_count, fmt_grouped, median_of, mode_of, text_minmax};
use crate::grid::{self as result_grid};
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

/// 【分组页】分组键内某列能否上 / 下移一位（纯函数）。
///
/// 对齐 DBeaver `GroupingColumnsContainer.moveColumns:126-152`：分组键（属性列）顺序
/// 就是分组嵌套顺序，移动会改层级。本仓用「上移 / 下移一位」表达同一动作
/// （键盘可达、headless 可验；DBeaver 是拖到目标列的拖放）。
/// 可用条件 = 选中列**在分组键里**且移动后仍在界内。
pub(crate) fn can_move_group_column(
    group_columns: &[usize],
    selected: Option<usize>,
    delta: i8,
) -> bool {
    let Some(selected) = selected else {
        return false;
    };
    let Some(index) = group_columns.iter().position(|column| *column == selected) else {
        return false;
    };
    let target = index as i64 + delta as i64;
    target >= 0 && (target as usize) < group_columns.len()
}

/// 【分组页】把分组键里某列上 / 下移一位；返回是否真的动了
/// （列不在分组键里 / 已经到边界 = `false`，调用方据此决定要不要通知）。
///
/// 这一层只动 `Vec<usize>` 顺序：分组行的键值排列由 `arrange_groups` 现算，
/// 顺序一变层级就跟着变，不需要在别处补状态。
pub(crate) fn move_group_column_in_place(
    group_columns: &mut [usize],
    column: usize,
    delta: i8,
) -> bool {
    let Some(index) = group_columns.iter().position(|existing| *existing == column) else {
        return false;
    };
    let target = index as i64 + delta as i64;
    if target < 0 || (target as usize) >= group_columns.len() {
        return false;
    }
    group_columns.swap(index, target as usize);
    true
}

/// 【分组页】列下拉里某一项要显示的文字：**已选中的列带上它在分组键里的位次**（1 起）。
///
/// DBeaver 的分组列顺序在 `GroupingConfigDialog` 的列表格里直接可见（表格行序 = 分组顺序）；
/// 本仓用勾选式下拉承载同一件事，若不标位次，用户从菜单上看不出「先按 A 再按 B」，
/// 只能靠猜。位次只加在已勾选项上，未勾选项保持列名原文。
pub(crate) fn group_column_menu_label(
    group_columns: &[usize],
    column: usize,
    name: &str,
) -> String {
    match group_columns.iter().position(|existing| *existing == column) {
        Some(index) => format!("{}. {}", index + 1, name),
        None => name.to_string(),
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

/// 【计算页】右键菜单条目（`PopupMenu` 内部不可读，所以清单抽成纯函数 + 单测）。
///
/// 对齐 `AggregateColumnsPanel` 的右键菜单（`:106-116`）：
/// `CopyAction` → `CopyAllAction` → `Separator` → `fillToolBar` 那一组。
/// `fillToolBar:322-331` 的顺序是 Add / Remove / Reset / SEP / GroupBy / ValueType，
/// 本仓差异（都属能力缺口，不是漏做）：Remove 换成了行尾 ×（本仓无树行选中模型，
/// §8.83）、GroupBy 不做（本仓无组内聚合列，§8.87）。
pub(crate) enum StatMenuEntry {
    CopyValue,
    CopyAll,
    Separator,
    /// 「添加」= 只列**尚未启用**的函数（`AddFunctionAction.createMenuManager:372-386`）
    AddMissing(StatsItem),
    Reset,
    /// 口径切换：菜单项报的是**点它会切到哪一档**（与工具条按钮报当前档相反）
    ToggleMode(StatsMode),
}

/// 右键菜单条目清单（顺序照 `menuMgr.addMenuListener` + `fillToolBar`）。
///
/// 缺项为空时不出「添加」那一段（DBeaver 此时菜单为空、点了没反应）；为了让分隔线
/// 不留悬空的两条，缺项为空就整段跳过（不渲染 Separator）。
pub(crate) fn stat_menu_entries(enabled: &[StatsItem], mode: StatsMode) -> Vec<StatMenuEntry> {
    let mut entries = vec![
        StatMenuEntry::CopyValue,
        StatMenuEntry::CopyAll,
        StatMenuEntry::Separator,
    ];
    let missing: Vec<StatMenuEntry> = StatsItem::ALL
        .into_iter()
        .filter(|item| !enabled.contains(item))
        .map(StatMenuEntry::AddMissing)
        .collect();
    if !missing.is_empty() {
        entries.extend(missing);
        entries.push(StatMenuEntry::Separator);
    }
    entries.push(StatMenuEntry::Reset);
    entries.push(StatMenuEntry::Separator);
    entries.push(StatMenuEntry::ToggleMode(match mode {
        StatsMode::Numeric => StatsMode::Text,
        StatsMode::Text => StatsMode::Numeric,
    }));
    entries
}

/// 菜单条目文案（纯函数，测试直接钉它）：口径项报「切到哪一档」，其余与工具条同款。
pub(crate) fn stat_menu_label(entry: &StatMenuEntry) -> String {
    match entry {
        StatMenuEntry::CopyValue => "复制值".to_string(),
        StatMenuEntry::CopyAll => "复制全部".to_string(),
        StatMenuEntry::Separator => String::new(),
        StatMenuEntry::AddMissing(item) => format!("添加「{}」", item.label()),
        StatMenuEntry::Reset => "恢复默认统计项（默认只有「非空」）".to_string(),
        StatMenuEntry::ToggleMode(StatsMode::Text) => "改为按字符串聚合".to_string(),
        StatMenuEntry::ToggleMode(StatsMode::Numeric) => "改为按数值聚合".to_string(),
    }
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
            // 空态也保留页内工具条（五页同形，见 `empty_page`）；
            // 值页正文是**两行**（DBeaver `ValueViewerPanel` 的空态形态，见 [`value_empty_lines`]）
            return self.empty_page_lines(
                RightTab::Value,
                "editor-result-value-toolbar",
                "值",
                &value_empty_lines(),
                cx,
            );
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
                .tooltip(tr("复制当前单元格的值"))
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
        // 【在外部程序中打开】对齐 DBeaver `ContentValueManager` 的
        // 「Open in external editor」（`:100-140`，与「保存至文件」同一处无条件贡献）：
        // 落一份临时文件后交系统默认程序打开。零内容格与保存钮同口径置灰。
        let open_external = {
            let queue = self.queue.clone();
            Button::new("editor-result-value-open-external")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-value-open-external".to_string())
                .icon(IconName::ExternalLink)
                .disabled(!savable)
                .tooltip(if savable {
                    "用系统默认程序打开当前值（先落一份临时文件）"
                } else {
                    "该格没有内容可打开（NULL / 行号槽）"
                })
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::OpenValueExternally, app);
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
                // 顺序照 `ValueViewerPanel.fillToolBar:409-428`：内容类动作（保存至文件 /
                // 在外部程序打开）在前，写回类 SaveValueAction 在后（本仓无写回）。
                vec![marker, copy, save, open_external],
                // 分隔线：全仓只有值详情页声明 `needsSeparator() = true`
                // （`ValueViewerPanel.java:171-173`）。
                true,
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
            // 空态也保留页内工具条（五页同形，见 `empty_page`）
            return self.empty_page(
                RightTab::Stats,
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
        // 整列取值：字符串口径的极值要它，去重计数 / 中位数 / 众数也要它
        // （数值口径的 min/max/avg/sum 走事实档列式统计，不必物化矩阵）
        let need_values = matches!(self.stats_mode, StatsMode::Text)
            || self.stats_items.iter().any(|item| item.needs_column_values());
        let values = need_values.then(|| delegate.column_values(data_col));
        let text_extremes = matches!(self.stats_mode, StatsMode::Text)
            .then(|| text_minmax(values.as_deref().unwrap_or(&[])));
        // 【函数=值】启用项 + 当前值先算成一对表：行渲染与「复制全部」共用同一份快照
        // （复制全部必须拷「此刻看到的这些」，不能在菜单回调里再算一遍）
        let extras = StatExtras {
            text_extremes: text_extremes.as_ref(),
            distinct: values.as_deref().map(distinct_count),
            median: values.as_deref().and_then(median_of),
            mode: values.as_deref().and_then(mode_of),
        };
        let entries: Vec<(StatsItem, String)> = self
            .stats_items
            .iter()
            .map(|item| {
                let value = stat_value(*item, &stats, &extras, self.stats_mode);
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
    ///
    /// **列位照 `addPanel:1337-1383`**：那边是 `GridLayout(2, false)` 的两列——左列
    /// （`panel.contributeActions(...).createControl(trControl)` + `FILL_HORIZONTAL`）吃满剩余宽度，
    /// 页内动作因此**贴左**；右列（`addDefaultPanelActions` 的视图菜单）`HORIZONTAL_ALIGN_END`，
    /// 贴右。本仓照同一列位：左 = 标题 + 动作组，右 = 视图菜单。
    /// 标题是本仓额外保留的（DBeaver 页名只在页签上；统计页标题带列名，仍有用），
    /// 登记为「保留的有意差异」。
    ///
    /// **分隔线照 `panel.needsSeparator()`**：`ResultSetViewer.java:1345-1347` 只在面板
    /// 声明 `needsSeparator()` 时才在工具条与正文之间插一条水平分隔；全仓只有
    /// `ValueViewerPanel:171-173` 返回 `true`，其余继承 `ResultSetPanelBase:26-28` 的 `false`。
    /// 所以这里的分隔线由调用方按页传入，不再每页无条件画。
    /// **动作只给本仓已有能力**——不造没有底层实现的按钮：DBeaver 的引用页
    /// `contributeActions` 是空实现（`ReferencesPanel.java:156-158`），该页保持空态。
    pub(crate) fn panel_toolbar(
        &self,
        debug_id: &'static str,
        title: SharedString,
        actions: Vec<AnyElement>,
        separator: bool,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut row = div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_1()
            .px_2()
            .h(rems(ui::RESULT_PANEL_TOOLBAR_HEIGHT))
            .flex_none()
            .text_xs()
            .debug_selector(move || debug_id.to_string());
        if separator {
            row = row
                .border_b(ui::HAIRLINE)
                .border_color(theme.colors.border);
        }
        row.child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .max_w(rems(14.))
                        .truncate()
                        .text_color(theme.colors.muted_foreground)
                        .child(title),
                )
                .child(
                    Toolbar::new(format!("{debug_id}-actions"))
                        .xsmall()
                        .p_0()
                        .contents(actions),
                ),
        )
        .child(self.panel_view_menu(debug_id, cx))
        .into_any_element()
    }

    /// 【右栏视图菜单】每页条右端那颗「视图菜单」（对齐 DBeaver `addDefaultPanelActions`，
    /// `ResultSetViewer.java:1590-1614`：那边 `addPanel` 给**每个面板**的面板工具条右端都挂
    /// 一颗 `IMG_LCL_VIEW_MENU` 按钮，点开是 `fillPanelsMenu()`（`:1559-1588`）的菜单）。
    ///
    /// 菜单 = 五页勾选项（`STYLE_CHECK`，当前页为勾选态）→ 分隔 → 面板最大化 → 分隔 →
    /// 面板总开关 → 布局方向（段位照 `fillPanelsMenu:1559-1588`；`CMD_ACTIVATE_PANELS`
    /// 本仓无端口、按「能力没有就不摆」不列）。条目清单与文案分别由纯函数
    /// `panel_view_menu_items` / `panel_view_item_label` 定
    /// （`PopupMenu` 内部不可读，测试钉在纯函数上）。
    fn panel_view_menu(&self, debug_id: &'static str, cx: &App) -> AnyElement {
        let _ = cx;
        let queue = self.queue.clone();
        let open_tabs: Vec<RightTab> = self.open_tabs.clone();
        let panels_visible = self.panels_visible();
        let vertical_layout = self.vertical_layout;
        let panel_maximized = self.panel_maximized;
        let items = panel_view_menu_items();
        let menu_id = format!("{debug_id}-view-menu");
        let button = Button::new(menu_id.clone())
            .ghost()
            .xsmall()
            .icon(IconName::Menu)
            .tooltip(tr("视图菜单（切换面板 / 显示与布局）"))
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
                            // 【勾选态 = 已启用 / 点击 = 开或关】对齐 DBeaver
                            // `fillPanelsMenu:1559-1588` + `ResultSetHandlerTogglePanel`：
                            // 每页是 `STYLE_CHECK` 的 `CMD_TOGGLE_PANEL`，勾 =
                            // `isPanelVisible(id)`（`updateElement:99`）；点击走
                            // `toggleResultsPanel:64-69` → 「已可见 → `closePanel`（关掉该页）；
                            // 其余 → `activatePanel(id,true,true)`（打开并切过去）」。
                            // 所以「已启用且整栏可见」= 关闭，其余一律 = 打开。
                            let is_open = open_tabs.contains(&tab);
                            // 判定抽成纯函数（`PopupMenu` 内部不可读，菜单用例只能钉纯函数）
                            let closes = super::events::page_item_closes(panels_visible, is_open);
                            let target = if closes {
                                ResultRequest::CloseRightTab(tab)
                            } else {
                                ResultRequest::OpenRightTab(tab)
                            };
                            menu = menu.item(
                                PopupMenuItem::new(panel_view_item_label(
                                    item,
                                    panels_visible,
                                    vertical_layout,
                                    panel_maximized,
                                ))
                                .checked(is_open)
                                .on_click(move |_, _window, app| {
                                    queue.request(target.clone(), app);
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
                .tooltip(tr("九项统计都已启用"))
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
                        .tooltip(tr("添加统计项")),
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
                .tooltip(tr("恢复默认统计项（对齐 DBeaver 默认集：非空 + 去重计数）"))
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
                .selected(text_mode)
                .toggled(text_mode)
                .tooltip(tr("聚合口径：数值时最小/最大值按数值比较；字符串时按字典序（文本列也给极值）"))
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
            page_needs_separator(RightTab::Stats),
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
        let menu_queue = self.queue.clone();
        // 菜单条目按「此刻启用的集合 + 当前口径」定稿（与 `stat_menu_entries` 纯函数同一口径）
        let menu_entries = stat_menu_entries(&self.stats_items, self.stats_mode);
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
                for entry in menu_entries.iter() {
                    if matches!(entry, StatMenuEntry::Separator) {
                        menu = menu.separator();
                        continue;
                    }
                    // 空文本不写剪贴板（DBeaver `CopyAction` 靠 SWT 自己早退，本仓显式判空）
                    let label = stat_menu_label(entry);
                    let item = PopupMenuItem::new(label);
                    menu = menu.item(match entry {
                        StatMenuEntry::CopyValue => {
                            let one = copy_value.clone();
                            item.on_click(move |_, _window, app| {
                                if one.is_empty() {
                                    return;
                                }
                                app.write_to_clipboard(ClipboardItem::new_string(one.clone()));
                            })
                        }
                        StatMenuEntry::CopyAll => {
                            let all = copy_all.clone();
                            item.on_click(move |_, _window, app| {
                                if all.is_empty() {
                                    return;
                                }
                                app.write_to_clipboard(ClipboardItem::new_string(all.clone()));
                            })
                        }
                        StatMenuEntry::AddMissing(target) => {
                            let queue = menu_queue.clone();
                            let target = *target;
                            item.on_click(move |_, _window, app| {
                                queue.request(ResultRequest::AddStatsItem(target), app);
                            })
                        }
                        StatMenuEntry::Reset => {
                            let queue = reset_queue.clone();
                            item.on_click(move |_, _window, app| {
                                queue.request(ResultRequest::ResetStatsItems, app);
                            })
                        }
                        StatMenuEntry::ToggleMode(target) => {
                            let queue = menu_queue.clone();
                            let target = *target;
                            item.on_click(move |_, _window, app| {
                                queue.request(ResultRequest::SetStatsMode(target), app);
                            })
                        }
                        StatMenuEntry::Separator => item,
                    });
                }
                menu
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
    /// 【右栏】分组页（DBeaver「分组」）——**页内嵌结果集**。
    ///
    /// 对齐 DBeaver `GroupingResultsContainer.java:61-73`：那一页里嵌着一整个
    /// `ResultSetViewer`（`:64-70` 覆写 `refreshWithFilter` 记 currentFilter），
    /// 分组结果只喂这个内嵌 viewer；主结果集与标签条一概不动。所以这一页的正文
    /// 就是 [`Self::group_grid`]（宿主构造期建好、回执泵按
    /// [`crate::store::ResultPlacement::Grouping`] 写入），本函数只负责画。
    ///
    /// 触发执行的是工具条最右的「重查」：DBeaver 没有这个独立按钮，它的 rebuild 由
    /// 各动作内部触发（`GroupingPanel.refresh(true)` → `rebuildGrouping`，
    /// `GroupingResultsContainer:211-272`）；本仓的执行在宿主侧，面板拿不到执行器，
    /// 只能把意图经队列交出去，所以需要一个显式触发点。
    pub(crate) fn render_group_tab(&self, cx: &App) -> AnyElement {
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();

        // 分组键列：显式选择的列集合优先（顺序即 GROUP BY 列序 = 嵌套层级）；
        // 集合空 = 退回当前选中列（与 DBeaver「无键 = 空态」不同，本仓这一档是
        // 便利默认，清空动作仍照 `ClearGroupingAction` 的可用条件）
        let cols: Vec<usize> = if self.group_columns.is_empty() {
            self.inspect_column(delegate).into_iter().collect()
        } else {
            self.group_columns
                .iter()
                .copied()
                .filter(|column| *column < delegate.columns().len())
                .collect()
        };
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
        let has_keys = !names.is_empty();
        let title = if has_keys {
            format!("按 {} 分组", names.join(" + "))
        } else {
            "分组".to_string()
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
                        .label(tr("列")),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    for (column, name) in all_names.iter().enumerate() {
                        if hidden.get(column).copied().unwrap_or(false) {
                            continue;
                        }
                        let queue = queue.clone();
                        let checked = selected.contains(&column);
                        // 已勾选的项标出分组位次（顺序即层级；见 group_column_menu_label）
                        let label = group_column_menu_label(&selected, column, name);
                        menu = menu.item(
                            PopupMenuItem::new(label)
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
                .tooltip(tr("切换分组结果的排序口径").to_string())
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
                .label(tr("仅重复"))
                .selected(duplicates_on)
                .toggled(duplicates_on)
                // DBeaver `controls_resultset_grouping_show_duplicates_only`
                // = "Show duplicates only (COUNT > 1)"
                .tooltip(tr("仅显示重复分组（COUNT > 1）").to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SetGroupDuplicatesOnly(!duplicates_on), app);
                })
                .into_any_element()
        };

        // 动作四：分组键顺序上移 / 下移（作用于**当前选中列**）。
        // 对齐 DBeaver `GroupingColumnsContainer.moveColumns:126-152`：那边靠拖拽
        // （`GroupingResultsDecorator.dropColumns`），本仓用一对按钮——键盘可达，
        // 且 headless 下能用窗口用例钉住「顺序变 = 层级变」。
        let selected = self.selected_col;
        let [move_up, move_down] = [
            (-1i8, IconName::ChevronUp, "上移分组列", "editor-result-group-move-up"),
            (1i8, IconName::ChevronDown, "下移分组列", "editor-result-group-move-down"),
        ]
        .map(|(delta, icon, tip, id)| {
            let queue = self.queue.clone();
            let enabled = can_move_group_column(&self.group_columns, selected, delta);
            let column = selected.unwrap_or(0);
            Button::new(id)
                .ghost()
                .xsmall()
                .debug_selector(move || id.to_string())
                .icon(icon)
                .disabled(!enabled)
                .tooltip(tip.to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::MoveGroupColumn { column, delta }, app);
                })
                .into_any_element()
        });

        // 动作五：移除分组列（对齐 DBeaver `DeleteColumnAction` +
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
                .tooltip(tr("移除分组列").to_string())
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
                .tooltip(tr("清空分组列").to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ClearGroupColumns, app);
                })
                .into_any_element()
        };

        // 动作：重查（本仓的 rebuild 触发点，见函数注释）
        let run_button = {
            let queue = self.queue.clone();
            let columns = names.clone();
            let duplicates_only = self.group_duplicates_only;
            Button::new("editor-result-group-run")
                .ghost()
                .xsmall()
                .debug_selector(|| "editor-result-group-run".to_string())
                .icon(IconName::Play)
                .disabled(!has_keys)
                .tooltip("按分组列重查（GROUP BY 结果落本页，不动主结果集）".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(
                        ResultRequest::RunGroupingQuery {
                            columns: columns.clone(),
                            duplicates_only,
                        },
                        app,
                    );
                })
                .into_any_element()
        };

        // 正文：页内嵌结果集（对齐 `GroupingResultsContainer` 里那个 `ResultSetViewer`）。
        // 有分组键且有结果行 → 网格；否则空态（工具条常驻，见 DBeaver `fillToolBar`）。
        // 网格选择器换名（`editor-result-group-pane` / `editor-result-group-grid`）：
        // 主结果区同在一个面板里，同名会让 `debug_bounds` 拿到先渲染的那个。
        let has_rows = self
            .group_grid
            .as_ref()
            .is_some_and(|grid| grid.read(cx).delegate().view_total() > 0);
        let body: AnyElement = match (has_keys, self.group_grid.as_ref()) {
            (true, Some(group_grid)) if has_rows => result_grid::render(
                group_grid,
                result_grid::ResultPane {
                    pane_selector: "editor-result-group-pane",
                    grid_selector: "editor-result-group-grid",
                    ..Default::default()
                },
                cx,
            )
            .into_any_element(),
            (true, _) => self.sidebar_empty("点工具条最左的「运行」按分组列重查", cx),
            (false, _) => self.sidebar_empty("选中一个数据列查看分组", cx),
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
                // → SEP → `ClearGrouping` → SEP → 扩展动作（本仓无扩展点）。
                // 前面多一个「运行」：它是本仓执行侧的 rebuild 触发点（见函数注释），
                // 放最左是因为右栏窄、最右的位置会被裁出可视区（点击落不到）。
                // 「编辑列」在那边是 `GroupingConfigDialog`（多列编辑表格），本仓用勾选式下拉
                // 承载同一件事（列清单 + 勾选状态，能力等价、形态更轻）。
                vec![
                    run_button,
                    Separator::vertical().into_any_element(),
                    sort_button,
                    duplicates_button,
                    Separator::vertical().into_any_element(),
                    columns_menu,
                    // 上移 / 下移（本仓对 `moveColumns` 的等价表达，见 can_move_group_column 注释）
                    move_up,
                    move_down,
                    remove_button,
                    Separator::vertical().into_any_element(),
                    clear_button,
                ],
                page_needs_separator(RightTab::Group),
                cx,
            ))
            .child(body)
            .into_any_element()
    }

    /// 【引用】外键引用查看页签 —— **当前口径：置灰占位**（2026-11-24 拍板 Q36-A）。
    ///
    /// 页签位置保留（五页齐全、序号与 DBeaver 一致），正文只摆一句说明；
    /// 原因：DBeaver 这一页的价值在于「下拉下面嵌一整份 `ResultSetViewer` 显示引用行」
    /// （`ReferencesResultsContainer.java`），本仓这一半还没接线，只给键下拉会让用户以为
    /// 引用能力已就绪。原「键下拉 + 打开目标对象 + 列映射」的实现在
    /// [`Self::render_reference_tab_live`] 里原样保留（`references` 状态机、宿主取数、
    /// 单元测试都留着），接线后把这个入口换回去即可，不用重写。
    pub(crate) fn render_reference_tab(&self, cx: &App) -> AnyElement {
        self.empty_page(
            RightTab::Reference,
            "editor-result-reference-toolbar",
            "引用",
            "外键引用页暂未接入：引用行要按 DBeaver 那样内嵌结果集显示，接入前保留页签位置",
            cx,
        )
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
    /// `B′` 方案另开结果集，尚未接线）。**入口当前被上面的占位挡住**（置灰口径）。
    #[allow(dead_code)]
    pub(crate) fn render_reference_tab_live(&self, cx: &App) -> AnyElement {
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
                page_needs_separator(RightTab::Reference),
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
                    .tooltip(tr("选择引用键（本表外键）").to_string())
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
            .label(tr("查看引用行"))
            .tooltip(tr("按当前行的外键值查询目标表（另开一份结果集）").to_string())
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
            .label(tr("打开目标对象"))
            .tooltip(tr("在导航树里定位该外键的目标表").to_string())
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

    /// 【侧栏】「28px 页内工具条 + 空态正文」的整页骨架。
    ///
    /// 右栏五页统一形态：页签条 24px + 页内工具条 28px + 正文吃满剩余高。
    /// 空态也保留工具条——否则切页时正文起点会比其它页高 28px（2026-11-17 对齐）。
    fn empty_page(
        &self,
        tab: RightTab,
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
                page_needs_separator(tab),
                cx,
            ))
            .child(self.sidebar_empty(body, cx))
            .into_any_element()
    }

    /// 【侧栏】右栏各页共用的空态（居中灰字；页内容区 `flex_1`）
    pub(crate) fn sidebar_empty(&self, text: &str, cx: &App) -> AnyElement {
        self.sidebar_empty_lines(&[text], cx)
    }

    /// 【侧栏】多行空态：每行一条灰字（DBeaver 值页空态是**两行**，
    /// 见 `ValueViewerPanel.createContents:89-106`：第一行 `value_viewer_select_view_message`
    /// 画在 y=0，第二行 `value_viewer_hide_panel_message` 画在 y=30）。
    pub(crate) fn sidebar_empty_lines(&self, lines: &[&str], cx: &App) -> AnyElement {
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
            .children(
                lines
                    .iter()
                    .map(|line| SharedString::from(*line))
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }

    /// 【侧栏】「页内工具条 + 多行空态正文」的整页骨架（同 [`Self::empty_page`]，
    /// 只是正文允许多行——值页空态两行要用它）。
    fn empty_page_lines(
        &self,
        tab: RightTab,
        toolbar_id: &'static str,
        title: &str,
        lines: &[&str],
        cx: &App,
    ) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-empty-page".to_string())
            .child(self.panel_toolbar(
                toolbar_id,
                SharedString::from(title),
                Vec::new(),
                page_needs_separator(tab),
                cx,
            ))
            .child(self.sidebar_empty_lines(lines, cx))
            .into_any_element()
    }
}

/// 【值页空态】两行提示（纯函数，便于单测）。
///
/// 形态对齐 DBeaver `ValueViewerPanel.createContents:89-106`：空态在 `PaintListener` 里
/// 画**两条**消息（第一条在 y=0、第二条在 y=30），不是一条长文本。
/// 第一条的语义 = 「先在结果集里选一个值」（`value_viewer_select_view_message`）；
/// 第二条的语义 = 「怎么收起这个面板腾出空间」（`value_viewer_hide_panel_message`，
/// 原文把 toggle-panels **命令名**绑进去，如 `NLS.bind(..., hidePanelCmd)`）。
///
/// **文案是本仓自拟**：dbeaver-ref 快照里的 `bundle.properties` / `bundle_zh.properties`
/// 都没有这两个 key 的正文（全文件仅 12 行含 `viewer`，无一命中 `value_viewer_`），
/// 逐字翻译无从取得，故按语义写、并把第二行指向本仓真实存在的两个入口
/// （右缘竖条、视图菜单的面板项），不编造 DBeaver 的快捷键。
pub(crate) fn value_empty_lines() -> [&'static str; 2] {
    [
        "在网格中选中一个单元格，这里显示它的值",
        "右缘竖条或视图菜单可收起全部面板，腾出更多高度",
    ]
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

/// 【分组页表头】分组结果表的列头：每个分组键一列 + 末尾函数列 `COUNT(*)`。
///
/// 【D14 起不再由分组页调用】分组页正文已换成页内嵌结果集（列头由网格自己画，
/// 列名来自查询列 —— 也就是 SQL 里 `<键…>, COUNT(*) AS count` 那份）。
/// 函数保留给纯函数用例（`arrange_groups` / `group_table_header` 的排序口径仍是
/// 本地即时分组算法的口径说明），并作为「本地分组 = 键列 + 一个计数函数」的
/// 文档化落点；不删是为了不丢用例与口径。
#[allow(dead_code)]
pub(crate) fn group_table_header(names: &[String]) -> Vec<String> {
    let mut header: Vec<String> = names.to_vec();
    header.push("COUNT(*)".to_string());
    header
}

/// 【D14 分组页】对**页内嵌结果集的行**按排序口径客户端排序（纯函数，便于单测）。
///
/// 对齐 DBeaver `GroupingResultsContainer.rebuildGrouping:249-265`：那边取下
/// `RS_GROUPING_DEFAULT_SORTING` 后对分组结果**在 viewer 里排序**（不动 SQL）；
/// 本仓同一性质——行进网格前按末列（函数列 `count`）或键列排，SQL 里不写 ORDER BY。
/// 行形状 = `<分组键…>, <函数列>`，所以末列是计数。
pub(crate) fn arrange_group_rows(rows: &mut Vec<Vec<String>>, sort: GroupSort) {
    let count_of = |row: &Vec<String>| {
        row.last()
            .and_then(|cell| cell.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let keys_of = |row: &Vec<String>| row[..row.len().saturating_sub(1)].to_vec();
    match sort {
        // 不排序 = 数据源给的顺序（DBeaver `RS_GROUPING_DEFAULT_SORTING=""`：不设排序）
        GroupSort::None => {}
        GroupSort::Value => rows.sort_by(|a, b| keys_of(a).cmp(&keys_of(b))),
        GroupSort::CountDesc => rows.sort_by(|a, b| {
            count_of(b)
                .cmp(&count_of(a))
                .then_with(|| keys_of(a).cmp(&keys_of(b)))
        }),
    }
}

/// 【R3 分组页】分组结果排序 + 「仅重复」筛选（纯函数，便于单测）。
///
/// 【D14 起不再由分组页调用】这一页现在执行 `GROUP BY` 重查、结果进页内嵌结果集，
/// 「仅重复」由 SQL 的 `HAVING COUNT(*) > 1` 表达（`rewrite_with_grouping`），
/// 排序由数据源的 `GROUP BY` 顺序表达。函数保留：它仍是本地即时分组算法的口径
/// （见 `group_by_columns_for`）与那批纯函数用例的被测目标。
#[allow(dead_code)]
pub(crate) fn arrange_groups(
    groups: &mut Vec<(Vec<String>, usize)>,
    sort: GroupSort,
    duplicates_only: bool,
) {
    if duplicates_only {
        groups.retain(|(_, count)| *count > 1);
    }
    match sort {
        // 不排序 = 沿用 `group_by_columns_for` 的首现顺序（DBeaver `RS_GROUPING_DEFAULT_SORTING=""`：
        // 不设 ORDER BY，结果就是数据源/扫描顺序）
        GroupSort::None => {}
        GroupSort::Value => groups.sort_by(|a, b| a.0.cmp(&b.0)),
        GroupSort::CountDesc => groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))),
    }
}

/// 【右栏】页签下标 → 枚举（TabBar 回调用；与 `RightTab` 定义顺序一致——
/// Browser=0 / Value=1 / Stats=2 / Group=3 / Reference=4，
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

/// 【R2 计算页】单项取值需要的「整列派生量」：字符串口径极值、去重计数、中位数、众数。
///
/// 为什么打包：这几项都要扫整列取值，而数值口径的 min/max/avg/sum 走事实档列式统计。
/// 打包后 `render_stats_tab` **只取一次值**（`needs_column_values` 为假时连取都不取），
/// 不用为每个新函数各扫一遍。
pub(crate) struct StatExtras<'a> {
    /// 字符串口径的字典序极值（非字符串口径 = `None`）
    pub text_extremes: Option<&'a (Option<String>, Option<String>)>,
    /// 去重计数（列取值未取 = `None` → 显示 `—`）
    pub distinct: Option<usize>,
    /// 数值中位数
    pub median: Option<f64>,
    /// 众数
    pub mode: Option<String>,
}

/// 【R2 计算页】单项取值（纯函数，便于单测）：
/// - 计数类（非空 / 去重计数 / NULL）不受口径影响，走千分位整数
/// - 数值口径：Min/Max/Avg/Sum/Median 走千分位浮点，无样本 = —
/// - 字符串口径：Min/Max 走字典序（文本列也给极值）；众数两种口径都给值文本；
///   Avg/Sum/Median 不出值（DBeaver 的 SUM/AVG 与中位数都要能排序的数字）
pub(crate) fn stat_value(
    item: StatsItem,
    stats: &ColumnStats,
    extras: &StatExtras<'_>,
    mode: StatsMode,
) -> String {
    match item {
        StatsItem::Count => fmt_grouped(stats.count as f64),
        StatsItem::Nulls => fmt_grouped(stats.nulls as f64),
        StatsItem::CountDistinct => extras
            .distinct
            .map(|count| fmt_grouped(count as f64))
            .unwrap_or_else(|| "—".to_string()),
        StatsItem::Min | StatsItem::Max => {
            if matches!(mode, StatsMode::Text) {
                let pick = extras.text_extremes.and_then(|(min, max)| {
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
        StatsItem::Median => {
            if matches!(mode, StatsMode::Text) {
                "—".to_string()
            } else {
                fmt_stat(extras.median)
            }
        }
        StatsItem::Mode => extras
            .mode
            .clone()
            .unwrap_or_else(|| "—".to_string()),
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
