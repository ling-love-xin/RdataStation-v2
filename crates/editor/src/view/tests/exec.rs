//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::{
    Entity, Focusable as _, TestAppContext, VisualTestContext,
};

use crate::channel::ExecChannel;
use crate::execution::{self, QueryData, QueryRunner};
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::EditorHostPanel;

use super::{
    bind_editor_keys, grid_rows, open_panel, result_tabs,
    shared_with_document, shared_with_runner,
    shared_with_sized_runner, wait_for_all_pending,
    wait_for_result, wait_until,
};
use super::SeenConnections;
use super::dialogs::dialog_button_rendered;

#[gpui_kit::test]
fn ctrl_enter_runs_the_statement_under_the_cursor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id, seen, _seen_conn) = shared_with_runner(
        "select 1;
select 2;
select boom;",
        EditorMode::Sql,
    );
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 光标停在第二句里（键位路径仍走真按键）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(11, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    assert_eq!(
        seen.lock().expect("锁").as_slice(),
        ["select 2".to_string()],
        "执行的应当是光标所在的那一句"
    );

    // 结果进了权威存储（数字都是真实值）
    let stored = shared
        .results()
        .active(&id)
        .map(|entry| (entry.row_count(), entry.columns.len(), entry.summary()));
    assert_eq!(stored, Some((2, 1, "2 行 × 1 列 · 5 ms".to_string())));

    // 界面：结果行 + 网格里的行 + 不 panic 的一帧
    let summary = cx.update(|_window, cx| {
        panel
            .read(cx)
            .result_summary_for_test(cx)
            .map(|text| text.to_string())
    });
    // 【B15】工具栏左段最后跟来源摘要（血缘）：普通执行就是「原查询」
    assert_eq!(summary.as_deref(), Some("行数 2 · 耗时 5 ms · 原查询"));
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        2,
        "网格要拿到行"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn ctrl_shift_enter_runs_the_whole_script(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (_shared, id, seen, _seen_conn) = shared_with_runner(
        "select 1;
select 2;",
        EditorMode::Sql,
    );
    let (panel, cx) = open_panel(cx, &_shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-shift-enter");
    wait_for_result(cx, &panel);

    assert_eq!(
        seen.lock().expect("锁").as_slice(),
        ["select 1;
select 2;"
            .to_string()],
        "“执行全部”发的是整篇脚本"
    );
}

#[gpui_kit::test]
fn a_failing_execution_gets_an_error_card_instead_of_an_empty_grid(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id, _seen, _seen_conn) = shared_with_runner("select boom;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    // 【B5】工具栏只说“失败”（原型 §2.4 的 `行数 │ 耗时 │ 连接名` 里失败占一段）
    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test(cx))
        .expect("失败也要有结果区文案");
    assert!(summary.contains("失败"), "{summary}");
    // 【B6】原因在**错误卡片**里（原型 §2.4「错误呈现」），不是一句灰字
    let card = cx
        .update(|_window, cx| panel.read(cx).result_error_card_for_test(cx))
        .expect("失败要有错误卡片");
    assert!(card.message.contains("boom"), "{}", card.message);
    assert!(
        card.location.is_none(),
        "这条错误认不出位置，就不给定位按钮"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-error-card"),
        "卡片要真的画出来"
    );
    assert!(
        !dialog_button_rendered(cx, "editor-result-error-locate"),
        "认不出位置就不摆定位按钮"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        0,
        "失败不该有网格行"
    );
    // 失败原因同时写进结果存储与状态栏
    let stored = shared
        .results()
        .active(&id)
        .and_then(|entry| entry.error.clone());
    assert!(stored.is_some(), "错误要进 ResultStore");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.expect("状态栏也要说原因").contains("boom"));
}

// ===== B2：批量执行与多结果集 =====

/// 批量：三句语句 → 三个结果集，**失败不中断**且失败的那份带标记
#[gpui_kit::test]
fn a_batch_lands_three_statements_in_three_result_sets(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=1;\nselect boom;\nselect rows=3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 走菜单同样的落地入口（菜单点击在 headless 下命中测试不可靠：目标解析已有纯函数单测）
    let target = execution::batch_target("select rows=1;\nselect boom;\nselect rows=3;");
    assert_eq!(target.statements().len(), 3, "先确认切出了三句");
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(target, execution::ResultPlacement::NewSet, cx)
        })
    });
    wait_for_all_pending(cx, &panel);

    let tabs = result_tabs(cx, &panel);
    assert_eq!(
        tabs,
        vec![
            ("结果 1".to_string(), false),
            ("结果 2".to_string(), true),
            ("结果 3".to_string(), false),
        ],
        "每句一个结果集，中间那句失败要标出来"
    );
    assert_eq!(shared.results().set_count(&id), 3, "三份结果都在权威存储里");
    // 失败不中断：第三句真的跑了（否则它不会有自己的结果集）
    // 2026-10-27：每次执行（含批量）都会切到最新结果集
    assert_eq!(grid_rows(cx, &panel), 3, "默认选中最新一份（第三句）");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 点开第三份：网格跟着换（headless 下点击不稳，直接驱动落地入口）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(2, cx)));
    assert_eq!(grid_rows(cx, &panel), 3, "第三句的结果真的落地了");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test(cx)),
        2
    );
    // 界面上真的画出了标签条（两份以上结果才画）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

// ===== 【并排对比】双栏并排 =====

/// 两份以上结果 → 开对比模式：双栏并排，右栏自动选中「除主选中外最近一份」
#[gpui_kit::test]
fn compare_mode_shows_two_side_by_side_panes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 跑三份结果（第一份替换、后两份追加），主选中保持第 0 份
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx)));
    wait_for_result(cx, &panel);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select rows=3".to_string()),
                execution::ResultPlacement::NewSet,
                cx,
            )
        })
    });
    wait_for_all_pending(cx, &panel);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select rows=5".to_string()),
                execution::ResultPlacement::NewSet,
                cx,
            )
        })
    });
    wait_for_all_pending(cx, &panel);
    assert_eq!(result_tabs(cx, &panel).len(), 3, "三份结果齐了");

    // 开对比（驱动入口；按钮接线由真机 / 选择器验证，headless 下直接走入口）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_compare_mode(cx)));
    assert!(
        cx.update(|_window, cx| panel.read(cx).compare_mode_for_test(cx)),
        "开关已开"
    );
    assert_eq!(
        shared.results().compare_index(&id),
        Some(1),
        "自动选了除主选中（最新，2）外最近一份（1）"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).compare_grid_rows_for_test(cx)),
        3,
        "对比栏摆上了第二份的数据"
    );
    assert_eq!(grid_rows(cx, &panel), 5, "主栏还是最新一份（r5）");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切主选中到第一份：对比对象不变（对比是显式选择，不随主选中切换）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(0, cx)));
    assert_eq!(
        shared.results().compare_index(&id),
        Some(1),
        "对比对象不跟着主选中走"
    );
    assert_eq!(grid_rows(cx, &panel), 1, "主栏切到第一份");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 右栏标签条换对比对象：点某份 = 切过去；点当前对比项 = 取消对比
#[gpui_kit::test]
fn compare_pane_pick_switches_or_clears_the_compare_set(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx)));
    wait_for_result(cx, &panel);
    for sql in ["select rows=3", "select rows=5"] {
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.execute(
                    execution::ExecTarget::Statement(sql.to_string()),
                    execution::ResultPlacement::NewSet,
                    cx,
                )
            })
        });
        wait_for_all_pending(cx, &panel);
    }
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_compare_mode(cx)));
    // 主选中 = 最新（r5，index 2）；自动选除它外最近一份（r3，index 1）
    assert_eq!(shared.results().compare_index(&id), Some(1));

    // 右栏标签条点第一份：对比对象切过去（行数跟着换）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_compare_set(0, cx)));
    assert_eq!(shared.results().compare_index(&id), Some(0), "切到第一份");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).compare_grid_rows_for_test(cx)),
        1
    );

    // 再点当前对比项：取消对比（右栏回空态，但对比模式还开着）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_compare_set(0, cx)));
    assert_eq!(shared.results().compare_index(&id), None, "再点 = 取消");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).compare_grid_rows_for_test(cx)),
        0,
        "右栏回到空态"
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).compare_mode_for_test(cx)),
        "对比模式本身还开着"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 关闭对比：开关复位、对比选中清掉、右栏消失
#[gpui_kit::test]
fn closing_compare_mode_resets_the_switch_and_clears_the_compare_set(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx)));
    wait_for_result(cx, &panel);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select rows=3".to_string()),
                execution::ResultPlacement::NewSet,
                cx,
            )
        })
    });
    wait_for_all_pending(cx, &panel);
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_compare_mode(cx)));
    assert!(cx.update(|_window, cx| panel.read(cx).compare_mode_for_test(cx)));
    // 主选中 = 最新（r3，index 1）；自动选除它外最近一份（r1，index 0）
    assert_eq!(shared.results().compare_index(&id), Some(0));

    // 关掉：开关复位、对比选中清掉（下次开对比重新选）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_compare_mode(cx)));
    assert!(!cx.update(|_window, cx| panel.read(cx).compare_mode_for_test(cx)));
    assert_eq!(shared.results().compare_index(&id), None, "对比选中跟着清掉");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 「在新结果标签中执行」：新结果放旁边，**原结果集仍选中**（原型 §4.4）
#[gpui_kit::test]
fn running_into_a_new_set_keeps_the_previous_one_selected(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=2;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 第一次：普通执行（替换语义）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx)));
    wait_for_result(cx, &panel);
    assert_eq!(grid_rows(cx, &panel), 2);
    assert_eq!(result_tabs(cx, &panel).len(), 1);

    // 第二次：同文档、新结果集
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select rows=5".to_string()),
                execution::ResultPlacement::NewSet,
                cx,
            )
        })
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(shared.results().set_count(&id), 2, "新的一份追加在后面");
    assert_eq!(result_tabs(cx, &panel).len(), 2, "两份结果 → 标签条该出现");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test(cx)),
        1,
        "新结果被选中（2026-10-27：每次执行切到最新结果）"
    );
    assert_eq!(grid_rows(cx, &panel), 5, "网格是最新一份（r5）的数据");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切到旧的一份：网格与选中态一起跟着走
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(0, cx)));
    assert_eq!(grid_rows(cx, &panel), 2);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test(cx)),
        0
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_summary_for_test(cx)),
        Some("行数 2 · 耗时 3 ms · 原查询".to_string()),
        "状态行跟着选中的结果集走"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

// ===== B3：中断与超时 =====

/// 假执行器（B3）：卡在 `run` 里等中断（模拟慢查询；驱动侧由取消令牌打断）
struct BlockingRunner {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 假执行器真的进去跑了吗
    ///
    /// “提交”不等于“驱动在跑”：工作线程要先取到作业、再读中断标记，然后才进 `run`。
    /// 中断的用例必须等它真的跑起来，否则测到的是“还没开始的那条被标已取消”
    /// （那是另一条语义，由批量用例钉住）。
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cancels: SeenConnections,
}

impl QueryRunner for BlockingRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        self.started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !self.stop.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(std::time::Instant::now() < deadline, "假执行器没等到中断");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Err("Query cancelled".to_string())
    }

    fn cancel(&self, connection: Option<&str>) -> Result<bool, String> {
        self.cancels
            .lock()
            .expect("锁")
            .push(connection.map(str::to_string));
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(true)
    }
}

/// 带可中断假执行器的共享状态 + 一份文档
fn shared_with_blocking_runner(
    content: &str,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    SeenConnections,
) {
    let shared = EditorShared::new();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancels: SeenConnections = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(BlockingRunner {
        stop: stop.clone(),
        started: started.clone(),
        cancels: cancels.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, stop, started, cancels)
}

/// 中断：慢查询就此结束（结果集报错），中断请求真的到了执行器，跑完不再计时
#[gpui_kit::test]
fn interrupting_a_slow_query_ends_it_with_a_visible_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id, stop, started, cancels) = shared_with_blocking_runner("select slow;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select slow".to_string()),
                execution::ResultPlacement::Replace,
                cx,
            )
        })
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).pending_for_test()),
        1
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).elapsed_for_test())
            .is_some(),
        "执行中就该有耗时（状态栏的“执行中 3.4s…”靠它）"
    );
    // 执行中画一帧：状态栏那个 ■ 中断 按钮真的渲染（布局/借用问题会在这里暴露）
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 等它**真的跑起来**再中断：中断要打断的是“跑着的查询”
    wait_until(
        || started.load(std::sync::atomic::Ordering::SeqCst),
        "假执行器开始跑",
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.interrupt(cx)));
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("中断要有回执").contains("已请求中断"),
        "点了中断必须看得见"
    );

    wait_for_all_pending(cx, &panel);
    assert_eq!(
        cancels.lock().expect("锁").as_slice(),
        [None],
        "未绑定连接的文档取消的就是“当前活动连接”"
    );
    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test(cx))
        .expect("中断也要有结果区文案");
    assert!(summary.contains("失败"), "{summary}");
    // 原因在错误卡片里（原型 §2.4）
    let card = cx
        .update(|_window, cx| panel.read(cx).result_error_card_for_test(cx))
        .expect("中断要有错误卡片");
    assert!(card.message.contains("cancel"), "{}", card.message);
    assert!(
        cx.update(|_window, cx| panel.read(cx).elapsed_for_test())
            .is_none(),
        "跑完就不再计时"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // 释放仍在自旋的假执行器（它已经返回了，这里只是让变量活着）
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// 没在跑时点中断：回绝并说原因（“点了没反应”不可接受）
#[gpui_kit::test]
fn interrupting_when_nothing_runs_says_why(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (_shared, id, _stop, _started, _cancels) = shared_with_blocking_runner("select 1;");
    let (panel, cx) = open_panel(cx, &_shared, &id);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).pending_for_test()),
        0
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.interrupt(cx)));
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("回绝也要说原因").contains("没有执行"),
        "没在跑就该直说"
    );
}

// ===== B4：事务 =====

/// 假执行器（B4）：支持事务，记录执行选项与事务动作
struct TxRunner {
    actions: std::sync::Arc<std::sync::Mutex<Vec<(crate::execution::TxAction, Option<String>)>>>,
    options: std::sync::Arc<std::sync::Mutex<Vec<crate::execution::RunOptions>>>,
    open: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl QueryRunner for TxRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        options: crate::execution::RunOptions,
    ) -> Result<QueryData, String> {
        self.options.lock().expect("锁").push(options);
        if options.use_transaction {
            // 模拟引擎：自动提交关掉时，执行会先开一个事务
            self.open.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 1,
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

    fn transaction_snapshot(&self, _connection: Option<&str>) -> crate::execution::TxSnapshot {
        crate::execution::TxSnapshot {
            in_transaction: self.open.load(std::sync::atomic::Ordering::SeqCst),
        }
    }

    fn transaction(
        &self,
        connection: Option<&str>,
        action: crate::execution::TxAction,
    ) -> Result<(), String> {
        self.actions
            .lock()
            .expect("锁")
            .push((action, connection.map(str::to_string)));
        let open = match action {
            crate::execution::TxAction::Begin => true,
            crate::execution::TxAction::Commit | crate::execution::TxAction::Rollback => false,
        };
        self.open.store(open, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn supports_transactions(&self) -> bool {
        true
    }
}

/// 带事务假执行器的共享状态 + 一份文档（返回记录用的两个句柄）
#[allow(clippy::type_complexity)]
fn shared_with_tx_runner(
    content: &str,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::Mutex<Vec<crate::execution::RunOptions>>>,
    std::sync::Arc<std::sync::Mutex<Vec<(crate::execution::TxAction, Option<String>)>>>,
) {
    let shared = EditorShared::new();
    let actions = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let options = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(TxRunner {
        actions: actions.clone(),
        options: options.clone(),
        open: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, options, actions)
}

/// 等事务动作的回执都回来了
fn wait_for_tx_idle(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.tx_pending_for_test() == 0
            })
        });
        if done {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "事务动作的回执迟迟没回来"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 自动提交关掉时，**下一次执行**要带上“进事务”，执行后的 TX 区也要跟着变
#[gpui_kit::test]
fn toggling_autocommit_puts_the_next_execution_in_a_transaction(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id, options, _actions) = shared_with_tx_runner("insert into t values (1);");
    let (panel, cx) = open_panel(cx, &shared, &id);

    assert!(
        cx.update(|_window, cx| panel.read(cx).autocommit_for_test()),
        "默认是自动提交"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test()),
        "TX 未开启"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_autocommit(cx)));
    assert!(
        !cx.update(|_window, cx| panel.read(cx).autocommit_for_test()),
        "点一下就关掉"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx)));
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        options.lock().expect("锁").as_slice(),
        [crate::execution::RunOptions {
            use_transaction: true
        }],
        "自动提交关 → 执行要进事务"
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test())
            .starts_with("TX 已开启"),
        "执行后停在事务里，状态栏要如实说"
    );
    // 画一帧：此时 TX 区应出现提交 / 回滚按钮（渲染路径不 panic）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 状态栏的事务动作：开始 → 提交，动作真的到执行器，TX 区跟着开合
#[gpui_kit::test]
fn transaction_actions_from_the_status_bar_show_up_in_the_tx_area(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id, _options, actions) = shared_with_tx_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.tx_action(crate::execution::TxAction::Begin, cx)
        })
    });
    wait_for_tx_idle(cx, &panel);
    assert!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test())
            .starts_with("TX 已开启"),
        "开始事务后 TX 区要开"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.tx_action(crate::execution::TxAction::Commit, cx)
        })
    });
    wait_for_tx_idle(cx, &panel);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test()),
        "TX 未开启",
        "提交后 TX 区要关"
    );
    assert_eq!(
        actions
            .lock()
            .expect("锁")
            .iter()
            .map(|(action, _)| *action)
            .collect::<Vec<_>>(),
        [
            crate::execution::TxAction::Begin,
            crate::execution::TxAction::Commit
        ],
        "两个动作都要真的送到执行器"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn text_mode_refuses_to_execute(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (_shared, id, seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::General);
    let (panel, cx) = open_panel(cx, &_shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-enter");

    assert!(
        seen.lock().expect("锁").is_empty(),
        "文本模式不得把 SQL 发给驱动"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.expect("拒绝要留原因").contains("文本模式"));
}

// ===== B16：参数绑定（DBeaver 式 `:name` → 弹表单 → 字面替换后执行）=====

/// 带参数的目标不直接提交：挂起等表单；填完替换后继续执行（替换后的 SQL 才发给驱动）
#[gpui_kit::test]
fn named_parameters_hold_execution_until_filled(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let sql = "SELECT * FROM t WHERE id = :id AND name = :name";
    let (shared, id, seen, _seen_conn) = shared_with_runner(sql, EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let submitted = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::All(sql.to_string()),
                execution::ResultPlacement::Replace,
                cx,
            )
        })
    });
    assert!(!submitted, "带参数的目标不直接提交");
    assert!(
        cx.update(|_window, cx| panel.read(cx).pending_bind_for_test()),
        "参数表单在挂起"
    );
    assert!(
        seen.lock().expect("锁").is_empty(),
        "SQL 还没发给驱动（在等表单）"
    );

    // 填完：替换（含单引号转义）后继续原提交链
    let mut values = std::collections::HashMap::new();
    values.insert("id".to_string(), "7".to_string());
    values.insert("name".to_string(), "o'brien".to_string());
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.finish_bind(values, cx));
    });
    assert!(
        !cx.update(|_window, cx| panel.read(cx).pending_bind_for_test()),
        "填完清掉挂起"
    );
    // 修复并行 flaky（2026-10-29）：`finish_bind` 只负责提交，执行器工作线程写入
    // `seen` 在并行全量下可能滞后；断言前必须等它落盘（语义不变，只消除时序假设）。
    wait_until(|| !seen.lock().expect("锁").is_empty(), "执行器收到替换后的 SQL");
    let seen = seen.lock().expect("锁");
    assert_eq!(
        seen.as_slice(),
        &["SELECT * FROM t WHERE id = '7' AND name = 'o''brien'"],
        "替换后的 SQL 才发给驱动"
    );
}

/// 取消参数表单：清掉挂起，SQL 不发给驱动（「这次不跑了」不是只关个窗）
#[gpui_kit::test]
fn cancelling_bind_aborts_the_run(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let sql = "SELECT * FROM t WHERE id = :id";
    let (shared, id, seen, _seen_conn) = shared_with_runner(sql, EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement(sql.to_string()),
                execution::ResultPlacement::Replace,
                cx,
            )
        });
    });
    assert!(cx.update(|_window, cx| panel.read(cx).pending_bind_for_test()));

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.cancel_bind(cx));
    });
    assert!(
        !cx.update(|_window, cx| panel.read(cx).pending_bind_for_test()),
        "取消清掉挂起"
    );
    assert!(
        seen.lock().expect("锁").is_empty(),
        "取消后 SQL 不发给驱动"
    );
}

// ===== A11：查找 / 替换（内核能力，本 crate 不自建）=====

/// 建一份带内容的文档与面板（查找不需要执行器）
fn panel_with_text<'a>(
    cx: &'a mut TestAppContext,
    content: &str,
) -> (Entity<EditorHostPanel>, &'a mut VisualTestContext) {
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };
    (panel, cx)
}

#[gpui_kit::test]
fn the_kernel_find_panel_takes_over_on_ctrl_f(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (panel, cx) = panel_with_text(cx, "select 1;\nselect 2;");
    // 焦点给编辑内核（真机上用户就在这儿打字）
    let editor_handle = cx.update(|_window, cx| panel.read(cx).editor_focus_handle_for_test(cx));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.focus(&editor_handle, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // Ctrl+F 由内核处理（`input::Search` → 组件库的查找面板）。
    // 可观察信号：焦点被面板的查找框拿走 —— 这正是“查找栏打开了”的真实表现。
    cx.simulate_keystrokes("ctrl-f");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let focused = cx.update(|window, cx| window.focused(cx));
    assert!(
        focused.as_ref() != Some(&editor_handle),
        "Ctrl+F 之后焦点应当离开编辑内核（内核把焦点交给查找框）"
    );

    // 再画一帧、再切走再回来都不该 panic（查找面板是组件库的浮层）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn ctrl_h_opens_the_kernel_replace_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (panel, cx) = panel_with_text(cx, "select 1;");
    let editor_handle = cx.update(|_window, cx| panel.read(cx).editor_focus_handle_for_test(cx));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.focus(&editor_handle, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.simulate_keystrokes("ctrl-h");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let focused = cx.update(|window, cx| window.focused(cx));
    assert!(
        focused.as_ref() != Some(&editor_handle),
        "Ctrl+H 之后焦点应当离开编辑内核（替换行出现并聚焦）"
    );
}

// ===== A13：文件档位 =====

/// 造一个指定大小的稀疏文件（秒级，不真写满磁盘）
fn sparse_file(dir: &std::path::Path, name: &str, bytes: u64) -> std::path::PathBuf {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).expect("建文件");
    file.set_len(bytes).expect("扩到目标大小");
    drop(file);
    path
}

fn temp_dir_for(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_editor_tier_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

#[gpui_kit::test]
fn a_huge_file_is_not_read_into_memory_and_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir_for("huge");
    let path = sparse_file(&dir, "huge.sql", crate::limits::HUGE_FILE_BYTES + 4096);

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        let panel = panel.read(cx);
        assert!(
            !panel.is_editable_for_test(),
            "超大文件不做可编辑会话（只读）"
        );
        let notice = panel.tier_notice().expect("要有提示卡");
        assert!(notice.contains("未加载"), "{notice}");
    });

    // 内容确实是空的：200MB 没被读进来
    let content_len = cx.update(|_window, _cx| {
        shared
            .service()
            .find(&id)
            .map(|doc| doc.content().len())
            .unwrap_or(usize::MAX)
    });
    assert_eq!(content_len, 0, "超大文件不该被整份读进内存");

    // 提示卡在渲染路径上（画一帧不 panic）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn a_large_file_stays_editable_but_warns_about_heavy_features(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir_for("large");
    // 真写 50MiB 太慢；用稀疏文件 + 写一行内容：档位只看大小，不读全文件也能判定
    let path = sparse_file(&dir, "large.sql", crate::limits::LARGE_FILE_BYTES + 1024);

    let shared = EditorShared::new();
    let tier = crate::limits::tier_for_path(&path);
    assert_eq!(tier, crate::limits::FileTier::Large);
    assert!(tier.disables_completion(), "大文件关补全");

    // 用带档位的请求打开（跳过真读 50MiB：这里只验面板对档位的表现）
    let id = shared
        .open(OpenRequest::file(&path, "select 1;", EditorMode::Sql).with_tier(tier))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        let panel = panel.read(cx);
        assert!(panel.is_editable_for_test(), "大文件仍可编辑");
        assert!(
            panel.tier_notice().expect("要有提示").contains("补全"),
            "提示要说清关了什么"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn a_normal_file_has_no_notice_card(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir_for("normal");
    let path = dir.join("small.sql");
    std::fs::write(&path, "select 1;").expect("写盘");

    let (shared, id) = shared_with_document(path.to_string_lossy().as_ref(), "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };
    cx.update(|_window, cx| {
        assert!(
            panel.read(cx).tier_notice().is_none(),
            "常规文件不显示提示卡"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

// ═══════════════════════════════════════════════════════════════════════
// 对话框流程（A9）：模式切换确认
