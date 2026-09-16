//! 数据源导航运行时（M4）：连接 / 断开入口。
//!
//! 复用 `ConnectionService` + 全局 `ConnectionManager`；依据设计
//! `docs/architecture/database/database-navigator-prototype-design.md`：
//! - 连接：URL 组装走 `connection::url::build_connection_url`（C3 收敛：百分号编码已统一），
//!   建立运行时连接；
//! - 断开：关闭运行时连接，**保留**元数据缓存（缓存只在显式「缓存管理」中清理）；
//! - 标签：读写连接组织存储 `engine::persistence::ConnectionOrgStore`（连接域数据，非视图状态）。

use std::path::Path;
use std::sync::OnceLock;

use connection::model::DataSource;
use database::model::NavState;
use engine::connection_manager::ConnectionType;

use crate::services::connection_service::{
    resolve_network_method_with_project, ConnectRequest, ConnectionService,
};
use crate::services::data_source_service::DataSourceService;

/// 进程级桥接运行时：`nav_runtime` 的同步入口用它把异步调用落地。
///
/// **必须进程级共享**：sqlx / 原生驱动的连接池建立在首次 `connect` 的那个运行时上，
/// 池的后台任务（连接 I/O、`min_connections` 维持、生命周期回收）随该运行时存活。
/// 若每次调用都 `Runtime::new()` 再丢弃，池会随运行时空转而**永久不可用**——
/// 表现为「点连接看似成功，但对象树 / 预热全部挂起（无法连接）」。
static BRIDGE_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// 取（或首次创建）进程级桥接运行时。
///
/// 公开给同 crate 的其他装配层（`services::mock_generator` 的内省 / 生成同样需要
/// 一个稳定运行时；各自 `Runtime::new()` 会让连接池的后台任务失去存活宿主）。
pub(crate) fn bridge_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    if let Some(rt) = BRIDGE_RUNTIME.get() {
        return Ok(rt);
    }
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
    let _ = BRIDGE_RUNTIME.set(rt);
    BRIDGE_RUNTIME
        .get()
        .ok_or_else(|| "运行时初始化失败".to_string())
}

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
    let rt = bridge_runtime()?;
    rt.block_on(service.get_with_project(conn_id, project_path))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "数据源不存在".to_string())
}

/// 连接一条已保存数据源（按记录字段组装 URL；`project_path` 项目连接需提供）。
pub fn connect_entry(conn_id: &str, project_path: Option<&str>) -> Result<(), String> {
    let service = DataSourceService::global().map_err(|e| e.to_string())?;

    let ds = load_entry_with(&service, conn_id, project_path)?;

    let rt = bridge_runtime()?;
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
    let rt = bridge_runtime()?;
    let conn_service = ConnectionService::new(engine::get_connection_manager().clone());
    rt.block_on(conn_service.close_connection(conn_id))
        .map_err(|e| e.to_string())
}

/// 测试一条已保存数据源（导航右键「测试连接」）。
///
/// 独立会话：建连探测后立即释放隧道，**不注册连接池、不写库**（与对话框「测试连接」同源）。
/// 成功返回可直接展示的文案；失败以 `Err` 给出原因。
///
/// 注：走进程级 `BRIDGE_RUNTIME`（与 `connect_entry` 同一运行时），即便从导航工作线程
/// 调用也能保证隧道后台任务落在稳定运行时上。
pub fn test_entry(conn_id: &str, project_path: Option<&str>) -> Result<String, String> {
    let service = DataSourceService::global().map_err(|e| e.to_string())?;
    let rt = bridge_runtime()?;
    let result = rt
        .block_on(service.test_saved(conn_id, project_path))
        .map_err(|e| e.to_string())?;
    if !result.success {
        return Err(format!("连接失败：{}", result.message));
    }
    // 成功：沿用服务层文案（含耗时与隧道 / TLS 备注），有版本时补在最前面。
    Ok(match result.version.as_deref() {
        Some(v) if !v.is_empty() => format!("{}（版本 {}）", result.message, v),
        _ => result.message,
    })
}

/// 运行时是否已连接。
pub fn is_connected(conn_id: &str) -> bool {
    let Ok(rt) = bridge_runtime() else {
        return false;
    };
    rt.block_on(engine::get_connection_manager().has_connection(&conn_id.to_string()))
}

/// 打开导航持久化存储：全局连接（含遗留 `conn-`）→ 全局库；项目 / 共享连接 → 项目库。
fn open_store(
    conn_id: &str,
    project_root: Option<&Path>,
) -> Result<engine::persistence::NavigatorStateStore, String> {
    if engine::persistence::id_prefix::uses_project_storage(conn_id) {
        let root = project_root.ok_or_else(|| "未打开项目，无法读写项目导航状态".to_string())?;
        engine::persistence::NavigatorStateStore::open_project(root).map_err(|e| e.to_string())
    } else {
        engine::persistence::NavigatorStateStore::open_global().map_err(|e| e.to_string())
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
    store
        .save_state(conn_id, scope, state)
        .map_err(|e| e.to_string())
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

/// 新建分组（名称 + 描述），返回分组 ID。
pub fn create_group_with(
    project_root: Option<&Path>,
    name: &str,
    description: Option<&str>,
) -> Result<String, String> {
    let store = open_org_project(project_root)?;
    let id = engine::persistence::id_prefix::generate_pid("grp");
    store
        .create_group(&id, name, description)
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// 更新分组（名称 + 描述），**排序保留库中现值**。
///
/// `update_group` 要求完整字段（名称 + 描述 + 排序），故先读回现值再写。
pub fn update_group(
    project_root: Option<&Path>,
    group_id: &str,
    name: &str,
    description: Option<&str>,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let sort_order = store
        .list_groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.sort_order)
        .unwrap_or(0);
    store
        .update_group(group_id, name, description, sort_order)
        .map_err(|e| e.to_string())
}

/// 重命名分组（保留描述与排序）。
///
/// 不再传 `None` 描述：那会把已有描述洗掉（曾经的缺陷），改名时描述必须保留。
pub fn rename_group(
    project_root: Option<&Path>,
    group_id: &str,
    name: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let current = store
        .list_groups()
        .into_iter()
        .find(|g| g.id == group_id);
    let (description, sort_order) = current
        .map(|g| (g.description, g.sort_order))
        .unwrap_or((None, 0));
    store
        .update_group(group_id, name, description.as_deref(), sort_order)
        .map_err(|e| e.to_string())
}

/// 移出全部分组（连接回到「未分组」）。
///
/// 与分组内联编辑器的「全部取消勾选」同效果：先清关系，再写未分组顺序
/// （`set_container_order` 会把该连接追加到未分组末尾）。
pub fn remove_from_all_groups(project_root: Option<&Path>, conn_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store
        .set_connection_groups(conn_id, &[])
        .map_err(|e| e.to_string())
}

/// 「未分组」容器的显式顺序（连接 ID，未手动排序的不出现）。
pub fn list_ungrouped_order(project_root: Option<&Path>) -> Vec<String> {
    open_org_project(project_root)
        .map(|s| s.list_ungrouped_order())
        .unwrap_or_default()
}

/// 重写容器内成员顺序（`scope_id` = 分组 ID 或 [`engine::persistence::UNGROUPED_SCOPE`]）。
///
/// 一次性 `0..n` 重写（不是相对插入），保证序号完整、与屏上顺序一致。
/// 未分组容器走单独的表（它不是真实分组，成员由推导得出）。
pub fn set_container_order(
    project_root: Option<&Path>,
    scope_id: &str,
    conn_ids: &[String],
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let result = if scope_id == engine::persistence::UNGROUPED_SCOPE {
        store.set_ungrouped_order(conn_ids)
    } else {
        store.set_member_order_all(scope_id, conn_ids)
    };
    result.map_err(|e| e.to_string())
}

/// 重写分组之间的顺序（序号 = 下标）。
pub fn set_group_order(project_root: Option<&Path>, group_ids: &[String]) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.set_group_order(group_ids).map_err(|e| e.to_string())
}

/// 删除分组（不删连接）。
pub fn delete_group(project_root: Option<&Path>, group_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.delete_group(group_id).map_err(|e| e.to_string())
}

/// 分组成员连接 ID（按组内顺序）。
///
/// 未手动排序的成员排在最后，且**未按名称排**——名称不在组织存储里，
/// 要靠视图侧 [`list_group_members_detailed`] + `nav_order_members` 补齐。
pub fn list_group_members(project_root: Option<&Path>, group_id: &str) -> Vec<String> {
    open_org_project(project_root)
        .map(|s| s.list_group_members(group_id))
        .unwrap_or_default()
}

/// 分组成员 + 是否手动排序过（`None` = 未排；`Some(idx)` = 手动序号）。
pub fn list_group_members_detailed(
    project_root: Option<&Path>,
    group_id: &str,
) -> Vec<(String, Option<i64>)> {
    open_org_project(project_root)
        .map(|s| s.list_group_members_detailed(group_id))
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

/// 显式主组映射（连接 ID → 主组 ID；仅显式指定过的连接）。
pub fn list_primary_groups(
    project_root: Option<&Path>,
) -> std::collections::HashMap<String, String> {
    open_org_project(project_root)
        .map(|s| s.list_primary_group_pairs().into_iter().collect())
        .unwrap_or_default()
}

/// 设置连接的主组（同一连接同时只能有一个）。
pub fn set_primary_group(
    project_root: Option<&Path>,
    conn_id: &str,
    group_id: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store
        .set_primary_group(conn_id, group_id)
        .map_err(|e| e.to_string())
}

/// 清除主组标记（回退到按分组排序推导）。
pub fn clear_primary_group(project_root: Option<&Path>, conn_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store
        .clear_primary_group(conn_id)
        .map_err(|e| e.to_string())
}
