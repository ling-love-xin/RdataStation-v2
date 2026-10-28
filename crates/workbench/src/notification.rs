//! 全局通知系统。
//!
//! 基于 gpui-kit 的 NotificationList 组件，提供轻量级的全局通知能力，
//! 替代部分 Dialog 弹窗场景（如操作成功、失败提示等）。
//!
//! # 特性
//!
//! - **按类型区分自动消失策略**：Success/Info/Warning 自动消失，Error 默认不自动消失
//! - **同类通知合并窗口**：短时间内连续多个同类通知合并成一个计数（如批量连接时）
//! - **设置可配**：自动消失时间、合并窗口都在设置页可调

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui_kit::{App, AppContext as _, Entity, Window};
use gpui_kit::component::notification::{Notification, NotificationList, NotificationType};
use settings::SettingsService;

/// 全局 NotificationList entity 指针。
static NOTIFICATION_LIST: OnceLock<Entity<NotificationList>> = OnceLock::new();

/// 待合并的通知缓存。
///
/// 记录最近一条推送的通知，用于合并窗口：短时间内同类通知合并计数。
struct PendingMerge {
    /// 通知类型（本地枚举，用于比较）
    type_: MergeType,
    /// 通知标题
    title: String,
    /// 首次推送时间
    first_at: Instant,
    /// 合并计数（1 = 只有一条）
    count: usize,
    /// 消息列表（合并时追加）
    messages: Vec<String>,
}

/// 通知类型的本地副本（用于比较，因为 gpui-kit 的 NotificationType 没实现 PartialEq）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeType {
    Info,
    Success,
    Warning,
    Error,
}

impl From<NotificationType> for MergeType {
    fn from(t: NotificationType) -> Self {
        match t {
            NotificationType::Info => Self::Info,
            NotificationType::Success => Self::Success,
            NotificationType::Warning => Self::Warning,
            NotificationType::Error => Self::Error,
        }
    }
}

static PENDING_MERGE: OnceLock<std::sync::Mutex<Option<PendingMerge>>> = OnceLock::new();

fn pending_merge() -> &'static std::sync::Mutex<Option<PendingMerge>> {
    PENDING_MERGE.get_or_init(|| std::sync::Mutex::new(None))
}

/// 初始化全局通知系统。
///
/// 在 WorkbenchView 初始化时调用一次，创建 NotificationList entity。
pub fn init_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList> {
    let list = cx.new(|cx| NotificationList::new(window, cx));
    let _ = NOTIFICATION_LIST.set(list.clone());
    list
}

/// 获取全局 NotificationList entity（懒初始化）。
///
/// 在 render 时调用，确保第一次 render 时创建 NotificationList。
/// 返回的 Entity 可以直接作为 child 挂载到渲染树。
pub fn ensure_notifications(window: &mut Window, cx: &mut App) -> Entity<NotificationList> {
    if let Some(list) = NOTIFICATION_LIST.get() {
        return list.clone();
    }
    let list = cx.new(|cx| NotificationList::new(window, cx));
    let _ = NOTIFICATION_LIST.set(list.clone());
    list
}

/// 获取全局 NotificationList entity。
///
/// 如果未初始化，返回 None。
pub fn notification_list() -> Option<&'static Entity<NotificationList>> {
    NOTIFICATION_LIST.get()
}

/// 读取设置里的通知配置。
fn read_notify_settings(cx: &App) -> NotifySettings {
    let settings = SettingsService::get(cx);
    NotifySettings {
        success_autohide_secs: settings.notifications.success_autohide_secs,
        warning_autohide_secs: settings.notifications.warning_autohide_secs,
        error_autohide: settings.notifications.error_autohide,
        merge_window_secs: settings.notifications.merge_window_secs,
    }
}

struct NotifySettings {
    success_autohide_secs: u32,
    warning_autohide_secs: u32,
    error_autohide: bool,
    merge_window_secs: u32,
}

/// 推送一个通知。
///
/// # 参数
///
/// * `type_` - 通知类型（Info / Success / Warning / Error）
/// * `title` - 通知标题
/// * `message` - 通知内容（可选）
/// * `cx` - App 上下文（内部从 `cx.windows()` 解析主窗口）
pub fn notify(
    type_: NotificationType,
    title: impl Into<String>,
    message: Option<impl Into<String>>,
    cx: &mut App,
) {
    let Some(list) = NOTIFICATION_LIST.get() else {
        tracing::warn!("NotificationList not initialized, dropping notification: {}", title.into());
        return;
    };

    let title = title.into();
    let msg = message.map(|m| m.into());

    let settings = read_notify_settings(cx);

    // === 合并窗口检查 ===
    let merge_window = Duration::from_secs(settings.merge_window_secs as u64);
    let merge_type = MergeType::from(type_);
    let mut guard = pending_merge().lock().unwrap();

    let merged = if let Some(ref mut pending) = *guard
        && pending.type_ == merge_type
        && pending.title == title
        && pending.first_at.elapsed() < merge_window
    {
        // 在合并窗口内 → 合并
        pending.count += 1;
        if let Some(ref m) = msg {
            pending.messages.push(m.clone());
        }
        Some((pending.count, pending.messages.clone()))
    } else {
        // 不在合并窗口内 → 清空缓存，记录新的
        *guard = Some(PendingMerge {
            type_: merge_type,
            title: title.clone(),
            first_at: Instant::now(),
            count: 1,
            messages: msg.iter().cloned().collect(),
        });
        None
    };
    drop(guard);

    // === 构造通知 ===
    let (final_title, final_message) = if let Some((count, messages)) = merged {
        // 合并后的通知：标题带计数，消息列出前几个
        let display_title = if count > 1 {
            format!("已连接 {count} 个数据库")
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

    let mut notification = match type_ {
        NotificationType::Info => Notification::info(final_title.as_str().into()),
        NotificationType::Success => Notification::success(final_title.as_str().into()),
        NotificationType::Warning => Notification::warning(final_title.as_str().into()),
        NotificationType::Error => Notification::error(final_title.as_str().into()),
    };

    if let Some(ref msg) = final_message {
        notification = notification.message(msg.as_str().into());
    }

    // 按类型设置 autohide（从设置读取）
    match type_ {
        NotificationType::Success | NotificationType::Info => {
            let secs = settings.success_autohide_secs;
            if secs == 0 {
                notification = notification.autohide(false);
            } else {
                notification = notification.autohide(true);
            }
        }
        NotificationType::Warning => {
            let secs = settings.warning_autohide_secs;
            if secs == 0 {
                notification = notification.autohide(false);
            } else {
                notification = notification.autohide(true);
            }
        }
        NotificationType::Error => {
            notification = notification.autohide(settings.error_autohide);
        }
    }

    // 单窗口应用取第一个窗口；窗口已全部关闭则丢弃（无 UI 可展示）。
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

/// 推送一个信息通知。
pub fn notify_info(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Info, title, message, cx);
}

/// 推送一个成功通知。
pub fn notify_success(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Success, title, message, cx);
}

/// 推送一个警告通知。
pub fn notify_warning(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Warning, title, message, cx);
}

/// 推送一个错误通知。
pub fn notify_error(title: impl Into<String>, message: Option<impl Into<String>>, cx: &mut App) {
    notify(NotificationType::Error, title, message, cx);
}
