//! 表探查（Tab「表」，Phase 3.1）：列元数据表 + 行数 + 评估入口 + 表级质量
//!
//! 列名是下钻热点（点它切到该列的「列」Tab）——表 → 列是用户最常走的下一步。
//! 「评估全表」逐列串行（避免撞后端并发上限），进度一列列回填；已经算出来的列分数
//! 不清空（清空会让面板闪一下空白）。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, IconName, Sizable as _, Theme};
use gpui_kit::*;

use super::{grade_color, ratio_bar};
use crate::insight_view::InsightView;
use crate::model::{ColumnKind, InsightTarget, TableColumnView, TableProfileView};
use crate::ui;

impl InsightView {
    /// 表探查（Tab「表」，Phase 3.1）：列元数据表 + 行数 + 评估入口 + 表级质量。
    ///
    /// 列名是下钻热点（点它切到该列的「列」Tab）——表→列是用户最常走的下一步。
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

        // 评估入口（串行逐列；评估中禁用，避免重复排队撞并发上限）
        body = body.child(
            div().h_flex().w_full().gap_2().child(div().flex_1()).child(
                Button::new("insight-eval-table")
                    .small()
                    .icon(if evaluating {
                        IconName::RotateCw
                    } else {
                        IconName::TriangleAlert
                    })
                    .label(if evaluating { "评估中…" } else { "评估全表" })
                    .disabled(evaluating || profile.columns.is_empty())
                    .tooltip("逐列串行评分（避免撞后端并发上限），进度会一列列回填")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| view.request_table_evaluation(cx))
                        }
                    }),
            ),
        );

        // 表级质量：评估完才有（没评估就是没有，不是 0 分）
        if let Some(quality) = &profile.quality {
            let color = grade_color(quality.grade, theme);
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .p_2()
                    .rounded_sm()
                    .bg(colors.list_hover)
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(rems(ui::INSIGHT_SCORE_FONT * 0.75))
                                    .text_color(color)
                                    .child(format!("{:.0}", quality.overall)),
                            )
                            .child(div().text_xs().text_color(color).child(quality.grade.label()))
                            .child(div().flex_1().min_w_0().text_xs().text_ellipsis().child(
                                if quality.problem_columns > 0 {
                                    format!("{} 列需关注", quality.problem_columns)
                                } else {
                                    "无问题列".to_string()
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(quality.summary.clone()),
                    ),
            );
        }

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
                        progress.ratio(),
                        ui::INSIGHT_RATIO_BAR_HEIGHT,
                        colors.primary,
                        theme,
                    )),
            );
        }

        // 列元数据表：# 列名（PK） / 类型 / 可空 / 质量
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
                .child("行数与质量分基于全量统计；样本值为前 5 行，直方图至少 10 行"),
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
                .w(rems(ui::INSIGHT_TABLE_QUALITY_WIDTH))
                .child("质量"),
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
    let quality = match (column.score, column.grade()) {
        (Some(score), Some(grade)) => div()
            .text_color(grade_color(grade, theme))
            .child(format!("{:.0} {}", score, grade.label())),
        _ => div().text_color(colors.muted_foreground).child("—"),
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
                                    Some(InsightTarget::Table { temp_table, .. }) => temp_table.clone(),
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
                .w(rems(ui::INSIGHT_TABLE_QUALITY_WIDTH))
                .text_xs()
                .child(quality),
        );

    // 类型未识别：给一个可点的提示（不是所有列都能算质量）
    if column.kind == ColumnKind::Unknown {
        row = row.child(
            Icon::new(IconName::Info)
                .size(inline_icon)
                .text_color(colors.muted_foreground),
        );
    }
    row
}
