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
//! **sh / bash 下一律加单引号**：`RDS_TEST_SQLITE_PATH=D:\FossilT\T.fossil` 里的反斜杠会被
//! 当成转义吃掉（变成相对路径 `FossilTT.fossil`），于是驱动在工作目录里新建一个空库——
//! 看着“通过”了，实际一句都没碰到真库。
//!
//! 建连失败**不会中断整轮**（一个坏驱动不该把后面的库全遮住），最后会一并报出来。
//!
//! 与 `engine/tests/transaction_affinity.rs` 同一套环境变量与建连方式（探针口径一致）。
//! 执行走的是"当前活动连接"，所以这里先 `set_active_connection` 再提交。
//!
//! **六个内置驱动都跑**（`mysql` / `mysql_native` / `postgres` / `postgres_native` / `sqlite` /
//! `duckdb`）：sqlx 版与 native 版是两套实现——事务、取消、取数各写一遍，只验一边等于只验一半。

use std::sync::Arc;
use std::time::{Duration, Instant};

use editor::execution::{ExecTarget, ResultPlacement, RunOptions, TxAction, TxSnapshot, batch_target};
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

const TARGETS: [Target; 6] = [
    Target {
        driver: "mysql",
        env: "RDS_TEST_MYSQL_URL",
        file: false,
        slow_sql: "SELECT SLEEP(3)",
    },
    Target {
        // 同一台库的**原生驱动**（与 sqlx 版是两套实现：事务、取消、取数都各写一遍）
        driver: "mysql_native",
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
        driver: "postgres_native",
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
        .submit(
            document,
            target,
            placement,
            RunOptions::default(),
        )
        .expect("提交执行");
    collect_outcomes(shared, expected)
}

/// 等回填（生产由面板的轮询泵做；这里手动轮询），落位用结论自带的
fn collect_outcomes(shared: &EditorShared, expected: usize) -> Vec<editor::store::ResultEntry> {
    collect_with_snapshots(shared, expected)
        .into_iter()
        .map(|(entry, _)| entry)
        .collect()
}

/// 同上，但把每份结果**执行后的事务状态**一起带回来（B4 要用）
fn collect_with_snapshots(
    shared: &EditorShared,
    expected: usize,
) -> Vec<(editor::store::ResultEntry, TxSnapshot)> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut collected = Vec::new();
    while collected.len() < expected {
        for outcome in shared.drain_exec() {
            let transaction = outcome.transaction;
            let entry = match outcome.result {
                Ok(data) => editor::store::ResultEntry::success(
                    outcome.document,
                    outcome.sql,
                    data.elapsed_ms,
                    data.truncated,
                    data.columns,
                    data.rows,
                )
                .with_affected_rows(data.affected_rows)
                .with_has_more(data.has_more),
                Err(error) => {
                    editor::store::ResultEntry::failure(outcome.document, outcome.sql, error, 0)
                }
            };
            // 落位来自结论自己（"结果放哪"是执行时的语义，不是调用方事后猜的）
            shared.update_results(|store| store.push(entry.clone(), outcome.placement));
            collected.push((entry, transaction));
        }
        if collected.len() >= expected {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "只等回 {} 份结果（预期 {expected} 份），30s 超时",
            collected.len()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    collected
}

/// 跑一句（带执行选项），成功则返回结果 + 执行后的事务状态
fn run_with_options(
    shared: &EditorShared,
    document: DocumentId,
    sql: &str,
    options: RunOptions,
) -> (editor::store::ResultEntry, TxSnapshot) {
    shared
        .submit(
            document,
            &ExecTarget::Statement(sql.to_string()),
            ResultPlacement::Replace,
            options,
        )
        .expect("提交执行");
    let (entry, transaction) = collect_with_snapshots(shared, 1)
        .into_iter()
        .next()
        .expect("有结果回填");
    assert!(entry.error.is_none(), "`{sql}` 报错 —— {:?}", entry.error);
    (entry, transaction)
}

/// 跑一句（默认选项）
fn run_one(shared: &EditorShared, document: DocumentId, sql: &str) -> editor::store::ResultEntry {
    run_with_options(shared, document, sql, RunOptions::default()).0
}

/// 跑一句**允许失败**的语句（B6 要看驱动真实的错误文本）
fn run_one_allowing_failure(
    shared: &EditorShared,
    document: DocumentId,
    sql: &str,
) -> editor::store::ResultEntry {
    shared
        .submit(
            document,
            &ExecTarget::Statement(sql.to_string()),
            ResultPlacement::Replace,
            RunOptions::default(),
        )
        .expect("提交执行");
    collect_with_snapshots(shared, 1)
        .into_iter()
        .next()
        .expect("有结果回填")
        .0
}

/// `SELECT count(*)` 的整数值（界面拿到的就是字符串，这里转回数字断言）
fn count_rows(shared: &EditorShared, document: DocumentId, table: &str) -> i64 {
    let entry = run_one(
        shared,
        document,
        &format!("SELECT count(*) AS n FROM {table}"),
    );
    entry.rows[0][0].parse().expect("count 应当是可解析的整数")
}

/// 走编辑器端口做一个事务动作，并等回执（生产由面板的轮询泵做）
fn tx_action(shared: &EditorShared, document: DocumentId, action: TxAction) {
    shared
        .request_transaction(document, action)
        .expect("事务动作应当被接受");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(note) = shared
            .drain_tx_notes()
            .into_iter()
            .find(|note| note.action == action)
        {
            note.result.expect("事务动作应当成功");
            return;
        }
        assert!(Instant::now() < deadline, "{action:?} 的回执迟迟没回来");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_editor_execution_port_runs_a_query_on_every_configured_database() {
    AutoDriverRegistrar::register_builtin_drivers();

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    // 生产路径用的是**全局连接管理器**，测试也用它：执行器从活动连接出发找库
    let manager = engine::connection_manager::get_connection_manager().clone();

    let mut checked = 0;
    // 建连失败**只记账、不中断整轮**：一个坏驱动（环境 / feature 缺失）不该把后面的库
    // 全遮住——MySQL 的 `mysql-rsa` 缺失就是这类
    let mut failed_to_connect: Vec<String> = Vec::new();
    for target in TARGETS {
        let Ok(value) = std::env::var(target.env) else {
            eprintln!("⏭️  {}：未设 {} ，跳过", target.driver, target.env);
            continue;
        };
        checked += 1;

        // 每次换一个活动连接：编辑器执行走"当前活动连接"
        let Some(_conn_id) = runtime.block_on(connect_active(&manager, &target, &value)) else {
            failed_to_connect.push(target.driver.to_string());
            continue;
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
        // 用**相对口径**断言份数：这个文档后面还有别的用例在推结果集，写死总数会随流程改动而碎
        let sets_before_batch = shared.results().set_count(&document);
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
            sets_before_batch + 3,
            "{}：批量三句该新增三个结果集（批量前 {} 份）",
            target.driver,
            sets_before_batch
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
            failed_to_connect.push(format!("{}（B3 中断用连接）", target.driver));
            continue;
        };
        shared
            .submit(
                document.clone(),
                &ExecTarget::Statement(target.slow_sql.to_string()),
                ResultPlacement::Replace,
            RunOptions::default(),
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

        // B4：事务——自动提交关 → 执行进事务 → **回滚作废 / 提交生效**
        runtime.block_on(manager.close_all_connections());
        let tx_value = b3_value(&target, &value, "tx");
        let Some(_tx_conn) = connect_with_retry(&runtime, &manager, &target, &tx_value, None) else {
            failed_to_connect.push(format!("{}（B4 事务用连接）", target.driver));
            continue;
        };
        let table = format!("rds_tx_probe_{}", std::process::id());
        // 建表在自动提交下做：四个库对“事务内 DDL”的态度不一样（MySQL 会隐式提交），
        // 把 DDL 拉进事务只会验出驱动差异，验不出我们要的东西
        run_one(&shared, document.clone(), &format!("CREATE TABLE {table} (n INTEGER)"));

        let (_, in_tx) = run_with_options(
            &shared,
            document.clone(),
            &format!("INSERT INTO {table} VALUES (1)"),
            RunOptions {
                use_transaction: true,
            },
        );
        assert!(
            in_tx.in_transaction,
            "{}：自动提交关时，执行应当进事务（结论里要看得到）",
            target.driver
        );
        assert_eq!(
            count_rows(&shared, document.clone(), &table),
            1,
            "{}：事务内的插入自己看得见",
            target.driver
        );
        tx_action(&shared, document.clone(), TxAction::Rollback);
        assert_eq!(
            count_rows(&shared, document.clone(), &table),
            0,
            "{}：回滚后数据不该变",
            target.driver
        );

        let (_, committed) = run_with_options(
            &shared,
            document.clone(),
            &format!("INSERT INTO {table} VALUES (2)"),
            RunOptions {
                use_transaction: true,
            },
        );
        assert!(committed.in_transaction, "{}：第二次也要进事务", target.driver);
        tx_action(&shared, document.clone(), TxAction::Commit);
        assert_eq!(
            count_rows(&shared, document.clone(), &table),
            1,
            "{}：提交后数据要生效",
            target.driver
        );
        run_one(&shared, document.clone(), &format!("DROP TABLE {table}"));
        eprintln!("✅ {}：事务（回滚作废 / 提交生效）", target.driver);

        // B5：写语句的**真实影响行数**（引擎侧 P0.6：六个驱动的 DML 都要给出真值，
        // 而不是“无结果集”这种看不清成败的结论）
        let affected_table = format!("rds_affected_probe_{}", std::process::id());
        run_one(
            &shared,
            document.clone(),
            &format!("CREATE TABLE {affected_table} (n INTEGER)"),
        );
        let inserted = run_one(
            &shared,
            document.clone(),
            &format!("INSERT INTO {affected_table} VALUES (1), (2), (3)"),
        );
        assert_eq!(
            inserted.affected_rows,
            Some(3),
            "{}：INSERT 三行要报 3（实得 {:?}）",
            target.driver,
            inserted.affected_rows
        );
        assert!(
            !inserted.has_grid(),
            "{}：写语句不该有结果集",
            target.driver
        );
        assert!(
            inserted.summary().starts_with("影响 3 行"),
            "{}：结果区状态行要报影响行数（实得 {}）",
            target.driver,
            inserted.summary()
        );

        let updated = run_one(
            &shared,
            document.clone(),
            &format!("UPDATE {affected_table} SET n = n + 1 WHERE n > 1"),
        );
        assert_eq!(
            updated.affected_rows,
            Some(2),
            "{}：UPDATE 两行要报 2（实得 {:?}）",
            target.driver,
            updated.affected_rows
        );

        // 零行也要如实报 0：`Some(0)` 和 `None` 在界面上是两回事
        let deleted = run_one(
            &shared,
            document.clone(),
            &format!("DELETE FROM {affected_table} WHERE n = 99"),
        );
        assert_eq!(
            deleted.affected_rows,
            Some(0),
            "{}：DELETE 零行要报 0（实得 {:?}）",
            target.driver,
            deleted.affected_rows
        );
        // B6：故意写错列名 —— 真机上回的错误文本要能被定位到**那个词**
        // （定位靠 `editor::diagnostics`：PG 走结构化位置，其余驱动从文本里认）
        // 这一步要在 `DROP TABLE` **之前**：表还在，服务器才会去查列名
        let bad_column = format!("SELECT no_such_column_xyz FROM {affected_table}");
        let failed = run_one_allowing_failure(&shared, document.clone(), &bad_column);
        let error_text = failed.error.clone().unwrap_or_default();
        assert!(
            failed.error.is_some(),
            "{}：写错列名却没报错？—— {bad_column}",
            target.driver
        );
        let site = editor::diagnostics::site_in_document(&bad_column, &bad_column, &error_text)
            .unwrap_or_else(|| panic!("{}：错误文本没能定位 —— {error_text}", target.driver));
        assert_eq!(
            &bad_column[site.range()],
            "no_such_column_xyz",
            "{}：定位到的应当是写错的那个列名（{}）—— {error_text}",
            target.driver,
            site.location_text()
        );
        eprintln!(
            "✅ {}：错误定位 —— {}（{}）",
            target.driver,
            site.location_text(),
            site.token.as_deref().unwrap_or("?")
        );

        // B5b：分段抓取 —— 引擎把原 SQL 套成窗口再取（`SqlService::execute_segment`）
        // 这张表专门用来验分段：5 行不同值，看“拼起来到底重不重、漏不漏”
        let seg_table = format!("rds_seg_probe_{}", std::process::id());
        run_one(
            &shared,
            document.clone(),
            &format!("CREATE TABLE {seg_table} (n INTEGER)"),
        );
        run_one(
            &shared,
            document.clone(),
            &format!("INSERT INTO {seg_table} VALUES (1), (2), (3), (4), (5)"),
        );
        let service = engine::services::sql_service::SqlService::new(manager.clone());
        let segment = |sql: &str, offset: usize, limit: usize| -> Vec<String> {
            let outcome = runtime.block_on(service.execute_segment(
                None,
                sql,
                limit,
                offset,
                None,
            ));
            let result = outcome.unwrap_or_else(|error| {
                panic!("{}：第 {offset} 段取数失败 —— {error}", target.driver)
            });
            result
                .result
                .to_rows()
                .into_iter()
                .flatten()
                .map(|value| value.to_string())
                .collect()
        };

        let sql = format!("SELECT n FROM {seg_table} ORDER BY n");
        let mut collected = Vec::new();
        for offset in [0, 2, 4] {
            collected.extend(segment(&sql, offset, 2));
        }
        assert_eq!(
            collected,
            ["1", "2", "3", "4", "5"],
            "{}：三段拼起来要不重不漏（窗口重跑的取舍就靠这条盯住）",
            target.driver
        );
        // 拿不满一段 = 到底了（编辑器据此决定“还有没有下一段”）
        assert_eq!(segment(&sql, 4, 2).len(), 1, "{}：最后一段只有 1 行", target.driver);

        // 没有结果集的语句不能分段（DML 没有“第 2 段”可言）
        let dml = runtime.block_on(service.execute_segment(
            None,
            &format!("DELETE FROM {seg_table} WHERE n = 99"),
            2,
            0,
            None,
        ));
        assert!(dml.is_err(), "{}：DML 不该能分段", target.driver);

        // CTE 也能分段（子查询里套 `WITH`：四个方言都得认）
        let cte = format!("WITH x AS (SELECT n FROM {seg_table}) SELECT n FROM x ORDER BY n");
        assert_eq!(
            segment(&cte, 0, 2),
            ["1", "2"],
            "{}：CTE 也要能分段",
            target.driver
        );
        run_one(
            &shared,
            document.clone(),
            &format!("DROP TABLE {seg_table}"),
        );
        eprintln!(
            "✅ {}：分段抓取 —— 三段拼成 5 行不重不漏（DML 拒绝 / CTE 可分段也验了）",
            target.driver
        );

        // B5b：**走编辑器**的分段抓取（首段 1000 行 → 取下一段接在同一份结果上）
        // 建 1200 行：分成 6 批插（每批 200 行——SQLite 对多行 VALUES 有上限，不一次塞 1200 行）
        let page_table = format!("rds_page_probe_{}", std::process::id());
        run_one(
            &shared,
            document.clone(),
            &format!("CREATE TABLE {page_table} (n INTEGER)"),
        );
        for chunk in 0..6 {
            let values: Vec<String> = (1..=200)
                .map(|i| format!("({})", chunk * 200 + i))
                .collect();
            run_one(
                &shared,
                document.clone(),
                &format!("INSERT INTO {page_table} VALUES {}", values.join(", ")),
            );
        }
        let page_sql = format!("SELECT n FROM {page_table} ORDER BY n");
        let sets_before_page = shared.results().set_count(&document);
        let first = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Statement(page_sql.clone()),
            ResultPlacement::Replace,
            1,
        );
        let first = first.into_iter().next().expect("有结果回填");
        assert_eq!(
            first.row_count(),
            editor::execution::SEGMENT_ROWS,
            "{}：首段就是一段（{} 行）",
            target.driver,
            editor::execution::SEGMENT_ROWS
        );
        assert!(
            first.can_fetch_more(),
            "{}：首段拿满了 → 还能取下一段",
            target.driver
        );

        let more = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Segment {
                sql: page_sql.clone(),
                offset: editor::execution::SEGMENT_ROWS,
                limit: editor::execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            1,
        );
        let more = more.into_iter().next().expect("取段也要有回填");
        assert!(
            more.error.is_none(),
            "{}：取下一段失败 —— {:?}",
            target.driver,
            more.error
        );

        let active = shared
            .results_active(&document)
            .expect("有选中结果");
        assert_eq!(
            shared.results().set_count(&document),
            sets_before_page,
            "{}：取下一段既不新开也不吞掉结果集（取段前 {} 份，现 {} 份）",
            target.driver,
            sets_before_page,
            shared.results().set_count(&document)
        );
        assert_eq!(active.row_count(), 1_200, "{}：两段接起来 1200 行", target.driver);
        assert!(!active.can_fetch_more(), "{}：第二段没拿满 → 到底了", target.driver);
        // 拼起来不重不漏：看接缝两边的值
        assert_eq!(
            (active.rows[0][0].as_str(), active.rows[999][0].as_str()),
            ("1", "1000"),
            "{}：首段是 1..1000",
            target.driver
        );
        assert_eq!(
            (active.rows[1000][0].as_str(), active.rows[1199][0].as_str()),
            ("1001", "1200"),
            "{}：第二段接在 1001..1200",
            target.driver
        );
        run_one(
            &shared,
            document.clone(),
            &format!("DROP TABLE {page_table}"),
        );
        eprintln!(
            "✅ {}：分段抓取（编辑器链路）—— 首段 1000 行 + 取下一段接成 1200 行不重不漏",
            target.driver
        );

        run_one(
            &shared,
            document.clone(),
            &format!("DROP TABLE {affected_table}"),
        );
        eprintln!(
            "✅ {}：写语句影响行数 —— INSERT {} / UPDATE {:?} / DELETE {:?}",
            target.driver,
            inserted.summary(),
            updated.affected_rows,
            deleted.affected_rows
        );

        // B14：下发源库 —— 把筛选词翻成一条 WHERE 重查（CAST 目标类型按方言给，宿主编）
        let push_table = format!("rds_push_probe_{}", std::process::id());
        run_one(
            &shared,
            document.clone(),
            &format!("CREATE TABLE {push_table} (n INTEGER, tag VARCHAR(32))"),
        );
        run_one(
            &shared,
            document.clone(),
            &format!("INSERT INTO {push_table} VALUES (1, 'alpha'), (2, 'beta'), (3, 'gamma')"),
        );
        let push_sql = format!("SELECT n, tag FROM {push_table} ORDER BY n");
        let base = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Statement(push_sql.clone()),
            ResultPlacement::Replace,
            1,
        );
        assert!(
            base[0].error.is_none(),
            "{}：基准查询失败 —— {:?}",
            target.driver,
            base[0].error
        );
        let columns = base[0].columns.clone();
        let pushed = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Filtered {
                sql: push_sql.clone(),
                filter: "mm".to_string(),
                columns: columns.clone(),
            },
            ResultPlacement::NewSet,
            1,
        );
        assert!(
            pushed[0].error.is_none(),
            "{}：下发筛选失败 —— {:?}",
            target.driver,
            pushed[0].error
        );
        assert_eq!(
            pushed[0].rows,
            vec![vec!["3".to_string(), "gamma".to_string()]],
            "{}：下发只该回命中的那一行（`mm` 只命中 gamma）",
            target.driver
        );
        // LIMIT 要被去掉：原查询 LIMIT 1 只能看到 alpha，不去掉就永远筛不到第 3 行
        let limited = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Filtered {
                sql: format!("{push_sql} LIMIT 1"),
                filter: "mm".to_string(),
                columns: columns.clone(),
            },
            ResultPlacement::NewSet,
            1,
        );
        assert_eq!(
            limited[0].rows.len(),
            1,
            "{}：带 LIMIT 的原查询下发后仍能筛到第 3 行（说明 LIMIT 真的去掉了）—— {:?}",
            target.driver,
            limited[0].error
        );
        // 「已去掉 LIMIT」的提示在编辑器侧（随执行结论进状态栏），面板测试已盯；
        // 这里只钉“去掉之后真能筛到”。
        // 排序下发：按点的列重查（不 CAST——让源库按自己的列类型排）
        let sorted_down = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::SortedDown {
                sql: format!("SELECT n, tag FROM {push_table}"),
                column: "tag".to_string(),
                descending: true,
            },
            ResultPlacement::NewSet,
            1,
        );
        assert!(
            sorted_down[0].error.is_none(),
            "{}：排序下发失败 —— {:?}",
            target.driver,
            sorted_down[0].error
        );
        assert_eq!(
            sorted_down[0]
                .rows
                .iter()
                .map(|row| row[1].clone())
                .collect::<Vec<_>>(),
            ["gamma", "beta", "alpha"],
            "{}：降序下发要由源库按 tag 排",
            target.driver
        );
        run_one(
            &shared,
            document.clone(),
            &format!("DROP TABLE {push_table}"),
        );
        eprintln!(
            "✅ {}：下发源库 —— 筛选词拼 WHERE 重查（命中 1 行；带 LIMIT 的原查询也能筛到全部）· 排序下发按列重查",
            target.driver
        );

        // B8：历史面板的数据源 —— 走编辑器执行三次（含一次失败），引擎的 `history_store`
        // 里就该有这三条，且耗时 / 成败 / 失败原因 / 行数 / 时间都是真值。
        let before = editor::history::load(500).map(|items| items.len()).unwrap_or(0);
        run_one(&shared, document.clone(), "select 1 as n");
        run_one(&shared, document.clone(), "select 2 as n");
        // 失败那次不能用 `run_one`（它断言语句得成功）：走同一条编辑器链路，只断言“确实失败了”
        let failed_entry = run_through_editor(
            &shared,
            document.clone(),
            &ExecTarget::Statement("select no_such_column_xyz as n".to_string()),
            ResultPlacement::Replace,
            1,
        );
        assert!(
            failed_entry[0].error.is_some(),
            "{}：这一句本来就该失败（探针用错了语句？）",
            target.driver
        );
        let items = editor::history::load(500).expect("能读到历史（引擎的 history_store）");
        assert!(
            items.len() >= before + 3,
            "{}：三次执行都要留痕（执行前 {} 条，现 {} 条）",
            target.driver,
            before,
            items.len()
        );
        let succeeded = items
            .iter()
            .find(|item| item.sql == "select 1 as n")
            .unwrap_or_else(|| panic!("{}：成功的那条没留痕", target.driver));
        assert!(
            succeeded.rows_text.as_deref() == Some("返回 1 行"),
            "{}：成功那条要带返回行数，实得 {:?}",
            target.driver,
            succeeded.rows_text
        );
        assert_eq!(
            succeeded.time_text, "刚刚",
            "{}：刚执行的记录时间就是“刚刚”",
            target.driver
        );
        assert!(
            succeeded.duration_text.ends_with("ms") || succeeded.duration_text.ends_with('s'),
            "{}：耗时是真值（实得 {}）",
            target.driver,
            succeeded.duration_text
        );
        let failed = items
            .iter()
            .find(|item| item.sql.contains("no_such_column_xyz"))
            .unwrap_or_else(|| panic!("{}：失败的那条也要留痕", target.driver));
        assert!(
            failed.failed() && failed.error.as_deref().is_some_and(|text| !text.is_empty()),
            "{}：失败要带原因，实得 {:?}",
            target.driver,
            failed.error
        );
        eprintln!(
            "✅ {}：历史面板数据 —— 三次执行都留痕（成功 {} · 耗时 {} · 失败带原因）",
            target.driver, succeeded.rows_text.as_deref().unwrap_or("-"), succeeded.duration_text
        );

        runtime.block_on(manager.close_all_connections());
        let Some(_timeout_conn) =
            connect_with_retry(&runtime, &manager, &target, &timeout_value, Some(1))
        else {
            failed_to_connect.push(format!("{}（B3 超时用连接）", target.driver));
            continue;
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

    assert!(
        failed_to_connect.is_empty(),
        "这些库已配置环境变量但建不上连接（真机验证被阻塞，其余库照跑）：{failed_to_connect:?}"
    );

    if checked == 0 {
        eprintln!("⚠️ 没有任何 RDS_TEST_* 环境变量，真机链路未验证（不算失败）");
    }
}
