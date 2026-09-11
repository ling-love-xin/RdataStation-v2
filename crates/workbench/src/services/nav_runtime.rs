//! 数据源导航运行时（M4）：连接 / 断开入口。
//!
//! 复用 `ConnectionService` + 全局 `ConnectionManager`；依据设计
//! `docs/architecture/database/database-navigator-prototype-design.md`：
//! - 连接：URL 组装走 `connection::url::build_connection_url`（C3 收敛：百分号编码已统一），
//!   建立运行时连接；
//! - 断开：关闭运行时连接，**保留**元数据缓存（缓存只在显式「缓存管理」中清理）；
//! - 标签：读写连接组织存储 `engine::persistence::ConnectionOrgStore`（连接域数据，非视图状态）。

use std::path::Path;

use connection::model::DataSource;
use database::model::{NavSource, NavState};
use engine::connection_manager::ConnectionType;

use crate::services::connection_service::{ConnectRequest, ConnectionService};
use crate::services::data_source_service::DataSourceService;
use crate::services::nav_store::NavStore;

/// 解析导航树入口的连接记录（项目侧 `P_`/`GP_` 只存项目库，必须带项目根）。
pub fn load_entry(conn_id: &str, project_path: Option<&str>) -> Result<DataSource, String> {
    let service = DataSourceService::global().map_err(|e| e.to_string())?;
    load_entry_with(&service, conn_id, project_path)
}

/// 同 [`load_entry`]，但显式注入服务（测试用，避免依赖全局单例）。
///
/// 与对话框编辑回读同源：走 `get_with_project`，不走只查全局库的 `get`。
pub fn load_entry_with(
    service: &DataSourceService,
    conn_id: &str,
    project_path: Option<&str>,
) -> Result<DataSource, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
    rt.block_on(service.get_with_project(conn_id, project_path))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "数据源不存在".to_string())
}

/// 连接一条已保存数据源（按记录字段组装 URL；`project_path` 项目连接需提供）。
pub fn connect_entry(conn_id: &str, project_path: Option<&str>) -> Result<(), String> {
    let service = DataSourceService::global().map_err(|e| e.to_string())?;

    let ds = load_entry_with(&service, conn_id, project_path)?;

    let url = connection::url::build_connection_url(&ds)?;
    let connection_type = if ds.id.starts_with("G_") {
        ConnectionType::Global
    } else {
        ConnectionType::Project
    };

    let req = ConnectRequest {
        conn_id: Some(ds.id.clone()),
        db_type: ds.db_type.clone(),
        url,
        name: Some(ds.name.clone()),
        connection_type,
        project_path: project_path.map(|s| s.to_string()),
        description: ds.description.clone(),
        driver_id: ds.driver_id.clone(),
        environment_id: ds.environment_id.clone(),
        auth_config_id: ds.auth_config_id.clone(),
        auth_method: ds.auth_method.clone(),
        network_config_id: ds.network_config_id.clone(),
        driver_properties: ds.driver_properties.clone(),
        advanced_options: ds.advanced_options.clone(),
        options: ds.options.clone(),
        tags: ds.tags.clone(),
        metadata_path: ds.metadata_path.clone(),
        schema_name: ds.schema_name.clone(),
        use_duckdb_fed: Some(ds.use_duckdb_fed),
        password: None,
        skip_persistence: Some(true),
        network_method: None,
    };

    let conn_service = ConnectionService::new(engine::get_connection_manager().clone());
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
    rt.block_on(conn_service.connect_with_type(req))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 断开运行时连接（保留元数据缓存）。
pub fn disconnect_entry(conn_id: &str) -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
    let conn_service = ConnectionService::new(engine::get_connection_manager().clone());
    rt.block_on(conn_service.close_connection(conn_id))
        .map_err(|e| e.to_string())
}

/// 运行时是否已连接。
pub fn is_connected(conn_id: &str) -> bool {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return false,
    };
    rt.block_on(engine::get_connection_manager().has_connection(&conn_id.to_string()))
}

/// 打开导航持久化存储：全局连接 → 全局库；项目 / 共享连接 → 项目库。
fn open_store(conn_id: &str, project_root: Option<&Path>) -> Result<NavStore, String> {
    match NavSource::from_conn_id(conn_id) {
        NavSource::Global => NavStore::open_global(),
        _ => {
            let root =
                project_root.ok_or_else(|| "未打开项目，无法读写项目导航状态".to_string())?;
            NavStore::open_project(root)
        }
    }
}

/// 读取连接导航状态（失败返回默认）。
pub fn load_nav_state(conn_id: &str, project_root: Option<&Path>) -> NavState {
    match open_store(conn_id, project_root) {
        Ok(store) => store.load_state(conn_id),
        Err(_) => NavState::default(),
    }
}

/// 保存连接导航状态。
pub fn save_nav_state(
    conn_id: &str,
    project_root: Option<&Path>,
    state: &NavState,
) -> Result<(), String> {
    let store = open_store(conn_id, project_root)?;
    let scope = match NavSource::from_conn_id(conn_id) {
        NavSource::Global => "global",
        _ => "project",
    };
    store.save_state(conn_id, scope, state)
}

/// 打开连接组织元数据存储（标签 / 分组的权威源）：
/// 全局连接 → 全局库；项目 / 共享连接 → 项目库。
fn open_org_store(
    conn_id: &str,
    project_root: Option<&Path>,
) -> Result<engine::persistence::ConnectionOrgStore, String> {
    match NavSource::from_conn_id(conn_id) {
        NavSource::Global => {
            engine::persistence::ConnectionOrgStore::open_global().map_err(|e| e.to_string())
        }
        _ => {
            let root = project_root.ok_or_else(|| "未打开项目，无法读写项目连接标签".to_string())?;
            engine::persistence::ConnectionOrgStore::open_project(root)
                .map_err(|e| e.to_string())
        }
    }
}

/// 读取连接标签（多值）。权威源为连接组织存储（连接域数据），非导航视图状态。
pub fn list_tags(conn_id: &str, project_root: Option<&Path>) -> Vec<String> {
    open_org_store(conn_id, project_root)
        .map(|s| s.list_tags(conn_id))
        .unwrap_or_default()
}

/// 覆盖式设置连接标签。
pub fn set_tags(conn_id: &str, project_root: Option<&Path>, tags: &[String]) -> Result<(), String> {
    let store = open_org_store(conn_id, project_root)?;
    store.set_tags(conn_id, tags).map_err(|e| e.to_string())
}
