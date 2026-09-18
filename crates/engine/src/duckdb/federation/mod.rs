//! 联邦查询：把多个外部源挂进**同一条** DuckDB 会话，做**只读**的跨源查询
//!
//! 设计文档：`docs/architecture/federation/`（`README.md` 是入口，另有原型 / 架构 / 方案）。
//!
//! # 三层（选型有实测：`crates/engine/tests/duckdb_extensions_probe.rs`）
//!
//! - **L1 官方 scanner**：`mysql` / `postgres` / `sqlite`（+ duckdb 文件 / parquet / csv / httpfs）
//!   —— 一律 `ATTACH … (READ_ONLY)`，跨源 join 尽量下推给源库；
//! - **L2 社区 scanner**：`mssql` / `oracle_scanner` / `firebird` / …（`INSTALL … FROM community`，
//!   社区扩展由 DuckDB 官方签名托管）——**没有真机验收前不算可用**；
//! - **L3 桥接兜底**：任何我们有驱动的源 —— 拉 Arrow 数据 → DuckDB 临时表 → 参与跨源 join；
//!   覆盖“两种 scanner 都没有”的库（国产库 / 私有协议 / 受限环境）。
//!
//! ADBC（`adbc` / `adbc_scanner` 扩展）是**可选层**：只在“两种 scanner 都没有、但有 ADBC 驱动”
//! 时接，不进主干（驱动分发是桌面应用的负担）。
//!
//! # 与邻居的分工
//!
//! - [`super::accel`]（加速档）：**一条源**一条专用连接（源库整库 `ATTACH … (READ_ONLY)` + `USE`）。
//!   联邦是它的推广——多条源挂在同一条连接上；本模块负责“多”出来的那些事
//!   （登记 / 别名 / 主源 / 按源刷新 / 桥接），会话基建照用 accel 那套；
//! - [`super::extensions`]：扩展的安装与状态（L2 要用）；
//! - [`super::analysis`] / [`super::temp_table`]：临时表的生命周期与登记（L3 要用）。
//!
//! # 硬约束（改这个模块前先读 `docs/architecture/federation/README.md`）
//!
//! 1. **一律只读**：外部源只以 `READ_ONLY` 挂载（写语义未定义就不开放）；本地临时对象照常允许；
//! 2. **扩展显式管理**：关掉 DuckDB 的 `autoinstall_known_extensions` /
//!    `autoload_known_extensions`（**实测默认 `true`**：SQL 里一出现扩展函数名就静默联网下载），
//!    由本模块显式安装并如实报状态；扩展目录钉在应用数据目录（可离线预置）；
//! 3. **主源语义**：未限定表名**只在主源解析**；跨源必须写 `<别名>.<schema>.<表>`；
//!    两源同名表时未限定名**报错**，不猜；
//! 4. **资源边界**：会话上钉 `memory_limit` / `temp_directory` / `max_temp_directory_size`
//!    ——跨源 join 极易把内存打爆、把系统盘写满；
//! 5. **状态如实**：挂不上的源**留在清单里并带原因**（不静默丢）；不一致（非事务快照）与代价
//!    （扫描 / 拉取行数）都要能看见。
//!
//! # 现状
//!
//! 目录已落位（本文件 + `legacy` / `registry` / `session` / `bridge`）。
//! **第一期已经能用**：`registry`（源描述 / 别名 / 快照）与 `session`（多源只读挂载 +
//! 进程内会话缓存）已落地，执行路径在 `workbench/services/editor_exec.rs` 的联邦分支上；
//! `bridge`（L3 桥接）仍只有接口草案，见 `docs/architecture/federation/federation-dev-plan.md`。

pub mod bridge;
pub mod legacy;
pub mod registry;
pub mod session;

pub use legacy::{DataSourceConfig, DataSourceType, FederationManager};
