//! 列卡「规则」区：规则摘要列表 + 载入按钮 + SQL 查看入口。

use super::super::*;
use gpui_kit::assets::IconName as AssetsIconName;
use gpui_kit::component::IconName;

impl super::super::super::InsightWorkspacePanel {
    /// 列卡「规则」区：适用该列的规则摘要列表 + 载入按钮 + SQL 查看入口。
    pub(crate) fn render_col_rules_zone(
        &self,
        col: &TableColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let Some(briefs) = self.col_rules.borrow().get(&col.name).cloned() else {
            let entity = entity.clone();
            let name = col.name.clone();
            let kind = col.kind;
            return div()
                .v_flex()
                .w_full()
                .gap_1()
                .child(zone_note(
                    theme,
                    "规则摘要尚未载入（点击下方按钮获取适用于该列的规则）",
                ))
                .child(
                    Button::new(SharedString::from(format!(
                        "insight-ws-colrule-load-{}",
                        col.name
                    )))
                    .ghost()
                    .xsmall()
                    .icon(IconName::RefreshCw)
                    .label("载入规则摘要")
                    .on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.ensure_col_rules(&name, kind, cx);
                        });
                    }),
                );
        };
        if briefs.is_empty() {
            return zone_note(
                theme,
                "没有适用于该类型的列级规则（内置统计见「基础统计」）",
            );
        }
        let mut body = div().v_flex().w_full().gap_1();
        for brief in briefs {
            let sql_id =
                SharedString::from(format!("insight-ws-colrule-sql-{}-{}", col.name, brief.id));
            let entity = entity.clone();
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_0p5()
                    .p_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_ellipsis()
                                    .text_color(colors.foreground)
                                    .child(format!("{} · {}", brief.id, brief.name)),
                            )
                            .child(
                                Button::new(sql_id)
                                    .ghost()
                                    .xsmall()
                                    .icon(AssetsIconName::Braces)
                                    .label("SQL")
                                    .tooltip("在 SQL 编辑器中查看该规则的代入语句（不执行）")
                                    .on_click({
                                        let sql = brief.sql.clone();
                                        let title = format!("规则 {} · {}", brief.id, col.name);
                                        move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.insight_view().update(cx, |_v, cx| {
                                                    cx.emit(InsightEvent::OpenSqlRequested {
                                                        sql: sql.clone(),
                                                        title: title.clone(),
                                                    });
                                                });
                                            });
                                        }
                                    }),
                            ),
                    )
                    .when(!brief.description.is_empty(), |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(brief.description.clone()),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(SharedString::from(brief.sql.clone())),
                    ),
            );
        }
        body
    }

    /// 列规则摘要载入（事件路径：on_click 触发；同步 `with_rules` 无 I/O，
    /// 数十条规则代入毫秒级——不合规范的地方在于它在**宿主 App 上下文**，
    /// 故经后台执行器走一道，与 jobs 同款姿势）。
    pub(crate) fn ensure_col_rules(&mut self, column: &str, kind: ColumnKind, cx: &mut Context<Self>) {
        if self.col_rules.borrow().contains_key(column) {
            return;
        }
        let column = column.to_string();
        let column_for_task = column.clone();
        let task = cx.background_executor().spawn(async move {
            crate::service::InsightService::column_rule_briefs(None, &column_for_task, kind)
        });
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Ok(briefs) = task.await {
                let _ = weak.update(cx, |panel, cx| {
                    // 取到后再查一次：并发触发下后到的写覆盖同值（幂等）
                    panel.col_rules.borrow_mut().insert(column, briefs);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
