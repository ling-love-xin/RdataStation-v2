//! 渲染片段助手（仅中间工作页使用）：卡片 / 分区 / 条形 / 徽标原语。

use super::*;


/// 工具栏竖分隔线
pub(crate) fn divider(theme: &Theme) -> Div {
    div()
        .w_px()
        .h(rems(ui::ROW_HEIGHT * 0.6))
        .bg(theme.colors.border)
        .mx_1()
}

/// 概览五卡之一（原型 .card：键 + 值 + 说明）
pub(crate) fn summary_card(theme: &Theme, key: &str, value: String, sub: impl Into<String>) -> Div {
    let colors = theme.colors;
    let sub = sub.into();
    div()
        .flex_1()
        .min_w_0()
        .v_flex()
        .gap_0p5()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(colors.border)
        .child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(key.to_string()),
        )
        .child(
            div()
                .text_size(rems(ui::INSIGHT_CARD_VALUE_FONT * 0.75))
                .text_color(colors.foreground)
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .text_ellipsis()
                .child(sub),
        )
}

/// 带标题的 panel 容器（原型 .panel：panel-head + panel-body）
pub(crate) fn panel_box(theme: &Theme, title: &str, tag: String, body: Div) -> Div {
    let colors = theme.colors;
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .border_1()
        .border_color(colors.border)
        .rounded_sm()
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.to_string()),
                )
                .child(div().flex_1())
                .when(!tag.is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(tag),
                    )
                }),
        )
        .child(div().w_full().px_2().pb_2().child(body))
}

/// 质量提示的一行（图标 + 文案）
pub(crate) fn note_line(icon: IconName, text: String, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(Icon::new(icon).text_color(colors.info))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(text),
        )
}

/// 缺失率的探索价值权重（原型 exploreScore 规则：>20% +3、>5% +1.5、>0 +0.3）
pub(crate) fn explore_missing_weight(col: &TableColumnView) -> f64 {
    let Some(rate) = col.null_rate else {
        return 0.0;
    };
    if rate > 0.2 {
        3.0
    } else if rate > 0.05 {
        1.5
    } else if rate > 0.0 {
        0.3
    } else {
        0.0
    }
}

/// 该列检查器（评估后的 `TableQualityView::checker` 按列过滤）。
/// 未评估 / 无检查器 → 空表（不编假值）。
pub(crate) fn column_checkers(profile: &TableProfileView, col: &TableColumnView) -> Vec<ColumnChecker> {
    profile
        .quality
        .as_ref()
        .map(|q| {
            q.checker
                .iter()
                .filter(|c| c.column == col.name)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// 列探索价值（原型 exploreScore：缺失权重 + 高基数/唯一率≈行数 +2
/// （主键天然高唯一不重复计）+ 未识别类型 +1.5 + 检查器未命中每条 +1.0）。
///
/// 检查器加分系数 1.0 是经验默认（原型「检查器未命中加分」未给系数）：一条未命中
/// ≈ 一条轻度缺失的信号强度（原型缺失权重 +1.5 与 +0.3 之间取中），线性累加使
/// 多规则未命中的列在「值得关注」里自然前置——探索分是**排序工具**（决定值得
/// 关注的先后与列卡徽标 ≥3/≥1 档位），极端值只影响排序不破坏分级，故不封顶。
/// 不适用-类型与排除态不计——它们不代表数据有问题。系数待原型定稿。
pub(crate) fn col_explore_score(col: &TableColumnView, checker: &[ColumnChecker]) -> f64 {
    let mut score = explore_missing_weight(col);
    if col.unique_rate.map(|u| u > 0.9).unwrap_or(false) && !col.primary_key {
        score += 2.0;
    }
    if col.kind == ColumnKind::Unknown {
        score += 1.5;
    }
    score += checker
        .iter()
        .filter(|c| c.state == ColumnCheckState::Miss)
        .count() as f64
        * 1.0;
    score
}

/// 展开区的一行提示（无数据 / 未评估占位）
pub(crate) fn zone_note(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 展开区键值行列表
pub(crate) fn zone_rows(theme: &Theme, rows: Vec<(String, String)>) -> Div {
    let colors = theme.colors;
    let mut body = div().v_flex().w_full().gap_1();
    for (key, value) in rows {
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(key),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(colors.foreground)
                        .child(value),
                ),
        );
    }
    body
}

/// 分布条（直方图 / Top 值 / 月度 / 布尔共用）：label + 条 + 计数。
/// 条用原生 flex 宽度（布局不是控件，不必组件化）；取前 `limit` 条防爆版面。
pub(crate) fn dist_bars(theme: &Theme, bins: &[crate::model::types::DistributionBin], limit: usize) -> Div {
    let colors = theme.colors;
    let mut body = div().v_flex().w_full().gap_1();
    for bin in bins.iter().take(limit) {
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                        .text_xs()
                        .text_ellipsis()
                        .text_color(colors.muted_foreground)
                        .child(bin.label.clone()),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Progress::new(SharedString::from(format!("insight-ws-dist-{}", bin.label)))
                            .value(bin.ratio.clamp(0.0, 1.0) as f32)
                            .color(colors.primary),
                    ),
                )
                .child(
                    div()
                        .flex_none()
                        .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(fmt_int(bin.count)),
                ),
        );
    }
    body
}

/// 取样进度里的呼吸圆点（暂态指示，循环动效；`reduce_motion` 下静止在 1.0）
pub(crate) fn pulse_dot(theme: &Theme) -> impl IntoElement {
    div()
        .w(rems(0.375))
        .h(rems(0.375))
        .rounded_full()
        .bg(theme.colors.info)
        .with_animation(
            "insight-ws-pulse",
            gpui_kit::Animation::new(Duration::from_millis(1200))
                .repeat()
                .with_easing(gpui_kit::bounce(gpui_kit::ease_in_out)),
            |el, delta| el.opacity(1.0 - delta * 0.5),
        )
}

/// 两段式进度的一步（源取样 / 画像）：圆点 + 标签
pub(crate) fn step_dot(name: &str, active: bool, done: bool, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .w(rems(0.75))
                .h(rems(0.75))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(if done {
                    colors.success
                } else if active {
                    colors.info
                } else {
                    colors.muted_foreground
                })
                .bg(if done || active {
                    colors.list_active
                } else {
                    colors.list_hover
                })
                .child(if done { "✓" } else { "" }),
        )
        .child(
            div()
                .text_xs()
                .text_color(if active || done {
                    colors.foreground
                } else {
                    colors.muted_foreground
                })
                .child(name.to_string()),
        )
}
