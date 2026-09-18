//! 列画像（Tab「列」）：评分卡 + 四个折叠区
//!
//! 评分卡**钉在滚动区之外**（由宿主的 `render` 组装）：分数是这一列的头号结论，不该滚走。
//! 四区（基础统计 / 数据分布 / 数据质量 / 样本数据）在滚动区内，折叠态属用户偏好。
//!
//! 这里的每一条都是「有就说，没有就说没有」：不算假分布（[`distribution_zone`] 说明原因）、
//! 不产假分数（`ColumnProfileView::score` 在全空列上不给分）、NULL 显式呈现。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::{Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::*;

use super::{grade_color, muted_line, ratio_bar};
use crate::insight_view::{ColumnSection, InsightView};
use crate::model::{
    ColumnKind, ColumnProfileView, DimensionView, DistributionBar, Emphasis, NoteLevel, PanelTab,
    QualityNote, SampleCell, StatRow,
};
use crate::quality_scorer::Grade;
use crate::ui;

impl InsightView {
    /// 评分卡（Phase 2）：**钉在滚动区之外**——分数是这列的头号结论，不该滚走。
    ///
    /// 只有「列」Tab 且列非全空时出现（后者见 `ColumnProfileView::score`：不产假分数）。
    pub(crate) fn render_score_card(&self, theme: &Theme) -> Option<Div> {
        if self.tab != PanelTab::Column {
            return None;
        }
        let score = self.data.column.as_ref()?.score.as_ref()?;
        let colors = theme.colors;
        let color = grade_color(score.grade, theme);

        let mut card = div()
            .flex_none()
            .v_flex()
            .w_full()
            .gap_1()
            .px(rems(ui::PANEL_PADDING))
            .py_1()
            .border_t_1()
            .border_color(colors.border)
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(ui::INSIGHT_SCORE_FONT))
                            .text_color(color)
                            .child(format!("{:.0}", score.overall)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(color)
                            .child(score.grade.label()),
                    ),
            );
        for dim in &score.dimensions {
            card = card.child(dimension_row(dim, theme));
        }
        Some(card.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(score.summary.clone()),
        ))
    }

    /// 列画像四区
    pub(in crate::view) fn render_column_profile(
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

/// 评分卡的一维：名称 + 细条 + 分数（条色按**该维自己的**等级，便于一眼看出短板）
fn dimension_row(dim: &DimensionView, theme: &Theme) -> Div {
    let colors = theme.colors;
    let color = grade_color(Grade::of(dim.score), theme);
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(dim.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(format!("{:.0}", dim.score)),
                ),
        )
        .child(ratio_bar(
            (dim.score / 100.0).clamp(0.0, 1.0) as f32,
            ui::INSIGHT_RATIO_BAR_HEIGHT,
            color,
            theme,
        ))
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

/// 数据分布：条 + 占比；无数可画时说明原因（不造假分布）
fn distribution_zone(profile: &ColumnProfileView, theme: &Theme) -> Vec<AnyElement> {
    if profile.distribution.is_empty() {
        let hint = match profile.kind {
            ColumnKind::Unknown => "类型未识别，未生成分布",
            _ => "样本不足，未生成分布",
        };
        return vec![muted_line(hint, theme).into_any_element()];
    }
    // 布尔列只有一条 True 占比：用矮条，其余用直方图条高
    let bar_height = if profile.kind == ColumnKind::Boolean {
        ui::INSIGHT_RATIO_BAR_HEIGHT
    } else {
        ui::INSIGHT_HISTOGRAM_BAR_HEIGHT
    };
    profile
        .distribution
        .iter()
        .map(|bar| distribution_bar(bar, bar_height, theme).into_any_element())
        .collect()
}

fn distribution_bar(bar: &DistributionBar, bar_height: f32, theme: &Theme) -> Div {
    let colors = theme.colors;
    let ratio = bar.ratio.clamp(0.0, 1.0) as f32;
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(colors.muted_foreground)
                        .child(bar.label.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(bar.text.clone()),
                ),
        )
        .child(ratio_bar(ratio, bar_height, colors.primary, theme))
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
