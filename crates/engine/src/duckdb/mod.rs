//! DuckDB 分析引擎模块
//!
//! 本模块提供 DuckDB 本地分析引擎的完整封装，包括连接池管理、临时表、
//! 联邦查询、数据导入导出、全文搜索、查询计划分析、插件系统接口、扩展管理和性能监控。
//!
//! # 模块结构（2026-10-29 按责任域目录化）
//! - 顶层：`manager.rs` 实例管理 / `executor.rs` SQL 执行 / `accel.rs` 本地加速档 /
//!   `explain.rs` 计划分析 / `metrics.rs` 指标
//! - `federation/` 联邦查询（多源挂载 / 只读跨源查询；目录见 `docs/architecture/federation/`）
//! - `probe/` 文件探查与读取（file_probe / file_reader）
//! - `transfer/` 数据导入导出（import_export）
//! - `temp_table/` 临时表域（temp_table + analysis 分析用临时表生命周期）
//! - `search/` 全文检索（fts）
//! - `snapshot/` 快照与备份
//! - `extension/` DuckDB 扩展插件接口（plugin）
//!
//! 目录化只移动代码不重写逻辑；各目录 `mod.rs` 通配重导出 + 顶层 `pub use` 传导，
//! 外部 `engine::duckdb::X` 路径（含 `file_probe` / `file_reader` / `import_export` /
//! `analysis` / `temp_table` 等模块路径与 `probe_file` / `file_reader_function` 顶层函数）零改动。
//!
//! 2026-09-19：删除 `extensions.rs`（`ExtensionManager` / `ExtensionInfo` / `ExtensionStatus`，
//! 约 570 行）——实测零调用，只有它自己的测试在用；扩展的安装与状态实际由 `accel.rs`
//! （加速档 / 联邦）承担。台账见 `docs/architecture/data-layer-wiring-matrix.md` §5.3。

mod executor;
mod explain;
mod manager;
mod metrics;
pub mod accel;
pub mod federation;
mod probe;
pub mod transfer;
mod temp_table;
mod search;
mod snapshot;
mod extension;

// 行→Arrow 转换已下沉 `shared::duckdb_rows`（engine 瘦身第三刀前置：driver 与 engine 两侧共用），
// 此处保留命名空间薄壳，外部 `engine::duckdb::row_to_arrow::duckdb_rows_to_arrow` 路径零改动。
pub mod row_to_arrow {
    pub use shared::duckdb_rows::duckdb_rows_to_arrow;
}
// DuckDB 值渲染（时间戳/日期/时间/区间/BLOB/兜底文本）已下沉 `shared::duckdb_value_text`
// （engine 瘦身第三刀前置），此处保留命名空间薄壳，外部 `engine::duckdb::value_text::*` 路径零改动。
pub mod value_text {
    pub use shared::duckdb_value_text::*;
}

// 导出所有核心类型
pub use executor::{DuckDBExecutor, DuckDBResult};
pub use explain::{ExplainAnalyzer, PlanNode, PlanNodeType};
pub use federation::{DataSourceConfig, DataSourceType, FederationManager};
pub use manager::{DuckDBManager, MemoryStats};
pub use metrics::{DuckDBMetrics, MetricsSnapshot};

// 责任域目录：各目录 `mod.rs` 已做域内 `pub use`，这里通配传导到 duckdb 根。
// 保真点：`probe::*` 同时带出 `file_probe` / `file_reader` 模块与顶层函数；
// `temp_table::*` 带出 `temp_table` / `analysis` 模块（外部 `engine::duckdb::analysis::*` 路径）。
pub use probe::*;
pub use transfer::*;
pub use temp_table::*;
pub use snapshot::*;
pub use extension::*;
// 全文检索（fts）：原 `pub use fts::FTSManager` 导出到 duckdb 根，目录化后经 search 通配保真
pub use search::*;
// 标识符加引号的**唯一规则**在临时表模块（B7 切片二的导出 COPY 也要用）；
// 目录内保持 `pub(crate)`，这里同样以 `pub(crate)` 承接（不提升可见性）。
pub(crate) use temp_table::quote_ident;

/// 初始化一个新的 DuckDB 库文件（连接对话框「新建文件…」用）。
///
/// - 路径不存在 → 直接 `Connection::open` 建库（DuckDB 对不存在的路径自动写文件头）；
/// - 路径存在且为 0 字节残留文件（旧 `File::create` 遗留的空文件）→ 删除后重建；
/// - 路径存在且非空 → 拒绝（应改用「打开文件…」引用它，不覆盖用户数据）。
///
/// 为什么不能只建 0 字节文件：SQLite 对空文件合法初始化，而 DuckDB 对**已存在的空文件**
/// 只校验文件头、不做初始化，打开即报 `not a valid DuckDB database file`（真机反馈，
/// 2026-10-02）。「新建文件…」要保证新建出来的文件**立刻可连**，而不是留给首次连接。
pub fn initialize_duckdb_file(path: &std::path::Path) -> Result<(), shared::error::CoreError> {
    use shared::error::{CoreError, DatabaseError};
    let describe = || format!("初始化 DuckDB 库文件: {}", path.display());
    if path.exists() {
        let meta = std::fs::metadata(path).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "initialize_file_stat".to_string(),
                source: format!("{}: {}", describe(), e),
            })
        })?;
        if meta.len() > 0 {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "initialize_file".to_string(),
                source: format!(
                    "文件已存在且非空（{} 字节），请改用「打开文件…」引用它: {}",
                    meta.len(),
                    path.display()
                ),
            }));
        }
        std::fs::remove_file(path).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "initialize_file_remove".to_string(),
                source: format!("{}: {}", describe(), e),
            })
        })?;
    }
    // 打开即建库（写入文件头）；用完即关，后续由真正的连接接管。
    let _conn = duckdb::Connection::open(path).map_err(|e| {
        CoreError::database(DatabaseError::Driver {
            db_type: "duckdb".to_string(),
            operation: "initialize_file".to_string(),
            source: format!("{}: {}", describe(), e),
        })
    })?;
    Ok(())
}
