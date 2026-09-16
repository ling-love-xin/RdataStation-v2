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
}

const TARGETS: [Target; 4] = [
    Target {
        driver: "mysql",
        env: "RDS_TEST_MYSQL_URL",
        file: false,
    },
    Target {
        driver: "postgres",
        env: "RDS_TEST_PG_URL",
        file: false,
    },
    Target {
        driver: "sqlite",
        env: "RDS_TEST_SQLITE_PATH",
        file: true,
    },
    Target {
        driver: "duckdb",
        env: "RDS_TEST_DUCKDB_PATH",
        file: true,
    },
];

/// 建连并设为活动连接（返回 conn_id；失败返回 None 并打印原因）
async fn connect_active(manager: &Arc<ConnectionManager>, target: &Target, value: &str) -> Option<String> {
    let mut config = DriverConnectionConfig::new(target.driver);
    config.name = Some(format!("A14 编辑器执行探针（{}）", target.driver));
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

    // 手动轮询（生产由面板的轮询泵做）
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

        runtime.block_on(manager.close_all_connections());
    }

    if checked == 0 {
        eprintln!("⚠️ 没有任何 RDS_TEST_* 环境变量，真机链路未验证（不算失败）");
    }
}
