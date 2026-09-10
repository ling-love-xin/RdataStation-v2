//! 日志订阅器
//!
//! 实现自定义 tracing Layer，拦截所有 tracing 事件，
//! 同时输出到：
//! 1. 文件（tracing-appender 滚动日志）
//! 2. stderr（控制台）
//! 3. 数据库（通过 channel 异步批量写入 LogStore）
//!
//! 支持运行时通过 reload handle 动态修改日志级别。

use crate::core::logging::record::{LogLevel, LogRecord, TIMESTAMP_FMT};
use crate::core::logging::redact::redact_sensitive;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::Subscriber;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

static SESSION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// reload handle 用于运行时修改 EnvFilter
static RELOAD_HANDLE: std::sync::OnceLock<
    tracing_subscriber::reload::Handle<EnvFilter, tracing_subscriber::Registry>,
> = std::sync::OnceLock::new();

pub fn init_session_id() -> String {
    SESSION_ID
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .clone()
}

pub fn get_session_id() -> String {
    SESSION_ID
        .get()
        .cloned()
        .unwrap_or_else(|| "unknown".to_string())
}

/// 数据库日志层
pub struct DatabaseLogLayer {
    tx: tokio::sync::mpsc::UnboundedSender<LogRecord>,
}

impl DatabaseLogLayer {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<LogRecord>) -> Self {
        Self { tx }
    }
}

impl<S> Layer<S> for DatabaseLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut visitor = LogFieldVisitor::default();
        event.record(&mut visitor);

        let record = LogRecord {
            id: 0,
            timestamp: chrono::Utc::now().format(TIMESTAMP_FMT).to_string(),
            level: LogLevel::from(*meta.level()),
            target: meta.target().to_string(),
            message: redact_sensitive(&visitor.message),
            fields: if visitor.fields.is_empty() {
                None
            } else {
                match serde_json::to_string(&visitor.fields) {
                    Ok(json) => Some(json),
                    Err(e) => {
                        tracing::warn!("Failed to serialize log fields to JSON: {}", e);
                        None
                    }
                }
            },
            file: meta.file().map(|s| s.to_string()),
            line: meta.line(),
            session_id: get_session_id(),
        };

        let _ = self.tx.send(record);
    }
}

#[derive(Default)]
struct LogFieldVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl tracing::field::Visit for LogFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let field_name = field.name();
        if field_name == "message" {
            self.message = format!("{:?}", value);
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else {
            self.fields
                .push((field_name.to_string(), format!("{:?}", value)));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

/// 初始化带数据库持久化的 tracing 订阅器（含 reload handle）
///
/// 输出到 stderr + 滚动文件 + 数据库（通过 channel）。
/// 返回 receiver 端供 spawn_log_consumer 消费。
pub fn init_tracing_with_db(
    log_dir: &PathBuf,
    min_level: LogLevel,
    retention_days: u32,
) -> Result<tokio::sync::mpsc::UnboundedReceiver<LogRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    std::fs::create_dir_all(log_dir).map_err(|e| format!("Failed to create log dir: {}", e))?;

    // 启动时清理过期日志文件
    cleanup_log_files(log_dir, retention_days);

    let _session_id = init_session_id();
    let file_appender = tracing_appender::rolling::daily(log_dir, "app");

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(min_level.as_str().to_lowercase()));

    // 使用 reload layer 包装 EnvFilter，支持运行时动态修改级别
    let (filter_layer, reload_handle) = tracing_subscriber::reload::Layer::new(env_filter);

    RELOAD_HANDLE
        .set(reload_handle)
        .map_err(|_| "Reload handle already set")?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<LogRecord>();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .compact();

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .with_ansi(false)
        .compact();

    let db_layer = DatabaseLogLayer::new(tx.clone());

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(file_layer)
        .with(db_layer)
        .try_init()
        .map_err(|e| format!("Failed to initialize tracing subscriber: {}", e))?;

    Ok(rx)
}

/// 动态修改全局日志级别
pub fn reload_log_level(level: &str) -> Result<(), String> {
    let handle = RELOAD_HANDLE
        .get()
        .ok_or_else(|| "Reload handle not initialized".to_string())?;

    let new_filter = EnvFilter::new(level);
    handle
        .modify(|filter| *filter = new_filter)
        .map_err(|e| format!("Failed to reload filter: {}", e))?;

    tracing::info!("Log level reloaded to: {}", level);
    Ok(())
}

/// 清理过期日志文件
///
/// 扫描日志目录，删除超过 retention_days 天的 `app.YYYY-MM-DD` 文件。
/// 在应用启动时调用一次，防止文件无限堆积。
pub fn cleanup_log_files(log_dir: &PathBuf, retention_days: u32) {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);

    let entries = match std::fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Failed to read log directory for cleanup: {}", e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if !file_name.starts_with("app.") || file_name.len() < 14 {
            continue;
        }

        let date_str = &file_name[4..14]; // "app.2026-05-10" → "2026-05-10"
        if let Ok(file_date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
            let file_datetime = file_date.and_hms_opt(0, 0, 0).map(|d| d.and_utc());
            if let Some(file_dt) = file_datetime {
                if file_dt < cutoff {
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!("Failed to remove old log file {}: {}", path.display(), e);
                    } else {
                        tracing::info!("Removed old log file: {}", path.display());
                    }
                }
            }
        }
    }
}

/// 启动数据库日志消费任务
pub fn spawn_log_consumer(
    rx: tokio::sync::mpsc::UnboundedReceiver<LogRecord>,
    log_store: Arc<crate::core::persistence::log_store::LogStore>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rx = rx;
        let mut batch: Vec<LogRecord> = Vec::with_capacity(100);
        let mut fail_count: u64 = 0;
        loop {
            tokio::select! {
                record = rx.recv() => {
                    match record {
                        Some(r) => {
                            batch.push(r);
                            if batch.len() >= 100 {
                                if let Err(e) = write_batch_to_store(&log_store, std::mem::take(&mut batch)).await {
                                    fail_count += 1;
                                    eprintln!("Log batch write failed (#{}): {}", fail_count, e);
                                }
                                batch = Vec::with_capacity(100);
                            }
                        }
                        None => {
                            if !batch.is_empty() {
                                if let Err(e) = write_batch_to_store(&log_store, batch).await {
                                    fail_count += 1;
                                    eprintln!("Final log batch write failed (#{}): {}", fail_count, e);
                                }
                            }
                            if fail_count > 0 {
                                eprintln!("Log consumer exiting with {} total write failures", fail_count);
                            }
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    if !batch.is_empty() {
                        if let Err(e) = write_batch_to_store(&log_store, std::mem::take(&mut batch)).await {
                            fail_count += 1;
                            eprintln!("Periodic log batch write failed (#{}): {}", fail_count, e);
                        }
                        batch = Vec::with_capacity(100);
                    }
                }
            }
        }
    })
}

async fn write_batch_to_store(
    log_store: &Arc<crate::core::persistence::log_store::LogStore>,
    records: Vec<LogRecord>,
) -> Result<(), crate::core::error::CoreError> {
    log_store.flush_records(&records).await
}
