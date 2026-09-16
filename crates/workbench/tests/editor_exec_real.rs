//! 编辑器执行端口的真机验证（A14）
//!
//! 链路：`EditorShared::submit` → 编辑器执行通道（工作线程）→ `EngineQueryRunner` →
//! `SqlService` → 真驱动 → `QueryResult::batches` → 结果集（编辑器侧）。
//!
//! 需要环境变量（未设的库自动跳过，不算失败）：
//!
//! ```text
//! $env:RDS_TEST_MYSQL_URL="mysql://root:root@192.168.3.138:3306/mysql"
//! $env:RDS_TEST_PG_URL="postgres://postgres:postgresql@192.168.3.138:5432/postgres"
//! $env:RDS_TEST_SQLITE_PATH="D:\FossilT\T.fossil"
//! $env:RDS_TEST_DUCKDB_PATH="D:\data\123"
//! ```
//!
//! 与 `engine/tests/transaction_affinity.rs` 同一套环境变量与建连方式（探针口径一致）。
//! 执行走的是"当前活动连接"，所以这里先 `set_active_connection` 再提交。

use std::sync::Arc;
use std::time::{Duration, Instant};

use editor::execution::{ExecTarget, ResultPlacement, batch_target};
use editor::model::{DocumentId, EditorMode};
use editor::service::OpenRequest;
use editor::shared::EditorShared;
use engine::connection_manager::ConnectionManager;
use engine::{AutoDriverRegistrar, DriverConnectionConfig};

/// 一个库的探测目标
struct Target {
    driver: &'static str,
    env: &'static str,
    /// 文件型库走 `file_path`，网络库走 `url_override`
    file: bool,
    /// 一句“够久”的查询（B3：超时要一句跑得比超时长的查询；中断要一句跑得比手速长的）
    ///
    /// 刻意选**不物化大结果**的写法（聚合 / count）：被放弃的查询还会在后台跑完，
    /// 不能让它一下吃掉几 GB 内存。
    slow_sql: &'static str,
}

const TARGETS: [Target; 4] = [
    Target {
        driver: "mysql",
        env: "RDS_TEST_MYSQL_URL",
        file: false,
        slow_sql: "SELECT SLEEP(3)",
    },
    Target {
        driver: "postgres",
        env: "RDS_TEST_PG_URL",
        file: false,
        slow_sql: "SELECT pg_sleep(3)",
    },
    Target {
        driver: "sqlite",
        env: "RDS_TEST_SQLITE_PATH",
        file: true,
        // 递归 CTE：只计数，行是流式产生的（不会把几百万行攒在内存里）；
        // 量级取“两秒左右”：要跑得比 1s 超时长，又不能长到拖住后续重连
        slow_sql: "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 5000000) SELECT count(*) FROM c",
    },
    Target {
        driver: "duckdb",
        env: "RDS_TEST_DUCKDB_PATH",
        file: true,
        // 交叉连接求和：聚合流式，~4e8 次乘法约一秒到两秒
        slow_sql: "SELECT sum(a.range::BIGINT * b.range::BIGINT) FROM range(20000) a, range(20000) b",
    },
];

/// 建连并设为活动连接（返回 conn_id；失败返回 None 并打印原因）
async fn connect_active(
    manager: &Arc<ConnectionManager>,
    target: &Target,
    value: &str,
) -> Option<String> {
    connect_active_with(manager, target, value, None).await
}

/// 建连（**带重试**）：B3 的两个案例都会放弃一句还在跑的查询，而 sqlite / duckdb 的取消
/// 只是断掉等待——驱动侧的任务要跑完才撒手（连接与库文件因此被占住好几秒）。这是引擎侧现状，
/// 不是编辑器的问题，所以这里重试而不是当成失败。
fn connect_with_retry(
    runtime: &tokio::runtime::Runtime,
    manager: &Arc<ConnectionManager>,
    target: &Target,
    value: &str,
    query_timeout_secs: Option<u32>,
) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(id) = runtime.block_on(connect_active_with(
            manager,
            target,
            value,
            query_timeout_secs,
        )) {
            return Some(id);
        }
        if Instant::now() > deadline {
            return None;
        }
        eprintln!(
            "⏳ {}：连接暂时不可用（上一次被放弃的查询还占着），1s 后重试",
            target.driver
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// B3 用的库值：文件型库给一个**临时库文件**（非文件型库原样用 URL）
fn b3_value(target: &Target, value: &str, tag: &str) -> String {
    if !target.file {
        return value.to_string();
    }
    let path = std::env::temp_dir().join(format!(
        "rds_b3_{}_{}_{tag}.db",
        target.driver,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path.to_string_lossy().to_string()
}

/// 同上，但可以给连接配一个查询超时（秒；B3 要一个“1 秒就到点”的连接）
async fn connect_active_with(
    manager: &Arc<ConnectionManager>,
    target: &Target,
    value: &str,
    query_timeout_secs: Option<u32>,
) -> Option<String> {
    let mut config = DriverConnectionConfig::new(target.driver);
    config.name = Some(format!("A14 编辑器执行探针（{}）", target.driver));
    config.query_timeout = query_timeout_secs;
    if target.file {
        config.file_path = Some(value.to_string());
    } else {
        config.url_override = Some(value.to_string());
    }

    match manager.create_connection_with_registry(config).await {
        Ok((conn_id, _db)) => {
            manager.set_active_connection(conn_id.clone()).await;
            Some(conn_id)
        }
        Err(error) => {
            eprintln!("❌ {}：建连失败 —— {error}", target.driver);
            None
        }
    }
}

/// 跑一次执行（走编辑器的执行通道），把**这次提交**带来的每一份结果都取回落库
///
/// `expected` = 预期结论条数（单条 = 1；批量 = 语句数）。返回本次的每份结果。
fn run_through_editor(
    shared: &EditorShared,
    document: DocumentId,
    target: &ExecTarget,
    placement: ResultPlacement,
    expected: usize,
) -> Vec<editor::store::ResultEntry> {
    shared
        .submit(document, target, placement)
        .expect("提交执行");
    collect_outcomes(shared, expected)
}

/// 等回填（生产由面板的轮询泵做；这里手动轮询），落位用结论自带的
fn collect_outcomes(shared: &EditorShared, expected: usize) -> Vec<editor::store::ResultEntry> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut entries = Vec::new();
    while entries.len() < expected {
        for outcome in shared.drain_exec() {
            let entry = match outcome.result {
                Ok(data) => editor::store::ResultEntry::success(
                    outcome.document,
                    outcome.sql,
                    data.elapsed_ms,
                    data.truncated,
                    data.columns,
                    data.rows,
                ),
                Err(error) => {
                    editor::store::ResultEntry::failure(outcome.document, outcome.sql, error, 0)
                }
            };
            // 落位来自结论自己（"结果放哪"是执行时的语义，不是调用方事后猜的）
            shared.update_results(|store| store.push(entry.clone(), outcome.placement));
            entries.push(entry);
        }
        if entries.len() >= expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "只等回 {} 份结果（预期 {expected} 份），30s 超时",
            entries.len()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    entries
}

#[test]
fn the_editor_execution_port_runs_a_query_on_every_configured_database() {
    AutoDriverRegistrar::register_builtin_drivers();

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    // 生产路径用的是**全局连接管理器**，测试也用它：执行器从活动连接出发找库
    let manager = engine::connection_manager::get_connection_manager().clone();

    let mut checked = 0;
    for target in TARGETS {
        let Ok(value) = std::env::var(target.env) else {
            eprintln!("⏭️  {}：未设 {} ，跳过", target.driver, target.env);
            continue;
        };
        checked += 1;

        // 每次换一个活动连接：编辑器执行走"当前活动连接"
        let Some(_conn_id) = runtime.block_on(connect_active(&manager, &target, &value)) else {
            panic!("{} 已配置环境变量但建连失败", target.driver);
        };

        // 走**生产路径**：`editor_exec::attach` 把引擎执行器接到编辑器上
        let shared = EditorShared::new();
        rds_workbench::services::editor_exec::attach(&shared);

        let document = shared
            .open(OpenRequest::untitled("select 1 as n", EditorMode::Sql))
            .id()
            .clone();

        let entries = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Statement("select 1 as n".to_string()),
            ResultPlacement::Replace,
            1,
        );
        let entry = entries.into_iter().next().expect("有结果回填");
        assert!(
            entry.error.is_none(),
            "{}：执行报错 —— {:?}",
            target.driver,
            entry.error
        );
        assert!(!entry.columns.is_empty(), "{}：应当有列名", target.driver);
        assert_eq!(entry.row_count(), 1, "{}：select 1 应当一行", target.driver);
        eprintln!(
            "✅ {}：{}（列 = {:?}，值 = {:?}）",
            target.driver,
            entry.summary(),
            entry.columns,
            entry.rows
        );

        // B2：批量（逐条独立）——每句一个结果集，失败不中断
        let script = "select 1 as n;\nselect 2 as n;\nselect 3 as n;";
        let batch = run_through_editor(
            &shared,
            document.clone(),
            &batch_target(script),
            ResultPlacement::NewSet,
            3,
        );
        assert_eq!(batch.len(), 3, "{}：批量应当三份结果", target.driver);
        assert!(
            batch.iter().all(|entry| entry.error.is_none()),
            "{}：三条简单查询不该失败 —— {:?}",
            target.driver,
            batch.iter().map(|entry| &entry.error).collect::<Vec<_>>()
        );
        assert_eq!(
            batch.iter().map(|entry| entry.row_count()).collect::<Vec<_>>(),
            vec![1, 1, 1],
            "{}：每句各自一行",
            target.driver
        );
        assert_eq!(
            shared.results().set_count(&document),
            4,
            "{}：一份替换结果 + 三份批量结果 = 4 个结果集",
            target.driver
        );
        eprintln!(
            "✅ {}：批量三句 → 三个结果集（{}）",
            target.driver,
            batch
                .iter()
                .map(|entry| format!("{} 行", entry.row_count()))
                .collect::<Vec<_>>()
                .join(" / ")
        );

        // B3 的库值：文件型库换成**临时库文件**（每例一个）——被放弃的查询会占着库文件，
        // 而两个案例要接连重连；临时文件让它们互不干扰（且在 temp 目录，用完就丢）。
        let cancel_value = b3_value(&target, &value, "cancel");
        let timeout_value = b3_value(&target, &value, "timeout");

        // B3-a：中断——慢查询跑到一半按中断，应当很快回一条取消错误
        runtime.block_on(manager.close_all_connections());
        let Some(_cancel_conn) =
            connect_with_retry(&runtime, &manager, &target, &cancel_value, None)
        else {
            panic!("{} 建连接失败", target.driver);
        };
        shared
            .submit(
                document.clone(),
                &ExecTarget::Statement(target.slow_sql.to_string()),
                ResultPlacement::Replace,
            )
            .expect("提交慢查询");
        // 等它真的开始跑（取消要中断的是“跑着的查询”，不是“刚提交”那个瞬间）
        std::thread::sleep(Duration::from_millis(400));
        let cancel_started = Instant::now();
        shared.cancel().expect("中断应当被接受");
        let cancelled = collect_outcomes(&shared, 1);
        let cancel_waited = cancel_started.elapsed();
        let error = cancelled[0]
            .error
            .clone()
            .unwrap_or_else(|| format!("{}：这句没被中断", target.driver));
        assert!(
            error.to_lowercase().contains("cancel"),
            "{}：中断要如实报错，实得 —— {error}",
            target.driver
        );
        assert!(
            cancel_waited < Duration::from_secs(5),
            "{}：中断应当很快返回（等了 {cancel_waited:?}）",
            target.driver
        );
        eprintln!(
            "✅ {}：中断回了一条可读错误（{cancel_waited:?}，{error}）",
            target.driver
        );

        // B3-b：超时——连接配了 1 秒，慢查询应当被引擎取消并报可读错误
        runtime.block_on(manager.close_all_connections());
        let Some(_timeout_conn) =
            connect_with_retry(&runtime, &manager, &target, &timeout_value, Some(1))
        else {
            panic!("{} 建超时连接失败", target.driver);
        };
        let started = Instant::now();
        let timed_out = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Statement(target.slow_sql.to_string()),
            ResultPlacement::Replace,
            1,
        );
        let waited = started.elapsed();
        let error = timed_out[0]
            .error
            .clone()
            .unwrap_or_else(|| format!("{}：这句没超时（{}），超时分支没验到", target.driver, target.slow_sql));
        assert!(
            error.contains("timed out"),
            "{}：超时要报可读错误，实得 —— {error}",
            target.driver
        );
        assert!(
            waited < Duration::from_secs(10),
            "{}：超时后不该等慢查询跑完（等了 {waited:?}）",
            target.driver
        );
        eprintln!("✅ {}：超时可读（{error}）", target.driver);

        runtime.block_on(manager.close_all_connections());
    }

    if checked == 0 {
        eprintln!("⚠️ 没有任何 RDS_TEST_* 环境变量，真机链路未验证（不算失败）");
    }
}
