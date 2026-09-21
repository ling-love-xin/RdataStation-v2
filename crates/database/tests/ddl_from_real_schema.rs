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
const TABLE: &str = "rds_ddl_probe";

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

    let db = manager.get_connection(&key).await.expect("取连接");
    db.query(&format!("DROP TABLE IF EXISTS {TABLE}"))
        .await
        .expect("清理旧表");
    db.query(&format!(
        "CREATE TABLE {TABLE} (\
             id int4 PRIMARY KEY, \
             amount numeric(10,2) NOT NULL DEFAULT 0, \
             tag varchar(8) UNIQUE, \
             note text)"
    ))
    .await
    .expect("建表");

    let svc = MetadataService::new(Arc::clone(&manager));
    let cols = svc
        .list_columns(&key, "postgres", "public", TABLE)
        .await
        .expect("列内省");
    let cons = svc
        .list_constraints(&key, "postgres", "public", TABLE)
        .await
        .expect("约束内省");
    let idx = svc
        .list_indexes(&key, "postgres", "public", TABLE)
        .await
        .expect("索引内省");

    let ddl =
        rds_database::sql_gen::create_table_ddl(&format!("public.{TABLE}"), &cols, &cons, &idx);
    println!("\n----- 合成 DDL -----\n{ddl}\n--------------------\n");

    assert!(
        ddl.contains(&format!("CREATE TABLE public.{TABLE} (")),
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
    // 主键走的是**列上的 `is_primary_key`**（最后一档回退）：
    // `list_constraints` 六个驱动一个都没实现，约束/索引那两档当下恒空。
    assert!(ddl.contains("PRIMARY KEY (id)"), "主键丢了：{ddl}");

    // ⚠️ 钉住**当前限制**：UNIQUE / FOREIGN KEY / CHECK 要 `list_constraints`，
    // 而六个驱动都只实现了 `get_constraints`（它委派给 `list_constraints`）→ 默认空实现。
    // 那一块补上之后，把这两条断言翻过来（改成 `contains`）。
    assert!(
        !ddl.contains("UNIQUE (tag)"),
        "意料之外的进展：UNIQUE 出来了，说明 `list_constraints` 已实现——请把本测试的断言翻成肯定式。DDL：{ddl}"
    );
    assert!(
        !ddl.contains("FOREIGN KEY"),
        "意料之外的进展：外键出来了，说明 `list_constraints` 已实现——请把本测试的断言翻成肯定式。DDL：{ddl}"
    );

    // 收尾：不留测试表
    let _ = db.query(&format!("DROP TABLE IF EXISTS {TABLE}")).await;
}
