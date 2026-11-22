//! 设置桥接层
//!
//! 从 `settings` crate 读取通知配置，转换成 `NotifyConfig`。
//! 避免 notification crate 直接依赖 settings crate 的内部模型。

use crate::core::NotifyConfig;

/// 从 settings 读取通知配置
///
/// 如果 settings crate 还没初始化，就用默认值。
pub fn read_notify_config(cx: &gpui_kit::App) -> NotifyConfig {
    // 尝试从 settings service 读取
    let settings = cx.try_global::<settings::model::Settings>();

    match settings {
        Some(settings) => {
            let n = &settings.notifications;
            NotifyConfig {
                success_autohide_secs: n.success_autohide_secs,
                info_autohide_secs: n.info_autohide_secs,
                warning_autohide_secs: n.warning_autohide_secs,
                error_autohide_secs: n.error_autohide_secs,
                merge_window_secs: n.merge_window_secs,
                error_to_system: n.error_to_system,
                warning_to_system: n.warning_to_system,
                success_to_system: n.success_to_system,
                info_to_system: n.info_to_system,
                silent_in_app: n.silent_in_app,
                silent_system: n.silent_system,
            }
        }
        None => NotifyConfig::default(),
    }
}

/// 两条通道的静音开关 `(in_app, system)`
///
/// 状态栏每秒都可能重绘，铃铛只需要这两个 bool，不必为了一行 tooltip
/// 把整份 `NotifyConfig` 拷出来。同样只读 global 快照，不碰磁盘。
pub fn read_silent_flags(cx: &gpui_kit::App) -> (bool, bool) {
    match cx.try_global::<settings::model::Settings>() {
        Some(settings) => {
            let n = &settings.notifications;
            (n.silent_in_app, n.silent_system)
        }
        None => (false, false),
    }
}
