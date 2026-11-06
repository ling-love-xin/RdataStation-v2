//! 文本列卡（YData 密度对齐）：分组统计表 / Top 频次图 / 折叠态 Top 条。

use crate::workspace::*;

/// 基础统计**分组**行集（长度 / 频次两组）。
pub(crate) fn stats_groups(col: &TableColumnView) -> Vec<(&'static str, Vec<(String, String)>)> {
    let Some(crate::model::types::ColumnStatsDetail::Text(t)) = &col.stats_detail else {
        return vec![("统计", vec![("—".into(), "无可用统计".into())])];
    };
    vec![
        (
            "长度",
            vec![
                ("最短".into(), fmt_int(t.min_length)),
                ("最长".into(), fmt_int(t.max_length)),
            ],
        ),
        (
            "频次",
            vec![(
                "Top 值".into(),
                t.top_values
                    .iter()
                    .take(3)
                    .map(|f| format!("{}（{}）", f.value, f.count))
                    .collect::<Vec<_>>()
                    .join(" · "),
            )],
        ),
    ]
}

/// 折叠行统计块：最高频值。
pub(crate) fn folded_preview(col: &TableColumnView) -> Vec<(String, String)> {
    let Some(crate::model::types::ColumnStatsDetail::Text(t)) = &col.stats_detail else {
        return Vec::new();
    };
    match t.top_values.first() {
        Some(f) => vec![("Top".into(), format!("{} ×{}", f.value, f.count))],
        None => Vec::new(),
    }
}

/// 分布纯图：Top 值频次条（前 5）。
pub(crate) fn dist_visual(col: &TableColumnView, theme: &Theme) -> Div {
    let Some(crate::model::types::ColumnStatsDetail::Text(t)) = &col.stats_detail else {
        return zone_note(theme, "无可用统计");
    };
    dist_text_bars(theme, &t.top_values, 5, &col.name)
}

/// 迷你 Top 条（文本 / 月度共用）：`variant` 进元素 id——折叠行（"fold"）与
/// 展开体 Stats 右栏（"zone"）可能**同帧共存**（Collapsible 收起后内容保持
/// 挂载），id 不带位置前缀就是 Duplicate a11y node id panic。
pub(crate) fn mini_freq_rows(
    col_name: &str,
    variant: &str,
    items: &[crate::model::types::TextFrequency],
    theme: &Theme,
) -> Option<Div> {
    let colors = theme.colors;
    let first = items.first()?;
    let max = first.count.max(1);
    let mut rows = div().v_flex().w_full().gap_1();
    // id 用列名 + 序号：f.value 是用户数据文本，同列 top 值可能撞文本（如 NULL/null），
    // 用序号兜底避免 a11y 节点 id 重复 panic。
    for (ix, f) in items.iter().take(3).enumerate() {
        let key = SharedString::from(format!("insight-ws-mini-{col_name}-{variant}-{ix}"));
        rows = rows.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .max_w(rems(8.))
                        .text_ellipsis()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(f.value.clone()),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Progress::new(key)
                            .value((f.count as f64 / max as f64) as f32)
                            .color(colors.primary),
                    ),
                ),
        );
    }
    Some(rows)
}
