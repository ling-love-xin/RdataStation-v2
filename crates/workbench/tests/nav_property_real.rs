//! SQLite / DuckDB 全链路验证（2026-10-03）：连接 → 内省 → 导航加载 → 属性面板。
//!
//! 外部端点（MySQL / PG）在当前环境不可达时的替代验证：本地文件库全量链路，
//! 与 UI 同源（`DataSourceService::test` = 对话框「测试连接」；`ConnectionService::connect_with_type`
//! = 导航「连接」；`NavigatorService::load_children` = 树展开；`property_panel::load_properties` =
//! 属性面板取数；`PropertyRegistryImpl` = 属性页注册表（权限 / 依赖灰置））。
//!
//! fixture 库由本测试**自建**（临时目录，含表 / 视图 / 索引 / 外键 / 默认值 / 数据），
//! 不依赖用户机器上的任何路径；数据根由 `paths` test-support 隔离，不写产品目录。
//!
//! 运行：`cargo test -p rds-workbench --test nav_property_real -j 2 -- --nocapture`

use std::sync::Arc;

use connection::model::DataSourceSaveInput;
use database::metadata_service::MetadataService;
use database::model::{NavFolder, NavNodeKind, NavPath};
use database::navigator_service::NavigatorService;
use database::property_panel::{self, PropertyRegistryImpl};
use engine::connection_manager::{ConnectionInfo, ConnectionManager, ConnectionType};
use engine::driver::{AutoDriverRegistrar, DriverRegistry};
use engine::driver::registry::DriverConnectionConfig;
use rds_workbench::services::connection_service::{ConnectRequest, ConnectionService};
use rds_workbench::services::data_source_service::DataSourceService;
use property::{PageId, PropertyKind, PropertyRegistry as _, SupportState};

/// 唯一临时库路径（并行跑不撞名）。
fn fixture_path(driver: &str) -> String {
    let ext = if driver == "sqlite" { "db" } else { "duckdb" };
    std::env::temp_dir()
        .join(format!("rds_nav_prop_{}_{}.{ext}", driver, std::process::id()))
        .to_string_lossy()
        .into_owned()
}

/// 建 fixture：父表 + 订单表（外键 / 默认值 / 唯一索引）+ 视图 + 两行数据。
async fn create_fixture(conn_id: &str, manager: &Arc<ConnectionManager>, driver: &str) {
    let db = manager
        .get_connection(&conn_id.to_string())
        .await
        .unwrap_or_else(|| panic!("{driver} 取连接失败"));
    for sql in [
        "CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, parent_id INTEGER, tag TEXT, \
         note TEXT DEFAULT 'x', FOREIGN KEY (parent_id) REFERENCES parent(id))",
        "CREATE UNIQUE INDEX idx_orders_tag ON orders (tag)",
        "CREATE VIEW v_orders AS SELECT id, tag FROM orders",
        "INSERT INTO parent (id, name) VALUES (1, 'a'), (2, 'b')",
        "INSERT INTO orders (id, parent_id, tag) VALUES (1, 1, 't1'), (2, 2, 't2')",
    ] {
        db.query(sql).await.unwrap_or_else(|e| panic!("{driver} 执行 {sql} 失败：{e}"));
    }
}

/// 建立一条真实连接并注册进共享管理器（与导航「连接」同源）。
async fn connect_real(
    driver: &str,
    url: &str,
    conn_service: &ConnectionService,
) -> (String, Arc<ConnectionManager>) {
    let manager = Arc::new(ConnectionManager::new());
    let req = ConnectRequest {
        conn_id: Some(format!("nav_prop_{driver}")),
        db_type: driver.to_string(),
        url: url.to_string(),
        name: Some(driver.to_string()),
        connection_type: ConnectionType::Global,
        project_path: None,
        description: None,
        driver_id: Some(driver.to_string()),
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: Some(false),
        password: None,
        skip_persistence: Some(true),
        network_method: None,
    };
    let (cid, db) = conn_service
        .connect_with_type(req)
        .await
        .unwrap_or_else(|e| panic!("{driver} 真实连接失败：{e}"));
    // 与 zz_fixture_probe 不同：本测试自建管理器，把连接挂进同一个 manager 供导航消费。
    let info = ConnectionInfo {
        id: cid.clone(),
        name: driver.to_string(),
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
    };
    manager.add_connection(cid.clone(), db, info, DriverConnectionConfig::new(driver)).await
        .unwrap_or_else(|e| panic!("{driver} 注册连接失败：{e}"));
    (cid, manager)
}

/// 一条 SQLite / DuckDB 的全链路验证。
async fn run_nav_property_chain(driver: &str) {
    let url = fixture_path(driver);
    AutoDriverRegistrar::auto_register();

    let dir = std::env::temp_dir().join(format!("rds_nav_prop_meta_{}_{}", driver, std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let manager_meta = engine::persistence::global_db::GlobalDatabaseManager::new(
        dir.join("global.db"),
        dir.join("analytics.duckdb"),
        2,
    )
    .await
    .expect("init global db");
    let manager_meta: &'static _ = Box::leak(Box::new(manager_meta));
    let ds_service = DataSourceService::new(manager_meta).with_analysis_db(dir.join("secret.duckdb"));
    let conn_service = ConnectionService::new(Arc::new(ConnectionManager::new()));

    // 1) 服务层「测试连接」（对话框按钮同源）
    let input = DataSourceSaveInput::new(driver, driver, &url);
    let t = ds_service.test(&input, None).await;
    assert!(t.success, "{driver} 测试连接应成功：{}", t.message);
    println!("[{driver}] 测试连接 OK（{t:?}）");

    // 2) 真实连接（导航「连接」同源）+ 建 fixture
    let (cid, manager) = connect_real(driver, &url, &conn_service).await;
    println!("[{driver}] 真实连接 OK：{cid}");
    create_fixture(&cid, &manager, driver).await;

    // 3) 导航加载（与树展开同源，cache-aside 写穿 L2）
    let nav = NavigatorService::new(manager.clone());
    let catalogs = nav.load_children(&cid, &NavPath::Connection).await
        .unwrap_or_else(|e| panic!("{driver} 加载 catalogs 失败：{e}"));
    assert!(!catalogs.is_empty(), "{driver} catalogs 非空");
    let catalog = catalogs
        .iter()
        .find(|n| n.kind == NavNodeKind::Catalog)
        .unwrap_or_else(|| panic!("{driver} catalogs 应含 Catalog 节点：{catalogs:?}"));
    println!("[{driver}] Catalog 层 OK：{}", catalog.name);

    let folders = nav.load_children(&cid, &NavPath::Catalog { catalog: catalog.name.clone() })
        .await
        .unwrap_or_else(|e| panic!("{driver} 加载文件夹失败：{e}"));
    let folder_names: Vec<&str> = folders.iter().map(|n| n.name.as_str()).collect();
    assert!(
        folder_names.iter().any(|n| n.starts_with("表")),
        "{driver} 应有「表」文件夹（可带计数）：{folder_names:?}"
    );
    println!("[{driver}] 类别文件夹 OK：{folder_names:?}");

    // 表文件夹 → 两张表
    let tables = nav
        .load_children(
            &cid,
            &NavPath::Folder {
                catalog: catalog.name.clone(),
                schema: catalog.name.clone(),
                folder: NavFolder::Tables,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{driver} 加载表失败：{e}"));
    let table_names: Vec<&str> = tables.iter().map(|n| n.name.as_str()).collect();
    assert!(table_names.contains(&"orders"), "{driver} 表含 orders：{table_names:?}");
    assert!(table_names.contains(&"parent"), "{driver} 表含 parent：{table_names:?}");
    println!("[{driver}] 表层 OK：{table_names:?}");

    // 视图文件夹 → v_orders
    let views = nav
        .load_children(
            &cid,
            &NavPath::Folder {
                catalog: catalog.name.clone(),
                schema: catalog.name.clone(),
                folder: NavFolder::Views,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{driver} 加载视图失败：{e}"));
    let view_names: Vec<&str> = views.iter().map(|n| n.name.as_str()).collect();
    assert!(view_names.contains(&"v_orders"), "{driver} 视图含 v_orders：{view_names:?}");
    println!("[{driver}] 视图层 OK：{view_names:?}");

    // 列层（orders → 4 列）
    let cols = nav
        .load_children(
            &cid,
            &NavPath::Table {
                catalog: catalog.name.clone(),
                schema: catalog.name.clone(),
                table: "orders".to_string(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{driver} 加载列失败：{e}"));
    let col_names: Vec<&str> = cols.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(col_names.len(), 4, "{driver} orders 应有 4 列：{col_names:?}");
    println!("[{driver}] 列层 OK：{col_names:?}");

    // 4) 属性面板（抽取 rds-property 后的取数）
    let metadata = MetadataService::new(manager.clone());
    let make_ref = |kind: PropertyKind, name: &str| database::model::PropertyRef {
        conn_id: cid.clone(),
        source: database::model::NavSource::Global,
        catalog: Some(catalog.name.clone()),
        schema: Some(catalog.name.clone()),
        parent: if kind == PropertyKind::Column { Some("orders".to_string()) } else { None },
        name: name.to_string(),
        kind,
    };

    // 表：网格（列数）+ 列 / 索引 / 约束 / DDL 分区
    let table_props = property_panel::load_properties(
        &metadata,
        &make_ref(PropertyKind::Table, "orders"),
        driver,
        driver,
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("{driver} 表属性失败：{e}"));
    assert!(
        table_props
            .properties
            .iter()
            .any(|r| r.label == "列数" && r.value == "4"),
        "{driver} 表属性网格应有列数=4：{:?}",
        table_props.properties
    );
    let section_ids: Vec<PageId> = table_props.sections.iter().map(|s| s.id).collect();
    assert!(section_ids.contains(&PageId::Columns), "{driver} 表分区含列：{section_ids:?}");
    assert!(section_ids.contains(&PageId::Indexes), "{driver} 表分区含索引：{section_ids:?}");
    assert!(section_ids.contains(&PageId::Constraints), "{driver} 表分区含约束：{section_ids:?}");
    assert!(section_ids.contains(&PageId::Ddl), "{driver} 表分区含 DDL：{section_ids:?}");
    let cols_section = table_props.sections.iter().find(|s| s.id == PageId::Columns).unwrap();
    assert_eq!(cols_section.table.rows.len(), 4, "{driver} 列分区 4 行");
    println!(
        "[{driver}] 表属性面板 OK：网格 {} 行 / 分区 {:?}",
        table_props.properties.len(),
        section_ids
    );

    // 视图：列 + DDL
    let view_props = property_panel::load_properties(
        &metadata,
        &make_ref(PropertyKind::View, "v_orders"),
        driver,
        driver,
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("{driver} 视图属性失败：{e}"));
    assert!(
        view_props.sections.iter().any(|s| s.id == PageId::Columns),
        "{driver} 视图分区含列"
    );
    println!("[{driver}] 视图属性面板 OK");

    // 列：类型 / 可空 / 主键 / 外键 / 默认值
    let col_props = property_panel::load_properties(
        &metadata,
        &make_ref(PropertyKind::Column, "tag"),
        driver,
        driver,
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("{driver} 列属性失败：{e}"));
    assert!(
        col_props
            .properties
            .iter()
            .any(|r| r.label == "所属表"),
        "{driver} 列属性应含所属表：{:?}",
        col_props.properties
    );
    println!("[{driver}] 列属性面板 OK：{:?}", col_props.properties.iter().map(|r| r.label.as_str()).collect::<Vec<_>>());

    // 连接：名称 / 归属域 / 驱动
    let conn_props = property_panel::load_properties(
        &metadata,
        &make_ref(PropertyKind::Connection, driver),
        driver,
        driver,
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("{driver} 连接属性失败：{e}"));
    assert!(
        conn_props.properties.iter().any(|r| r.label == "驱动"),
        "{driver} 连接属性含驱动"
    );
    println!("[{driver}] 连接属性面板 OK");

    // 5) 属性页注册表（权限 / 依赖灰置）
    let registry = PropertyRegistryImpl;
    let table_pages = registry.pages_for(PropertyKind::Table);
    let ids: Vec<PageId> = table_pages.iter().map(|p| p.id).collect();
    assert!(ids.contains(&PageId::Privileges) && ids.contains(&PageId::Dependencies), "{driver} 表注册表含权限/依赖页");
    assert!(table_pages.iter().all(|p| {
        p.state == SupportState::Supported
            || matches!(p.id, PageId::Privileges | PageId::Dependencies)
    }));
    println!("[{driver}] 注册表 OK：{ids:?}");

    // 6) 二次加载命中 L2（cache-aside：结果一致）
    let tables_again = nav
        .load_children(
            &cid,
            &NavPath::Folder {
                catalog: catalog.name.clone(),
                schema: catalog.name.clone(),
                folder: NavFolder::Tables,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{driver} 二次加载失败：{e}"));
    assert_eq!(
        tables_again.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        table_names,
        "{driver} 二次加载结果一致"
    );
    println!("[{driver}] 二次加载（cache-aside）OK");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&url);
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_full_nav_property_chain() {
    run_nav_property_chain("sqlite").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_full_nav_property_chain() {
    run_nav_property_chain("duckdb").await;
}
