//! 数值列卡（YData 密度对齐）：分组统计表 / 16 柱直方图 / 折叠统计块。
//!
//! 数据全部来自评估回填的 `NumericStats` + `histogram`（引擎 `numeric-stats` /
//! `histogram` 规则）；本文件只做**渲染**，不取数。

use crate::workspace::*;

/// 基础统计**分组**行集（Stats 区左表；对齐 YData Statistics 的分组呈现：
/// 概览 / 分位 / 离散 / 形状）。
pub(crate) fn stats_groups(col: &TableColumnView) -> Vec<(&'static str, Vec<(String, String)>)> {
    let Some(crate::model::types::ColumnStatsDetail::Numeric(n)) = &col.stats_detail else {
        return vec![("统计", vec![("—".into(), "无可用统计".into())])];
    };
    vec![
        (
            "概览",
            vec![
                ("平均".into(), fmt_num(n.avg)),
                ("中位".into(), fmt_num(n.median)),
            ],
        ),
        (
            "分位",
            vec![
                ("最小".into(), fmt_num(n.min)),
                ("P25".into(), fmt_num(n.p25)),
                ("P75".into(), fmt_num(n.p75)),
                (
                    "P95".into(),
                    n.p95.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
                ("最大".into(), fmt_num(n.max)),
            ],
        ),
        (
            "离散",
            vec![
                (
                    "标准差".into(),
                    n.stddev.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
                (
                    "0 值".into(),
                    n.zeros.map(fmt_int).unwrap_or_else(|| "—".into()),
                ),
                (
                    "负值".into(),
                    n.negatives.map(fmt_int).unwrap_or_else(|| "—".into()),
                ),
            ],
        ),
        (
            "形状",
            vec![
                (
                    "偏度".into(),
                    n.skewness.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
                (
                    "峰度".into(),
                    n.kurtosis.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
            ],
        ),
    ]
}

/// 折叠行统计块（YData 式一眼读全）：均值 + 范围。
pub(crate) fn folded_preview(col: &TableColumnView) -> Vec<(String, String)> {
    let Some(crate::model::types::ColumnStatsDetail::Numeric(n)) = &col.stats_detail else {
        return Vec::new();
    };
    vec![
        ("均值".into(), fmt_num(n.avg)),
        (
            "范围".into(),
            format!("{} ~ {}", fmt_num(n.min), fmt_num(n.max)),
        ),
    ]
}

/// 分布纯图：分位条 + 16 柱直方图（内置 `histogram` 规则同柱数）。
pub(crate) fn dist_visual(col: &TableColumnView, theme: &Theme) -> Div {
    let mut body = div().v_flex().w_full().gap_2();
    // 分位条在直方图上方（YData Statistics 同位）：段宽 = 分位间距
    if let Some(strip) = quantile_strip(col, theme) {
        body = body.child(strip);
    }
    match col.histogram.as_ref() {
        Some(bins) if !bins.is_empty() => body = body.child(dist_bars(theme, bins, 16, &col.name)),
        _ => body = body.child(zone_note(theme, "样本行数不足，未生成直方图")),
    }
    body
}

/// 分布附注：偏度 / 峰度 + 正态性判定（数值卡独有）。
pub(crate) fn dist_notes(col: &TableColumnView, theme: &Theme) -> Option<Div> {
    let Some(crate::model::types::ColumnStatsDetail::Numeric(n)) = &col.stats_detail else {
        return None;
    };
    let (Some(skew), Some(kurt)) = (n.skewness, n.kurtosis) else {
        return None;
    };
    let colors = theme.colors;
    let mut line = div().text_xs().text_color(colors.muted_foreground);
    if let Some(judge) = normality_judge(skew, kurt) {
        line = line.text_color(colors.warning).child(format!(
            "{judge}：偏度 {} · 峰度 {}",
            fmt_num(skew),
            fmt_num(kurt)
        ));
    } else {
        line = line.child(format!(
            "偏度 {}（{}）· 峰度 {}（{}）",
            fmt_num(skew),
            skew_direction(skew),
            fmt_num(kurt),
            kurt_direction(kurt)
        ));
    }
    Some(line)
}

/// 分位条（YData Statistics 右栏的分位可视化对应物，纯渲染无交互）：
/// min→max 一条轨道，按 (p25/median/p75 − min)/(max − min) 分四段交替着色，
/// 段宽即分位间距——一眼看出数据集中在哪一侧；下方 tick 行给五个分位值。
pub(crate) fn quantile_strip(col: &TableColumnView, theme: &Theme) -> Option<Div> {
    let Some(crate::model::types::ColumnStatsDetail::Numeric(n)) = &col.stats_detail else {
        return None;
    };
    let span = n.max - n.min;
    if !span.is_finite() || span <= 0.0 {
        return None; // 常数列：分位条无意义（单点）
    }
    let colors = theme.colors;
    let pos = |v: f64| -> f32 { (((v - n.min) / span).clamp(0.0, 1.0) * 100.0) as f32 };
    let seg = |a: f64, b: f64, id: &str, alt: bool| {
        let w = pos(b) - pos(a);
        if w <= 0.0 {
            return None;
        }
        Some(
            div()
                .h_full()
                .bg(if alt {
                    colors.primary
                } else {
                    colors.muted_foreground
                })
                .flex_grow(w)
                .id(SharedString::from(format!(
                    "insight-ws-quant-{}-{}",
                    col.name, id
                ))),
        )
    };
    let track = div()
        .h_flex()
        .w_full()
        .h(rems(0.625))
        .rounded_sm()
        .overflow_hidden()
        .bg(colors.sidebar)
        .when_some(seg(n.min, n.p25, "q1", true), |t, c| t.child(c))
        .when_some(seg(n.p25, n.median, "q2", false), |t, c| t.child(c))
        .when_some(seg(n.median, n.p75, "q3", true), |t, c| t.child(c))
        .when_some(seg(n.p75, n.max, "q4", false), |t, c| t.child(c));
    Some(
        div().v_flex().w_full().gap_1().child(track).child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("min {}", fmt_num(n.min)))
                .child(format!("P25 {}", fmt_num(n.p25)))
                .child(format!("中位 {}", fmt_num(n.median)))
                .child(format!("P75 {}", fmt_num(n.p75)))
                .child(format!("max {}", fmt_num(n.max))),
        ),
    )
}

/// 折叠行迷你直方图（16 柱等宽，柱高 = 最小可读 + 比例段）。
pub(crate) fn mini_histogram(theme: &Theme, bins: &[crate::model::types::DistributionBin]) -> Div {
    let colors = theme.colors;
    let max = bins.iter().map(|b| b.count).max().unwrap_or(0).max(1) as f32;
    let mut bars = div()
        .h_flex()
        .w_full()
        .items_end()
        .gap_1()
        // 迷你直方图总高 44px（对齐原型 .mini-bar 高度档）
        .h(rems(2.75));
    for bin in bins.iter().take(16) {
        let h = rems(0.375 + 2.375 * (bin.count as f32 / max));
        bars = bars.child(div().flex_1().rounded_sm().bg(colors.primary).h(h));
    }
    bars
}
