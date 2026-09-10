//! Round 21：真实数据接入链路验证
//!
//! 验证 workbench 视图可用的连接数据链路：
//! 1. `GlobalDatabaseManager::new`（含全局迁移）可初始化；
//! 2. `save_global_connection` 持久化连接元数据；
//! 3. `get_global_connections` 读回并映射为视图 `ConnectionItem`。
//!
//! 注意：Windows 上同进程同一 DuckDB 文件仅 1 连接句柄，本测试使用唯一临时目录。

use std::path::PathBuf;

use engine::persistence::global_db::{GlobalConnectionSaveInput, GlobalDatabaseManager};
use rds_workbench::ConnectionItem;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_r21_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// GlobalConnectionInfo → ConnectionItem 的映射（与视图层共享的语义）。
fn to_connection_items(
    infos: Vec<engine::persistence::global_db::GlobalConnectionInfo>,
) -> Vec<ConnectionItem> {
    infos
        .into_iter()
        .map(|c| ConnectionItem {
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
        })
        .collect()
}

#[tokio::test]
async fn global_db_roundtrip_maps_to_connection_items() {
    let dir = temp_dir("roundtrip");
    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    let manager = GlobalDatabaseManager::new(sqlite.clone(), duckdb.clone(), 2)
        .await
        .expect("init global db with migrations");

    manager
        .save_global_connection(GlobalConnectionSaveInput {
            conn_id: "conn-001",
            name: "测试 MySQL 库",
            db_type: "mysql",
            url: "mysql://127.0.0.1:3306/test",
            username: Some("root"),
            password: Some("secret"),
            tags: None,
            server_version: None,
            description: Some("round 21 集成测试"),
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            options: None,
            driver_properties: None,
            advanced_options: None,
            use_duckdb_fed: Some(true),
            metadata_path: None,
            schema_name: None,
        })
        .await
        .expect("save global connection");

    let infos = manager
        .get_global_connections(None, None)
        .await
        .expect("list global connections");

    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].name, "测试 MySQL 库");
    assert_eq!(infos[0].driver, "mysql");
    assert_eq!(infos[0].use_duckdb_fed, true);

    let items = to_connection_items(infos);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "conn-001");
    assert_eq!(items[0].name, "测试 MySQL 库");
    assert_eq!(items[0].driver, "mysql");
    // save 后连接默认激活（is_active=1），映射如实呈现。
    assert_eq!(items[0].connected, true);
    // Round 22：真实元数据字段完整映射。
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(3306));
    assert_eq!(items[0].database.as_deref(), Some("test"));
    assert_eq!(items[0].use_duckdb_fed, true);
    assert_eq!(items[0].description.as_deref(), Some("round 21 集成测试"));

    drop(manager);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn empty_db_returns_empty_list() {
    let dir = temp_dir("empty");
    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    let manager = GlobalDatabaseManager::new(sqlite.clone(), duckdb.clone(), 2)
        .await
        .expect("init global db");

    let infos = manager.get_global_connections(None, None).await.expect("list");
    assert!(infos.is_empty());

    drop(manager);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn second_connection_persists_after_reopen() {
    let dir = temp_dir("reopen");
    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    {
        let manager = GlobalDatabaseManager::new(sqlite.clone(), duckdb.clone(), 2)
            .await
            .expect("init global db");
        manager
            .save_global_connection(GlobalConnectionSaveInput {
                conn_id: "conn-002",
                name: "分析数仓 DuckDB",
                db_type: "duckdb",
                url: "duckdb:///data/analytics.duckdb",
                username: None,
                password: None,
                tags: None,
                server_version: None,
                description: None,
                driver_id: None,
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                options: None,
                driver_properties: None,
                advanced_options: None,
                use_duckdb_fed: Some(true),
                metadata_path: None,
                schema_name: None,
            })
            .await
            .expect("save");
        drop(manager);
    }

    // 重新打开（模拟下次启动），连接元数据仍在。
    {
        let manager = GlobalDatabaseManager::new(sqlite.clone(), duckdb.clone(), 2)
            .await
            .expect("reopen global db");
        let infos = manager.get_global_connections(None, None).await.expect("list");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "分析数仓 DuckDB");
        drop(manager);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// save/load 均为同步入口（内部自建 runtime），普通 #[test] 避免嵌套 runtime。
#[test]
fn save_connection_at_then_load_roundtrip() {
    let dir = temp_dir("save_at");
    // 写路径（同步入口，测试线程无 tokio 上下文亦可直接调用）。
    rds_workbench::services::workspace_loader::save_connection_at(
        &dir,
        "表单新建库",
        "postgres",
        "postgres://127.0.0.1:5432/analytics",
        "admin",
        "secret",
    )
    .expect("save ok");

    let (items, notice) =
        rds_workbench::services::workspace_loader::load_persisted_connections_from(&dir);
    assert!(notice.is_none());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "表单新建库");
    assert_eq!(items[0].driver, "postgres");
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(5432));
    assert_eq!(items[0].database.as_deref(), Some("analytics"));
    assert_eq!(items[0].use_duckdb_fed, true);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_connection_at_removes_from_list() {
    let dir = temp_dir("delete_at");
    rds_workbench::services::workspace_loader::save_connection_at(
        &dir,
        "待删除库",
        "mysql",
        "mysql://10.0.0.8:3306/orders",
        "",
        "",
    )
    .expect("save ok");

    let (items, _) = rds_workbench::services::workspace_loader::load_persisted_connections_from(&dir);
    assert_eq!(items.len(), 1);
    let conn_id = items[0].id.clone();

    // 删除后列表为空（物理删除，is_active=1 过滤下自然消失）。
    rds_workbench::services::workspace_loader::delete_connection_at(&dir, &conn_id)
        .expect("delete ok");
    let (items2, _) = rds_workbench::services::workspace_loader::load_persisted_connections_from(&dir);
    assert!(items2.is_empty(), "删除后连接应消失");

    let _ = std::fs::remove_dir_all(&dir);
}
