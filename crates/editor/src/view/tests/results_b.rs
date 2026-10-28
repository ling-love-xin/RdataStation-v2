//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::{
    TestAppContext,
};

use crate::channel::ExecChannel;
use crate::execution::{self, QueryData, QueryRunner};
use crate::export::{self, ExportFormat, ExportScope};
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::{
    grid_rows, open_panel, result_tabs, run_statement, shared_with_segment_runner,
    shared_with_sized_runner, shared_with_toolbar_runner, wait_for_all_pending,
};
use super::MoreUnsupportedRunner;
use super::dialogs::{FakeConnections, option, dialog_button_rendered};

// ===== B15：本地筛选 / 排序 =====

/// 【B15】筛选只作用于视图：网格里只剩命中的行，工具栏明示“已筛选 N / M 行”，清除后原样回来
#[gpui_kit::test]
fn filtering_hides_non_matching_rows_and_reports_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(grid_rows(cx, &panel), 3, "三行都看得见");

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.set_filter_for_test("1", cx)));
    assert_eq!(grid_rows(cx, &panel), 1, "只剩命中的那行（值 = 1）");
    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test(cx))
        .expect("有工具栏");
    assert_eq!(
        toolbar.filtered,
        Some((1, 3)),
        "工具栏要明示“已筛选 1 / 3 行”"
    );
    assert!(
        toolbar
            .segments()
            .iter()
            .any(|segment| segment == "已筛选 1 / 3 行"),
        "统计跟随筛选：{:?}",
        toolbar.segments()
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-filter-clear"),
        "有筛选词就摆 ✕（能一键回去）"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.set_filter_for_test("", cx)));
    assert_eq!(grid_rows(cx, &panel), 3, "清除筛选：三行都回来");
    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test(cx))
        .expect("有工具栏");
    assert_eq!(toolbar.filtered, None, "没筛选就不摆那一段");
}

/// 【B15】筛选是 300ms 防抖的（连打不会每敲一个字都重算）
#[gpui_kit::test]
fn filtering_waits_for_the_debounce(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    cx.update(|_window, cx| {
        // Dock 化后筛选框与防抖在面板实体（host 只转发面板实体给测试）
        let result_panel = panel.read(cx).result_panel_for_test();
        result_panel.update(cx, |p, cx| p.on_filter_input("1".to_string(), cx))
    });
    assert_eq!(grid_rows(cx, &panel), 3, "刚敲下去还没应用（防抖窗口内）");

    // 测试里的 timer 是**虚拟时钟**：推进 400ms 越过防抖窗口，再让就绪的任务跑起来
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    assert_eq!(grid_rows(cx, &panel), 1, "防抖到点后筛选生效");
}

/// 【B15】导出跟随筛选（原型 §5.5：导出的就是当前筛选后的行集，并且要明示）
#[gpui_kit::test]
fn export_follows_the_filter(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3");
    let path = export_temp_path("filtered", "csv");
    std::fs::remove_file(&path).ok();
    attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.set_filter_for_test("1", cx)));
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Csv, cx)
        });
    });

    let text = std::fs::read_to_string(&path).expect("要真的写出来");
    assert_eq!(text, "n\n1", "导出的是筛选后的行（三行里只剩值 1 的那行）");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("导出回执");
    assert!(
        message.contains("已筛选"),
        "回执要说明是筛选后的结果：{message}"
    );
    std::fs::remove_file(&path).ok();
}

/// 【B7 切片三】`SQL Updates` 落盘用**表结构端口**给的键列（菜单与落盘同一份 key）
#[gpui_kit::test]
fn export_sql_updates_uses_key_columns_from_the_table_port(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select * from orders");
    // 注入假连接（绑定后 `bound_connection()` 才有值——键列查询按连接走）
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    // 注入假表结构端口：orders 表的主键是 n（与结果列一致；note 列可 SET）
    shared.attach_table_keys(std::rc::Rc::new(OrdersKeyPort));
    let path = export_temp_path("updates", "sql");
    std::fs::remove_file(&path).ok();
    attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });

    run_statement(
        cx,
        &panel,
        "select * from orders",
        execution::ResultPlacement::Replace,
    );
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::SqlUpdates, cx)
        });
    });

    let text = std::fs::read_to_string(&path).expect("要真的写出来");
    assert!(
        text.lines().all(|line| line.starts_with("UPDATE")),
        "有键列时 SQL Updates 真编码成 UPDATE 语句，而不是回绝"
    );
    assert!(text.contains("WHERE n = 1"), "键列来自端口（n）");
    assert!(text.contains("SET note = 'a'"), "非键列进 SET");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("导出回执");
    assert!(message.contains("SQL Updates"), "回执说清格式：{message}");
    std::fs::remove_file(&path).ok();
}

// ===== B14：下发源库 =====

/// 假执行器（B14）：记下发筛选收到的原料，并回一条**带提示**的结果
struct PushdownRunner {
    /// 每次“下发筛选”收到的 `(原 SQL, 筛选词, 列名)`
    seen: std::sync::Arc<std::sync::Mutex<Vec<(String, String, Vec<String>)>>>,
    /// 每次“排序下发”收到的 `(列名, 是否降序)`
    sorted: std::sync::Arc<std::sync::Mutex<Vec<(String, bool)>>>,
}

impl QueryRunner for PushdownRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        Ok(QueryData {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![vec!["1".to_string(), "orders".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 3,
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

    fn run_filtered(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        sql: &str,
        filter: &str,
        columns: &[String],
    ) -> Result<QueryData, String> {
        self.seen
            .lock()
            .expect("锁")
            .push((sql.to_string(), filter.to_string(), columns.to_vec()));
        Ok(QueryData {
            columns: columns.to_vec(),
            rows: vec![vec!["2".to_string(), "orders_archive".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 9,
            truncated: false,
            affected_rows: None,
            has_more: false,
            // 下发的执行器会带上“已去掉 LIMIT”这类说明
            notice: Some("已去掉原查询的 LIMIT 10（下发筛选要能查到全部行）".to_string()),
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }

    fn run_sorted_down(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        column: &str,
        descending: bool,
    ) -> Result<QueryData, String> {
        self.sorted
            .lock()
            .expect("锁")
            .push((column.to_string(), descending));
        Ok(QueryData {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![vec!["2".to_string(), "orders_archive".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 11,
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
}

/// 【B14】下发源库：开关打开且已有筛选词 → 立刻重查，结果落**新结果集**
#[gpui_kit::test]
fn pushdown_re_runs_on_the_source_as_a_new_result_set(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(PushdownRunner {
        seen: seen.clone(),
        sorted: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }));
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(shared.results().set_count(&id), 1, "先是本地那一份");

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_filter_for_test("orders", cx);
            panel.set_pushdown(true, cx);
            // Dock 化：开关翻转只入队 PushdownFilter，drain 才真正重查（对齐生产事件路径）
            panel.drain_result_actions(cx);
        });
    });
    wait_for_all_pending(cx, &panel);

    let seen = seen.lock().expect("锁").clone();
    assert_eq!(seen.len(), 1, "打开开关且已有筛选词 → 立刻下发一次");
    assert_eq!(
        seen[0].0, "select 1",
        "下发用的是原 SQL（改写是执行器的事）"
    );
    assert_eq!(seen[0].1, "orders", "筛选词原样过去");
    assert_eq!(
        seen[0].2,
        vec!["id".to_string(), "name".to_string()],
        "结果集的列名要带过去（拼条件要用）"
    );
    assert_eq!(
        shared.results().set_count(&id),
        2,
        "下发产生新结果集，原结果保留（原型 §5.5）"
    );
    // 【B15】血缘：这份是“下发筛选”来的（工具栏的来源段读它；与“原查询”分得开）
    assert_eq!(
        shared
            .results()
            .sets(&id)
            .last()
            .and_then(|entry| entry.lineage.as_deref()),
        Some("下发筛选")
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("执行器带的说明要显示出来");
    assert!(message.contains("已去掉"), "{message}");
}

/// 【B14】开关关着（默认）：筛选只在本地，不打扰源库
#[gpui_kit::test]
fn local_filtering_does_not_touch_the_source(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(PushdownRunner {
        seen: seen.clone(),
        sorted: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }));
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.set_filter_for_test("orders", cx)));
    wait_for_all_pending(cx, &panel);

    assert!(
        seen.lock().expect("锁").is_empty(),
        "没开下发就不该重查源库"
    );
    assert_eq!(shared.results().set_count(&id), 1, "本地筛选不产生新结果集");
}

/// 【B14】排序下发：按用户点的那一列重查，结果落新结果集
#[gpui_kit::test]
fn sorting_down_re_runs_on_the_source(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sorted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(PushdownRunner {
        seen: seen.clone(),
        sorted: sorted.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    assert!(
        cx.update(|_window, cx| grid.read(cx).delegate().has_sort_down_hook()),
        "面板要把「排序下发」的钩子接上（右键菜单才有这两项）"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.sort_down("name", true, cx)));
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        sorted.lock().expect("锁").clone(),
        [("name".to_string(), true)],
        "列名与方向原样给执行器（改写是它的事）"
    );
    assert_eq!(
        shared.results().set_count(&id),
        2,
        "排序下发也产生新结果集，原结果保留"
    );
}

/// 【A2】刷新**下发筛选**结果：带当初的筛选原料重查（保持形态，不退回全量）
///
/// 刷新前的结果集是「下发筛选」产物（`sql` 仍是原 SQL），刷新必须**再一次下发**，
/// 而不是重跑原查询把用户看到的筛选结果变回全量。
#[gpui_kit::test]
fn refreshing_a_filtered_result_keeps_its_shape(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(PushdownRunner {
        seen: seen.clone(),
        sorted: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }));
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_filter_for_test("orders", cx);
            panel.set_pushdown(true, cx);
            // Dock 化：开关翻转只入队 PushdownFilter，drain 才真正重查（对齐生产事件路径）
            panel.drain_result_actions(cx);
        });
    });
    wait_for_all_pending(cx, &panel);
    assert_eq!(shared.results().set_count(&id), 2, "下发产生新结果集");
    assert_eq!(seen.lock().expect("锁").len(), 1, "下发执行了一次");

    // 下发产生的新结果集已被选中（2026-10-27：每次执行切到最新），直接刷新
    cx.update(|_window, _cx| {
        shared.update_results(|store| {
            assert_eq!(store.active_index(&id), Some(1), "下发结果集已选中");
        });
    });
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.refresh_active_result(cx));
    });
    wait_for_all_pending(cx, &panel);

    let seen = seen.lock().expect("锁").clone();
    assert_eq!(seen.len(), 2, "刷新 = 再一次下发（不是退回原查询）");
    assert_eq!(
        seen[1],
        (
            "select 1".to_string(),
            "orders".to_string(),
            vec!["id".to_string(), "name".to_string()],
        ),
        "筛选原料（词 + 列名）原样带过去"
    );
}

/// 【A2】刷新**排序下发**结果：带当初的列与方向重查（保持形态）
#[gpui_kit::test]
fn refreshing_a_sorted_result_keeps_its_shape(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let sorted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(PushdownRunner {
        seen: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        sorted: sorted.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.sort_down("name", true, cx)));
    wait_for_all_pending(cx, &panel);
    assert_eq!(sorted.lock().expect("锁").len(), 1, "排序下发执行了一次");

    // 排序下发产生的新结果集已被选中（2026-10-27：每次执行切到最新），直接刷新
    cx.update(|_window, _cx| {
        shared.update_results(|store| {
            assert_eq!(store.active_index(&id), Some(1), "排序下发结果集已选中");
        });
    });
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.refresh_active_result(cx));
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        sorted.lock().expect("锁").clone(),
        [
            ("name".to_string(), true),
            ("name".to_string(), true),
        ],
        "刷新 = 再一次排序下发（列与方向原样）"
    );
}

/// 【B14】执行器不支持下发：留一句可读原因（不静默）
#[gpui_kit::test]
fn pushdown_without_support_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_filter_for_test("a", cx);
            panel.set_pushdown(true, cx);
            // Dock 化：开关翻转只入队 PushdownFilter，drain 才真正重查（对齐生产事件路径）
            panel.drain_result_actions(cx);
        });
    });
    wait_for_all_pending(cx, &panel);

    // 失败的是**新结果集**（不抢选中）→ 标签上有红点，状态栏也要说一句
    let tabs = result_tabs(cx, &panel);
    assert_eq!(tabs.len(), 2, "下发失败也留一份（能被看见）");
    assert!(
        tabs.iter().any(|(_, failed)| *failed),
        "失败的那份要带标记：{tabs:?}"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("不支持下发筛选"), "{message}");
}

/// 【B14】按值筛选（右键菜单的落点）：值写进筛选框就生效
#[gpui_kit::test]
fn filtering_by_value_from_the_menu_works(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    assert!(
        cx.update(|_window, cx| grid.read(cx).delegate().has_filter_value_hook()),
        "面板要把「按值筛选」的钩子接上（右键菜单才有这一项）"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.apply_filter_value("1", cx)));
    assert_eq!(grid_rows(cx, &panel), 1, "按值筛出那一行");
    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test(cx))
        .expect("有工具栏");
    assert_eq!(toolbar.filtered, Some((1, 3)), "工具栏明示已筛选");
}

/// 【M8】「洞察此列」的钩子只在**宿主接了端口**时装上；结果不可取样时菜单项也不出现
#[gpui_kit::test]
fn insight_column_hook_follows_the_host_port_and_the_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    // ① 宿主没接端口：面板不装钩子（菜单里就没有这一项）
    let (shared, id) = shared_with_sized_runner("select rows=3");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    assert!(
        !cx.update(|_window, cx| grid.read(cx).delegate().has_insight_column_hook()),
        "宿主没接端口时不该装钩子"
    );

    // ② 接了端口：装钩子；但这份结果没绑定连接 → 仍不给入口
    let (shared, id) = shared_with_sized_runner("select rows=3");
    shared.attach_insight_column(std::rc::Rc::new(
        |_request: crate::shared::InsightColumnRequest, _cx: &mut gpui_kit::App| {},
    ));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    assert!(
        cx.update(|_window, cx| grid.read(cx).delegate().has_insight_column_hook()),
        "宿主接了端口就要装钩子"
    );
    let entry = shared.results_active(&id).expect("应有当前结果").clone();
    assert!(
        entry.connection.is_none(),
        "这个假执行器不会绑定连接——用它验“跟随活动连接”那一档"
    );
    assert!(
        entry.can_insight_column(),
        "条目本身没问题（成功 / 有列 / 只读查询）"
    );
    assert!(
        !cx.update(|_window, cx| grid.read(cx).delegate().insight_available()),
        "但假执行器报不出活动连接 → 入口不该出现（取样得知道回哪条连接）"
    );
}

/// 【B15】冻结列：原生 `Column.fixed` 真的落到列定义上（改完要重建列组）
#[gpui_kit::test]
fn freezing_a_column_pins_it_left(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    let before = cx.update(|_window, cx| grid.read(cx).delegate().column(1, cx).fixed);
    assert_eq!(before, None, "数据列默认不钉");

    cx.update(|_window, cx| {
        grid.update(cx, |state, cx| {
            state.delegate_mut().toggle_freeze(0);
            state.refresh(cx);
        })
    });
    assert!(
        cx.update(|_window, cx| grid.read(cx).delegate().is_frozen(0)),
        "冻结状态在 delegate 里"
    );
    let pinned = cx.update(|_window, cx| grid.read(cx).delegate().column(1, cx).fixed);
    assert_eq!(
        pinned,
        Some(gpui_kit::component::table::ColumnFixed::Left),
        "冻结要落成 `Column.fixed`（原生能力，不手搓）"
    );
}

// ===== B7：导出 =====

/// 假导出路径选择器：记下（格式，默认文件名），返回给定路径（`None` = 用户取消）
type PickedExports = std::sync::Arc<std::sync::Mutex<Vec<(ExportFormat, String)>>>;

pub(crate) fn attach_export_picker(shared: &EditorShared, path: Option<PathBuf>) -> PickedExports {
    let seen: PickedExports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = seen.clone();
    shared.attach_export_path_picker(std::rc::Rc::new(move |format, name| {
        recorder.lock().expect("锁").push((format, name));
        path.clone()
    }));
    seen
}

/// 【B7 切片三】假表结构端口：orders 表主键 = n（测试用；见
/// `export_sql_updates_uses_key_columns_from_the_table_port`）
struct OrdersKeyPort;

impl crate::export::TableKeysPort for OrdersKeyPort {
    fn primary_key_columns(&self, _conn_id: &str, table: &str) -> Option<Vec<String>> {
        (table == "orders").then(|| vec!["n".to_string()])
    }
}

/// 导出用的临时文件路径（不写产品目录；`.cargo/config.toml` 把临时目录钉在仓库内）
pub(crate) fn export_temp_path(tag: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rds_export_{tag}_{}.{extension}",
        std::process::id()
    ))
}

/// 选中结果集导出（仅已抓取）：问一次路径、写出的内容与提示都是真值
#[gpui_kit::test]
fn exporting_the_fetched_rows_writes_the_chosen_file(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1");
    let path = export_temp_path("csv", "csv");
    let picked = attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Csv, cx)
        });
    });

    let asked = picked.lock().expect("锁").clone();
    assert_eq!(asked.len(), 1, "导出要问一次路径");
    assert_eq!(asked[0].0, ExportFormat::Csv, "格式要原样给到对话框");
    assert_eq!(
        asked[0].1, "result_1.csv",
        "猜不出表名时用结果序号当默认名（`select 1` 里没有 FROM）"
    );
    let text = std::fs::read_to_string(&path).expect("文件要真的写出来");
    assert_eq!(text, "n,note\n1,a", "CSV 是表头 + 那一行");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("导出成功要留回执");
    assert!(message.contains("已导出 1 行"), "{message}");
    assert!(
        message.contains(path.to_string_lossy().as_ref()),
        "回执要带上落盘位置：{message}"
    );
    std::fs::remove_file(&path).ok();
}

/// 用户在对话框里取消：什么都不做（不弹提示、不落文件）——与另存为同口径
#[gpui_kit::test]
fn cancelling_the_export_changes_nothing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1");
    let path = export_temp_path("cancel", "csv");
    std::fs::remove_file(&path).ok();
    let picked = attach_export_picker(&shared, None);
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Csv, cx)
        });
    });

    assert_eq!(picked.lock().expect("锁").len(), 1, "取消也要先问过路径");
    assert!(!path.exists(), "取消不该落文件");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.is_none(), "取消不是错误，不留提示：{message:?}");
}

/// 没有注入路径选择器：明确报“未接入”（不静默失败）
#[gpui_kit::test]
fn exporting_without_a_picker_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result(ExportFormat::Csv, cx)
        });
    });

    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留原因");
    assert!(message.contains("未接入"), "{message}");
}

/// 抓全量后导出：先把剩下的段抓完，再落盘（json 里是全部 4 行）
#[gpui_kit::test]
fn exporting_everything_fetches_the_rest_first(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_segment_runner("select n from t");
    let path = export_temp_path("all", "json");
    let picked = attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    // 首段两行且“还有下一段”——这时点「抓全量后导出」
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result_all(ExportFormat::Json, cx)
        });
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "导出前要把剩下的段取回来（offset = 已抓到的行数）"
    );
    let picked = picked.lock().expect("锁").clone();
    assert_eq!(
        picked.as_slice(),
        [(ExportFormat::Json, "t.json".to_string())],
        "默认名从 SQL 里的表名来"
    );
    let text = std::fs::read_to_string(&path).expect("抓完就该落盘");
    assert_eq!(
        text.matches("\"n\":").count(),
        4,
        "导出的应当是四行（首段 2 + 取回 2）：{text}"
    );
    assert!(text.contains("\"4\""), "最后一段的值也在：{text}");
    assert_eq!(shared.results().set_count(&id), 1, "抓全量不该新开结果集");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        4,
        "抓到的东西留着（用户没白等）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("落盘回执");
    assert!(message.contains("已导出 4 行"), "{message}");
    std::fs::remove_file(&path).ok();
}

/// 抓全量中途取段失败：**不落半截文件**，已经抓到的行也不动
#[gpui_kit::test]
fn a_failed_segment_cancels_the_export(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(MoreUnsupportedRunner));
    let id = shared
        .open(OpenRequest::untitled("select n from t", EditorMode::Sql))
        .id()
        .clone();
    let path = export_temp_path("failed", "csv");
    std::fs::remove_file(&path).ok();
    attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.export_active_result_all(ExportFormat::Csv, cx)
        });
    });
    wait_for_all_pending(cx, &panel);

    assert!(!path.exists(), "取段失败就不该落一个半截文件");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要留原因");
    assert!(message.contains("不支持分段抓取"), "{message}");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        1,
        "已抓到的行不动"
    );
}

/// 菜单项随“还有没有下一段”长出来（面板投影与纯函数对得上）
#[gpui_kit::test]
fn the_export_menu_grows_with_a_next_segment(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _asked) = shared_with_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert!(status.has_more, "这条假执行器首段拿满");

    // 面板投影出来的两项真值（菜单就是按它们长的）
    let items = export::menu_items(status.has_more, "2");
    let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();
    assert!(
        labels.contains(&"已抓取 2 行".to_string()),
        "还有下一段时要先报已抓多少：{labels:?}"
    );
    assert!(
        labels.contains(&"抓全量后导出（会重跑查询）".to_string()),
        "还有下一段才有第二组：{labels:?}"
    );
    assert!(
        items
            .iter()
            .any(|item| item.action == Some((ExportFormat::Csv, ExportScope::All))),
        "第二组指向“抓全量”：{labels:?}"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-export"),
        "有网格就摆导出"
    );

    // 抓到底之后：第二组消失（不再摆一个多余的重跑入口）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.fetch_more(cx)));
    wait_for_all_pending(cx, &panel);
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("还有状态行");
    assert!(!status.has_more, "第二段没拿满 → 到底了");
    let labels: Vec<String> = export::menu_items(status.has_more, "4")
        .into_iter()
        .map(|item| item.label)
        .collect();
    assert_eq!(
        labels,
        ["CSV", "JSON", "INSERT", "Parquet", "XLSX"],
        "到底了只给“仅已抓取”那几项"
    );
}

// ══════════════════════════════════════════════════════════════════════
// B13：执行通道（源库 / 本地加速 / 联邦）
