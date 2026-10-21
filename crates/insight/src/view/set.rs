//! 右栏「已洞察表」集合管理卡（T18）+ 关系排查结果区（T19）。
//!
//! 集合 = 中间工作页 `opened` 的快照（只有进入过洞察的表才入列，用户口径
//! 「只有进入洞察后的表才能选择对比」）。卡放 body 滚动区顶部：它是会话级
//! 管理入口，先于当前目标的 Tab 下钻可见。
//!
//! 行操作三件：**对比**（转发到中间页 Compare 形态）、**关系**（与当前目标
//! 做列值重叠排查，T19）、**归档**（画像快照 → 资产库，T20）。关系 / 归档
//! 都以洞察临时表名为 SQL 前提，条目未取样（`temp_table = None`）时按钮置灰。

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{IconName, Sizable as _, Theme};use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::insight_view::{InsightEvent, InsightView};
use crate::model::{fmt_rows, InsightSetKind, OverlapView};
use crate::ui;

impl InsightView {
    /// 集合卡：头（折叠按钮 + 计数）+ Collapsible 列表 + 关系排查结果区。
    ///
    /// 无集合时给一句引导（说的是「去哪儿进洞察」，不是一句「无数据」）。
    pub(crate) fn render_insight_set(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        // 工作集提示：集合内表超过设置阈值时在头行尾给出警告（集合表跳过 TTL /
        // 上限清理，长期占用分析引擎内存——这正是设置「工作集提示阈值」的用途）。
        let over_limit = self.work_set_limit > 0 && self.insight_set.len() > self.work_set_limit;
        let count_label = if self.work_set_limit > 0 {
            format!("已洞察表 {} / {}", self.insight_set.len(), self.work_set_limit)
        } else {
            format!("已洞察表 {}", self.insight_set.len())
        };
        let head = Button::new("insight-set-toggle")
            .ghost()
            .xsmall()
            .icon(if self.insight_set_open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .label(count_label)
            .on_click({
                let entity = entity.clone();
                move |_, _, app| {
                    entity.update(app, |view, cx| view.toggle_insight_set(cx));
                }
            });
        let mut body = div().v_flex().gap_1().w_full().pt_1();
        if self.insight_set.is_empty() {
            body = body.child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("进入过洞察的表会列在这里，可对比 / 关系排查 / 归档"),
            );
        } else {
            for entry in &self.insight_set {
                body = body.child(self.render_set_row(entry, entity, theme));
            }
        }
        div()
            .w_full()
            .v_flex()
            .gap_1()
            .child(
                div()
                    .w_full()
                    .border_b_1()
                    .border_color(colors.border)
                    .pb_2()
                    .child(
                        div()
                            .w_full()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .child(head)
                            .when(over_limit, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(colors.warning)
                                        .child(format!(
                                            "超 {} 张，建议先清理（设置可调）",
                                            self.insight_set.len() - self.work_set_limit
                                        )),
                                )
                            }),
                    )
                    .child(
                        Collapsible::new()
                            .open(self.insight_set_open)
                            .motion_id("insight-set")
                            .content(body),
                    ),
            )
            .when(
                self.overlap.is_some() || self.overlap_running || self.overlap_notice.is_some(),
                |card| card.child(self.render_overlap_zone(entity, theme)),
            )
    }

    /// 集合行：来源徽标 + 表名 + 行数 + 对比 / 关系 / 归档；当前目标高亮。
    fn render_set_row(
        &self,
        entry: &crate::model::InsightSetEntry,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let badge = match entry.kind {
            InsightSetKind::SourceTable => Tag::info(),
            InsightSetKind::ResultSet => Tag::warning(),
            InsightSetKind::File => Tag::secondary(),
        };
        let mut row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_sm()
            .h(rems(ui::ROW_HEIGHT))
            .when(entry.is_current, |row| {
                row.bg(colors.list_active)
            })
            .child(
                badge
                    .outline()
                    .xsmall()
                    .flex_none()
                    .child(entry.kind.label()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_xs()
                    .text_color(if entry.is_current {
                        colors.foreground
                    } else {
                        colors.muted_foreground
                    })
                    .text_ellipsis()
                    .child(entry.label.clone()),
            );
        if let Some(rows) = entry.row_count {
            row = row.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(fmt_rows(rows)),
            );
        }
        // 检查器命中计数（原型右栏行「检查器 N」）：评估全表后才有，未评估不显示
        if let (Some(applicable), Some(hits)) = (entry.checker_applicable, entry.checker_hits) {
            row = row.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if applicable > 0 && hits == applicable {
                        colors.success
                    } else if applicable > 0 && hits == 0 {
                        colors.danger
                    } else {
                        colors.muted_foreground
                    })
                    .child(format!("检查器 {hits}/{applicable}")),
            );
        }
        // 三个行操作的按钮实体分开建（各自持有自己的 key 克隆与事件）
        let entity_compare = entity.clone();
        let entity_relation = entity.clone();
        let entity_archive = entity.clone();
        let key_compare = entry.key.clone();
        let key_relation = entry.key.clone();
        let key_archive = entry.key.clone();
        let sampled = entry.temp_table.is_some();
        row.child(
            Button::new(SharedString::from(format!("insight-set-compare-{}", entry.key)))
                .ghost()
                .xsmall()
                .label("对比")
                .on_click(move |_, _, app| {
                    entity_compare.update(app, |_view, cx| {
                        cx.emit(InsightEvent::CompareRequested { key: key_compare.clone() });
                    });
                }),
        )
        .child(
            Button::new(SharedString::from(format!("insight-set-relation-{}", entry.key)))
                .ghost()
                .xsmall()
                .label("关系")
                .disabled(!sampled)
                .on_click(move |_, _, app| {
                    entity_relation.update(app, |_view, cx| {
                        cx.emit(InsightEvent::RelationRequested { key: key_relation.clone() });
                    });
                }),
        )
        .child(
            Button::new(SharedString::from(format!("insight-set-archive-{}", entry.key)))
                .ghost()
                .xsmall()
                .label("归档")
                .disabled(!sampled)
                .on_click(move |_, _, app| {
                    entity_archive.update(app, |_view, cx| {
                        cx.emit(InsightEvent::ArchiveRequested { key: key_archive.clone() });
                    });
                }),
        )
    }

    /// 关系排查结果区（集合卡下方）：loading / 失败 / 结果三态。
    fn render_overlap_zone(&self, entity: &Entity<Self>, theme: &Theme) -> Div {        let colors = theme.colors;
        let mut zone = div()
            .w_full()
            .v_flex()
            .gap_1()
            .px_1()
            .py_1()
            .rounded_sm()
            .bg(colors.sidebar)
            .border_1()
            .border_color(colors.border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(colors.foreground)
                            .child("关系排查"),
                    )
                    .child(
                        Button::new("insight-overlap-dismiss")
                            .ghost()
                            .xsmall()
                            .label("收起")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |view, cx| view.dismiss_overlap(cx));
                                }
                            }),
                    ),
            );
        if self.overlap_running {
            zone = zone.child(
                div().h_flex().items_center().gap_1().px_1().child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("正在对比列值重叠…"),
                ),
            );
        } else if let Some(notice) = &self.overlap_notice {
            zone = zone.child(
                div()
                    .px_1()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(notice.clone()),
            );
        } else if let Some(view) = &self.overlap {
            zone = zone.child(self.render_overlap_rows(view, theme));
        }
        zone
    }

    /// 重叠结果明细：两表名 + 每列「重叠数 / 占比」。
    fn render_overlap_rows(&self, view: &OverlapView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let mut body = div().v_flex().gap_1().px_1();
        body = body.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("{} ⟷ {}", view.table_a, view.table_b)),
        );
        if view.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("两表没有类型兼容的同名列，无法做值重叠对比"),
            );
            return body;
        }
        for row in &view.rows {
            body = body.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_xs()
                            .text_ellipsis()
                            .child(row.column.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(colors.foreground)
                            .child(format!(
                                "{}  · {:.0}% / {:.0}%",
                                fmt_rows(row.overlap.max(0) as u64),
                                row.rate_a * 100.0,
                                row.rate_b * 100.0
                            )),
                    ),
            );
        }
        body.child(
            div()
                .pt_1()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("百分比 = 重叠值占该表去重值域的比例"),
        )
    }
}
