//! 真机回归：**源版** DDL 的取法（`Database::get_table_ddl`）与它的边界。
//!
//! 属性面板的 DDL 是三分：**能取源就取源**，取不到才由目录信息合成
//! （`sql_gen::create_table_ddl`，另一半在 `ddl_from_real_schema.rs`）。这里钉取源那一半。
//!
//! 为什么必须真机验：源版 DDL 的取法每个库都不同，且都读各自的目录表——
//! MySQL `SHOW CREATE TABLE`（对**视图也有效**，返回的列名是 `Create View`）、
//! SQLite `sqlite_master.sql`、DuckDB `duckdb_tables()` / `duckdb_views()` 的 `sql` 列。
//! 假端点验不出「取回来的到底是不是原文」。
//!
//! 断言口径：挑**合成器给不出**的特征（`ENGINE=InnoDB` / `CHECK` 表达式 / 视图定义 /
//! 用户写下的原文），否则「取源成功」这句话本身没被验到。
//!
//! 未设环境变量时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! RDS_TEST_DUCKDB_PATH='D:\data\123' \
//!   cargo test -p rds-database --test source_ddl_from_real_schema -- --nocapture
//! ```

use std::sync::Arc;

use engine::connection_manager::{ConnectionInfo, ConnectionManager, ConnectionType};
use engine::driver::registry::DriverConnectionConfig;
use engine::driver::{AutoDriverRegistrar, DriverRegistry};
use rds_database::metadata_service::MetadataService;

/// 表名词干：每例再加自己的后缀（并行跑，不能撞名 —— 与 `ddl_from_real_schema` 同规矩）。
const TABLE_STEM: &str = "rds_src_ddl";

async fn manager_with(url: &str, driver: &str) -> Arc<ConnectionManager> {
    AutoDriverRegistrar::auto_register();
    let factory =
        DriverRegistry::get(driver).unwrap_or_else(|| panic!("驱动 {driver} 应在注册表里"));
    let mut config = DriverConnectionConfig::new(driver);
    config.name = Some("src-ddl-probe".to_string());
    config.url_override = Some(url.to_string());
    let db = factory.create(config.clone()).await.expect("建连成功");

    let manager = Arc::new(ConnectionManager::new());
    manager
        .add_connection(
            "src-ddl-probe".to_string(),
            db,
            ConnectionInfo {
                id: "src-ddl-probe".to_string(),
                name: "src-ddl-probe".to_string(),
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

fn show(label: &str, ddl: &Option<String>) {
    match ddl {
        Some(text) => println!("\n----- {label} -----\n{text}\n--------------------\n"),
        None => println!("\n----- {label}：该库给不出源版 -----\n"),
    }
}

/// MySQL（sqlx）：`SHOW CREATE TABLE` —— 表与视图同一个语句。
#[tokio::test(flavor = "multi_thread")]
async fn source_ddl_from_a_real_mysql_schema() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "mysql").await;
    let key = "src-ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_mysql");
    let view = format!("{TABLE_STEM}_mysql_v");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP VIEW IF EXISTS {view}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!(
        "CREATE TABLE {table} (id INT PRIMARY KEY, tag VARCHAR(8), \
         note VARCHAR(16) DEFAULT 'x', CHECK (length(tag) > 0)) \
         ENGINE=InnoDB DEFAULT CHARSET=utf8mb4"
    ))
    .await
    .expect("建表");
    db.query(&format!("CREATE VIEW {view} AS SELECT id FROM {table}"))
        .await
        .expect("建视图");

    let svc = MetadataService::new(Arc::clone(&manager));
    let table_ddl = svc
        .get_table_ddl(&key, "mysql", "mysql", &table)
        .await
        .expect("取表 DDL");
    let view_ddl = svc
        .get_table_ddl(&key, "mysql", "mysql", &view)
        .await
        .expect("取视图 DDL");
    show("源版 DDL（MySQL）", &table_ddl);
    show("源版 DDL（MySQL 视图）", &view_ddl);

    let table_ddl = table_ddl.expect("MySQL 应给源版 DDL");
    // 这三样合成器都给不出（表级选项 / 引擎 / CHECK 表达式）
    assert!(table_ddl.contains("ENGINE=InnoDB"), "不是原文：{table_ddl}");
    assert!(table_ddl.contains("utf8mb4"), "不是原文：{table_ddl}");
    assert!(table_ddl.contains("CHECK"), "CHECK 表达式丢了：{table_ddl}");

    let view_ddl = view_ddl.expect("MySQL 对视图也应给源版 DDL");
    assert!(view_ddl.contains("VIEW"), "视图定义丢了：{view_ddl}");
    assert!(
        view_ddl.contains(&table),
        "视图定义里应提到基表：{view_ddl}"
    );

    let _ = db.query(&format!("DROP VIEW IF EXISTS {view}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}

/// MySQL(Official)：同一个语句，验两个驱动都接上了。
#[tokio::test(flavor = "multi_thread")]
async fn source_ddl_from_a_real_official_mysql_schema() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "mysql_native").await;
    let key = "src-ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_mysqlnative");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!(
        "CREATE TABLE {table} (id INT PRIMARY KEY, tag VARCHAR(8)) \
         ENGINE=InnoDB DEFAULT CHARSET=utf8mb4"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let ddl = svc
        .get_table_ddl(&key, "mysql", "mysql", &table)
        .await
        .expect("取 DDL")
        .expect("MySQL(Official) 应给源版 DDL");
    show("源版 DDL（MySQL Official）", &Some(ddl.clone()));
    assert!(ddl.contains("ENGINE=InnoDB"), "不是原文：{ddl}");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}

/// SQLite：`sqlite_master.sql` 就是用户写下的原文（含视图定义）。
#[tokio::test(flavor = "multi_thread")]
async fn source_ddl_from_a_real_sqlite_schema() {
    let Ok(path) = std::env::var("RDS_TEST_SQLITE_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_SQLITE_PATH，跳过");
        return;
    };
    let manager = manager_with(&path, "sqlite").await;
    let key = "src-ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_sqlite");
    let view = format!("{TABLE_STEM}_sqlite_v");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP VIEW IF EXISTS {view}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!(
        "CREATE TABLE {table} (id INTEGER PRIMARY KEY, tag TEXT UNIQUE, \
         note TEXT DEFAULT 'x')"
    ))
    .await
    .expect("建表");
    db.query(&format!("CREATE VIEW {view} AS SELECT id FROM {table}"))
        .await
        .expect("建视图");

    let svc = MetadataService::new(Arc::clone(&manager));
    let table_ddl = svc
        .get_table_ddl(&key, "main", "main", &table)
        .await
        .expect("取表 DDL");
    let view_ddl = svc
        .get_table_ddl(&key, "main", "main", &view)
        .await
        .expect("取视图 DDL");
    show("源版 DDL（SQLite）", &table_ddl);
    show("源版 DDL（SQLite 视图）", &view_ddl);

    let table_ddl = table_ddl.expect("SQLite 应给源版 DDL");
    assert!(
        table_ddl.contains("DEFAULT 'x'"),
        "应逐字是原文：{table_ddl}"
    );
    assert!(table_ddl.contains("UNIQUE"), "原文丢了约束：{table_ddl}");

    let view_ddl = view_ddl.expect("SQLite 对视图也应给源版 DDL");
    assert!(view_ddl.contains("CREATE VIEW"), "视图定义丢了：{view_ddl}");

    let _ = db.query(&format!("DROP VIEW IF EXISTS {view}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}

/// DuckDB：`duckdb_tables()` / `duckdb_views()` 的 `sql` 列。
#[tokio::test(flavor = "multi_thread")]
async fn source_ddl_from_a_real_duckdb_schema() {
    let Ok(path) = std::env::var("RDS_TEST_DUCKDB_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_DUCKDB_PATH，跳过");
        return;
    };
    let manager = manager_with(&path, "duckdb").await;
    let key = "src-ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_duckdb");
    let view = format!("{TABLE_STEM}_duckdb_v");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP VIEW IF EXISTS {view}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!(
        "CREATE TABLE {table} (id INTEGER PRIMARY KEY, tag VARCHAR UNIQUE, note VARCHAR)"
    ))
    .await
    .expect("建表");
    db.query(&format!("CREATE VIEW {view} AS SELECT id FROM {table}"))
        .await
        .expect("建视图");

    let svc = MetadataService::new(Arc::clone(&manager));
    let table_ddl = svc
        .get_table_ddl(&key, "main", "main", &table)
        .await
        .expect("取表 DDL");
    let view_ddl = svc
        .get_table_ddl(&key, "main", "main", &view)
        .await
        .expect("取视图 DDL");
    show("源版 DDL（DuckDB）", &table_ddl);
    show("源版 DDL（DuckDB 视图）", &view_ddl);

    let table_ddl = table_ddl.expect("DuckDB 应给源版 DDL");
    assert!(table_ddl.contains("CREATE TABLE"), "{table_ddl}");
    assert!(table_ddl.contains("UNIQUE"), "原文丢了约束：{table_ddl}");

    let view_ddl = view_ddl.expect("DuckDB 对视图也应给源版 DDL");
    assert!(view_ddl.contains("CREATE VIEW"), "视图定义丢了：{view_ddl}");

    let _ = db.query(&format!("DROP VIEW IF EXISTS {view}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}

/// 边界：**PostgreSQL 给不出源版**（没有 `SHOW CREATE TABLE` 的等价物）。
///
/// 这条是**否定式**断言，钉的是「面板为什么会退化成合成」——PG 上不存在源版可取，
/// 不是取失败。哪天有人用 `pg_get_*def` 拼出原文，这条要翻成肯定式。
#[tokio::test(flavor = "multi_thread")]
async fn postgres_has_no_source_ddl() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "postgres").await;
    let key = "src-ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_pg");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!("CREATE TABLE {table} (id int4 PRIMARY KEY)"))
        .await
        .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let ddl = svc
        .get_table_ddl(&key, "postgres", "public", &table)
        .await
        .expect("取 DDL 本身不该报错");
    show("源版 DDL（PostgreSQL）", &ddl);
    assert!(
        ddl.is_none(),
        "PG 没有源版可取，应返回 None（面板据此合成）：{ddl:?}"
    );

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}
