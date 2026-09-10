//! connection_store 公共 API 集成测试
//!
//! 测试 ConnectionInfo 创建、ConnectionStore CRUD、搜索与序列化。
//! 源码文件超 500 行，测试体量超 100 行，按规范外移。

use std::path::PathBuf;

use rdata_station_lib::core::persistence::connection_store::{
    ConnectionInfo, ConnectionStore, MAX_CONNECTIONS,
};

#[test]
fn test_connection_info_new() {
    let conn = ConnectionInfo::new(
        "test-id".to_string(),
        "Test Connection".to_string(),
        "mysql".to_string(),
        "mysql://localhost:3306/test".to_string(),
    );

    assert_eq!(conn.id, "test-id");
    assert_eq!(conn.name, "Test Connection");
    assert_eq!(conn.db_type, "mysql");
    assert_eq!(conn.url, "mysql://localhost:3306/test");
    assert!(conn.created_at > 0);
    assert_eq!(conn.last_used, conn.created_at);
}

#[test]
fn test_connection_store_add_and_get() {
    let mut store = ConnectionStore::new(PathBuf::from("/tmp/test_connections.json"));

    let conn = ConnectionInfo::new(
        "conn-1".to_string(),
        "MySQL Local".to_string(),
        "mysql".to_string(),
        "mysql://localhost:3306/db".to_string(),
    );

    store.add_connection(conn);

    assert_eq!(store.len(), 1);
    assert!(store.get_connection("conn-1").is_some());
    assert!(store.is_modified());
}

#[test]
fn test_connection_store_remove() {
    let mut store = ConnectionStore::new(PathBuf::from("/tmp/test_connections.json"));

    let conn = ConnectionInfo::new(
        "conn-1".to_string(),
        "MySQL Local".to_string(),
        "mysql".to_string(),
        "mysql://localhost:3306/db".to_string(),
    );

    store.add_connection(conn);
    assert!(store.remove_connection("conn-1"));
    assert_eq!(store.len(), 0);
    assert!(!store.remove_connection("non-existent"));
}

#[test]
fn test_connection_store_max_limit() {
    let mut store = ConnectionStore::new(PathBuf::from("/tmp/test_connections.json"));

    // 添加超过最大限制的连接
    for i in 0..MAX_CONNECTIONS + 5 {
        let conn = ConnectionInfo::new(
            format!("conn-{}", i),
            format!("Connection {}", i),
            "mysql".to_string(),
            format!("mysql://localhost:3306/db{}", i),
        );
        store.add_connection(conn);
    }

    assert_eq!(store.len(), MAX_CONNECTIONS);
}

#[test]
fn test_connection_store_search() {
    let mut store = ConnectionStore::new(PathBuf::from("/tmp/test_connections.json"));

    store.add_connection(ConnectionInfo::new(
        "conn-1".to_string(),
        "Production MySQL".to_string(),
        "mysql".to_string(),
        "mysql://prod.example.com:3306/db".to_string(),
    ));

    store.add_connection(ConnectionInfo::new(
        "conn-2".to_string(),
        "Local Postgres".to_string(),
        "postgres".to_string(),
        "postgres://localhost:5432/db".to_string(),
    ));

    let results = store.search("mysql");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "conn-1");

    let results = store.search("local");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "conn-2");
}

#[test]
fn test_json_serialization() {
    let mut store = ConnectionStore::new(PathBuf::from("/tmp/test_connections.json"));

    store.add_connection(ConnectionInfo::new(
        "test-1".to_string(),
        "Test \"Quoted\" Name".to_string(),
        "mysql".to_string(),
        "mysql://localhost:3306/test".to_string(),
    ));

    let json = ConnectionStore::serialize_connections(store.get_connections());
    assert!(json.contains("\"id\": \"test-1\""));
    assert!(json.contains("Test \\\"Quoted\\\" Name")); // 转义的引号
}
