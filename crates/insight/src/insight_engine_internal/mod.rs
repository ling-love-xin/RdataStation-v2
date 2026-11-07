//! 洞察分析引擎
//!
//! 承载列画像、表画像、取样统计、极值检测、直方图等核心计算逻辑。

use std::sync::OnceLock;

use tokio::sync::Semaphore;

use engine::services::duckdb_service::DuckDbService;
use shared::error::CoreError;

/// Default number of sample rows pulled from a column for display
pub const DEFAULT_SAMPLE_SIZE: usize = 5;

/// Minimum rows required to generate a histogram
pub const HISTOGRAM_MIN_ROWS: i64 = 10;

/// 取样档查询级过滤的行数上限（Q2：取样 = 对评估全表的轻量现算，不建子表）。
///
/// 迷你统计随取样现算（C7），500 行 × 列数取回 Rust 侧聚合毫秒级完成；
/// 行数选择入口留给后续 UI（决策文档 §11 记录），当前固定取样上限。
pub const SAMPLE_MINI_LIMIT: u64 = 500;

/// 洞察分析的并发上限，用于防止资源耗尽。
///
/// DuckDB 单例连接由 `std::sync::Mutex` 全局串行化，本信号量额外提供背压。
/// **刻意采用快速失败**（`try_acquire`）而非排队：同步函数内无法 await，
/// 阻塞等待会把调用线程（可能是 UI 主线程）卡住。超限时调用方收到错误、
/// 由 UI 呈现「正在分析中，请稍候」并允许重试。
/// 批量场景（如「评估全表」）应由调用方串行化，不依赖本上限兜底。
#[cfg(not(test))]
const INSIGHT_MAX_CONCURRENT: usize = 4;
/// 测试构建放宽到 64：窗口级测试并行度高（cargo 默认 test-threads = 核数），
/// 多个评估用例并发抢 4 个 permit 是「D12 配额 flake」的根因（生产无此形态）；
/// 背压语义本身属于生产，测试环境不模拟资源紧张。
#[cfg(test)]
const INSIGHT_MAX_CONCURRENT: usize = 64;

/// SQL 标识符引用（`"name"`；内部双引号双写）。
///
/// 与 `engine::duckdb::temp_table::quote_ident` 同一实现（那边是 `pub(crate)`，
/// 不跨 crate 导出，这里留洞察侧唯一来源）。**所有手拼 SQL 的列名 / 表名必须走它**：
/// CSV 表头完全可以含 `"`（如 `amount(USD)"`），裸拼会把查询拼坏（报一句与
/// 「列名带引号」无关的语法错）甚至改变语义；规则执行路径靠
/// `RuleExecutor::validate_identifiers` 拦，手拼路径靠这里的转义兜。
pub(crate) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 并发受限时的统一错误文案（面向用户，UI 直接展示）。
pub const ERR_TOO_MANY_CONCURRENT: &str = "洞察分析任务过多，请稍候重试";

static INSIGHT_SEM: OnceLock<Semaphore> = OnceLock::new();

pub(crate) fn insight_semaphore() -> &'static Semaphore {
    INSIGHT_SEM.get_or_init(|| Semaphore::new(INSIGHT_MAX_CONCURRENT))
}

/// 获取全局内存 DuckDB 连接。
///
/// 实现为 `DuckDBManager` 的**进程级单例**（`Arc<Mutex<Connection>>`），
/// 不是连接池：所有调用方共享同一条内存连接，访问由 `std::sync::Mutex` 串行化。
/// 因此临时表在同一进程内全局可见——在一条调用链上创建的表，后续调用能直接读到。
pub fn get_or_create_duckdb()
-> Result<std::sync::Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
    DuckDbService::get_or_create_duckdb()
}

pub mod column;
pub mod profile;
pub mod sample;
pub mod extremes;
pub mod histogram;
pub mod overlap;
pub mod rules;

// 重新导出常用函数
pub use column::{
    get_column_insight_full, get_column_insight_full_on,
    get_column_insight_full_with_basics, get_column_insight_full_on_with_basics,
    get_column_insights, get_column_stats_internal, get_column_stats_with_basics,
    get_column_extremes_internal, get_column_sample_internal,
};
pub use profile::{
    get_temp_table_profile, get_temp_table_profile_on,
    compute_table_column_summary, compute_table_column_summary_on,
    compute_dup_rows, compute_dup_rows_on,
};
pub use sample::{
    compute_sample_mini_stats, compute_sample_mini_stats_on,
    get_temp_table_sample, get_temp_table_sample_on,
    get_table_eda_bundle,
};
pub use extremes::detect_extremes;
pub use histogram::{get_column_histogram_internal, get_column_histogram_with_non_null};
pub use overlap::compute_column_overlap;
pub use rules::{execute_insight_rule, list_insight_rules, list_rules_for_column};
