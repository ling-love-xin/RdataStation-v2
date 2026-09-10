//! 日志记录数据模型
//!
//! 定义日志记录的结构，与 SQLite app_logs 表对应。
//! 支持 Serde 序列化，用于前后端通信。

use serde::{Deserialize, Serialize};
use specta::Type;

/// ISO 8601 时间戳格式（毫秒精度）
pub const TIMESTAMP_FMT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }

    pub fn parse_level(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "TRACE" => Some(LogLevel::Trace),
            "DEBUG" => Some(LogLevel::Debug),
            "INFO" => Some(LogLevel::Info),
            "WARN" => Some(LogLevel::Warn),
            "ERROR" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl From<tracing::Level> for LogLevel {
    fn from(level: tracing::Level) -> Self {
        match level {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        }
    }
}

/// 日志记录
///
/// 对应 SQLite app_logs 表结构，用于前后端通信
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogRecord {
    pub id: i32,
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub fields: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub session_id: String,
}

/// 日志查询参数
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub level: Option<LogLevel>,
    pub target: Option<String>,
    pub keyword: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
}

impl Default for LogQuery {
    fn default() -> Self {
        Self {
            page: Some(1),
            page_size: Some(50),
            level: None,
            target: None,
            keyword: None,
            start: None,
            end: None,
        }
    }
}

/// 分页日志查询结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogPage {
    pub records: Vec<LogRecord>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// 日志统计
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogStats {
    pub total: u32,
    pub by_level: LogLevelCounts,
    pub by_target: Vec<TargetStat>,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
}

/// 各级别日志计数
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogLevelCounts {
    pub trace: u32,
    pub debug: u32,
    pub info: u32,
    pub warn: u32,
    pub error: u32,
}

/// 模块日志计数
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TargetStat {
    pub target: String,
    pub count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::CoreError;

    #[test]
    fn test_log_level_from_str_all_variants() {
        assert_eq!(LogLevel::parse_level("TRACE"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::parse_level("trace"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::parse_level("Debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse_level("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse_level("Warn"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse_level("ERROR"), Some(LogLevel::Error));
    }

    #[test]
    fn test_log_level_from_str_invalid() {
        assert_eq!(LogLevel::parse_level("CRITICAL"), None);
        assert_eq!(LogLevel::parse_level(""), None);
        assert_eq!(LogLevel::parse_level("INVALID"), None);
    }

    #[test]
    fn test_log_level_as_str_roundtrip() -> Result<(), CoreError> {
        for original in &[
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            let s = original.as_str();
            let parsed = LogLevel::parse_level(s)
                .ok_or_else(|| CoreError::from(format!("failed to parse level: {}", s)))?;
            assert_eq!(*original, parsed);
        }
        Ok(())
    }

    #[test]
    fn test_log_level_from_tracing() {
        assert_eq!(LogLevel::from(tracing::Level::TRACE), LogLevel::Trace);
        assert_eq!(LogLevel::from(tracing::Level::DEBUG), LogLevel::Debug);
        assert_eq!(LogLevel::from(tracing::Level::INFO), LogLevel::Info);
        assert_eq!(LogLevel::from(tracing::Level::WARN), LogLevel::Warn);
        assert_eq!(LogLevel::from(tracing::Level::ERROR), LogLevel::Error);
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Error > LogLevel::Warn);
        assert!(LogLevel::Warn > LogLevel::Info);
        assert!(LogLevel::Info > LogLevel::Debug);
        assert!(LogLevel::Debug > LogLevel::Trace);
    }

    #[test]
    fn test_log_query_default() {
        let query = LogQuery::default();
        assert_eq!(query.page, Some(1));
        assert_eq!(query.page_size, Some(50));
        assert!(query.level.is_none());
        assert!(query.keyword.is_none());
    }

    #[test]
    fn test_log_page_total_pages() {
        let page = LogPage {
            records: vec![],
            total: 125,
            page: 1,
            page_size: 50,
            total_pages: 125_u32.div_ceil(50),
        };
        assert_eq!(page.total_pages, 3);

        let page = LogPage {
            records: vec![],
            total: 0,
            page: 1,
            page_size: 50,
            total_pages: 0,
        };
        assert_eq!(page.total_pages, 0);
    }

    #[test]
    fn test_log_level_serde_uppercase() {
        let level = LogLevel::Error;
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, r#""ERROR""#);

        let parsed: LogLevel = serde_json::from_str(r#""WARN""#).unwrap();
        assert_eq!(parsed, LogLevel::Warn);
    }
}
