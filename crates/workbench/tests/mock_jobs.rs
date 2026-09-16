//! Mock 后台任务集成测试（M7）：工作线程 + 进度 + 结果取回。
//!
//! 走 `services::mock_jobs`（生产入口）：生成 / 追加 / **三个出口**都应在**工作线程**上跑，
//! UI 侧只提交任务、读进度、取结果。本文件不取消任务（取消在独立进程的
//! `mock_job_cancel.rs` 里验证——`MockEngine::cancel` 是进程级全局标志）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mock::mock_view::{
    MockDraft, MockJobDone, MockJobKind, MockJobState, MockRunOptions,
};
use mock::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale, MockExportFormat};
use rds_workbench::services::mock_generator;
use rds_workbench::services::mock_jobs;

/// 任务单例是**进程级**的：同二进制内的用例必须串行，否则会互相看到对方的「进行中」。
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 任务路径（除草稿箱用例外，项目根均为「未打开项目」）。
fn paths(db: &Path) -> mock_jobs::JobPaths {
    mock_jobs::JobPaths {
        db_path: Some(db.to_path_buf()),
        project_root: None,
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_mockjob_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn column(name: &str, generator: GeneratorConfig) -> mock::mock_view::MockColumnSpec {
    mock::mock_view::MockColumnSpec {
        id: 0,
        def: ColumnDef {
            name: name.to_string(),
            data_type: ColumnDataType::Integer,
            generator,
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        confidence: "high".to_string(),
        sample_value: String::new(),
    }
}

/// 两列草稿：自增主键 + 随机整数。
fn draft(table: &str, rows: u32) -> MockDraft {
    MockDraft {
        table_name: table.to_string(),
        columns: vec![
            column("id", GeneratorConfig::AutoIncrement { start: 1, step: 1 }),
            column("amount", GeneratorConfig::RandomInt { min: 1, max: 100 }),
        ],
        options: MockRunOptions::new(rows, Some(42), Locale::ZhCn),
    }
}

/// 轮询到任务结束（带超时，避免测试挂死）。
fn wait_done(timeout: Duration) -> Result<MockJobDone, String> {
    let started = Instant::now();
    loop {
        if let Some(result) = mock_jobs::take_done() {
            return result;
        }
        assert!(
            started.elapsed() < timeout,
            "后台任务超时（{:?} 未结束）",
            timeout
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 生成任务：提交即返回（UI 不阻塞）→ 进度可读 → 结果一次性取回。
#[test]
fn generate_job_reports_progress_then_done() {
    let _guard = serial();
    let dir = temp_dir("gen");
    let db = dir.join("analytics.duckdb");
    // 5 批（10k 行/批）：足以观察到中间进度
    let job = draft("t_job_gen", 50_000);

    mock_jobs::start(&job, MockJobKind::Generate, &paths(&db)).expect("提交任务");
    // 提交后立刻：进行中，且拿不到结果
    assert!(
        matches!(mock_jobs::state(), MockJobState::Running(_)),
        "提交后应处于进行中"
    );
    assert!(mock_jobs::take_done().is_none(), "结果未就绪");

    let done = wait_done(Duration::from_secs(180)).expect("生成应成功");
    match done {
        MockJobDone::Generated(info) => {
            assert_eq!(info.row_count, 50_000);
            assert_eq!(info.temp_table_name, "temp_mock_t_job_gen");
            assert!(!info.preview.rows.is_empty(), "应带预览");
            assert_eq!(info.preview.columns, ["id", "amount"]);
        }
        other => panic!("期望 Generated，实际 {other:?}"),
    }
    assert!(
        matches!(mock_jobs::state(), MockJobState::Idle),
        "取走结果后应归位空闲"
    );
    // 关键语义：生成不写库
    assert!(
        mock_generator::existing_tables_at(&db).is_empty(),
        "生成不应在分析库建表: {:?}",
        mock_generator::existing_tables_at(&db)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 进行中重复提交 → 拒绝；结束后可再次提交（证明串行语义不是「永久占用」）。
#[test]
fn start_while_running_is_rejected_then_recovers() {
    let _guard = serial();
    let dir = temp_dir("busy");
    let db = dir.join("analytics.duckdb");
    let job = draft("t_job_busy", 50_000);

    mock_jobs::start(&job, MockJobKind::Generate, &paths(&db)).expect("首个任务应成功提交");
    let err = mock_jobs::start(&job, MockJobKind::Generate, &paths(&db)).expect_err("应拒绝并发任务");
    assert!(err.contains("已有任务"), "err: {err}");

    wait_done(Duration::from_secs(180)).expect("首个任务应成功");
    mock_jobs::start(&job, MockJobKind::Generate, &paths(&db)).expect("结束后应可再次提交");
    wait_done(Duration::from_secs(180)).expect("第二个任务应成功");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 追加任务：一次任务里完成「生成 + 写入既有表」，回传表内总行数。
#[test]
fn append_job_reports_total_rows() {
    let _guard = serial();
    let dir = temp_dir("append");
    let db = dir.join("analytics.duckdb");
    let job = draft("t_job_append", 1_000);

    // 先建表（30 行）
    let seed_draft = draft("t_job_append", 30);
    let info = mock_generator::generate_at(Some(&db), &seed_draft, None).expect("首次生成");
    mock_generator::persist_table_at(&db, &info).expect("建表");

    // 追加任务：生成 1000 行后写入，自增起点接续表内 30 行
    mock_jobs::start(
        &job,
        MockJobKind::AppendTo("t_job_append".to_string()),
        &paths(&db),
    )
    .expect("提交追加任务");
    let done = wait_done(Duration::from_secs(180)).expect("追加应成功");
    match done {
        MockJobDone::Appended { table, total_rows } => {
            assert_eq!(table, "t_job_append");
            assert_eq!(total_rows, 1030, "表内应为 30 + 1000 行");
        }
        other => panic!("期望 Appended，实际 {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// 项目切换清理：宿主删掉 mock 临时表后，本进程里不再有它（面板据此作废预览）。
///
/// 走的是宿主真的会调的那一层（`services::mock_generator::clear_temp_tables`）。
#[test]
fn clear_temp_tables_wipes_process_temp_tables() {
    let _guard = serial();
    let dir = temp_dir("clear");
    let db = dir.join("analytics.duckdb");
    let job = draft("t_job_clear", 1_000);

    mock_jobs::start(&job, MockJobKind::Generate, &paths(&db)).expect("提交任务");
    let temp_table = match wait_done(Duration::from_secs(180)).expect("生成应成功") {
        MockJobDone::Generated(info) => info.temp_table_name,
        other => panic!("期望 Generated，实际 {other:?}"),
    };

    let cleared = rds_workbench::services::mock_generator::clear_temp_tables();
    assert!(
        cleared.contains(&temp_table),
        "应删掉刚生成的临时表: {cleared:?}"
    );
    let after = mock::MockEngine::temp_tables().expect("列临时表");
    assert!(!after.contains(&temp_table), "清理后不应还在: {after:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 生成任务的便捷封装：提交 → 等结果 → 取出 `MockGenInfo`（出口任务要用它）。
fn generate_and_take(draft: &MockDraft, db: &Path) -> mock::mock_view::MockGenInfo {
    mock_jobs::start(draft, MockJobKind::Generate, &paths(db)).expect("提交生成任务");
    match wait_done(Duration::from_secs(180)).expect("生成应成功") {
        MockJobDone::Generated(info) => info,
        other => panic!("期望 Generated，实际 {other:?}"),
    }
}

/// 表内行数（直连分析库读）。
fn count_rows(db: &Path, table: &str) -> i64 {
    let conn = duckdb::Connection::open(db).expect("open analysis db");
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .expect("count rows")
}

/// 出口（落库）走后台任务：新建表 + 回传行数；生成与写入是**两个**任务，不是一次。
#[test]
fn persist_job_creates_table_and_reports_rows() {
    let _guard = serial();
    let dir = temp_dir("persist_job");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_job_persist", 1_000);

    let info = generate_and_take(&draft, &db);
    mock_jobs::start(&draft, MockJobKind::Persist(info), &paths(&db)).expect("提交落库任务");
    let done = wait_done(Duration::from_secs(180)).expect("落库应成功");
    match done {
        MockJobDone::Persisted { table, rows } => {
            assert_eq!(table, "t_job_persist");
            assert_eq!(rows, 1_000);
        }
        other => panic!("期望 Persisted，实际 {other:?}"),
    }
    assert_eq!(count_rows(&db, "t_job_persist"), 1_000);
    assert_eq!(
        mock_generator::existing_tables_at(&db),
        ["t_job_persist".to_string()],
        "新表应出现在既有表清单里（面板据此刷新追加目标）"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 落库同名的既有表：任务以可读错误收尾（引导改用「追加」），不覆盖既有数据。
#[test]
fn persist_job_reports_existing_table_error() {
    let _guard = serial();
    let dir = temp_dir("persist_taken");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_job_taken", 30);

    // 先建表（同步装配层入口，绕开任务）
    let seed = mock_generator::generate_at(Some(&db), &draft, None).expect("首次生成");
    mock_generator::persist_table_at(&db, &seed).expect("建表");

    let info = generate_and_take(&draft, &db);
    mock_jobs::start(&draft, MockJobKind::Persist(info), &paths(&db)).expect("提交落库任务");
    let err = wait_done(Duration::from_secs(180)).expect_err("同名表应报错");
    assert!(err.contains("已存在"), "err: {err}");
    assert_eq!(count_rows(&db, "t_job_taken"), 30, "既有数据不应被覆盖");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 出口（导出）：写文件也是后台任务，回传含落地路径的文案。
#[test]
fn export_job_writes_csv_file() {
    let _guard = serial();
    let dir = temp_dir("export_job");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_job_export", 20);

    let info = generate_and_take(&draft, &db);
    let csv = dir.join("out.csv");
    mock_jobs::start(
        &draft,
        MockJobKind::Export {
            info,
            format: MockExportFormat::Csv,
            path: csv.to_string_lossy().to_string(),
        },
        &paths(&db),
    )
    .expect("提交导出任务");

    let done = wait_done(Duration::from_secs(180)).expect("导出应成功");
    match done {
        MockJobDone::Exported { message } => assert!(message.contains("已导出"), "{message}"),
        other => panic!("期望 Exported，实际 {other:?}"),
    }
    let text = std::fs::read_to_string(&csv).expect("读取导出文件");
    assert!(text.starts_with("id,amount"), "首行应为表头: {text}");
    assert_eq!(text.lines().count(), 21, "表头 + 20 行");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 出口（草稿箱）：项目根由宿主在提交前解析（本处直接给 `JobPaths`）；未打开项目 → 可读错误。
#[test]
fn scratchpad_job_writes_under_project_root() {
    let _guard = serial();
    let dir = temp_dir("scratch_job");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_job_scratch", 5);

    let info = generate_and_take(&draft, &db);

    // 未打开项目：任务照样能跑完，以可读错误收尾
    mock_jobs::start(
        &draft,
        MockJobKind::Scratchpad {
            info: info.clone(),
            format: MockExportFormat::Csv,
        },
        &paths(&db),
    )
    .expect("提交草稿箱任务");
    let err = wait_done(Duration::from_secs(180)).expect_err("无项目应报错");
    assert!(err.contains("未打开项目"), "err: {err}");

    // 有项目根：写到 {项目根}/mock/
    mock_jobs::start(
        &draft,
        MockJobKind::Scratchpad {
            info,
            format: MockExportFormat::Csv,
        },
        &mock_jobs::JobPaths {
            db_path: Some(db.clone()),
            project_root: Some(dir.clone()),
        },
    )
    .expect("提交草稿箱任务");
    let done = wait_done(Duration::from_secs(180)).expect("保存草稿箱应成功");
    match done {
        MockJobDone::Exported { message } => {
            assert!(message.contains("已保存到草稿箱"), "{message}")
        }
        other => panic!("期望 Exported，实际 {other:?}"),
    }
    let saved: Vec<String> = std::fs::read_dir(dir.join("mock"))
        .expect("mock 目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].starts_with("mock_t_job_scratch_"), "{saved:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 未打开项目：落库与追加**明确拒绝**，且原因里写清替代路径；
/// 而纯生成不受影响（内存临时表是进程级的，与库无关）。
#[test]
fn sinks_without_a_project_are_refused_with_a_readable_reason() {
    let _guard = serial();
    let job = draft("t_job_no_project", 50);
    let no_project = mock_jobs::JobPaths {
        db_path: None,
        project_root: None,
    };

    // 生成：不碰库，照常可行
    mock_jobs::start(&job, MockJobKind::Generate, &no_project).expect("提交生成");
    let info = match wait_done(Duration::from_secs(180)).expect("未打开项目也能生成") {
        MockJobDone::Generated(info) => info,
        other => panic!("期望 Generated，实际 {other:?}"),
    };

    // 落库：拒绝，且理由要给替代路径（用户想进全局时的正路是存档升级）
    mock_jobs::start(&job, MockJobKind::Persist(info.clone()), &no_project).expect("提交落库");
    let err = wait_done(Duration::from_secs(60)).expect_err("未打开项目应拒绝落库");
    assert!(err.contains("未打开项目"), "{err}");
    assert!(err.contains("资产库存档"), "理由里要给替代路径：{err}");

    // 追加：生成阶段就要读目标表，同样拒绝
    mock_jobs::start(
        &job,
        MockJobKind::AppendTo("t_no_project".to_string()),
        &no_project,
    )
    .expect("提交追加");
    let err = wait_done(Duration::from_secs(60)).expect_err("未打开项目应拒绝追加");
    assert!(err.contains("未打开项目"), "{err}");
}

/// 场景模板任务：按内置模板一次生成多张临时表，逐表带预览与列定义，**不写库**；
/// 再把其中一张落到项目分析库——表名与列定义都取自**结果自己**，不是草稿。
///
/// 用最便宜的内置模板（人力资源系统：500 + 20 + 500 行），避免把测试拖成负担。
#[test]
fn scenario_job_generates_every_table_without_writing_to_the_db() {
    let _guard = serial();
    let dir = temp_dir("scenario_job");
    let db = dir.join("analytics.duckdb");
    // 草稿与场景毫无关系，且**一列都没有**：场景模板自带表与列
    let job = MockDraft {
        table_name: "unused_by_scenario".to_string(),
        columns: Vec::new(),
        options: MockRunOptions::new(1, None, Locale::ZhCn),
    };
    let no_project = mock_jobs::JobPaths {
        db_path: None,
        project_root: None,
    };

    mock_jobs::start(
        &job,
        MockJobKind::Scenario("builtin:hr".to_string()),
        &no_project,
    )
    .expect("提交场景任务");
    let done = wait_done(Duration::from_secs(300)).expect("场景生成应成功");
    let tables = match done {
        MockJobDone::ScenarioGenerated {
            template_name,
            tables,
        } => {
            assert_eq!(template_name, "人力资源系统");
            assert_eq!(
                tables
                    .iter()
                    .map(|info| info.table_name.as_str())
                    .collect::<Vec<_>>(),
                ["employees", "departments", "salaries"],
                "顺序即模板里的表序"
            );
            assert_eq!(
                tables.iter().map(|info| info.row_count).collect::<Vec<_>>(),
                [500, 20, 500]
            );
            for info in &tables {
                assert_eq!(
                    info.temp_table_name,
                    format!("temp_mock_{}", info.table_name)
                );
                assert!(!info.columns.is_empty(), "结果要带列定义：出口据此建表");
                assert_eq!(
                    info.columns
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>(),
                    info.preview.columns,
                    "预览列与列定义同一套（{}）",
                    info.table_name
                );
                assert!(!info.preview.rows.is_empty(), "逐表补预览");
            }
            tables
        }
        other => panic!("期望 ScenarioGenerated，实际 {other:?}"),
    };

    // 生成不写库：项目分析库文件根本不该出现（场景任务连 db_path 都不需要）
    assert!(!db.exists(), "场景生成不应碰分析库");

    // 出口：把选中的那张（departments）写成分析库新表
    let chosen = tables.into_iter().nth(1).expect("第二张表");
    mock_jobs::start(&job, MockJobKind::Persist(chosen), &paths(&db)).expect("提交落库任务");
    let done = wait_done(Duration::from_secs(300)).expect("场景结果应能落库");
    match done {
        MockJobDone::Persisted { table, rows } => {
            assert_eq!(
                table, "departments",
                "表名取自结果，不是草稿的 unused_by_scenario"
            );
            assert_eq!(rows, 20);
        }
        other => panic!("期望 Persisted，实际 {other:?}"),
    }
    assert_eq!(count_rows(&db, "departments"), 20);
    assert_eq!(
        mock_generator::existing_tables_at(&db),
        ["departments".to_string()]
    );

    let _ = std::fs::remove_dir_all(&dir);
}
