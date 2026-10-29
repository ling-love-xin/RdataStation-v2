//! 右 dock 模块面板（真面板化）：洞察 / Mock 生成 / 历史 / 存档详情。
//!
//! 与左 dock（`left_tabs.rs`）同构：每个模块 = 独立 dock tab，内容仍是各 crate
//! 自持视图（`InsightView` / `MockPanel` / `HistoryView` / `render_detail`），
//! 本文件只做薄壳 + 面板协议。`set_active` 把活动栏高亮同步回 `Shared::active_right`。

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;

use crate::view::RightPanel;

use analytics_resource::detail_view::{DetailActions, render_detail};
use editor::view::history::HistoryView;
use insight::InsightView;
use mock::mock_view::MockPanel;

use super::Shared;

/// 洞察 tab：内容 = `insight` crate 的 `InsightView`。项目开关在渲染时同步
/// （面板据此显示「未打开项目」提示，与右侧栏原 `render_insight_panel` 同口径）。
pub struct InsightPanelTab {
    shared: Shared,
    focus_handle: FocusHandle,
    insight: Entity<InsightView>,
}

impl InsightPanelTab {
    pub fn new(shared: Shared, insight: Entity<InsightView>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            insight,
        }
    }
}

impl EventEmitter<BasePanelEvent> for InsightPanelTab {}

impl Focusable for InsightPanelTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InsightPanelTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.shared.project.borrow().is_some();
        self.insight
            .update(cx, |panel, cx| panel.set_project_open(open, cx));
        let insight = self.insight.clone();
        // 顶部留 8px 间隔：dock 标题栏与模块内容（含内置工具栏）不贴边（#15）。
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .pt_2()
            .child(insight)
    }
}

impl BasePanel for InsightPanelTab {
    fn panel_name(&self) -> &'static str {
        "insight"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::Insight);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for InsightPanelTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(RightPanel::Insight.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(RightPanel::Insight.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        // 我们自己的面板有自己的工具栏，不需要 Dock 再加顶部 padding
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        // 业务按钮移到模块内部工具栏，Dock 标题栏只做容器管理。
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        // 业务项已移到模块内部工具栏，Dock 标题栏菜单只留容器管理项。
        menu
    }

}
pub struct MockPanelTab {
    shared: Shared,
    focus_handle: FocusHandle,
    mock: Entity<MockPanel>,
}

impl MockPanelTab {
    pub fn new(shared: Shared, mock: Entity<MockPanel>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            mock,
        }
    }
}

impl EventEmitter<BasePanelEvent> for MockPanelTab {}

impl Focusable for MockPanelTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MockPanelTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mock = self.mock.clone();
        // 顶部留 8px 间隔：dock 标题栏与模块内置工具栏不贴边（#15）。
        div().v_flex().size_full().min_h_0().pt_2().child(mock)
    }
}

impl BasePanel for MockPanelTab {
    fn panel_name(&self) -> &'static str {
        "mock"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::Mock);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for MockPanelTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(RightPanel::Mock.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(RightPanel::Mock.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        // 我们自己的面板有自己的工具栏，不需要 Dock 再加顶部 padding
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        // 业务按钮移到模块内部工具栏，Dock 标题栏只做容器管理。
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        // 业务项已移到模块内部工具栏，Dock 标题栏菜单只留容器管理项。
        menu
    }

}
pub struct HistoryPanelTab {
    shared: Shared,
    focus_handle: FocusHandle,
    history: Entity<HistoryView>,
}

impl HistoryPanelTab {
    pub fn new(shared: Shared, history: Entity<HistoryView>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            history,
        }
    }
}

impl EventEmitter<BasePanelEvent> for HistoryPanelTab {}

impl Focusable for HistoryPanelTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HistoryPanelTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let history = self.history.clone();
        // 顶部留 8px 间隔：dock 标题栏与模块内容不贴边（#15）。
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .pt_2()
            .child(history)
    }
}

impl BasePanel for HistoryPanelTab {
    fn panel_name(&self) -> &'static str {
        "history"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::History);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for HistoryPanelTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(RightPanel::History.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(RightPanel::History.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        // 我们自己的面板有自己的工具栏，不需要 Dock 再加顶部 padding
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        // 业务按钮移到模块内部工具栏，Dock 标题栏只做容器管理。
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        // 业务项已移到模块内部工具栏，Dock 标题栏菜单只留容器管理项。
        menu
    }

}
///
/// 选中变化经宿主观察（`init_workspace` 观察资产库面板实体 → 唤醒本面板）；
/// 未选中时显示空态——说的是「去哪儿选」，不是一句「无数据」。
pub struct ArchivePanel {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl ArchivePanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_detail_or_empty(&mut self, cx: &mut Context<Self>) -> Div {
        let detail = self
            .shared
            .resources_panel
            .borrow()
            .as_ref()
            .and_then(|panel| panel.upgrade())
            .and_then(|panel| panel.read(cx).selected_detail().cloned());
        match detail {
            Some(detail) => {
                let read_only = self.shared.project_ui.borrow().read_only;
                let actions = DetailActions {
                    host: crate::components::resource_host::build_host(&self.shared),
                    read_only,
                };
                div()
                    .v_flex()
                    .size_full()
                    .min_h_0()
                    .pt_2()
                    .child(render_detail(&detail, Some(actions), cx))
            }
            None => {
                let muted = cx.theme().colors.muted_foreground;
                div()
                    .v_flex()
                    .size_full()
                    .min_h_0()
                    .pt_2()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(div().text_sm().text_color(muted).child("未选中存档"))
                    .child(
                        div()
                            .px_4()
                            .text_xs()
                            .text_color(muted)
                            .child("在左侧「资产库」选中一行，这里显示它的归档凭证"),
                    )
            }
        }
    }
}

impl EventEmitter<BasePanelEvent> for ArchivePanel {}

impl Focusable for ArchivePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ArchivePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_detail_or_empty(cx)
    }
}

impl BasePanel for ArchivePanel {
    fn panel_name(&self) -> &'static str {
        "archive"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_right.set(RightPanel::Archive);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ArchivePanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(RightPanel::Archive.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(RightPanel::Archive.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        // 我们自己的面板有自己的工具栏，不需要 Dock 再加顶部 padding
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        // 业务按钮移到模块内部工具栏，Dock 标题栏只做容器管理。
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        // 业务项已移到模块内部工具栏，Dock 标题栏菜单只留容器管理项。
        menu
    }

}
