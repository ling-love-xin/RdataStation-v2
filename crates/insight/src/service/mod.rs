//! 洞察服务层（M8）。
//!
//! 与 `insight_engine`（列画像计算）的区别：本层做**编排与状态**，不实现统计算法。
//! 当前包含规则索引同步（[`indexer`]）与规则目录监听（[`watcher`]）。
//!
//! 规划内容（Phase 1 起）：`profile`（画像编排）、`quality`（表级评估编排）、
//! `rules`（规则管理）、`snapshot`（快照历史）。服务层统一经 [`crate::with_rules`]
//! 取规则集，不在各处自行加锁。

pub mod indexer;
pub mod watcher;
