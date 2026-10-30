//! RdataStation 通知系统
//!
//! 全局通知推送、合并、历史记录、通知中心。
//!
//! # 快速开始
//!
//! ```ignore
//! use rds_notification::{notify_success, notify_error};
//!
//! // 在任何地方推送通知
//! notify_success("连接成功", Some("已连接到 MySQL 数据库"), cx);
//! notify_error("连接失败", Some("无法连接到服务器"), cx);
//! ```
//!
//! # 特性
//!
//! - **按类型区分策略**：Success/Info/Warning 自动消失，Error 默认不自动消失
//! - **同类通知合并**：短时间内连续多个同类通知合并成一个计数
//! - **设置可配**：自动消失时间、合并窗口、系统通知开关都在设置页可调
//! - **通知中心**：状态栏铃铛图标 + 历史记录面板
//! - **系统通知联动**：重要通知同时推送到 OS 通知中心

pub mod core;
pub mod service;
pub mod settings;
pub mod view;

// 重新导出常用类型和函数
pub use core::{NotificationRecord, NotificationType, NotifyConfig};
pub use service::{
    clear_history, ensure_notifications, history, init_notifications, mark_all_read, notify,
    notify_error, notify_info, notify_success, notify_warning, unread_count,
};
pub use view::notification_bell;

// TODO: 未来导出 UI 组件
// pub mod view;
// pub use view::NotificationBell;
