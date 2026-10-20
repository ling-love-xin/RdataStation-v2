//! ER 视图：**原生 GPUI 绘制**（实体卡片 + 外键连线），不引入 webview。
//!
//! 分层：卡片是**面板级绘制**（矩形 + 表头 + 列行，对照设计文档 §8），连线走 gpui 的
//! `canvas` + `PathBuilder` 描正交折线（几何在 [`crate::er::edge`]，本模块只负责画）。
//!
//! 约束：
//! - **零裸值**：颜色一律 `cx.theme().colors.*`；尺寸一律 `workbench_shell::ui::ER_*`
//!   经 [`metrics_for_theme`] 换算（rem 倍率 × 主题字号）。
//! - **render 纯读**：本函数不发起任何 I/O，也不改状态；数据（`ErDiagram`）与布局
//!   （`LaidOutDiagram`）由面板在数据到达时算好、缓存，这里只读。
//! - pan / zoom / 拖拽重排属第三期（设计 §10），本片只有只读渲染 + 双向滚动。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::*;

use workbench_shell::ui;

use crate::er::edge::{edge_paths, ErPoint};
use crate::er::layout::{ErRect, LaidOutDiagram, LayoutMetrics};
use crate::er::model::{ErDiagram, ErEntity};

/// 由主题字号派生的布局度量（rem 倍率 × 字号 = 逻辑像素）。
///
/// **视图唯一入口**：不要在别处用 [`LayoutMetrics::default`]（那是测试 / 无主题环境的兜底）。
pub fn metrics_for_theme(theme: &Theme) -> LayoutMetrics {
    let fs = f32::from(theme.font_size);
    LayoutMetrics {
        card_width: ui::ER_CARD_WIDTH * fs,
        header_height: ui::ER_CARD_HEADER_HEIGHT * fs,
        row_height: ui::ER_CARD_ROW_HEIGHT * fs,
        h_gap: ui::ER_LAYER_H_GAP * fs,
        v_gap: ui::ER_NODE_V_GAP * fs,
        padding: ui::ER_CANVAS_PADDING * fs,
    }
}

/// 画一张只读 ER 图（内容尺寸 = `layout.bounds`，超出视口双向滚动）。
///
/// `diagram` 与 `layout` 必须同源（`layout` 由同一个 `diagram` 经 `layered_layout` 得到）；
/// `layout.boxes` 与 `diagram.entities` 同序同长（见 [`crate::er::layout`]）。
pub fn render_er_diagram(
    diagram: &ErDiagram,
    layout: &LaidOutDiagram,
    metrics: &LayoutMetrics,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let canvas_bg = theme.colors.background;
    let border = theme.colors.border;
    let card_bg = theme.colors.list;
    let head_bg = theme.colors.list_head;
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let pk_color = theme.colors.primary;
    let fk_color = theme.colors.info;

    // 连线几何是纯函数结果：**拥有**一份塞进 `'static` 的 paint 闭包（闭包不能借用）。
    let edges = edge_paths(diagram, layout, metrics);
    let edge_color = border;
    let edge_width = ui::ER_EDGE_WIDTH;

    // 底层：连线画布（铺满内容区）。
    let lines = canvas(
        move |_bounds, _window, _cx| {},
        move |_bounds, _state, window, _cx| {
            for e in &edges {
                let Some(path) = polyline(&e.points, edge_width) else {
                    continue;
                };
                window.paint_path(path, edge_color);
            }
        },
    )
    .absolute()
    .size_full();

    // 上层：实体卡片（绝对定位到各自矩形）。
    let mut content = div()
        .relative()
        .w(px(layout.bounds.width))
        .h(px(layout.bounds.height))
        .child(lines);
    for (b, entity) in layout.boxes.iter().zip(diagram.entities.iter()) {
        content = content.child(render_card(entity, &b.rect, metrics, card_bg, head_bg, border, fg, muted, pk_color, fk_color));
    }

    div()
        .id("er-diagram")
        .size_full()
        .bg(canvas_bg)
        .overflow_scroll()
        .child(content)
}

/// 把折线顶点转成 gpui 的描边 `Path`（少于两点或 tessellation 失败 → `None`）。
fn polyline(points: &[ErPoint], width: Pixels) -> Option<Path<Pixels>> {
    let first = points.first()?;
    let mut builder = PathBuilder::stroke(width);
    builder.move_to(point(px(first.x), px(first.y)));
    for p in &points[1..] {
        builder.line_to(point(px(p.x), px(p.y)));
    }
    builder.build().ok()
}

/// 一张实体卡片（表头 + 列行）。
#[allow(clippy::too_many_arguments)]
fn render_card(
    entity: &ErEntity,
    rect: &ErRect,
    metrics: &LayoutMetrics,
    card_bg: Hsla,
    head_bg: Hsla,
    border: Hsla,
    fg: Hsla,
    muted: Hsla,
    pk_color: Hsla,
    fk_color: Hsla,
) -> impl IntoElement {
    let title = if entity.is_view {
        format!("{}（视图）", entity.name)
    } else {
        entity.name.clone()
    };

    let mut rows = div().v_flex().w_full();
    for c in &entity.columns {
        let mut left = div().flex().items_center().gap_1().min_w_0();
        left = left.child(div().text_xs().text_color(fg).truncate().child(c.name.clone()));
        // 主键 / 外键标记：极小文字 chip（面板级绘制，不引组件库标记件）。
        if c.is_primary_key {
            left = left.child(div().text_xs().text_color(pk_color).child("PK"));
        }
        if c.is_foreign_key {
            left = left.child(div().text_xs().text_color(fk_color).child("FK"));
        }
        rows = rows.child(
            div()
                .h(px(metrics.row_height))
                .px_2()
                .flex()
                .items_center()
                .justify_between()
                .child(left)
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(muted)
                        .child(c.data_type.clone()),
                ),
        );
    }

    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.width))
        .h(px(rect.height))
        .bg(card_bg)
        .border_1()
        .border_color(border)
        .rounded_md()
        .overflow_hidden()
        .child(
            div()
                .h(px(metrics.header_height))
                .px_2()
                .flex()
                .items_center()
                .bg(head_bg)
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .truncate()
                        .child(title),
                ),
        )
        .child(rows)
}
