//! 通知设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::SettingsService;
use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};

/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
        Slot::NotificationSuccessAutohideSecs => {
            let Some(n) = value.as_number() else {
                return false;
            };
            // 只认 0–3600 秒（页面只给预设档；手改坏时拒绝，不猜）。
            if !(0.0..=3600.0).contains(&n) {
                return false;
            }
            SettingsService::set_notify_success_autohide_secs(n as u32, cx);
        }
        Slot::NotificationInfoAutohideSecs => {
            let Some(n) = value.as_number() else {
                return false;
            };
            if !(0.0..=3600.0).contains(&n) {
                return false;
            }
            SettingsService::set_notify_info_autohide_secs(n as u32, cx);
        }
        Slot::NotificationWarningAutohideSecs => {
            let Some(n) = value.as_number() else {
                return false;
            };
            if !(0.0..=3600.0).contains(&n) {
                return false;
            }
            SettingsService::set_notify_warning_autohide_secs(n as u32, cx);
        }
        Slot::NotificationErrorAutohideSecs => {
            let Some(n) = value.as_number() else {
                return false;
            };
            // 与成功/警告同一档位上限：只认 0–3600 秒，手改坏时拒绝，不猜。
            if !(0.0..=3600.0).contains(&n) {
                return false;
            }
            SettingsService::set_notify_error_autohide_secs(n as u32, cx);
        }
        Slot::NotificationMergeWindowSecs => {
            let Some(n) = value.as_number() else {
                return false;
            };
            if !(0.0..=3600.0).contains(&n) {
                return false;
            }
            SettingsService::set_notify_merge_window_secs(n as u32, cx);
        }
        Slot::NotificationErrorToSystem => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_notify_error_to_system(on, cx);
        }
        Slot::NotificationWarningToSystem => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_notify_warning_to_system(on, cx);
        }
        Slot::NotificationSuccessToSystem => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_notify_success_to_system(on, cx);
        }
        Slot::NotificationInfoToSystem => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_notify_info_to_system(on, cx);
        }
        Slot::NotificationSilentInApp => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_notify_silent_in_app(on, cx);
        }
        Slot::NotificationSilentSystem => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_notify_silent_system(on, cx);
        }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 设置并持久化成功类通知自动消失秒数（0 = 不自动消失）。
    pub fn set_notify_success_autohide_secs(secs: u32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.success_autohide_secs = secs.clamp(0, 3600);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化信息类通知自动消失秒数（0 = 不自动消失）。
    pub fn set_notify_info_autohide_secs(secs: u32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.info_autohide_secs = secs.clamp(0, 3600);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化警告类通知自动消失秒数（0 = 不自动消失）。
    pub fn set_notify_warning_autohide_secs(secs: u32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.warning_autohide_secs = secs.clamp(0, 3600);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化错误类通知自动消失秒数（0 = 不自动消失）。
    pub fn set_notify_error_autohide_secs(secs: u32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.error_autohide_secs = secs.clamp(0, 3600);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化同类通知合并窗口秒数（0 = 不合并）。
    pub fn set_notify_merge_window_secs(secs: u32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.merge_window_secs = secs.clamp(0, 3600);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化错误通知是否推送到系统通知中心。
    pub fn set_notify_error_to_system(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.error_to_system = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化警告通知是否推送到系统通知中心。
    pub fn set_notify_warning_to_system(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.warning_to_system = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化成功通知是否推送到系统通知中心。
    pub fn set_notify_success_to_system(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.success_to_system = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化普通信息通知是否推送到系统通知中心。
    pub fn set_notify_info_to_system(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.info_to_system = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化「静音 in-app 通知」（历史照记，系统通知仍按类型开关）。
    pub fn set_notify_silent_in_app(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.silent_in_app = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 设置并持久化「静音系统通知」（in-app 弹出仍按类型与时长）。
    pub fn set_notify_silent_system(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.silent_system = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
}
