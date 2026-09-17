//! DuckDB 取值 → 展示文本（结果集 / 预览 / SQL 导出共用一套格式化）。
//!
//! 为什么单独成模块：时间戳 / 日期 / 时间 / 十进制 / 区间这些取值，**arrow 转换**与 **SQL 字面量**
//! 两处都要渲染，各写一份必然漂移——历史上就是两边都漏，于是结果集与导出里这些列**静默变 NULL**
//! （`row_to_arrow` 只覆盖 5 类变体、mock 的 `value_to_sql_literal` 只覆盖 9 类）。
//! 现在「怎么把值变成文字」只有这里一处，覆盖不全由 `display_text` 的穷尽 match 暴露。

use duckdb::types::{TimeUnit, Value};

/// 时间戳（`TimeUnit` + 原始计数）→ `YYYY-MM-DD HH:MM:SS[.ffffff]`（UTC）。
pub fn timestamp_text(unit: TimeUnit, raw: i64) -> String {
    chrono::DateTime::from_timestamp_micros(unit.to_micros(raw))
        .map(|dt| dt.naive_utc().to_string())
        .unwrap_or_else(|| raw.to_string())
}

/// `DATE`（自 Unix 纪元的**天数**）→ `YYYY-MM-DD`。
pub fn date_text(days: i32) -> String {
    chrono::DateTime::from_timestamp_micros(i64::from(days) * 86_400_000_000)
        .map(|dt| dt.date_naive().to_string())
        .unwrap_or_else(|| days.to_string())
}

/// `TIME`（自午夜的计数）→ `HH:MM:SS[.ffffff]`。
pub fn time_text(unit: TimeUnit, raw: i64) -> String {
    let micros = unit.to_micros(raw);
    let seconds = micros.div_euclid(1_000_000);
    let fraction = micros.rem_euclid(1_000_000);
    let (hour, minute, second) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if fraction == 0 {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{hour:02}:{minute:02}:{second:02}.{fraction:06}")
    }
}

/// `INTERVAL` 的三段式文本（DuckDB 认 `'3 months 2 days 01:00:00'`）。
pub fn interval_text(months: i32, days: i32, nanos: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if months != 0 {
        parts.push(format!("{months} months"));
    }
    if days != 0 {
        parts.push(format!("{days} days"));
    }
    let micros = nanos.div_euclid(1000);
    if micros != 0 || parts.is_empty() {
        let seconds = micros.div_euclid(1_000_000);
        let fraction = micros.rem_euclid(1_000_000);
        let (hour, minute, second) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
        parts.push(if fraction == 0 {
            format!("{hour:02}:{minute:02}:{second:02}")
        } else {
            format!("{hour:02}:{minute:02}:{second:02}.{fraction:06}")
        });
    }
    parts.join(" ")
}

/// 二进制 → `\xAA\xBB`（DuckDB 的 BLOB 字面量内容；展示时也读得懂）。
pub fn blob_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 4);
    for byte in bytes {
        let _ = write!(out, "\\x{byte:02X}");
    }
    out
}

/// 单值 → 展示文本（**不做 SQL 引用 / 转义**：那属于调用方）。
///
/// 返回 `None` 只在 `NULL` 时发生——这正是要区分的两件事：「值是空的」与「我们不认识这个类型」。
pub fn display_text(value: &Value) -> Option<String> {
    Some(match value {
        Value::Null => return None,
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(i) => i.to_string(),
        Value::SmallInt(i) => i.to_string(),
        Value::Int(i) => i.to_string(),
        Value::BigInt(i) => i.to_string(),
        Value::HugeInt(i) => i.to_string(),
        Value::UHugeInt(i) => i.to_string(),
        Value::UTinyInt(i) => i.to_string(),
        Value::USmallInt(i) => i.to_string(),
        Value::UInt(i) => i.to_string(),
        Value::UBigInt(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Enum(s) => s.clone(),
        Value::Timestamp(unit, raw) => timestamp_text(*unit, *raw),
        Value::Date32(days) => date_text(*days),
        Value::Time64(unit, raw) => time_text(*unit, *raw),
        Value::Interval {
            months,
            days,
            nanos,
        } => interval_text(*months, *days, *nanos),
        Value::Blob(bytes) | Value::Geometry(bytes) => blob_hex(bytes),
        Value::List(items) | Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| display_text(item).unwrap_or_else(|| "NULL".to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Struct(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|(key, item)| format!(
                    "{key}: {}",
                    display_text(item).unwrap_or_else(|| "NULL".to_string())
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Map(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, item)| format!(
                    "{}={}",
                    display_text(key).unwrap_or_else(|| "NULL".to_string()),
                    display_text(item).unwrap_or_else(|| "NULL".to_string())
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Union(inner) => display_text(inner).unwrap_or_else(|| "NULL".to_string()),
        // `Value` 是 `#[non_exhaustive]`：新版本新增变体时走这里，不辞不响地变 NULL
        other => format!("{other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_dates_and_decimals_have_text() {
        // 2024-01-02 03:04:05 UTC = 1_704_164_645 秒
        assert_eq!(
            timestamp_text(TimeUnit::Second, 1_704_164_645),
            "2024-01-02 03:04:05"
        );
        // 1970-01-01 起第 19724 天 = 2024-01-02
        assert_eq!(date_text(19724), "2024-01-02");
        assert_eq!(time_text(TimeUnit::Microsecond, 3_600_000_000), "01:00:00");
        assert_eq!(time_text(TimeUnit::Second, 5), "00:00:05");
    }

    #[test]
    fn intervals_and_blobs_render() {
        assert_eq!(interval_text(0, 0, 0), "00:00:00");
        assert_eq!(
            interval_text(1, 2, 3_600_000_000_000),
            "1 months 2 days 01:00:00"
        );
        assert_eq!(blob_hex(&[0x41, 0x0A]), "\\x41\\x0A");
    }

    #[test]
    fn null_and_containers_are_distinguishable() {
        assert!(display_text(&Value::Null).is_none(), "NULL 没有文本");
        assert_eq!(
            display_text(&Value::List(vec![Value::Int(1), Value::Null])),
            Some("[1, NULL]".to_string())
        );
    }
}
