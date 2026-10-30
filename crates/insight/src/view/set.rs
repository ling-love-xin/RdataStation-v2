//! 右栏「已洞察表」集合（对齐 EDA 原型 it-head / it-list / it-actions / it-note，
//! 2026-10-30 重构）+ 关系排查结果区（T19）。
//!
//! 集合 = 中间工作页 `opened` 的快照（只有进入过洞察的表才入列，用户口径
//! 「只有进入洞察后的表才能选择对比」）。卡放 body 滚动区顶部：它是会话级
//! 管理入口，先于选中目标的下钻可见。
//!
//! 版式对齐 `insight-eda-prototype.html`：
//! - `it-head`「已洞察表」+ 计数（`itCount`）+ 右侧 mini-switch「多选」；
//! - `it-list`：卡片行 r1 = 多选框（仅多选显）+ 等宽名 + 行数（等宽 dim）；
//!   r2 = 来源 + 对比 / 关系 / 归档；`cur` = 当前选中描边（原型 coral → 主题 primary）；
//! - `it-actions`「质量对比」：仅勾选 ≥2 张时显示（对齐原型「轻量对比（至少勾选 2 张）」）；
//! - `it-note`：洞察对象统一为 DuckDB 临时表的说明 + 集合超阈提示。
//!
//! 行操作三件：**对比**（转发到中间页 Compare 形态）、**关系**（与当前目标
//! 做列值重叠排查，T19）、**归档**（画像快照 → 资产库，T20）。关系 / 归档
//! 都以洞察临时表名为 SQL 前提，条目未取样（`temp_table = None`）时按钮置灰。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{Sizable as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::insight_view::{InsightEvent, InsightView};
use crate::model::{InsightSetEntry, InsightSetKind, OverlapView, fmt_rows};
use crate::ui;

impl InsightView {
    /// 集合卡（对齐原型 it-head → it-list → it-actions → it-note）+ 关系排查结果区。
    ///
    /// 无集合时给一句引导（说的是「去哪儿进洞察」，不是一句「无数据」）。
    pub(crate) fn render_insight_set(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        // 集合超阈提示：集合内表超过设置阈值时提示（集合表跳过 TTL / 上限清理，
        // 长期占用分析引擎内存——这正是设置「工作集提示阈值」的用途）
        let over_limit = self.work_set_limit > 0 && self.insight_set.len() > self.work_set_limit;
        let selected_count = self.selected_tables.len();

        div()
            .w_full()
            .v_flex()
            .gap_1()
            // it-head：已洞察表 + 计数 + 多选开关
            .child(
                div()
                    .w_full()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child("已洞察表"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{}", self.insight_set.len())),
                    )
                    .child(div().flex_1())
                    // 多选：原型 mini-switch；开启后行首出现勾选框，勾选 ≥2 出质量对比
                    .child(
                        Switch::new("insight-set-multi")
                            .checked(self.set_multi)
                            .label("多选")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |view, cx| view.toggle_multi_mode(cx));
                                }
                            }),
                    ),
            )
            // it-list：卡片行
            .child(
                div()
                    .w_full()
                    .v_flex()
                    .gap_1()
                    .when(self.insight_set.is_empty(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child("进入过洞察的表会列在这里，可对比 / 关系排查 / 归档"),
                        )
                    })
                    .children(
                        self.insight_set
                            .iter()
                            .map(|entry| self.render_set_row(entry, entity, theme)),
                    ),
            )
            // it-actions：质量对比（仅多选勾选 ≥2 张时显示）
            .when(selected_count >= 2, |zone| {
                zone.child(
                    div()
                        .w_full()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(format!("已勾选 {selected_count} 张表")),
                        )
                        .child(
                            Button::new("insight-set-quality-compare")
                                .secondary()
                                .xsmall()
                                .label("质量对比")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _, app| {
                                        entity.update(app, |view, cx| {
                                            view.request_quality_compare(cx);
                                        });
                                    }
                                }),
                        ),
                )
            })
            // it-note：洞察对象语义 + 超阈提示
            .child(
                div()
                    .w_full()
                    .v_flex()
                    .gap_0p5()
                    .px_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(
                                "洞察对象统一为 DuckDB 临时表（源表取样 / 结果集 / CSV 映射）。\
                                 勾选多张表可做质量对比；表间可关系排查（值重叠）；\
                                 需要持久化的归档到分析存档。",
                            ),
                    )
                    .when(over_limit, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(colors.warning)
                                .child(format!(
                                    "集合超 {} 张上限，建议先归档清理（设置可调）",
                                    self.work_set_limit
                                )),
                        )
                    }),
            )
            // 关系排查结果区（it-note 之下，同属集合管理语境）
            .when(
                self.overlap.is_some() || self.overlap_running || self.overlap_notice.is_some(),
                |card| card.child(self.render_overlap_zone(entity, theme)),
            )
    }

    /// 集合行（对齐原型 it-row 卡片）：r1 = 多选框 + 名称 + 行数；r2 = 来源 + 操作。
    ///
    /// 多选框仅在多选模式（`it-head` 开关）下出现；当前选中目标用主题描边 +
    /// 列表激活底色表达（原型 `.cur` coral 描边，真机走主题 token）。
    fn render_set_row(
        &self,
        entry: &InsightSetEntry,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let selected = self.selected_tables.contains(&entry.key);
        div()
            .w_full()
            .v_flex()
            .gap_0p5()
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(if entry.is_current {
                colors.primary
            } else {
                colors.border
            })
            .bg(if entry.is_current || selected {
                colors.list_active
            } else {
                colors.sidebar
            })
            // r1：多选框（仅多选）+ 等宽名 + 行数（等宽 dim）
            .child(
                div()
                    .w_full()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(self.set_multi, |row| {
                        row.child(
                            Checkbox::new(SharedString::from(format!(
                                "insight-set-sel-{}",
                                entry.key
                            )))
                            .checked(selected)
                            .on_click({
                                let entity = entity.clone();
                                let key = entry.key.clone();
                                move |_, _, app| {
                                    entity.update(app, |view, cx| {
                                        view.toggle_table_selection(&key, cx);
                                    });
                                }
                            }),
                        )
                    })
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
                    )
                    .when_some(entry.row_count, |row, rows| {
                        row.child(
                            div()
                                .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .flex_none()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(fmt_rows(rows)),
                        )
                    }),
            )
            // r2：来源 + 检查器命中 + 对比 / 关系 / 归档
            .child(
                div()
                    .w_full()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(self.source_badge(entry.kind))
                    .when_some(
                        entry
                            .checker_applicable
                            .zip(entry.checker_hits)
                            .filter(|(a, _)| *a > 0),
                        |row, (applicable, hits)| {
                            row.child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(if hits == applicable {
                                        colors.success
                                    } else if hits == 0 {
                                        colors.danger
                                    } else {
                                        colors.muted_foreground
                                    })
                                    .child(format!("检查器 {hits}/{applicable}")),
                            )
                        },
                    )
                    .child(div().flex_1())
                    // 三个行操作的按钮实体分开建（各自持有自己的 key 克隆与事件）
                    .child(self.row_op_button(
                        entry,
                        "对比",
                        "insight-set-compare",
                        entity,
                        false,
                        InsightSetOp::Compare,
                    ))
                    .child(self.row_op_button(
                        entry,
                        "关系",
                        "insight-set-relation",
                        entity,
                        entry.temp_table.is_none(),
                        InsightSetOp::Relation,
                    ))
                    .child(self.row_op_button(
                        entry,
                        "归档",
                        "insight-set-archive",
                        entity,
                        entry.temp_table.is_none(),
                        InsightSetOp::Archive,
                    )),
            )
    }

    /// 来源徽标（SourceTable / ResultSet / File → 语义色 Tag）
    fn source_badge(&self, kind: InsightSetKind) -> impl IntoElement {
        let badge = match kind {
            InsightSetKind::SourceTable => Tag::info(),
            InsightSetKind::ResultSet => Tag::warning(),
            InsightSetKind::File => Tag::secondary(),
        };
        badge.outline().xsmall().flex_none().child(kind.label())
    }

    /// 行操作按钮（对比 / 关系 / 归档）：key 与实体在闭包外克隆，避免借用冲突。
    fn row_op_button(
        &self,
        entry: &InsightSetEntry,
        label: &'static str,
        id_prefix: &'static str,
        entity: &Entity<Self>,
        disabled: bool,
        op: InsightSetOp,
    ) -> Button {
        let entity = entity.clone();
        let key = entry.key.clone();
        Button::new(SharedString::from(format!("{id_prefix}-{key}")))
            .ghost()
            .xsmall()
            .label(label)
            .disabled(disabled)
            .on_click(move |_, _, app| {
                entity.update(app, |_view, cx| match op {
                    InsightSetOp::Compare => {
                        cx.emit(InsightEvent::CompareRequested { key: key.clone() });
                    }
                    InsightSetOp::Relation => {
                        cx.emit(InsightEvent::RelationRequested { key: key.clone() });
                    }
                    InsightSetOp::Archive => {
                        cx.emit(InsightEvent::ArchiveRequested { key: key.clone() });
                    }
                });
            })
    }

    /// 关系排查结果区（集合卡下方）：loading / 失败 / 结果三态。
    fn render_overlap_zone(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
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

/// 集合行操作类型（对比 / 关系 / 归档）——按钮回调按此分派事件
#[derive(Clone, Copy)]
enum InsightSetOp {
    Compare,
    Relation,
    Archive,
}
