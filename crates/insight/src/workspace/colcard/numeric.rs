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

/// 分布纯图：16 柱等宽直方图（内置 `histogram` 规则同柱数）。
pub(crate) fn dist_visual(col: &TableColumnView, theme: &Theme) -> Div {
    match col.histogram.as_ref() {
        Some(bins) if !bins.is_empty() => dist_bars(theme, bins, 16, &col.name),
        _ => zone_note(theme, "样本行数不足，未生成直方图"),
    }
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
