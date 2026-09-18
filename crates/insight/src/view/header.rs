//! 面板头与目标头（原型 §2：「洞察」+ ⚙ 规则管理 + ⟳ 重算；有目标时加一行目标头）
//!
//! 两个入口都只**发意图**：⚙ 交给规则对话框自己去取数（`jobs::attach_rules` 那条路径），
//! ⟳ 走 [`InsightView::reload`] 按当前 Tab 重发请求（渲染路径零 I/O，D20）。
//!
//! 目标头是**常显**的：目标名 + 类型徽标 + 该目标当前载荷能给出的那句结论
//! （列 → 空值率，表 → 行数）。载荷还没到时只显示前两样——不编一个 0。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{IconName, Sizable as _, Theme};
use gpui_kit::*;

use super::kind_badge;
use crate::insight_view::InsightView;
use crate::model::ColumnKind;
use crate::ui;

impl InsightView {
    pub(crate) fn render_header(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        div()
            .flex_none()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child("洞察"),
            )
            .child(
                Button::new("insight-rules")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings)
                    // 规则落在项目目录下（`{项目}/.RSmeta/insight-rules/`）：
                    // 无项目时不给入口，而不是给一个点了没反应的按钮
                    .disabled(!self.project_open)
                    .tooltip(if self.project_open {
                        "规则管理"
                    } else {
                        "规则管理（需先打开项目）"
                    })
                    .on_click({
                        let rules = self.rules.clone();
                        move |_, window, app| {
                            // 开窗 + 发一次取数请求；数据由接缝回填
                            rules.update(app, |view, cx| view.begin_load(window, cx));
                        }
                    }),
            )
            .child(
                Button::new("insight-refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCw)
                    .tooltip("重算")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| view.reload(cx));
                        }
                    }),
            )
    }

    /// 无项目提示：画像仍可算（临时表在内存里），但依赖项目目录的能力不可用
    pub(crate) fn render_project_hint(&self, theme: &Theme) -> Option<Div> {
        if self.project_open {
            return None;
        }
        Some(
            div()
                .flex_none()
                .w_full()
                .px(rems(ui::PANEL_PADDING))
                .py_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("未打开项目：画像可用，规则管理与快照不可用"),
        )
    }

    /// 目标头：常显当前目标（列名 + 类型徽标 + 空值率）
    pub(crate) fn render_target_head(&self, theme: &Theme) -> Option<Div> {
        let target = self.target.as_ref()?;
        let colors = theme.colors;
        let mut head = div()
            .flex_none()
            .px(rems(ui::PANEL_PADDING))
            .py_1()
            .bg(colors.list_active)
            .v_flex()
            .gap_1()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .text_color(colors.foreground)
                            .text_ellipsis()
                            .child(target.title()),
                    )
                    .child(kind_badge(self.data_kind(), theme)),
            );
        if let Some(detail) = target.detail() {
            head = head.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .text_ellipsis()
                    .child(detail),
            );
        }
        if let Some(profile) = &self.data.column {
            head = head.child(
                div()
                    .text_xs()
                    .text_color(if profile.null_rate > crate::model::NULL_RATE_WARN {
                        colors.warning
                    } else {
                        colors.muted_foreground
                    })
                    .child(format!(
                        "空值率 {}",
                        crate::model::fmt_pct(profile.null_rate)
                    )),
            );
        }
        if let Some(profile) = &self.data.table {
            head = head.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("{} 行", crate::model::fmt_rows(profile.row_count))),
            );
        }
        Some(head)
    }

    /// 当前数据的类型族（无数据时退回目标声明的类型字符串）
    fn data_kind(&self) -> ColumnKind {
        self.data
            .column
            .as_ref()
            .map(|profile| profile.kind)
            .unwrap_or(ColumnKind::Unknown)
    }
}
