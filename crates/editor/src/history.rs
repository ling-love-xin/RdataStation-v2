//! 右 Dock「历史」面板的数据服务（B8）
//!
//! 数据来自**引擎的 SQL 历史存储**（`engine::persistence::history_store`）：每条执行的
//! 真实耗时 / 成功标志 / 失败原因 / 行数都在里面（写侧从 B5-1 起就是真值）。这里只做两件事：
//! 把记录投影成界面要用的形状（纯函数，可逐条断言），以及薄封装读 / 搜 / 删 / 清。
//!
//! ## 为什么要有版本号
//!
//! 面板要能“执行完自动出现新记录”，但**渲染路径不能读盘**（架构约定）。所以执行路径写完
//! 历史后 [`bump`] 一下，面板轮询 [`version`]（`AtomicU64`，O(1)）决定要不要重新加载：
//! 既不每帧读文件，也不会永远看不到新记录。
//!
//! 历史文件是**全局**的（`paths::data_dir()` 下的 `sql_history.json`），与连接、所属项目无关
//! ——同一台机器上跑过的 SQL 在这里都能查到。

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Local, Utc};
use engine::persistence::history_store::{self, SqlHistoryRecord};

/// 面板一次最多加载多少条（列表是虚拟滚动的，这个上限只是别让内存里堆几万条）
pub const HISTORY_LIMIT: usize = 500;

/// 预览最长多少字符（再长就截断——列表要单行）
const PREVIEW_CHARS: usize = 72;

/// 历史版本号（执行路径写完历史后 +1；面板轮询它决定要不要重新加载）
static VERSION: AtomicU64 = AtomicU64::new(0);

/// 执行路径报告“历史变了”
pub fn bump() {
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// 当前版本号（O(1)；面板每帧读它没有代价）
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// 列表里的一条（界面只读这个，不直接碰引擎的记录）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryItem {
    /// 记录 id（删除时用它）
    pub id: String,
    /// 原文（重放时整条送进编辑器）
    pub sql: String,
    /// 单行预览（空白折叠 + 截断）
    pub preview: String,
    /// 时间文案（相对时间；久远的给日期）
    pub time_text: String,
    /// 耗时文案
    pub duration_text: String,
    /// 行数文案（`返回 N 行` / `影响 N 行`；都没有就是 `None`）
    pub rows_text: Option<String>,
    /// 来源文案（库类型；没有就不显示）
    pub source_text: Option<String>,
    /// 【联邦】参与源文案（`源 mysql_src, pg_warehouse`；不是联邦档就没有这一项）
    pub sources_text: Option<String>,
    /// 失败原因（成功为 `None`）
    pub error: Option<String>,
    /// 当时绑定的连接（重放时带过去；`None` = 跟随当前连接）
    pub conn_id: Option<String>,
}

impl HistoryItem {
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }
}

/// 加载最近的历史（最新在前）
pub fn load(limit: usize) -> Result<Vec<HistoryItem>, String> {
    let now = Utc::now();
    history_store::get_sql_history(limit)
        .map(|records| records.iter().map(|record| item_from(record, now)).collect())
        .map_err(|error| format!("读取历史失败：{error}"))
}

/// 按关键字搜索（引擎侧对 SQL 文本做包含匹配）
pub fn search(keyword: &str, limit: usize) -> Result<Vec<HistoryItem>, String> {
    let now = Utc::now();
    history_store::search_sql_history(keyword, limit)
        .map(|records| records.iter().map(|record| item_from(record, now)).collect())
        .map_err(|error| format!("搜索历史失败：{error}"))
}

/// 删除一条
pub fn remove(id: &str) -> Result<(), String> {
    history_store::remove_sql_history(id).map_err(|error| format!("删除失败：{error}"))
}

/// 清空全部
pub fn clear() -> Result<(), String> {
    history_store::clear_sql_history().map_err(|error| format!("清空失败：{error}"))
}

/// 引擎记录 → 列表项（**纯函数**：`now` 从外面给，便于断言相对时间）
pub fn item_from(record: &SqlHistoryRecord, now: DateTime<Utc>) -> HistoryItem {
    HistoryItem {
        id: record.id.clone(),
        sql: record.sql.clone(),
        preview: preview_text(&record.sql),
        time_text: time_text(now, record.executed_at),
        duration_text: duration_text(record.duration_ms.unwrap_or(0)),
        rows_text: rows_text(record.rows_returned, record.rows_affected),
        source_text: source_text(record.db_type.as_deref(), record.channel.as_deref()),
        sources_text: sources_text(record.channel.as_deref(), record.sources.as_deref()),
        error: record.error_message.clone(),
        conn_id: record.conn_id.clone(),
    }
}

/// 单行预览：把换行与连续空白折叠成一个空格，超长截断
pub fn preview_text(sql: &str) -> String {
    let mut folded = String::with_capacity(sql.len());
    let mut last_was_space = false;
    for ch in sql.chars() {
        if ch.is_whitespace() {
            if !last_was_space && !folded.is_empty() {
                folded.push(' ');
                last_was_space = true;
            }
            continue;
        }
        last_was_space = false;
        folded.push(ch);
    }
    let trimmed = folded.trim_end();
    if trimmed.chars().count() > PREVIEW_CHARS {
        let mut text: String = trimmed.chars().take(PREVIEW_CHARS).collect();
        text.push('…');
        text
    } else {
        trimmed.to_string()
    }
}

/// 时间文案：一分钟内说“刚刚”，一天内说“N 分钟/小时前”，再久给本地日期时间
///
/// 相对时间对“刚才跑的那句”最有信息量；隔天的记录谁也不会去算差了几小时。
pub fn time_text(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let seconds = (now - at).num_seconds();
    if seconds < 0 {
        // 时钟回拨 / 记录来自“未来”：不编造相对时间，直接给日期
        return at.with_timezone(&Local).format("%m-%d %H:%M").to_string();
    }
    if seconds < 60 {
        return "刚刚".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes} 分钟前");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours} 小时前");
    }
    at.with_timezone(&Local).format("%m-%d %H:%M").to_string()
}

/// 耗时文案（与结果工具栏同一口径：不到一秒报毫秒）
pub fn duration_text(elapsed_ms: u64) -> String {
    if elapsed_ms < 1000 {
        format!("{elapsed_ms} ms")
    } else {
        format!("{:.1} s", elapsed_ms as f64 / 1000.0)
    }
}

/// 行数文案：查询报返回行数、写语句报影响行数（两者都缺就不显示这一项）
pub fn rows_text(rows_returned: Option<u64>, rows_affected: Option<u64>) -> Option<String> {
    if let Some(returned) = rows_returned {
        return Some(format!("返回 {} 行", thousands(returned as usize)));
    }
    rows_affected.map(|affected| format!("影响 {} 行", thousands(affected as usize)))
}

/// 来源文案：库类型（`mysql` / `sqlite` / …）+ **执行通道**（`·加速` / `·联邦`）
///
/// 两个维度分开来：`MYSQL·加速` 说的是“数据是 MySQL 的、语句是在本地加速档上跑的”。
/// 通道认不出就不缀（老记录没有这一项）；库类型没有就不显示。
///
/// 不在这里翻连接名：面板没有连接列表的快照（那是编辑器绑定与导航的事），
/// 拿一个 id 去猜名字不如老实显示库类型。
pub fn source_text(db_type: Option<&str>, channel: Option<&str>) -> Option<String> {
    let db_type = db_type.map(str::trim).filter(|text| !text.is_empty())?;
    let mut text = db_type.to_uppercase();
    if let Some(mark) = channel_mark(channel) {
        text.push('·');
        text.push_str(mark);
    }
    Some(text)
}

/// 通道后缀（源库档不缀：那是默认档，缀上去只是噪音）
fn channel_mark(channel: Option<&str>) -> Option<&'static str> {
    match crate::channel::ExecChannel::from_code(channel?) {
        crate::channel::ExecChannel::Source => None,
        crate::channel::ExecChannel::Accelerated => Some("加速"),
        crate::channel::ExecChannel::Federated => Some("联邦"),
    }
}

/// 【联邦】参与源文案（`源 mysql_src, pg_warehouse`）
///
/// 三个“不显示”的理由，各自都是真话：
/// - 不是联邦档（单一源库 / 本地加速没有“参与源”这回事）；
/// - 老记录没有这一项（不编一个）；
/// - 记了但是空的（没意义的标签不如不给）。
pub fn sources_text(channel: Option<&str>, sources: Option<&str>) -> Option<String> {
    if crate::channel::ExecChannel::from_code(channel?) != crate::channel::ExecChannel::Federated {
        return None;
    }
    let aliases = sources?.trim();
    if aliases.is_empty() {
        return None;
    }
    Some(format!("源 {aliases}"))
}

/// 千分位（与结果区的数字写法一致）
pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(ch);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryItem, duration_text, item_from, preview_text, rows_text, source_text, sources_text,
        time_text,
    };
    use chrono::{Duration, TimeZone as _, Utc};
    use engine::persistence::history_store::SqlHistoryRecord;

    fn record(sql: &str) -> SqlHistoryRecord {
        SqlHistoryRecord {
            id: "rec-1".to_string(),
            sql: sql.to_string(),
            conn_id: Some("conn-1".to_string()),
            db_type: Some("mysql".to_string()),
            channel: None,
            sources: None,
            executed_at: Utc.with_ymd_and_hms(2026, 9, 17, 6, 0, 0).unwrap(),
            duration_ms: Some(12),
            success: Some(true),
            error_message: None,
            rows_affected: None,
            rows_returned: Some(1204),
        }
    }

    /// 【联邦】参与源文案：只在联邦档上出现，且只在真的记了源的时候
    #[test]
    fn sources_text_only_speaks_for_the_federated_channel() {
        let aliases = Some("mysql_src, pg_warehouse");
        assert_eq!(
            sources_text(Some("federated"), aliases).as_deref(),
            Some("源 mysql_src, pg_warehouse")
        );
        assert_eq!(sources_text(Some("accelerated"), aliases), None, "加速只有一条源");
        assert_eq!(sources_text(Some("source"), aliases), None);
        assert_eq!(sources_text(Some("federated"), None), None, "老记录不编一个");
        assert_eq!(sources_text(Some("federated"), Some("   ")), None);
        assert_eq!(sources_text(None, aliases), None);
    }

    #[test]
    fn preview_folds_whitespace_and_truncates() {
        assert_eq!(
            preview_text("SELECT *\n  FROM orders\n WHERE id = 1"),
            "SELECT * FROM orders WHERE id = 1",
            "换行与连续空白折叠成一个空格（列表是单行）"
        );
        let long = format!("SELECT {}", "x".repeat(200));
        let preview = preview_text(&long);
        assert_eq!(preview.chars().count(), 73, "截断到 72 字符 + 省略号");
        assert!(preview.ends_with('…'), "{preview}");
        assert_eq!(preview_text("   "), "", "纯空白给空串（调用方据此不画）");
    }

    #[test]
    fn time_text_switches_from_relative_to_a_date() {
        let now = Utc.with_ymd_and_hms(2026, 9, 17, 12, 0, 0).unwrap();
        assert_eq!(time_text(now, now - Duration::seconds(5)), "刚刚");
        assert_eq!(time_text(now, now - Duration::minutes(3)), "3 分钟前");
        assert_eq!(time_text(now, now - Duration::hours(5)), "5 小时前");
        let old = now - Duration::days(3);
        assert_eq!(
            time_text(now, old),
            old.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string(),
            "一天以上给日期（相对时间在这儿没有信息量）"
        );
        // 时钟回拨：记录比“现在”还新，也不编造相对时间
        assert_eq!(
            time_text(now, now + Duration::minutes(5)),
            (now + Duration::minutes(5))
                .with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        );
    }

    #[test]
    fn duration_and_rows_text_are_the_real_numbers() {
        assert_eq!(duration_text(0), "0 ms");
        assert_eq!(duration_text(999), "999 ms");
        assert_eq!(duration_text(1_200), "1.2 s");
        assert_eq!(rows_text(Some(1204), None).as_deref(), Some("返回 1,204 行"));
        assert_eq!(rows_text(None, Some(3)).as_deref(), Some("影响 3 行"));
        assert_eq!(
            rows_text(Some(1), Some(3)).as_deref(),
            Some("返回 1 行"),
            "两者都有时优先返回行数（写语句不会同时有两个）"
        );
        assert_eq!(rows_text(None, None), None, "都没有就不显示这一项");
    }

    #[test]
    fn source_text_is_the_database_kind() {
        assert_eq!(source_text(Some("mysql"), None).as_deref(), Some("MYSQL"));
        assert_eq!(source_text(Some("  "), None).as_deref(), None);
        assert_eq!(source_text(None, None).as_deref(), None);
        // 【B13】通道是第二个维度：源库档不缀（默认档缀上去只是噪音）
        assert_eq!(source_text(Some("mysql"), Some("source")).as_deref(), Some("MYSQL"));
        assert_eq!(
            source_text(Some("mysql"), Some("accelerated")).as_deref(),
            Some("MYSQL·加速")
        );
        assert_eq!(
            source_text(Some("postgres"), Some("federated")).as_deref(),
            Some("POSTGRES·联邦")
        );
        // 认不出的通道码不缀（老记录里也没有这一项）
        assert_eq!(
            source_text(Some("mysql"), Some("notebook-v9")).as_deref(),
            Some("MYSQL")
        );
    }

    #[test]
    fn a_record_becomes_a_list_item() {
        let now = Utc.with_ymd_and_hms(2026, 9, 17, 6, 0, 30).unwrap();
        let item: HistoryItem = item_from(&record("SELECT 1\nFROM t"), now);
        assert_eq!(item.id, "rec-1");
        assert_eq!(item.sql, "SELECT 1\nFROM t", "原文原样留着（重放要用）");
        assert_eq!(item.preview, "SELECT 1 FROM t");
        assert_eq!(item.time_text, "刚刚");
        assert_eq!(item.duration_text, "12 ms");
        assert_eq!(item.rows_text.as_deref(), Some("返回 1,204 行"));
        assert_eq!(item.source_text.as_deref(), Some("MYSQL"));
        assert_eq!(item.conn_id.as_deref(), Some("conn-1"));
        assert!(!item.failed());
    }

    #[test]
    fn a_failed_record_keeps_its_reason() {
        let mut failed = record("SELECT nope");
        failed.success = Some(false);
        failed.error_message = Some("no such column: nope".to_string());
        failed.rows_returned = None;
        let item = item_from(&failed, failed.executed_at);
        assert!(item.failed(), "失败要能被列表标出来");
        assert_eq!(item.error.as_deref(), Some("no such column: nope"));
        assert_eq!(item.rows_text, None);
    }
}
