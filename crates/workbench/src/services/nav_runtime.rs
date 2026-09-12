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
use database::model::NavState;
use engine::connection_manager::ConnectionType;

use crate::services::connection_service::{
    resolve_network_method_with_project, ConnectRequest, ConnectionService,
};
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

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
    // 引用的网络配置档案（协议链 / SSH / 代理 / SSL）→ `ConnectionMethod`。
    // 此前恒为 None：对话框里配好的跳板机 / 代理在连接时被静默忽略（审计 #20）。
    let network_method = rt
        .block_on(resolve_network_method_with_project(
            ds.network_config_id.as_deref(),
            project_path,
        ))
        .map_err(|e| e.to_string())?;

    let req = build_connect_request(&ds, project_path, network_method)?;
    let conn_service = ConnectionService::new(engine::get_connection_manager().clone());
    rt.block_on(conn_service.connect_with_type(req))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 组装连接请求（**纯函数，可测**）：作用域路由 + 字段透传；网络方式由 [`connect_entry`] 解析后传入。
pub fn build_connect_request(
    ds: &DataSource,
    project_path: Option<&str>,
    network_method: Option<connection::config::ConnectionMethod>,
) -> Result<ConnectRequest, String> {
    let url = connection::url::build_connection_url(ds)?;
    // 存于项目库（P_/GP_）→ Project；其余（G_ 与遗留 conn-）→ Global。
    // 与 `DataSourceService` 同一判定（`id_prefix::uses_project_storage`），
    // 不再各自实现前缀推导（遗留 ID 曾在此被误归项目）。
    let connection_type = if engine::persistence::id_prefix::uses_project_storage(&ds.id) {
        ConnectionType::Project
    } else {
        ConnectionType::Global
    };

    Ok(ConnectRequest {
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
        network_method,
    })
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

/// 打开导航持久化存储：全局连接（含遗留 `conn-`）→ 全局库；项目 / 共享连接 → 项目库。
fn open_store(conn_id: &str, project_root: Option<&Path>) -> Result<NavStore, String> {
    if engine::persistence::id_prefix::uses_project_storage(conn_id) {
        let root = project_root.ok_or_else(|| "未打开项目，无法读写项目导航状态".to_string())?;
        NavStore::open_project(root)
    } else {
        NavStore::open_global()
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
    let scope = if engine::persistence::id_prefix::uses_project_storage(conn_id) {
        "project"
    } else {
        "global"
    };
    store.save_state(conn_id, scope, state)
}

/// 打开连接组织元数据存储（标签 / 分组的权威源）：
/// 全局连接（含遗留 `conn-`）→ 全局库；项目 / 共享连接 → 项目库。
fn open_org_store(
    conn_id: &str,
    project_root: Option<&Path>,
) -> Result<engine::persistence::ConnectionOrgStore, String> {
    if engine::persistence::id_prefix::uses_project_storage(conn_id) {
        let root = project_root.ok_or_else(|| "未打开项目，无法读写项目连接标签".to_string())?;
        engine::persistence::ConnectionOrgStore::open_project(root).map_err(|e| e.to_string())
    } else {
        engine::persistence::ConnectionOrgStore::open_global().map_err(|e| e.to_string())
    }
}

/// 驱动目录元数据（连接行徽标 / tooltip 用）。
#[derive(Debug, Clone)]
pub struct DriverMeta {
    /// 数据库类型 id（`drivers.type_id`，如 `postgresql`）。
    pub type_id: String,
    /// 驱动显示名（`drivers.name`，如 `PostgreSQL (Official)`）。
    pub name: String,
}

/// 读取全局驱动目录（`driver id → type_id / 显示名`）。
///
/// 供导航连接行解析「类型形状 + 2 字母」与 tooltip 里的驱动显示名。
/// 渲染期不做 I/O，故由面板在 `cx.defer_in` 中一次性加载并缓存到 `DatabaseNavView`。
/// 读取失败返回空表（消费方回退通用形状），不影响导航可用性。
pub fn driver_catalog() -> std::collections::HashMap<String, DriverMeta> {
    let Ok(path) = engine::migration::get_global_db_path() else {
        return std::collections::HashMap::new();
    };
    let load = || -> Result<std::collections::HashMap<String, DriverMeta>, String> {
        let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
        let _ = conn.busy_timeout(std::time::Duration::from_secs(3));
        let drivers = engine::persistence::driver_store::get_all_drivers(&conn)
            .map_err(|e| e.to_string())?;
        Ok(drivers
            .into_iter()
            .map(|d| {
                (
                    d.id,
                    DriverMeta {
                        type_id: d.type_id,
                        name: d.name,
                    },
                )
            })
            .collect())
    };
    load().unwrap_or_default()
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

/// 读取当前项目可见连接的全部标签映射（全局库 + 项目库合并）。
///
/// 一次性读取，供导航面板在渲染分组树前建立「连接 → 标签」映射。
pub fn list_all_tags(
    project_root: Option<&Path>,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut merge = |pairs: Vec<(String, String)>| {
        for (conn_id, tag) in pairs {
            map.entry(conn_id).or_default().push(tag);
        }
    };
    if let Ok(store) = engine::persistence::ConnectionOrgStore::open_global() {
        merge(store.list_tag_pairs());
    }
    if let Some(root) = project_root {
        if let Ok(store) = engine::persistence::ConnectionOrgStore::open_project(root) {
            merge(store.list_tag_pairs());
        }
    }
    map
}

/// 打开项目级连接组织存储（分组仅项目级）。
fn open_org_project(
    project_root: Option<&Path>,
) -> Result<engine::persistence::ConnectionOrgStore, String> {
    let root = project_root.ok_or_else(|| "未打开项目，无法读写分组".to_string())?;
    engine::persistence::ConnectionOrgStore::open_project(root).map_err(|e| e.to_string())
}

/// 列出全部分组（项目级，按 sort_order 排序）。
pub fn list_groups(project_root: Option<&Path>) -> Vec<engine::persistence::ConnectionGroup> {
    open_org_project(project_root)
        .map(|s| s.list_groups())
        .unwrap_or_default()
}

/// 新建分组，返回分组 ID。
pub fn create_group(project_root: Option<&Path>, name: &str) -> Result<String, String> {
    let store = open_org_project(project_root)?;
    let id = engine::persistence::id_prefix::generate_pid("grp");
    store
        .create_group(&id, name, None)
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// 重命名分组。
///
/// `update_group` 要求完整的 `sort_order`（同步改排序），重命名时保留库中现值（缺省 0）。
pub fn rename_group(
    project_root: Option<&Path>,
    group_id: &str,
    name: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let sort_order = store
        .list_groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.sort_order)
        .unwrap_or(0);
    store
        .update_group(group_id, name, None, sort_order)
        .map_err(|e| e.to_string())
}

/// 删除分组（不删连接）。
pub fn delete_group(project_root: Option<&Path>, group_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.delete_group(group_id).map_err(|e| e.to_string())
}

/// 分组成员连接 ID（按组内顺序）。
pub fn list_group_members(project_root: Option<&Path>, group_id: &str) -> Vec<String> {
    open_org_project(project_root)
        .map(|s| s.list_group_members(group_id))
        .unwrap_or_default()
}

/// 加入分组。
pub fn add_to_group(
    project_root: Option<&Path>,
    group_id: &str,
    conn_id: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.add_member(group_id, conn_id).map_err(|e| e.to_string())
}

/// 移出分组。
pub fn remove_from_group(
    project_root: Option<&Path>,
    group_id: &str,
    conn_id: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.remove_member(group_id, conn_id).map_err(|e| e.to_string())
}
