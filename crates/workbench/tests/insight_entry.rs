//! 洞察入口 → 后台取数 → 回填面板的**端到端**（真 DuckDB 内存临时表）。
//!
//! 走的是**真实接线**：`insight_jobs::request_profile`——即面板发出
//! `InsightEvent::ProfileRequested` 之后宿主调用的那一个函数。它覆盖了中间那段胶水
//! （提交前解析项目根 → 后台执行器跑阻塞画像 → 弱句柄回填四态），而不只是重测服务层
//! （服务层的端到端在 `crates/insight/tests/column_profile_e2e.rs`）。
//!
//! 不连任何外部数据库：DuckDB 是本地内存引擎。
//!
//! 注意 `TestAppContext::update` 收的是**单参闭包**（`&mut App`）；带窗口的双参版本
//! 属 `VisualTestContext`，别照搬窗口测试的写法。

use gpui_kit::{AppContext as _, TestAppContext};
use insight::insight_engine::get_or_create_duckdb;
use insight::{ColumnKind, InsightPanelState, InsightTarget, InsightView};
use rds_workbench::services::insight_jobs::{ProfileRequest, request_profile};

/// 建临时表并插数据（表名唯一：DuckDB 连接是进程级单例）
fn seed(table: &str, ddl: &str, values: &str) {
    let duckdb = get_or_create_duckdb().expect("应能拿到内存 DuckDB 连接");
    let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE \"{table}\" ({ddl})"
    ))
    .expect("建临时表");
    conn.execute_batch(&format!("INSERT INTO \"{table}\" {values}"))
        .expect("插数据");
}

fn column_target(temp_table: &str) -> InsightTarget {
    InsightTarget::Column {
        temp_table: temp_table.to_string(),
        column: "amount".into(),
        data_type: "INTEGER".into(),
    }
}

fn request_of(temp_table: &str) -> ProfileRequest {
    ProfileRequest {
        temp_table: temp_table.to_string(),
        column: "amount".into(),
    }
}

/// 正常路径：请求提交后，面板自己从「加载中」走到「数据」
#[gpui_kit::test]
fn profile_request_fills_the_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let table = "t_insight_entry_ok";
    let rows: Vec<String> = (1..=12).map(|i| format!("({i})")).collect();
    seed(table, "amount INTEGER", &format!("VALUES {}", rows.join(",")));

    let view = cx.update(|cx| {
        let view = cx.new(InsightView::new);
        // 真实入口会 `set_target`（面板据此进加载态并发请求）；这里补上这一步，
        // 再直接调宿主侧的落点——两步合起来就是「列头右键 → 出数」的全程。
        view.update(cx, |view, cx| view.set_target(column_target(table), cx));
        request_profile(&view, None, request_of(table), cx);
        view
    });
    cx.run_until_parked();

    cx.update(|cx| match view.read(cx).state() {
        InsightPanelState::Data(profile) => {
            assert_eq!(profile.column, "amount");
            assert_eq!(profile.kind, ColumnKind::Numeric);
            assert_eq!(profile.total_count, 12);
        }
        other => panic!("后台取数应回填为数据态，实际：{other:?}"),
    });
}

/// 失败路径：目标不存在时，面板拿到的是**可读文案 + 不可重试**（不是 `CoreError` 原文）
#[gpui_kit::test]
fn missing_table_reaches_the_panel_as_a_friendly_error(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let table = "t_insight_entry_absent";

    let view = cx.update(|cx| {
        let view = cx.new(InsightView::new);
        view.update(cx, |view, cx| view.set_target(column_target(table), cx));
        request_profile(&view, None, request_of(table), cx);
        view
    });
    cx.run_until_parked();

    cx.update(|cx| match view.read(cx).state() {
        InsightPanelState::Error {
            message,
            retryable,
        } => {
            assert_eq!(message, "结果集已失效或已过期，请重新执行查询");
            assert!(!retryable, "结果集没了不该给重试");
            assert!(!message.contains('['), "不得展示内部错误码：{message}");
        }
        other => panic!("应为错误态，实际 {other:?}"),
    });
}
