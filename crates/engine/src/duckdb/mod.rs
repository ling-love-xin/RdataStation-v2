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
//! - `metrics.rs` - 性能监控与指标采集
//! - `snapshot.rs` - 快照与备份管理
//! - `analysis.rs` - 分析用临时表的生命周期（建 / 登记 / 用完即删 / 惰性清理）
//! - `accel.rs` - 本地加速档（源库只读挂载；扩展清单与 `install_sql` 也在这里）
//! - `file_reader.rs` - 扩展名 → DuckDB 读取函数（唯一定义处）
//!
//! 2026-09-19：删除 `extensions.rs`（`ExtensionManager` / `ExtensionInfo` / `ExtensionStatus`，
//! 约 570 行）——实测零调用，只有它自己的测试在用；扩展的安装与状态实际由 `accel.rs`
//! （加速档 / 联邦）承担。台账见 `docs/architecture/data-layer-wiring-matrix.md` §5.3。

mod executor;
mod explain;
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
pub mod file_reader;

// 导出所有核心类型
pub use executor::{DuckDBExecutor, DuckDBResult};
pub use explain::{ExplainAnalyzer, PlanNode, PlanNodeType};
pub use file_reader::file_reader_function;
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
// 标识符加引号的**唯一规则**在临时表模块（B7 切片二的导出 COPY 也要用）
pub(crate) use temp_table::quote_ident;
