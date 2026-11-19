//! SQL 执行基础设施服务层（自 v1 `core/services` 迁移）
//!
//! - `sql_service`：SQL 统一执行入口（连接管理 + 缓存 + 历史记录）
//! - `sql_parser_service`：SQL 解析/语句类型识别
//! - `duckdb_service`：DuckDB 专用服务（临时表 / 行转 Arrow / 列洞察数据）
//! - `execution_service`：执行编排（串行/并发执行）
//! - `policy_guard`：环境策略解算与执行前拦截（原型 ⑨ 执行语义矩阵）
//! - `vault_service`：密钥来源三档的壳层入口（状态 / 切档编排）
//! - `snapshot_service`：快照持久化
//! - `result_types`：结果集类型（自 result_service 抽取，随 workbench 轮次归属确认）

pub mod connection_probe;
pub mod driver_audit;
pub mod duckdb_service;
pub mod execution_service;
pub mod policy_guard;
pub mod result_types;
pub mod snapshot_service;
pub mod sql_parser_service;
pub mod sql_service;
pub mod vault_service;
