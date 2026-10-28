//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::{
    Entity, TestAppContext, VisualTestContext,
};

use crate::channel::ExecChannel;
use crate::execution::{self, QueryData, QueryRunner};
use crate::export::ExportFormat;
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::EditorHostPanel;

use super::{
    grid_rows, open_panel, run_statement, shared_with_runner,
};
use super::results_b::{attach_export_picker, export_temp_path};

// ═══════════════════════════════════════════════════════════════════════

/// 假执行器（B7 切片二）：记下收到的 DuckDB 导出请求，并回写出的行数
///
/// 真写文件是引擎那边的事（`COPY … TO …`）：这里只验**编辑器真的把请求交给了执行器**、
/// 交过去的原料对不对、回执怎么落到状态栏。
struct DuckDbExportRunner {
    seen: std::sync::Arc<std::sync::Mutex<Vec<execution::DuckDbExportRequest>>>,
}

impl QueryRunner for DuckDbExportRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        Ok(QueryData {
            columns: vec!["id".to_string(), "tag".to_string()],
            rows: vec![
                vec!["1".to_string(), "alpha".to_string()],
                vec!["2".to_string(), "beta".to_string()],
                vec!["3".to_string(), "gamma".to_string()],
            ],
            column_types: Vec::new(),
            elapsed_ms: 4,
            truncated: false,
            affected_rows: None,
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }

    fn export_via_duckdb(&self, request: &execution::DuckDbExportRequest) -> Result<usize, String> {
        self.seen.lock().expect("锁").push(request.clone());
        Ok(request.rows.len())
    }
}

/// 等导出回执（后台线程；导出不在 `pending` 里，所以自己转轮询泵）
fn wait_for_export(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.export_pending_for_test() == 0
            })
        });
        if done {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "导出回执迟迟没回来");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 【B7 切片二】Parquet：不在本地编码，而是把行交给执行器（`COPY` 那条路）
#[gpui_kit::test]
fn parquet_export_goes_through_the_runner(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(DuckDbExportRunner {
        seen: seen.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(
            "select id, tag from t",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let path = export_temp_path("parquet", "parquet");
    std::fs::remove_file(&path).ok();
    attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select id, tag from t",
        execution::ResultPlacement::Replace,
    );

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Parquet, cx)
        })
    });
    // 提交要立刻说“在跑”（首次装扩展可能几秒，不能点完什么都不说）
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要有一句在途提示");
    assert!(message.contains("正在导出 Parquet"), "{message}");

    wait_for_export(cx, &panel);

    let seen = seen.lock().expect("锁").clone();
    assert_eq!(seen.len(), 1, "导出应当交给执行器一次");
    assert_eq!(seen[0].format, ExportFormat::Parquet);
    assert_eq!(seen[0].path, path);
    assert_eq!(
        seen[0].columns,
        vec!["id".to_string(), "tag".to_string()],
        "列名照搬结果集"
    );
    assert_eq!(seen[0].rows.len(), 3, "已抓到的三行都过去");
    assert_eq!(seen[0].rows[0], vec!["1".to_string(), "alpha".to_string()]);
    assert!(!seen[0].filtered, "没筛选就不该说“已筛选”");
    // 【D1】无事实档 = 猜型降级档：标记随请求走，回执文案明示口径
    assert!(seen[0].degraded, "无事实档要置降级标记");

    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("回执要落到状态栏");
    assert!(message.contains("已导出 3 行"), "{message}");
    assert!(message.contains("Parquet"), "{message}");
    assert!(message.contains("数字列按文本存储"), "降级档要在回执明示：{message}");
    assert!(!path.exists(), "假执行器不写文件（真写是引擎那条路的事）");
}

/// 【B7 切片二】筛选中导出：交给执行器的就是**筛后的行集**，回执也明示
#[gpui_kit::test]
fn duckdb_export_follows_the_local_filter(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(DuckDbExportRunner {
        seen: seen.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(
            "select id, tag from t",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let path = export_temp_path("xlsx", "xlsx");
    attach_export_picker(&shared, Some(path));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select id, tag from t",
        execution::ResultPlacement::Replace,
    );
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.set_filter_for_test("gamma", cx)));
    assert_eq!(grid_rows(cx, &panel), 1, "筛选后只剩一行");

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Xlsx, cx)
        })
    });
    wait_for_export(cx, &panel);

    let seen = seen.lock().expect("锁").clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].format, ExportFormat::Xlsx);
    assert!(seen[0].filtered, "筛选中导出要带上这个事实");
    assert!(seen[0].degraded, "无事实档要置降级标记");
    assert_eq!(
        seen[0].rows,
        vec![vec!["3".to_string(), "gamma".to_string()]],
        "过去的是筛后的行，不是三行"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("回执要落到状态栏");
    assert!(message.contains("已导出 1 行（已筛选）"), "{message}");
    assert!(message.contains("数字列按文本存储"), "降级档要在回执明示：{message}");
}

/// 【B7 切片二】执行器不支持这条路时，如实说原因（不静默失败）
#[gpui_kit::test]
fn a_runner_without_duckdb_export_reports_the_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // `shared_with_runner` 的假执行器只实现了 `run`
    let (shared, id, _seen, _seen_conn) =
        shared_with_runner("select id, tag from t", EditorMode::Sql);
    let path = export_temp_path("unsupported", "parquet");
    attach_export_picker(&shared, Some(path));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select id, tag from t",
        execution::ResultPlacement::Replace,
    );

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Parquet, cx)
        })
    });
    wait_for_export(cx, &panel);

    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("导出失败"), "{message}");
    assert!(message.contains("不支持"), "{message}");
}

// ═══════════════════════════════════════════════════════════════════════
// 结果集本地分析（B15）
