//! Service 层 — 业务逻辑编排
//!
//! ═══════════ 架构边界 ═══════════
//! Services 层定位：业务逻辑层，位于 commands 之下、dbi/driver 之上。
//!
//!    commands ──► services ──► dbi ──► driver ──► native
//!                  (本层)       (引擎)   (trait)    (实现)
//!
//! ✅ commands 层只能调用 services，不能跨层访问 dbi
//! ✅ services 内部可以调用 dbi 引擎（DuckDbService → DuckDBEngine）
//! ✅ services 通过 ConnectionManager 间接访问 driver
//!
//! 职责分层：
//! - ConnectionService / ConnectionManager → 连接生命周期
//! - SqlService → SQL 执行 + 历史 + 缓存
//! - **MetadataService → 元数据浏览统一入口（MetadataBrowser 主路径 + Database fallback）**
//! - DuckDbService → DuckDB 加速/联邦查询
//! - ResultService → 结果集管理
//! - insight_engine / quality_scorer / table_profile_service → 数据质量分析

pub mod connection_manager;
pub mod connection_service;
pub mod driver_service;
pub mod duckdb_service;
pub mod execution_service;
pub mod insight_engine;
pub mod metadata_service;
pub mod persistence_service;
pub mod plugin_bridge;
pub mod plugin_service;
pub mod quality_scorer;
pub mod result_service;
pub mod snapshot_service;
pub mod sql_parser_service;
pub mod sql_service;
pub mod table_profile_service;

pub use connection_manager::{
    get_connection_manager, ConnId, ConnectionConfig, ConnectionInfo, ConnectionManager,
    ConnectionType,
};
pub use connection_service::ConnectionService;
pub use metadata_service::MetadataService;
pub use result_service::{ColumnStats, ResultService, ResultSet};
pub use sql_service::SqlService;
