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
                warning_autohide_secs: n.warning_autohide_secs,
                error_autohide: n.error_autohide,
                merge_window_secs: n.merge_window_secs,
                error_to_system: n.error_to_system,
                warning_to_system: n.warning_to_system,
                success_to_system: n.success_to_system,
                info_to_system: n.info_to_system,
                do_not_disturb: n.do_not_disturb,
            }
        }
        None => NotifyConfig::default(),
    }
}
