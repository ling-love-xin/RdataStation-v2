//! 表探查（Tab「表」，Phase 3.1）：列元数据表 + 行数 + 评估入口。
//!
//! 列名是下钻热点（点它切到该列的「列」Tab）——表 → 列是用户最常走的下一步。
//! 「评估全表」逐列串行（避免撞后端并发上限），进度一列列回填；已经算出来的列明细
//! 不清空（清空会让面板闪一下空白）。本视图只呈现事实统计，不合成质量总分或等级。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ratio_bar;
use crate::insight_view::InsightView;
use crate::model::{ColumnKind, InsightTarget, TableColumnView, TableProfileView, fmt_num};
use crate::ui;

impl InsightView {
    /// 表探查（Tab「表」，Phase 3.1）：列元数据表 + 行数 + 评估入口。
    ///
    /// 列名是下钻热点（点它切到该列的「列」Tab）——表→列是用户最常走的下一步。
    ///
    /// 2026-10-30 右栏对齐 EDA 原型后不再由右栏渲染（表画像卡由下钻区
    /// `drill.rs` 承担）；保留供中间页 / 资产库复用，勿删。
    #[allow(dead_code)]
    pub(in crate::view) fn render_table_profile(
        &self,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let evaluating = profile.progress.is_some();

        let mut body = div().v_flex().w_full().gap_2();

        // 评估入口（串行逐列；评估中禁用，避免重复排队撞并发上限）+
        // C5 取消入口（评估中显示：停止派发、已完成列保留、未完成列灰态可补算）
        body = body.child(
            div().h_flex().w_full().gap_2().when(evaluating, |row| {
                row.child(
                    Button::new("insight-eval-cancel")
                        .small()
                        .ghost()
                        .label("取消")
                        .tooltip("停止评估：已完成列保留，未完成列灰态可补算")
                        .on_click({
                            let entity = entity.clone();
                            move |_, _, app| {
                                entity.update(app, |view, cx| view.cancel_table_evaluation(cx))
                            }
                        }),
                )
            }).child(div().flex_1()).child(
                Button::new("insight-eval-table")
                    .small()
                    .icon(if evaluating {
                        IconName::RotateCw
                    } else {
                        IconName::TriangleAlert
                    })
                    .label(if evaluating {
                        "评估中…"
                    } else {
                        "评估全表"
                    })
                    .disabled(evaluating || profile.columns.is_empty())
                    .tooltip("逐列串行统计（避免撞后端并发上限），进度会一列列回填")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| view.request_table_evaluation(cx))
                        }
                    }),
            ),
        );

        // 评估对象提示（建议② / 决策文档 Q1 语义）：评估全表 = 确认窗口生成的表；
        // 取样是对评估全表的取样，不仅数据量还有字段
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_start()
                .gap_1()
                .child(
                    Icon::new(IconName::Info)
                        .with_size(Size::Size(inline_icon))
                        .text_color(colors.muted_foreground),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(format!(
                            "评估全表 = 确认窗口生成的表（{} 行 × {} 列）；取样是对评估全表的取样，不仅数据量还有字段",
                            fmt_num(profile.row_count as f64),
                            profile.columns.len()
                        )),
                ),
        );

        // 进度行（进行中）：进度条 + 文字，让用户知道还在动
        if let Some(progress) = profile.progress {
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("正在评估 {}/{} 列…", progress.done, progress.total)),
                    )
                    .child(ratio_bar(
                        "insight-eval-progress",
                        progress.ratio(),
                        Size::Small,
                        colors.primary,
                    )),
            );
        }

        // 列元数据表：# 列名（PK） / 类型 / 可空 / 缺失率
        body = body.child(table_header_row(theme));
        if profile.columns.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("该临时表没有可见的列"),
            );
        }
        for column in &profile.columns {
            body = body.child(table_column_row(
                profile,
                column,
                entity,
                theme,
                inline_icon,
            ));
        }

        // 采样口径必须常驻：口径写错比不写更坏——这里写的是**实际**口径
        body.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("行数与缺失率基于全量统计；样本值为前 5 行，直方图至少 10 行"),
        )
    }
}

/// 表探查的表头行
fn table_header_row(theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .py_0p5()
        .bg(colors.list_hover)
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(div().w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH)).child("#"))
        .child(div().flex_1().min_w_0().child("列名"))
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_TYPE_WIDTH))
                .text_ellipsis()
                .child("类型"),
        )
        .child(div().w(rems(ui::INSIGHT_TABLE_FLAG_WIDTH)).child("可空"))
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_NULL_RATE_WIDTH))
                .text_ellipsis()
                .child("缺失率"),
        )
}

/// 表探查的一行：列名为下钻热点（点击切到该列的「列」Tab）
fn table_column_row(
    profile: &TableProfileView,
    column: &TableColumnView,
    entity: &Entity<InsightView>,
    theme: &Theme,
    inline_icon: Pixels,
) -> Div {
    let colors = theme.colors;
    // 缺失率事实：未统计为 `None` 时显示中性占位，不编假值
    let null_rate_cell = match column.null_rate {
        Some(rate) => div()
            .text_xs()
            .text_color(colors.foreground)
            .child(format!("{:.1}%", rate * 100.0)),
        None => div()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child("—"),
    };

    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::ROW_HEIGHT))
        .px_1()
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH))
                .text_color(colors.muted_foreground)
                .child(column.index.to_string()),
        )
        .child(
            div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    // 列名用 Button（ghost、无内距）：白搓 div 会丢 hover / 键盘 / a11y
                    Button::new(ElementId::Name(
                        format!("insight-table-col-{}", column.name).into(),
                    ))
                    .ghost()
                    .xsmall()
                    .label(column.name.clone())
                    .tooltip("看这一列的画像")
                    .on_click({
                        let entity = entity.clone();
                        let next = column.clone();
                        let temp_table = profile.table_name.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| {
                                // 表头里的临时表名与目标一致：目标已换时以当前目标为准
                                let temp = match &view.target {
                                    Some(InsightTarget::Table { temp_table, .. }) => {
                                        temp_table.clone()
                                    }
                                    _ => temp_table.clone(),
                                };
                                view.set_target(
                                    InsightTarget::Column {
                                        temp_table: temp,
                                        column: next.name.clone(),
                                        data_type: next.data_type.clone(),
                                    },
                                    cx,
                                );
                            });
                        }
                    }),
                )
                .child(if column.primary_key {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.info)
                        .child("PK")
                } else {
                    div()
                }),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_TYPE_WIDTH))
                .text_xs()
                .text_color(colors.muted_foreground)
                .text_ellipsis()
                .child(column.data_type.clone()),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_FLAG_WIDTH))
                .text_xs()
                .text_color(if column.nullable {
                    colors.muted_foreground
                } else {
                    colors.foreground
                })
                .child(if column.nullable { "YES" } else { "NO" }),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_NULL_RATE_WIDTH))
                .text_xs()
                .child(null_rate_cell),
        );

    // 类型未识别：给一个提示（不是所有列都能识别类型并统计分布）
    if column.kind == ColumnKind::Unknown {
        row = row.child(
            Icon::new(IconName::Info)
                .size(inline_icon)
                .text_color(colors.muted_foreground),
        );
    }
    row
}
