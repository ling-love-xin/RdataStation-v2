//! 日志命令
//!
//! 提供前端可调用的日志查询和管理 Tauri 命令。

use crate::core::error::CoreError;
use crate::core::logging::get_log_store;
use crate::core::logging::record::{LogLevel, LogPage, LogQuery, LogRecord, LogStats};

/// 分页查询日志
#[tauri::command]
#[specta::specta]
pub async fn get_logs(
    page: Option<u32>,
    page_size: Option<u32>,
    level: Option<String>,
    target: Option<String>,
    keyword: Option<String>,
    start: Option<String>,
    end: Option<String>,
) -> Result<LogPage, CoreError> {
    let store = get_log_store().ok_or_else(|| "Log store not initialized".to_string())?;

    let query = LogQuery {
        page,
        page_size,
        level: level.and_then(|l| LogLevel::parse_level(&l)),
        target,
        keyword,
        start,
        end,
    };

    store
        .query_logs(&query)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 搜索日志（关键字搜索）
#[tauri::command]
#[specta::specta]
pub async fn search_logs(
    keyword: String,
    level: Option<String>,
    target: Option<String>,
) -> Result<LogPage, CoreError> {
    let store = get_log_store().ok_or_else(|| "Log store not initialized".to_string())?;

    let query = LogQuery {
        keyword: Some(keyword),
        level: level.and_then(|l| LogLevel::parse_level(&l)),
        target,
        ..Default::default()
    };

    store
        .query_logs(&query)
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 获取日志统计
#[tauri::command]
#[specta::specta]
pub async fn get_log_stats() -> Result<LogStats, CoreError> {
    let store = get_log_store().ok_or_else(|| "Log store not initialized".to_string())?;

    store
        .get_stats()
        .await
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 清理旧日志
#[tauri::command]
#[specta::specta]
pub async fn clear_logs(before: Option<String>) -> Result<u32, CoreError> {
    let store = get_log_store().ok_or_else(|| "Log store not initialized".to_string())?;

    store
        .cleanup(before.as_deref())
        .await
        .map(|n| n as u32)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 获取当前日志会话 ID
#[tauri::command]
#[specta::specta]
pub fn get_log_session_id() -> String {
    crate::core::logging::session_id()
}

/// 导出日志（JSON 格式）
#[tauri::command]
#[specta::specta]
pub async fn export_logs(
    level: Option<String>,
    start: Option<String>,
    end: Option<String>,
    max_results: Option<u32>,
) -> Result<Vec<LogRecord>, CoreError> {
    let store = get_log_store().ok_or_else(|| "Log store not initialized".to_string())?;

    let limit = max_results.unwrap_or(10000).min(50000);

    let query = LogQuery {
        page: Some(1),
        page_size: Some(limit),
        level: level.and_then(|l| LogLevel::parse_level(&l)),
        start,
        end,
        ..Default::default()
    };

    let page = store
        .query_logs(&query)
        .await
        .map_err(|e| CoreError::from(e.to_string()))?;
    Ok(page.records)
}

/// 动态设置全局日志级别
///
/// 通过 tracing reload handle 运行时修改 EnvFilter，
/// 立即影响所有后续日志输出的级别过滤。
/// 支持格式："info"、"debug"、"warn,my_crate=trace"等 EnvFilter 语法。
#[tauri::command]
#[specta::specta]
pub fn set_log_level(level: String) -> Result<(), CoreError> {
    use crate::core::logging::subscriber;
    subscriber::reload_log_level(&level).map_err(|e| CoreError::from(e.to_string()))
}
