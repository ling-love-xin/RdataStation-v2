//! 对比表自由函数：滚动容器 / 指标行 / 行原语 / 表头。

use super::*;

pub(crate) fn compare_table_scroll(
    min_width: f32,
    header: Div,
    rows: impl IntoIterator<Item = impl IntoElement>,
) -> impl IntoElement {
    div().flex_none().w_full().overflow_x_scrollbar().child(
        div()
            .v_flex()
            .w_full()
            .min_w(rems(min_width))
            .gap_0p5()
            .child(header)
            .children(rows),
    )
}

/// 转置对比表的指标行（原型 `.cmp-table`）：**指标为行、每张勾选表为一列**。
///
/// 与旧版「表为行、指标为列」相反：指标集合固定 6 项，表数才是变量，故窄窗下横向
/// 增长的是列而不是行，读起来是「同一指标横着比几张表」而不是「一张表竖着看七个
/// 指标」。列宽随表数变化，由 `ui::metric_table_min_width` 兜底横向滚动。
pub(crate) fn metric_rows(entries: &[&OpenedTarget], theme: &Theme) -> Vec<Div> {
    let colors = theme.colors;
    // 六项事实指标；无摘要（未完成 / 非表目标）时统一出占位「—」而不是编 0
    let metrics: Vec<(&'static str, Box<dyn Fn(&TargetSummary) -> (String, Hsla)>)> = vec![
        (
            "总行数",
            Box::new(move |s: &TargetSummary| {
                (
                    s.row_count.map(fmt_rows).unwrap_or_else(|| "—".into()),
                    colors.muted_foreground,
                )
            }),
        ),
        (
            "列数",
            Box::new(move |s: &TargetSummary| {
                (
                    s.column_count
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "—".into()),
                    colors.muted_foreground,
                )
            }),
        ),
        (
            "检查器命中",
            Box::new(
                move |s: &TargetSummary| match (s.checker_applicable, s.checker_hits) {
                    (Some(applicable), Some(hits)) if applicable > 0 => (
                        format!("{hits}/{applicable}"),
                        if hits == applicable {
                            colors.success
                        } else if hits == 0 {
                            colors.danger
                        } else {
                            colors.warning
                        },
                    ),
                    _ => ("—".into(), colors.muted_foreground),
                },
            ),
        ),
        (
            "缺失率",
            Box::new(move |s: &TargetSummary| match s.null_rate {
                Some(r) => (
                    fmt_pct(r),
                    if r > NULL_RATE_NOTE {
                        colors.warning
                    } else {
                        colors.muted_foreground
                    },
                ),
                None => ("—".into(), colors.muted_foreground),
            }),
        ),
        (
            "唯一率",
            Box::new(move |s: &TargetSummary| match s.unique_rate {
                Some(r) => (
                    fmt_pct(r),
                    if r > crate::quality_scorer::CHECKER_UNIQUE_MAX {
                        colors.warning
                    } else {
                        colors.muted_foreground
                    },
                ),
                None => ("—".into(), colors.muted_foreground),
            }),
        ),
        (
            "值得关注",
            Box::new(move |s: &TargetSummary| match s.attention_columns {
                Some(n) => (
                    format!("{n} 列"),
                    if n > 0 {
                        colors.danger
                    } else {
                        colors.muted_foreground
                    },
                ),
                None => ("—".into(), colors.muted_foreground),
            }),
        ),
    ];

    metrics
        .into_iter()
        .map(|(label, value_of)| {
            let mut row = div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .px_1()
                .h(rems(ui::ROW_HEIGHT))
                .text_sm()
                .child(
                    div()
                        .w(rems(ui::INSIGHT_WS_METRIC_LABEL_WIDTH))
                        .flex_none()
                        .text_color(colors.foreground)
                        .text_ellipsis()
                        .child(label),
                );
            for entry in entries {
                let (text, color) = match entry.summary {
                    Some(s) => value_of(&s),
                    None => ("—".into(), colors.muted_foreground),
                };
                row = row.child(
                    div()
                        .flex_1()
                        .min_w(rems(ui::INSIGHT_WS_METRIC_COL_MIN_WIDTH))
                        .text_xs()
                        .text_color(color)
                        .text_ellipsis()
                        .child(text),
                );
            }
            row
        })
        .collect()
}

/// 对比行原语：居中行 + 行高（列内容由调用方追加）。
///
/// 带 `id` 后是 `Stateful<Div>`：hover / 点击按 id 记录状态（业务键，非下标）。
pub(crate) fn compare_row(theme: &Theme, id: SharedString) -> gpui::Stateful<Div> {
    let colors = theme.colors;
    div()
        .id(id)
        .h(rems(ui::ROW_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .rounded_sm()
        .hover(|this| this.bg(colors.sidebar_accent))
}

/// 对比候选表头（勾选 / 表名 / 数据源 / 行数 / 列数 / 状态）
pub(crate) fn compare_table_header(theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(div().w(rems(ui::INSIGHT_WS_CHECKBOX_WIDTH)).flex_none())
        .child(
            div()
                .flex_1()
                .min_w(rems(ui::INSIGHT_WS_TABLE_COL_MIN_WIDTH))
                .child("表"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                .flex_none()
                .child("数据源"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                .flex_none()
                .child("行数"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                .flex_none()
                .child("列数"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_STATUS_WIDTH))
                .flex_none()
                .child("状态"),
        )
}
