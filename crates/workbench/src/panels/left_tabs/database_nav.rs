use super::*;

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
        // 顶部留 8px 间隔：dock 标题栏与模块内容不贴边（与右侧 dock 统一）
        div().v_flex().size_full().min_h_0().pt_2().child(nav)
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
            tracing::info!(
                panel = "database_nav",
                "【探针】左 dock Tab 激活：数据源导航"
            );
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Database);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for DatabaseNavPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
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
        // 模块内部已有完整工具栏（NavView 自带 Toolbar），Dock 标题栏只做容器管理。
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
