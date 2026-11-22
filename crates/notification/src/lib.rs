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
//! # 层职责
//!
//! - `core`：纯数据（记录模型、类型、相对时间格式化），不做 I/O
//! - `service`：推送、合并、节流落盘、已读/清空，全部为进程级无状态 API
//! - `view`：状态栏铃铛 + 通知中心浮层，读数一律来自 `service`
//! - `settings`：把设置槽位映射成 `NotifyConfig`；另给铃铛提供两条通道的静音标志
//!
//! # 特性
//!
//! - **按类型区分策略**：Success / Info / Warning 各有自动消失时长，Error 默认不自动消失
//! - **同类通知合并**：合并窗口内同标题同类型合并计数，toast 用 `id1` 替换而不是堆叠
//! - **三条通道独立开关**：toast 时长（按类型）、系统通知（按类型）、以及两条通道各自的静音
//! - **通知中心**：状态栏铃铛 + 可筛选历史面板，支持单条已读/全部已读/清空
//! - **系统通知联动**：需要应用标识（`gpui::App::set_app_identity`），Windows 上缺它会被静默丢弃

pub mod core;
pub mod service;
pub mod settings;
pub mod view;

pub use core::{
    format_relative_time, MAX_HISTORY, NotificationRecord, NotificationType, NotifyConfig,
};
pub use service::{
    clear_history, ensure_notifications, flush_now, history, init_notifications, mark_all_read,
    mark_read, notify, notify_error, notify_info, notify_success, notify_warning, remove,
    unread_count,
};
pub use view::{bell, NotificationBell};
