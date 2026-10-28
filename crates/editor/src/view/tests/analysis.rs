//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::rc::Rc;

use gpui_kit::component::table::TableEvent;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{
    AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext,
};

use crate::channel::ExecChannel;
use crate::completion::{Candidate, CandidateKind, Catalog};
use crate::connection::SchemaInfo;
use crate::execution::{self, CommitReport, QueryData, QueryRunner};
use crate::export::RowKey;
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::EditorHostPanel;

use super::{
    bind_editor_keys, open_panel, run_statement,
    shared_with_document, wait_for_all_pending,
    wait_for_result,
};

use super::channels::{FakeCompletion, FakeProject};
use super::dialogs::{dialog_harness, dialog_button_rendered};

// ═══════════════════════════════════════════════════════════════════════

/// 假执行器（B15）：普通执行回一份**带两行**的结果（分析要桥接它），
/// `analyze` 记下收到的请求并回一条按桥接行数算的聚合结果
struct AnalysisRunner {
    seen: std::sync::Arc<std::sync::Mutex<Vec<crate::analysis::AnalysisRequest>>>,
}

impl QueryRunner for AnalysisRunner {
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
                vec!["1".to_string(), "a".to_string()],
                vec!["2".to_string(), "b".to_string()],
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

    fn analyze(&self, request: &crate::analysis::AnalysisRequest) -> Result<QueryData, String> {
        self.seen.lock().expect("锁").push(request.clone());
        Ok(QueryData {
            columns: vec!["行数".to_string()],
            rows: vec![vec![request.bridged_rows().to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 2,
            truncated: false,
            affected_rows: None,
            has_more: false,
            // 与宿主同一口径：说明由执行器带回来
            notice: Some(request.notice()),
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

/// 【B15】分析：SQL 与**已抓到的行**原样到执行器，结果落新结果集并带分析标记
#[gpui_kit::test]
fn analysis_sends_the_sql_and_the_grabbed_rows(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(AnalysisRunner { seen: seen.clone() }));
    let id = shared
        .open(OpenRequest::untitled(
            "select id, tag from t",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select id, tag from t",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(shared.results().set_count(&id), 1, "先有一份结果才能分析");

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.run_analysis("SELECT count(*) AS \"行数\" FROM {table}".to_string(), cx)
        })
    });
    wait_for_all_pending(cx, &panel);

    let seen = seen.lock().expect("锁").clone();
    assert_eq!(seen.len(), 1, "预置项直接跑，不再问一遍");
    assert!(
        seen[0].sql.contains("{table}"),
        "占位符留给执行器替换：{}",
        seen[0].sql
    );
    assert_eq!(
        seen[0].columns,
        vec!["id".to_string(), "tag".to_string()],
        "列名照搬结果集"
    );
    assert_eq!(
        seen[0].rows,
        vec![
            vec!["1".to_string(), "a".to_string()],
            vec!["2".to_string(), "b".to_string()],
        ],
        "桥接的是已抓到的行"
    );
    assert_eq!(seen[0].dropped_rows, 0);

    assert_eq!(
        shared.results().set_count(&id),
        2,
        "分析落新结果集（原结果一行不动）"
    );
    // 落位口径与 B2/B10 一致：新一份**追加在后面**，不抢用户正在看的那份（不就地替换）
    let store = shared.results();
    let sets = store.sets(&id);
    let entry = sets.last().expect("分析结果应当在末尾");
    assert!(entry.analysis, "分析结果要带标记（界面据此不摆“刷新”）");
    assert_eq!(
        entry.title.as_deref(),
        Some(crate::analysis::ANALYSIS_TITLE),
        "标签上直接写“分析”"
    );
    assert_eq!(entry.columns, vec!["行数".to_string()]);
    assert_eq!(
        entry.lineage.as_deref(),
        Some("本地分析"),
        "血缘要说清这份不是原查询（工具栏的来源段）"
    );
    assert_eq!(store.active_index(&id), Some(1), "分析结果最新被选中（2026-10-27：每次执行切到最新）");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("说明要留一句“基于多少行”");
    assert!(message.contains("基于已抓取的 2 行"), "{message}");
}

/// 【B15】空 SQL 不提交：本地就说清，不去打扰执行器
#[gpui_kit::test]
fn an_empty_analysis_sql_is_refused_locally(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(AnalysisRunner { seen: seen.clone() }));
    let id = shared
        .open(OpenRequest::untitled(
            "select id, tag from t",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select id, tag from t",
        execution::ResultPlacement::Replace,
    );

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.run_analysis("   \n".to_string(), cx))
    });
    wait_for_all_pending(cx, &panel);

    assert!(seen.lock().expect("锁").is_empty(), "没写东西就不该提交");
    assert_eq!(shared.results().set_count(&id), 1, "不产生结果集");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要留一句可读的原因");
    assert!(message.contains("为空"), "{message}");
}

/// 【B15 切片二】自定义分析 SQL：对话框真打开、输入框与运行按钮真在
///
/// 点击按架构 §12 #29 的口径不赌（headless 下对话框的坐标与命中测试对不上），
/// 落地入口（带上 SQL 调 [`EditorHostPanel::run_analysis`]）由上一条用例盖住。
#[gpui_kit::test]
fn the_custom_analysis_dialog_opens_with_a_sql_box(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(AnalysisRunner {
        seen: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }));
    let id = shared
        .open(OpenRequest::untitled(
            "select id, tag from t",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-analysis");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    run_statement(
        cx,
        &panel,
        "select id, tag from t",
        execution::ResultPlacement::Replace,
    );

    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.request_custom_analysis(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "自定义分析要先问一条 SQL"
    );
    assert!(
        cx.debug_bounds("editor-analysis-sql").is_some(),
        "输入框要真渲染（不是只有状态）"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-analysis-run"),
        "运行按钮要真在"
    );

    // 没有结果集时不该弹框（没有行可桥接）：换成一份干净文档验证
    let empty_id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    let empty_panel = cx.update(|window, cx| {
        cx.new(|cx| EditorHostPanel::new(shared.clone(), empty_id, window, cx))
    });
    cx.update(|window, cx| {
        empty_panel.update(cx, |panel, cx| panel.request_custom_analysis(window, cx))
    });
    let message = cx
        .update(|_window, cx| empty_panel.read(cx).message.clone())
        .expect("要留一句可读的原因");
    assert!(message.contains("没有可分析的结果"), "{message}");
}

// ---------- 【T2a-4】单元格编辑：双击进入 / Enter 提交 / Esc 取消 / 失焦提交 ----------

/// orders 表假执行器：返回 `id, name` 两列两行（编辑判据要的是“结果里有主键列”）
struct OrdersRunner;

impl QueryRunner for OrdersRunner {
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
            rows: vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["2".to_string(), "bob".to_string()],
            ],
            column_types: Vec::new(),
            elapsed_ms: 5,
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

/// 【T2a-4】编辑就绪的共享状态：orders 结果（探测端口给主键）+ 绑定连接
///
/// 探测链路真实走通：执行回执入库前 `probe_primary_key` 从补全端口取主键列，
/// 判据（`cell_editable`）据此放行——不手动塞字段，测的就是生产路径。
fn edit_ready_shared(content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(OrdersRunner));
    shared.attach_completion(Rc::new(FakeCompletion {
        catalog: Catalog {
            objects: vec![Candidate::new("public.orders", CandidateKind::Table)],
            columns: vec![],
            truncated: false,
        },
        templates: vec![] as Vec<crate::completion::TemplateSnippet>,
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_connection(&id, Some("P_orders".to_string())));
    (shared, id)
}

/// 把焦点交给正在编辑的编辑框（渲染出编辑框后调用）
///
/// 时机说明：进入编辑时 `render_edit_cell` 已把焦点指给编辑框，但**焦点事件要到
/// 下一帧才派发**（gpui 的 focus 更新在 draw 时落地），直接打字会丢在窗口级。
/// 这里再显式聚焦一次并补一帧，让打字稳定落在编辑框里。
fn focus_edit_input(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let handle = cx.update(|_window, cx| {
        panel
            .read(cx)
            .grid_for_test()
            .read(cx)
            .delegate()
            .edit_input()
            .expect("宿主已注入编辑框")
            .read(cx)
            .focus_handle(cx)
    });
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 双击进入编辑 → 打字同步 → Enter 提交落变更集（链路：原生事件 + 内核事件 + 钩子）
#[gpui_kit::test]
fn cell_edit_enters_on_double_click_and_commits_on_enter(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = edit_ready_shared("select * from orders");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 执行 → 结果带主键（探测链路真实走通）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);
    assert!(
        shared
            .results()
            .active(&id)
            .is_some_and(|entry| entry.row_key().is_some()),
        "orders 结果应探测到主键 id"
    );

    // 双击 (0, 2)：组件列 0 是行号槽，列 2 = 数据列 1（name）→ 进入编辑
    cx.update(|_window, cx| {
        panel.read(cx).grid_for_test().update(cx, |_state, cx| {
            cx.emit(TableEvent::DoubleClickedCell(0, 2));
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx)); // 渲染编辑框（初值同步）
    assert!(
        cx.update(|_window, cx| {
            panel
                .read(cx)
                .grid_for_test()
                .read(cx)
                .delegate()
                .editing()
                .is_some()
        }),
        "双击后应进入编辑"
    );
    focus_edit_input(cx, &panel);

    // 打字：值变化同步到编辑态（`InputEvent::Change` → `sync_edit_value`）
    cx.simulate_keystrokes("2");
    let input_value = cx.update(|_window, cx| {
        panel
            .read(cx)
            .grid_for_test()
            .read(cx)
            .delegate()
            .edit_input()
            .map(|input| input.read(cx).value().to_string())
    });
    let value = cx.update(|_window, cx| {
        panel
            .read(cx)
            .grid_for_test()
            .read(cx)
            .delegate()
            .editing()
            .map(|edit| edit.value.clone())
    });
    assert_eq!(
        value.as_deref(),
        Some("alice2"),
        "输入应同步到编辑态（编辑框实体值={input_value:?}）"
    );

    // Enter → 提交 → 变更集（`PressEnter` → `commit_edit` → 钩子 → 权威存储）
    cx.simulate_keystrokes("enter");
    let change = shared
        .results()
        .change_set(&id, 0)
        .and_then(|changes| changes.pending().into_iter().next().cloned());
    assert_eq!(
        change.map(|c| (c.row, c.column, c.original, c.edited)),
        Some((0, 1, "alice".to_string(), "alice2".to_string())),
        "提交后权威变更集应有一条 (0,1) alice→alice2"
    );
}

/// Esc 取消：进了编辑、改了字，按 Esc 丢弃，变更集不动
#[gpui_kit::test]
fn cell_edit_esc_cancels_without_touching_changes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = edit_ready_shared("select * from orders");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    // 进编辑（列 2 = name）→ 聚焦编辑框 → 打字 → Esc
    cx.update(|_window, cx| {
        panel.read(cx).grid_for_test().update(cx, |_state, cx| {
            cx.emit(TableEvent::DoubleClickedCell(1, 2));
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    focus_edit_input(cx, &panel);
    cx.simulate_keystrokes("x");
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 编辑态清空、变更集为空（Esc 取消 = 丢弃输入值）
    assert!(
        !cx.update(|_window, cx| {
            panel
                .read(cx)
                .grid_for_test()
                .read(cx)
                .delegate()
                .editing()
                .is_some()
        }),
        "Esc 后应退出编辑"
    );
    assert_eq!(
        shared
            .results()
            .change_set(&id, 0)
            .map(|changes| changes.pending_count()),
        Some(0),
        "Esc 取消不该落任何变更"
    );
}

// 失焦提交：`PressEnter | Blur` 在订阅里共用**同一提交分支**（commit_edit + 脏格投影），
// 其逻辑已被 `cell_edit_enters_on_double_click_and_commits_on_enter` 完整覆盖
// （同一分支、同一代码路径）。
//
// 为什么不写窗口级失焦用例：headless 里 `window.focus(&other_handle)` 只改窗口焦点
// 跟踪（`is_focused` 立即翻转），但**不派发元素级焦点事件**——`on_blur` 回调不触发、
// `InputEvent::Blur` 不 emit（实测补多帧 draw 仍无回调）。这是测试环境限制；真实窗口
// 焦点切换会派发（Input 内核的上下文菜单关闭等同样依赖它）。

// ---------- 【T2a-6】批量提交：四道防线拦截 / 回执组装成功与冲突 / 失败留痕 ----------

/// orders + 可控提交回执的假执行器：`run` 与 [`OrdersRunner`] 同数据；`commit_changes`
/// 返回测试预设的回执（成功带逐条影响行数 / 失败带原因）
struct CommitOrdersRunner {
    commit_result: std::sync::Mutex<Result<CommitReport, String>>,
}

impl QueryRunner for CommitOrdersRunner {
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
            rows: vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["2".to_string(), "bob".to_string()],
            ],
            column_types: Vec::new(),
            elapsed_ms: 5,
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

    fn commit_changes(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _table: &str,
        _key: &RowKey,
        _updates: &str,
    ) -> Result<CommitReport, String> {
        self.commit_result.lock().unwrap().clone()
    }
}

/// 提交就绪：orders 结果 + 可控提交回执的执行器 + 绑定连接（编辑判据 / 探测链路同
/// [`edit_ready_shared`]；提交回执由测试预设）
fn commit_ready_shared(
    content: &str,
    commit_result: Result<CommitReport, String>,
) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(CommitOrdersRunner {
        commit_result: std::sync::Mutex::new(commit_result),
    }));
    shared.attach_completion(Rc::new(FakeCompletion {
        catalog: Catalog {
            objects: vec![Candidate::new("public.orders", CandidateKind::Table)],
            columns: vec![],
            truncated: false,
        },
        templates: vec![] as Vec<crate::completion::TemplateSnippet>,
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_connection(&id, Some("P_orders".to_string())));
    (shared, id)
}

/// 直塞一格变更到权威变更集（测提交防线与回执组装，不重复走双击链路）
fn seed_cell_change(
    shared: &EditorShared,
    id: &DocumentId,
    row: usize,
    column: usize,
    original: &str,
    edited: &str,
) {
    shared.update_results(|store| {
        if let Some(index) = store.active_index(id) {
            store.mutate_change_set(id, index, |changes| {
                changes.upsert(crate::changes::CellChange::pending(
                    row, column, original, edited,
                ));
            });
        }
    });
}

/// 等提交回执被泵消化（真实提交线程 → 回执队列 → 泵 `drain_commit_notes`；带超时）
fn wait_commit_done(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.commit_in_flight_for_test().is_none()
            })
        });
        if done {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "提交回执迟迟没回来");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 执行出 orders 结果并等回填（T2a-6 用例共用开头）
fn run_orders_result(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, panel);
}

/// 提交成功：影响行数全 1 → 变更集全部移出待提交、无冲突；状态栏如实报数
#[gpui_kit::test]
fn commit_receipt_marks_committed_on_success(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared(
        "select * from orders",
        Ok(CommitReport {
            committed: 2,
            skipped: 0,
            per_statement: vec![1, 1],
            conflicts: vec![],
        }),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);

    // 两格待提交（行 0 列 1、行 1 列 1 = name 列）
    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");
    seed_cell_change(&shared, &id, 1, 1, "bob", "bob2");

    // 提交请求被接受 → 记为在途（索引与语句→行/列映射都在，回执组装的原料）
    let ok = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).is_ok())
    });
    assert!(ok, "有变更 + 源库通道 + 可编辑，提交应被接受");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).commit_in_flight_for_test())
            .map(|(set, per_line, skipped)| (set, per_line, skipped)),
        Some((0, vec![(0, vec![1]), (1, vec![1])], 0)),
        "在途状态应带结果集下标与语句→行/列映射"
    );

    wait_commit_done(cx, &panel);

    // 全成功：两格都不再待提交、没有冲突；状态栏如实报数
    let store = shared.results();
    let set = store.change_set(&id, 0).expect("变更集在");
    assert_eq!(set.pending_count(), 0, "成功的变更应全部移出待提交");
    assert!(!set.has_conflicts(), "全成功不该有冲突");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("状态栏有消息");
    assert_eq!(message, "已提交 2 条变更", "{message}");
}

/// 冲突：影响行数 [1, 0] → 第一行标已提交、第二行标冲突；状态栏报"已提交 1 条变更（1 条冲突）"
#[gpui_kit::test]
fn commit_receipt_marks_conflicts_and_reports(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared(
        "select * from orders",
        Ok(CommitReport {
            committed: 1,
            skipped: 0,
            per_statement: vec![1, 0],
            conflicts: vec![],
        }),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);

    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");
    seed_cell_change(&shared, &id, 1, 1, "bob", "bob2");

    assert!(
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).is_ok())
        }),
        "提交应被接受"
    );
    wait_commit_done(cx, &panel);

    // 行 0 已提交（移出待提交）、行 1 冲突（留在冲突区给用户看，不是待提交也不是已提交）
    let store = shared.results();
    let set = store.change_set(&id, 0).expect("变更集在");
    assert_eq!(set.pending_count(), 0, "冲突与已提交都不算待提交");
    assert!(set.has_conflicts(), "有冲突要明示");
    assert_eq!(set.conflicts(), vec![(1, 1)], "冲突格应标在数据行 1 列 1");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("状态栏有消息");
    assert!(message.contains("已提交 1 条变更"), "{message}");
    assert!(message.contains("1 条冲突"), "{message}");
}

/// 提交失败：执行器报错 → 在途清空、变更集原样保留（用户可再提交），状态栏报失败原因
#[gpui_kit::test]
fn commit_failure_leaves_changes_intact(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared(
        "select * from orders",
        Err("连接已断开".to_string()),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);

    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");

    assert!(
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).is_ok())
        }),
        "请求本身要成功（失败在执行器那一步，不是提交被拦）"
    );
    wait_commit_done(cx, &panel);

    let store = shared.results();
    let set = store.change_set(&id, 0).expect("变更集在");
    assert_eq!(set.pending_count(), 1, "失败后变更保持待提交，用户可重试");
    assert!(!set.has_conflicts(), "失败不该被当成冲突");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("状态栏有消息");
    assert!(message.contains("批量提交失败：连接已断开"), "{message}");
}

/// 提交前防线：连接只读 / 项目只读 / 通道只读挂载 → 提交被拦（变更不离开）
#[gpui_kit::test]
fn submit_pending_changes_respects_three_guards(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared("select * from orders", Ok(CommitReport::default()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);
    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");

    // ① 连接只读（文档的连接只读维）
    shared.update(|service| {
        service.set_read_only(
            &id,
            crate::model::ReadOnly {
                editor: false,
                connection: true,
            },
        )
    });
    let reason = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).unwrap_err())
    });
    assert!(reason.contains("连接为只读"), "{reason}");

    // ② 项目只读（项目锁那半）
    shared.update(|service| service.set_read_only(&id, crate::model::ReadOnly::none()));
    shared.attach_project(Rc::new(FakeProject { read_only: true }));
    let reason = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).unwrap_err())
    });
    assert!(reason.contains("项目为只读"), "{reason}");
    shared.attach_project(Rc::new(FakeProject { read_only: false }));

    // ③ 通道只读挂载（加速档对源库是只读挂载，写回一律拒）
    shared.update(|service| service.set_channel(&id, ExecChannel::Accelerated));
    let reason = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).unwrap_err())
    });
    assert!(reason.contains("源库通道"), "{reason}");

    // 变更全程没离开（防线拦截后仍待提交）
    let store = shared.results();
    let set = store.change_set(&id, 0).expect("变更集在");
    assert_eq!(set.pending_count(), 1, "被拦的提交不能把变更弄丢");
}

/// 无行标识：跑一份探测不到主键的结果（unknown_t 不在补全端口的主键表里）→ 整份只读
#[gpui_kit::test]
fn submit_pending_changes_refuses_without_row_key(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared("select * from unknown_t", Ok(CommitReport::default()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);
    assert!(
        shared
            .results()
            .active(&id)
            .map_or(true, |entry| entry.row_key().is_none()),
        "unknown_t 结果应探测不到主键"
    );

    // 直塞一格变更（绕过编辑判据，只测提交防线）→ 提交被拦
    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");
    let reason = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).unwrap_err())
    });
    assert!(reason.contains("没探测到主键"), "{reason}");
}

/// 没有待提交变更：提交入口直接说清楚（按钮不该亮）
#[gpui_kit::test]
fn submit_pending_changes_refuses_without_changes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared("select * from orders", Ok(CommitReport::default()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);

    let reason = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).unwrap_err())
    });
    assert!(reason.contains("没有待提交的变更"), "{reason}");
}

/// T2a-7①：结果工具栏的提交/回滚对只在有**待提交**变更时出现（参考 DBeaver）；
/// 计数来自权威变更集（改 N 格 = 提交 N 处）
#[gpui_kit::test]
fn commit_controls_appear_with_pending_changes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared("select * from orders", Ok(CommitReport::default()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);

    // 没改格：按钮组不出现
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-commit-group").is_none(),
        "无变更时提交/回滚对不该出现"
    );

    // 改两格 → 按钮组出现；计数 = 权威变更集的待提交条数
    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");
    seed_cell_change(&shared, &id, 1, 1, "bob", "bob2");
    // 测试直塞变更不走编辑提交路径，投影需手动刷新（生产路径由提交回调自动 sync）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.sync_result_view(cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // 2026-10-28 commit 组迁 `toolbar::ToolbarGroup`：ToolbarGroup 是 RenderOnce 无
    // debug_selector（debug_bounds 查不到、find 需 test_support 注册也查不到），面板在
    // 组件外层挂了无样式观测壳 div（debug_selector 锚点），断言仍走 debug_bounds
    assert!(
        cx.debug_bounds("editor-result-commit-group").is_some(),
        "有待提交变更时提交/回滚对应出现"
    );
    let count = shared
        .results()
        .change_set(&id, 0)
        .expect("变更集在")
        .pending_count();
    assert_eq!(count, 2, "按钮计数来源应为权威变更集的待提交条数");
}

/// T2a-7②：提交请求被接受（在途）后，提交中再提交被拦（按钮置灰的行为侧）；
/// 回执全部成功回来后变更清空、按钮组消失（对齐 DBeaver：提交完就没有可提交的）
#[gpui_kit::test]
fn commit_controls_submit_then_vanish_on_receipt(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared(
        "select * from orders",
        Ok(CommitReport {
            committed: 1,
            skipped: 0,
            per_statement: vec![1],
            conflicts: vec![],
        }),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);
    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");

    // 提交请求被接受 → 在途；提交中再提交被拦（置灰对应的行为：不重复提交）
    assert!(
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).is_ok())
        }),
        "有变更时应接受提交"
    );
    let reason = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.submit_pending_changes(cx).unwrap_err())
    });
    assert!(reason.contains("已有提交在进行中"), "{reason}");

    // 回执回来：提交成功 → 没有待提交 → 按钮组消失
    wait_commit_done(cx, &panel);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-commit-group").is_none(),
        "提交全部成功后按钮组应消失"
    );
}

/// T2a-7③：撤销（回滚侧）→ 待提交变更全部丢弃、按钮组消失；冲突项保留（是证据，不抹掉）
#[gpui_kit::test]
fn discard_controls_clear_pending_and_keep_conflicts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = commit_ready_shared("select * from orders", Ok(CommitReport::default()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_orders_result(cx, &panel);
    // 预置：1 条待提交 + 1 条冲突（冲突模拟提交回执留下的证据）
    seed_cell_change(&shared, &id, 0, 1, "alice", "alice2");
    shared.update_results(|store| {
        store.mutate_change_set(&id, 0, |changes| {
            changes.upsert(crate::changes::CellChange::pending(1, 1, "bob", "bob2"));
            changes.mark_conflict(1, 1);
        });
    });
    assert_eq!(
        shared.results().change_set(&id, 0).map(|c| c.pending_count()),
        Some(1),
        "预置：待提交 1 条 + 冲突 1 条"
    );

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.discard_pending_changes(cx));
    });
    let store = shared.results();
    let set = store.change_set(&id, 0).expect("变更集在");
    assert_eq!(set.pending_count(), 0, "撤销应清空待提交");
    assert_eq!(set.conflicts(), vec![(1, 1)], "冲突是证据，回滚不该抹掉");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-commit-group").is_none(),
        "撤销后没有待提交变更，按钮组应消失"
    );
}

// ---------------------------------------------------------------------------
// T2c-1 连接 ▸ 库 ▸ Schema 面包屑（状态栏最左；可切换，参考 DBeaver）
// ---------------------------------------------------------------------------

/// T2c-1 假 schema 端口：内存列表，渲染路径可调（不做 I/O）
pub(crate) struct FakeSchemas(pub(crate) Vec<SchemaInfo>);

impl crate::connection::SchemasPort for FakeSchemas {
    fn schemas(&self, _conn_id: &str) -> Vec<SchemaInfo> {
        self.0.clone()
    }
}

pub(crate) fn schema_doc(shared: &EditorShared, id: &DocumentId) -> Option<String> {
    shared.document_schema(id)
}

/// T2c-1①：面包屑在「SQL 模式 + 绑定连接 + 有 schema 列表」时渲染；
/// 切 schema = 文档绑定真变；「使用连接默认」= 解绑（None）
#[gpui_kit::test]
fn breadcrumb_renders_and_switches_schema_binding(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let shared = EditorShared::new();
    shared.attach_schemas(Rc::new(FakeSchemas(vec![
        SchemaInfo {
            catalog: Some("rdb".to_string()),
            schema: "public".to_string(),
        },
        SchemaInfo {
            catalog: Some("rdb".to_string()),
            schema: "analytics".to_string(),
        },
    ])));
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_connection(&id, Some("P_orders".to_string())));

    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 【#47】面包屑已上抛全局状态栏（「连接 ▸ Schema」文本段）：编辑器面板内部不再渲染它
    assert!(
        cx.debug_bounds("editor-breadcrumb").is_none(),
        "面包屑上抛后，编辑器面板内部不应再渲染它"
    );
    // 未选 schema：文档绑定保持 None（不拿列表第一个假装是当前）；全局状态栏数据源同口径
    assert_eq!(schema_doc(&shared, &id), None);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).status_snapshot(cx))
            .unwrap()
            .schema,
        None,
        "未选 schema 时全局状态栏快照 schema 应为 None"
    );

    // 菜单项点击与直接调 set_schema 是同一函数（headless 弹菜单不可靠，走提交函数纪律）：
    // 切到 analytics → 文档绑定真变 + 快照可见
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_schema(Some("analytics".to_string()), cx);
        });
    });
    assert_eq!(
        schema_doc(&shared, &id).as_deref(),
        Some("analytics"),
        "切 schema 必须落进文档绑定（不是纯 UI 装饰）"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).status_snapshot(cx))
            .unwrap()
            .schema
            .as_deref(),
        Some("analytics"),
        "全局状态栏快照要随绑定变化"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 「使用连接默认」= 解绑：绑定回 None
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_schema(None, cx);
        });
    });
    assert_eq!(
        schema_doc(&shared, &id),
        None,
        "「使用连接默认」应解绑文档级 schema"
    );
}

/// T2c-1②：没有 schema 列表（端口未接 / 连接无缓存）→ 面包屑不渲染
/// （退化为文本连接段——`labels` 的 connection 段兜底，不造一个假控件）
#[gpui_kit::test]
fn breadcrumb_absent_without_schema_port(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = shared_with_document(r"D:\sql\report.sql", "select 1");
    shared.update(|service| service.set_connection(&id, Some("P_orders".to_string())));
    let (_panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.debug_bounds("editor-breadcrumb").is_none(),
        "没有 schema 列表就不该有面包屑控件（文本连接段兜底）"
    );
}

/// T2c-1③：文本模式不显示面包屑（无 execute 能力 → 无连接概念）
#[gpui_kit::test]
fn breadcrumb_absent_in_text_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let shared = EditorShared::new();
    shared.attach_schemas(Rc::new(FakeSchemas(vec![SchemaInfo {
        catalog: Some("rdb".to_string()),
        schema: "public".to_string(),
    }])));
    let id = shared
        .open(OpenRequest::untitled("随便记点什么", EditorMode::General))
        .id()
        .clone();
    shared.update(|service| service.set_connection(&id, Some("P_orders".to_string())));
    let (_panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.debug_bounds("editor-breadcrumb").is_none(),
        "文本模式是纯记事本：没有执行就没有面包屑"
    );
}

/// T2c-4：缩进重设真改文档状态 + 内核调用不 panic（重设菜单走「渲染断言 + 直调函数」纪律）
#[gpui_kit::test]
fn indent_reset_changes_document_and_renders(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = shared_with_document(r"D:\sql\indent.sql", "select 1");
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 默认（#48 设置默认：空格 × 4，非制表符；测试环境无 Settings global 走出厂默认 4）
    assert_eq!(shared.document_tab_size(&id), 4, "默认缩进宽度");
    assert!(!shared.document_hard_tabs(&id), "默认不用制表符");

    // 重设空格宽度为 4：文档状态真变 + 渲染正常（内核 set_tab_size 调用不 panic）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_tab_size(4, cx));
    });
    assert_eq!(shared.document_tab_size(&id), 4, "重设要落到文档上");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切制表符：状态真变 + 宽度保留 + 渲染正常
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_hard_tabs(true, cx));
    });
    assert!(shared.document_hard_tabs(&id), "制表符开关要落到文档上");
    assert_eq!(shared.document_tab_size(&id), 4, "切制表符保留空格宽度");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

