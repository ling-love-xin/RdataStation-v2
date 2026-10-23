//! 底部结果 dock 薄壳（Dock 化阶段 3，DBeaver Data Editor 对齐）。
//!
//! 本壳只做两件事：按活动文档渲染结果面板、经 `set_active_host` 切换目标宿主。
//! 面板按钮的队列分派由 editor 宿主订阅面板 notify 自 drain（§8.5，用户已确认），
//! 本壳不掺和事件；dock 的「有结果自动开 / 无结果自动关」由 workbench 事件路径驱动。

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use std::cell::RefCell;
use std::rc::Rc;

use editor::view::host::EditorHostPanel;

/// 底部结果 dock：内容 = 活动文档的 `ResultPanel`（workbench 保证同一时刻
/// 只有一个活动文档，故底部只挂这一个薄壳实体，切换靠换槽）。
pub struct ResultPanelDock {
    focus_handle: FocusHandle,
    /// 活动文档的 editor 宿主弱引用槽（workbench 在文档激活 / 状态变化时更新；
    /// Rc<RefCell> 便于宿主直接替换引用，不建实体互引）
    active_host: Rc<RefCell<Option<WeakEntity<EditorHostPanel>>>>,
}

impl ResultPanelDock {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            active_host: Rc::new(RefCell::new(None)),
        }
    }

    /// workbench 在活动文档变化时更新目标宿主（无活动文档传 None）
    pub fn set_active_host(&self, host: Option<WeakEntity<EditorHostPanel>>) {
        *self.active_host.borrow_mut() = host;
    }

    /// 活动宿主槽的克隆引用（workbench 判「有结果自动开 / 无结果自动关」时
    /// 与渲染共用同一实体判断，避免两处各拍一次）
    pub fn active_host(&self) -> Rc<RefCell<Option<WeakEntity<EditorHostPanel>>>> {
        self.active_host.clone()
    }
}

impl EventEmitter<BasePanelEvent> for ResultPanelDock {}

impl Focusable for ResultPanelDock {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ResultPanelDock {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let host = self.active_host.borrow();
        let result = host
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .map(|host| host.read(cx).result_panel());
        match result {
            // 面板自带 `render(&self, cx) -> AnyElement`（纯读；不是 gpui Render trait，
            // 无需 update），薄壳只把活动面板的元素挂进来。
            Some(panel) => div().v_flex().size_full().min_h_0().child(panel.read(cx).render(cx)),
            // 兜底空态：正常路径下 dock 在无结果时自动关闭，此处仅防 dock 未关而宿主缺失
            None => div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().colors.muted_foreground)
                .child("运行查询后，结果会显示在这里"),
        }
    }
}

impl BasePanel for ResultPanelDock {
    fn panel_name(&self) -> &'static str {
        "result_panel"
    }

    fn closable(&self, _cx: &App) -> bool {
        // 用户可手动关闭；「有结果自动开 / 无结果自动关」由 workbench 事件路径驱动
        true
    }

    fn set_active(&mut self, _active: bool, _window: &mut Window, _cx: &mut Context<Self>) {}
}

impl ComponentPanel for ResultPanelDock {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("结果".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("结果"),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}
