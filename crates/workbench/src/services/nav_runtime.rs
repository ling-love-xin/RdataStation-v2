//! 数据源导航运行时（M4）：连接 / 断开入口。
//!
//! 复用 `ConnectionService` + 全局 `ConnectionManager`；依据设计
//! `docs/architecture/database/database-navigator-prototype-design.md`：
//! - 连接：URL 组装走 `connection::url::build_connection_url`（C3 收敛：百分号编码已统一），
//!   建立运行时连接；
//! - 断开：关闭运行时连接，**保留**元数据缓存（缓存只在显式「缓存管理」中清理）。
//!
//! 这里只留**需要宿主服务**的部分（连接生命周期必须走 `ConnectionService` /
//! `DataSourceService`）。连接的组织元数据（标签 / 分组 / 排序）与导航视图状态的读写已移至
//! [`database::nav_store`]——它们只依赖 `engine`，不需要宿主参与。

use std::sync::OnceLock;

use connection::model::DataSource;
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
