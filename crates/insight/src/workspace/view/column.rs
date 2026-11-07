//! 列画像（Tab「列」）：四个折叠区（基础统计 / 数据分布 / 质量提示 / 样本数据）。
//!
//! 这里的每一条都是「有就说，没有就说没有」：不算假分布（[`distribution_zone`] 说明原因）、
//! 不编假统计（未识别类型只给计数）、NULL 显式呈现；本视图不合成质量总分或等级。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::{Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::*;

use super::muted_line;
use crate::insight_view::{ColumnSection, InsightView};
use crate::model::{
    ColumnKind, ColumnProfileView, DistributionBar, Emphasis, NoteLevel, QualityNote, SampleCell,
    StatRow,
};
use crate::ui;

impl InsightView {
    /// 列画像四区
    ///
    /// `pub(crate)`：中间工作页（`workspace.rs`）对列目标复用同一版式（宽版）。
    pub(crate) fn render_column_profile(
        &self,
        profile: &ColumnProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let sections = ColumnSection::ALL;
        let mut accordion = Accordion::new("insight-column-zones")
            .multiple(true)
            .with_size(Size::Small)
            .on_toggle_click({
                let entity = entity.clone();
                move |open, _window, app| {
                    entity.update(app, |view, cx| view.set_open_sections(open, cx));
                }
            });

        for (i, section) in sections.iter().enumerate() {
            let open = self.open_sections[i];
            accordion = accordion.item(|item| {
                item.title(zone_title(section.title(), theme))
                    .open(open)
                    .children(zone_content(*section, profile, theme, inline_icon))
            });
        }

        div().v_flex().w_full().gap_2().child(accordion)
    }
}

fn zone_title(title: &str, theme: &Theme) -> Div {
    div()
        .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
        .text_color(theme.colors.foreground)
        .child(title.to_string())
}

/// 四区内容分派
fn zone_content(
    section: ColumnSection,
    profile: &ColumnProfileView,
    theme: &Theme,
    icon_px: Pixels,
) -> Vec<AnyElement> {
    match section {
        ColumnSection::Basics => profile
            .basics
            .iter()
            .map(|row| stat_row(row, theme).into_any_element())
            .collect(),
        ColumnSection::Distribution => distribution_zone(profile, theme),
        ColumnSection::Quality => quality_zone(&profile.notes, theme, icon_px),
        ColumnSection::Sample => sample_zone(&profile.sample, theme),
    }
}

/// 基础统计一行：键（次级色）+ 值（正文色，warning 强调）
fn stat_row(row: &StatRow, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(rems(4.5))
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(row.label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .text_color(match row.emphasis {
                    Emphasis::Normal => colors.foreground,
                    Emphasis::Muted => colors.muted_foreground,
                    Emphasis::Warning => colors.warning,
                })
                .child(row.value.clone()),
        )
}

/// 数据分布：BarChart 直方图（自带 hover/tooltip/动画）；无数可画时说明原因
fn distribution_zone(profile: &ColumnProfileView, theme: &Theme) -> Vec<AnyElement> {
    if profile.distribution.is_empty() {
        let hint = match profile.kind {
            ColumnKind::Unknown => "类型未识别，未生成分布",
            _ => "样本不足，未生成分布",
        };
        return vec![muted_line(hint, theme).into_any_element()];
    }
    // 用 gpui-kit BarChart 渲染分布直方图：X 轴=值域标签，Y 轴=占比，hover 显示完整文案
    let chart = BarChart::new(profile.distribution.clone())
        .band(|b: &DistributionBar| b.label.clone())
        .value(|b: &DistributionBar| b.ratio)
        .label_axis(true)
        .value_axis(true)
        .grid(true)
        .tooltip_value(|b: &DistributionBar, _v: f64| b.text.clone().into());
    vec![div().h(rems(6.)).child(chart).into_any_element()]
}

/// 数据质量：提示列表（Info / Warning 两档）
fn quality_zone(notes: &[QualityNote], theme: &Theme, icon_px: Pixels) -> Vec<AnyElement> {
    if notes.is_empty() {
        return vec![muted_line("未发现需要提示的质量问题", theme).into_any_element()];
    }
    notes
        .iter()
        .map(|note| {
            let colors = theme.colors;
            let (icon, color) = match note.level {
                NoteLevel::Info => (IconName::Info, colors.info),
                NoteLevel::Warning => (IconName::TriangleAlert, colors.warning),
            };
            div()
                .h_flex()
                .w_full()
                .gap_1()
                .items_start()
                .child(
                    Icon::new(icon)
                        .with_size(Size::Size(icon_px))
                        .flex_none()
                        .text_color(color),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(color)
                        .child(note.text.clone()),
                )
                .into_any_element()
        })
        .collect()
}

/// 样本数据：序号 + 值（NULL 显式显示）
fn sample_zone(cells: &[SampleCell], theme: &Theme) -> Vec<AnyElement> {
    if cells.is_empty() {
        return vec![muted_line("无可显示的样本", theme).into_any_element()];
    }
    let colors = theme.colors;
    cells
        .iter()
        .map(|cell| {
            let is_null = cell.value.is_none();
            let text = match &cell.value {
                Some(v) => truncate(v, ui::INSIGHT_SAMPLE_MAX_CHARS),
                None => "NULL".to_string(),
            };
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(rems(1.5))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(cell.index.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(if is_null {
                            colors.muted_foreground
                        } else {
                            colors.foreground
                        })
                        .child(text),
                )
                .into_any_element()
        })
        .collect()
}

/// 样本单元格截断（超长文本不得撑破 17.5rem 面板）
pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}
