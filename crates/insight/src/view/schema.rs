//! 结构洞察（Tab「结构」，Phase 4.2）：健康条 + 四个折叠区 + 表名下钻 + 导出
//!
//! 四个区**都渲染**（哪怕为空）：空区的意义是「检查过、没问题」，直接隐藏会让人以为
//! 这项没做。健康条上的导出是**导出你看到的这份结论**（内容由面板算好，宿主只选路径写文件）。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::*;

use super::{grade_color, muted_line};
use crate::insight_view::InsightView;
use crate::schema_view::{SchemaExportFormat, SchemaReportView, SchemaSection, SchemaTone};
use crate::ui;

impl InsightView {
    /// 结构洞察（Tab「结构」，Phase 4.2）：健康条 + 四个折叠区 + 表名下钻。
    ///
    /// 四个区**都渲染**（哪怕为空）：空区的意义是「检查过、没问题」，
    /// 直接隐藏会让人以为这项没做。
    pub(in crate::view) fn render_schema_report(
        &self,
        report: &SchemaReportView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let color = grade_color(report.grade, theme);

        // 健康条：分数是这张报告的头号结论
        let body = div().v_flex().w_full().gap_2().child(
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
                                .text_size(rems(ui::INSIGHT_HEALTH_SCORE_FONT))
                                .text_color(color)
                                .child(format!("{:.0}", report.health)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(color)
                                .child(report.grade.label()),
                        )
                        .child(div().flex_1().min_w_0().text_xs().text_ellipsis().child(
                            format!(
                                "{} 表 · {} 列 · 需关注 {} 项",
                                report.table_count,
                                report.total_columns,
                                report.issue_count()
                            ),
                        ))
                        // 导出入口：没报告时不渲染（结构 Tab 只在有报告时走到这）
                        .child(
                            Button::new("insight-schema-export")
                                .ghost()
                                .xsmall()
                                .label("导出 ▾")
                                .tooltip("把这份报告存成文件（导出的是你看到的这份结论）")
                                .dropdown_menu({
                                    let entity = entity.clone();
                                    move |menu, _window, _cx| {
                                        let mut menu = menu;
                                        for format in SchemaExportFormat::ALL {
                                            let entity = entity.clone();
                                            menu = menu.item(
                                                PopupMenuItem::new(format.label()).on_click(
                                                    move |_, _, app| {
                                                        entity.update(app, |view, cx| {
                                                            view.request_schema_export(format, cx)
                                                        })
                                                    },
                                                ),
                                            );
                                        }
                                        menu
                                    }
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(report.summary.clone()),
                ),
        );

        // 四个折叠区（默认全展开：报告本身不长，折起来反而多点一下）
        let mut accordion = Accordion::new("insight-schema-sections")
            .multiple(true)
            .with_size(Size::Small)
            .on_toggle_click({
                let entity = entity.clone();
                move |open, _window, app| {
                    entity.update(app, |view, cx| view.set_open_schema_sections(open, cx));
                }
            });
        for (ix, section) in SchemaSection::ALL.iter().enumerate() {
            let group = report.group(*section);
            let open = self.open_schema_sections[ix];
            let title = schema_section_title(*section, group.map_or(0, |g| g.rows.len()), theme);
            accordion = accordion.item(|item| {
                item.title(title).open(open).children({
                    match group {
                        Some(group) if !group.is_empty() => group
                            .rows
                            .iter()
                            .map(|row| schema_row(row, entity, theme, inline_icon))
                            .collect::<Vec<_>>(),
                        _ => vec![muted_line(section.empty_hint(), theme).into_any_element()],
                    }
                })
            });
        }

        body.child(accordion)
    }
}

/// 分区标题：`外键候选 (3)`
fn schema_section_title(section: SchemaSection, count: usize, theme: &Theme) -> Div {
    div()
        .h_flex()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground)
                .child(section.title()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!("({count})")),
        )
}

/// 报告的一行：主文案 + 说明 + 可下钻的表名热点
fn schema_row(
    row: &crate::schema_view::SchemaRowView,
    entity: &Entity<InsightView>,
    theme: &Theme,
    inline_icon: Pixels,
) -> AnyElement {
    let colors = theme.colors;
    let (tone_color, icon) = match row.tone {
        SchemaTone::Normal => (colors.muted_foreground, IconName::Info),
        SchemaTone::Warning => (colors.warning, IconName::TriangleAlert),
        SchemaTone::Danger => (colors.danger, IconName::TriangleAlert),
    };

    let mut body = div()
        .v_flex()
        .w_full()
        .gap_1()
        .py_1()
        .child(
            div()
                .h_flex()
                .gap_1()
                .text_sm()
                .text_color(colors.foreground)
                .child(Icon::new(icon).size(inline_icon).text_color(tone_color))
                .child(div().flex_1().min_w_0().text_ellipsis().child(row.title.clone())),
        )
        .child(
            div()
                .pl_4()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(row.detail.clone()),
        );

    // 涉及的表 → 下钻热点（最多几个，长清单会让面板变成一条链接列表）
    if !row.tables.is_empty() {
        let mut links = div().h_flex().flex_wrap().gap_1().pl_4();
        for table in row.tables.iter().take(ui::INSIGHT_SCHEMA_DRILLDOWN_LIMIT) {
            links = links.child(
                Button::new(ElementId::Name(
                    format!("insight-schema-to-{}", table).into(),
                ))
                .ghost()
                .xsmall()
                .label(table.clone())
                .tooltip("看这张表的表探查")
                .on_click({
                    let entity = entity.clone();
                    let table = table.clone();
                    move |_, _, app| {
                        entity.update(app, |view, cx| view.request_table_drilldown(table.clone(), cx))
                    }
                }),
            );
        }
        body = body.child(links);
    }
    body.into_any_element()
}
