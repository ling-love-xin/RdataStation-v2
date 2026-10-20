//! 左 dock 模块面板（真面板化）：数据源导航 / 草稿箱 / 资产库 / 插件。
//!
//! 每个模块 = 独立 dock tab（实现 gpui-kit `Panel` trait），dock 自动生成 tab 条：
//! 点击 tab 同面切换，拖拽可分栏并排显示多个模块。内容仍是各 crate 自持视图
//! （`NavView` / `ScratchpadView` / `ResourcesPanel`），本文件只做薄壳 + 面板协议：
//! - `tab_name` / `title`：tab 条展示（带活动栏同款图标）；
//! - `set_active`：dock 切 tab 时把活动栏高亮同步回 `Shared::active_left`；
//! - `closable = false`：模块 tab 是布局的一部分，不给 ✕ 关闭（避免重建逻辑）。

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::*;

use crate::view::LeftPanel;

use analytics_resource::resource_view::ResourcesPanel;
use database::nav_view::NavView;
use scratchpad::ScratchpadView;

use super::Shared;

/// 数据源导航 tab：内容 = `database` crate 的 `NavView`（搜索框 + 连接树自带 chrome）。
pub struct DatabaseNavPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    nav: Entity<NavView>,
}

impl DatabaseNavPanel {
    pub fn new(shared: Shared, nav: Entity<NavView>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            nav,
        }
    }
}

impl EventEmitter<BasePanelEvent> for DatabaseNavPanel {}

impl Focusable for DatabaseNavPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DatabaseNavPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let nav = self.nav.clone();
        div().v_flex().size_full().min_h_0().child(nav)
    }
}

impl BasePanel for DatabaseNavPanel {
    fn panel_name(&self) -> &'static str {
        "database_nav"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Database);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for DatabaseNavPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Database.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Database.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Database.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}

/// 草稿箱 tab：内容 = `scratchpad` crate 的 `ScratchpadView`。
pub struct ScratchPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    scratchpad: Entity<ScratchpadView>,
}

impl ScratchPanel {
    pub fn new(shared: Shared, scratchpad: Entity<ScratchpadView>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            scratchpad,
        }
    }
}

impl EventEmitter<BasePanelEvent> for ScratchPanel {}

impl Focusable for ScratchPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScratchPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let scratchpad = self.scratchpad.clone();
        div().v_flex().size_full().min_h_0().child(scratchpad)
    }
}

impl BasePanel for ScratchPanel {
    fn panel_name(&self) -> &'static str {
        "scratchpad"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Draft);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ScratchPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Draft.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Draft.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Draft.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}

/// 资产库 tab：内容 = `analytics_resource` crate 的 `ResourcesPanel`
/// （该面板自身已实现 `Panel` trait，这里薄壳化以统一 tab 语义 + 同步活动栏）。
pub struct ResourcePanelTab {
    shared: Shared,
    focus_handle: FocusHandle,
    resources: Entity<ResourcesPanel>,
}

impl ResourcePanelTab {
    pub fn new(
        shared: Shared,
        resources: Entity<ResourcesPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            resources,
        }
    }
}

impl EventEmitter<BasePanelEvent> for ResourcePanelTab {}

impl Focusable for ResourcePanelTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ResourcePanelTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let resources = self.resources.clone();
        div().v_flex().size_full().min_h_0().child(resources)
    }
}

impl BasePanel for ResourcePanelTab {
    fn panel_name(&self) -> &'static str {
        "resources"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Resources);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ResourcePanelTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Resources.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Resources.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Resources.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}

/// 插件 tab：占位视图（下一轮接入插件市场）。
pub struct PluginPanel {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl PluginPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<BasePanelEvent> for PluginPanel {}

impl Focusable for PluginPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PluginPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("插件"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 插件市场（下一轮接入）"),
            )
    }
}

impl BasePanel for PluginPanel {
    fn panel_name(&self) -> &'static str {
        "plugin"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Plugin);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for PluginPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Plugin.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Plugin.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Plugin.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}
