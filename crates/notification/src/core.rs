//! 通知系统核心类型
//!
//! 定义通知类型、通知记录、通知配置等基础数据结构。
//! 本模块**不做 I/O**：记录只描述数据，落盘与合并窗口都在 `service`。

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// 历史记录条数上限（内存与磁盘共用同一口径）
pub const MAX_HISTORY: usize = 50;

/// 通知类型（对外枚举，和 gpui-kit 的 NotificationType 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum NotificationType {
    /// 信息（蓝色）
    #[default]
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

    /// 对应的 Lucide 图标（与 `to_gpui_type` 的语义一一对应）
    ///
    /// 用图标而不是 emoji：emoji 的渲染随字体走，各家机器不一样，
    /// 状态栏里还会把行高撑得不齐。
    pub fn icon(&self) -> gpui_kit::assets::IconName {
        use gpui_kit::assets::IconName;
        match self {
            Self::Info => IconName::Info,
            Self::Success => IconName::CircleCheck,
            Self::Warning => IconName::TriangleAlert,
            Self::Error => IconName::CircleX,
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

/// 一条通知的完整记录（历史与通知中心共用）
///
/// 除 `id` / `title` 外所有字段都带 `#[serde(default)]`：历史文件是本系统自己写的，
/// 但它会被用户手工改、会被上一个版本写、也可能被外部工具碰过。任何一处字段演进
/// （改名、增删）都会让「整体反序列化」直接失败并丢掉全部历史，代价远大于
/// 「缺字段就用默认值」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRecord {
    /// 唯一 ID（历史侧标识；与 toast 的 id 无关）
    ///
    /// 不设默认：ID 缺失的记录无法被「标记已读 / 删除」定位，宁可跳过这条。
    pub id: String,
    /// 通知类型
    #[serde(default)]
    pub type_: NotificationType,
    /// 标题
    ///
    /// 不设默认：没有标题的行在列表里是一块空白，跳过更有意义。
    pub title: String,
    /// 详细内容（可选）
    #[serde(default)]
    pub message: Option<String>,
    /// 最近一次推送时间（RFC 3339）；同主题合并后取最新，用于排序
    #[serde(default)]
    pub timestamp: String,
    /// 合并计数：合并窗口内同一 (类型, 标题) 反复推送时累加
    #[serde(default = "default_count")]
    pub count: u32,
    /// 是否已读
    #[serde(default)]
    pub read: bool,
}

fn default_count() -> u32 {
    1
}

impl NotificationRecord {
    /// 创建一条新的通知记录
    pub fn new(type_: NotificationType, title: String, message: Option<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            type_,
            title,
            message,
            timestamp: Local::now().to_rfc3339(),
            count: 1,
            read: false,
        }
    }

    /// 合并命中：计数 +1 并刷新时间，标题右侧据此显示 `×N`
    pub fn absorb(&mut self, message: Option<&String>) {
        self.count = self.count.saturating_add(1);
        self.timestamp = Local::now().to_rfc3339();
        self.read = false;
        // 内容取最新一次，**含 None**：toast/系统通知里的正文就是本次传入的 message。
        // 若这里只在 Some 时覆盖，会出现「这次推送没有正文，历史里却还留着上一条的正文」——
        // 同一条记录两种口径，用户点开通知中心会以为最新那次带了说明。
        // 旧版曾把多次消息拼成一行，长消息会把行撑成多行，所以不保留历史正文。
        self.message = message.cloned();
    }
}

/// 通知配置（从 settings crate 读出来的快照）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    /// 成功类自动消失秒数（0 = 不自动消失）
    pub success_autohide_secs: u32,
    /// 信息类自动消失秒数（0 = 不自动消失）
    pub info_autohide_secs: u32,
    /// 警告类自动消失秒数（0 = 不自动消失）
    pub warning_autohide_secs: u32,
    /// 错误类自动消失秒数（0 = 不自动消失）
    pub error_autohide_secs: u32,
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
    /// 静音 in-app 弹出（历史照记，系统通知仍按类型开关）
    pub silent_in_app: bool,
    /// 静音系统通知（in-app 弹出仍按类型与时长）
    pub silent_system: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            success_autohide_secs: 3,
            info_autohide_secs: 3,
            warning_autohide_secs: 5,
            error_autohide_secs: 0,
            merge_window_secs: 2,
            error_to_system: true,
            warning_to_system: false,
            success_to_system: false,
            info_to_system: false,
            silent_in_app: false,
            silent_system: false,
        }
    }
}

impl NotifyConfig {
    /// 某类型的通知是否需要推送到系统通知
    ///
    /// 只看**类型开关**，不含全局的 `silent_system`——后者是「现在全静音」的
    /// 临时动作，由 `service::notify` 与本函数取交集，好让两套语义在设置页各自可解释。
    pub fn should_deliver_to_system(&self, type_: NotificationType) -> bool {
        match type_ {
            NotificationType::Error => self.error_to_system,
            NotificationType::Warning => self.warning_to_system,
            NotificationType::Success => self.success_to_system,
            NotificationType::Info => self.info_to_system,
        }
    }
}

/// 把 RFC 3339 时间戳格式化成给人看的相对时间
///
/// 历史里存的是绝对时刻（跨重启、跨时区都得能复原），但列表里直接展示
/// `2026-10-30T12:43:25.224370+08:00` 就没法扫读，所以展示层做一次换算。
/// `now` 由调用方传入，便于测试固定基准。
pub fn format_relative_time(timestamp: &str, now: DateTime<Local>) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
        // 老版本落盘的记录可能不是 RFC 3339：原样返回，好过显示空白
        return timestamp.to_string();
    };
    let at = parsed.with_timezone(&Local);
    let secs = (now - at).num_seconds();

    // 时钟回拨 / 跨设备时间不一致时，退化为绝对时刻，避免出现「-5 分钟前」
    if secs < 0 {
        return at.format("%m-%d %H:%M").to_string();
    }
    if secs < 60 {
        return "刚刚".to_string();
    }
    if secs < 3600 {
        return format!("{} 分钟前", secs / 60);
    }

    let at_day = at.date_naive();
    let now_day = now.date_naive();
    if at_day == now_day {
        return format!("今天 {}", at.format("%H:%M"));
    }
    if now_day.pred_opt() == Some(at_day) {
        return format!("昨天 {}", at.format("%H:%M"));
    }
    at.format("%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .expect("测试用时间是明确的")
    }

    #[test]
    fn relative_time_covers_recent_ranges() {
        let now = at(2026, 10, 30, 12, 0);
        // 差 30 秒：不足一分钟，统一说「刚刚」（精确到分会让人以为刚发生过两件事）
        let stamp = (at(2026, 10, 30, 11, 59) + chrono::Duration::seconds(30)).to_rfc3339();
        assert_eq!(format_relative_time(&stamp, now), "刚刚");

        let stamp = at(2026, 10, 30, 11, 55).to_rfc3339();
        assert_eq!(format_relative_time(&stamp, now), "5 分钟前");

        // 整 60 秒是「1 分钟前」的下界，不能被并进「刚刚」
        let stamp = at(2026, 10, 30, 11, 59).to_rfc3339();
        assert_eq!(format_relative_time(&stamp, now), "1 分钟前");
    }

    #[test]
    fn relative_time_switches_to_clock_after_an_hour() {
        let now = at(2026, 10, 30, 12, 0);
        let stamp = at(2026, 10, 30, 10, 0).to_rfc3339();
        assert_eq!(format_relative_time(&stamp, now), "今天 10:00");

        let stamp = at(2026, 10, 29, 23, 0).to_rfc3339();
        assert_eq!(format_relative_time(&stamp, now), "昨天 23:00");

        let stamp = at(2026, 10, 20, 9, 30).to_rfc3339();
        assert_eq!(format_relative_time(&stamp, now), "10-20 09:30");
    }

    #[test]
    fn relative_time_degrades_on_clock_skew() {
        let now = at(2026, 10, 30, 12, 0);
        // 记录时间比 now 还晚（时钟回拨）：不能输出「-1 分钟前」
        let stamp = at(2026, 10, 30, 12, 5).to_rfc3339();
        let out = format_relative_time(&stamp, now);
        assert!(out.contains(':'), "未来时间应退化为绝对时刻，实际为 {out}");
    }

    #[test]
    fn relative_time_passes_through_garbage() {
        assert_eq!(format_relative_time("不是时间", at(2026, 10, 30, 12, 0)), "不是时间");
    }

    #[test]
    fn absorb_bumps_count_and_clears_read() {
        let mut rec = NotificationRecord::new(
            NotificationType::Error,
            "执行失败".to_string(),
            Some("旧原因".to_string()),
        );
        rec.read = true;
        rec.absorb(Some(&"新原因".to_string()));
        assert_eq!(rec.count, 2);
        assert!(!rec.read, "合并命中后应重新算未读");
        assert_eq!(rec.message.as_deref(), Some("新原因"));
    }

    /// 合并时本次没有正文，历史也必须跟着清空：否则「toast 无正文、历史有正文」，
    /// 同一条记录两套口径（旧实现只在 `Some` 时覆盖，正是这个缺陷）。
    #[test]
    fn absorb_without_message_clears_stale_body() {
        let mut rec = NotificationRecord::new(
            NotificationType::Warning,
            "批量连接".to_string(),
            Some("3 个失败".to_string()),
        );
        rec.absorb(None);
        assert_eq!(rec.count, 2);
        assert_eq!(rec.message, None, "本次没正文，历史不能留着上一条的");
    }
}
