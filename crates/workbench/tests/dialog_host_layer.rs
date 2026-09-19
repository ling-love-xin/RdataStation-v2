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
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    Subscription, TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::panels::{
    EditorPanel, Shared, install_editor_bridge, install_host_redraw_bridge,
};

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
        // 面板构造会读设置 global（`SettingsService::*`）：注入默认值，不读用户磁盘配置。
        if !cx.has_global::<settings::model::Settings>() {
            cx.set_global(settings::model::Settings::default());
        }
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        // 宿主重绘桥：**调生产同一份接线**（不只写一份“长得像”的）——
        // 曾经这里只唤醒宿主、不唤醒编辑区，于是漏掉了「在编辑区 update 里调
        // notify_host」的 double-lease（真机 0xc0000409）。
        install_host_redraw_bridge(&shared, cx.entity().downgrade(), editor.clone());
        // 编辑区命令端口（与生产 `WorkbenchView::init_workspace` 同一份接线）。
        install_editor_bridge(&shared, editor.clone());
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
    let shared = cx.update(|_, cx| host.read(cx).shared.clone());
    cx.update(|_, cx| shared.notify_host(cx));
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

/// 回归：从**编辑区自己的 `update` 里**发起的宿主重绘不得双租。
///
/// 真机路径（2026-09-20 定位，事件日志 `0xc0000409` + panic 日志）：导航栏「＋」
/// → `install_editor_bridge` 的 `editor.update` → `EditorPanel::request_new_connection`
/// → `Shared::notify_host` → 宿主重绘桥。桥里若**同步**再进一次编辑区，GPUI 会以
/// `cannot update … while it is already being updated` panic；而那个 panic 从窗口
/// 过程里 unwind 出去拿不到捕获 → abort（真机表现为点一下就没了）。
///
/// 本用例对桥装的**是生产那一份**（`install_host_redraw_bridge`），所以桥里再把
/// `defer` 去掉就会当场复现。
#[gpui_kit::test]
fn host_redraw_from_inside_the_editor_update_does_not_double_lease(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (host, cx) = open_host(cx);
    let (shared, editor) = cx.update(|_, cx| {
        let host = host.read(cx);
        (host.shared.clone(), host.editor.clone())
    });
    // 先渲染一帧完成装配（端口在首帧注入）。
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // ① 走端口（导航栏「＋」的同一条链：update 内 → 面板内部 notify_host）。
    cx.update(|window, cx| {
        let bridge = shared
            .editor_bridge
            .borrow()
            .clone()
            .expect("编辑区端口已注入");
        (*bridge.new_connection)(window, cx);
    });
    // ② 再显式来一次「在编辑区 update 里调 notify_host」（面板内有几处这个形状）。
    cx.update(|_, cx| {
        editor.update(cx, |_panel, cx| shared.notify_host(cx));
    });

    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "双租被修掉后，这条链应正常打开对话框（而不是 panic/abort）"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "延迟一帧后对话框层仍应进入元素树"
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

#[gpui_kit::test]
fn sidebar_edit_request_renders_dialog_layer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (host, cx) = open_host(cx);
    let (shared, _editor) = cx.update(|_, cx| {
        let host = host.read(cx);
        (host.shared.clone(), host.editor.clone())
    });

    // 先渲染一帧完成装配（端口在 `init_workspace` 注入；生产里导航入口在首帧之后才可点）。
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 模拟侧边栏「编辑」入口：走命令端口（与导航点击同一路径；对话框在事件路径直接打开）。
    cx.update(|window, cx| {
        let bridge = shared
            .editor_bridge
            .borrow()
            .clone()
            .expect("编辑区端口已注入");
        (*bridge.edit_connection)("G_conn_demo".to_string(), window, cx);
    });

    // 宿主重绘，层进入元素树。
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "编辑入口应打开对话框"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "编辑入口的对话框层应渲染"
    );
}

#[gpui_kit::test]
fn sidebar_new_connection_request_renders_dialog_layer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (host, cx) = open_host(cx);
    let (shared, _editor) = cx.update(|_, cx| {
        let host = host.read(cx);
        (host.shared.clone(), host.editor.clone())
    });

    // 先渲染一帧完成装配（端口在 `init_workspace` 注入）。
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 模拟导航面板头「＋」/ 空态「新建连接」：走命令端口（对话框在事件路径直接打开）。
    cx.update(|window, cx| {
        let bridge = shared
            .editor_bridge
            .borrow()
            .clone()
            .expect("编辑区端口已注入");
        (*bridge.new_connection)(window, cx);
    });

    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "新建数据源入口应打开对话框"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "新建数据源入口的对话框层应渲染"
    );
}
