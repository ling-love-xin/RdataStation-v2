//! 展示格式：体积、规模、相对时间与时间戳。

use super::*;

pub fn format_size(bytes: Option<i32>) -> String {
    let Some(bytes) = bytes else {
        return String::new();
    };
    let bytes = bytes.max(0) as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes < KB {
        format!("{} B", bytes as i64)
    } else if bytes < MB {
        format!("{:.1} KB", bytes / KB)
    } else if bytes < GB {
        format!("{:.1} MB", bytes / MB)
    } else {
        format!("{:.1} GB", bytes / GB)
    }
}
/// 规模文案（分析表型）：`12,480 行 × 18 列`；缺失任一维度时退化为能给出的那部分。
pub fn format_scale(rows: Option<i32>, columns: Option<i32>) -> String {
    match (rows, columns) {
        (Some(rows), Some(columns)) => format!("{} 行 × {} 列", with_thousands(rows), columns),
        (Some(rows), None) => format!("{} 行", with_thousands(rows)),
        (None, Some(columns)) => format!("{} 列", columns),
        (None, None) => String::new(),
    }
}
/// 千分位（只在整数文案上用；避免引入依赖）。
fn with_thousands(value: i32) -> String {
    let digits = value.max(0).to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
/// 相对时间：刚归档的东西说"刚刚"，久远的说"3 个月前"。
///
/// 刻意不显示绝对时间：面板窄（240px），绝对时间放不下；悬浮显示完整值由渲染层补。
pub fn format_relative_time(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds();
    if seconds < 0 {
        // 时钟回拨 / 时区异常：不显示"-3 分钟前"这种怪东西。
        return "刚刚".to_string();
    }
    const MINUTE: i64 = 60;
    const HOUR: i64 = MINUTE * 60;
    const DAY: i64 = HOUR * 24;

    if seconds < MINUTE {
        "刚刚".to_string()
    } else if seconds < HOUR {
        format!("{} 分钟前", seconds / MINUTE)
    } else if seconds < DAY {
        format!("{} 小时前", seconds / HOUR)
    } else if seconds < DAY * 30 {
        format!("{} 天前", seconds / DAY)
    } else if seconds < DAY * 365 {
        format!("{} 个月前", seconds / (DAY * 30))
    } else {
        format!("{} 年前", seconds / (DAY * 365))
    }
}
/// 绝对时间文案（详情面板用）：`2026-09-16 14:03`。
///
/// 行上用相对时间（面板窄、扫读快），详情面板是看"归档凭证"的地方，要的是精确值。
/// 时区口径与仓库既有格式化一致（`connector.rs` 同）：直接展示库里的 UTC 值，
/// 本地时区转换是全局议题，不在本模块单独改。
pub fn format_timestamp(when: DateTime<Utc>) -> String {
    when.format("%Y-%m-%d %H:%M").to_string()
}
/// 一行存档的尾巴字段（**字段优先级**：大小/规模 > 相对时间；两者都无则空串）。
pub fn tail_for(resource: &AnalyticsResource, kind: ArchiveKind, now: DateTime<Utc>) -> String {
    let detail = match kind {
        ArchiveKind::File => format_size(resource.file_size),
        ArchiveKind::Analysis => format_scale(resource.row_count, resource.column_count),
        // 引用型没有可校验的指纹与体积：明说"无指纹"，而不是留空让人以为"还没算"。
        ArchiveKind::TableRef => "无指纹".to_string(),
    };
    let modified = format_relative_time(now, resource.updated_at);
    crate::resource_view::row_tail(&detail, &modified, resource.version)
}
