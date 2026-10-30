//! 选中目标下钻（对齐 EDA 原型右栏 drillzone，2026-10-30 重构）。
//!
//! 版式对齐 `insight-eda-prototype.html` 的 `.drillzone`：
//! - `dz-title`「当前目标下钻」+ 目标名（原型 `dzTarget`）+ 类型徽标 + 取样口径；
//! - 摘要卡（原型 `#dzSummary`）：检查器命中环（`grade-ring`：hits≥80%→success、
//!   ≥50%→warning、否则 danger）+ kv 行；
//! - 表画像卡（原型 `#dzTable` `tbl-row`）：列名 + 类型徽标 + 缺失率，探索价值
//!   排序前 8 列 +「…共 N 列（完整清单见中间报告）」；
//! - 多列聚合占位（原型 `agg-placeholder`）：GROUP BY / 求和 / 均值 / 计数——
//!   后续实现，先占位给用户看得到的能力边界。
//!
//! **下钻区按目标类型自适应**（用户拍板）：表 → 表画像卡；列 → 列摘要卡；
//! schema → 结构摘要卡（健康分 + 问题数）；多列 → 聚合占位。非表目标不另走中间页。
//!
//! 与中间工作页是 D65 双栏单源：完整列卡 / 关联 / 缺失 / 样本由中间页 EDA 承担，
//! 右栏因窄（280px）只做**集合 + 摘要级下钻**，本视图不重复取数（只读 `data`）。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{empty_state, error_block, grade_color, kind_badge, muted_line, skeleton, sample_loading};
use crate::insight_view::InsightView;
use crate::model::{
    ColumnKind, ColumnProfileView, InsightPanelState, InsightTarget, MultiColumnView, PanelTab,
    TableProfileView,
};
use crate::schema_view::SchemaReportView;
use crate::ui;

// component::IconName 只是 106 个兼容子集；Sigma / ListFilter 在完整 Lucide
// 目录（gpui_kit::assets::IconName）——`Icon::new` 经 `IconNamed` 泛化可接收
// assets 枚举，无需转换。
const ICON_SIGMA: gpui_kit::assets::IconName = gpui_kit::assets::IconName::Sigma;
const ICON_LIST_FILTER: gpui_kit::assets::IconName = gpui_kit::assets::IconName::ListFilter;

impl InsightView {
    /// 下钻区（对齐原型 drillzone）：dz-title + 按目标类型自适应的摘要卡 + 聚合占位。
    ///
    /// 渲染以**载荷**为准（与旧 render_body 同口径）：错误态整页接管；无目标 →
    /// 空态引导；加载中 → 阶段说明 / 骨架；否则按目标类型渲染摘要。
    pub(crate) fn render_drillzone(
        &self,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        ring_px: Pixels,
        empty_icon: Pixels,
    ) -> Vec<AnyElement> {
        if let InsightPanelState::Error { message, retryable } = &self.state {
            return vec![error_block(
                message,
                *retryable,
                entity.clone(),
                theme,
                inline_icon,
            )
            .into_any_element()];
        }
        let Some(target) = self.target.as_ref() else {
            return vec![
                empty_state(IconName::Info, "在结果表列头或导航树上右键，选择洞察", theme, empty_icon)
                    .into_any_element(),
            ];
        };
        if self.state.is_loading() {
            return vec![if let Some(note) = self.loading_note.as_deref() {
                sample_loading(note, theme)
            } else {
                skeleton(theme)
            }
            .into_any_element()];
        }

        let mut out: Vec<AnyElement> = vec![self.render_dz_title(target, theme, inline_icon).into_any_element()];
        match target {
            InsightTarget::Table { .. } | InsightTarget::SourceTable { .. } => {
                if let Some(profile) = self.data.as_table() {
                    out.push(
                        drill_table_summary(profile, theme, inline_icon, ring_px).into_any_element(),
                    );
                    out.push(drill_table_columns(profile, theme, inline_icon).into_any_element());
                }
            }
            InsightTarget::Column { .. } | InsightTarget::SourceColumn { .. } => {
                if let Some(profile) = self.data.as_column() {
                    out.push(
                        drill_column_summary(profile, theme, inline_icon).into_any_element(),
                    );
                }
            }
            InsightTarget::Schema { .. } => {
                if let Some(report) = self.data.as_schema() {
                    out.push(
                        drill_schema_summary(report, theme, inline_icon, ring_px).into_any_element(),
                    );
                }
            }
            InsightTarget::MultiColumn { .. } => {
                if let Some(view) = self.data.as_multi() {
                    out.push(drill_multi_summary(view, theme, inline_icon).into_any_element());
                }
            }
        }
        // 聚合能力边界只在**多列语境**可见（原型 agg-placeholder 即多列选中语境，
        // 见 insight-eda-prototype.html:527）：目标为多列、或停留在多列 Tab 时渲染；
        // 表 / 列 / schema 目标的正常画像不渲染死区占位（2026-10-31 冗余收敛）。
        let multi_context = matches!(target, InsightTarget::MultiColumn { .. })
            || matches!(self.tab, PanelTab::MultiColumn);
        if multi_context {
            out.push(agg_placeholder(theme).into_any_element());
        }
        out
    }

    /// dz-title：原型「当前目标下钻」+ `dzTarget`（目标名 + 类型徽标 + 取样口径）。
    ///
    /// 目标名与口径常显——「这数是什么口径」的歧义在右栏同样要消除
    /// （口径徽标与中间工作页 scope 切换同一语义）。
    fn render_dz_title(&self, target: &InsightTarget, theme: &Theme, _inline_icon: Pixels) -> Div {
        let colors = theme.colors;
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("当前目标下钻"),
            )
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_sm()
                            .text_color(colors.foreground)
                            .text_ellipsis()
                            .child(target.title()),
                    )
                    .child(kind_badge(self.data_kind()))
                    .child(
                        Tag::secondary()
                            .outline()
                            .xsmall()
                            .flex_none()
                            .child(self.scope.label()),
                    ),
            )
            .when_some(target.detail(), |this, detail| {
                this.child(muted_line(&detail, theme))
            })
    }

    /// 当前数据的类型族（无数据时退回目标声明的类型字符串）。
    /// 只被下钻区使用，故放在本模块（原 header.rs 的 render_target_head 已随目标头移除）。
    fn data_kind(&self) -> ColumnKind {
        self.data
            .column
            .as_ref()
            .map(|profile| profile.kind)
            .unwrap_or(ColumnKind::Unknown)
    }
}

/// 表目标摘要卡（原型 `#dzSummary`）：检查器命中环 + kv 行。
///
/// 环只表达事实计数（适用 / 命中），不做「良好 / 较差」评级（评级属资产库审计侧）；
/// 配色按命中比例：≥80% success、≥50% warning、否则 danger（对齐原型 grade-ring 判定）。
fn drill_table_summary(
    profile: &TableProfileView,
    theme: &Theme,
    inline_icon: Pixels,
    ring_px: Pixels,
) -> Div {
    let colors = theme.colors;
    let quality = profile.quality.as_ref();
    let checker_applicable = quality.map(|q| q.checker_applicable).unwrap_or(0);
    let checker_hits = quality.map(|q| q.checker_hits).unwrap_or(0);
    let ring_value = if checker_applicable > 0 {
        checker_hits as f32 / checker_applicable as f32 * 100.0
    } else {
        0.0
    };
    // 对齐原型 grade-ring 判定：≥80% success / ≥50% warning / 否则 danger；未评估 muted
    let ring_color = if checker_applicable == 0 {
        colors.muted_foreground
    } else if ring_value >= 80.0 {
        colors.success
    } else if ring_value >= 50.0 {
        colors.warning
    } else {
        colors.danger
    };

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
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child("摘要"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(ring_color)
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
                    ProgressCircle::new("insight-drill-ring")
                        .value(ring_value)
                        .color(ring_color)
                        .with_size(Size::Size(ring_px))
                        .accessibility_label("检查器命中")
                        .child(div().text_sm().text_color(ring_color).child(match quality {
                            Some(_) => format!("{checker_hits}/{checker_applicable}"),
                            None => "—".into(),
                        })),
                )
                .child({
                    // 环下方的命中说明行（评估后才有人数；未评估给引导）
                    let hint = match quality {
                        Some(q) if q.checker_applicable > 0 => format!(
                            "适用 {} 条 · 命中 {} 条",
                            q.checker_applicable, q.checker_hits
                        ),
                        _ => "评估全表后统计检查器命中".to_string(),
                    };
                    muted_line(&hint, theme)
                }),
        )
        .child(
            DescriptionList::vertical()
                .bordered(false)
                .item("总行数", profile.row_count.to_string(), 1)
                .item("列数", profile.columns.len().to_string(), 1)
                .item(
                    "取样",
                    crate::service::persistence::SOURCE_SAMPLE_LIMIT.to_string(),
                    1,
                ),
        )
}

/// 表画像卡（原型 `#dzTable` `tbl-row`）：探索价值排序前 8 列 + 缺失率 + 卡头计数。
///
/// 缺失率按原型标色：>20% danger（需关注）/ >5% warning / 其余 muted；
/// 前 8 列是窄面板可读上限，超出的以一行说明引向中间页完整清单。
/// 「值得关注 N 列」只在卡头计数一次（2026-10-31 冗余收敛：卡底重复行
/// 「值得关注 N 列 · 缺失率>20%」「检查器未命中 N 条」删除——命中事实
/// 已由摘要卡命中环的 hint 承担，列级未命中清单归中间页值得关注区）。
fn drill_table_columns(profile: &TableProfileView, theme: &Theme, inline_icon: Pixels) -> Div {
    let colors = theme.colors;
    let shown = profile.columns.iter().take(8);
    let high_null = profile
        .columns
        .iter()
        .filter(|c| c.null_rate.map(|r| r > 0.2).unwrap_or(false))
        .count();

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
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child("表画像"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(if high_null > 0 {
                            colors.danger
                        } else {
                            colors.muted_foreground
                        })
                        .child(format!("{high_null} 列值得关注")),
                ),
        )
        .child(
            div().w_full().v_flex().gap_1().children(shown.map(|c| {
                let rate_color = match c.null_rate {
                    Some(r) if r > 0.2 => colors.danger,
                    Some(r) if r > 0.05 => colors.warning,
                    _ => colors.muted_foreground,
                };
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_xs()
                            .text_color(colors.foreground)
                            .text_ellipsis()
                            .child(c.name.clone()),
                    )
                    .child(kind_badge(c.kind))
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_TABLE_NULL_RATE_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_color(rate_color)
                            .child(c.null_rate.map(crate::model::fmt_pct).unwrap_or_else(|| "—".to_string())),
                    )
            })),
        )
        .when(profile.columns.len() > 8, |card| {
            card.child(muted_line(
                &format!(
                    "…共 {} 列（完整清单见中间报告）",
                    profile.columns.len()
                ),
                theme,
            ))
        })
}

/// 列目标摘要卡（用户拍板：列 → 列摘要卡）：类型 + 总行数 / 空值数 / 空值率 / 样本。
fn drill_column_summary(profile: &ColumnProfileView, theme: &Theme, inline_icon: Pixels) -> Div {
    let colors = theme.colors;
    let warn = profile.null_rate > crate::model::NULL_RATE_WARN;
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
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .text_color(colors.foreground)
                        .text_ellipsis()
                        .child(profile.column.clone()),
                )
                .child(kind_badge(profile.kind)),
        )
        .child(
            DescriptionList::vertical()
                .bordered(false)
                .item("类型", profile.data_type.clone(), 1)
                .item("总行数", profile.total_count.to_string(), 1)
                .item("空值数", profile.null_count.to_string(), 1)
                .item(
                    "空值率",
                    crate::model::fmt_pct(profile.null_rate),
                    1,
                )
                .item("样本", profile.sample.len().to_string(), 1),
        )
        .when(warn, |card| {
            card.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(inline_icon)
                            .text_color(colors.warning),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.warning)
                            .child("空值率偏高，建议评估该列"),
                    ),
            )
        })
}

/// 结构摘要卡（schema 目标自适应）：健康分环 + 表数 / 列数 / 问题数。
fn drill_schema_summary(
    report: &SchemaReportView,
    theme: &Theme,
    inline_icon: Pixels,
    ring_px: Pixels,
) -> Div {
    let colors = theme.colors;
    let grade_color = grade_color(report.grade, theme);
    let issue_count = report.issue_count();
    summary_card(theme)
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(Icon::new(ICON_SIGMA).size(inline_icon))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child("结构摘要"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(grade_color)
                        .child(report.grade.label()),
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
                    ProgressCircle::new("insight-drill-schema-ring")
                        .value(report.health as f32)
                        .color(grade_color)
                        .with_size(Size::Size(ring_px))
                        .accessibility_label("结构健康分")
                        .child(
                            div()
                                .text_sm()
                                .text_color(grade_color)
                                .child(format!("{}%", report.health as u32)),
                        ),
                )
                .child(muted_line(report.summary.as_str(), theme)),
        )
        .child(
            DescriptionList::vertical()
                .bordered(false)
                .item("表数", report.table_count.to_string(), 1)
                .item("列数", report.total_columns.to_string(), 1)
                .item(
                    "问题数",
                    format!("{issue_count}"),
                    1,
                ),
        )
}

/// 多列目标摘要（原型聚合占位由 [`agg_placeholder`] 承担；此处只列出参与列）。
fn drill_multi_summary(view: &MultiColumnView, theme: &Theme, inline_icon: Pixels) -> Div {
    let colors = theme.colors;
    summary_card(theme)
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(Icon::new(ICON_LIST_FILTER).size(inline_icon))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child("参与列"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(format!("{} 列", view.columns.len())),
                ),
        )
        .child(
            div()
                .w_full()
                .v_flex()
                .gap_0p5()
                .children(view.columns.iter().take(6).map(|c| {
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_xs()
                                .text_color(colors.foreground)
                                .text_ellipsis()
                                .child(c.name.clone()),
                        )
                        .child(kind_badge(c.kind))
                })),
        )
        .when(view.columns.len() > 6, |card| {
            card.child(muted_line(
                &format!("…共 {} 列", view.columns.len()),
                theme,
            ))
        })
}

/// 多列聚合占位（原型 `agg-placeholder`）：GROUP BY / 求和 / 均值 / 计数——后续实现。
///
/// 用标准卡样式（border + sidebar 底，与摘要卡同语言）承载能力边界说明；
/// 原型为虚线框，真机统一走卡片语言（gpui-kit-dev：样式统一优先）。
fn agg_placeholder(theme: &Theme) -> impl IntoElement {
    let colors = theme.colors;
    summary_card(theme)
        .id(ElementId::Name(SharedString::from("insight-agg-placeholder")))
        .test_support()
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(Icon::new(ICON_SIGMA).size(rems(ui::INSIGHT_INLINE_ICON_SIZE)))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(colors.foreground)
                        .child("多列聚合统计"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("后续实现"),
                ),
        )
        .child(muted_line(
            "选中列做 GROUP BY 分组 / 求和 / 均值 / 计数（DBeaver 式聚合）",
            theme,
        ))
}

/// 摘要卡容器（与旧摘要 / 中间页同卡片语言：`border + rounded_md + sidebar 底`；
/// 不用 GroupBox——outline 变体默认 p_4 在 280px 右栏太宽）
fn summary_card(theme: &Theme) -> Div {
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(theme.colors.border)
        .bg(theme.colors.sidebar)
}
