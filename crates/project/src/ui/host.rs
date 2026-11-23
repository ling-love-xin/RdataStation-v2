//! 宿主桥辅助：只读守卫、动作反馈通道（`notice` + toast）、当前项目在名册中的条目。

use super::*;

/// 只读模式守卫：被拦截时写入提示并返回 `true`。
pub(super) fn read_only_blocked(host: &ProjectUiHost, cx: &mut App, action: &str) -> bool {
    if host.state.borrow().read_only {
        host.state.borrow_mut().notice = Some(format!("只读模式：不允许{action}"));
        host.notify(cx);
        true
    } else {
        false
    }
}

/// 动作成功反馈：`notice` 与 toast **两条通道都写**。
///
/// `notice` 只在选择器里有一行提示（`render_picker`），设置面板与标题栏菜单里没有
/// 它；只写状态等于用户看不到。toast 走 `window.push_notification`，与结构树「复制
/// 路径」同一通道；`notice` 保留是为了让状态可断言、且回选择器后仍能看到。
pub(super) fn notify_success(
    host: &ProjectUiHost,
    window: &mut Window,
    cx: &mut App,
    message: impl Into<String>,
) {
    let message = message.into();
    host.state.borrow_mut().notice = Some(message.clone());
    window.push_notification(
        gpui_kit::component::notification::Notification::success(message),
        cx,
    );
    host.notify(cx);
}

/// 动作失败反馈：与 [`notify_success`] 同两条通道，toast 用 `error` 变体。
pub(super) fn notify_error(
    host: &ProjectUiHost,
    window: &mut Window,
    cx: &mut App,
    message: impl Into<String>,
) {
    let message = message.into();
    host.state.borrow_mut().notice = Some(message.clone());
    window.push_notification(
        gpui_kit::component::notification::Notification::error(message),
        cx,
    );
    host.notify(cx);
}

/// 当前项目在名册中的条目（会话只存根 / 名，其余信息按根反查）。
pub(super) fn current_summary(host: &ProjectUiHost) -> Option<ProjectSummary> {
    let root = host.root()?;
    project_service::list_all()
        .ok()?
        .into_iter()
        .find(|p| p.path == root)
}
