//! 分析内核：联邦 / 洞察 / 结果集二次分析共用的一层装配。
//!
//! 设计见 `docs/architecture/federation/federation-kernel-design.md`。
//! 通道（`channel`）→ 挂载计划（`mount`）→ 开会话（`kernel`），
//! 物化（形态 I / II）在 `materialize` 收口，结果集取数面在 `result_set`。
//! 会话本身仍由 `duckdb::accel` / `duckdb::federation::session` 提供（不重造）。
//!
//! **执行面四条**（用户能选的是三档，第 2 条是内部底座）：
//! `Source`（驱动）/ `ProcessDuckDb`（进程内存单例：结果集、导出、洞察取样）/
//! `Accelerated`（单源加速会话）/ `Federated`（多源联邦会话）。

pub mod channel;
/// 全局库作为会话源（根连接已挂 `rds_global`，按 owner 取实例会话）
pub mod global;
pub mod kernel;
pub mod materialize;
pub mod mount;
/// 项目分析库里的持久表（结果集落库产物）的清单与删除口径
pub mod persistent;
pub mod result_set;
