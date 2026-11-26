//! 单元格渲染域（`grid/` 第七块）：类型感知样式（`CellStyle` / `format_cell_text`）+
//! 展示裁剪 / JSON / BLOB 摘要 + `TableDelegate::render_td`。

use gpui_kit::base::TreeItem;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Colorize;
use gpui_kit::component::table::TableState;
use gpui_kit::*;

use std::borrow::Cow;

use super::array_cell::{
    self, ARRAY_COLLAPSED_GLYPH, ARRAY_EXPANDED_GLYPH, ARRAY_GLYPH_SPACING, ARRAY_HINT_SPACING,
    ArrayCellDisplay,
};
use super::delegate::{CELL_DISPLAY_LIMIT, ResultGridDelegate};
use super::status::format_integer_cell;
use super::text::result_cell_tooltip;
use crate::store::ColumnKind;
use gpui_kit::prelude::FluentBuilder as _;
use workbench_shell::product_tokens;
use workbench_shell::result_style::{
    AlignPref, ColorRolePref, NumberFormatPref, TypeFamily, TypeStylePrefs,
};
use workbench_shell::result_style::{BgColorPref, FontStylePref};
/// 一格的渲染决策（**纯函数**产出就是这一份；渲染只负责把它落到元素上）
///
/// 为什么单独拿出来：`Unknown` 这一档必须是**今天的样式**，而“没带类型就一个像素不变”
/// 这句话只有被断言过才算数——渲染函数里测不到（gpui 不暴露算出来的对齐与字体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellStyle {
    /// 文本对齐（数字右对齐：个位对齐才看得出一列的数量级差）
    pub(crate) align: TextAlign,
    /// 用等宽字体吗（布尔 / 时间族；字体族从主题取，不引新依赖）
    pub(crate) monospace: bool,
    /// 按族前景色角色（`Inherit` = 不染色，跟随普通前景；渲染时映射到主题语义色）
    pub(crate) color_role: ColorRolePref,
    /// 按族背景色（`Inherit` = 无背景；渲染时语义角色 → 主题语义色浅底 / Custom → parse_hex）
    pub(crate) bg: BgColorPref,
    /// 按族字体样式（`Inherit` = 跟随类型默认；NULL 斜体等既有语义保持）
    pub(crate) font_style: FontStylePref,
}

/// 列类型 → 类型族（与设置页的五族口径一致；未知类型按文本渲染）
fn type_family(kind: ColumnKind) -> TypeFamily {
    match kind {
        ColumnKind::Integer | ColumnKind::Float | ColumnKind::Decimal => TypeFamily::Numeric,
        ColumnKind::Timestamp | ColumnKind::Date | ColumnKind::Time => TypeFamily::Temporal,
        ColumnKind::Boolean => TypeFamily::Boolean,
        ColumnKind::Binary => TypeFamily::Blob,
        // 数组按文本族：展示是 `{a,b}` 文本（左对齐、非等宽），配色也跟随文本族
        ColumnKind::Array
        | ColumnKind::Text
        | ColumnKind::Json
        | ColumnKind::Uuid
        | ColumnKind::Unknown => TypeFamily::Text,
    }
}

/// 列类型 + 全局偏好 → 这一列所有格子的样式
///
/// 【类型样式】对齐覆盖：`AlignPref::Inherit` = 跟随类型默认（数字右对齐 / 其余左对齐）；
/// 其余显式覆盖。前景色角色直接进 `CellStyle`，渲染时按角色取主题语义色（零裸色值）。
pub(crate) fn cell_style(kind: ColumnKind, prefs: &TypeStylePrefs) -> CellStyle {
    let default_align = if kind.is_numeric() {
        TextAlign::Right
    } else {
        TextAlign::Left
    };
    let align = match prefs.align_for(type_family(kind)) {
        AlignPref::Left => TextAlign::Left,
        AlignPref::Center => TextAlign::Center,
        AlignPref::Right => TextAlign::Right,
        AlignPref::Inherit => default_align,
    };
    CellStyle {
        align,
        monospace: kind.is_monospace(),
        color_role: prefs.color_for(type_family(kind)),
        bg: prefs.bg_for(type_family(kind)).clone(),
        font_style: prefs.font_for(type_family(kind)),
    }
}

/// 【类型样式】单元格显示文本的纯函数：NULL 占位 / 布尔映射 / 数字格式。
///
/// 输入**原始驱动文本**（`text == "NULL"` 判定在这里做，与渲染灰斜体口径同一处）；
/// 只影响显示——复制 / 导出 / 编辑 / 悬停 tooltip 仍用原始值。
/// JSON 美化与 BLOB 摘要在渲染层先于本函数处理，不进这里（职责单一）。
pub fn format_cell_text(kind: ColumnKind, text: &str, prefs: &TypeStylePrefs) -> String {
    if text == "NULL" {
        return prefs.null_placeholder.rendered().to_string();
    }
    if kind == ColumnKind::Boolean {
        let (yes, no) = prefs.bool_display.rendered();
        return match text {
            "true" => yes.to_string(),
            "false" => no.to_string(),
            // 驱动给了其它形态（如 `1`/`0`、大小写变体）：保持原文，不猜映射
            _ => text.to_string(),
        };
    }
    if kind.is_numeric() {
        return match prefs.number_format {
            NumberFormatPref::Raw => text.to_string(),
            // 千分位只对整数值分组（小数 / 异常样本回退原文；与既有 format_integer_cell 同口径）
            NumberFormatPref::Grouped => {
                format_integer_cell(text).unwrap_or_else(|| text.to_string())
            }
            NumberFormatPref::Fixed2 => text
                .parse::<f64>()
                .map(|v| format!("{v:.2}"))
                .unwrap_or_else(|_| text.to_string()),
            NumberFormatPref::Scientific => text
                .parse::<f64>()
                .map(|v| format!("{v:e}"))
                .unwrap_or_else(|_| text.to_string()),
        };
    }
    text.to_string()
}
/// 单元格展示文本字符级裁剪：超过 `limit` 个字符时取前 `limit` 个字符并补省略号 `…`。
///
/// 按 `char` 边界取前若干个，不会裁断中文 / emoji / 组合字符的多字节序列；省略号不占
/// `limit` 额度（与 dbdelve `clip_to` 同口径：300 字符 + 一个尾巴符号，机制借鉴
/// GPL-3.0 dbdelve，仅参考思路未复制代码）。未达上限原样返回。
///
/// **只遍历到第 `limit` 个字符**（`char_indices().nth(limit)`），不先 `chars().count()`
/// 数全串——大值（JSONB / PostGIS 几何，可达数百万字符）每帧渲染成本因此有硬上界，
/// 不随值长线性涨（dbdelve `clip` 同思路：每帧每可见格裁剪，扫描有界）。
pub(crate) fn clip_cell_display(value: &str, limit: usize) -> String {
    match value.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &value[..end]),
        None => value.to_string(),
    }
}

/// 【P1-4】格内 JSON 美化展示：仅当文本以 `{` / `[` 开头且能解析成 JSON 时，
/// 返回缩进格式化串；非 JSON / 解析失败一律 `None`（按普通文本走裁剪，不猜）。
///
/// 只改**展示**：复制 / 悬停 / 右键 / 导出 / 编辑仍取原始 `text`（见 `render_td`）。
/// 裸标量（数字 / 布尔 / 引号串）本来就短，不进这条路径；解析失败不抛错、不 panic。
pub(crate) fn pretty_json_display(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !matches!(trimmed.chars().next(), Some('{') | Some('[')) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// 【dbdelve 借鉴】单元格展示串：短值先 JSON 美化 / 类型格式化（可读性），**超长值
/// 跳过解析、零拷贝借原文**——`serde_json` 全串解析随值长线性涨，大 JSON / 几何文档
/// 每帧重跑成本不可控；机制借鉴 GPL-3.0 dbdelve 的 `CLIP_SCAN_LIMIT` 思路（仅参考
/// 思路未复制代码）。只探测到 `CELL_DISPLAY_LIMIT` 处即知长短，不遍历全串；调用方
/// 随后统一 `clip_cell_display` 收尾（超长分支借出的是原文，裁剪 O(limit) 零拷贝）。
pub(crate) fn cell_display_text<'a>(
    text: &'a str,
    kind: ColumnKind,
    prefs: &TypeStylePrefs,
) -> Cow<'a, str> {
    let short = text.char_indices().nth(CELL_DISPLAY_LIMIT).is_none();
    if short {
        match pretty_json_display(text) {
            Some(pretty) => Cow::Owned(pretty),
            None => Cow::Owned(format_cell_text(kind, text, prefs)),
        }
    } else {
        Cow::Borrowed(text)
    }
}

/// 【P2】BLOB 单元格的摘要文案：`hex → BLOB · N B`
///
/// Arrow 的 display 把二进制序列化成**小写 hex**（每字节两位，见 `arrow-cast` 的
/// `DisplayIndex for GenericBinaryArray`），所以 `len / 2` = 字节数。非偶数 / 含
/// 非 hex 字符的串（驱动 / 上游异常）退回 `None`，按原文本显示，不猜。
pub(crate) fn blob_summary(text: &str) -> Option<String> {
    let hex = text.strip_prefix("0x").unwrap_or(text);
    if hex.len() >= 2 && hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(format!("BLOB · {} B", hex.len() / 2))
    } else {
        None
    }
}
/// 行号槽这一格要不要铺表头底色（纯判据，便于单测）。
///
/// DBeaver 的行头格逐行取 `getHeaderBackground(element, selected)`
/// （`GridRowRenderer.java:55-70`）：选中行取选中色、其余取表头色。本仓的整行选中色
/// 已经铺在**行容器**上（`table_impl.rs::render_tr`，半透明 `table_active`），
/// 所以选中行时行号格必须留空、让行容器的色透上来；再铺一层同色会叠成两倍透明度，
/// 看上去那一小格比数据格深一档。本地新增 / 复制行有身份色，也不叠加表头色。
pub(crate) fn row_number_paints_head(is_local: bool, selected: bool) -> bool {
    !is_local && !selected
}

impl ResultGridDelegate {
    /// 第 `row_ix` 个可见行的行号槽要不要铺表头底色（把行身份与整行选中读进纯判据）。
    pub fn row_number_paints_head(&self, row_ix: usize) -> bool {
        row_number_paints_head(
            self.row_is_local(row_ix),
            self.selected_view_row() == Some(row_ix),
        )
    }

    /// 第 `row_ix` 个可见行的行号要不要加粗：DBeaver 只给当前行换粗体
    /// （`GridRowRenderer.java:96`），本仓「当前行」即整行选中行。
    pub fn row_number_is_bold(&self, row_ix: usize) -> bool {
        self.selected_view_row() == Some(row_ix)
    }
}

/// 单元格渲染（2026-10-29 目录拆分：由 `TableDelegate::render_td` 薄转发至此，
/// Rust trait impl 不可分块）
pub(crate) fn render_td(
    delegate: &mut ResultGridDelegate,
    row_ix: usize,
    col_ix: usize,
    window: &mut Window,
    cx: &mut Context<TableState<ResultGridDelegate>>,
) -> impl IntoElement {
    // 【T2a-4】正在编辑的这一格：画编辑框，不画普通文本（渲染位仍是原生 render_td）
    if delegate.is_editing(row_ix, col_ix) {
        return delegate
            .render_edit_cell(row_ix, col_ix, window, cx)
            .into_any_element();
    }
    let text = delegate.cell(row_ix, col_ix);
    // 【P2-4】转置视图每行对应一个不同的原列，类型按「行」取（字段名列是纯文本）；
    // 普通视图按列取。数值右对齐 / BLOB 摘要 / 等宽因此都随原列走
    let kind = if delegate.transposed {
        match col_ix {
            1 => ColumnKind::Unknown,
            _ => delegate
                .transpose_columns()
                .get(row_ix)
                .and_then(|d| delegate.column_types.get(*d))
                .and_then(Option::as_ref)
                .map_or(ColumnKind::Unknown, |column| column.kind),
        }
    } else {
        delegate.column_kind(col_ix)
    };
    let style = cell_style(kind, &delegate.type_style);
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let foreground = theme.colors.foreground;
    // 字体族从主题取（不引新依赖）：只在真要等宽的那几类上取一次
    let mono_family = style.monospace.then(|| theme.mono_font_family.clone());
    // 【类型样式】按族前景色角色 → 主题语义色（零裸色值；`Inherit` = 不染色）。
    // NULL / 被删行的灰字优先级高于染色（它们是「非数据」语义，先灰再谈颜色）。
    let role_color = match style.color_role {
        ColorRolePref::Info => Some(theme.colors.info),
        ColorRolePref::Success => Some(theme.colors.success),
        ColorRolePref::Warning => Some(theme.colors.warning),
        ColorRolePref::Danger => Some(theme.colors.danger),
        ColorRolePref::Primary => Some(theme.colors.primary),
        ColorRolePref::Muted => Some(theme.colors.muted_foreground),
        ColorRolePref::Inherit => None,
    };
    // 【类型样式】按族背景色：语义角色 → 主题语义色浅底（0.16 透明度，与脏格同口径）；
    // 自定义 hex → `Colorize::parse_hex`（gpui-component 现成 API），非法值回退无背景。
    let bg_color = match &style.bg {
        BgColorPref::Inherit => None,
        BgColorPref::Info => Some(theme.colors.info.opacity(0.16)),
        BgColorPref::Success => Some(theme.colors.success.opacity(0.16)),
        BgColorPref::Warning => Some(theme.colors.warning.opacity(0.16)),
        BgColorPref::Danger => Some(theme.colors.danger.opacity(0.16)),
        BgColorPref::Primary => Some(theme.colors.primary.opacity(0.16)),
        BgColorPref::Muted => Some(theme.colors.muted_foreground.opacity(0.16)),
        BgColorPref::Custom(hex) => Colorize::parse_hex(hex.as_str()).ok(),
    };

    // 行号槽永远是灰的（它不是数据）
    if ResultGridDelegate::is_row_number(col_ix) {
        let is_local = delegate.row_is_local(row_ix);
        let is_deleted = delegate.is_deleted_row(row_ix);
        let local_bg = is_local.then(|| product_tokens::get(cx).result_local_row_background(theme));
        // 【行号槽底色】DBeaver 的每个行头格都铺表头底色（`GridRowRenderer.java:55-70`
        // 逐行 `getHeaderBackground(element, selected)`：选中→选中色，否则表头色），
        // 于是行号列读起来是一条独立的浅色竖条。这里同口径，但**选中行不铺**：
        // 整行底色（`render_tr` 的 `table_active`，半透明）已经在行容器上，行号格再铺
        // 一层同色会叠成两倍透明度、比数据格深一档（判据见 `row_number_paints_head`）。
        let paints_head = delegate.row_number_paints_head(row_ix);
        let head_bg = theme.tokens.table_head;
        // 【获焦行加粗】DBeaver 只给「当前行」的行号换粗体（`GridRowRenderer.java:96`
        // `selected ? boldFont : normalFont`），列头反而恒用 normalFont
        // （`GridColumnRenderer.java:84-87`）。本仓「当前行」= 整行选中行（行模式下
        // `selected_row` 唯一非空），所以判据与铺色共用同一个来源。
        let is_focused_row = delegate.row_number_is_bold(row_ix);
        let table = cx.entity().clone();
        // 【行号槽三角 2026-11-26】真值行头对「有集合元素的行」画展开三角
        // （`GridRowRenderer.java:73-86`）：`state != NONE` 时在 `LEFT_MARGIN` 处画 16×16 图标
        // （`▶` 收起 / `▼` 展开），随后 `x += icon.w + EXPANDER_SPACING(2)` 才是行号数字；
        // 点击落在 `isOverExpander` 区间（`:3117-3122`）→ `toggleRowExpand(row, null)`，
        // 即**整行所有数组列一起展开 / 收起**（`:3279-3283`）。
        // 判定只用数据行（视图行可能是筛选 / 排序后的位序，也可能本身是数组子行）。
        let data_row = match delegate.reference_of(row_ix) {
            Some(crate::rowset::RowRef::Data(data_row)) => Some(data_row),
            _ => None,
        };
        let expander_glyph = data_row.and_then(|data_row| delegate.row_expander_glyph(data_row));
        // 行级展开会插 / 摘子行 → 行数变了，与单元格三角同一条通知路径
        let toggle_row_expand = {
            let table = cx.entity().clone();
            move |_: &MouseDownEvent, window: &mut Window, app: &mut App| {
                let changed = table.update(app, |state, cx| {
                    let delegate = state.delegate_mut();
                    let Some(crate::rowset::RowRef::Data(data_row)) =
                        delegate.reference_of(row_ix)
                    else {
                        return false;
                    };
                    let changed = delegate.toggle_row_array_expand(data_row);
                    if changed {
                        cx.notify();
                    }
                    changed
                });
                if changed {
                    window.refresh();
                }
            }
        };
        return div()
            .id(("editor-result-rowno-cell", row_ix))
            // 内距 4px（DBeaver `GridRowRenderer.LEFT/RIGHT_MARGIN = 4`——行头是 4、
            // 单元格与列头才是 6，两个渲染器各写了一组）
            .pl(crate::ui::RESULT_ROW_HEAD_MARGIN_X)
            .pr(crate::ui::RESULT_ROW_HEAD_MARGIN_X)
            .flex()
            .items_center()
            .h_full()
            .truncate()
            .text_xs()
            .text_color(muted)
            // 行号槽是最左一列，它的左边界就是网格左边缘：网格线不画在这里
            // （DBeaver 的竖线只画在列与列之间，`LightGrid.java:2426-2437` 跳过首个边界）。
            // 它与数据列之间的那条线由右侧数据格的 `border_l` 画。
            .when_some(local_bg, |cell, bg| cell.bg(bg))
            .when(paints_head, |cell| cell.bg(head_bg))
            .when(is_focused_row, |cell| cell.font_weight(FontWeight::BOLD))
            .when(is_deleted, |cell| cell.line_through())
            // 用例按它量行高（`.id()` 不登记坐标）：这是「表格密度」那条口径的回归哨兵
            .debug_selector(move || format!("editor-result-rowno-{row_ix}"))
            // 【DBeaver 对齐】行号格右键要能弹「复制行号」：组件只认**最近一次右键的
            // 单元格**（`delegate.context_cell`），数据格在 `on_mouse_down(Right)` 里记，
            // 行号格同样记一笔（列传 0 = 行号槽）。不记的话点行号格会沿用上一个数据格
            // 的落点，弹出那一格的菜单。
            .on_mouse_down(MouseButton::Right, move |_, _window, app| {
                table.update(app, |state, _cx| {
                    state.delegate_mut().set_context_cell(Some((row_ix, 0)));
                });
            })
            .when_some(expander_glyph, |cell, glyph| {
                cell.child(
                    div()
                        .id(("editor-result-rowno-expander", row_ix))
                        .flex_none()
                        .w(crate::ui::RESULT_ROW_EXPANDER_SIZE)
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        // 回归哨兵：几何在 headless 下量不到，用它坐实「这一行真的画了三角」
                        .debug_selector(move || format!("editor-result-rowno-expander-{row_ix}"))
                        .child(SharedString::from(glyph))
                        .on_mouse_down(MouseButton::Left, toggle_row_expand),
                )
            })
            .child(
                div()
                    // 三角与数字的间距 2px（真值 `EXPANDER_SPACING`）；没有三角的行不留空位
                    // （真值只在 `isElementExpandable` 的行上留 `icon.w + 2`，本仓同理）
                    .when(expander_glyph.is_some(), |this| {
                        this.ml(crate::ui::RESULT_ROW_EXPANDER_SPACING)
                    })
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(text)),
            )
            .into_any_element();
    }

    // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（原型 §2.4）
    let is_null = text == "NULL";
    // 【#28-B4】行身份渲染标记：本地新增 / 复制行铺专用底色；被删底层行灰字 + 删除线，
    // 保留在网格内可见（对齐 DBeaver 默认删除呈现；撤销可恢复）。两类身份互斥。
    let is_deleted = delegate.is_deleted_row(row_ix);
    let is_local = delegate.row_is_local(row_ix);
    let local_bg = is_local.then(|| product_tokens::get(cx).result_local_row_background(theme));
    // 【P2】二进制列：格子里不铺一长串 hex，给「BLOB · 大小」摘要；
    // 悬停 / 查看完整值仍然给完整 hex（那是看全它的路）
    let is_blob = !is_null && kind == ColumnKind::Binary;
    let display = if is_blob {
        blob_summary(&text).unwrap_or_else(|| text.clone())
    } else {
        // 【dbdelve 借鉴】展示派生统一走 `cell_display_text`：短值 JSON 美化 →
        // 类型格式化；超长值跳过解析零拷贝借原文（解析成本有上界），再统一裁剪。
        clip_cell_display(
            &cell_display_text(&text, kind, &delegate.type_style),
            CELL_DISPLAY_LIMIT,
        )
    };
    // 【B14】右键菜单要用“哪个单元格”：组件库只公开了 `right_clicked_row`，
    // 没有单元格访问器，所以在单元格自己身上记一笔（事件从最内层派发，不会丢）
    let table = cx.entity().clone();
    // 【刀2】数组列：`▶ 首值 [+N-1]`（三角在单元格内、文本前，间距 `INSIDE_MARGIN = 3`；
    // hint 在文本右侧，间距 `LEFT_MARGIN = 6` —— `GridCellRenderer.java:158-172` /
    // `renderHints:339`）。
    // 【刀3】展开状态从 delegate 读（父数据行 + 数据列）；数组子行自己不再套一层三角
    //（二级展开本仓没做，不给假图标）。
    let row_reference = delegate.reference_of(row_ix);
    let array_expanded = match row_reference {
        Some(crate::rowset::RowRef::Data(data_row)) if !delegate.transposed => {
            col_ix
                .checked_sub(1)
                .is_some_and(|view_col| delegate.is_array_expanded(data_row, delegate.data_column(view_col)))
        }
        _ => false,
    };
    let array_display = if row_reference.is_some_and(|reference| reference.is_array_child()) {
        ArrayCellDisplay::Plain
    } else {
        array_cell::array_cell_display(kind, &text, array_expanded)
    };
    // 【结果集配色槽】脏格 / 冲突格底色在闭包外先求值：`product_tokens::get` 要借 `cx`，
    // 而 `.when` 的闭包捕获后与后面的多次借用打架。
    let dirty_bg = product_tokens::get(cx).result_cell_modified_background(theme);
    let conflict_bg = product_tokens::get(cx).result_cell_error_background(theme);
    // 【D10】矩形选区：矩形里的格叠一层选中覆盖层，**焦点格除外**——组件自己会给
    // 选中格画同槽覆盖层（vendor `table/state.rs` 的 `is_cell_selected`），再叠一层
    // 半透明会比矩形内其他格深一档（DBeaver 里整块矩形是同一个色）。
    let in_cell_range = delegate.in_cell_range(row_ix, col_ix);
    let paints_range = in_cell_range && delegate.range_focus() != Some((row_ix, col_ix));
    let range_bg = theme.tokens.table_active;
    // 【刀3】三角点击：展开 / 收起这一格数组（真值入口是行号槽三角
    // `LightGrid.handleMouseDown:3117-3122`；本仓把三角画在单元格内、点的也是它，
    // 与截图形态一致）。行数会变 → 通知表格重算行数并重画。
    // 先单独 clone：下面的右键闭包会把 `table` 整个 move 走。
    let toggle_array_expand = {
        let table = cx.entity().clone();
        move |_: &MouseDownEvent, window: &mut Window, app: &mut App| {
            let toggled = table.update(app, |state, cx| {
                let delegate = state.delegate_mut();
                let Some(crate::rowset::RowRef::Data(data_row)) = delegate.reference_of(row_ix) else {
                    return None;
                };
                let column = delegate.data_column(col_ix.checked_sub(1)?);
                delegate.toggle_array_expand(data_row, column);
                cx.notify();
                Some(())
            });
            if toggled.is_some() {
                window.refresh();
            }
        }
    };
    let mut cell = div()
        // 悬停提示要求元素有 id：gpui 的**流式** `tooltip` 声明在
        // `StatefulInteractiveElement` 上（没 id 的裸 `div` 只有直调 `Interactivity` 的写法），
        // 组件库自己的表头 / 行头单元格也是这么挂的
        .id((
            "editor-result-cell",
            row_ix * (delegate.columns.len() + 1) + col_ix,
        ))
        // 左右内距 6px：与列宽自适应共用 `ui::RESULT_CELL_PADDING_X` 一个来源
        // （DBeaver `GridCellRenderer.LEFT/RIGHT_MARGIN = 6`）
        .px(crate::ui::RESULT_CELL_PADDING_X)
        .truncate()
        .text_xs()
        // 【网格线】列分隔：DBeaver `LightGrid` 的 `linesVisible` 默认开（`:295`），逐列在
        // 列边界画 1px 线（`:2425-2439`）。画在自己盒子的**左**边界上：本格撑满整列宽，
        // 所以这条线就落在列边界；表头用同一个基准（表头内容区右端被组件 `offset_pr`
        // 占去 4px，右边界线会落在列内侧，见 `header.rs::render_th`）
        .border_l(crate::ui::HAIRLINE)
        .border_color(theme.table_row_border)
        // 被删行灰字（与 NULL 同走 muted；NULL 斜体口径不变）；其余按类型样式角色染色
        .text_color(if is_null || is_deleted {
            muted
        } else {
            role_color.unwrap_or(foreground)
        })
        // 【类型样式】按族背景色（先铺，本地行 / 脏格 / 冲突格底色后续覆盖其上）
        .when_some(bg_color, |cell, bg| cell.bg(bg))
        // 【类型样式】按族字体样式覆盖（inherit = 跟随类型默认；NULL 斜体等既有语义保持，
        // 与 Bold/Italic/Underline/LineThrough 叠加时按用户显式样式优先呈现）
        .when(style.font_style == FontStylePref::Bold, |cell| {
            cell.font_weight(FontWeight::BOLD)
        })
        .when(style.font_style == FontStylePref::Italic, |cell| {
            cell.italic()
        })
        .when(style.font_style == FontStylePref::Underline, |cell| {
            cell.underline()
        })
        .when(style.font_style == FontStylePref::LineThrough, |cell| {
            cell.line_through()
        })
        // 【#28-B4】本地行底色（先铺，脏格 / 冲突格后续底色覆盖其上）
        .when_some(local_bg, |cell, bg| cell.bg(bg))
        // 【T2a-3】脏格（未提交变更）：DBeaver `cell.modified.background` 同角色底色
        //（产品 token `result.cell.modified.background`；资产缺失回退旧口径 warning 16%）
        .when(delegate.is_dirty(row_ix, col_ix), |cell| {
            cell.bg(dirty_bg)
        })
        // 【T2a-6】冲突格（提交没写进去）：DBeaver `cell.error.background` 同角色底色，
        // 与脏格区分——优先级高于脏格（冲突格先被标过 dirty 的可能性存在：同一格再编辑
        // 后提交，两段底色会叠）；产品 token `result.cell.error.background`，
        // 资产缺失回退旧口径 danger 16%
        .when(delegate.is_conflict(row_ix, col_ix), |cell| {
            cell.bg(conflict_bg)
        })
        // 【D10】矩形选区覆盖层（与组件画焦点格那层同构：绝对铺满 + 同槽半透明色）。
        // 用覆盖层而不是 `.bg()`：类型样式底色 / 本地行底色 / 脏格色都要透出来——
        // DBeaver 的选中也是叠在格子底色**之上**画的，不是替换底色。
        .when(paints_range, |cell| {
            cell.relative().child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(range_bg)
                    // 回归哨兵：颜色在 headless 下读不到，用它坐实「矩形内的非焦点格真的铺了」
                    .debug_selector(move || {
                        format!("editor-result-cell-in-range-{row_ix}-{col_ix}")
                    }),
            )
        })
        // 【类型感知】数字右对齐；其余左对齐
        .text_align(style.align)
        // 【类型感知】布尔 / 时间族用主题的等宽字体（两列放一起时字形对得齐）
        .when_some(mono_family, |cell, family| cell.font_family(family))
        .when(is_null, |cell| cell.italic())
        // 【#28-B4】被删行整格删除线（颜色跟随文字色，gpui-pre 无独立删除线色 builder）
        .when(is_deleted, |cell| cell.line_through())
        // 用例按它点/量真单元格（`Ctrl+C` 复制选中、行距口径都从这里进去）
        .debug_selector(move || format!("editor-result-cell-{row_ix}-{col_ix}"))
        .on_mouse_down(MouseButton::Right, move |_, _window, app| {
            table.update(app, |state, _cx| {
                state
                    .delegate_mut()
                    .set_context_cell(Some((row_ix, col_ix)));
            });
        });
    // 取值可能很宽（JSON / 长文本），`truncate` 之后就只剩悬停这一条看全的路
    if !text.is_empty() {
        let full = text.clone();
        cell = cell.tooltip(move |window, cx| result_cell_tooltip(full.clone(), window, cx));
    }
    // 【刀2】数组格子拆成「三角 / 首值 / hint」三段（普通格子仍是单个文本）
    let body: gpui_kit::AnyElement = match array_display {
        ArrayCellDisplay::Plain => SharedString::from(display).into_any_element(),
        ArrayCellDisplay::Collapsed { lead, hint } => {
            let mut row = div()
                .flex()
                .items_center()
                // 回归哨兵：几何在 headless 下量不到，用它坐实「收起态真的走这条分支」
                .debug_selector(move || format!("editor-result-array-collapsed-{row_ix}-{col_ix}"))
                .child(
                    div()
                        .id(("editor-result-array-toggle", row_ix * 64 + col_ix))
                        .flex_none()
                        .cursor_pointer()
                        .text_color(muted)
                        .child(SharedString::from(ARRAY_COLLAPSED_GLYPH))
                        .on_mouse_down(MouseButton::Left, toggle_array_expand.clone()),
                )
                .child(
                    div()
                        .ml(gpui_kit::px(ARRAY_GLYPH_SPACING))
                        .truncate()
                        .child(SharedString::from(lead)),
                );
            if let Some(hint) = hint {
                row = row.child(
                    div()
                        .ml(gpui_kit::px(ARRAY_HINT_SPACING))
                        .flex_none()
                        .text_color(muted)
                        .child(SharedString::from(hint)),
                );
            }
            row.into_any_element()
        }
        ArrayCellDisplay::Expanded { label } => div()
            .flex()
            .items_center()
            .debug_selector(move || format!("editor-result-array-expanded-{row_ix}-{col_ix}"))
            .child(
                div()
                    .id(("editor-result-array-toggle", row_ix * 64 + col_ix))
                    .flex_none()
                    .cursor_pointer()
                    .text_color(muted)
                    .child(SharedString::from(ARRAY_EXPANDED_GLYPH))
                    .on_mouse_down(MouseButton::Left, toggle_array_expand),
            )
            .child(
                div()
                    .ml(gpui_kit::px(ARRAY_GLYPH_SPACING))
                    .child(SharedString::from(label)),
            )
            .into_any_element(),
    };
    cell.child(body).into_any_element()
}

// ========== 值查看器内容分派（纯函数；事件路径重建树，render 只摆） ==========

/// 每节点最多展开子项数（大字段截断：数组 / 对象超限时追加「… 还有 N 项」占位）
pub(crate) const JSON_TREE_CHILD_LIMIT: usize = 500;
/// hex 转储每行字节数（offset · hex · ASCII 三栏）。
/// 页大小在面板侧按「本常量 × 每页行数」算（翻页必须按行首对齐）
pub(crate) const HEX_DUMP_BYTES_PER_LINE: usize = 16;

/// 值查看器内容模式（按列类型 + 值形态分派）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ViewerMode {
    /// 普通文本：等宽滚动块
    Text,
    /// 可展开 JSON 树（`tree::Tree` 组件）
    Json,
    /// 【刀4】PostgreSQL 数组字面量：按集合树展开（与 JSON 树同一套 `Tree`，
    /// 树内容由 `array_tree_items` 建）
    Array,
    /// 十六进制转储（offset · hex · ASCII 三栏，分页）
    Hex,
    /// 图片预览（格式 + 字节数；解码走事件路径）
    Image { format: &'static str, size: usize },
}

/// 值查看器分派（轻量判定；真正的 JSON 校验在事件路径 `json_tree_items`）：
/// - Binary：图片魔数命中 → Image，否则 → Hex
/// - Array：能按 PG 字面量解析 → Array，否则按文本（不猜形态）
/// - 其余：值以 `{` / `[` 开头（trim 后）→ Json，否则 → Text
pub(crate) fn classify_value(kind: ColumnKind, text: &str) -> ViewerMode {
    match kind {
        ColumnKind::Binary => match detect_image_format(text) {
            Some(format) => ViewerMode::Image {
                format,
                size: text.len() / 2,
            },
            None => ViewerMode::Hex,
        },
        // 【刀4】数组列走集合树：`{a,b}` 长得像 JSON 的对象/数组，但 PG 数组字面量和
        // JSON 语法不同（无引号裸词、双引号元素、`NULL` 裸词），送 JSON 解析必失败
        ColumnKind::Array => {
            if crate::store::parse_pg_array_literal(text).is_some() {
                ViewerMode::Array
            } else {
                ViewerMode::Text
            }
        }
        _ => {
            let t = text.trim_start();
            if t.starts_with('{') || t.starts_with('[') {
                ViewerMode::Json
            } else {
                ViewerMode::Text
            }
        }
    }
}

/// 图片魔数检测（hex 文本前缀，大小写不敏感）：PNG / JPEG / GIF / WebP
pub(crate) fn detect_image_format(hex: &str) -> Option<&'static str> {
    let h = hex.trim();
    let has = |prefix: &str| {
        h.get(..prefix.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
    };
    if has("89504e47") {
        Some("PNG")
    } else if has("ffd8ff") {
        Some("JPEG")
    } else if has("47494638") {
        Some("GIF")
    } else if has("52494646")
        && h.get(16..24)
            .is_some_and(|s| s.eq_ignore_ascii_case("57454250"))
    {
        // WebP 容器：'RIFF'(0..8) + 4 字节大小(8..16) + 'WEBP'(16..24)
        Some("WebP")
    } else {
        None
    }
}

/// hex 文本 → 字节（偶数位才认；末尾不完整忽略）
/// 魔数格式名 → gpui 图片格式（只认值查看器能直接显示的静态格式；
/// 认不出返回 `None`，调用处回 hex 转储或文本占位）
pub(crate) fn image_format_of(name: &str) -> Option<ImageFormat> {
    match name {
        "PNG" => Some(ImageFormat::Png),
        "JPEG" => Some(ImageFormat::Jpeg),
        "GIF" => Some(ImageFormat::Gif),
        "WebP" => Some(ImageFormat::Webp),
        _ => None,
    }
}

pub(crate) fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut iter = hex.bytes();
    loop {
        let Some(hi) = iter.next() else { break };
        let Some(lo) = iter.next() else { break };
        if let (Some(hi), Some(lo)) = ((hi as char).to_digit(16), (lo as char).to_digit(16)) {
            bytes.push((hi * 16 + lo) as u8);
        }
        // 非法字节对：跳过（保持后续对齐，不静默吞掉整个串）
    }
    bytes
}

/// hex 转储分页：每行 `[offset] xx xx … |ascii`（16 字节一行，offset 8 位十六进制）
pub(crate) fn hex_lines(bytes: &[u8], offset: usize, limit: usize) -> Vec<String> {
    (0..limit)
        .filter_map(|i| {
            let base = offset + i * HEX_DUMP_BYTES_PER_LINE;
            if base >= bytes.len() {
                return None;
            }
            let end = (base + HEX_DUMP_BYTES_PER_LINE).min(bytes.len());
            let chunk = &bytes[base..end];
            let hex = chunk
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii: String = chunk
                .iter()
                .map(|b| {
                    if b.is_ascii_graphic() || *b == b' ' {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            Some(format!("[{base:08x}] {hex:<48} |{ascii}"))
        })
        .collect()
}

/// JSON 文本 → 可展开树（嵌套 `TreeItem`；每节点子项上限 `JSON_TREE_CHILD_LIMIT`，
/// 超出追加「… 还有 N 项」占位；顶层预展开 2 层）。非法 JSON → `None`。
pub(crate) fn json_tree_items(text: &str) -> Option<Vec<TreeItem>> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    fn build(value: &serde_json::Value, key: &str, path: &str, depth: usize) -> TreeItem {
        match value {
            serde_json::Value::Object(map) => {
                let mut item =
                    TreeItem::new(path.to_string(), format!("{key} 对象（{} 项）", map.len()))
                        .expanded(depth < 2);
                for (k, v) in map.iter().take(JSON_TREE_CHILD_LIMIT) {
                    item = item.child(build(v, k, &format!("{path}.{k}"), depth + 1));
                }
                if map.len() > JSON_TREE_CHILD_LIMIT {
                    item = item.child(TreeItem::new(
                        format!("{path}.__trunc__"),
                        format!("… 还有 {} 项", map.len() - JSON_TREE_CHILD_LIMIT),
                    ));
                }
                item
            }
            serde_json::Value::Array(arr) => {
                let mut item =
                    TreeItem::new(path.to_string(), format!("{key} 数组 [{}]", arr.len()))
                        .expanded(depth < 2);
                for (i, v) in arr.iter().take(JSON_TREE_CHILD_LIMIT).enumerate() {
                    item = item.child(build(v, &i.to_string(), &format!("{path}[{i}]"), depth + 1));
                }
                if arr.len() > JSON_TREE_CHILD_LIMIT {
                    item = item.child(TreeItem::new(
                        format!("{path}.__trunc__"),
                        format!("… 还有 {} 项", arr.len() - JSON_TREE_CHILD_LIMIT),
                    ));
                }
                item
            }
            _ => TreeItem::new(path.to_string(), format!("{key}: {value}")),
        }
    }
    Some(vec![build(&value, "$", "$", 0)])
}

/// 数组完整预览项上限（真值 `SpreadsheetPresentation.MAX_FULL_ARRAY_PREVIEW_ITEMS = 100`）
pub(crate) const ARRAY_TREE_CHILD_LIMIT: usize = 100;

/// 【刀4】PostgreSQL 数组字面量 → 可展开树（值查看器用，与 JSON 树共用 `Tree` 组件）
///
/// 与 `json_tree_items` 的差别只在**解析器**：PG 数组用 `{a,b}`、裸词不引号、`NULL` 是
/// 裸词、元素用双引号转义——JSON 解析器一律不收，所以数组列单走这一条。嵌套元素
/// （多维数组 `{{1,2},{3,4}}`）递归成子节点；顶层超过 100 项追加「… 还有 N 项」。
pub(crate) fn array_tree_items(text: &str) -> Option<Vec<TreeItem>> {
    use crate::store::ArrayItem;

    fn build(item: &ArrayItem, key: &str, path: &str, depth: usize) -> TreeItem {
        match item {
            ArrayItem::Nested(inner) => {
                let mut node =
                    TreeItem::new(path.to_string(), format!("{key} 数组 [{}]", inner.len()))
                        .expanded(depth < 2);
                for (i, child) in inner.iter().take(ARRAY_TREE_CHILD_LIMIT).enumerate() {
                    node = node.child(build(child, &i.to_string(), &format!("{path}[{i}]"), depth + 1));
                }
                if inner.len() > ARRAY_TREE_CHILD_LIMIT {
                    node = node.child(TreeItem::new(
                        format!("{path}.__trunc__"),
                        format!("… 还有 {} 项", inner.len() - ARRAY_TREE_CHILD_LIMIT),
                    ));
                }
                node
            }
            other => TreeItem::new(path.to_string(), format!("{key}: {}", other.display())),
        }
    }

    let items = crate::store::parse_pg_array_literal(text)?;
    let mut root = TreeItem::new("$".to_string(), format!("$ 数组 [{}]", items.len())).expanded(true);
    for (i, item) in items.iter().take(ARRAY_TREE_CHILD_LIMIT).enumerate() {
        root = root.child(build(item, &i.to_string(), &format!("$[{i}]"), 1));
    }
    if items.len() > ARRAY_TREE_CHILD_LIMIT {
        root = root.child(TreeItem::new(
            "$.__trunc__".to_string(),
            format!("… 还有 {} 项", items.len() - ARRAY_TREE_CHILD_LIMIT),
        ));
    }
    Some(vec![root])
}
