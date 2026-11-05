//! 数值列卡（YData 密度对齐，2026-11-06）：统计表行集 / 分布纯图 / 迷你直方图。
//!
//! 数据全部来自评估回填的 `NumericStats` + `histogram`（引擎 `numeric-stats` /
//! `histogram` 规则）；本文件只做**渲染**，不取数。

use crate::workspace::*;

/// 基础统计行集（Stats 区左表；键序对齐 YData Statistics 面板习惯：集中趋势 →
/// 分位 → 离散 → 形状 → 特殊值）。
pub(crate) fn stats_rows(col: &TableColumnView) -> Vec<(String, String)> {
    let Some(crate::model::types::ColumnStatsDetail::Numeric(n)) = &col.stats_detail else {
        return vec![("统计".into(), "无可用统计".into())];
    };
    vec![
        ("平均".into(), fmt_num(n.avg)),
        ("中位".into(), fmt_num(n.median)),
        ("最小".into(), fmt_num(n.min)),
        ("最大".into(), fmt_num(n.max)),
        ("P25".into(), fmt_num(n.p25)),
        ("P75".into(), fmt_num(n.p75)),
        (
            "P95".into(),
            n.p95.map(fmt_num).unwrap_or_else(|| "—".into()),
        ),
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
        (
            "偏度".into(),
            n.skewness.map(fmt_num).unwrap_or_else(|| "—".into()),
        ),
        (
            "峰度".into(),
            n.kurtosis.map(fmt_num).unwrap_or_else(|| "—".into()),
        ),
    ]
}

/// 分布纯图：等宽 8 柱直方图（`dist_bars` 与右栏/导出同源）。
pub(crate) fn dist_visual(col: &TableColumnView, theme: &Theme) -> Div {
    match col.histogram.as_ref() {
        Some(bins) if !bins.is_empty() => dist_bars(theme, bins, 8, &col.name),
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

/// 折叠行迷你直方图（8 柱等宽，柱高 = 最小可读 + 比例段）。
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
    for bin in bins.iter().take(8) {
        let h = rems(0.375 + 2.375 * (bin.count as f32 / max));
        bars = bars.child(div().flex_1().rounded_sm().bg(colors.primary).h(h));
    }
    bars
}
