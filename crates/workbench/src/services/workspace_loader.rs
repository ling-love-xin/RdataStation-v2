//! 工作台连接数据加载器（M3 数据源连接 → 视图模型）
//!
//! 生产入口与 `DataSourceService` 共用全局系统库单例
//! （`<RDS_HOME>/data/system/global.db`，由 `initialize_global_system` 初始化），
//! 消除"服务写一份、列表读另一份"的路径分裂。
//!
//! 职责：
//! - `load_persisted_connections`：全局连接（单例优先，路径注入降级）；
//! - `load_connections_for_scope`：全局 + 当前项目侧（P_/GP_）合并（项目打开时可见）；
//! - 统一填充运行时连接状态（连接服务/连接管理器维护，非记录字段）；
//! - `delete_connection`：委托 `DataSourceService`（作用域路由 + Secret 清理）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::persistence::global_db::{GlobalConnectionInfo, GlobalDatabaseManager};
use engine::persistence::project_connection_store::{ProjectConnection, ProjectConnectionStore};
use engine::persistence::project_db::ProjectDatabaseManager;

use crate::view::ConnectionItem;

/// 全局系统目录：`<RDS_HOME>/data/system`（global.db 与分析库 analytics.duckdb 均在此）。
pub fn default_global_dir() -> PathBuf {
    engine::migration::get_system_dir().unwrap_or_else(|_| {
        // 系统数据目录不可用时的兜底：退到数据根下的同名结构（改造前退到 %TEMP%，
        // 系统清理会连全局配置与分析库一起删掉）。
        paths::data_dir().join("system")
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
        Ok(infos) => {
            let mut items = map_infos(infos);
            fill_connected(&mut items);
            (items, None)
        }
        Err(e) => (Vec::new(), Some(format!("加载全局连接失败: {e}"))),
    }
}

/// 加载连接列表（全局 + 当前项目侧 P_/GP_ 合并）。
///
/// - `project_root` 提供时，追加项目库连接（P_ 本地 / GP_ 共享快照）；
///   未打开项目时仅全局可见（与作用域语义一致）。
/// - 项目侧与全局同 ID 时以先到者为准（ID 前缀已区分来源，正常不冲突）。
/// - 统一填充运行时连接状态。
pub fn load_connections_for_scope(project_root: Option<&Path>) -> (Vec<ConnectionItem>, Option<String>) {
    let (mut items, mut notice) = load_persisted_connections();

    if let Some(root) = project_root {
        match load_project_connections(root) {
            Ok(project_items) => {
                for item in project_items {
                    if !items.iter().any(|existing| existing.id == item.id) {
                        items.push(item);
                    }
                }
            }
            Err(e) => {
                if notice.is_none() {
                    notice = Some(format!("加载项目连接失败: {e}"));
                }
            }
        }
    }

    fill_connected(&mut items);
    (items, notice)
}

/// 删除连接（委托 M3 服务：作用域路由 G_/P_/GP_ + DuckDB Secret 联动清理）。
///
/// `project_path` 为当前项目根（含 .RSmeta）；项目作用域连接必需，
/// 未打开项目时传 `None`（仅全局连接可删）。
/// 服务未就绪时返回错误，不做"只删 global 表"的静默降级——
/// 否则 P_/GP_ 项目连接会表现为删掉了、实则仍在项目库中。
pub fn delete_connection(conn_id: &str, project_path: Option<&str>) -> Result<(), String> {
    let service = crate::services::data_source_service::DataSourceService::global()
        .map_err(|e| format!("服务未就绪: {e}"))?;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("无法启动异步运行时: {e}"))?;
    runtime
        .block_on(service.delete(conn_id, project_path))
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
        Ok(infos) => {
            let mut items = map_infos(infos);
            fill_connected(&mut items);
            (items, None)
        }
        Err(e) => (Vec::new(), Some(format!("加载全局连接失败: {e}"))),
    }
}

/// 项目根判定：目录存在且含项目元数据目录（`.RSmeta`）。
///
/// 比较时**忽略大小写**：历史代码里同时存在 `.RSmeta`（项目模块，权威）与 `.RSmeta`（早
/// 期 engine/workbench 写法）两种拼写，Windows 下同目录而大小写敏感系统会分叉。
/// 这里宽容读，写路径由 engine 统一用权威拼写创建。
pub(crate) fn is_project_root(root: &std::path::Path) -> bool {
    if !root.is_dir() {
        return false;
    }
    match std::fs::read_dir(root) {
        Ok(entries) => entries.flatten().any(|e| {
            // 注意带前导点：`.RSmeta` / `.rsmeta` 都算命中（容错大小写差异）。
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            name == ".rsmeta" && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
        }),
        Err(_) => false,
    }
}

/// 读取项目库连接（P_ 本地 / GP_ 共享快照）。
///
/// 预检项目根（含 `.RSmeta`）：否则 `ProjectDatabaseManager::open` 会
/// `create_dir_all(.RSmeta)`——一次列表刷新就能在磁盘上造出假项目骨架。
fn load_project_connections(root: &Path) -> Result<Vec<ConnectionItem>, String> {
    if !is_project_root(root) {
        return Err(format!(
            "{} 不是项目根（缺少 .RSmeta）",
            root.to_string_lossy()
        ));
    }
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("无法启动异步运行时: {e}"))?;
    runtime.block_on(async {
        let db = ProjectDatabaseManager::open(root, 4)
            .await
            .map_err(|e| format!("打开项目库失败: {e}"))?;
        let store = ProjectConnectionStore::new(Arc::new(db));
        let conns = store
            .get_all_connections()
            .await
            .map_err(|e| format!("读取项目连接失败: {e}"))?;
        Ok(conns.into_iter().map(project_connection_to_item).collect())
    })
}

/// 批量映射（持久化记录 → 视图模型）。
pub fn map_infos(infos: Vec<GlobalConnectionInfo>) -> Vec<ConnectionItem> {
    infos.into_iter().map(connection_item_from_info).collect()
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`super::*` 会把 gpui 的 `test` 宏带入作用域）。
    use super::is_project_root;

    #[test]
    fn project_root_detection_is_case_insensitive_and_strict() {
        let base = std::env::temp_dir().join(format!("rds_proj_root_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("proj");
        std::fs::create_dir_all(root.join(".RSmeta")).expect("mkdir .RSmeta");
        assert!(is_project_root(&root), "含 .RSmeta 应为项目根");

        // 大小写变体（历史写法）同样识别。
        let upper = base.join("proj-upper");
        std::fs::create_dir_all(upper.join(".rsmeta")).expect("mkdir .rsmeta");
        assert!(is_project_root(&upper), "大小写变体应识别");

        // 普通目录 / 不存在 / 同名文件（非目录）都不算项目根。
        let plain = base.join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir plain");
        assert!(!is_project_root(&plain), "普通目录不是项目根");
        assert!(!is_project_root(&base.join("missing")), "不存在不是项目根");
        let file_like = base.join("file-like");
        std::fs::create_dir_all(&file_like).expect("mkdir file-like");
        std::fs::write(file_like.join(".RSmeta"), b"not a dir").expect("write file");
        assert!(!is_project_root(&file_like), ".RSmeta 为文件不算项目根");

        let _ = std::fs::remove_dir_all(&base);
    }
}

/// 单条映射：`connected` 初始为 false，由 `fill_connected` 填充运行时状态；
/// `is_active` 语义见 `connection_item_from_info`。
pub fn connection_item_from_info(c: GlobalConnectionInfo) -> ConnectionItem {
    ConnectionItem {
        id: c.id,
        name: c.name,
        driver: c.driver,
        connected: false,
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

/// 项目连接记录 → 视图模型（`connected` 由 `fill_connected` 填充）。
fn project_connection_to_item(c: ProjectConnection) -> ConnectionItem {
    ConnectionItem {
        id: c.id,
        name: c.name,
        driver: c.driver,
        connected: false,
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

/// 运行时已连接 ID 集合（一次性查询，避免逐条创建运行时）。
fn connected_ids() -> HashSet<String> {
    let Ok(runtime) = tokio::runtime::Runtime::new() else {
        return HashSet::new();
    };
    runtime
        .block_on(engine::get_connection_manager().get_all_connection_ids())
        .into_iter()
        .collect()
}

/// 用连接管理器的运行态覆盖 `connected` 字段（记录字段 is_active 不再冒充连接状态）。
fn fill_connected(items: &mut [ConnectionItem]) {
    let connected = connected_ids();
    if connected.is_empty() {
        return;
    }
    for item in items.iter_mut() {
        item.connected = connected.contains(&item.id);
    }
}
