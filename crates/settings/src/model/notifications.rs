//! 通知设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notifications {
    /// 成功类通知自动消失时间（秒）。0 = 不自动消失。
    #[serde(default = "default_notify_success_autohide_secs")]
    pub success_autohide_secs: u32,
    /// 信息类通知自动消失时间（秒）。0 = 不自动消失。
    ///
    /// 早先与成功共用一个秒数，而系统通知开关却是成功/信息分开的——
    /// 「信息想静默、成功想弹」这种组合当时表达不出来。
    #[serde(default = "default_notify_info_autohide_secs")]
    pub info_autohide_secs: u32,
    /// 警告类通知自动消失时间（秒）。0 = 不自动消失。
    #[serde(default = "default_notify_warning_autohide_secs")]
    pub warning_autohide_secs: u32,
    /// 错误类通知自动消失时间（秒）。0 = 不自动消失（默认，避免漏掉错误）。
    ///
    /// 早先这里是 bool（`error_autohide`），与成功/警告的「秒数，0 = 不消失」两套表达，
    /// 设置页一处开关一处下拉、语义也对不上。改成同一个秒数口径。
    #[serde(default)]
    pub error_autohide_secs: u32,
    /// 同类通知合并窗口（秒）。短时间内连续多个同类通知合并成一个计数。0 = 不合并。
    #[serde(default = "default_notify_merge_window_secs")]
    pub merge_window_secs: u32,
    /// 错误通知是否推送到系统通知中心。
    #[serde(default = "default_notify_error_to_system")]
    pub error_to_system: bool,
    /// 警告通知是否推送到系统通知中心。
    #[serde(default)]
    pub warning_to_system: bool,
    /// 成功通知是否推送到系统通知中心。
    #[serde(default)]
    pub success_to_system: bool,
    /// 信息通知是否推送到系统通知中心。
    #[serde(default)]
    pub info_to_system: bool,
    /// 静音 in-app 弹出通知：历史照常记录，系统通知仍按类型开关。
    ///
    /// 早先这里是单一的「勿扰」（`do_not_disturb`），一刀切同时关掉弹出与系统通知，
    /// 「全屏工作时只留系统通知」这种组合表达不出来。拆成两个独立开关。
    ///
    /// `alias` 承接旧配置文件里的 `do_not_disturb`：升级后至少保住「不弹」这一半，
    /// 不至于把当时开着的勿扰静默丢掉（旧值 `true` 也意味着本来就不该弹）。
    /// 另一半（`silent_system`）无法从同一个键读两次，只能回到默认关闭，
    /// 即旧勿扰用户升级后仍会收到系统通知——方向是「变吵」，不会漏消息。
    #[serde(default, alias = "do_not_disturb")]
    pub silent_in_app: bool,
    /// 静音系统通知：不推送到操作系统通知中心，in-app 弹出仍按类型与时长。
    #[serde(default)]
    pub silent_system: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            success_autohide_secs: default_notify_success_autohide_secs(),
            info_autohide_secs: default_notify_info_autohide_secs(),
            warning_autohide_secs: default_notify_warning_autohide_secs(),
            error_autohide_secs: 0,
            merge_window_secs: default_notify_merge_window_secs(),
            error_to_system: default_notify_error_to_system(),
            warning_to_system: false,
            success_to_system: false,
            info_to_system: false,
            silent_in_app: false,
            silent_system: false,
        }
    }
}

fn default_notify_success_autohide_secs() -> u32 {
    3
}

fn default_notify_info_autohide_secs() -> u32 {
    3
}

fn default_notify_warning_autohide_secs() -> u32 {
    5
}

fn default_notify_merge_window_secs() -> u32 {
    2
}

fn default_notify_error_to_system() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::Notifications;

    /// 旧配置里的 `do_not_disturb` 通过 `alias` 落到 `silent_in_app`：
    /// 升级不能把当时开着的勿扰静默丢掉（那会让通知突然又弹出来）。
    #[test]
    fn legacy_do_not_disturb_migrates_to_silent_in_app() {
        let n: Notifications = serde_json::from_str(r#"{"do_not_disturb": true}"#)
            .expect("旧配置应能解析");
        assert!(n.silent_in_app, "旧的勿扰至少应保住「不弹」这一半");
        assert!(!n.silent_system, "同一个键读不到第二个字段，系统通知回到默认");
        // 其余字段仍取默认
        assert_eq!(n.success_autohide_secs, 3);
        assert_eq!(n.info_autohide_secs, 3);
        assert_eq!(n.warning_autohide_secs, 5);
        assert_eq!(n.error_autohide_secs, 0);
        assert_eq!(n.merge_window_secs, 2);
        assert!(n.error_to_system);
    }

    /// 未知键（更早版本删掉的字段、别人手写的垃圾）不该让整份配置解析失败。
    #[test]
    fn unknown_keys_are_ignored() {
        let n: Notifications =
            serde_json::from_str(r#"{"legacy_unknown": 1}"#).expect("未知键应被忽略");
        assert!(!n.silent_in_app);
        assert!(!n.silent_system);
    }
}
