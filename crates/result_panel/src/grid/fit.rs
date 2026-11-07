//! 列宽自适应（`grid/` 第十块）：双击列头分隔线时按内容算这一列该多宽。
//!
//! 入口在组件库：双击 resize handle 的判定在 vendor `table/state.rs:1733-1736`，它调
//! [`TableState::fit_col`](vendor `:1284`)，后者问 delegate 要 `TableDelegate::fit_column`
//! 的结果，加上单元格 padding、夹进列的 `min_width` / `max_width`，最后 emit
//! `TableEvent::ColumnWidthsChanged`。本仓此前没实现 `fit_column`（trait 默认返回 `None`），
//! 所以双击一直是“什么也不发生”。
//!
//! 分工：这里只回答**内容宽度**（不含单元格 padding，padding 由组件加）。它不改 delegate
//! 状态——列宽最终由随后的 `ColumnWidthsChanged` 回灌（与拖拽同一条通道，落盘 / 跨会话
//! 恢复都不用另接线）。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

use super::cell::{blob_summary, cell_display_text, clip_cell_display};
use super::delegate::{CELL_DISPLAY_LIMIT, ResultGridDelegate};
use crate::store::ColumnKind;

/// 采样行数上限（对齐 dbdelve `FITTED_ROWS = 200` / cellar 前 200 行 / tusk 首行 cap）：
/// 只看前 200 个**视图行**（筛选排序之后看着的那些），不是全部数据行——一列 10 万行全测
/// 会把双击变成一次可感知的卡顿，而第 201 行更宽的概率低到不值得付这个代价。
pub(crate) const FIT_SAMPLE_ROWS: usize = 200;

/// 单元格自身左右内边距（`render_td` 的 `px_2` = 8px × 2）。
///
/// 组件还会再加 `table_cell_padding()`（Medium = 8px × 2），两层都要算：只算一层的话
/// 文字正好压在余量里被省略号吃掉，双击就成了“点了个寂寞”。
const CELL_H_PADDING: Pixels = px(16.);

/// 表头 `main_row` 的 `gap_1`（badge / 列名 / 排序箭头 / 筛选 ▽ 之间）
const HEADER_GAP: Pixels = px(4.);

/// 类型 badge 的 `px_1`（4px × 2）
const BADGE_H_PADDING: Pixels = px(8.);

/// 双击列头分隔线：这一列按内容该多宽（`None` = 这一列不自适应）
pub(crate) fn fit_column(
    delegate: &ResultGridDelegate,
    col_ix: usize,
    window: &Window,
    cx: &App,
) -> Option<Pixels> {
    // 行号槽不可拖宽（组件侧 `resizable(false)` 已先拦住，这里再守一道）
    if ResultGridDelegate::is_row_number(col_ix) {
        return None;
    }
    // `text_xs()` = `rems(0.75)`：表头与单元格都是这一档，所以这里用同一个字号
    let font_size = window.rem_size() * 0.75;
    let base_style = window.text_style();
    let base_font = base_style.font();
    let color = base_style.color;
    // 布尔 / 时间等族在单元格里用主题等宽字体（`cell_style` 的 `monospace`），
    // 测宽必须用同一个字体，否则中文 / 数字混排的列会偏窄
    let mono_font = {
        let mut font = base_font.clone();
        font.family = cx.theme().mono_font_family.clone();
        font
    };

    let header = header_width(delegate, col_ix, window, &base_font, color, font_size);
    let cells = cells_width(
        delegate, col_ix, window, &base_font, &mono_font, color, font_size,
    );
    Some(header.max(cells))
}

/// 表头块宽：badge + 列名 + 排序箭头 + 筛选 ▽，加上它们之间的 `gap_1`
///
/// 与 [`crate::grid::header::render_th`] 同一份构成。列头菜单按钮（`DropdownButton`）不算：
/// 它浮在列头右端、被截掉也不影响读列名，算进去会让每列凭空多 28px（竞品同样不含）。
fn header_width(
    delegate: &ResultGridDelegate,
    col_ix: usize,
    window: &Window,
    font: &Font,
    color: Hsla,
    font_size: Pixels,
) -> Pixels {
    let name = if delegate.transposed {
        // 转置视图固定两列：字段 / 值
        if col_ix == 1 { "字段" } else { "值" }.to_string()
    } else {
        delegate
            .columns
            .get(delegate.data_column(col_ix - 1))
            .cloned()
            .unwrap_or_default()
    };
    // 【筛选】排序箭头 / 筛选 ▽ 都按数据列查（行号槽与转置视图没有）
    let column = if delegate.transposed {
        usize::MAX
    } else {
        delegate.data_column(col_ix - 1)
    };
    let mut widths: Vec<Pixels> = Vec::new();
    // 类型 badge（与 `render_th` 同一个符号；未知类型不画，也就不占宽）
    if let Some(sym) = ResultGridDelegate::type_kind_sym(delegate.column_kind(col_ix)) {
        widths.push(BADGE_H_PADDING + measure(window, sym, font, color, font_size));
    }
    widths.push(measure(window, &name, font, color, font_size));
    if let Some((sort_col, descending)) = delegate.sort_state() {
        if sort_col == column {
            let arrow = if descending { "↓" } else { "↑" };
            widths.push(measure(window, arrow, font, color, font_size));
        }
    }
    if delegate.has_column_filter(column) {
        widths.push(measure(window, "▽", font, color, font_size));
    }
    let gaps = HEADER_GAP * widths.len().saturating_sub(1) as f32;
    widths.into_iter().fold(px(0.), |sum, width| sum + width) + gaps
}

/// 单元格块宽：前 [`FIT_SAMPLE_ROWS`] 个视图行里最宽的**展示串**，加单元格内边距
///
/// 测的是“画出来的那串”而不是原始值：类型样式（NULL 占位 / 千分位 / 布尔中文）、
/// JSON 美化、BLOB 摘要都会改变实际占用宽度。超长值仍按 `CELL_DISPLAY_LIMIT` 截断后测
/// ——和屏幕上看到的一致。
fn cells_width(
    delegate: &ResultGridDelegate,
    col_ix: usize,
    window: &Window,
    font: &Font,
    mono_font: &Font,
    color: Hsla,
    font_size: Pixels,
) -> Pixels {
    let kind = delegate.column_kind(col_ix);
    let rows = delegate.view_total().min(FIT_SAMPLE_ROWS);
    let mut widest = px(0.);
    for row_ix in 0..rows {
        let raw = delegate.cell(row_ix, col_ix);
        // 与 `render_td` 同口径：二进制列画摘要，其余走「JSON 美化 / 类型格式化 → 裁剪」
        let shown = if kind == ColumnKind::Binary && raw != "NULL" {
            blob_summary(&raw)
        } else {
            Some(clip_cell_display(
                &cell_display_text(&raw, kind, &delegate.type_style),
                CELL_DISPLAY_LIMIT,
            ))
        };
        let Some(shown) = shown else {
            continue;
        };
        let font = if kind.is_monospace() {
            mono_font
        } else {
            font
        };
        widest = widest.max(measure(window, &shown, font, color, font_size));
    }
    widest + CELL_H_PADDING
}

/// 单行文本的渲染宽度（字形推进宽，`ShapedLine::width`）
///
/// `shape_line` 遇到换行符会 panic：单行单元格里换行被当空白，测宽等价替换成空格。
fn measure(window: &Window, text: &str, font: &Font, color: Hsla, font_size: Pixels) -> Pixels {
    if text.is_empty() {
        return px(0.);
    }
    let flattened;
    let text = if text.bytes().any(|b| b == b'\n' || b == b'\r' || b == b'\t') {
        flattened = text.replace('\n', " ").replace('\r', " ").replace('\t', " ");
        flattened.as_str()
    } else {
        text
    };
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text), font_size, &[run], None)
        .width()
}
