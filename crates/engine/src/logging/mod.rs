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

// 常用入口从 `logging` 直接可达（调用方不必知道它们住在 subscriber 里）
pub use subscriber::reload_log_level;

use crate::logging::config::LogConfig;
use crate::persistence::log_store::LogStore;
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
/// 一次调用完成所有初始化，返回 JoinHandle（保存下来供退出前 flush / abort）。
pub fn init_logging(
    config: &LogConfig,
    log_store: Arc<LogStore>,
) -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    let rx = subscriber::init_tracing_with_db(config)?;
    set_log_store(log_store.clone());
    let handle = subscriber::spawn_log_consumer(rx, log_store);
    Ok(handle)
}

/// 把已入队的日志立刻落库（并等它写完）。
///
/// 库侧是"每 100 条或每 1 秒"批量提交，**进程直接退出会丢掉最后一批**；
/// 退出钩子（`crates/app` 的 `on_app_quit`）调本函数把窗口关到最后一条之间产生的日志先落库。
/// 返回 `false` = 日志未接线或消费者已退出（没什么要等的，不是错误）。
pub async fn flush_logs() -> bool {
    subscriber::request_flush().await
}

/// 按应用口径初始化日志系统（启动时调用一次）。
///
/// - 目录 / 级别 / 保留期 / 单文件上限 / 目录配额：全部取自 `config`
///   （`LogConfig::default()` 的 `log_dir` 已是 `paths::log_dir()`；级别由装配层从设置里读）
/// - 三层输出：stderr + 滚动文件（逐行脱敏 + 单文件上限）+ 全局库 `app_logs` 表
///
/// # 前置条件
///
/// 必须在 `migration::initialize_global_system()` **之后**、且在 tokio 运行时上下文里调用：
/// 库层要往全局库的 `app_logs` 表写（表由迁移创建），并起一个异步消费者任务。
/// 因此**全局库建立之前产生的日志不落文件也不落库**（那时还没有订阅者）——
/// 启动代码应在调用本函数后补记一条启动结果，否则最早的失败只能从 stderr 看。
///
/// 返回的 `JoinHandle` 即日志消费者任务；应用保持它存活（丢弃也不会中止任务）。
pub fn init_app_logging(config: &LogConfig) -> Result<tokio::task::JoinHandle<()>, String> {
    let manager = crate::migration::get_global_db_manager()
        .ok_or_else(|| "全局系统库尚未初始化，日志无法落库".to_string())?;
    let store = Arc::new(LogStore::with_config(
        manager.sqlite_pool(),
        config.max_db_records,
        config.retention_days,
    ));
    init_logging(config, store).map_err(|e| e.to_string())
}

/// 获取当前会话 ID
pub fn session_id() -> String {
    subscriber::get_session_id()
}
