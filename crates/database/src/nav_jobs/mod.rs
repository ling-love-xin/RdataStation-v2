//! 数据源导航后台任务（加载 / 预热 / 预取）。
//!
//! 为什么需要：元数据内省会走数据库驱动（依赖 tokio），在 UI 线程上 `block_on`
//! 会冻结界面。本模块用**一个分发线程 + 每任务独立工作线程**处理任务，视图只提交
//! 任务、轮询进度（原子量）并取回结果（结果队列），不参与阻塞。
//!
//! 每任务独立线程的原因：同步阻塞（rusqlite / 驱动查询）卡住时只挂起该线程，
//! 不拖累其他任务。**代价是并发无上限**——多节点同时展开会并发 acquire 撞满连接池
//! （真机：3 个 schema 展开 → 15 个 folder 任务抢 2 条池连接 → 排队 30s 超时）。
//! 调度分两层（B-2-1）：每连接容量 1 的 per-conn 信号量保证同连接元数据任务串行；
//! 外层全局软帽 `LOAD_GLOBAL_CAP` 限制跨连接并发上界；两层都忙的任务在许可处排队
//! （loading 状态保持，UI 显示「加载中」，不丢任务）。
//!
//! 任务类型：
//! - `LoadChildren`：导航树懒加载 / 分页加载（主线程 render 不再做 I/O）；
//! - `SearchIndex`：搜索框的跨连接索引搜索（命中行回填到导航树列表最前，`NavRow::SearchHit`）；
//! - `LoadProperties`：属性面板对象加载；
//! - `GenerateDml`：右键「生成 INSERT/UPDATE/DELETE」（先取列，再拼模板）；
//! - `TestConnection`：右键「测试连接」（独立会话探测，不注册连接池）；
//! - `Warm`（C1 预热，方案 C）：内省 catalogs / schemas 并写入 L2，可取消、有进度；
//! - `PrefetchColumns`（C2 邻接预取）：预取指定表 / 视图的列写入 L2，失败静默。
//!
//! 缓存写入由 `NavigatorService` 的 cache-aside 完成；本模块不直接碰缓存。
//!
//! 职责分离（2026-10-29）：`model` 为数据模型 / 常量，`hits` 为搜索命中模型与转换，
//! `worker` 为队列调度 / 执行体 / 对外接口，`tests` 为单测；对外路径保持 `nav_jobs::*` 不变。

mod hits;
mod model;
mod worker;
#[cfg(test)]
mod tests;

pub use hits::{
    HitSource, NameMatchMode, SearchConsumer, SearchHit, SearchKind, SearchResult, SearchTarget,
    SEARCH_LIMIT_PER_CONN, SEARCH_MAX_HITS, SEARCH_PARALLEL,
};
pub use model::{
    ColumnTarget, ConnectResult, LoadResult, PropsResult, RowEstimateResult, SqlGenResult,
    TestConnResult, JOB_TIMEOUT, PAGE_SIZE, PREFETCH_BATCH, ROW_ESTIMATE_MIN_TABLES,
};
pub use worker::{
    cancel_warm, connect_pending_dec, connect_pending_inc, drain_connect_results, drain_load_results,
    drain_props_results, drain_row_estimates, drain_search_results, drain_sql_results,
    drain_test_results, drop_conn_sem, enqueue_generate_dml, enqueue_load, enqueue_load_page,
    enqueue_locate_page, enqueue_properties, enqueue_row_estimate, enqueue_search,
    enqueue_test_connection, enqueue_warm_completion, has_pending_connect, has_pending_loads,
    has_pending_props, has_pending_row_estimates, has_pending_search, has_pending_sql,
    has_pending_test, JobPriority, nav_result_is_stale, picker_catalog_schemas, prefetch_columns,
    push_search_results_for_test, submit_connect_outcome, warm_active, warm_after_connect,
    warm_progress,
};

// 泵间隔常量供 nav_view 回填泵使用（对外不可见）。
pub(crate) use worker::NAV_PUMP_INTERVAL_MS;

// 调度 / 搜索内部项仅供 tests 引用（对外不可见）。
#[cfg(test)]
pub(crate) use hits::{fts_hit_to_search_hit, fts_type_to_object_kind, nav_split_two_segment, search_targets};
#[cfg(test)]
pub(crate) use worker::{lock, per_conn_semaphore, shared, submit, Job, QueuedJob, Shared};
