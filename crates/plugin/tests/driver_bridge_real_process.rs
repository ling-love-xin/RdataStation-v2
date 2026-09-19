//! 驱动桥的真实进程端到端测试（P1 验收清单里「3000 行 Arrow 到宿主」的那一半）
//!
//! 靶子同前（`tests/fixture/sidecar.rs`，**独立实现**一遍帧编解码、并用 arrow crate 真的
//! 编出 IPC 流）。这里考察的是**线格式与承载方式**：
//!
//! - 小结果走内联 JSON、大结果走 Arrow 附件 —— 而**消费方看不出区别**（承载方式只是线格式）
//! - 3000 行过进程边界后逐值对得上、schema metadata（`rds.*`）也在
//! - 取消真的能停掉在跑的查询（对端回 `-32004`，宿主翻成 `DriverError::Cancelled`）
//! - SQL 错带得出 `sqlstate`（UI 要能把它与「驱动不支持」分开呈现）
//!
//! ⚠️ 这里**不连真库**：`sql` 是靶子脚本化的。真库那一步要 PostgreSQL 在场，属实机验收。

use std::time::{Duration, Instant};

use serde_json::json;

use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::driver::{DriverError, QueryRequest, SessionDriver};
use rds_plugin::sidecar::lifecycle::{Concurrency, ProcessSpec};
use rds_plugin::sidecar::supervisor::{Deployment, SessionOpened, SidecarSupervisor};

/// 起一个靶子实例并开一条会话，返回（supervisor, 会话 id）。
async fn session_for(plugin_id: &str) -> (SidecarSupervisor, String) {
    session_for_with(plugin_id, &[]).await
}

/// 同上，但给靶子带命令行参数（如 `--force-inline`）。
async fn session_for_with(plugin_id: &str, args: &[&str]) -> (SidecarSupervisor, String) {
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

    let now = Instant::now();
    let opened = supervisor
        .open_session(plugin_id, "fixture", "s1", json!({}), now)
        .await
        .expect("开会话应当成功");
    assert!(matches!(opened, SessionOpened::Live { .. }), "{opened:?}");
    (supervisor, "s1".to_string())
}

#[tokio::test]
async fn describes_the_driver() {
    let (supervisor, session_id) = session_for("test.bridge.describe").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let descriptor = driver.describe("fixture").await.expect("描述应当成功");
    assert_eq!(descriptor.display_name, "Fixture Driver");
    assert_eq!(descriptor.server_version.as_deref(), Some("fixture-1"));
    assert!(descriptor.supports("cancel"));
    assert!(!descriptor.supports("streaming"), "没声明的能力按不支持");
    assert_eq!(descriptor.identifier_quote.as_deref(), Some("\""));
    assert_eq!(descriptor.default_schema.as_deref(), Some("public"));

    // 会话级探活（进程级探活是 supervisor 的 `ping()`）
    driver.session_ping().await.expect("会话应当活着");
}

/// 小结果走内联 JSON：**不**带附件，行直接给出来。
#[tokio::test]
async fn a_small_result_comes_back_inline() {
    let (supervisor, session_id) = session_for("test.bridge.inline").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let page = driver
        .execute(&QueryRequest::new("req-inline", "select rows=10"))
        .await
        .expect("查询应当成功");

    assert_eq!(page.columns.len(), 5);
    assert_eq!(page.columns[0].name, "id");
    assert_eq!(page.columns[2].type_raw, "numeric(38,10)");
    let rows = page.data.rows().expect("小结果应当内联");
    assert_eq!(rows.len(), 10);
    assert_eq!(rows[0]["name"], json!("row-1"));
    assert_eq!(page.row_count, 10);
    assert!(!page.has_more);
    assert!(
        page.notices.iter().any(|n| n.contains("select rows=10")),
        "SQL 应当原样过边界：{:?}",
        page.notices
    );
}

/// **3000 行过进程边界**（P1 验收的那一条）：走 Arrow 附件，逐值对得上，
/// schema metadata 也在（类型映射的落点，§4.5.1）。
#[tokio::test]
async fn three_thousand_rows_come_back_as_arrow() {
    let (supervisor, session_id) = session_for("test.bridge.arrow").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let page = driver
        .execute(&QueryRequest::new("req-arrow", "select rows=3000"))
        .await
        .expect("查询应当成功");

    assert_eq!(page.row_count, 3000, "对端声称的行数");
    let batches = page.data.batches().expect("大结果应当走 Arrow");
    assert_eq!(page.rows_here(), 3000, "实际载荷的行数");

    let schema = batches[0].schema();
    let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    assert_eq!(names, vec!["id", "name", "amount", "flag", "ts"]);

    // `rds.*` metadata（类型映射的落点）：两份 `type_raw` 必须一致（口径见 driver.rs 模块文档）
    let amount = schema.field_with_name("amount").unwrap();
    assert_eq!(amount.metadata()["rds.canonical"], "DECIMAL");
    assert_eq!(amount.metadata()["rds.type_raw"], "numeric(38,10)");
    assert_eq!(amount.metadata()["rds.format"], "decimal(scale=10)");
    let id_field = schema.field_with_name("id").unwrap();
    assert_eq!(id_field.metadata()["rds.is_pk"], "true");
    assert_eq!(id_field.metadata()["rds.nullable"], "false");
    // 时间类型按 §4.2.4 归一化到 Timestamp(us, UTC)
    assert_eq!(
        schema.field_with_name("ts").unwrap().data_type(),
        &arrow::datatypes::DataType::Timestamp(
            arrow::datatypes::TimeUnit::Microsecond,
            Some("UTC".into())
        )
    );

    // 逐值抽查（首行 / 末行 / 总量）
    use arrow::array::{Array, Float64Array, Int64Array};
    let ids = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("id 应当是 Int64");
    assert_eq!(ids.value(0), 1);
    assert_eq!(ids.value(2999), 3000);

    let amounts = batches[0]
        .column(2)
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("amount 应当是 Float64");
    assert_eq!(amounts.value(0), 1.5);
    assert_eq!(amounts.value(2999), 4500.0);

    let sum: i64 = batches.iter().map(|b| b.num_rows() as i64).sum();
    assert_eq!(sum, 3000);
}

/// **承载方式对消费方不可见**：让对端走内联那条路，结果的行数与列应当一模一样。
#[tokio::test]
async fn a_forced_inline_result_is_the_same_page() {
    let (supervisor, session_id) =
        session_for_with("test.bridge.forceinline", &["--force-inline"]).await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let page = driver
        .execute(&QueryRequest::new("req-forced", "select rows=3000"))
        .await
        .expect("查询应当成功");

    assert!(page.data.batches().is_none(), "这条路应当没有 Arrow 附件");
    assert_eq!(page.rows_here(), 3000);
    assert_eq!(page.row_count, 3000);
    assert_eq!(page.columns.len(), 5);
    assert_eq!(page.columns[0].name, "id");
    // 内联那条路的时间是 RFC3339 字符串（Arrow 侧才是 Timestamp(us, UTC)）
    let rows = page.data.rows().expect("应当是内联行");
    assert_eq!(rows[2999]["amount"], json!(4500.0));
}

/// 取消：宿主把取消递过去，被取消的那次查询以 `-32004` 收场。
#[tokio::test]
async fn cancelling_stops_a_slow_query() {
    let (supervisor, session_id) = session_for("test.bridge.cancel").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let request = QueryRequest::new("req-slow", "select hold_ms=5000");
    let started = Instant::now();

    let (result, ()) = tokio::join!(driver.execute(&request), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        driver
            .cancel(&request.request_id)
            .await
            .expect("取消调用本身应当成功");
    });

    let error = result.expect_err("被取消的查询不该成功");
    assert!(error.is_cancelled(), "{error:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "取消应当立刻生效，而不是等它自己跑完：{:?}",
        started.elapsed()
    );
}

/// SQL 错要带得出 `sqlstate` / `position`（UI 靠它定位错误，`capability_denied` 也靠码分流）。
#[tokio::test]
async fn sql_errors_carry_the_sqlstate() {
    let (supervisor, session_id) = session_for("test.bridge.sqlerr").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let error = driver
        .execute(&QueryRequest::new("req-fail", "select fail"))
        .await
        .expect_err("这条 SQL 应当失败");
    assert!(error.is_sql_error(), "{error:?}");
    assert!(!error.is_cancelled() && !error.is_capability_denied());

    match &error {
        DriverError::Rpc { data, .. } => {
            let data = data.as_ref().expect("SQL 错应当带 data");
            assert_eq!(data["sqlstate"], json!("42P01"));
            assert_eq!(data["position"], json!(15));
        }
        other => panic!("期望 Rpc，得到 {other:?}"),
    }
    assert!(error.to_string().contains("42P01"), "{error}");
}
