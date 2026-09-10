//! 数据源导航运行时（M4）：连接 / 断开入口。
//!
//! 复用 `ConnectionService` + 全局 `ConnectionManager`；依据设计
//! `docs/architecture/database/database-navigator-prototype-design.md`：
//! - 连接：从持久化记录组装 URL，建立运行时连接；
//! - 断开：关闭运行时连接，**保留**元数据缓存（缓存只在显式「缓存管理」中清理）。

use connection::model::DataSource;
use engine::connection_manager::ConnectionType;

use crate::services::connection_service::{ConnectRequest, ConnectionService};
use crate::services::data_source_service::DataSourceService;

/// 连接一条已保存数据源（按记录字段组装 URL；`project_path` 项目连接需提供）。
pub fn connect_entry(conn_id: &str, project_path: Option<&str>) -> Result<(), String> {
    let service = DataSourceService::global().map_err(|e| e.to_string())?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;

    let ds = rt
        .block_on(service.get(conn_id))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "数据源不存在".to_string())?;

    let url = build_entry_url(&ds)?;
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

/// 由记录组装连接 URL：
/// - 网络库：`{driver}://{user:pass@}{host}:{port}/{database}`
/// - 文件库（sqlite/duckdb）：`{driver}://{path}`
///
/// 说明：Phase A 未对密码做百分号编码（与现有 `build_effective_url` 行为一致），
/// 含特殊字符的密码需后续统一处理。
fn build_entry_url(ds: &DataSource) -> Result<String, String> {
    let driver = ds.db_type.as_str();
    if matches!(driver, "sqlite" | "duckdb") {
        let path = ds
            .database
            .clone()
            .or_else(|| ds.host.clone())
            .filter(|p| !p.is_empty())
            .ok_or_else(|| "文件型连接缺少数据库路径".to_string())?;
        return Ok(format!("{driver}://{path}"));
    }

    let password = match ds.password_encrypted.as_deref() {
        Some(enc) if !enc.is_empty() => {
            Some(shared::crypto::decrypt_password(enc).map_err(|e| format!("解密密码失败: {e}"))?)
        }
        _ => None,
    };

    let host = ds.host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = ds.port.map(|p| format!(":{p}")).unwrap_or_default();
    let database = ds.database.clone().unwrap_or_default();
    let cred = match (ds.username.as_deref(), password.as_deref()) {
        (Some(u), Some(p)) if !u.is_empty() => format!("{u}:{p}@"),
        (Some(u), None) if !u.is_empty() => format!("{u}@"),
        _ => String::new(),
    };

    Ok(format!("{driver}://{cred}{host}{port}/{database}"))
}
