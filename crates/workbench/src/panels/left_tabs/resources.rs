use super::*;

/// 资产库 tab：内容 = `analytics_resource` 的 `ResourcesPanel`。
/// （该面板自身已实现 `Panel` trait，这里薄壳化以统一 tab 语义 + 同步活动栏）。
pub struct ResourcePanelTab {
    shared: Shared,
    focus_handle: FocusHandle,
    resources: Entity<ResourcesPanel>,
}

impl ResourcePanelTab {
    pub fn new(shared: Shared, resources: Entity<ResourcesPanel>, cx: &mut Context<Self>) -> Self {
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
        // 顶部留 8px 间隔：dock 标题栏与模块内容不贴边（与右侧 dock 统一）
        div().v_flex().size_full().min_h_0().pt_2().child(resources)
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
            tracing::info!(panel = "resources", "【探针】左 dock Tab 激活：资产库");
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Resources);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ResourcePanelTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
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
        // 模块内部已有工具栏（搜索框/筛选/排序/更多），Dock 标题栏只做容器管理。
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
