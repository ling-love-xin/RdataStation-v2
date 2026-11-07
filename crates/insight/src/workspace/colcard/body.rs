//! 列卡展开体：规则开关 + 警告块 + 五区 Tab + 当前区内容 + 检查器汇总 + SQL 入口。

use super::*;
use gpui_kit::assets::IconName as AssetsIconName;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};

impl super::super::InsightWorkspacePanel {
    /// 列卡展开体（折叠行在 folded.rs）。
    pub(crate) fn render_col_card_body(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        zone: ColZone,
        entity: &Entity<Self>,
        theme: &Theme,
        temp_table: Option<String>,
    ) -> Div {
        let rule_toggle = div().h_flex().w_full().items_center().gap_2().child(
            div().flex_1().min_w_0().child(
                Switch::new(SharedString::from(format!(
                    "insight-ws-rule-only-{}",
                    col.name
                )))
                .checked(
                    self.col_rule_only
                        .borrow()
                        .get(&col.name)
                        .copied()
                        .unwrap_or(false),
                )
                .label("只看规则属性")
                .on_click({
                    let entity = entity.clone();
                    let name = col.name.clone();
                    move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            let on = panel
                                .col_rule_only
                                .borrow()
                                .get(&name)
                                .copied()
                                .unwrap_or(false);
                            panel.col_rule_only.borrow_mut().insert(name.clone(), !on);
                            cx.notify();
                        });
                    }
                }),
            ),
        );
        let rule_only = self
            .col_rule_only
            .borrow()
            .get(&col.name)
            .copied()
            .unwrap_or(false);
        if rule_only {
            return div()
                .v_flex()
                .w_full()
                .gap_1()
                .px_1()
                .pb_1()
                .child(rule_toggle)
                .child(self.render_checker_list(col, checker, theme));
        }
        // 警告块（YData Warnings 对应物）：未命中摘要直接摆在展开体顶部
        // （最多 3 条 + 等价计数），amber 底框——用户不用切到「检查器」区就知道问题
        let warnings: Vec<&ColumnChecker> = checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Miss)
            .collect();
        let warning_box = (!warnings.is_empty()).then(|| {
            let mut boxd = div()
                .v_flex()
                .w_full()
                .gap_0p5()
                .p_1()
                .rounded_sm()
                .bg(theme.colors.list)
                .border_1()
                .border_color(theme.colors.danger)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.colors.danger)
                        .child(format!("⚠ {} 项未命中", warnings.len())),
                );
            for check in warnings.iter().take(3) {
                boxd = boxd.child(div().text_xs().text_color(theme.colors.danger).child(
                    SharedString::from(format!("{}：{}", check.name, check.detail)),
                ));
            }
            if warnings.len() > 3 {
                boxd = boxd.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("…等 {} 条（详见检查器区）", warnings.len())),
                );
            }
            boxd
        });
        // 五区 Tab（segmented；id 带列名防跨列串台）+ 当前区内容 + 检查器汇总
        let tabs = TabBar::new(SharedString::from(format!(
            "insight-ws-col-zones-{}",
            col.name
        )))
        .segmented()
        .selected_index(zone.index())
        .children(ColZone::ALL.iter().map(|z| Tab::new().label(z.label())))
        .on_click({
            let entity = entity.clone();
            let name = col.name.clone();
            let col_kind = col.kind; // Copy，闭包外捕获（col 引用不能进 'static 闭包）
            move |index, _, app| {
                entity.update(app, |panel, cx| {
                    panel
                        .col_detail_zone
                        .borrow_mut()
                        .insert(name.clone(), *index);
                    // 切到「规则」Tab 顺带惰性载入摘要（事件路径，允许取数）
                    if ColZone::of_index(*index) == ColZone::Rules {
                        panel.ensure_col_rules(&name, col_kind, cx);
                    }
                    cx.notify();
                });
            }
        });
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .px_1()
            .pb_1()
            .child(rule_toggle)
            .when_some(warning_box, |body, w| body.child(w))
            .child(tabs)
            .child(self.render_col_zone(col, checker, zone, entity, theme))
            .child(self.render_checker_summary(col, checker, theme))
            // 列卡「在 SQL 编辑器中查看」（2026-10-30 对齐原型 col-card 尾部）：
            // 生成该列的探查 SQL（带引号转义），宿主 `open_query_document` 打开
            // 不执行（`run = false`：看语句，不跑）。
            .when_some(temp_table, |body, tmp| {
                let entity = entity.clone();
                let name = col.name.clone();
                let quoted_col = col.name.replace('"', "\"\"");
                let quoted_tbl = tmp.replace('"', "\"\"");
                let sql = format!("SELECT \"{quoted_col}\" FROM \"{quoted_tbl}\" LIMIT 100");
                let title = format!("{} · {}", tmp, col.name);
                body.child(
                    div().h_flex().w_full().justify_end().child(
                        Button::new(SharedString::from(format!("insight-ws-col-sql-{}", name)))
                            .ghost()
                            .xsmall()
                            .icon(AssetsIconName::Braces)
                            .label("在 SQL 编辑器中查看")
                            .on_click(move |_, _, app| {
                                entity.update(app, |panel, cx| {
                                    panel.insight_view().update(cx, |_view, cx| {
                                        cx.emit(InsightEvent::OpenSqlRequested {
                                            sql: sql.clone(),
                                            title: title.clone(),
                                        });
                                    });
                                });
                            }),
                    ),
                )
            })
    }
}
