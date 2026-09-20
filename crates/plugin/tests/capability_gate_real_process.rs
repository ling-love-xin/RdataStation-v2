//! 能力门控的真实进程测试（P2：**不得静默退化**）
//!
//! 驱动在 `driver.describe` 里报的能力是**运行时**的那一份（清单可以撒谎，跑起来的进程没法撒谎）。
//! 这里考察的是宿主拿它做门控的样子：
//!
//! - 没声明 `cancel`：用户点「中断」时要**如实说没停下来**，而不是显示成「已中断」而语句还在跑
//! - `transactions`：驱动没有这个能力，与宿主桥还没接，是两件不同的事（两种下一步）
//! - `schemas`：决定导航要不要多一层 schema（`has_schema_level`）——单层库不该长出空的一层
//! - 浏览器面（`MetadataBrowser`）与驱动面（`Database`）是**同一份实现**（两个面不许各答各的）
//!
//! ⚠️ 这里**不连真库**：能力开关是靶子的命令行参数（`--cap-na=<键>`）。

use std::time::{Duration, Instant};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use engine::driver::{ColumnDetail, Database, SchemaObjectKind};
use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::driver::{SessionDriver, SidecarDatabase};
use rds_plugin::sidecar::lifecycle::{Concurrency, ProcessSpec};
use rds_plugin::sidecar::supervisor::{Deployment, SessionOpened, SidecarSupervisor};
use shared::error::{CommonError, CoreError};

/// 靶子 + 会话 + 接上引擎的驱动。
///
/// supervisor 必须**一直活着**：会话（以及那个进程）归它管，句柄一丢连接就作废。
struct App {
    _supervisor: SidecarSupervisor,
    database: SidecarDatabase,
}

/// 起靶子（带命令行参数）、开会话。
async fn app_for(plugin_id: &str, args: &[&str]) -> App {
    let mut supervisor = SidecarSupervisor::new();
    supervisor
        .deploy(Deployment {
            plugin_id: plugin_id.to_string(),
            spec: ProcessSpec::new(["fixture"], 1, Concurrency::Serial),
            command: BackendCommand {
                program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture")),
                args: args.iter().map(|a| (*a).to_string()).collect(),
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
    App {
        _supervisor: supervisor,
        database: SidecarDatabase::new(&conn, "s1", "fixture", descriptor),
    }
}

fn not_supported(error: CoreError, what: &str) -> String {
    match error {
        CoreError::Common(CommonError::NotSupported(reason)) => reason,
        other => panic!("{what} 应当报 NotSupported：{other:?}"),
    }
}

fn column_names(columns: &[ColumnDetail]) -> Vec<String> {
    columns.iter().map(|c| c.name.clone()).collect()
}

/// 在 `after` 毫秒后放令牌。
fn cancel_after(token: &CancellationToken, after_ms: u64) -> tokio::task::JoinHandle<()> {
    let token = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(after_ms)).await;
        token.cancel();
    })
}

/// 没声明 `cancel`：中断请求要**如实说没停下来**，而且不把那条语句撇在一边。
#[tokio::test]
async fn a_driver_that_cannot_cancel_says_so_instead_of_pretending() {
    let app = app_for("test.gate.cancel", &["--cap-na=cancel"]).await;
    let token = CancellationToken::new();
    let fire = cancel_after(&token, 50);

    let started = Instant::now();
    let Err(error) = app.database.query_with_cancel("select hold_ms=300", token).await else {
        panic!("没声明 cancel 的驱动不该把「中断」当成功");
    };
    let reason = not_supported(error, "不能中断");
    assert!(reason.contains("fixture"), "要说得清是哪个驱动：{reason}");
    assert!(reason.contains("没能停下来"), "{reason}");

    // 等了整条语句跑完才报：**没有**把它撇在一边（那样库里会多一条没人管的语句）
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "应当等它自己收场，实际 {:?}",
        started.elapsed()
    );
    fire.await.expect("放令牌的任务");
}

/// 声明了 `cancel` 的驱动照旧走真取消（门控不该把好路径也关掉）。
#[tokio::test]
async fn a_driver_that_can_cancel_still_cancels() {
    let app = app_for("test.gate.cancel-ok", &[]).await;
    let token = CancellationToken::new();
    let fire = cancel_after(&token, 50);

    let started = Instant::now();
    let Err(error) = app
        .database
        .query_with_cancel("select hold_ms=3000", token)
        .await
    else {
        panic!("被取消的查询不该成功");
    };
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "真取消该立刻收场，实际 {:?}",
        started.elapsed()
    );
    // 取消走 `-32004` → SQL 域（UI 按 SQL 域呈现「已中断」）
    assert!(
        matches!(error, CoreError::Database(_)),
        "取消应当进 SQL 域：{error:?}"
    );
    fire.await.expect("放令牌的任务");
}

/// `transactions`：驱动没这个能力 vs 宿主桥还没接 —— 两件不同的事，两种下一步。
#[tokio::test]
async fn transactions_distinguish_not_declared_from_not_wired() {
    let declared = app_for("test.gate.tx-yes", &[]).await;
    let Err(error) = declared.database.begin_transaction().await else {
        panic!("事务桥还没接，不该假装开出来了");
    };
    let reason = not_supported(error, "事务");
    assert!(reason.contains("桥还没接"), "{reason}");
    assert!(reason.contains("没有真的开事务"), "{reason}");

    let not_declared = app_for("test.gate.tx-no", &["--cap-na=transactions"]).await;
    let Err(error) = not_declared.database.begin_transaction().await else {
        panic!("没声明 transactions 就不该开事务");
    };
    let reason = not_supported(error, "事务");
    assert!(reason.contains("没声明 transactions 能力"), "{reason}");
}

/// `schemas`：决定导航给不给 schema 层；单层库**不**回退成 catalog 列表（那样会出现重复层）。
#[tokio::test]
async fn the_schema_layer_follows_the_declaration() {
    let with_schemas = app_for("test.gate.schema-yes", &[]).await;
    let browser = with_schemas
        .database
        .as_metadata_browser()
        .expect("sidecar 驱动要给浏览器面");
    assert!(browser.has_schema_level(), "靶子声明了 schemas");
    assert_eq!(
        browser.get_schemas("main").await.expect("schema 清单").len(),
        2
    );

    let without = app_for("test.gate.schema-no", &["--cap-na=schemas"]).await;
    let browser = without
        .database
        .as_metadata_browser()
        .expect("浏览器面与能力无关");
    assert!(!browser.has_schema_level(), "没声明 schemas 就没有独立 schema 层");
    assert!(
        browser
            .get_schemas("main")
            .await
            .expect("get_schemas 不该报错")
            .is_empty(),
        "没有 schema 层要返回空，不能回退成 catalog 列表"
    );
    // 但第一层照旧有（导航不能是空的）
    assert_eq!(browser.get_catalogs().await.expect("catalog 清单").len(), 1);
}

/// 两个面同一份实现：`MetadataBrowser` 与 `Database` 不许各答各的。
#[tokio::test]
async fn the_browser_face_and_the_database_face_agree() {
    let app = app_for("test.gate.faces", &[]).await;
    let browser = app.database.as_metadata_browser().expect("浏览器面");

    let from_browser: Vec<String> = browser
        .get_tables("main", "public")
        .await
        .expect("浏览器面取表")
        .into_iter()
        .map(|n| n.name)
        .collect();
    let from_database: Vec<String> = app
        .database
        .list_tables("main", Some("public"))
        .await
        .expect("驱动面取表")
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert_eq!(from_browser, from_database);
    assert_eq!(
        from_browser,
        vec!["orders", "customers", "recent_orders", "mv_daily"]
    );

    // 详情：节点类别 + 列 + 索引数 + 行数估计
    let detail = browser
        .get_table_detail("main", "public", "orders")
        .await
        .expect("详情");
    assert_eq!(detail.node.name, "orders");
    assert_eq!(detail.node.kind, SchemaObjectKind::Table);
    assert_eq!(detail.columns.len(), 5);
    assert_eq!(detail.index_count, Some(3));
    assert_eq!(detail.row_count_estimate, Some(12000));

    let columns = app
        .database
        .list_columns("main", Some("public"), "orders")
        .await
        .expect("驱动面取列");
    assert_eq!(
        column_names(&detail.columns),
        column_names(&columns),
        "两个面的列要一致"
    );
    assert_eq!(
        detail.columns[2].data_type, columns[2].data_type,
        "归一化类型也不例外"
    );

    // 视图：详情说得清它是视图（属性面板按类别选表单）
    let view = browser
        .get_table_detail("main", "public", "recent_orders")
        .await
        .expect("视图详情");
    assert_eq!(view.node.kind, SchemaObjectKind::View);

    // 序列 / 触发器：两个面同一个答案（触发器还带所属表）
    assert_eq!(
        browser
            .get_sequences("main", "public")
            .await
            .expect("序列")
            .len(),
        1
    );
    let triggers = browser.get_triggers("main", "public").await.expect("触发器");
    assert_eq!(triggers.len(), 1);
    assert_eq!(triggers[0].parent_name.as_deref(), Some("orders"));

    // 索引 / 约束明细：两个面都如实报不支持
    for error in [
        browser
            .get_indexes("main", "public", "orders")
            .await
            .expect_err("索引明细"),
        browser
            .get_constraints("main", "public", "orders")
            .await
            .expect_err("约束明细"),
    ] {
        assert!(not_supported(error, "明细").contains("明细"));
    }
}
