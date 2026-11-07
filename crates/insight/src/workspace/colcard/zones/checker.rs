//! 列卡「质量」区：检查器命中汇总 + 四态列表。

use super::super::*;

impl super::super::super::InsightWorkspacePanel {
    /// 检查器命中汇总（原型 749 行）：`检查器 h/t 命中 · n 不适用` + 规则列表。
    pub(crate) fn render_checker_summary(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        if checker.is_empty() {
            return div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("该列无检查器属性（未评估全表）");
        }
        let hits = checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Hit)
            .count();
        let applicable = checker
            .iter()
            .filter(|c| c.state != ColumnCheckState::NotApplicableType)
            .count();
        let nas = checker.len() - applicable;
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .border_t_1()
            .border_color(colors.border)
            .pt_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "检查器 {hits}/{applicable} 命中{}",
                        if nas > 0 {
                            format!(" · {nas} 不适用")
                        } else {
                            String::new()
                        }
                    )),
            )
            .child(self.render_checker_list(col, checker, theme))
    }

    /// 检查器四态列表（原型 `ruleList`）：命中绿 / 未命中红 / 不适用灰。
    pub(crate) fn render_checker_list(
        &self,
        _col: &TableColumnView,
        checker: &[ColumnChecker],
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        if checker.is_empty() {
            return div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("该列无检查器属性");
        }
        let mut rows = div().v_flex().w_full().gap_1();
        // 未命中前置（YData Warnings 语义：问题一眼看到）；其余保持原序
        let mut ordered: Vec<&ColumnChecker> = checker.iter().collect();
        ordered.sort_by_key(|c| !matches!(c.state, ColumnCheckState::Miss));
        for check in ordered {
            let (state_text, color) = match check.state {
                ColumnCheckState::Hit => ("命中", colors.success),
                ColumnCheckState::Miss => ("未命中", colors.danger),
                ColumnCheckState::NotApplicableType => ("不适用", colors.muted_foreground),
                ColumnCheckState::Excluded => ("已排除", colors.muted_foreground),
            };
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_STATUS_WIDTH))
                            .text_xs()
                            .text_color(color)
                            .child(state_text),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child(check.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(colors.muted_foreground)
                            .child(check.detail.clone()),
                    ),
            );
        }
        rows
    }
}
