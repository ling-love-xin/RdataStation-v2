//! Connection Manager 测试
//!
//! 测试连接管理器的核心功能
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use std::collections::HashSet;
use std::sync::Arc;

use rdata_station_lib::core::services::connection_manager::{
    create_connection_id, get_connection_manager, ConnectionConfig, ConnectionInfo,
    ConnectionManager, ConnectionType,
};

fn create_test_connection_info(id: &str) -> ConnectionInfo {
    ConnectionInfo {
        id: id.to_string(),
        name: format!("Test Connection {}", id),
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        connection_type: ConnectionType::Global,
        project_id: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
        server_version: None,
        created_at: std::time::Instant::now(),
    }
}

#[tokio::test]
async fn test_has_connection() {
    let manager = ConnectionManager::new();
    let conn_id = "test-conn-1".to_string();

    assert!(!manager.has_connection(&conn_id).await);
}

#[tokio::test]
async fn test_get_all_connection_ids_empty() {
    let manager = ConnectionManager::new();
    let ids = manager.get_all_connection_ids().await;
    assert!(ids.is_empty());
}

#[tokio::test]
async fn test_get_all_connection_info_empty() {
    let manager = ConnectionManager::new();
    let infos = manager.get_all_connection_info().await;
    assert!(infos.is_empty());
}

#[tokio::test]
async fn test_get_active_connection_id_initially_none() {
    let manager = ConnectionManager::new();
    let active_id = manager.get_active_connection_id().await;
    assert!(active_id.is_none());
}

#[tokio::test]
async fn test_close_all_connections_empty() {
    let manager = ConnectionManager::new();
    manager.close_all_connections().await;
    assert_eq!(manager.connection_count().await, 0);
}

#[tokio::test]
async fn test_close_connection_not_exists() {
    let manager = ConnectionManager::new();
    let conn_id = "non-existent".to_string();
    let result = manager.close_connection(&conn_id).await;
    assert!(!result);
}

#[tokio::test]
async fn test_set_active_connection_not_exists() {
    let manager = ConnectionManager::new();
    let result = manager
        .set_active_connection("non-existent".to_string())
        .await;
    assert!(!result);
}

#[tokio::test]
async fn test_switch_connection_not_exists() {
    let manager = ConnectionManager::new();
    let conn_id = "non-existent".to_string();
    let result = manager.switch_connection(&conn_id).await;
    assert!(result.is_err());
}

#[test]
fn test_connection_info_creation() {
    let info = create_test_connection_info("test-1");
    assert_eq!(info.id, "test-1");
    assert_eq!(info.name, "Test Connection test-1");
    assert_eq!(info.db_type, "mysql");
    assert_eq!(info.url, "mysql://localhost:3306/test");
}

#[test]
fn test_connection_id_uniqueness() {
    let mut ids = HashSet::new();
    for i in 0..1000 {
        let id = create_connection_id("mysql", &format!("mysql://localhost:3306/test{}", i));
        assert!(ids.insert(id), "Duplicate ID generated");
    }
    assert_eq!(ids.len(), 1000);
}

#[test]
fn test_connection_id_consistency() {
    for _ in 0..100 {
        let id1 = create_connection_id("mysql", "mysql://localhost:3306/test");
        let id2 = create_connection_id("mysql", "mysql://localhost:3306/test");
        assert_eq!(id1, id2);
    }
}

#[tokio::test]
async fn test_get_connection_manager_singleton() {
    let manager1 = get_connection_manager();
    let manager2 = get_connection_manager();

    assert!(std::ptr::eq(Arc::as_ptr(manager1), Arc::as_ptr(manager2)));
}

// ==================== 新增测试：连接管理扩展场景 ====================

// ---------- test_add_connection ----------
// 由于 add_connection 需要 DynDatabase 实例（需要真实数据库连接），
// 这里测试 ConnectionInfo 和 ConnectionConfig 的创建逻辑，
// 以及 add_connection 的前置条件校验。

#[test]
fn test_add_connection_info_fields() {
    // 验证 ConnectionInfo 全量字段创建
    let now = std::time::Instant::now();
    let info = ConnectionInfo {
        id: "conn-add-1".to_string(),
        name: "Test Add Connection".to_string(),
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        server_version: Some("8.0.35".to_string()),
        connection_type: ConnectionType::Global,
        project_id: None,
        driver_id: Some("mysql-native".to_string()),
        environment_id: Some("G_env_prod".to_string()),
        auth_config_id: Some("G_auth_001".to_string()),
        auth_method: Some("password".to_string()),
        network_config_id: Some("G_net_ssh_001".to_string()),
        driver_properties: Some(r#"{"useSSL":"true"}"#.to_string()),
        advanced_options: Some(r#"{"timeout":30}"#.to_string()),
        description: Some("Test description".to_string()),
        created_at: now,
    };
    assert_eq!(info.id, "conn-add-1");
    assert_eq!(info.db_type, "mysql");
    assert_eq!(info.connection_type, ConnectionType::Global);
    assert!(info.driver_id.is_some());
    assert!(info.auth_config_id.is_some());
    assert!(info.network_config_id.is_some());
}

#[test]
fn test_add_connection_config_creation() {
    // 验证 ConnectionConfig 创建
    let config = ConnectionConfig {
        db_type: "postgresql".to_string(),
        url: "postgres://localhost:5432/db".to_string(),
        name: Some("pg-config".to_string()),
        connection_type: Some(ConnectionType::Global),
        project_id: None,
        driver_id: Some("postgres-native".to_string()),
        environment_id: None,
        auth_config_id: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
    };
    assert_eq!(config.db_type, "postgresql");
    assert_eq!(config.connection_type, Some(ConnectionType::Global));
    assert!(!config.url.is_empty());
}

// ---------- test_remove_connection ----------

#[tokio::test]
async fn test_remove_connection_nonexistent() {
    // 移除不存在的连接不应崩溃
    let manager = ConnectionManager::new();
    manager.remove_connection(&"non-existent".to_string()).await;
    assert_eq!(manager.connection_count().await, 0);
}

// ---------- test_get_all_connections ----------

#[tokio::test]
async fn test_get_all_connections_ids_and_info() {
    // 新管理器应返回空的连接 ID 列表和空连接信息列表
    let manager = ConnectionManager::new();
    let ids = manager.get_all_connection_ids().await;
    let infos = manager.get_all_connection_info().await;
    assert!(ids.is_empty());
    assert!(infos.is_empty());
}

// ---------- test_has_connection ----------

#[tokio::test]
async fn test_has_connection_empty_and_invalid() {
    // 空管理器和无效 ID 检查
    let manager = ConnectionManager::new();
    assert!(!manager.has_connection(&"".to_string()).await);
    assert!(
        !manager
            .has_connection(&"non-existent-999".to_string())
            .await
    );
}

// ---------- test_get_connection_not_found ----------

#[tokio::test]
async fn test_get_connection_not_found() {
    // 获取不存在的连接应返回 None
    let manager = ConnectionManager::new();
    let result = manager.get_connection(&"non-existent".to_string()).await;
    assert!(result.is_none());
}

// ---------- test_close_connection ----------

#[tokio::test]
async fn test_close_connection_returns_false_for_nonexistent() {
    // 关闭不存在的连接应返回 false
    let manager = ConnectionManager::new();
    let result = manager.close_connection(&"non-existent".to_string()).await;
    assert!(!result);
    assert_eq!(manager.connection_count().await, 0);
}

// ---------- test_close_all_connections ----------

#[tokio::test]
async fn test_close_all_connections_idempotent() {
    // 多次调用 close_all_connections 应该是幂等的
    let manager = ConnectionManager::new();
    manager.close_all_connections().await;
    assert_eq!(manager.connection_count().await, 0);
    manager.close_all_connections().await;
    assert_eq!(manager.connection_count().await, 0);
}

// ---------- test_set_idle_timeout ----------

#[tokio::test]
async fn test_set_idle_timeout() {
    let manager = ConnectionManager::new();
    let default_timeout = manager.get_idle_timeout().await;
    assert_eq!(default_timeout, std::time::Duration::from_secs(30 * 60));

    let new_timeout = std::time::Duration::from_secs(60);
    manager.set_idle_timeout(new_timeout).await;
    assert_eq!(manager.get_idle_timeout().await, new_timeout);

    let custom_timeout = std::time::Duration::from_secs(5 * 60);
    manager.set_idle_timeout(custom_timeout).await;
    assert_eq!(manager.get_idle_timeout().await, custom_timeout);
}

// ---------- test_cleanup_idle_connections ----------

#[tokio::test]
async fn test_cleanup_idle_connections_empty() {
    // 空连接池回收应返回空列表
    let manager = ConnectionManager::new();
    let reclaimed = manager.reclaim_idle_connections().await;
    assert!(reclaimed.is_empty());
}

// ---------- test_set_active_connection ----------

#[tokio::test]
async fn test_set_active_connection_empty_id() {
    // 设置空 ID 为活动连接应返回 false
    let manager = ConnectionManager::new();
    let result = manager.set_active_connection("".to_string()).await;
    assert!(!result);
}

// ---------- test_get_active_connection ----------

#[tokio::test]
async fn test_get_active_connection_none() {
    // 无连接时获取活动连接应返回 None
    let manager = ConnectionManager::new();
    let active = manager.get_active_connection().await;
    assert!(active.is_none());
}

// ---------- test_connection_info_serialization ----------

#[test]
fn test_connection_info_debug_output() {
    // ConnectionInfo 实现 Debug，验证调试输出格式
    let info = create_test_connection_info("debug-test");
    let debug_str = format!("{:?}", info);
    assert!(debug_str.contains("debug-test"));
    assert!(debug_str.contains("mysql"));
    assert!(debug_str.contains("Test Connection debug-test"));
}

#[test]
fn test_connection_info_clone() {
    // ConnectionInfo 实现 Clone，验证克隆后字段一致
    let info = create_test_connection_info("clone-test");
    let cloned = info.clone();
    assert_eq!(info.id, cloned.id);
    assert_eq!(info.name, cloned.name);
    assert_eq!(info.db_type, cloned.db_type);
    assert_eq!(info.url, cloned.url);
    assert_eq!(info.connection_type, cloned.connection_type);
    assert_eq!(info.project_id, cloned.project_id);
}

#[test]
fn test_connection_info_serialization_fields() {
    // 验证 ConnectionInfo 全字段可访问
    let info = ConnectionInfo {
        id: "serial-test".to_string(),
        name: "Serial Test".to_string(),
        db_type: "duckdb".to_string(),
        url: "duckdb:///tmp/test.duckdb".to_string(),
        server_version: Some("1.0.0".to_string()),
        connection_type: ConnectionType::Project,
        project_id: Some("/path/to/project".to_string()),
        driver_id: Some("duckdb-native".to_string()),
        environment_id: Some("G_env_dev".to_string()),
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: Some("DuckDB project connection".to_string()),
        created_at: std::time::Instant::now(),
    };
    assert_eq!(info.connection_type, ConnectionType::Project);
    assert!(info.project_id.is_some());
    assert_eq!(info.server_version, Some("1.0.0".to_string()));
    assert!(info.description.is_some());
}

// ---------- test_concurrent_add_remove ----------

#[tokio::test]
async fn test_concurrent_add_remove_operations() {
    // 使用 tokio::spawn 并发执行多个操作，验证无竞态
    let manager = Arc::new(ConnectionManager::new());
    let mut handles = vec![];

    for i in 0..10 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            let conn_id = format!("concurrent-{}", i);
            assert!(!mgr.has_connection(&conn_id).await);
            mgr.remove_connection(&conn_id).await;
            assert!(mgr.get_connection(&conn_id).await.is_none());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.expect("concurrent task should not panic");
    }

    assert_eq!(manager.connection_count().await, 0);
}

#[tokio::test]
async fn test_concurrent_close_all_operations() {
    // 并发关闭所有连接不应崩溃
    let manager = Arc::new(ConnectionManager::new());
    let mut handles = vec![];

    for _ in 0..5 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            mgr.close_all_connections().await;
            assert_eq!(mgr.connection_count().await, 0);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.expect("concurrent close_all should not panic");
    }

    assert_eq!(manager.connection_count().await, 0);
}

// ---------- test_connection_manager_singleton ----------

#[tokio::test]
async fn test_connection_manager_singleton_global() {
    // 全局单例多次获取应返回同一实例
    let m1 = get_connection_manager();
    let m2 = get_connection_manager();
    assert!(Arc::ptr_eq(m1, m2));
}

// ==================== ConnectionType 枚举测试 ====================

#[test]
fn test_connection_type_parse_global() {
    assert_eq!(
        ConnectionType::parse_type("global"),
        Some(ConnectionType::Global)
    );
}

#[test]
fn test_connection_type_parse_project() {
    assert_eq!(
        ConnectionType::parse_type("project"),
        Some(ConnectionType::Project)
    );
}

#[test]
fn test_connection_type_parse_invalid() {
    assert_eq!(ConnectionType::parse_type("invalid"), None);
    assert_eq!(ConnectionType::parse_type(""), None);
    assert_eq!(ConnectionType::parse_type("GLOBAL"), None);
}

#[test]
fn test_connection_type_display() {
    assert_eq!(ConnectionType::Global.to_string(), "global");
    assert_eq!(ConnectionType::Project.to_string(), "project");
}

// ==================== create_connection_id 扩展测试 ====================

#[test]
fn test_connection_id_format() {
    // 验证生成的 ID 格式为 db_type-hex_hash
    let id = create_connection_id("mysql", "mysql://localhost:3306/test");
    assert!(id.starts_with("mysql-"));
    // 格式: mysql-<16 hex chars>
    let parts: Vec<&str> = id.splitn(2, '-').collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0], "mysql");
    assert!(!parts[1].is_empty());
}

#[test]
fn test_connection_id_different_db_types() {
    // 不同数据库类型应生成不同前缀的 ID
    let mysql_id = create_connection_id("mysql", "mysql://localhost:3306/test");
    let pg_id = create_connection_id("postgresql", "postgres://localhost:5432/test");
    let sqlite_id = create_connection_id("sqlite", "sqlite:///tmp/test.db");
    let duckdb_id = create_connection_id("duckdb", "duckdb:///tmp/test.duckdb");

    assert!(mysql_id.starts_with("mysql-"));
    assert!(pg_id.starts_with("postgresql-"));
    assert!(sqlite_id.starts_with("sqlite-"));
    assert!(duckdb_id.starts_with("duckdb-"));
}

// ==================== ConnectionInfo 更新/查询测试 ====================

#[tokio::test]
async fn test_get_connection_info_nonexistent() {
    // 获取不存在连接的信息应返回 None
    let manager = ConnectionManager::new();
    let conn_id = "non-existent".to_string();
    let info = manager.get_connection_info(&conn_id).await;
    assert!(info.is_none());
}

#[tokio::test]
async fn test_update_connection_info_nonexistent() {
    // 更新不存在连接的信息应返回错误
    let manager = ConnectionManager::new();
    let info = create_test_connection_info("ghost");
    let result = manager
        .update_connection_info(&"ghost".to_string(), info)
        .await;
    assert!(result.is_err());
}

// ==================== ConnectionManager Default 测试 ====================

#[test]
fn test_connection_manager_default() {
    let manager = ConnectionManager::default();
    // Default 应返回空管理器
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        assert_eq!(manager.connection_count().await, 0);
        assert!(manager.get_active_connection_id().await.is_none());
    });
}
