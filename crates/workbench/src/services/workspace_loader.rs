//! 工作台连接数据加载器（M3 数据源连接 → 视图模型）
//!
//! 生产入口与 `DataSourceService` 共用全局系统库单例
//! （`{data_dir}/RdataStation/system/global.db`，由 `initialize_global_system` 初始化），
//! 消除"服务写一份、列表读另一份"的路径分裂。
//! `load_persisted_connections_from(dir)` 保留目录注入，供测试与单例未就绪时的降级加载。

use std::path::{Path, PathBuf};

use engine::persistence::global_db::{GlobalConnectionInfo, GlobalDatabaseManager};

use crate::view::ConnectionItem;

/// 全局系统目录：`{data_dir}/RdataStation/system`（global.db 与分析库 analytics.duckdb 均在此）。
pub fn default_global_dir() -> PathBuf {
    engine::migration::get_system_dir().unwrap_or_else(|_| {
        // 系统数据目录不可用时的兜底：退到临时目录，保持同名结构。
        std::env::temp_dir().join("RdataStation").join("system")
    })
}

/// 全局分析库（DuckDB）路径：Secret 注册、导航树与 SQL 执行的统一目标。
pub fn global_analysis_db_path() -> PathBuf {
    engine::migration::get_global_duckdb_path()
        .unwrap_or_else(|_| default_global_dir().join("analytics.duckdb"))
}

/// 从全局系统库加载连接（单例优先；未初始化时按路径降级加载）。
///
/// 返回 `(连接列表, 错误提示)`：错误提示仅在有失败时填充，连接列表为空不代表失败。
pub fn load_persisted_connections() -> (Vec<ConnectionItem>, Option<String>) {
    match engine::migration::get_global_db_manager() {
        Some(manager) => query_manager(manager),
        None => load_persisted_connections_from(&default_global_dir()),
    }
}

/// 从指定目录加载持久化连接（目录可注入，便于测试与降级加载）。
pub fn load_persisted_connections_from(dir: &Path) -> (Vec<ConnectionItem>, Option<String>) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => return (Vec::new(), Some(format!("无法启动异步运行时: {e}"))),
    };

    let sqlite = dir.join("global.db");
    let duckdb = dir.join("analytics.duckdb");

    match runtime.block_on(async {
        let manager = GlobalDatabaseManager::new(sqlite, duckdb, 2).await?;
        manager.get_global_connections(None, None).await
    }) {
        Ok(infos) => (map_infos(infos), None),
        Err(e) => (Vec::new(), Some(format!("加载全局连接失败: {e}"))),
    }
}

/// 删除连接（委托 M3 服务：作用域路由 G_/P_/GP_ + DuckDB Secret 联动清理）。
///
/// 服务未就绪时返回错误，不做"只删 global 表"的静默降级——
/// 否则 P_/GP_ 项目连接会表现为删掉了、实则仍在项目库中。
pub fn delete_connection(conn_id: &str) -> Result<(), String> {
    let service = crate::services::data_source_service::DataSourceService::global()
        .map_err(|e| format!("服务未就绪: {e}"))?;
    let runtime =
        tokio::runtime::Runtime::new().map_err(|e| format!("无法启动异步运行时: {e}"))?;
    runtime
        .block_on(service.delete(conn_id, None))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 单例查询（同步入口，内部自建运行时）。
fn query_manager(manager: &GlobalDatabaseManager) -> (Vec<ConnectionItem>, Option<String>) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => return (Vec::new(), Some(format!("无法启动异步运行时: {e}"))),
    };
    match runtime.block_on(manager.get_global_connections(None, None)) {
        Ok(infos) => (map_infos(infos), None),
        Err(e) => (Vec::new(), Some(format!("加载全局连接失败: {e}"))),
    }
}

/// 批量映射（持久化记录 → 视图模型）。
pub fn map_infos(infos: Vec<GlobalConnectionInfo>) -> Vec<ConnectionItem> {
    infos.into_iter().map(connection_item_from_info).collect()
}

/// 单条映射：`connected` 承载记录有效性（global_connections.is_active），
/// 运行时连接状态由连接服务托管（尚未接入）。
pub fn connection_item_from_info(c: GlobalConnectionInfo) -> ConnectionItem {
    ConnectionItem {
        id: c.id,
        name: c.name,
        driver: c.driver,
        connected: c.is_active,
        host: c.host,
        port: c.port,
        database: c.database,
        schema: c.schema_name,
        description: c.description,
        use_duckdb_fed: c.use_duckdb_fed,
        created_at: c.created_at,
        updated_at: c.updated_at,
    }
}
