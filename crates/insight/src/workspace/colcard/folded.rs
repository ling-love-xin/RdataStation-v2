//! 折叠行（YData 单行版式，2026-11-06 自 colcard/mod.rs 抽出）：
//! 名称 + 类型徽标 + 缺失/唯一 + **统计块** + 探索价值 + 检查器未命中。
//!
//! 统计块按类型分派（numeric 均值+范围 / text Top 值 / datetime 范围 / boolean
//! 真占比）——YData 折叠头一眼读全的关键；检查器未命中数直接亮在折叠行
//! （YData 的行内告警对应物，数据来自已算好的 checker，零额外查询）。

use super::*;

impl InsightWorkspacePanel {
    /// 折叠行。`checker` 由 `render_col_card` 已算好传入（未命中数上屏）。
    /// 参数面是 GPUI 渲染惯例（列上下文 + 折叠态 + 徽标），不强行打包。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_card_folded_row(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        entity: &Entity<Self>,
        theme: &Theme,
        open: bool,
        explore_badge: Option<Tag>,
    ) -> Div {
        let colors = theme.colors;
        let misses = checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Miss)
            .count();
        // 统计块按类型分派（折叠行一眼读全的核心）
        let preview = match col.kind {
            ColumnKind::Numeric => numeric::folded_preview(col),
            ColumnKind::Text => text::folded_preview(col),
            ColumnKind::DateTime => datetime::folded_preview(col),
            ColumnKind::Boolean => boolean::folded_preview(col),
            ColumnKind::Unknown => Vec::new(),
        };
        div()
            .w_full()
            .h_flex()
            .items_center()
            .gap_2()
            .px_1()
            .py_0p5()
            .rounded_sm()
            .bg(colors.sidebar)
            .hover(|this| this.bg(colors.sidebar_accent))
            // 展开切换（语义 Button，键盘可达）
            .child({
                let entity = entity.clone();
                let name = col.name.clone();
                let toggled = open;
                Button::new(SharedString::from(format!(
                    "insight-ws-col-toggle-{}",
                    name
                )))
                .ghost()
                .xsmall()
                .icon(if toggled {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .tooltip(if toggled {
                    "收起"
                } else {
                    "展开统计 / 分布 / 检查器 / 样本 / 规则"
                })
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| {
                        let mut open = panel.col_open.borrow_mut();
                        if toggled {
                            open.remove(&name);
                        } else {
                            open.insert(name.clone());
                        }
                        cx.notify();
                    });
                })
            })
            .child(
                div()
                    .flex_none()
                    .min_w(rems(6.))
                    .max_w(rems(14.))
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(col.name.clone()),
            )
            .child(
                Tag::color(match col.kind {
                    ColumnKind::Numeric => ColorName::Green,
                    ColumnKind::Text => ColorName::Blue,
                    ColumnKind::DateTime => ColorName::Violet,
                    ColumnKind::Boolean => ColorName::Teal,
                    ColumnKind::Unknown => ColorName::Gray,
                })
                .xsmall()
                .child(col.kind.label()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "缺失 {}",
                        col.null_rate
                            .map(|n| format!("{:.0}%", n * 100.0))
                            .unwrap_or_else(|| "—".into())
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "唯一 {}",
                        col.unique_rate
                            .map(|u| format!("{:.0}%", u * 100.0))
                            .unwrap_or_else(|| "—".into())
                    )),
            )
            // 统计块（类型专属，YData 折叠头密度）
            .children(preview.into_iter().map(|(label, value)| {
                div()
                    .flex_none()
                    .max_w(rems(10.))
                    .text_xs()
                    .text_color(colors.foreground)
                    .text_ellipsis()
                    .child(SharedString::from(format!("{label} {value}")))
            }))
            .when_some(explore_badge, |this, badge| this.child(badge))
            .when(misses > 0, |this| {
                // 行内告警（YData 对应物）：未命中数直接可见，不必展开
                this.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.danger)
                        .child(format!("⚠ {misses} 未命中")),
                )
            })
            .when(col.interrupted, |this| {
                this.child(Tag::color(ColorName::Amber).xsmall().child("未完成"))
            })
            .child(div().flex_1().min_w_0())
    }
}
