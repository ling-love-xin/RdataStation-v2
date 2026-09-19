//! `SidecarDatabase` 的元数据面（P2：驱动进得了导航与搜索）
//!
//! 靶子（`tests/fixture/sidecar.rs`）带一份写死的假 schema。这里考察的是**接上引擎之后**，
//! 导航树 / 属性面板 / `#` 内容档要用的那几件是不是真的拿得到：
//!
//! - 五个文件夹各挑各的类别（表 / 视图 / 例程 / 序列 / 触发器），**共用同一次 `meta.objects`**
//! - 表与视图一次调用返回、由调用方按 `kind` 分成两个文件夹（`NavigatorService::collect_objects`）
//! - 列带得出原始类型与归一化类型（属性面板看前者，程序看后者）
//! - 没给 schema 时落到 `driver.describe` 的 `default_schema`
//! - 例程源码只对例程问；索引 / 约束明细**如实报不支持**
//! - 会话收掉之后：连接域错误（不是"空元数据"）
//!
//! ⚠️ 这里**不连真库**：假 schema 由靶子写死。接真库属实机验收。

use std::time::{Duration, Instant};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use engine::driver::{Database, SchemaObjectKind};
use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::driver::{SessionDriver, SidecarDatabase};
use rds_plugin::sidecar::lifecycle::{Concurrency, ProcessSpec};
use rds_plugin::sidecar::supervisor::{Deployment, SessionOpened, SidecarSupervisor};
use shared::error::{CommonError, ConnectionError, CoreError};

/// 起靶子、开会话，返回（supervisor, 会话 id, 接上引擎的驱动）。
async fn database_for(plugin_id: &str) -> (SidecarSupervisor, String, SidecarDatabase) {
    let mut supervisor = SidecarSupervisor::new();
    supervisor
        .deploy(Deployment {
            plugin_id: plugin_id.to_string(),
            spec: ProcessSpec::new(["fixture"], 1, Concurrency::Serial),
            command: BackendCommand {
                program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture")),
                args: Vec::new(),
            },
            env: Vec::new(),
            protocol: Some("rds-driver/1".to_string()),
        })
        .unwrap();

    let opened = supervisor
        .open_session(plugin_id, "fixture", "s1", json!({}), Instant::now())
        .await
        .expect("开会话应当成功");
    assert!(matches!(opened, SessionOpened::Live { .. }), "{opened:?}");

    let conn = supervisor
        .session_conn_handle("s1")
        .expect("会话应当有连接");
    let descriptor = SessionDriver::new(&conn, "s1")
        .describe("fixture")
        .await
        .expect("描述应当成功");
    let database = SidecarDatabase::new(&conn, "s1", "fixture", descriptor);
    (supervisor, "s1".to_string(), database)
}

fn names(nodes: Vec<engine::driver::NodeInfo>) -> Vec<String> {
    nodes.into_iter().map(|n| n.name).collect()
}

/// 五个文件夹各挑各的类别（同一次内省的五个切面）。
#[tokio::test]
async fn all_five_folders_come_from_the_same_introspection() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.folders").await;

    assert_eq!(
        database.list_catalogs().await.expect("catalog 清单"),
        vec!["main".to_string()]
    );
    assert_eq!(
        database
            .list_schemas("main")
            .await
            .expect("schema 清单"),
        vec!["public".to_string(), "sales".to_string()]
    );

    // 表与视图：一次调用返回两半（下面按 kind 分开）
    let tables = database
        .list_tables("main", Some("public"))
        .await
        .expect("表与视图");
    assert_eq!(names(tables.clone()), vec!["orders", "customers", "recent_orders", "mv_daily"]);

    assert_eq!(
        names(
            database
                .list_procedures("main", Some("public"))
                .await
                .expect("过程")
        ),
        vec!["refresh_orders"]
    );
    assert_eq!(
        names(
            database
                .list_functions("main", Some("public"))
                .await
                .expect("函数")
        ),
        vec!["order_total"]
    );
    assert_eq!(
        names(
            database
                .list_sequences("main", Some("public"))
                .await
                .expect("序列")
        ),
        vec!["orders_seq"]
    );

    // 触发器带所属表（缓存与属性面板都靠它）
    let triggers = database
        .list_triggers("main", Some("public"))
        .await
        .expect("触发器");
    assert_eq!(names(triggers.clone()), vec!["trg_orders_audit"]);
    assert_eq!(triggers[0].parent_name.as_deref(), Some("orders"));

    // 认不出的类别不进任何文件夹（`domain` 在靶子里有一份）
    assert!(
        tables.iter().all(|t| t.name != "order_state"),
        "domain 不该被画成表"
    );
}

/// 表与视图的分法就是导航的分法：调用方按 `kind == View` 切两半。
#[tokio::test]
async fn tables_and_views_split_by_kind() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.split").await;

    let objects = database
        .list_tables("main", Some("public"))
        .await
        .expect("表与视图");

    let (views, tables): (Vec<_>, Vec<_>) = objects
        .into_iter()
        .partition(|o| o.kind == SchemaObjectKind::View);
    assert_eq!(names(views), vec!["recent_orders", "mv_daily"], "物化视图也进视图文件夹");
    assert_eq!(names(tables), vec!["orders", "customers"]);

    // 另一个 schema 只有一张表（别把两个 schema 的结果混起来）
    assert_eq!(
        names(
            database
                .list_tables("main", Some("sales"))
                .await
                .expect("sales 的表")
        ),
        vec!["deals"]
    );
}

/// 列：原始类型给用户看，归一化类型给程序判（`extra.canonical`）。
#[tokio::test]
async fn columns_carry_raw_and_canonical_types() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.columns").await;

    let columns = database
        .list_columns("main", Some("public"), "orders")
        .await
        .expect("列");
    let name_of = |c: &engine::driver::ColumnDetail| c.name.clone();
    assert_eq!(
        columns.iter().map(name_of).collect::<Vec<_>>(),
        vec!["id", "customer_id", "amount", "created_at", "note"]
    );

    let id = &columns[0];
    assert!(id.is_primary_key && !id.nullable);
    assert_eq!(id.comment.as_deref(), Some("主键"));

    let amount = &columns[2];
    assert_eq!(amount.data_type, "numeric(38,10)", "属性面板看原始类型");
    assert_eq!(
        amount.extra.get("canonical").map(String::as_str),
        Some("DECIMAL"),
        "mock / 质量分看归一化类型（否则只认首词、最后全落文本）"
    );
    assert_eq!(
        amount.extra.get("format").map(String::as_str),
        Some("decimal(scale=10)")
    );

    // 视图也有列（点开视图要能展开）
    assert_eq!(
        database
            .list_columns("main", Some("public"), "recent_orders")
            .await
            .expect("视图的列")
            .len(),
        2
    );
}

/// 没给 schema：落到 `driver.describe` 的 `default_schema`（不在宿主侧猜一个）。
#[tokio::test]
async fn a_missing_schema_falls_back_to_the_descriptors_default() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.default-schema").await;
    // 靶子的 `default_schema` 就是 `public`
    assert_eq!(
        names(
            database
                .list_tables("main", None)
                .await
                .expect("不给 schema 也要能查")
        ),
        vec!["orders", "customers", "recent_orders", "mv_daily"]
    );
}

/// 例程源码：只对例程问；别的东西**不白跑一趟 RPC**。
#[tokio::test]
async fn routine_source_only_answers_for_routines() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.routine").await;

    let source = database
        .get_routine_source(
            "main",
            Some("public"),
            "order_total",
            SchemaObjectKind::Function,
        )
        .await
        .expect("例程源码")
        .expect("靶子有这个函数");
    assert!(source.starts_with("CREATE FUNCTION order_total"), "{source}");

    // 表 / 视图没有源码：返回 None，且不去问对端
    let none = database
        .get_routine_source("main", Some("public"), "orders", SchemaObjectKind::Table)
        .await
        .expect("表不该报错");
    assert_eq!(none, None);
}

/// 索引 / 约束明细：**如实报不支持**，不返回空（「没有」与「不支持」在界面上不同）。
#[tokio::test]
async fn index_and_constraint_details_are_honestly_unsupported() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.indexes").await;

    for error in [
        database
            .list_indexes("main", Some("public"), "orders")
            .await
            .expect_err("索引明细应当报不支持"),
        database
            .list_constraints("main", Some("public"), "orders")
            .await
            .expect_err("约束明细应当报不支持"),
    ] {
        match error {
            CoreError::Common(CommonError::NotSupported(reason)) => {
                assert!(reason.contains("明细"), "{reason}");
                assert!(reason.contains("fixture"), "要说得清是哪个驱动：{reason}");
            }
            other => panic!("应当是 NotSupported：{other:?}"),
        }
    }
}

/// 会话没了：进连接域，而不是「元数据是空的」。
#[tokio::test]
async fn a_dead_session_is_a_connection_error_not_empty_metadata() {
    let (mut supervisor, session_id, database) = database_for("test.dbmeta.dead").await;
    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
    assert!(supervisor.session_conn(&session_id).is_none());

    match database.list_catalogs().await.expect_err("会话收掉了") {
        CoreError::Connection(ConnectionError::Network { reason, .. }) => {
            assert!(reason.contains("收掉"), "{reason}");
        }
        other => panic!("应当是连接域错误：{other:?}"),
    }

    // 查询那条路同样如此（不给一个"空结果"让人以为查成功了）
    assert!(database.query("select 1").await.is_err());
}

/// 取消令牌那条路不受元数据面影响（同一把会话上，两种调用互不干扰）。
#[tokio::test]
async fn metadata_calls_and_queries_share_the_session() {
    let (_supervisor, _session, database) = database_for("test.dbmeta.share").await;

    database
        .list_catalogs()
        .await
        .expect("元数据调用");
    let token = CancellationToken::new();
    let result = database
        .query_with_cancel("select rows=1", token)
        .await
        .expect("查询应当成功");
    assert_eq!(result.total_rows(), 1);
    // 元数据照旧能用
    database.list_catalogs().await.expect("元数据调用");
}
