//! 时间列卡（YData 密度对齐）：统计表行集 / 月度分布图 / 折叠态 Top 条。
//!
//! 已知数据缺口（如实）：源库取样走 JSON 打型，DATETIME 列常被打成文本
//! （`ColumnKind::Text`）——本卡只在列类型族为 DateTime 时启用；类型面扩展
//! （P1-8b 方向）落地前，时间列大多走 text 卡的降级渲染。

use crate::workspace::*;

/// 基础统计行集（跨度 + 月度 Top 预览）。
pub(crate) fn stats_rows(col: &TableColumnView) -> Vec<(String, String)> {
    let Some(crate::model::types::ColumnStatsDetail::DateTime(d)) = &col.stats_detail else {
        return vec![("统计".into(), "无可用统计".into())];
    };
    vec![
        ("最早".into(), d.earliest.clone()),
        ("最晚".into(), d.latest.clone()),
        ("跨度（天）".into(), fmt_int(d.span_days.max(0) as u32)),
        (
            "月度 Top".into(),
            d.monthly_distribution
                .iter()
                .take(3)
                .map(|f| format!("{}（{}）", f.value, f.count))
                .collect::<Vec<_>>()
                .join(" · "),
        ),
    ]
}

/// 分布纯图：月度分布条（前 5）。
pub(crate) fn dist_visual(col: &TableColumnView, theme: &Theme) -> Div {
    let Some(crate::model::types::ColumnStatsDetail::DateTime(d)) = &col.stats_detail else {
        return zone_note(theme, "无可用统计");
    };
    dist_text_bars(theme, &d.monthly_distribution, 5, &col.name)
}

/// 折叠态月度 Top 条（与文本卡共用渲染）。
pub(crate) fn mini_freq_rows(
    col_name: &str,
    items: &[crate::model::types::TextFrequency],
    theme: &Theme,
) -> Option<Div> {
    super::text::mini_freq_rows(col_name, items, theme)
}
