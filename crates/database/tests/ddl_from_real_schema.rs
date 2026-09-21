//! 真机回归：从真实内省结果**合成**的 `CREATE TABLE` DDL 是否可用。
//!
//! 属性面板的「DDL（合成）」分区吃三份内省数据（列 / 约束 / 索引），由
//! `sql_gen::create_table_ddl` 纯函数拼出。纯函数本身有 8 项单测；这里补的是
//! **端到端**那一半：驱动真正返回的列类型与约束形状能拼出可用 DDL
//! （单测用的是手写结构体，测不到驱动侧的形状差异）。
//!
//! 未设 `RDS_TEST_PG_URL` 时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//!   cargo test -p rds-database --test ddl_from_real_schema -- --nocapture
//! ```

use std::sync::Arc;

use engine::connection_manager::{ConnectionInfo, ConnectionManager, ConnectionType};
use engine::driver::registry::DriverConnectionConfig;
use engine::driver::{AutoDriverRegistrar, DriverRegistry};
use rds_database::metadata_service::MetadataService;

/// 只造这一张表（名固定，跑完即删），避免污染端点。
/// 表的**词干**：每个用例再加自己的后缀。
///
/// 为什么每个用例要用不同的表名：`cargo test` 默认**并行**跑同一个文件里的用例，
/// 而四个库的用例各自建/删一张同名探针表 —— 并行下必然撞
/// `Table 'rds_ddl_probe' already exists`（真机踩到过）。
const TABLE_STEM: &str = "rds_ddl_probe";

/// 建一条真机连接并注册进管理器（`MetadataService` 从这里取连接）。
async fn manager_with(url: &str, driver: &str) -> Arc<ConnectionManager> {
    AutoDriverRegistrar::auto_register();
    let factory =
        DriverRegistry::get(driver).unwrap_or_else(|| panic!("驱动 {driver} 应在注册表里"));
    let mut config = DriverConnectionConfig::new(driver);
    config.name = Some("ddl-probe".to_string());
    config.url_override = Some(url.to_string());
    let db = factory.create(config.clone()).await.expect("建连成功");

    let manager = Arc::new(ConnectionManager::new());
    manager
        .add_connection(
            "ddl-probe".to_string(),
            db,
            ConnectionInfo {
                id: "ddl-probe".to_string(),
                name: "ddl-probe".to_string(),
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

#[tokio::test(flavor = "multi_thread")]
async fn composed_ddl_from_a_real_postgres_schema() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "postgres").await;
    let key = "ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_pg");

    let db = manager.get_connection(&key).await.expect("取连接");
    db.query(&format!("DROP TABLE IF EXISTS {table}"))
        .await
        .expect("清理旧表");
    db.query(&format!(
        "CREATE TABLE {table} (\
             id int4 PRIMARY KEY, \
             parent_id int4 REFERENCES {table}(id) ON DELETE CASCADE, \
             amount numeric(10,2) NOT NULL DEFAULT 0, \
             tag varchar(8) UNIQUE, \
             note text)"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "postgres", "public", &table)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "postgres", "public", &table)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "postgres", "public", &table)
        .await
        .expect("索引内省");

    let ddl =
        rds_database::sql_gen::create_table_ddl(&format!("public.{table}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL -----\n{ddl}\n--------------------\n");

    assert!(
        ddl.contains(&format!("CREATE TABLE public.{table} (")),
        "{ddl}"
    );
    // 列类型来自驱动内省：必须有真类型，不能是空串
    for c in ["id", "amount", "tag", "note"] {
        assert!(
            ddl.lines()
                .any(|l| l.trim_start().starts_with(&format!("{c} "))),
            "缺列 {c}：{ddl}"
        );
    }
    assert!(ddl.contains("NOT NULL"), "非空丢了：{ddl}");
    assert!(ddl.contains("DEFAULT"), "默认值丢了：{ddl}");
    // 主键三档回退里，约束那一档现在是真的了（`list_constraints` 已实现）
    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");
    // 唯一 / 外键：以前这两条断言是**否定式**（钉住「list_constraints 没实现」这个限制），
    // 实现补上之后翻成肯定式。
    assert!(ddl.contains("UNIQUE (tag)"), "唯一约束丢了：{ddl}");
    assert!(
        ddl.contains(&format!("FOREIGN KEY (parent_id) REFERENCES {table} (id)")),
        "外键丢了：{ddl}"
    );
    assert!(ddl.contains("ON DELETE CASCADE"), "外键规则丢了：{ddl}");

    // 收尾：不留测试表
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}

/// SQLite 那一份：三个约束来源各不相同（`table_info` 的 pk 列 / `foreign_key_list` /
/// `index_list` 的 `origin='u'`），没真库验不到。
#[tokio::test(flavor = "multi_thread")]
async fn composed_ddl_from_a_real_sqlite_schema() {
    let Ok(path) = std::env::var("RDS_TEST_SQLITE_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_SQLITE_PATH，跳过");
        return;
    };
    let manager = manager_with(&path, "sqlite").await;
    let key = "ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_sqlite");
    let db = manager.get_connection(&key).await.expect("取连接");

    // SQLite 要先有被引用的表才能建外键（且建表时就要声明）
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    let _ = db
        .query(&format!("DROP TABLE IF EXISTS {table}_parent"))
        .await;
    db.query(&format!(
        "CREATE TABLE {table}_parent (id INTEGER PRIMARY KEY)"
    ))
    .await
    .expect("建父表");
    db.query(&format!(
        "CREATE TABLE {table} (\
             id INTEGER PRIMARY KEY, \
             parent_id INTEGER REFERENCES {table}_parent(id) ON DELETE CASCADE, \
             amount NUMERIC NOT NULL DEFAULT 0, \
             tag TEXT UNIQUE)"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "main", "main", &table)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "main", "main", &table)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "main", "main", &table)
        .await
        .expect("索引内省");

    let ddl = rds_database::sql_gen::create_table_ddl(&format!("main.{table}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL（SQLite）-----\n{ddl}\n--------------------\n");

    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");
    assert!(ddl.contains("NOT NULL"), "非空丢了：{ddl}");
    assert!(ddl.contains("DEFAULT"), "默认值丢了：{ddl}");
    assert!(ddl.contains("UNIQUE (tag)"), "唯一约束丢了：{ddl}");
    assert!(
        ddl.contains(&format!(
            "FOREIGN KEY (parent_id) REFERENCES {table}_parent (id)"
        )),
        "外键丢了：{ddl}"
    );

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    let _ = db
        .query(&format!("DROP TABLE IF EXISTS {table}_parent"))
        .await;
}

/// MySQL 那一份：约束要从三张 `information_schema` 表拼出来，没真库验不到
/// （尤其 `GROUP_CONCAT ... ORDER BY ORDINAL_POSITION` 的列序）。
#[tokio::test(flavor = "multi_thread")]
async fn composed_ddl_from_a_real_mysql_schema() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "mysql").await;
    let key = "ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_mysql");
    let db = manager.get_connection(&key).await.expect("取连接");

    // MySQL 的连接串里库名是 `mysql`（系统库）—— 表名带 rds_ 前缀，跑完即删
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}_parent")).await;
    db.query(&format!(
        "CREATE TABLE {table}_parent (id INT PRIMARY KEY) ENGINE=InnoDB"
    ))
    .await
    .expect("建父表");
    db.query(&format!(
        "CREATE TABLE {table} (\
             id INT PRIMARY KEY, \
             parent_id INT, \
             amount DECIMAL(10,2) NOT NULL DEFAULT 0, \
             tag VARCHAR(8), \
             UNIQUE KEY uq_tag (tag), \
             CONSTRAINT fk_parent FOREIGN KEY (parent_id) REFERENCES {table}_parent(id) ON DELETE CASCADE\
         ) ENGINE=InnoDB"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "mysql", "mysql", &table)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "mysql", "mysql", &table)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "mysql", "mysql", &table)
        .await
        .expect("索引内省");

    let ddl = rds_database::sql_gen::create_table_ddl(&format!("mysql.{table}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL（MySQL）-----\n{ddl}\n--------------------\n");

    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");
    assert!(ddl.contains("NOT NULL"), "非空丢了：{ddl}");
    assert!(ddl.contains("DEFAULT"), "默认值丢了：{ddl}");
    assert!(ddl.contains("UNIQUE (tag)"), "唯一约束丢了：{ddl}");
    assert!(
        ddl.contains(&format!("FOREIGN KEY (parent_id) REFERENCES {table}_parent (id)")),
        "外键丢了：{ddl}"
    );
    assert!(ddl.contains("ON DELETE CASCADE"), "外键规则丢了：{ddl}");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}_parent")).await;
}

/// DuckDB 那一份：约束在 `duckdb_constraints()` 里（不是 information_schema）。
#[tokio::test(flavor = "multi_thread")]
async fn composed_ddl_from_a_real_duckdb_schema() {
    let Ok(path) = std::env::var("RDS_TEST_DUCKDB_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_DUCKDB_PATH，跳过");
        return;
    };
    let manager = manager_with(&path, "duckdb").await;
    let key = "ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_duckdb");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}_parent")).await;
    db.query(&format!("CREATE TABLE {table}_parent (id INTEGER PRIMARY KEY)"))
        .await
        .expect("建父表");
    db.query(&format!(
        "CREATE TABLE {table} (\
             id INTEGER PRIMARY KEY, \
             parent_id INTEGER REFERENCES {table}_parent(id), \
             amount DECIMAL(10,2) NOT NULL DEFAULT 0, \
             tag VARCHAR UNIQUE)"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "main", "main", &table)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "main", "main", &table)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "main", "main", &table)
        .await
        .expect("索引内省");

    let ddl = rds_database::sql_gen::create_table_ddl(&format!("main.{table}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL（DuckDB）-----\n{ddl}\n--------------------\n");

    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");
    assert!(ddl.contains("NOT NULL"), "非空丢了：{ddl}");
    assert!(ddl.contains("DEFAULT"), "默认值丢了：{ddl}");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}_parent")).await;
}

/// Official PostgreSQL：与 sqlx 那份**共用同一份 SQL**，这里验的是"两个驱动都真的接上了"。
#[tokio::test(flavor = "multi_thread")]
async fn composed_ddl_from_a_real_official_postgres_schema() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "postgres_native").await;
    let key = "ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_pgnative");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!(
        "CREATE TABLE {table} (\
             id int4 PRIMARY KEY, \
             amount numeric(10,2) NOT NULL DEFAULT 0, \
             tag varchar(8) UNIQUE)"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "postgres", "public", &table)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "postgres", "public", &table)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "postgres", "public", &table)
        .await
        .expect("索引内省");

    let ddl = rds_database::sql_gen::create_table_ddl(&format!("public.{table}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL（PG Official）-----\n{ddl}\n--------------------\n");

    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");
    assert!(ddl.contains("UNIQUE (tag)"), "唯一约束丢了：{ddl}");
    assert!(ddl.contains("NOT NULL"), "非空丢了：{ddl}");
    assert!(ddl.contains("DEFAULT"), "默认值丢了：{ddl}");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}

/// Official MySQL：同上，验两个 MySQL 驱动都接上了。
#[tokio::test(flavor = "multi_thread")]
async fn composed_ddl_from_a_real_official_mysql_schema() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let manager = manager_with(&url, "mysql_native").await;
    let key = "ddl-probe".to_string();
    let table = format!("{TABLE_STEM}_mysqlnative");
    let db = manager.get_connection(&key).await.expect("取连接");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
    db.query(&format!(
        "CREATE TABLE {table} (\
             id INT PRIMARY KEY, \
             amount DECIMAL(10,2) NOT NULL DEFAULT 0, \
             tag VARCHAR(8), \
             UNIQUE KEY uq_tag (tag)\
         ) ENGINE=InnoDB"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "mysql", "mysql", &table)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "mysql", "mysql", &table)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "mysql", "mysql", &table)
        .await
        .expect("索引内省");

    let ddl = rds_database::sql_gen::create_table_ddl(&format!("mysql.{table}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL（MySQL Official）-----\n{ddl}\n--------------------\n");

    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");
    assert!(ddl.contains("UNIQUE (tag)"), "唯一约束丢了：{ddl}");
    assert!(ddl.contains("NOT NULL"), "非空丢了：{ddl}");
    assert!(ddl.contains("DEFAULT"), "默认值丢了：{ddl}");

    let _ = db.query(&format!("DROP TABLE IF EXISTS {table}")).await;
}
