//! L3 **桥接**：把任意源的数据搬进 DuckDB 临时表，参与跨源 join
//!
//! ## 什么时候走这条路
//!
//! 源的库**两种 scanner 都没有**时（国产库 / 私有协议 / 受限环境），或者用户明确要
//! “把这张表物化下来反复查”时。它是**兜底**：能直连的源一律走 L1 / L2（下推能力强得多）。
//!
//! ## 设计要点（第二期落地）
//!
//! 1. **条件下推**：先用 sqlglot 的 `pushdown_predicates`（台账实测可用）与本仓库
//!    `sql/filter.rs` 的改写经验，把 WHERE / 投影推给源库——少搬一行是一行；
//!    **下推不了要说清楚**（“这条筛选推不下去，将全表拉取”），不静默全表搬；
//! 2. **行数上限**：默认给上限（可配）；到上限**如实标注**“已拉 N 行，结果可能不完整”，
//!    与分段抓取的“已截断至 N 行”同一口径；
//! 3. **分页与取消**：复用 `SqlService::execute_segment` 的分段抓取（可中断）；
//! 4. **数据形状**：驱动已经给 Arrow `RecordBatch`（`QueryResult.batches`），
//!    目标侧用 [`super::super::analysis`] 的临时表生命周期（建 / 登记 / 用完即删）；
//! 5. **两种语义分开**：临时表（会话结束即消失，自动）vs 显式“导入到分析库 / 项目库”
//!    （持久，走 `duckdb/import_export`）——别让一个动作有两种说不清的后果；
//! 6. **跨源一致性**：桥接进来的是一份**副本**（数据是拉取那一刻的），结果区要标
//!    “外部源 X：N 行 · 拉取于 HH:MM”，与 L1/L2 的“实时读”区分开。
//!
//! ## 接口草案
//!
//! ```text
//! pub struct BridgeRequest {
//!     pub source: String,     // 源 id
//!     pub query: String,      // 源库上的取数 SQL（已含下推后的 WHERE / 投影）
//!     pub table: String,      // 目标临时表名
//!     pub limit: usize,       // 行数上限
//! }
//!
//! pub struct BridgeReport {
//!     pub table: String,
//!     pub rows: usize,
//!     pub truncated: bool,          // 到上限了没有
//!     pub pushed_down: Vec<String>, // 实际推下去了什么（给用户看）
//!     pub elapsed_ms: u64,
//! }
//!
//! pub fn materialize(session: &mut FederatedSession, request: BridgeRequest)
//!     -> Result<BridgeReport, String>;
//! ```
