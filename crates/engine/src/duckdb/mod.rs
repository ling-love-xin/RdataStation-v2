//! DuckDB 分析引擎模块
//!
//! 本模块提供 DuckDB 本地分析引擎的完整封装，包括连接池管理、临时表、
//! 联邦查询、数据导入导出、全文搜索、查询计划分析、插件系统接口、扩展管理和性能监控。
//!
//! # 模块结构
//! - `manager.rs` - 连接池管理
//! - `executor.rs` - 统一SQL执行接口
//! - `temp_table.rs` - 临时表管理
//! - `federation/` - 联邦查询（多源挂载 / 只读跨源查询；目录见 `docs/architecture/federation/`）
//! - `import_export.rs` - 数据导入导出
//! - `fts.rs` - 全文搜索
//! - `explain.rs` - 查询计划分析
//! - `plugin.rs` - 插件系统接口
//! - `extensions.rs` - DuckDB扩展管理
//! - `metrics.rs` - 性能监控与指标采集
//! - `snapshot.rs` - 快照与备份管理
//! - `analysis.rs` - 分析用临时表的生命周期（建 / 登记 / 用完即删 / 惰性清理）

mod executor;
mod explain;
mod extensions;
mod fts;
mod import_export;
mod manager;
mod metrics;
mod plugin;
mod snapshot;
mod temp_table;
pub mod analysis;
pub mod accel;
pub mod federation;
pub mod row_to_arrow;
pub mod value_text;

// 导出所有核心类型
pub use executor::{DuckDBExecutor, DuckDBResult};
pub use explain::{ExplainAnalyzer, PlanNode, PlanNodeType};
pub use extensions::{ExtensionInfo, ExtensionManager, ExtensionStatus};
pub use federation::{DataSourceConfig, DataSourceType, FederationManager};
pub use fts::FTSManager;
pub use import_export::{DataFormat, ExportConfig, ImportConfig, ImportExportManager};
pub use manager::DuckDBManager;
pub use metrics::{DuckDBMetrics, MetricsSnapshot};
pub use plugin::{PluginConnection, PluginManager, PluginPermissionLevel};
pub use snapshot::{SnapshotInfo, SnapshotManager};
pub use temp_table::{
    drop_temp_table, generate_unique_name, TempTableConfig, TempTableManager, TempTableSource,
    TempTableStats,
};
