//! 分析内核：联邦 / 洞察 / 结果集二次分析共用的一层装配。
//!
//! 设计见 `docs/architecture/federation/federation-kernel-design.md`。
//! 三段：通道（`channel`）→ 挂载计划（`mount`）→ 开会话（`kernel`），
//! 物化（形态 I / II）在 `materialize` 收口。会话本身仍由
//! `duckdb::accel` / `duckdb::federation::session` 提供（不重造）。

pub mod channel;
pub mod kernel;
pub mod materialize;
pub mod mount;
