//! 分析内核：联邦 / 洞察 / 结果集二次分析共用的一层装配。
//!
//! 设计见 `docs/architecture/federation/federation-kernel-design.md`。
//! 这一层只负责**说清楚「跑在哪」与「源怎么挂」**，会话本身仍由
//! `duckdb::federation::session` 提供（不重造）。

pub mod channel;
