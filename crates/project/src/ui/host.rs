//! 宿主桥辅助：只读守卫、动作反馈通道（`notice` + toast）、当前项目在名册中的条目。

use super::*;

/// 只读守卫的判据：**只拦「当前项目自身」的写操作**。
///
/// 只读是因为另一实例持有当前项目的 `.RSmeta` 写锁；它保护的是**当前项目**的磁盘与
/// 配置，名册里别的项目不在这个锁的范围里——项目中心（覆盖层）里对别的项目固定 /
/// 归档 / 移出 / 删除都该照常可做，不能被当前项目的只读态一并置灰。
///
/// `target` 是本次写操作真正作用的项目根；目标不明（`None`）时保守拦下。
pub(super) fn write_blocked(
    host: &ProjectUiHost,
    target: Option<&Path>,
    cx: &mut App,
    action: &str,
) -> bool {
    if !host.state.borrow().read_only {
        return false;
    }
    let guarded = match (host.root(), target) {
        (Some(root), Some(target)) => root.as_path() == target,
        _ => true,
    };
    if !guarded {
        return false;
    }
    host.state.borrow_mut().notice = Some(format!("只读模式：不允许{action}"));
    host.notify(cx);
    true
}

/// 当前项目自身的写操作（项目信息 / 归档 / 默认连接 / 版本台账）的只读守卫。
///
/// 被拦截时写入提示并返回 `true`。
pub(super) fn read_only_blocked(host: &ProjectUiHost, cx: &mut App, action: &str) -> bool {
    write_blocked(host, host.root().as_deref(), cx, action)
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
