//! RdataStation v2 存储层（rds-store）
//!
//! 自 engine 抽取（2026-10-29，engine 瘦身第二刀）：`persistence`（元数据持久化）+
//! `migration`（迁移系统，启动编排 `global_init` 除外）+ `logging`（日志）三者强耦合同仓搬出。
//! engine 保留同名命名空间薄壳，`engine::persistence::*` / `engine::migration::*` /
//! `engine::logging::*` 路径零改动。
//!
//! 依赖方向：rds-store → rds-shared；engine → rds-store；rds-store 不得依赖 engine。

pub mod persistence;
pub mod migration;
pub mod logging;
