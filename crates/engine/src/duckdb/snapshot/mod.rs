//! 快照与备份（责任域：库快照 / 恢复）
//!
//! 目录化自 `duckdb/snapshot.rs`（engine 瘦身第 6 刀，2026-10-29）：
//! 只移动文件不重写逻辑；`pub mod snapshot` 与顶层项经 `pub use snapshot::*` 传导。

pub mod snapshot;

pub use snapshot::*;
