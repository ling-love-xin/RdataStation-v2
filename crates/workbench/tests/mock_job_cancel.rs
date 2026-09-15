//! Mock 后台任务**取消**集成测试（M7）。
//!
//! 单独一个测试二进制（= 独立进程）的原因：`MockEngine::cancel` 置的是**进程级**
//! 取消标志，而 `generate` 会在开始时重置它——同一进程里并发跑的其它生成用例会被
//! 误伤（或被悄悄重置）。放到独立进程里，取消语义才是确定性的。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mock::mock_view::{MockDraft, MockJobKind, MockJobState, MockRunOptions};
use mock::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale};
use rds_workbench::services::mock_jobs;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_mockcancel_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// 20 批（10k 行/批）：取消一定发生在中间批次边界上。
fn draft(table: &str) -> MockDraft {
    MockDraft {
        table_name: table.to_string(),
        columns: vec![mock::mock_view::MockColumnSpec {
            id: 0,
            def: ColumnDef {
                name: "id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
            confidence: "high".to_string(),
            sample_value: String::new(),
        }],
        options: MockRunOptions::new(200_000, Some(7), Locale::ZhCn),
    }
}

#[test]
fn cancel_interrupts_running_job_with_readable_error() {
    let dir = temp_dir("cancel");
    let db = dir.join("analytics.duckdb");
    mock_jobs::start(
        &draft("t_job_cancel"),
        MockJobKind::Generate,
        &mock_jobs::JobPaths {
            db_path: db.clone(),
            project_root: None,
        },
    )
    .expect("提交任务");

    // 等首批完成：此时 generate 已重置取消标志，取消一定作用在运行中的任务上
    let started = Instant::now();
    loop {
        if let Some(result) = mock_jobs::take_done() {
            panic!("生成结束得太快，无法验证取消: {result:?}");
        }
        match mock_jobs::state() {
            MockJobState::Running(progress) if progress.batches_done > 0 => break,
            _ => {}
        }
        assert!(started.elapsed() < Duration::from_secs(180), "等待首批超时");
        std::thread::sleep(Duration::from_millis(5));
    }

    mock_jobs::cancel();

    // 取消后必定很快收尾（每批边界检查一次）
    let deadline = Instant::now();
    let result = loop {
        if let Some(result) = mock_jobs::take_done() {
            break result;
        }
        assert!(
            deadline.elapsed() < Duration::from_secs(180),
            "取消后任务未在超时前结束"
        );
        std::thread::sleep(Duration::from_millis(10));
    };

    let err = result.expect_err("取消应回传 Err");
    assert!(err.contains("取消"), "err: {err}");
    assert!(
        matches!(mock_jobs::state(), MockJobState::Idle),
        "取消后应归位空闲"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
