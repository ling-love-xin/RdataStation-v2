//! 真机回归：PG 导航内省的**真实性**（2026-09-22 修的那三个坑）。
//!
//! 三条都是真机抓到的，且都不是「显示不漂亮」而是**看不到 / 直接报错**：
//!
//! 1. **不同 schema 下的同名表**：列详情的注释子查询只按 `relname` 匹配 `pg_class`，
//!    两张同名表 → 「子查询返回了多行」→ **整张表展开直接报错**；
//! 2. **schema 清单混进系统噪音**：`information_schema.schemata` 会把 `pg_toast*` /
//!    `pg_temp_*` 一起列出来（真机 28 个里 26 个是这类）；
//! 3. **物化视图不在 `information_schema.tables` 里**（外部表、分区表同理）→ 导航里根本看不到。
//!
//! 未设 `RDS_TEST_PG_URL` 时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//!   cargo test -p rds-database --test postgres_nav_introspection -- --nocapture
//! ```

use std::sync::Arc;

use engine::connection_manager::{ConnectionInfo, ConnectionManager, ConnectionType};
use engine::driver::registry::DriverConnectionConfig;
use engine::driver::{AutoDriverRegistrar, DriverRegistry};
use rds_database::metadata_service::MetadataService;

/// 探针对象名（跑完即删，不留痕）。
const STEM: &str = "rds_nav_probe";

async fn manager_with(url: &str, driver: &str) -> Arc<ConnectionManager> {
    AutoDriverRegistrar::auto_register();
    let factory =
        DriverRegistry::get(driver).unwrap_or_else(|| panic!("驱动 {driver} 应在注册表里"));
    let mut config = DriverConnectionConfig::new(driver);
    config.name = Some("nav-probe".to_string());
    config.url_override = Some(url.to_string());
    let db = factory.create(config.clone()).await.expect("建连成功");

    let manager = Arc::new(ConnectionManager::new());
    manager
        .add_connection(
            "nav-probe".to_string(),
            db,
            ConnectionInfo {
                id: "nav-probe".to_string(),
                name: "nav-probe".to_string(),
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

/// 两个驱动跑同一批断言（sqlx 与 Official 各一份 SQL，容易只改一边）。
async fn nav_introspection_cases(driver: &str) {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    println!("\n=== {driver} ===");
    let manager = manager_with(&url, driver).await;
    let key = "nav-probe".to_string();
    let db = manager.get_connection(&key).await.expect("取连接");
    let svc = MetadataService::new(Arc::clone(&manager));

    // 每个驱动一套自己的对象名：两条用例并行跑，同名对象会被对方 DROP / CREATE 掉
    let stem = format!("{STEM}_{driver}");
    let schema_a = format!("{stem}_a");
    let schema_b = format!("{stem}_b");
    let dup = "dup"; // 两个 schema 下同名
    let matview = format!("{stem}_mv");
    let part = format!("{stem}_part");
    let part_child = format!("{stem}_part_2026");

    // 清场（上一次跑崩了也要能重跑）
    for sql in [
        format!("DROP MATERIALIZED VIEW IF EXISTS public.{matview}"),
        format!("DROP TABLE IF EXISTS public.{part} CASCADE"),
        format!("DROP SCHEMA IF EXISTS {schema_a} CASCADE"),
        format!("DROP SCHEMA IF EXISTS {schema_b} CASCADE"),
    ] {
        let _ = db.query(&sql).await;
    }

    for sql in [
        format!("CREATE SCHEMA {schema_a}"),
        format!("CREATE SCHEMA {schema_b}"),
        format!("CREATE TABLE {schema_a}.{dup} (id int4 PRIMARY KEY)"),
        format!("CREATE TABLE {schema_b}.{dup} (id int4 PRIMARY KEY, note text)"),
        format!("CREATE MATERIALIZED VIEW public.{matview} AS SELECT 1 AS x"),
        format!("CREATE TABLE public.{part} (id int4, d date) PARTITION BY RANGE (d)"),
        format!(
            "CREATE TABLE public.{part_child} PARTITION OF public.{part} \
             FOR VALUES FROM ('2026-01-01') TO ('2027-01-01')"
        ),
    ] {
        db.query(&sql).await.expect("建探针对象");
    }

    // ---- 1) 同名表：两个 schema 都要拿到列，且列数各自正确
    let a = svc
        .list_columns(&key, "postgres", &schema_a, dup)
        .await
        .unwrap_or_else(|e| panic!("同名表（先建的那个）应能内省出来，实际报错：{e}"));
    let b = svc
        .list_columns(&key, "postgres", &schema_b, dup)
        .await
        .unwrap_or_else(|e| panic!("同名表（后建的那个）应能内省出来，实际报错：{e}"));
    assert_eq!(a.len(), 1, "先建的 dup 只有 1 列：{a:?}");
    assert_eq!(b.len(), 2, "后建的 dup 有 2 列（曾经这里是报错）：{b:?}");
    println!("✓ 同名表可内省：{schema_a}.{dup}={} 列，{schema_b}.{dup}={} 列", a.len(), b.len());

    // ---- 2) schema 清单里不许有 pg_ 前缀的系统噪音
    let schemas = svc
        .list_schemas(&key, "postgres")
        .await
        .expect("schema 内省");
    println!("✓ schema 清单：{schemas:?}");
    let noisy: Vec<&String> = schemas
        .iter()
        .filter(|s| s.starts_with("pg_") || s.as_str() == "information_schema")
        .collect();
    assert!(
        noisy.is_empty(),
        "系统噪音不该出现在导航里（pg_toast* / pg_temp_* / information_schema）：{noisy:?}"
    );
    assert!(
        schemas.iter().any(|s| s == &schema_a) && schemas.iter().any(|s| s == &schema_b),
        "用户 schema 必须在（过滤不能过头）：{schemas:?}"
    );

    // ---- 3) 物化视图 / 分区表都要能看见
    let tables = svc
        .list_tables(&key, "postgres", "public")
        .await
        .expect("表清单");
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&matview.as_str()),
        "物化视图要在表清单里（它不在 information_schema.tables 里）：{names:?}"
    );
    assert!(
        names.contains(&part.as_str()) && names.contains(&part_child.as_str()),
        "分区父表与子表都要在：{names:?}"
    );
    println!("✓ 物化视图 / 分区表可见，public 对象数 {}", tables.len());

    // ---- 列详情的内容抽检：类型带长度、默认值、注释、序号、引用
    let detail = svc
        .list_columns(&key, "postgres", "public", &part)
        .await
        .expect("分区表列内省");
    assert_eq!(detail.len(), 2, "分区父表两列：{detail:?}");
    assert_eq!(
        detail[1].data_type, "date",
        "类型应来自 format_type（带长度 / 精度）"
    );
    assert_eq!(
        (detail[0].ordinal, detail[1].ordinal),
        (1, 2),
        "序号是 1 基且连续"
    );
    println!("✓ 列详情：{} | {}", detail[0].data_type, detail[1].data_type);

    // 清场
    for sql in [
        format!("DROP MATERIALIZED VIEW IF EXISTS public.{matview}"),
        format!("DROP TABLE IF EXISTS public.{part} CASCADE"),
        format!("DROP SCHEMA IF EXISTS {schema_a} CASCADE"),
        format!("DROP SCHEMA IF EXISTS {schema_b} CASCADE"),
    ] {
        let _ = db.query(&sql).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_navigation_introspection_is_truthful() {
    nav_introspection_cases("postgres").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn official_postgres_navigation_introspection_is_truthful() {
    nav_introspection_cases("postgres_native").await;
}
