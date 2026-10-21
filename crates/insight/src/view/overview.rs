//! 右栏「摘要」Tab（D66 对齐原型右栏六 Tab 的 Overview：首置，汇总当前目标的画像摘要）。
//!
//! 版式对齐 `insight-workspace-prototype.html` 的 `renderRightBody` overview 分支：
//! - 表目标 → 三张摘要卡：①表名 / 来源 + kv（总行数 / 列数 / 取样）
//!   ②质量评分（68px 评分环 + 等级 + 四维条）③质量提示（前 2 条）。
//! - 列目标 → 列摘要卡（列名 + 类型徽标 + 总行数 / 空值率 + 列质量环 + 维度条）。
//! - 其它载荷 / 无目标 → `None`（走空态引导，不编假摘要）。
//!
//! 真实字段口径与中间工作页一致：四维从列元数据现算；未评估 = 空环 + 引导，
//! 不显示「0 分」。摘要**不触发取数**（`emit_request_for_tab` 对 Overview 返回
//! `None`），只用已加载的载荷渲染。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::progress::{Progress, ProgressCircle};
use gpui_kit::component::{Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::insight_view::InsightView;
use crate::model::{ColumnCheckState, ColumnKind, ColumnProfileView, TableProfileView};
use crate::quality_scorer::Grade;
use crate::ui;

use super::{grade_color, kind_badge};

impl InsightView {
    /// 摘要 Tab 的载荷：表 / 列画像任一份已有就渲染对应摘要；都没有 → 空态。
    pub(crate) fn render_overview(
        &self,
        _entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        ring_px: Pixels,
    ) -> Option<AnyElement> {
        if let Some(profile) = self.data.as_table() {
            let source = self
                .target
                .as_ref()
                .and_then(|t| t.detail())
                .unwrap_or_else(|| profile.source_label.into());
            return Some(
                overview_table(profile, source, theme, inline_icon, ring_px)
                    .into_any_element(),
            );
        }
        if let Some(profile) = self.data.as_column() {
            return Some(overview_column(profile, theme, inline_icon, ring_px).into_any_element());
        }
        None
    }
}

/// 表目标摘要（原型三卡：概要 / 质量评分 / 质量提示）
fn overview_table(
    profile: &TableProfileView,
    source: String,
    theme: &Theme,
    inline_icon: Pixels,
    ring_px: Pixels,
) -> Div {
    let colors = theme.colors;
    let quality = profile.quality.as_ref();
    let total = profile.columns.len().max(1);

    // 四维条（与中间工作页同口径：需关注 / 可空 / 主键 / 未识别，从列元数据现算）
    let dims: Vec<(String, f64, Hsla)> = {
        let problem = quality.map(|q| q.problem_columns).unwrap_or(0);
        let nullable = profile.columns.iter().filter(|c| c.nullable).count();
        let pk = profile.columns.iter().filter(|c| c.primary_key).count();
        let unknown = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Unknown)
            .count();
        vec![
            (
                "需关注".into(),
                (problem as f64 / total as f64) * 100.0,
                colors.danger,
            ),
            (
                "可空列".into(),
                (nullable as f64 / total as f64) * 100.0,
                colors.warning,
            ),
            (
                "主键列".into(),
                (pk as f64 / total as f64) * 100.0,
                colors.success,
            ),
            (
                "未识别".into(),
                (unknown as f64 / total as f64) * 100.0,
                colors.muted_foreground,
            ),
        ]
    };

    // 质量提示：前 2 条（真实数据，不编多条）
    let mut notes = Vec::new();
    if let Some(q) = quality {
        notes.push(q.summary.clone());
        if q.problem_columns > 0 {
            notes.push(format!("{} 列需关注（评分低于阈值）", q.problem_columns));
        }
        if q.scored_columns == 0 {
            notes.push("暂无可评分的列（类型均未识别）".into());
        }
    }

    // 检查器命中环（原型右栏）：事实计数，不做「良好 / 较差」评级（评级属资产库审计侧）
    let checker_applicable = quality.map(|q| q.checker_applicable).unwrap_or(0);
    let checker_hits = quality.map(|q| q.checker_hits).unwrap_or(0);
    let ring_value = if checker_applicable > 0 {
        checker_hits as f32 / checker_applicable as f32 * 100.0
    } else {
        0.0
    };
    let ring_color = if checker_applicable == 0 {
        colors.muted_foreground
    } else if checker_hits == checker_applicable {
        colors.success
    } else if checker_hits == 0 {
        colors.danger
    } else {
        colors.warning
    };
    // 检查器列表（四态）：命中 / 未命中 / 不适用-类型；列排除态接入后自动出现
    let checker_rows: Vec<(String, String, Hsla)> = quality
        .map(|q| q.checker.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let (label, color) = match c.state {
                ColumnCheckState::Hit => ("命中".into(), colors.success),
                ColumnCheckState::Miss => ("未命中".into(), colors.danger),
                ColumnCheckState::NotApplicableType => ("不适用".into(), colors.muted_foreground),
                ColumnCheckState::Excluded => ("排除".into(), colors.muted_foreground),
            };
            (format!("{} · {}", c.column, c.name), label, color)
        })
        .collect();

    div().v_flex().w_full().gap_2()
        // 卡①概要：表名 + 来源 + kv
        .child(
            summary_card(theme)
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(IconName::FileText).size(inline_icon))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.foreground)
                                .text_ellipsis()
                                .child(profile.table_name.clone()),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .text_ellipsis()
                                .child(source),
                        ),
                )
                .child(kv_row("总行数", format!("{}", profile.row_count), theme))
                .child(kv_row("列数", profile.columns.len().to_string(), theme))
                .child(kv_row("取样", crate::service::persistence::SOURCE_SAMPLE_LIMIT.to_string(), theme)),
        )
        // 卡②检查器命中：68px 环（适用/命中计数，无评级）+ 检查器四态列表 + 四维条
        .child(
            summary_card(theme)
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(IconName::Eye).size(inline_icon))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.foreground)
                                .child("检查器命中"),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_xs()
                                .text_color(match quality {
                                    Some(q) if q.checker_applicable > 0 => ring_color,
                                    _ => colors.muted_foreground,
                                })
                                .child(match quality {
                                    Some(q) if q.checker_applicable > 0 => {
                                        format!("{}/{}", q.checker_hits, q.checker_applicable)
                                    }
                                    Some(_) => "无不适用检查".to_string(),
                                    None => "未评估".to_string(),
                                }),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .v_flex()
                        .items_center()
                        .gap_1()
                        .py_1()
                        .child(
                            ProgressCircle::new("insight-overview-ring")
                                .value(ring_value)
                                .color(ring_color)
                                .with_size(Size::Size(ring_px))
                                .accessibility_label("检查器命中")
                                .child(
                                    div()
                                        .text_size(rems(ui::INSIGHT_SCORE_FONT * 0.6))
                                        .text_color(ring_color)
                                        .child(match quality {
                                            Some(_) => format!("{checker_hits}/{checker_applicable}"),
                                            None => "—".into(),
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(match quality {
                                    Some(q) if q.checker_applicable > 0 => {
                                        format!("适用 {} 条 · 命中 {} 条", q.checker_applicable, q.checker_hits)
                                    }
                                    Some(_) => "评估全表后统计检查器命中".to_string(),
                                    None => "评估全表后统计检查器命中".to_string(),
                                }),
                        ),
                )
                .when(!checker_rows.is_empty(), |card| {
                    card.child(
                        div()
                            .w_full()
                            .v_flex()
                            .gap_0p5()
                            .children(checker_rows.into_iter().take(6).map(|(name, label, color)| {
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(color)
                                            .child(label),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .text_xs()
                                            .text_color(colors.foreground)
                                            .text_ellipsis()
                                            .child(name),
                                    )
                            })),
                            )
                        })
                        .child(div().w_full().v_flex().gap_1().children(dims.into_iter().map(
                    |(name, ratio, fill)| {
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                    .flex_none()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(name.clone()),
                            )
                            .child(
                                Progress::new(SharedString::from(format!(
                                    "insight-overview-dim-{name}"
                                )))
                                .value(ratio as f32)
                                .color(fill)
                                .with_size(Size::Small)
                                .flex_1(),
                            )
                    },
                ))),
        )
        // 卡③质量提示（前 2 条）
        .child(
            summary_card(theme)
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(IconName::TriangleAlert).size(inline_icon))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.foreground)
                                .child("质量提示"),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .v_flex()
                        .gap_0p5()
                        .children(notes.iter().map(|note| {
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(format!("· {note}"))
                        }))
                        .when(notes.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("· 评估全表后查看质量提示"),
                            )
                        }),
                ),
        )
}

/// 列目标摘要：列名 + 类型 + 总行数 / 空值率 + 列质量环 + 维度条 + 结论
fn overview_column(
    profile: &ColumnProfileView,
    theme: &Theme,
    inline_icon: Pixels,
    ring_px: Pixels,
) -> Div {
    let colors = theme.colors;
    let score = profile.score.as_ref();
    let null_rate = format!("{:.1}%", profile.null_rate * 100.0);

    div().v_flex().w_full().gap_2()
        .child(
            summary_card(theme)
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(IconName::FileText).size(inline_icon))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.foreground)
                                .text_ellipsis()
                                .child(profile.column.clone()),
                        )
                        .child(div().flex_1())
                        .child(kind_badge(profile.kind)),
                )
                .child(kv_row("总行数", profile.total_count.to_string(), theme))
                .child(kv_row("空值率", null_rate, theme))
                .child(kv_row("样本", profile.sample.len().to_string(), theme)),
        )
        .child(
            summary_card(theme)
                .child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(IconName::Eye).size(inline_icon))
                        .child(
                            div()
                                .text_sm()
                                .text_color(colors.foreground)
                                .child("列质量评分"),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_xs()
                                .text_color(match score {
                                    Some(s) => grade_color(s.grade, theme),
                                    None => colors.muted_foreground,
                                })
                                .child(match score {
                                    Some(s) => s.grade.label(),
                                    None => "—",
                                }),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .v_flex()
                        .items_center()
                        .gap_1()
                        .py_1()
                        .child(
                            ProgressCircle::new("insight-overview-column-ring")
                                .value(score.map(|s| s.overall as f32).unwrap_or(0.0))
                                .color(match score {
                                    Some(s) => grade_color(s.grade, theme),
                                    None => colors.muted_foreground,
                                })
                                .with_size(Size::Size(ring_px))
                                .accessibility_label("列质量评分")
                                .child(
                                    div()
                                        .text_size(rems(ui::INSIGHT_SCORE_FONT * 0.6))
                                        .text_color(match score {
                                            Some(s) => grade_color(s.grade, theme),
                                            None => colors.muted_foreground,
                                        })
                                        .child(match score {
                                            Some(s) => format!("{:.0}", s.overall),
                                            None => "—".into(),
                                        }),
                                ),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .v_flex()
                        .gap_1()
                        .children(score.map(|s| s.dimensions.clone()).unwrap_or_default().into_iter().map(
                            |dim| {
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(dim.name.clone()),
                                    )
                                    .child(
                                        Progress::new(SharedString::from(format!(
                                            "insight-overview-coldim-{}",
                                            dim.name
                                        )))
                                        .value(dim.score as f32)
                                        .color(grade_color(Grade::of(dim.score), theme))
                                        .with_size(Size::Small)
                                        .flex_1(),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(format!("{:.0}", dim.score)),
                                    )
                            },
                        ))
                        .when(score.is_none(), |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("该列无评分数据（列全空或未评估）"),
                            )
                        }),
                ),
        )
        .when_some(score, |this, s| {
            this.child(
                summary_card(theme).child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(format!("· {}", s.summary)),
                ),
            )
        })
}

/// 摘要卡容器（原型 .summary-card）
fn summary_card(theme: &Theme) -> Div {
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(theme.colors.border)
}

/// 卡内键值行（原型 .kv）
fn kv_row(key: &str, value: String, theme: &Theme) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(key.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.foreground)
                .child(value),
        )
}
