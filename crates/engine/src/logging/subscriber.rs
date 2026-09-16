//! 日志订阅器
//!
//! 实现自定义 tracing Layer，拦截所有 tracing 事件，
//! 同时输出到：
//! 1. 文件（tracing-appender 滚动日志）
//! 2. stderr（控制台）
//! 3. 数据库（通过 channel 异步批量写入 LogStore）
//!
//! 支持运行时通过 reload handle 动态修改日志级别。

use crate::logging::config::LogConfig;
use crate::logging::record::{LogLevel, LogRecord, TIMESTAMP_FMT};
use crate::logging::redact::redact_sensitive;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tracing::Subscriber;
use tracing_appender::rolling::{RollingFileAppender, RollingWriter};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriter;
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
            // 字段值同样要脱敏：连接串大多是作为字段进来的
            // （`tracing::info!(url = %url, ...)`），只脱敏 message 等于没脱
            fields: if visitor.fields.is_empty() {
                None
            } else {
                let redacted: Vec<(String, String)> = visitor
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), redact_sensitive(v)))
                    .collect();
                match serde_json::to_string(&redacted) {
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

/// 文件层的共享状态：单文件写入量与"已达上限"标记。
///
/// 由 `MakeWriter` 持有（每条事件取一次写入器，状态得跨事件活着）。
struct FileSink {
    appender: RollingFileAppender,
    /// 本文件已写入字节数（启动时以该文件既有大小起算）
    written: AtomicU64,
    /// 已达上限：只补一行说明，之后不再写文件
    truncated: AtomicBool,
    cap: u64,
}

impl FileSink {
    fn new(appender: RollingFileAppender, cap: u64, initial_bytes: u64) -> Self {
        Self {
            appender,
            written: AtomicU64::new(initial_bytes),
            truncated: AtomicBool::new(false),
            cap,
        }
    }
}

/// 逐行脱敏 + 单文件上限的滚动文件写入器工厂。
///
/// 为什么包一层：文件层是**明文落盘**，而日志里经常带连接串与错误串。
/// 库侧有 `redact_sensitive`，文件侧之前没有——等于"密码不进库、但进文件"。
/// 这里在写盘前逐行脱敏，并顺手把单文件大小卡住，与库侧同一脱敏口径。
struct RedactingMakeWriter(Arc<FileSink>);

impl<'a> MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter<'a, RollingWriter<'a>>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new(self.0.appender.make_writer(), Some(&self.0))
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        RedactingWriter::new(self.0.appender.make_writer_for(meta), Some(&self.0))
    }
}

/// 按行攒够再脱敏落盘：脱敏模式（URL / `key=value`）不能跨行匹配。
///
/// `sink` 为 `None` 时只脱敏不计数（单测用）。
struct RedactingWriter<'s, W: Write> {
    inner: W,
    sink: Option<&'s FileSink>,
    pending: Vec<u8>,
}

impl<'s, W: Write> RedactingWriter<'s, W> {
    fn new(inner: W, sink: Option<&'s FileSink>) -> Self {
        Self {
            inner,
            sink,
            pending: Vec::new(),
        }
    }

    /// 把缓冲里已经完整的行逐行落盘。
    fn drain_lines(&mut self) -> io::Result<()> {
        while let Some(pos) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            self.write_line(&line)?;
        }
        Ok(())
    }

    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        let text = redact_sensitive(&String::from_utf8_lossy(line));
        let bytes = text.as_bytes();

        if let Some(sink) = self.sink {
            if sink.written.load(Ordering::Relaxed) >= sink.cap {
                // 单文件上限：只补一行说明，之后不再增长（库与 stderr 照常）
                if !sink.truncated.swap(true, Ordering::Relaxed) {
                    let note = format!(
                        "…… 日志文件已达上限（{} MiB），本进程后续记录只写库与 stderr ……\n",
                        sink.cap / (1024 * 1024)
                    );
                    self.inner.write_all(note.as_bytes())?;
                }
                return Ok(());
            }
            sink.written.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }

        self.inner.write_all(bytes)
    }
}

impl<W: Write> Write for RedactingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        self.drain_lines()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.write_line(&line)?;
        }
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactingWriter<'_, W> {
    fn drop(&mut self) {
        // fmt 层写完一条事件就丢弃 writer，不以换行结尾的最后一行要在这里兜住
        let _ = self.flush();
    }
}

/// 当前日志文件（`app.<UTC 日期>`）已有多大——用于把"单文件上限"算准。
///
/// 日期取 UTC：`tracing-appender` 的按天滚动用的就是 UTC（名字对不上就返回 0，
/// 后果只是本次计得偏少，不影响正确性）。
fn current_log_file_size(log_dir: &Path) -> u64 {
    let name = format!("app.{}", chrono::Utc::now().format("%Y-%m-%d"));
    std::fs::metadata(log_dir.join(name))
        .map(|m| m.len())
        .unwrap_or(0)
}

/// 初始化带数据库持久化的 tracing 订阅器（含 reload handle）
///
/// 输出到 stderr + 滚动文件（逐行脱敏 + 单文件上限）+ 数据库（通过 channel）。
/// 返回 receiver 端供 `spawn_log_consumer` 消费。
///
/// 目录、级别、保留期、文件/目录上限全部取自 `config`（单一来源，不再逐个传参）。
pub fn init_tracing_with_db(
    config: &LogConfig,
) -> Result<tokio::sync::mpsc::UnboundedReceiver<LogRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    let log_dir = &config.log_dir;
    std::fs::create_dir_all(log_dir).map_err(|e| format!("Failed to create log dir: {}", e))?;

    // 启动时清理：先按天删过期的，再按总量配额删最旧的
    cleanup_log_files(log_dir, config.retention_days, config.max_dir_bytes);

    let _session_id = init_session_id();
    let file_appender = tracing_appender::rolling::daily(log_dir, "app");
    let file_sink = Arc::new(FileSink::new(
        file_appender,
        config.max_file_bytes,
        current_log_file_size(log_dir),
    ));

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.min_level.as_str().to_lowercase()));

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
        .with_writer(RedactingMakeWriter(file_sink))
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
///
/// 两道闸：① 过期（按天）——老的不要；② 配额（按字节）——总量超了就从最旧的开始删。
/// 只有①挡不住"一天内写了几个 G"（这个由写入侧的单文件上限兜）。
pub fn cleanup_log_files(log_dir: &Path, retention_days: u32, max_dir_bytes: u64) {
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

    enforce_dir_quota(log_dir, max_dir_bytes);
}

/// 目录配额：总量超过 `max_dir_bytes` 时，从文件名（带日期）最旧的开始删。
///
/// 文件名 `app.YYYY-MM-DD` 的字典序就是时间序，所以不需要读 mtime。
fn enforce_dir_quota(log_dir: &Path, max_dir_bytes: u64) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };

    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("app.") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        files.push((name.to_string(), path, meta.len()));
    }

    let mut total: u64 = files.iter().map(|(_, _, size)| *size).sum();
    if total <= max_dir_bytes {
        return;
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, path, size) in files {
        if total <= max_dir_bytes {
            break;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                total = total.saturating_sub(size);
                tracing::info!(file = %name, "Removed old log file (dir quota)");
            }
            Err(e) => tracing::warn!(file = %name, error = %e, "Failed to remove log file"),
        }
    }
}

/// 日志消费者的 flush 请求通道。
///
/// 存在意义：库写入是"每 100 条或每 1 秒"批量提交，退出时最后一批会丢。`request_flush`
/// 把"立刻提交"排进消费者，供退出钩子等待（托盘：`crates/app` 的 `on_app_quit`）。
static FLUSH_TX: std::sync::OnceLock<
    tokio::sync::mpsc::UnboundedSender<tokio::sync::oneshot::Sender<()>>,
> = std::sync::OnceLock::new();

/// 启动数据库日志消费任务
pub fn spawn_log_consumer(
    rx: tokio::sync::mpsc::UnboundedReceiver<LogRecord>,
    log_store: Arc<crate::persistence::log_store::LogStore>,
) -> tokio::task::JoinHandle<()> {
    let (flush_tx, mut flush_rx) =
        tokio::sync::mpsc::unbounded_channel::<tokio::sync::oneshot::Sender<()>>();
    // 发送者存静态：消费者不会因为"没人发 flush"而退出
    let _ = FLUSH_TX.set(flush_tx);

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
                Some(done) = flush_rx.recv() => {
                    // 被请求立即落库（退出前）：写完再回信号，调用方 await 这个信号
                    if !batch.is_empty() {
                        if let Err(e) = write_batch_to_store(&log_store, std::mem::take(&mut batch)).await {
                            fail_count += 1;
                            eprintln!("Flush log batch write failed (#{}): {}", fail_count, e);
                        }
                        batch = Vec::with_capacity(100);
                    }
                    let _ = done.send(());
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
    log_store: &Arc<crate::persistence::log_store::LogStore>,
    records: Vec<LogRecord>,
) -> Result<(), shared::error::CoreError> {
    log_store.flush_records(&records).await
}

/// 请求消费者把已入队的记录立刻落库，返回是否真的等到了。
///
/// 日志未接线（或消费者已退出）时返回 `false`；调用方（退出钩子）把 `false` 当
/// "没什么要等的"处理即可，不是错误。
pub async fn request_flush() -> bool {
    let Some(tx) = FLUSH_TX.get() else {
        return false;
    };
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    if tx.send(done_tx).is_err() {
        return false;
    }
    done_rx.await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写文件的那一条路必须与写库的口径一致：密码与连接串不进盘。
    #[test]
    fn file_writer_redacts_credentials_per_line() {
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut w = RedactingWriter::new(&mut sink, None);
            w.write_all(b"connect mysql://root:s3cret@localhost:3306/db\n")
                .unwrap();
            // 故意拆成两次写：fmt 层并不保证一条记录一次 write
            w.write_all(b"next ").unwrap();
            w.write_all(b"password=hunter2 line\n").unwrap();
        }
        let text = String::from_utf8_lossy(&sink).to_string();
        assert!(!text.contains("s3cret"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("mysql://root:***@localhost:3306/db"), "{text}");
        assert!(text.contains("password=***"), "{text}");
    }

    /// 事件结尾没有换行时，最后一行不能凭空丢掉（drop 兜底）。
    #[test]
    fn file_writer_flushes_trailing_line_without_newline() {
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut w = RedactingWriter::new(&mut sink, None);
            w.write_all(b"tail password=leak").unwrap();
        }
        let text = String::from_utf8_lossy(&sink).to_string();
        assert!(!text.contains("leak"), "{text}");
        assert!(text.contains("tail"), "{text}");
    }

    /// 单文件上限：到达上限后不再增长，但会留一行说明（库与 stderr 不受影响）。
    #[test]
    fn file_writer_stops_at_size_cap_with_one_note() {
        // 真实 appender（要一个可写目录）；断言只关心"写进去多少"
        let dir = std::env::temp_dir().join(format!("rds_logcap_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let appender = tracing_appender::rolling::daily(&dir, "app");
        let file_sink = FileSink::new(appender, 40, 0);

        let mut out: Vec<u8> = Vec::new();
        {
            let mut w = RedactingWriter::new(&mut out, Some(&file_sink));
            for i in 0..10 {
                w.write_all(format!("line-{i} 0123456789\n").as_bytes())
                    .unwrap();
            }
        }
        let text = String::from_utf8_lossy(&out).to_string();
        let lines: Vec<&str> = text.lines().collect();
        // 前几行写进去了，后面被卡住
        assert!(text.contains("line-0"), "{text}");
        assert!(!text.contains("line-9"), "{text}");
        // 只留一行说明，而不是每行都补一句
        let notes = lines.iter().filter(|l| l.contains("已达上限")).count();
        assert_eq!(notes, 1, "{text}");
        assert!(file_sink.truncated.load(Ordering::Relaxed));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
