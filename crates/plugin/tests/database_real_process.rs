//! `SidecarDatabase`（接引擎 `Database` trait 的那一层）的真实进程端到端测试
//!
//! 靶子同前。这里考察的是**接上引擎之后的样子**：
//!
//! - `query` 拿到的就是既有驱动那样的 `QueryResult`：**只填 `batches`**（架构 §12 #21），
//!   而且两条线格式（内联 JSON / Arrow 附件）在消费方看来没有区别
//! - `to_rows()`（前端那条路）能走通，值也对
//! - `query_with_cancel` 走**真取消**：令牌一响就把 `query.cancel` 递过去，对端回 `-32004`
//! - 错误进对错误域：SQL 错进 `DatabaseError::Query`（带光标位置）、缺能力进 `NotSupported`、
//!   事务未接线**明确报不支持**而不是静默成功
//! - `meta()` 说得出「支持 Arrow」——这一项与原生驱动恰好相反，是本方案的价值所在

use std::time::{Duration, Instant};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use engine::driver::Database;
use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::driver::{SessionDriver, SidecarDatabase};
use rds_plugin::sidecar::lifecycle::{Concurrency, ProcessSpec};
use rds_plugin::sidecar::supervisor::{Deployment, SessionOpened, SidecarSupervisor};
use shared::error::{CommonError, CoreError, DatabaseError};

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

#[tokio::test]
async fn a_big_result_arrives_as_batches() {
    let (_supervisor, _session, database) = database_for("test.database.arrow").await;

    let result = database
        .query("select rows=3000")
        .await
        .expect("查询应当成功");

    // 驱动契约（架构 §12 #21）：只填 batches，`rows` 字段留空
    assert!(result.rows.is_empty(), "驱动不该填 rows 字段");
    assert_eq!(result.batches.len(), 1);
    assert_eq!(result.total_rows(), 3000);
    assert_eq!(result.total_rows, 3000, "字段也应当是真实值");
    assert_eq!(
        result.columns,
        vec!["id", "name", "amount", "flag", "ts"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        result.column_types[2], "numeric(38,10)",
        "类型映射的线格式交代"
    );
    assert_eq!(result.is_read_only, Some(true), "SELECT 是只读的");

    // 前端那条路（`to_rows`）走得通，且值对得上
    let rows = result.to_rows();
    assert_eq!(rows.len(), 3000);
    assert_eq!(rows[0][0].to_string(), "1");
    assert_eq!(rows[2999][0].to_string(), "3000");
    assert_eq!(rows[2999][2].to_string(), "4500");
}

/// 内联那条线格式也一样进 `batches`（承载方式对消费方不可见）。
#[tokio::test]
async fn an_inline_result_also_arrives_as_batches() {
    let (_supervisor, _session, database) = database_for("test.database.inline").await;

    let result = database.query("select rows=5").await.expect("查询应当成功");
    assert_eq!(result.total_rows(), 5);
    assert_eq!(result.batches.len(), 1);
    assert_eq!(result.columns[0], "id");

    let rows = result.to_rows();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0][1].to_string(), "row-1");
    // 内联那条路是按值形状推断的（Arrow 那条路才带保真类型）
    use arrow::datatypes::DataType;
    assert_eq!(
        result.batches[0].schema().field(1).data_type(),
        &DataType::LargeUtf8
    );
}

/// **真取消**：令牌响 → `query.cancel` 递过去 → 对端 `-32004` → 引擎拿到"已取消"。
#[tokio::test]
async fn cancelling_reaches_the_driver_process() {
    let (_supervisor, _session, database) = database_for("test.database.cancel").await;

    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        trigger.cancel();
    });

    let started = Instant::now();
    let error = database
        .query_with_cancel("select hold_ms=5000", token)
        .await
        .expect_err("被取消的查询不该成功");

    match &error {
        CoreError::Database(DatabaseError::Query { reason, .. }) => {
            assert!(reason.contains("取消"), "{reason}");
        }
        other => panic!("取消应当进 DatabaseError::Query（与原生驱动同口径）：{other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "取消要立刻生效，而不是等它跑完：{:?}",
        started.elapsed()
    );
}

/// SQL 错进 `DatabaseError::Query`，并把 1 基字符位置换算成字节偏移（B6 的光标位置）。
#[tokio::test]
async fn sql_errors_land_in_the_query_domain_with_a_position() {
    let (_supervisor, _session, database) = database_for("test.database.sqlerr").await;

    // 长度够长，位置才在范围内（靶子固定报 position=15）
    let error = database
        .query("select fail from somewhere")
        .await
        .expect_err("这条 SQL 应当失败");
    match error {
        CoreError::Database(DatabaseError::Query {
            position, reason, ..
        }) => {
            assert!(reason.contains("SQL 出错"), "{reason}");
            assert_eq!(position, Some(14), "1 基字符 15 → 0 基字节 14");
        }
        other => panic!("{other:?}"),
    }
}

/// 事务桥没接：**明确报不支持**（不是静默当成"没有事务"跑过去）。
#[tokio::test]
async fn transactions_report_unsupported_instead_of_pretending() {
    let (_supervisor, _session, database) = database_for("test.database.tx").await;

    // 不用 `expect_err`：`Box<dyn Transaction>` 没有 Debug（错误分支照样拿得到）
    let error = match database.begin_transaction().await {
        Ok(_) => panic!("事务桥还没接，不该成功"),
        Err(error) => error,
    };
    assert!(
        matches!(error, CoreError::Common(CommonError::NotSupported(_))),
        "{error:?}"
    );
    assert!(error.to_string().contains("P2"), "{error}");
}

#[tokio::test]
async fn meta_and_ping_describe_the_driver() {
    let (_supervisor, _session, database) = database_for("test.database.meta").await;

    let meta = database.meta();
    assert!(meta.supports_arrow, "数据面就是 Arrow —— 与原生驱动相反");
    assert!(meta.supports_transaction, "靶子声明了 transactions");
    assert!(!meta.supports_streaming, "没声明的能力按不支持");
    assert!(!meta.supports_federated, "联邦是宿主 DuckDB 的事");
    assert_eq!(meta.server_version.as_deref(), Some("fixture-1"));

    database.ping().await.expect("会话应当活着");
}

/// 会话被收掉之后，句柄**明确报失效**（弱引用口径：不抱着陈旧进程不放）。
#[tokio::test]
async fn a_retired_session_reports_an_unusable_connection() {
    let (mut supervisor, session_id, database) = database_for("test.database.retired").await;
    database.query("select rows=1").await.expect("先确认能用");

    supervisor
        .close_session(&session_id, Instant::now())
        .await
        .unwrap();
    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;

    let error = database
        .query("select rows=1")
        .await
        .expect_err("会话已经收掉了");
    assert!(
        matches!(error, CoreError::Connection(_)),
        "连接不可用应当进连接域：{error:?}"
    );
}
