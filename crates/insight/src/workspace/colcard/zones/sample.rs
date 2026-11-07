//! 列卡「样本」区：该列前 5 条样本值（评估回填）。

use super::super::*;

impl super::super::super::InsightWorkspacePanel {
    /// 样本区：该列前 5 条样本值（评估回填）。
    pub(crate) fn render_col_sample_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let colors = theme.colors;
        if col.sample_values.is_empty() {
            return zone_note(theme, "无样本值（未评估或列为空）");
        }
        let mut rows = div().v_flex().w_full().gap_1();
        for (ix, value) in col.sample_values.iter().enumerate() {
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("#{}", ix + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if value == "NULL" {
                                colors.muted_foreground
                            } else {
                                colors.foreground
                            })
                            .child(value.clone()),
                    ),
            );
        }
        rows
    }
}
