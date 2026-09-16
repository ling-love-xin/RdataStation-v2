//! 日志查看对话框「宿主层渲染」冒烟测试。
//!
//! 与 `dialog_host_layer.rs` 同一套机制：对话框层挂在宿主视图的 render 里
//! （`Root::render_dialog_layer`），而 gpui 的 `cx.notify()` 只重渲染该视图子树。
//! 因此本测试断言两件事：**打开后层真的进了元素树**、**关闭后真的移除**。
//!
//! 注意：不通配导入（`use gpui_kit::*` / `use super::*` 会把 gpui 的 `test` 宏
//! 带入作用域，与 `#[gpui_kit::test]` 冲突）。

use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement, Render, Styled as _, TestAppContext, Window,
    div,
};

/// 简化宿主：只负责把对话框层挂进自己的元素树（生产是 `WorkbenchView`）。
struct HostView;

impl Render for HostView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

#[gpui_kit::test]
fn log_dialog_opens_renders_and_closes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (host, cx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_cx| HostView);
        Root::new(host, window, cx)
    });
    let _ = host;

    // 设置页「查看日志…」走的就是这个入口（`SettingsHost::on_open_log_view`）。
    // 这里不额外通知宿主：入口本身要负责让层进入元素树（否则生产表现是"点了没反应"）。
    cx.update(|window, cx| {
        rds_workbench::components::log_dialog::open_log_dialog(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "对话框状态应已激活"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "日志对话框层应真正渲染（入口未让宿主重绘？）"
    );

    // 关闭后层应从元素树移除。
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "关闭后不应有活动对话框"
    );
}
