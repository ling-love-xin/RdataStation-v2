//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::{
    TestAppContext, VisualTestContext,
};

use crate::execution::{self};
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::{
    bind_editor_keys, open_panel, run_statement, shared_with_runner, shared_with_segment_runner,
    shared_with_sized_runner, shared_with_toolbar_runner, wait_for_all_pending,
};
use super::LocatedFailureRunner;
use super::MoreUnsupportedRunner;
use super::dialogs::dialog_button_rendered;

// ===== B5：结果工具栏（复制 / 刷新 / 影响行数 / 截断 / 分栏）=====

/// 有网格就有复制，剪贴板里是与网格一致的 TSV
#[gpui_kit::test]
fn copying_the_active_result_puts_tsv_on_the_clipboard(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert!(
        dialog_button_rendered(cx, "editor-result-copy"),
        "有网格就该有复制入口"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.copy_active_result(cx)));
    let text = cx.update(|_window, app| app.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(
        text.as_deref(),
        Some("n\tnote\n1\ta"),
        "复制的是网格那份 TSV（表头 + 行）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("复制要有回执");
    assert!(message.contains("已复制 1 行"), "{message}");
}

/// 【B14】结果网格里的 `Ctrl+C`：选中**一格**给那一格的原文，升级成**整行**后给整行 TSV。
///
/// 全走真入口：`simulate_click` 点真单元格（组件自己判选格；再点同一格升级为整行——我们
/// 关掉了组件自带的窄行头，那条升级就是选整行的路），按键走 `simulate_keystrokes`，
/// 读的是真剪贴板。**焦点也是这条链的一环**：组件只在 Tab 导航里拿焦点（点格子不聚焦），
/// 所以网格那层的左键按下会自己把焦点交给表格——那段断了用例同样会挂。
#[gpui_kit::test]
fn ctrl_c_copies_the_selected_cell_then_the_whole_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    // 默认测试窗口矮，结果区分栏里可能一行都放不下：先开高一点再点单元格
    cx.simulate_resize(gpui_kit::Size {
        width: gpui_kit::px(900.),
        height: gpui_kit::px(700.),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let clipboard = |cx: &mut VisualTestContext| {
        cx.update(|_window, app| app.read_from_clipboard().and_then(|item| item.text()))
    };
    let message = |cx: &mut VisualTestContext| {
        cx.update(|_window, cx| panel.read(cx).message.clone())
            .unwrap_or_default()
    };

    // 先让剪贴板里有一份已知内容（工具栏那份整结果），这样“没东西可复制时不能动剪贴板”
    // 才是可断言的
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.copy_active_result(cx)));
    assert_eq!(clipboard(cx).as_deref(), Some("n\tnote\n1\ta"));

    // 点一个单元格（第 0 行 · `n` 列）：`Ctrl+C` 给那一格的**原文**（不拼 TSV）
    let cell = cx
        .debug_bounds("editor-result-cell-0-1")
        .expect("单元格要画出来（顺带证明网格真的渲染了）");
    cx.simulate_click(cell.center(), gpui_kit::Modifiers::default());
    cx.simulate_keystrokes("ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("1"));
    assert!(
        message(cx).contains("已复制单元格"),
        "复制要有回执：{}",
        message(cx)
    );

    // 再点同一格 → 组件把选择升级为整行：`Ctrl+C` 给整行 TSV（与右键「复制整行」同一份实现）
    cx.simulate_click(cell.center(), gpui_kit::Modifiers::default());
    cx.simulate_keystrokes("ctrl-c");
    assert_eq!(clipboard(cx).as_deref(), Some("1\ta"));
    assert!(message(cx).contains("已复制整行"), "{}", message(cx));

    // `Esc` 清掉选择（组件的 `Cancel`）后按 `Ctrl+C`：没东西可复制就要**说出来**，
    // 而不是静默什么也不做，也不能拿旧选择去凑一份（剪贴板保持上一步的内容）
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("ctrl-c");
    assert_eq!(
        clipboard(cx).as_deref(),
        Some("1\ta"),
        "清掉选择后不该再写剪贴板"
    );
    assert!(
        message(cx).contains("选一格或一行"),
        "没得复制要说清怎么选：{}",
        message(cx)
    );
}

/// 刷新重跑的是**当前选中那份**的 SQL，并且原位替换（结果集数不变）
#[gpui_kit::test]
fn refreshing_reruns_the_selected_result_in_place(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select first",
        execution::ResultPlacement::Replace,
    );
    run_statement(
        cx,
        &panel,
        "select second",
        execution::ResultPlacement::NewSet,
    );
    run_statement(
        cx,
        &panel,
        "select third",
        execution::ResultPlacement::NewSet,
    );
    assert_eq!(shared.results().set_count(&id), 3);

    // 用户切到最后一份，刷新它
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(2, cx)));
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.refresh_active_result(cx)));
    wait_for_all_pending(cx, &panel);

    let sqls = seen.lock().expect("锁").clone();
    assert_eq!(
        sqls,
        [
            "select first",
            "select second",
            "select third",
            "select third"
        ],
        "刷新重跑的是选中那份的 SQL"
    );
    assert_eq!(
        shared.results().set_count(&id),
        3,
        "原位替换：刷出不新开一份结果集"
    );
}

/// 写语句：工具栏报影响行数，且不摆复制（没有网格）
#[gpui_kit::test]
fn a_write_statement_reports_affected_rows_and_offers_no_copy(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("insert into t values (1);");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "insert into t values (1)",
        execution::ResultPlacement::Replace,
    );

    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test(cx))
        .expect("写语句也要有结果区文案");
    assert!(summary.starts_with("影响 3 行"), "{summary}");
    assert!(
        !dialog_button_rendered(cx, "editor-result-copy"),
        "写语句没有可复制的结果集"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-refresh"),
        "有 SQL 就能重跑"
    );
}

/// 截断是警告：工具栏拿到**真实行数**，且已抓到的行仍然可复制
#[gpui_kit::test]
fn truncation_is_reported_as_a_warning(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select truncated;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select truncated",
        execution::ResultPlacement::Replace,
    );

    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test(cx))
        .expect("有结果就有工具栏");
    assert_eq!(toolbar.rows, Some(3), "工具栏报行数");
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert_eq!(
        status.truncated_hint.as_deref(),
        Some("已截断至 3 行"),
        "截断提示要说清拿到多少行"
    );
    let (sql, can_copy) = cx.update(|_window, cx| panel.read(cx).result_actions_for_test(cx));
    assert!(can_copy, "被截断的结果集仍能复制已抓到的行");
    assert_eq!(sql.as_deref(), Some("select truncated"));
}

/// 结果网格的密度口径：行距走组件 `XSmall`（26px，`ui::RESULT_TABLE_SIZE`）——与 Mock 预览表同档。
///
/// 这条用例是**判别性**的：组件默认档是 32px（`Medium`），而原型稿当年写的是 22px；
/// 三个数里只有一个是实际生效的。行高画在组件的行容器上（量不到它的坐标），所以这里量
/// **两行行号槽的间距**（行高 + 行分隔线），不设 size 的实现会在这里报 32 上下。
#[gpui_kit::test]
fn the_result_grid_rows_are_compact(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    // 默认测试窗口小，分栏下半区可能只放得下一行：先把窗口开高一点，量两行的间距
    cx.simulate_resize(gpui_kit::Size {
        width: gpui_kit::px(900.),
        height: gpui_kit::px(700.),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let first = cx
        .debug_bounds("editor-result-rowno-0")
        .expect("行号槽的第一行应画出来（顺带证明网格真的渲染了）");
    let second = cx
        .debug_bounds("editor-result-rowno-1")
        .expect("第二行也在");
    let pitch = second.top() - first.top();
    assert!(
        pitch >= gpui_kit::px(26.) && pitch <= gpui_kit::px(27.),
        "行距 = 组件 XSmall（26px）+ 行分隔线；实测 {pitch:?}"
    );
}

/// 结果网格表头的排序入口**真点击**：箭头在表头右端，不是表头正文。
///
/// 与 Mock 预览表同一条口径：组件 0.7.1 的 `on_col_head_click` 只做**列选择**（本表关掉了），
/// 排序挂在 `render_sort_icon` 上。表头现在由 `render_th` 自画（截断 + 悬停全文 + 登记坐标），
/// 本用例同时看住「自画之后箭头还在」——箭头是组件画在自己那一层的，接管表头容易碰坏它。
#[gpui_kit::test]
fn the_result_header_sorts_from_its_arrow(cx: &mut TestAppContext) {
    const PROBE: [f32; 7] = [2., 6., 10., 14., 18., 22., 26.];

    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_sized_runner("select rows=3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select rows=3",
        execution::ResultPlacement::Replace,
    );
    cx.simulate_resize(gpui_kit::Size {
        width: gpui_kit::px(900.),
        height: gpui_kit::px(700.),
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // `debug_selector` 只是给用例找坐标（`.id(...)` 不登记坐标）
    for slot in ["editor-result-th-0", "editor-result-th-1"] {
        assert!(
            cx.debug_bounds(slot).is_some(),
            "{slot} 应登记坐标（行号槽也走同一个 render_th）"
        );
    }
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    let sort_state =
        |cx: &mut VisualTestContext| cx.update(|_window, cx| grid.read(cx).delegate().sort_state());
    assert_eq!(sort_state(cx), None, "还没点过就不该有排序");

    // 行号槽：右侧没有箭头，点多少次都不该有反应
    let slot = cx.debug_bounds("editor-result-th-0").expect("行号槽坐标");
    for offset in PROBE {
        cx.simulate_click(
            gpui_kit::Point::new(slot.right() + gpui_kit::px(offset), slot.center().y),
            gpui_kit::Modifiers::default(),
        );
    }
    assert_eq!(sort_state(cx), None, "行号槽不给排序");

    // 数据列：右端往左探，命中箭头即按这一列本地排序（组件给的第一下是降序）
    let head = cx.debug_bounds("editor-result-th-1").expect("数据列坐标");
    let mut clicked = false;
    for offset in PROBE {
        cx.simulate_click(
            gpui_kit::Point::new(head.right() + gpui_kit::px(offset), head.center().y),
            gpui_kit::Modifiers::default(),
        );
        if sort_state(cx).is_some() {
            clicked = true;
            break;
        }
    }
    assert!(clicked, "表头右端应有可点的排序箭头（探了 {PROBE:?}）");
    assert_eq!(
        sort_state(cx),
        Some((0, true)),
        "箭头给的是「这一列 + 降序」，本地排序只重排视图行序"
    );
    let first = cx.update(|_window, cx| grid.read(cx).delegate().visible_rows()[0].clone());
    assert_eq!(first, ["2"], "降序之后第一行是最大的那个（0,1,2 → 2,1,0）");
}

/// 结果区在可拖拽分栏里：有结果时出现在下半区，且编辑区没被挤掉
#[gpui_kit::test]
fn the_result_pane_sits_in_a_split_without_squeezing_the_editor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 还没有结果：编辑区独占宿主主体（Dock 化后结果区归底部 dock，宿主不再分栏）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let before = cx
        .debug_bounds("editor-code-area")
        .expect("编辑区在")
        .size
        .height;

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let after = cx
        .debug_bounds("editor-code-area")
        .expect("编辑区还在")
        .size
        .height;
    assert!(
        after.as_f32() > 0.0,
        "编辑区不能被结果区压成 0（实得 {after:?}）"
    );
    // 可拖拽分栏（2026-10-28 恢复 v_resizable：此前被固定高度 div 替换）：结果区
    // 占下半区，编辑区相应变矮——断言「确实分栏了」比旧断言（无分栏时的
    // `after >= before`）更贴近当前语义；拖拽高度由组件库 ResizableState 管理，
    // 上限 `RESULT_MAX_HEIGHT` 保证编辑区始终有空间（不在本用例内重复验证）。
    assert!(
        after < before,
        "结果区分栏后编辑区应变矮（{before:?} → {after:?}）"
    );
}

// ===== B6：错误回填（诊断 + 定位 + 聚焦）=====

/// 能定位的失败：诊断画在出错词上、光标跳过去并选中它、状态栏说出位置
#[gpui_kit::test]
fn a_locatable_failure_marks_the_word_and_moves_the_caret(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select 1;\nselect * from t wheree x = 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );

    // 诊断是回填里当场画的（真机与 headless 走同一条路）
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        1,
        "出错词上要有一条诊断"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要有提示");
    assert!(
        message.contains("no such column: wheree") && message.contains("第 2 行 第 17 列"),
        "提示要说清错误 + 位置：{message}"
    );

    // 光标跳过去这一步：真机上回填发生在**后台轮询**（没有窗口）里，走的是面板存下的窗口句柄；
    // headless 里那条路会撞上“不能在窗口更新里再更新窗口”（`update_window` 直接返回 Err），
    // 所以这里直接驱动落地入口——与“对话框按钮的真点击”同一口径（架构 §12 #29）。
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.jump_to_error_site_with(window, cx)));
    // 文档：`select 1;\n`（10 字节）+ `select * from t `（16 字节）→ 出错词在第 2 行第 17 列
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx)),
        26..32,
        "光标要选中出错的那个词"
    );
}

/// 认不出位置的失败：只提示，不动光标、不画诊断（定位错比不定位更糟）
#[gpui_kit::test]
fn an_unlocatable_failure_leaves_the_caret_alone(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // `ScriptRunner` 的 `boom` 里没有任何可定位的线索
    let (shared, id, _seen, _connections) = shared_with_runner("select boom;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.set_caret_for_test(0, cx)));
    run_statement(
        cx,
        &panel,
        "select boom",
        execution::ResultPlacement::Replace,
    );

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx)),
        0..0,
        "没有位置就不动光标"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        0,
        "没有位置就不画诊断"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要有提示");
    assert!(message.contains("boom"), "{message}");
    assert!(
        !message.contains("第 "),
        "没有位置就别编一个出来：{message}"
    );
}

/// 下一次成功要把旧的诊断清掉（不留“已经修好了还红着”的假象）
#[gpui_kit::test]
fn a_successful_run_clears_the_error_marks(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select * from t wheree x = 1;\nselect 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        1
    );

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        0,
        "成功之后不该再留着上次的诊断"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.is_none(), "成功要清掉旧提示：{message:?}");
}

// ===== 原型对齐：结果区 ⑤⑥⑦（原型 §2.4）=====

/// 竖向顺序按原型：⑤ 标签条 → ⑥ 工具栏 → 网格 → ⑦ 状态行
#[gpui_kit::test]
fn the_result_area_stacks_the_prototype_blocks_in_order(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 两份结果才有标签条（⑤ 是“多结果”的切换器）
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    run_statement(cx, &panel, "select 2", execution::ResultPlacement::NewSet);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let y = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} 没画出来"))
            .origin
            .y
            .as_f32()
    };
    let tabs = y(cx, "editor-result-tabs");
    let toolbar = y(cx, "editor-result-toolbar");
    let grid = y(cx, "editor-result-grid");
    let status = y(cx, "editor-result-status-row");
    assert!(
        tabs < toolbar && toolbar < grid && grid < status,
        "原型 §2.4 的顺序是 ⑤标签条 → ⑥工具栏 → 网格 → ⑦状态行（实得 {tabs} / {toolbar} / {grid} / {status}）"
    );
}

/// ⑦ 的“已选第 N 行”跟着表格的选中走（点行不改结果集，只重画那一行）
#[gpui_kit::test]
fn the_status_row_reports_the_selected_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert_eq!(status.selected_row, None, "刚跑完没选中任何行");

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_grid_row_for_test(0, cx)));
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert_eq!(status.selected_row, Some(1), "选中第一行（1 基）");
    assert!(
        dialog_button_rendered(cx, "editor-result-status-row"),
        "状态行要真的画出来"
    );
}

/// 【8-左栏】无选中行时点「记录」也应打开转置（第 0 行兜底）——用户反馈
/// "记录按钮点击没反应，只有选中一行才可以"。直调面板方法验证 toggle_transpose
/// 在无选中行时确实生效（选中行只是改变转置起点，不是必要条件）。
#[gpui_kit::test]
fn transpose_opens_without_selection(cx: &mut TestAppContext) {
    use result_panel::rowset::RowRef;
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    // 无选中行
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert_eq!(status.selected_row, None, "刚跑完没选中任何行");

    // 直调「记录」按钮的最终动作（无选中行）
    let rp = cx.update(|_window, cx| panel.read(cx).result_panel_for_test());
    cx.update(|_window, cx| rp.update(cx, |p, cx| p.toggle_transpose(cx)));
    let transposed = cx
        .update(|_window, cx| rp.read(cx).grid().read(cx).delegate().is_transposed());
    assert!(transposed, "无选中行点记录也应打开转置（第 0 行兜底）");

    // 转置到第 0 行（数据行 0）
    let row = cx.update(|_window, cx| rp.read(cx).grid().read(cx).delegate().transpose_row());
    assert_eq!(row, RowRef::Data(0), "无选中行时从第 0 行开始竖排");
}

/// 【8-左栏】竖条「记录」按钮的真实点击要能打开转置——用户反馈"记录按钮点击
/// 没有反应"。方法层已验证（见 transpose_opens_without_selection），这里走真实
/// 点击路径验证按钮事件 → 队列 → 分派整条链路。
#[gpui_kit::test]
fn the_left_record_button_toggles_transpose_on_real_click(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 【对照】同竖条「文本」按钮：点击后不手动 drain，验证 request 自动通知生效
    let text_btn = cx
        .debug_bounds("editor-result-bar-text")
        .expect("文本按钮要画出来");
    cx.simulate_click(text_btn.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let presentation = cx.update(|_window, cx| panel.read(cx).presentation_for_test(cx));
    assert_eq!(
        presentation,
        result_panel::ports::Presentation::Text,
        "文本按钮点击应立即切到文本（request 自动通知宿主 drain）"
    );

    // 切回网格
    let grid_btn = cx
        .debug_bounds("editor-result-bar-grid")
        .expect("网格按钮要画出来");
    cx.simulate_click(grid_btn.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let presentation = cx.update(|_window, cx| panel.read(cx).presentation_for_test(cx));
    assert_eq!(
        presentation,
        result_panel::ports::Presentation::Grid,
        "网格按钮点击应立即切回网格"
    );

    let btn = cx
        .debug_bounds("editor-result-bar-record")
        .expect("记录按钮要画出来");
    cx.simulate_click(btn.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();

    let transposed = cx.update(|_window, cx| {
        let rp = panel.read(cx).result_panel_for_test();
        rp.read(cx).grid().read(cx).delegate().is_transposed()
    });
    assert!(transposed, "真实点击「记录」按钮应打开转置");
}

/// 能定位的失败：卡片带「定位到第 N 行」按钮，且它真能把光标送到出错词
#[gpui_kit::test]
fn the_error_card_offers_a_working_locate_button(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select * from t wheree x = 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );

    let card = cx
        .update(|_window, cx| panel.read(cx).result_error_card_for_test(cx))
        .expect("失败要有错误卡片");
    assert_eq!(card.location.as_deref(), Some("第 1 行 第 17 列"));
    assert!(
        dialog_button_rendered(cx, "editor-result-error-card"),
        "卡片要真的画出来"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-error-locate"),
        "能定位就有按钮"
    );
    assert!(
        !dialog_button_rendered(cx, "editor-result-grid"),
        "失败时画卡片，不画空网格"
    );

    // 按钮按下去（headless 里直接驱动落地入口）：光标落到出错词上
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.jump_to_error_site_with(window, cx)));
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx)),
        16..22,
        "选中 `wheree`"
    );
}

/// 卡片上的「复制」把驱动原文放进剪贴板（拿去搜索 / 报 bug）
#[gpui_kit::test]
fn the_error_card_can_copy_the_driver_message(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select * from t wheree x = 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-error-copy"),
        "卡片上要有复制"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.copy_error_message(cx)));
    let text = cx.update(|_window, app| app.read_from_clipboard().and_then(|item| item.text()));
    let text = text.expect("剪贴板里要有东西");
    assert!(text.contains("no such column: wheree"), "{text}");
    assert!(text.contains("第 1 行 第 17 列"), "位置一起复制走：{text}");
}

// ===== B5b：分段抓取（⑦ 状态行里的分页位）=====

/// 首段拿满：状态行报 `2+` 并摆「取下一段」；取回来接在同一份结果上
#[gpui_kit::test]
fn fetching_the_next_segment_appends_to_the_same_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_segment_runner("select n from t;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        2,
        "首段两行"
    );
    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test(cx))
        .expect("有结果");
    assert!(summary.contains("行数 2"), "{summary}");
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert!(status.has_more, "拿满了 → 还有下一段");
    assert!(
        dialog_button_rendered(cx, "editor-result-more"),
        "还有下一段就摆「取下一段」"
    );

    // 按下去（headless 里直接驱动落地入口）：再拿两行，接在同一份结果上
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.fetch_more(cx)));
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "要的正是“已经拿到的行数”之后那一段"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        4,
        "两段接起来"
    );
    assert_eq!(shared.results().set_count(&id), 1, "取下一段不新开结果集");
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("还有状态行");
    assert_eq!(status.total_rows, 4);
    assert!(!status.has_more, "最后一段没拿满 → 到底了");
    assert!(
        !dialog_button_rendered(cx, "editor-result-more"),
        "到底了就不摆按钮"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.is_none(), "一次成功的取段不留提示：{message:?}");
}

/// 执行器不支持取下一段：按钮按下去要留一句可读原因（不静默、不假装成功）
#[gpui_kit::test]
fn fetching_a_segment_without_support_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(MoreUnsupportedRunner));
    let id = shared
        .open(OpenRequest::untitled("select 1;", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert!(
        dialog_button_rendered(cx, "editor-result-more"),
        "有下一段就摆按钮"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.fetch_more(cx)));
    wait_for_all_pending(cx, &panel);

    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要留原因");
    assert!(message.contains("不支持分段抓取"), "{message}");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        1,
        "取不回来不该动已抓到的行"
    );
}

/// 【B5b】滚动到底自动加载：组件库调 delegate 的 `load_more` → 面板真的去取下一段
#[gpui_kit::test]
fn scrolling_to_the_bottom_fetches_the_next_segment(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());
    let wants = |cx: &mut VisualTestContext| {
        cx.update(|_window, cx| grid.update(cx, |state, _cx| state.delegate_mut().wants_more()))
    };
    assert!(wants(cx), "首段拿满 + 有下一段 → delegate 允许取更多");

    // 模拟组件库在“可见范围靠近末尾”时的调用（真机里由 `load_more_if_need` 触发；
    // 回调是异步挪出更新栈的，所以要 `run_until_parked` 把它推到位）
    cx.update(|window, cx| {
        grid.update(cx, |state, cx| {
            state.delegate_mut().load_more(window, cx);
            // 同一帧里再调一次：乐观置位要把它拦下来（真机上滚动会连发）
            state.delegate_mut().load_more(window, cx);
        });
    });
    cx.run_until_parked();
    // 提交之后到了回填之前：不许再触发一次（否则每次滚动都撞“执行中”的回绝）
    assert!(!wants(cx), "正在取下一段时不该重复触发");
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "自动加载要的正是“已经拿到的行数”之后那一段，而且同一帧里只发一次"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        4,
        "两段接起来"
    );
    assert!(!wants(cx), "抓到没拿满的一段就不该再取");
    assert_eq!(shared.results().set_count(&id), 1, "自动加载不新开结果集");
}

/// 【B5b】真渲染几帧：让**组件库自己**在“可见范围靠近末尾”时触发取段
///
/// 上一条测试是手动调 `load_more`（验回调链路），这一条让它从渲染路径自然发生：
/// 行数少于阈值（默认 20）时，`load_more_if_need` 的条件在上方恒成立。
#[gpui_kit::test]
fn rendering_a_short_result_asks_for_more_by_itself(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "渲染路径应当自己触发取段，而且只发一次"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        4,
        "两段接起来"
    );
}

/// 【B5b】没有下一段时 delegate 不该允许取更多（没有结果 / 写语句都不行）
#[gpui_kit::test]
fn a_finished_result_does_not_ask_for_more(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _asked) = shared_with_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);
    let grid = cx.update(|_window, cx| panel.read(cx).grid_for_test());

    let wants = |cx: &mut VisualTestContext| {
        cx.update(|_window, cx| grid.update(cx, |state, _cx| state.delegate_mut().wants_more()))
    };
    assert!(!wants(cx), "还没执行过：没有下一段可言");

    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    wait_for_all_pending(cx, &panel);
    // 取完两段（首段 2 行 + 取回 2 行）后到底
    cx.update(|window, cx| {
        grid.update(cx, |state, cx| {
            state.delegate_mut().load_more(window, cx);
        });
    });
    cx.run_until_parked();
    wait_for_all_pending(cx, &panel);
    assert!(!wants(cx), "第二段没拿满 → 到底了");
}

