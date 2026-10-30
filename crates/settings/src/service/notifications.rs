//! 通知设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::SettingsService;


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
            Slot::NotificationWarningAutohideSecs => {
                let Some(n) = value.as_number() else {
                    return false;
                };
                if !(0.0..=3600.0).contains(&n) {
                    return false;
                }
                SettingsService::set_notify_warning_autohide_secs(n as u32, cx);
            }
            Slot::NotificationErrorAutohide => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_notify_error_autohide(on, cx);
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
            Slot::NotificationDoNotDisturb => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_notify_do_not_disturb(on, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 设置并持久化成功/信息类通知自动消失秒数（0 = 不自动消失）。
    pub fn set_notify_success_autohide_secs(secs: u32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.success_autohide_secs = secs.clamp(0, 3600);
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
    /// 设置并持久化错误类通知是否自动消失（默认关闭，必须手动关闭错误通知）。
    pub fn set_notify_error_autohide(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.error_autohide = on;
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
    /// 设置并持久化勿扰模式（开启后通知只进历史，不弹出）。
    pub fn set_notify_do_not_disturb(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.notifications.do_not_disturb = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
}
