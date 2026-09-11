//! 连接对话框「宿主层渲染」回归测试。
//!
//! 关键机制：对话框层挂在宿主视图（生产为 `WorkbenchView`）的 render 中，而
//! gpui 的 `cx.notify()` 只重渲染该视图子树，`Root::open_dialog` 仅通知 Root。
//! 因此打开对话框后必须显式通知宿主重渲染，层才会真正进入元素树——否则表现
//! 就是「点了入口没反应」。本测试用简化宿主复现同一结构，并以
//! `debug_bounds("dialog-layer")` 断言层确实渲染（而非仅状态激活）。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的
//! `test` 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    Subscription, TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::panels::{EditorPanel, Shared};

/// 简化宿主：与 `WorkbenchView` 同构——挂对话框层 + 注入宿主重绘桥 + 观察编辑面板。
struct HostView {
    shared: Shared,
    editor: Entity<EditorPanel>,
    /// 编辑面板观察句柄（其 notify 级联到宿主，同生产 `WorkbenchView`）。
    _editor_subscription: Option<Subscription>,
}

impl HostView {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = Shared::new();
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        // 宿主重绘桥（生产由 `WorkbenchView::new` 注入）。
        let weak = cx.entity().downgrade();
        let bridge: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        *shared.host_redraw.borrow_mut() = Some(bridge);
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            shared,
            editor,
            _editor_subscription: Some(subscription),
        }
    }
}

impl Render for HostView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.editor.clone())
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

fn open_host(cx: &mut TestAppContext) -> (Entity<HostView>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<HostView>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let host = cx.new(|cx| HostView::new(window, cx));
        *slot_in.borrow_mut() = Some(host.clone());
        Root::new(host, window, cx)
    });
    let host = slot.borrow().clone().expect("host 已创建");
    (host, cx)
}

#[gpui_kit::test]
fn new_connection_request_renders_dialog_layer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (host, cx) = open_host(cx);
    let editor = cx.update(|_, cx| host.read(cx).editor.clone());

    // 入口（等价于点击编辑区「新建连接」按钮）。
    cx.update(|window, cx| {
        editor.update(cx, |panel, cx| panel.request_new_connection(window, cx));
    });

    // 渲染一帧：宿主应已重绘，层进入元素树。
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "对话框状态应已激活"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "对话框层应真正渲染（宿主重绘路径生效）"
    );

    // 关闭后宿主重绘，层应从元素树移除。
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|_, cx| {
        host.update(cx, |host, cx| {
            let shared = host.shared.clone();
            shared.notify_host(cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "关闭后不应有活动对话框"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_none(),
        "关闭后层应从元素树移除"
    );
}

#[gpui_kit::test]
fn editor_notify_cascades_to_host_layer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (host, cx) = open_host(cx);
    let editor = cx.update(|_, cx| host.read(cx).editor.clone());

    // 打开对话框（层进入元素树）。
    cx.update(|window, cx| {
        editor.update(cx, |panel, cx| panel.request_new_connection(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("dialog-layer").is_some());

    // 编辑面板自身的 notify（对话框内部状态刷新的通用模式，如切 Tab / 增删协议链跳）
    // 应级联到宿主，从而保持层内容刷新。
    cx.update(|_, cx| {
        editor.update(cx, |_, cx| cx.notify());
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "编辑面板 notify 后层应仍在并刷新"
    );
}
