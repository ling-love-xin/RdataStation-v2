//! 通知设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notifications {
    /// 成功/信息类通知自动消失时间（秒）。0 = 不自动消失。
    #[serde(default = "default_notify_success_autohide_secs")]
    pub success_autohide_secs: u32,
    /// 警告类通知自动消失时间（秒）。0 = 不自动消失。
    #[serde(default = "default_notify_warning_autohide_secs")]
    pub warning_autohide_secs: u32,
    /// 错误类通知是否自动消失（默认 false，必须手动关闭）。
    #[serde(default)]
    pub error_autohide: bool,
    /// 同类通知合并窗口（秒）。短时间内连续多个同类通知合并成一个计数。0 = 不合并。
    #[serde(default = "default_notify_merge_window_secs")]
    pub merge_window_secs: u32,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            success_autohide_secs: default_notify_success_autohide_secs(),
            warning_autohide_secs: default_notify_warning_autohide_secs(),
            error_autohide: false,
            merge_window_secs: default_notify_merge_window_secs(),
        }
    }
}

fn default_notify_success_autohide_secs() -> u32 {
    3
}

fn default_notify_warning_autohide_secs() -> u32 {
    5
}

fn default_notify_merge_window_secs() -> u32 {
    2
}
