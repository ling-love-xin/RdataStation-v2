/**
 * DBI (Database Interface) — 统一数据访问引擎层
 *
 * ═══════════ 架构边界 ═══════════
 * DBI 层定位：执行引擎抽象层，位于 services 之下、driver 之上。
 *
 *    commands ──► services ──► dbi ──► driver ──► native
 *                  (业务逻辑)   (本层)   (trait)    (实现)
 *
 * ## 职责边界
 * - 多引擎路由：根据 SQL 特征智能选择 DriverEngine / DuckDBEngine / StreamEngine
 * - 执行模式管理：Native（原生驱动）、DuckDB（分析加速）、UserChoice（用户选择）
 * - 会话与事务管理（Session）：维护连接生命周期和结果集注册表
 * - 查询上下文传递（QueryContext / ExecutionContext）：连接 ID + 执行模式
 * - 性能统计收集（PerformanceCollector）：记录查询耗时、行数、成功率
 * - SQL 特征分析（SqlFeatures）：复杂度评分、DuckDB 加速判定
 *
 * ## 与 driver 层的边界
 * - dbi 通过 DriverEngine 调用 driver::Database trait，不直接访问 driver/native
 * - dbi 不管理连接池（属于 driver/smart_pool）
 * - dbi 不定义数据库 trait（属于 driver/traits）
 *
 * ## 禁止事项
 * - ❌ commands 直接 import dbi（commands 只访问 services）
 * - ❌ dbi 直接调用 driver/native 的实现（必须通过 Database trait）
 * - ❌ dbi 中硬编码特定数据库逻辑（属于 driver/native）
 */
pub mod context;
#[allow(clippy::module_inception)]
pub mod dbi;
pub mod engine;
pub mod performance;
pub mod session;

pub use context::{ExecutionContext, QueryContext};
pub use dbi::DBI;
pub use engine::{ExecutionEngine, ExecutionMode, QueryRouter, SqlFeatures};
pub use performance::{
    ModePerformanceStats, PerformanceCollector, PerformanceStats, QueryPerformanceRecord,
};
pub use session::{Session, SessionConfig, SessionMode};
