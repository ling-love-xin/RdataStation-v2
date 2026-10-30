//! 通知服务
//!
//! 提供全局通知推送、合并、历史记录等核心功能。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui_kit::component::notification::{Notification, NotificationList, NotificationType as GpuiNotificationType};
use gpui_kit::{App, AppContext as _, Entity, SharedString, Window};

use crate::core::{NotificationRecord, NotificationType};
use crate::settings::read_notify_config;

/// 全局 NotificationList 句柄（per-App 全局，非进程级单例）
#[derive(Clone)]
struct NotificationHandle(Entity<NotificationList>);

impl gpui_kit::Global for NotificationHandle {}

/// 历史记录存储（内存里存最近 50 条，启动时从文件加载，关闭时保存）
struct HistoryStore {
    records: Vec<NotificationRecord>,
    max_records: usize,
}

impl HistoryStore {
    fn new() -> Self {
        let mut store = Self {
            records: Vec::new(),
            max_records: 50,
        };
        store.load_from_file();
        store
    }

    fn push(&mut self, record: NotificationRecord) {
        self.records.insert(0, record);
        if self.records.len() > self.max_records {
            self.records.truncate(self.max_records);
        }
        self.save_to_file();
    }

    fn list(&self) -> Vec<NotificationRecord> {
        self.records.clone()
    }

    fn unread_count(&self) -> usize {
        self.records.iter().filter(|r| !r.read).count()
    }

    fn mark_all_read(&mut self) {
        for r in &mut self.records {
            r.read = true;
        }
        self.save_to_file();
    }

    fn clear(&mut self) {
        self.records.clear();
        self.save_to_file();
    }

    /// 从文件加载历史记录
    fn load_from_file(&mut self) {
        let config_dir = paths::config_dir();
        let file_path = config_dir.join("notification_history.json");
        let Ok(content) = std::fs::read_to_string(file_path) else {
            return;
        };
        if let Ok(records) = serde_json::from_str(&content) {
            self.records = records;
        }
    }

    /// 保存历史记录到文件
    fn save_to_file(&self) {
        let config_dir = paths::config_dir();
        let _ = std::fs::create_dir_all(&config_dir);
        let file_path = config_dir.join("notification_history.json");
        if let Ok(json) = serde_json::to_string_pretty(&self.records) {
            let _ = std::fs::write(file_path, json);
        }
    }
}

static HISTORY: OnceLock<Mutex<HistoryStore>> = OnceLock::new();

fn history_store() -> &'static Mutex<HistoryStore> {
    HISTORY.get_or_init(|| Mutex::new(HistoryStore::new()))
}

/// 待合并的通知缓存
struct PendingMerge {
    /// 通知类型
    type_: NotificationType,
    /// 通知标题
    title: String,
    /// 首次推送时间
    first_at: Instant,
    /// 合并计数
    count: usize,
    /// 消息列表
    messages: Vec<String>,
}

static PENDING_MERGE: OnceLock<Mutex<Option<PendingMerge>>> = OnceLock::new();

fn pending_merge() -> &'static Mutex<Option<PendingMerge>> {
    PENDING_MERGE.get_or_init(|| Mutex::new(None))
}

/// 初始化全局通知系统
///
/// 在 WorkbenchView 初始化时调用一次，创建 NotificationList entity。
pub fn init_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList> {
    let list = cx.new(|cx| NotificationList::new(window, cx));
    if !cx.has_global::<NotificationHandle>() {
        cx.set_global(NotificationHandle(list.clone()));
    }
    list
}

/// 获取全局 NotificationList entity（懒初始化）
///
/// 在 render 时调用，确保第一次 render 时创建 NotificationList。
pub fn ensure_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList> {
    if let Some(handle) = cx.try_global::<NotificationHandle>() {
        return handle.0.clone();
    }
    let list = cx.new(|cx| NotificationList::new(window, cx));
    cx.set_global(NotificationHandle(list.clone()));
    list
}

/// 推送一个通知
///
/// # 参数
/// - `type_` - 通知类型
/// - `title` - 通知标题
/// - `message` - 通知内容（可选）
/// - `cx` - App 上下文
pub fn notify(
    type_: NotificationType,
    title: impl Into<String>,
    message: Option<impl Into<String>>,
    cx: &mut App,
) {
    let title = title.into();
    let msg = message.map(|m| m.into());

    let config = read_notify_config(cx);

    // 先记录到历史（不管勿扰模式，都进历史）
    let record = NotificationRecord::new(type_, title.clone(), msg.clone());
    history_store().lock().unwrap().push(record);

    // === 勿扰模式检查 ===
    if config.do_not_disturb {
        tracing::debug!("Do not disturb mode, only record to history: {}", title);
        return;
    }

    // === 检查是否有 NotificationList ===
    let Some(NotificationHandle(list)) = cx.try_global::<NotificationHandle>().cloned() else {
        tracing::warn!("NotificationList not initialized, dropping notification: {}", title);
        return;
    };

    // === 合并窗口检查 ===
    let merge_window = Duration::from_secs(config.merge_window_secs as u64);
    let mut guard = pending_merge().lock().unwrap();

    let merged = match *guard {
        Some(ref mut pending)
            if pending.type_ == type_
                && pending.title == title
                && pending.first_at.elapsed() < merge_window =>
        {
            // 在合并窗口内 → 合并
            pending.count += 1;
            if let Some(ref m) = msg {
                pending.messages.push(m.clone());
            }
            Some((pending.count, pending.messages.clone()))
        }
        _ => {
            // 不在合并窗口内 → 清空缓存，记录新的
            *guard = Some(PendingMerge {
                type_,
                title: title.clone(),
                first_at: Instant::now(),
                count: 1,
                messages: msg.iter().cloned().collect(),
            });
            None
        }
    };
    drop(guard);

    // === 构造通知 ===
    let (final_title, final_message) = if let Some((count, messages)) = merged {
        // 合并后的通知：标题带计数，消息列出前几个
        let display_title = if count > 1 {
            format!("{} × {}", title, count)
        } else {
            title.clone()
        };
        let display_msg = if messages.len() > 3 {
            let shown: Vec<&str> = messages.iter().take(3).map(|s| s.as_str()).collect();
            format!("{}, ...等 {} 个", shown.join(", "), messages.len())
        } else {
            messages.join(", ")
        };
        (display_title, Some(display_msg))
    } else {
        (title.clone(), msg)
    };

    let gpui_type = type_.to_gpui_type();
    let mut notification = match gpui_type {
        GpuiNotificationType::Info => Notification::info(SharedString::from(final_title.as_str())),
        GpuiNotificationType::Success => Notification::success(SharedString::from(final_title.as_str())),
        GpuiNotificationType::Warning => Notification::warning(SharedString::from(final_title.as_str())),
        GpuiNotificationType::Error => Notification::error(SharedString::from(final_title.as_str())),
    };

    if let Some(ref msg) = final_message {
        notification = notification.message(SharedString::from(msg.as_str()));
    }

    // 按类型设置 autohide 和具体时长
    match type_ {
        NotificationType::Success | NotificationType::Info => {
            let secs = config.success_autohide_secs;
            if secs == 0 {
                notification = notification.autohide(false);
            } else {
                notification = notification.autohide(true).timeout(Duration::from_secs(secs as u64));
            }
        }
        NotificationType::Warning => {
            let secs = config.warning_autohide_secs;
            if secs == 0 {
                notification = notification.autohide(false);
            } else {
                notification = notification.autohide(true).timeout(Duration::from_secs(secs as u64));
            }
        }
        NotificationType::Error => {
            notification = notification.autohide(config.error_autohide);
            if config.error_autohide {
                notification = notification.timeout(Duration::from_secs(config.warning_autohide_secs as u64));
            }
        }
    }

    // === 系统通知联动 ===
    if config.should_deliver_to_system(type_) {
        notification = notification.in_app_and_system();
    }

    // 单窗口应用取第一个窗口
    let Some(window) = cx.windows().first().cloned() else {
        tracing::warn!("no window available, dropping notification");
        return;
    };
    let _ = window.update(cx, move |_, window, cx| {
        list.update(cx, |list, cx| {
            list.push(notification, window, cx);
        });
    });
}

/// 推送一个信息通知
pub fn notify_info(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Info, title, message, cx);
}

/// 推送一个成功通知
pub fn notify_success(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Success, title, message, cx);
}

/// 推送一个警告通知
pub fn notify_warning(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Warning, title, message, cx);
}

/// 推送一个错误通知
pub fn notify_error(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Error, title, message, cx);
}

/// 获取历史记录（通知中心用）
pub fn history() -> Vec<NotificationRecord> {
    history_store().lock().unwrap().list()
}

/// 获取未读计数（铃铛图标用）
pub fn unread_count() -> usize {
    history_store().lock().unwrap().unread_count()
}

/// 标记全部已读
pub fn mark_all_read() {
    history_store().lock().unwrap().mark_all_read();
}

/// 清除所有历史
pub fn clear_history() {
    history_store().lock().unwrap().clear();
}
