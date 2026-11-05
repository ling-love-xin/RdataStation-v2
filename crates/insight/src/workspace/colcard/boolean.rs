//! 布尔列卡（YData 密度对齐）：统计表行集 / 真假占比图 / 折叠态占比条。

use crate::workspace::*;

/// 基础统计行集。
pub(crate) fn stats_rows(col: &TableColumnView) -> Vec<(String, String)> {
    let Some(crate::model::types::ColumnStatsDetail::Boolean(b)) = &col.stats_detail else {
        return vec![("统计".into(), "无可用统计".into())];
    };
    vec![
        ("真".into(), fmt_int(b.true_count)),
        ("假".into(), fmt_int(b.false_count)),
        ("真占比".into(), fmt_pct(b.true_ratio)),
    ]
}

/// 分布纯图：真 / 假两柱（中性色，不引入「真假=好坏」语义）。
pub(crate) fn dist_visual(col: &TableColumnView, theme: &Theme) -> Div {
    let Some(crate::model::types::ColumnStatsDetail::Boolean(b)) = &col.stats_detail else {
        return zone_note(theme, "无可用统计");
    };
    let bins = vec![
        crate::model::types::DistributionBin {
            label: "真".into(),
            count: b.true_count,
            ratio: b.true_ratio,
        },
        crate::model::types::DistributionBin {
            label: "假".into(),
            count: b.false_count,
            ratio: 1.0 - b.true_ratio,
        },
    ];
    dist_bars(theme, &bins, 2, &col.name)
}

/// 折叠态真假占比条（两段 + 图例）。
pub(crate) fn mini_ratio(theme: &Theme, true_ratio: f64) -> Div {
    let colors = theme.colors;
    let t = true_ratio.clamp(0.0, 1.0);
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .h_flex()
                .w_full()
                .h(rems(0.375))
                .rounded_sm()
                .overflow_hidden()
                .child(div().h_full().bg(colors.primary).flex_grow(t as f32))
                .child(
                    div()
                        .h_full()
                        .bg(colors.muted_foreground)
                        .flex_grow((1.0 - t) as f32),
                ),
        )
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("真 {:.0}%", t * 100.0))
                .child(format!("假 {:.0}%", (1.0 - t) * 100.0)),
        )
}
