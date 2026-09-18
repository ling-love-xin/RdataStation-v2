//! 联邦**会话**：把多个源挂到同一条 DuckDB 连接上
//!
//! ## 与 `accel` 的关系（先说清楚，避免又长出一套）
//!
//! `accel` 是“**一条源一条连接**”：源库整库 `ATTACH … (READ_ONLY)` + `USE rds_src`，
//! 会话按源缓存。联邦是它的**推广**——同一条连接挂**多条**源、每条一个别名，`USE` 指向
//! **主源**。所以这里不新造连接模型，而是复用 accel 已有的三样东西：
//!
//! 1. **全局 `MOUNT_LOCK`**（`INSTALL` / `LOAD` 是进程级资源，并发建会话会撞文件）；
//! 2. **旁路回执**（`SourceNote` 那一套：挂载 / 刷新在侧线程做，结果回主线程，不占执行位）；
//! 3. **写保护双保险**（引擎侧 `READ_ONLY` 让 DuckDB 自己拒 + 编辑器侧提交前拒）。
//!
//! ## 职责（第一期落地）
//!
//! - 建立 / 复用会话（归属见架构 D2：**联邦不是独立连接**，它是分析会话那条连接上的
//!   一个“挂载状态”，这样 1c 的分析单元与联邦自然共享临时对象）；
//! - 逐源 `ATTACH … (READ_ONLY)`：**挂不上的记录原因并继续**（部分可用好过整体失败）；
//! - 主源 `USE`；**按源** `DETACH` + 重新 `ATTACH`（刷新表清单，与加速档的刷新同语义）；
//! - 会话资源边界：`memory_limit` / `temp_directory`（指向 `RDS_HOME` 下）/
//!   `max_temp_directory_size`；
//! - **取消传播**：DuckDB 侧 `InterruptHandle`（建会话时取出，跑查询时连接锁被占）
//!   **加上**各源驱动的 `cancel`（B3 已实现）——只断 DuckDB 会让界面说“已中断”而源库还在跑。
//!
//! ## 接口草案
//!
//! ```text
//! pub struct FederatedSession { ... }
//!
//! impl FederatedSession {
//!     pub fn ensure(registry: &SourceRegistry) -> Result<Self, String>;
//!     pub fn mount(&mut self, entry: &SourceEntry) -> Result<(), String>;
//!     pub fn refresh(&mut self, alias: &str) -> Result<(), String>;   // 单源重挂
//!     pub fn set_primary(&mut self, alias: &str) -> Result<(), String>;
//!     pub fn snapshot(&self) -> SessionSnapshot;   // 表清单 + 各源状态（内存快照）
//! }
//! ```
//!
//! ## 表名解析的规则（要写进用户文档与错误提示）
//!
//! - 未限定名（`SELECT * FROM orders`）**只在主源解析**；
//! - 跨源一律 `<别名>.<schema>.<表>`；
//! - 两源同名表时，未限定名**报错**（提示“orders 在 mysql·orders 与 oracle·prod 都存在，
//!   请写限定名”），**不猜**。
