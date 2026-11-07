//! 列卡「分布」区：类型专属分布图 + 附注（数值偏度/峰度）+ 折叠行迷你图。

use super::super::*;

impl super::super::super::InsightWorkspacePanel {
    /// 分布区：类型专属图 + 附注（数值卡的偏度 / 峰度行）。
    pub(crate) fn render_col_dist_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let mut body = div().v_flex().w_full().gap_1();
        body = body.child(self.type_dist_visual(col, theme));
        if let Some(note) = self.type_dist_notes(col, theme) {
            body = body.child(note);
        }
        body
    }

    /// 类型分派：分布纯图。
    pub(crate) fn type_dist_visual(&self, col: &TableColumnView, theme: &Theme) -> Div {
        if col.stats_detail.is_none() {
            return zone_note(theme, "未评估全表，无分布数据");
        }
        match col.kind {
            ColumnKind::Numeric => numeric::dist_visual(col, theme),
            ColumnKind::Text => text::dist_visual(col, theme),
            ColumnKind::DateTime => datetime::dist_visual(col, theme),
            ColumnKind::Boolean => boolean::dist_visual(col, theme),
            ColumnKind::Unknown => zone_note(theme, "类型未识别，无分布数据"),
        }
    }

    /// 类型分派：分布附注（数值卡偏度 / 峰度；其余类型无附注）。
    pub(crate) fn type_dist_notes(&self, col: &TableColumnView, theme: &Theme) -> Option<Div> {
        match col.kind {
            ColumnKind::Numeric => numeric::dist_notes(col, theme),
            _ => None,
        }
    }
}
