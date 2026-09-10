//! 日志模块
//!
//! 提供统一的日志记录、持久化和查询功能。
//!
//! ## 架构设计
//!
//! ```text
//! tracing 宏 (info!/error!/warn!...)
//!     ↓
//! DatabaseLogLayer (自定义 Layer)
//!     ↓ channel
//! LogStore::flush_records() → SQLite
//!     ↓
//! Tauri Commands → 前端查询
//! ```
//!
//! ## 输出目标
//!
//! | 目标     | 说明                              |
//! |----------|-----------------------------------|
//! | stderr   | 控制台实时输出                    |
//! | 文件     | 按天滚动，`{log_dir}/app.YYYY-MM-DD` |
//! | SQLite   | 持久化到 global.db → app_logs 表 |
//!
//! ## 使用方式
//!
//! 启动时调用 `init_logging()` 初始化全局 subscriber。
//! 业务代码正常使用 `tracing::info!()` 等宏即可，无需额外操作。

pub mod config;
pub mod record;
pub mod redact;
pub mod subscriber;

use crate::core::logging::record::LogLevel;
use crate::core::persistence::log_store::LogStore;
use std::path::PathBuf;
use std::sync::Arc;

static LOG_STORE: std::sync::OnceLock<Arc<LogStore>> = std::sync::OnceLock::new();

pub fn get_log_store() -> Option<Arc<LogStore>> {
    LOG_STORE.get().cloned()
}

pub fn set_log_store(store: Arc<LogStore>) {
    let _ = LOG_STORE.set(store);
}

/// 初始化完整日志系统（stderr + 文件滚动 + 数据库持久化 + reload handle）
///
/// 一次调用完成所有初始化，返回 JoinHandle 供应用关闭时 abort。
pub fn init_logging(
    log_dir: &PathBuf,
    min_level: LogLevel,
    retention_days: u32,
    log_store: Arc<LogStore>,
) -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    let rx = subscriber::init_tracing_with_db(log_dir, min_level, retention_days)?;
    set_log_store(log_store.clone());
    let handle = subscriber::spawn_log_consumer(rx, log_store);
    Ok(handle)
}

/// 刷新日志缓冲区（consumer 异步写入，无需手动 flush）
pub async fn flush_logs() -> Result<(), crate::core::error::CoreError> {
    // consumer 通过 channel 异步批量写入，已在定时/阈值触发下自动落盘
    Ok(())
}

/// 获取当前会话 ID
pub fn session_id() -> String {
    subscriber::get_session_id()
}
