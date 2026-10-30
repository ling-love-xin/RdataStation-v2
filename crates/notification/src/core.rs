//! 通知系统核心类型
//!
//! 定义通知类型、通知记录、通知配置等基础数据结构。

use serde::{Deserialize, Serialize};

/// 通知类型（对外枚举，和 gpui-kit 的 NotificationType 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationType {
    /// 信息（蓝色）
    Info,
    /// 成功（绿色）
    Success,
    /// 警告（黄色）
    Warning,
    /// 错误（红色）
    Error,
}

impl NotificationType {
    /// 中文标签
    pub fn label(&self) -> &'static str {
        match self {
            Self::Info => "信息",
            Self::Success => "成功",
            Self::Warning => "警告",
            Self::Error => "错误",
        }
    }

    /// 默认是否自动消失
    pub fn default_autohide(&self) -> bool {
        !matches!(self, Self::Error)
    }

    /// 默认自动消失时长（秒），0 = 不自动消失
    pub fn default_autohide_secs(&self) -> u32 {
        match self {
            Self::Info | Self::Success => 3,
            Self::Warning => 5,
            Self::Error => 0,
        }
    }

    /// 对应的 gpui-kit 通知类型
    pub fn to_gpui_type(&self) -> gpui_kit::component::notification::NotificationType {
        match self {
            Self::Info => gpui_kit::component::notification::NotificationType::Info,
            Self::Success => gpui_kit::component::notification::NotificationType::Success,
            Self::Warning => gpui_kit::component::notification::NotificationType::Warning,
            Self::Error => gpui_kit::component::notification::NotificationType::Error,
        }
    }
}

impl Default for NotificationType {
    fn default() -> Self {
        Self::Info
    }
}

/// 一条通知的完整记录（用于通知中心历史展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRecord {
    /// 唯一 ID
    pub id: String,
    /// 通知类型
    pub type_: NotificationType,
    /// 标题
    pub title: String,
    /// 详细内容（可选）
    pub message: Option<String>,
    /// 推送时间（ISO 8601 字符串）
    pub timestamp: String,
    /// 是否已读
    pub read: bool,
    /// 是否已推送到系统通知
    pub delivered_to_system: bool,
}

impl NotificationRecord {
    /// 创建一条新的通知记录
    pub fn new(type_: NotificationType, title: String, message: Option<String>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let timestamp = chrono::Local::now().to_rfc3339();
        Self {
            id,
            type_,
            title,
            message,
            timestamp,
            read: false,
            delivered_to_system: false,
        }
    }
}

/// 通知配置（从 settings crate 读出来的快照）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    /// 成功/信息类自动消失秒数（0 = 不自动消失）
    pub success_autohide_secs: u32,
    /// 警告类自动消失秒数（0 = 不自动消失）
    pub warning_autohide_secs: u32,
    /// 错误类是否自动消失
    pub error_autohide: bool,
    /// 同类通知合并窗口（秒，0 = 不合并）
    pub merge_window_secs: u32,
    /// 错误通知是否推送到系统通知中心
    pub error_to_system: bool,
    /// 警告通知是否推送到系统通知中心
    pub warning_to_system: bool,
    /// 成功通知是否推送到系统通知中心
    pub success_to_system: bool,
    /// 信息通知是否推送到系统通知中心
    pub info_to_system: bool,
    /// 勿扰模式（开启后只进历史，不弹出来）
    pub do_not_disturb: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            success_autohide_secs: 3,
            warning_autohide_secs: 5,
            error_autohide: false,
            merge_window_secs: 2,
            error_to_system: true,
            warning_to_system: false,
            success_to_system: false,
            info_to_system: false,
            do_not_disturb: false,
        }
    }
}

impl NotifyConfig {
    /// 某类型的通知是否需要推送到系统通知
    pub fn should_deliver_to_system(&self, type_: NotificationType) -> bool {
        match type_ {
            NotificationType::Error => self.error_to_system,
            NotificationType::Warning => self.warning_to_system,
            NotificationType::Success => self.success_to_system,
            NotificationType::Info => self.info_to_system,
        }
    }
}
