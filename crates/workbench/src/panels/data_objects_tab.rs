//! 「数据对象」右 dock tab：所有可参与分析的对象一处分览。
//!
//! 取数在**事件路径**（构造 / 刷新）跑一次并写进 `Shared::data_objects`，渲染只读缓存——
//! `collect` 内含读盘与查库，放进 render 就是每帧 I/O（gpui-kit-dev 禁止）。
//!
//! 本轮只做「看得见 + 分组 + 通道 / 状态」（原型 07 的骨架），行内「分析」与工作集
//! 留到下一步；先把清单与分组口径钉住，避免一次改太多分不清是哪层出的问题。

use gpui_kit::base::StyledExt;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

use crate::components::data_objects::{self, Group};
use crate::view::RightPanel;

use super::Shared;

pub struct DataObjectsTab {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl DataObjectsTab {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        let tab = Self {
            shared,
            focus_handle: cx.focus_handle(),
        };
        tab.refresh(cx);
        tab
    }

    /// 重新取一次清单（事件路径）。
    pub fn refresh(&self, cx: &mut Context<Self>) {
        let collected = data_objects::collect(&self.shared);
        *self.shared.data_objects.borrow_mut() = data_objects::objects(&collected);
        cx.notify();
    }

    fn render_body(&self, cx: &Context<Self>) -> Div {
        let objects = self.shared.data_objects.borrow().clone();
        let colors = cx.theme().colors;

        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.foreground)
                    .child("数据对象"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("{} 个", objects.len())),
            );

        let mut list = div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .id("data-objects-list");

        let mut last_group: Option<Group> = None;
        for object in &objects {
            if last_group != Some(object.group) {
                last_group = Some(object.group);
                list = list.child(
                    div()
                        .px_3()
                        .pt_3()
                        .pb_1()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(object.group.title()),
                );
            }
            // 「怎么读」「还在不在」是选源时仅有的两个判断依据，放同一行右侧
            let state_color = if object.can_analyze() {
                colors.muted_foreground
            } else {
                colors.danger
            };
            list = list.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(colors.border)
                    // 失效行压暗：仍然看得见，但别让人以为能直接点
                    .opacity(if object.can_analyze() { 1.0 } else { 0.55 })
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(object.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(object.note.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(colors.muted_foreground)
                            .child(object.channel.label()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_none()
                            .text_color(state_color)
                            .child(object.state.label()),
                    ),
            );
        }

        if objects.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_6()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child("还没有可用的数据对象：先连一个数据源，或跑一次查询。"),
            );
        }

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .pt_2()
            .child(header)
            .child(list)
    }
}

impl EventEmitter<BasePanelEvent> for DataObjectsTab {}

impl Focusable for DataObjectsTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DataObjectsTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_body(cx)
    }
}

impl BasePanel for DataObjectsTab {
    fn panel_name(&self) -> &'static str {
        "data-objects"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::DataObjects);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for DataObjectsTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 与其它右 dock tab 一致：返回 None，用 title() 渲染图标 + 文字
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(RightPanel::DataObjects.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(RightPanel::DataObjects.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<gpui_kit::component::button::Button>> {
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        menu
    }
}
