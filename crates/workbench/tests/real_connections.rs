//! Round 21：真实数据接入链路验证
//!
//! 验证 workbench 视图可用的连接数据链路：
//! 1. `GlobalDatabaseManager::new`（含全局迁移）可初始化；
//! 2. `save_global_connection` 持久化连接元数据；
//! 3. loader 从指定目录读回并映射为视图 `ConnectionItem`。
//!
//! 注意：Windows 上同进程同一 DuckDB 文件仅 1 连接句柄，本测试使用唯一临时目录；
//! loader 是同步入口（内部自建 runtime），故用普通 `#[test]` 并显式释放 runtime，
//! 避免在 tokio 上下文中嵌套阻塞。

use std::path::PathBuf;

use engine::persistence::global_db::{GlobalConnectionSaveInput, GlobalDatabaseManager};
use rds_workbench::services::workspace_loader::{load_persisted_connections_from, map_infos};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_r21_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// 测试用保存输入（`db_type`/`url` 可按需覆盖）。
fn save_input<'a>(
    conn_id: &'a str,
    name: &'a str,
    db_type: &'a str,
    url: &'a str,
) -> GlobalConnectionSaveInput<'a> {
    GlobalConnectionSaveInput {
        conn_id,
        name,
        db_type,
        url,
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
    }
}

#[test]
fn global_db_roundtrip_maps_to_connection_items() {
    let dir = temp_dir("roundtrip");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let infos = runtime.block_on(async {
        let manager =
            GlobalDatabaseManager::new(dir.join("global.db"), dir.join("analytics.duckdb"), 2)
                .await
                .expect("init global db with migrations");
        manager
            .save_global_connection(save_input(
                "G_conn_demo",
                "测试 MySQL 库",
                "mysql",
                "mysql://root:secret@127.0.0.1:3306/test",
            ))
            .await
            .expect("save global connection");
        let infos = manager
            .get_global_connections(None, None)
            .await
            .expect("list global connections");
        drop(manager);
        infos
    });
    drop(runtime);

    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].name, "测试 MySQL 库");
    assert_eq!(infos[0].driver, "mysql");
    assert!(infos[0].use_duckdb_fed);

    let items = map_infos(infos);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "G_conn_demo");
    assert_eq!(items[0].name, "测试 MySQL 库");
    assert_eq!(items[0].driver, "mysql");
    // save 后连接记录默认有效（is_active=1），映射如实呈现。
    assert!(items[0].connected);
    // Round 22：真实元数据字段完整映射。
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(3306));
    assert_eq!(items[0].database.as_deref(), Some("test"));
    assert!(items[0].use_duckdb_fed);
    assert_eq!(items[0].description.as_deref(), Some("round 21 集成测试"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn empty_db_returns_empty_list() {
    let dir = temp_dir("empty");

    let (items, notice) = load_persisted_connections_from(&dir);
    assert!(notice.is_none());
    assert!(items.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn second_connection_persists_after_reopen() {
    let dir = temp_dir("reopen");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let manager =
            GlobalDatabaseManager::new(dir.join("global.db"), dir.join("analytics.duckdb"), 2)
                .await
                .expect("init global db");
        manager
            .save_global_connection(GlobalConnectionSaveInput {
                username: None,
                password: None,
                description: None,
                ..save_input(
                    "G_conn_analysis",
                    "分析数仓 DuckDB",
                    "duckdb",
                    "duckdb:///data/analytics.duckdb",
                )
            })
            .await
            .expect("save");
        drop(manager);
    });
    drop(runtime);

    // 重新打开（模拟下次启动），连接元数据仍在。
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let count = runtime.block_on(async {
        let manager =
            GlobalDatabaseManager::new(dir.join("global.db"), dir.join("analytics.duckdb"), 2)
                .await
                .expect("reopen global db");
        let infos = manager
            .get_global_connections(None, None)
            .await
            .expect("list");
        drop(manager);
        infos
    });
    drop(runtime);

    assert_eq!(count.len(), 1);
    assert_eq!(count[0].name, "分析数仓 DuckDB");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loader_reads_manager_written_connection() {
    let dir = temp_dir("loader");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let manager =
            GlobalDatabaseManager::new(dir.join("global.db"), dir.join("analytics.duckdb"), 2)
                .await
                .expect("init global db");
        manager
            .save_global_connection(save_input(
                "G_conn_loader",
                "加载器读回库",
                "mysql",
                "mysql://root:secret@127.0.0.1:3306/test",
            ))
            .await
            .expect("save");
        drop(manager);
    });
    drop(runtime);

    let (items, notice) = load_persisted_connections_from(&dir);
    assert!(notice.is_none());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "加载器读回库");
    assert_eq!(items[0].driver, "mysql");
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(3306));
    assert_eq!(items[0].database.as_deref(), Some("test"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loader_reflects_manager_delete() {
    let dir = temp_dir("delete");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let manager =
            GlobalDatabaseManager::new(dir.join("global.db"), dir.join("analytics.duckdb"), 2)
                .await
                .expect("init global db");
        manager
            .save_global_connection(save_input(
                "G_conn_delete",
                "待删除库",
                "mysql",
                "mysql://root:secret@127.0.0.1:3306/test",
            ))
            .await
            .expect("save");
        // 物理删除（is_active=1 过滤下自然从列表消失）。
        manager
            .delete_global_connection("G_conn_delete")
            .await
            .expect("delete");
        drop(manager);
    });
    drop(runtime);

    let (items, notice) = load_persisted_connections_from(&dir);
    assert!(notice.is_none());
    assert!(items.is_empty(), "删除后连接应消失");

    let _ = std::fs::remove_dir_all(&dir);
}
