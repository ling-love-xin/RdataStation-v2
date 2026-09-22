//! 真机回归：**PG 看得见服务器上所有的库**（跨库浏览）。
//!
//! 背景：PG 一条连接只绑一个库，`information_schema` 只看得到当前库 —— 此前导航里
//! **只有一个库**（用户以为别的库不见了）。现在 `get_catalogs` 列 `pg_database` 里
//! 全部可连接的库，展开别的库时由 `ConnectionManager::get_scoped_connection`
//! **按库另开一条连接**（主连接那份 URL 只换库名，参数一个不丢）。
//!
//! 钉三件事：
//! 1. 库清单里除了当前库还有别的库（端点上有两个）；
//! 2. 别的库能真展开出 schema / 表 / 列（不是「列出来但打不开」的假节点）；
//! 3. 跨库连接**不进用户可见的连接表**（派生物不该混进连接面板）。
//!
//! 未设 `RDS_TEST_PG_URL` 时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//!   cargo test -p rds-database --test postgres_cross_database -- --nocapture --test-threads=1
//! ```

use std::sync::Arc;

use engine::connection_manager::{ConnectionInfo, ConnectionManager, ConnectionType};
use engine::driver::registry::DriverConnectionConfig;
use engine::driver::{AutoDriverRegistrar, DriverRegistry};
use rds_database::metadata_service::MetadataService;

async fn manager_with(url: &str, driver: &str) -> Arc<ConnectionManager> {
    AutoDriverRegistrar::auto_register();
    let factory =
        DriverRegistry::get(driver).unwrap_or_else(|| panic!("驱动 {driver} 应在注册表里"));
    let mut config = DriverConnectionConfig::new(driver);
    config.name = Some("cross-db-probe".to_string());
    config.url_override = Some(url.to_string());
    let db = factory.create(config.clone()).await.expect("建连成功");

    let manager = Arc::new(ConnectionManager::new());
    manager
        .add_connection(
            "cross-db-probe".to_string(),
            db,
            ConnectionInfo {
                id: "cross-db-probe".to_string(),
                name: "cross-db-probe".to_string(),
                db_type: driver.to_string(),
                url: String::new(),
                server_version: None,
                connection_type: ConnectionType::Global,
                project_id: None,
                driver_id: Some(driver.to_string()),
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                driver_properties: None,
                advanced_options: None,
                description: None,
                use_duckdb_fed: false,
                created_at: std::time::Instant::now(),
            },
            config,
        )
        .await
        .expect("注册连接");
    manager
}

async fn cross_database_cases(driver: &str) {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    println!("\n=== {driver} ===");
    let manager = manager_with(&url, driver).await;
    let key = "cross-db-probe".to_string();
    let svc = MetadataService::new(Arc::clone(&manager));

    // 1) 库清单：应当含服务器上全部可连接的库（不再是 current_database() 一个）
    let catalogs = svc.list_catalogs(&key).await.expect("取库清单");
    println!("✓ 库清单（{} 个）：{catalogs:?}", catalogs.len());
    assert!(!catalogs.is_empty(), "库清单不该为空");

    // 当前库：由驱动自己告诉我们（上层据此决定「用自己的连接」还是「另开一条」）
    let current = {
        let db = manager.get_connection(&key).await.expect("取连接");
        let browser = db.as_metadata_browser().expect("PG 驱动应有元数据浏览器");
        browser.current_catalog().await
    };
    println!("✓ 当前库：{current:?}");
    let others: Vec<String> = catalogs
        .iter()
        .filter(|c| Some(c.as_str()) != current.as_deref())
        .cloned()
        .collect();
    if others.is_empty() {
        eprintln!(
            "⏭️  这个端点上只有一个库（{}），跨库这条路径验不到 —— 不假装验过",
            catalogs.len()
        );
        return;
    }

    // 2) 别的库要能**真展开**（这才是「展示不全」的正面修复：不是列个名字打不开）
    for db_name in &others {
        let schemas = svc
            .list_schemas(&key, db_name)
            .await
            .unwrap_or_else(|e| panic!("展开别的库 {db_name} 的 schema 失败：{e}"));
        // 别把 500 个 schema 全打出来：只报数量与头几个
        println!(
            "✓ {db_name} 的 schema（{} 个），前几个：{:?}",
            schemas.len(),
            schemas.iter().take(5).collect::<Vec<_>>()
        );
        let Some(schema) = schemas.first() else {
            continue;
        };
        let tables = svc
            .list_tables(&key, db_name, schema)
            .await
            .unwrap_or_else(|e| panic!("展开 {db_name}.{schema} 的对象失败：{e}"));
        println!("✓ {db_name}.{schema} 对象数：{}", tables.len());
        if let Some(table) = tables.iter().find(|t| {
            // 先挑基表（视图也能取列，但基表更常见）
            matches!(t.kind, engine::driver::SchemaObjectKind::Table)
        }) {
            let cols = svc
                .list_columns(&key, db_name, schema, &table.name)
                .await
                .unwrap_or_else(|e| panic!("取 {db_name}.{schema}.{} 的列失败：{e}", table.name));
            println!("✓ {db_name}.{schema}.{} 列数：{}", table.name, cols.len());
            assert!(!cols.is_empty(), "表应有列");
        }
    }

    // 3) 跨库连接是**派生物**：不进用户可见的连接表（否则用户会看到自己没建过的连接）
    let ids = manager.get_all_connection_ids().await;
    assert_eq!(
        ids,
        vec![key.clone()],
        "跨库连接不该出现在连接列表里：{ids:?}"
    );
    println!("✓ 连接列表仍只有主连接（跨库连接没混进去）");
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_covers_every_database_on_the_server() {
    cross_database_cases("postgres").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn official_postgres_covers_every_database_on_the_server() {
    cross_database_cases("postgres_native").await;
}
