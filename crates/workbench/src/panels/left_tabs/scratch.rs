use super::*;

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
        // 内容直接接官方标题条，不再补上间隔（同左侧其它 tab）。
        div().v_flex().size_full().min_h_0().child(scratchpad)
    }
}

impl BasePanel for ScratchPanel {
    fn panel_name(&self) -> &'static str {
        "scratchpad"
    }

    fn closable(&self, _cx: &App) -> bool {
        // 2026-11-24 用户验收：侧栏 tab 与中央标签一致——悬停出 ✕、点击关掉这个 tab
        // （最后一个关掉 → 整条侧栏收起）。侧栏面板没有"脏"状态，不需要三态确认，
        // 但必须在 `request_close` 里撤登记，否则 `apply_left_mode` 会把它挂回来。
        true
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            tracing::info!(panel = "scratchpad", "【探针】左 dock Tab 激活：草稿箱");
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Draft);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ScratchPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    /// 【关闭前询问】侧栏 tab 的 ✕：没有脏概念，直接撤登记交还皮肤移除。
    fn request_close(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::dock::CloseRequest {
        self.shared.close_left_panel(LeftPanel::Draft, cx);
        gpui_kit::component::dock::CloseRequest::Proceed
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
        // 模块内部已有可拖拽排序工具栏，Dock 标题栏只做容器管理。
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
