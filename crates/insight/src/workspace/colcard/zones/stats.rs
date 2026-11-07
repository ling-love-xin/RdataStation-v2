//! 列卡「基础统计」区：左侧键值统计表 + 右侧分布图并排。

use super::super::*;

impl super::super::super::InsightWorkspacePanel {
    /// 基础统计区（YData Statistics 版式）：**左侧键值统计表 + 右侧分布图**
    /// 并排；行集与图都按列类型族分派（type 文件）。
    pub(crate) fn render_col_stats_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        if col.stats_detail.is_none() {
            return zone_note(theme, "未评估全表，无列级统计（点工具栏「评估」）");
        }
        let groups = self.type_stats_groups(col);
        div()
            .h_flex()
            .w_full()
            .items_stretch()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(zone_rows_grouped(theme, &groups)),
            )
            .child(
                div()
                    .flex_none()
                    .w(rems(ui::INSIGHT_CARD_VIS_WIDTH))
                    .min_w_0()
                    .child(self.type_mini_chart(col, "zone", theme)),
            )
    }

    /// 类型分派：右栏 mini 图（zone 变体；数值直方 / Top 条 / 真假占比）。
    ///
    /// 右栏刻意用 **mini 渲染器**（无 dist id / zone 前缀 id）而不是完整
    /// `dist_visual`：Stats 右栏 / Dist 区 / 折叠行 mini 三处可能同帧共存
    /// （Collapsible 收起后内容保持挂载），Progress id 只按「列」区分会重复
    /// 注册 → Duplicate a11y node id panic（2026-11-06 真机闪退根因防御）。
    pub(crate) fn type_mini_chart(&self, col: &TableColumnView, variant: &str, theme: &Theme) -> Div {
        match &col.stats_detail {
            Some(crate::model::types::ColumnStatsDetail::Numeric(_)) => {
                match col.histogram.as_ref() {
                    Some(bins) if !bins.is_empty() => numeric::mini_histogram(theme, bins),
                    _ => zone_note(theme, "样本行数不足"),
                }
            }
            Some(crate::model::types::ColumnStatsDetail::Text(t)) => {
                text::mini_freq_rows(&col.name, variant, &t.top_values, theme)
                    .unwrap_or_else(|| zone_note(theme, "无频次数据"))
            }
            Some(crate::model::types::ColumnStatsDetail::DateTime(d)) => {
                datetime::mini_freq_rows(&col.name, variant, &d.monthly_distribution, theme)
                    .unwrap_or_else(|| zone_note(theme, "无频次数据"))
            }
            Some(crate::model::types::ColumnStatsDetail::Boolean(b)) => {
                boolean::mini_ratio(theme, b.true_ratio)
            }
            _ => zone_note(theme, "类型未识别"),
        }
    }

    /// 类型分派：基础统计**分组**行集（YData Statistics 分组呈现）。
    ///
    /// 最前统一插「计数」组（YData Overview 的 Distinct/Missing 对应物）：
    /// 空值数 / 唯一值——精确计数来自表级摘要（`ColumnStatBrief` 回填），
    /// 对所有类型通用。
    pub(crate) fn type_stats_groups(
        &self,
        col: &TableColumnView,
    ) -> Vec<(&'static str, Vec<(String, String)>)> {
        let mut groups = self.type_stats_groups_by_kind(col);
        let mut counts = Vec::new();
        if let Some(n) = col.null_count {
            counts.push(("空值数".into(), fmt_int(n.max(0) as u32)));
        }
        if let Some(u) = col.unique_count {
            counts.push(("唯一值".into(), fmt_int(u.max(0) as u32)));
        }
        if !counts.is_empty() {
            groups.insert(0, ("计数", counts));
        }
        groups
    }

    fn type_stats_groups_by_kind(
        &self,
        col: &TableColumnView,
    ) -> Vec<(&'static str, Vec<(String, String)>)> {
        match col.kind {
            ColumnKind::Numeric => numeric::stats_groups(col),
            ColumnKind::Text => text::stats_groups(col),
            ColumnKind::DateTime => datetime::stats_groups(col),
            ColumnKind::Boolean => boolean::stats_groups(col),
            ColumnKind::Unknown => vec![("统计", vec![("—".into(), "类型未识别".into())])],
        }
    }
}
