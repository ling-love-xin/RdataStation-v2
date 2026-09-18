//! 多列分析（Tab「多列」，Phase 3.3）：列多选 + 规则单选 + 执行 + 结果
//!
//! 选择顺序**就是**参数顺序（`col1` / `col2` …），所以选中的列带序号——方向性规则
//! （相关系数之类）是不对称的，没有序号的话「哪列进 col1」只能靠猜。
//!
//! 结果与失败提示同屏：一次失败不该把表单和已有结果一起收走（那是用户在填的东西）。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{Icon, IconName, Sizable as _, Theme};
use gpui_kit::*;

use super::{kind_badge, muted_line, section_title};
use crate::insight_view::InsightView;
use crate::model::{ColumnKind, MultiColumnView, MultiResultView, MultiRuleView, TableColumnView};
use crate::ui;

impl InsightView {
    /// 多列分析（Tab「多列」，Phase 3.3）：列多选 + 规则 + 执行 + 结果。
    ///
    /// 选择顺序就是参数顺序（`col1` / `col2` …），所以选中的列带序号——
    /// 没有序号的话「哪列进 col1」全靠猜，而方向性规则（相关系数）是不对称的。
    pub(in crate::view) fn render_multi_view(
        &self,
        view: &MultiColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let mut body = div().v_flex().w_full().gap_2();

        // 1) 列多选
        body = body.child(section_title("选择列（顺序即参数顺序）", theme));
        if view.columns.is_empty() {
            body = body.child(muted_line("该结果集没有可分析的列", theme));
        }
        for column in &view.columns {
            let picked = self
                .multi_selected
                .iter()
                .position(|name| name.as_str() == column.name.as_str())
                .map(|ix| ix + 1);
            body = body.child(multi_column_row(
                column,
                picked,
                entity,
                theme,
                inline_icon,
            ));
        }

        // 2) 规则单选
        body = body.child(section_title("选择规则", theme));
        if view.rules.is_empty() {
            body = body.child(muted_line(
                "没有可用的多列规则——可在面板头 ⚙ 里检查规则是否被禁用",
                theme,
            ));
        }
        let kinds = view.kinds_of(&self.multi_selected);
        for rule in &view.rules {
            let usable = rule.accepts(&kinds);
            body = body.child(multi_rule_row(
                rule,
                self.multi_rule.as_deref() == Some(rule.id.as_str()),
                usable,
                entity,
                theme,
            ));
        }

        // 3) 执行（不可用时置灰；原因由规则行的类型提示给出）
        body = body.child(
            div().h_flex().w_full().gap_2().child(div().flex_1()).child(
                Button::new("insight-multi-run")
                    .small()
                    .label(if self.multi_running { "分析中…" } else { "执行分析" })
                    .disabled(!self.multi_ready() || self.multi_running)
                    .tooltip("按选中顺序把列对位到规则的 col1 / col2 …")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| entity.update(app, |view, cx| view.run_multi(cx))
                    }),
            ),
        );

        // 4) 失败提示（表单照旧可见）
        if let Some(notice) = &self.multi_notice {
            body = body.child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(Icon::new(IconName::TriangleAlert).size(inline_icon))
                    .child(div().flex_1().min_w_0().child(notice.clone())),
            );
        }

        // 5) 结果
        match &view.result {
            None => {
                body = body.child(div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("选好列与规则后点「执行分析」"));
            }
            Some(result) if result.is_empty() => {
                body = body.child(muted_line("规则没有返回数据", theme));
            }
            Some(MultiResultView::Single(rows)) => {
                body = body.child(section_title("结果", theme));
                for row in rows {
                    body = body.child(
                        div()
                            .h_flex()
                            .w_full()
                            .gap_2()
                            .py_0p5()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .text_ellipsis()
                                    .child(row.label.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(row.value.clone()),
                            ),
                    );
                }
            }
            Some(MultiResultView::Table { headers, rows }) => {
                body = body.child(section_title("结果", theme));
                body = body.child(multi_table_header(headers, theme));
                for row in rows {
                    body = body.child(multi_table_row(row, theme));
                }
            }
        }

        // 6) 质量门控提示（只有未过的项）
        for note in &view.notes {
            body = body.child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.warning)
                    .child(Icon::new(IconName::TriangleAlert).size(inline_icon))
                    .child(div().flex_1().min_w_0().child(note.text.clone())),
            );
        }

        body
    }
}

/// 多列 Tab 的一列：勾选 + 序号 + 列名 + 类型徐标
fn multi_column_row(
    column: &TableColumnView,
    picked: Option<usize>,
    entity: &Entity<InsightView>,
    theme: &Theme,
    inline_icon: Pixels,
) -> Div {
    let colors = theme.colors;
    let name = column.name.clone();
    let row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::ROW_HEIGHT))
        .px_1()
        .child(
            Checkbox::new(ElementId::Name(
                format!("insight-multi-col-{}", column.name).into(),
            ))
            .checked(picked.is_some())
            .on_click({
                let entity = entity.clone();
                let name = name.clone();
                move |_, _, app| {
                    entity.update(app, |view, cx| view.toggle_multi_column(&name, cx))
                }
            }),
        )
        .child(
            // 序号：顺序即参数顺序（col1 / col2 …）
            div()
                .flex_none()
                .w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH))
                .text_xs()
                .text_color(if picked.is_some() {
                    colors.primary
                } else {
                    colors.muted_foreground
                })
                .child(match picked {
                    Some(ix) => format!("{ix}."),
                    None => "·".to_string(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(colors.foreground)
                .text_ellipsis()
                .child(column.name.clone()),
        )
        .child(kind_badge(column.kind, theme));
    if column.kind == ColumnKind::Unknown {
        return row.child(
            Icon::new(IconName::Info)
                .size(inline_icon)
                .text_color(colors.muted_foreground),
        );
    }
    row
}

/// 多列 Tab 的规则行：单选框 + 名称 + 类型提示（不可用时压暗并说明原因）
fn multi_rule_row(
    rule: &MultiRuleView,
    selected: bool,
    usable: bool,
    entity: &Entity<InsightView>,
    theme: &Theme,
) -> Div {
    let colors = theme.colors;
    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::ROW_HEIGHT))
        .px_1()
        .child(
            Radio::new(ElementId::Name(
                format!("insight-multi-rule-{}", rule.id).into(),
            ))
            .checked(selected)
            .disabled(!usable)
            .on_click({
                let entity = entity.clone();
                let id = rule.id.clone();
                move |_, _, app| entity.update(app, |view, cx| view.set_multi_rule(id.clone(), cx))
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(if usable {
                    colors.foreground
                } else {
                    colors.muted_foreground
                })
                .text_ellipsis()
                .child(rule.name.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(rule.types_hint()),
        );
    if !usable {
        // 不可用不是因为坏了，而是“这几列不对”：把原因写出来，别让点不动成为谜
        let need = if rule.arity() == 0 {
            "无需列".to_string()
        } else {
            format!("需 {} 列：{}", rule.arity(), rule.types_hint())
        };
        row = row.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(need),
        );
    }
    row
}

/// 列表结果的表头（列数动态：每列等宽，长文本省略尾）
fn multi_table_header(headers: &[String], theme: &Theme) -> Div {
    let colors = theme.colors;
    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .py_0p5()
        .bg(colors.list_hover)
        .text_xs()
        .text_color(colors.muted_foreground);
    for header in headers {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(header.clone()),
        );
    }
    row
}

fn multi_table_row(cells: &[String], theme: &Theme) -> Div {
    let colors = theme.colors;
    let mut row = div().h_flex().w_full().gap_2().px_1().py_0p5().text_xs();
    for (ix, cell) in cells.iter().enumerate() {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                // 第一列是行标签（如交叉频次表的行值）：给正常前景色，其余列压一级
                .text_color(if ix == 0 {
                    colors.foreground
                } else {
                    colors.muted_foreground
                })
                .child(cell.clone()),
        );
    }
    row
}
