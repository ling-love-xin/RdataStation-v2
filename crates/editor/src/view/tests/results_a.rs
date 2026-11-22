//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use gpui_kit::component::table::TableDelegate as _;
use gpui_kit::{TestAppContext, VisualTestContext};

use crate::execution::{self};
use crate::export::ExportFormat;
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::LocatedFailureRunner;
use super::MoreUnsupportedRunner;
use super::dialogs::dialog_button_rendered;
use super::results_b::{attach_export_picker, export_temp_path};
use super::{
    bind_editor_keys, open_panel, run_statement, shared_with_runner, shared_with_segment_runner,
    shared_with_sized_runner, shared_with_three_segment_runner, shared_with_toolbar_runner,
    wait_for_all_pending,
};

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
    // 0.9 起手动 `draw` 只重放保留帧：窗口不脏时结果面板不重建（表头坐标登记不进来）。
    // 先 `refresh` 把窗口置脏再出帧（等价于 `TestWindowExt::render_frame`）。
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });

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
    let transposed =
        cx.update(|_window, cx| rp.read(cx).grid().read(cx).delegate().is_transposed());
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

/// 【第 70 批】`CTRL+ALT+N` = 取下一段（DBeaver `CMD_FETCH_PAGE` → `readNextSegment()`，
/// `plugin.xml:1069`，context = `...resultset.focused`）。
///
/// 与状态栏「获取数据」钮**同一条路**（`ResultRequest::FetchMore`）：断言落在
/// 执行器真的被要了「已经拿到的行数之后那一段」以及行数翻倍上。
///
/// 这条用例的前提是**按键前后都不 draw**：渲染路径自己也会在「可见范围靠近末尾」时
/// 触发 `delegate.load_more`（见 `rendering_a_short_result_asks_for_more_by_itself`），
/// 一 draw 就分不清请求是谁发的。所以先断言「按键前 `asked` 为空」，把这个前提钉死；
/// 哪天 `simulate_keystrokes` 开始顺带渲染，这一句会先失败提醒改用例。
#[gpui_kit::test]
fn the_fetch_page_key_asks_for_the_next_segment(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, asked) = shared_with_segment_runner("select n from t");
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
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("有网格就有状态行");
    assert!(status.has_more, "拿满了 → 还有下一段");
    assert!(
        asked.lock().expect("锁").is_empty(),
        "按键前不该有取段请求（一半靠这条证明请求真是键位发的）"
    );

    // 焦点交给网格：键位只挂在 `RESULT_GRID_CONTEXT` 上
    let grid_focus = cx.update(|_window, app| {
        use gpui_kit::Focusable as _;
        panel.read(app).grid_for_test().read(app).focus_handle(app)
    });
    cx.update(|window, cx| window.focus(&grid_focus, cx));

    cx.simulate_keystrokes("ctrl-alt-n");
    cx.run_until_parked();
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "键位要的正是「已经拿到的行数」之后那一段（与「获取数据」钮同一条路），而且只发一次"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        4,
        "两段接起来"
    );
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("还有状态行");
    assert!(!status.has_more, "最后一段没拿满 → 到底了");
    assert_eq!(shared.results().set_count(&id), 1, "取下一段不新开结果集");
}

/// 【第 71 批】「获取全部数据」= 一直取到没有下一段（DBeaver `readAllData:4426-4459`）
///
/// 第 70 批之前 `dispatch` 的 `FetchAll` 分支只调一次 `fetch_more`，注释承诺的循环没实现
/// （钮叫「获取全部数据」，实际只取一段）。这条用**三段** runner 钉住「真的循环」：
/// 取全部要发两次 `fetch_next`（offset 2 → 4），而「取下一段」只发一次
/// （对照见上面 `the_fetch_page_key_asks_for_the_next_segment`）。
#[gpui_kit::test]
fn fetching_all_keeps_fetching_until_there_is_no_more(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_three_segment_runner("select n from t");
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
    assert!(
        asked.lock().expect("锁").is_empty(),
        "取全部之前不该有取段请求（与渲染路径分开：这条不 draw）"
    );

    // 与状态栏那颗钮同一条入队路径（`ResultRequest::FetchAll` → `dispatch` → `fetch_all`）
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::FetchAll, cx);
            host.drain_result_actions(window, cx);
        });
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS), (4, execution::SEGMENT_ROWS)],
        "取全部要跨轮：每一轮的 offset 都得是「已经拿到的行数」"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        6,
        "三段接起来"
    );
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test(cx))
        .expect("还有状态行");
    assert!(!status.has_more, "最后一段没拿满 → 到底了");
    assert_eq!(shared.results().set_count(&id), 1, "取全部不新开结果集");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("已取全部 6 行"),
        "收口要如实说一句（拿到多少行），实际是：{message}"
    );
}

/// 【第 71 批】已经取完时再按「获取全部数据」：只给一句提示，不再发取段请求
///
/// 门控与「取下一段」同源（都走 `submit_segment` 的 `can_fetch_more` 判定），
/// 不是为取全部另写一条真值表。
#[gpui_kit::test]
fn asking_for_all_after_the_end_only_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    // 先取一段：两段 runner 的第二段没拿满 → 到底
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::FetchMore, cx);
            host.drain_result_actions(window, cx);
        });
    });
    wait_for_all_pending(cx, &panel);
    let taken = asked.lock().expect("锁").len();
    assert_eq!(taken, 1, "先取了一段");

    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::FetchAll, cx);
            host.drain_result_actions(window, cx);
        });
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").len(),
        taken,
        "已经到底：不该再发取段请求"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("这份结果已经取完了"),
        "要如实说「已经取完」，实际是：{message}"
    );
}

/// 【第 72 批】`CTRL+SHIFT+=` = 取全部行（DBeaver `CMD_FETCH_ALL` → `readAllData()`，
/// `plugin.xml:1070`），但**先问一声**：`CONFIRM_RS_FETCH_ALL`
/// （`ResultSetViewer.readAllData:4433-4440`）。
///
/// 这条钉两件事：
/// 1. 键位真的命中（`ctrl-shift-=` 里 `=` 是物理键名，shift 下打出的是 `+`，
///    gpui 比的是规范化键名 + 修饰位，`platform/keystroke.rs:112`）；
/// 2. 命中的结果只是**弹确认**，一行都没取——第 71 批之前键位与按钮是直接开跑的。
#[gpui_kit::test]
fn the_fetch_all_key_asks_before_it_starts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, asked) = shared_with_three_segment_runner("select n from t");
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
    // 掐掉「渲染自动补齐」这条旁路：短结果一重绘就会自己取下一段
    // （`rendering_a_short_result_asks_for_more_by_itself` 钉的就是这个**有意**行为），
    // 但浮层一打开面板就要重绘，那条旁路会把「键位 → 确认」这条链路淹掉。
    // 只关 delegate 侧：store 里的 `has_more` 仍为真，所以 `ask_fetch_all` 照样该弹。
    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    cx.update(|_window, cx| {
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_has_more(false);
            cx.notify();
        });
    });
    assert!(
        asked.lock().expect("锁").is_empty(),
        "按键前不该有取段请求（与渲染路径分开：这条不 draw）"
    );
    assert!(
        !cx.update(|_window, cx| panel.read(cx).fetch_all_confirm_for_test(cx)),
        "出厂不弹"
    );

    // 焦点交给网格：键位只挂在 `RESULT_GRID_CONTEXT` 上
    let grid_focus = cx.update(|_window, app| {
        use gpui_kit::Focusable as _;
        panel.read(app).grid_for_test().read(app).focus_handle(app)
    });
    cx.update(|window, cx| window.focus(&grid_focus, cx));

    cx.simulate_keystrokes("ctrl-shift-=");
    cx.run_until_parked();

    assert!(
        cx.update(|_window, cx| panel.read(cx).fetch_all_confirm_for_test(cx)),
        "键位要先把确认浮层打开（DBeaver 也是先 CONFIRM_RS_FETCH_ALL）"
    );
    assert!(
        asked.lock().expect("锁").is_empty(),
        "确认之前一行都不该取：键位不是「开跑」而是「先问」"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        2,
        "行数保持首段"
    );
}

/// 【第 72 批】确认之后才真跑：`FetchAll`（浮层「继续」发的就是它）走完三段循环，
/// 顺手把浮层收掉。
#[gpui_kit::test]
fn confirming_the_prompt_starts_the_fetch_all(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, asked) = shared_with_three_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );

    // 先按键把确认浮层打开（与上一条同一条入队路径）
    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    cx.update(|_window, cx| {
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_has_more(false);
            cx.notify();
        });
    });
    let grid_focus = cx.update(|_window, app| {
        use gpui_kit::Focusable as _;
        panel.read(app).grid_for_test().read(app).focus_handle(app)
    });
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-shift-=");
    cx.run_until_parked();
    assert!(
        cx.update(|_window, cx| panel.read(cx).fetch_all_confirm_for_test(cx)),
        "前置：浮层已开"
    );

    // 浮层「继续」与这里同一条队列请求
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::FetchAll, cx);
            host.drain_result_actions(window, cx);
        });
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS), (4, execution::SEGMENT_ROWS)],
        "确认后真的要跨轮取到没有下一段"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        6,
        "三段接起来"
    );
    assert!(
        !cx.update(|_window, cx| panel.read(cx).fetch_all_confirm_for_test(cx)),
        "真跑起来要先把浮层收掉（不然遮罩盖着结果）"
    );
}

/// 【第 72 批】已经取完时按 `CTRL+SHIFT+=`：**不弹浮层**，只给一句提示
///
/// 对齐 DBeaver：`readAllData` 第一件事是 `!isHasMoreData()` → 静默 return
/// （`:4430-4432`）；本仓键位层统一「不可用留提示」，所以这里说一句而不是什么都不做。
#[gpui_kit::test]
fn the_fetch_all_key_after_the_end_only_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, asked) = shared_with_segment_runner("select n from t");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    // 先取一段：两段 runner 的第二段没拿满 → 到底
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::FetchMore, cx);
            host.drain_result_actions(window, cx);
        });
    });
    wait_for_all_pending(cx, &panel);
    let taken = asked.lock().expect("锁").len();
    assert_eq!(taken, 1, "先取了一段");

    let grid_focus = cx.update(|_window, app| {
        use gpui_kit::Focusable as _;
        panel.read(app).grid_for_test().read(app).focus_handle(app)
    });
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-shift-=");
    cx.run_until_parked();

    assert!(
        !cx.update(|_window, cx| panel.read(cx).fetch_all_confirm_for_test(cx)),
        "已经到底：不该弹确认（问了也没有下一段可取）"
    );
    assert_eq!(
        asked.lock().expect("锁").len(),
        taken,
        "不该再发取段请求"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("这份结果已经取完了"),
        "键位不可用要留提示（第 69 批口径），实际是：{message}"
    );
}

/// 【第 73 批】取全部在跑时导出要**在入口就被拦**（第 71 批的互斥只做了反方向）
///
/// 两个理由：
/// 1. 段与段之间通道是空闲的（`is_executing()` 会短暂为 false），那时放过去就会落半截文件；
/// 2. 两条都是 `Append` 取段，各算各的 offset（都按「已抓到的行数」），一起跑会重复抓同一段。
///
/// 这条同时钉住判据顺序：必须判在「执行器忙」之前，否则用户只看到「执行中，稍候」
/// ——听起来像等一下就好，实际取全部可能还要跑很久。
#[gpui_kit::test]
fn exporting_while_fetching_all_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_three_segment_runner("select n from t");
    let path = export_temp_path("fetchall", "csv");
    let picked = attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(asked.lock().expect("锁").len(), 0, "首段之外没有取段");

    // 起取全部（与浮层「继续」同一条队列路径），只 drain 不推进：首段还在途
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::FetchAll, cx);
            host.drain_result_actions(window, cx);
        });
    });
    assert_eq!(
        asked.lock().expect("锁").len(),
        1,
        "前置：取全部已提交首段"
    );

    // 此时 fetch_all 在跑、通道也忙：导出该被「取全部」拦
    cx.update(|_window, cx| {
        panel.update(cx, |host, cx| {
            host.export_active_result_all(ExportFormat::Csv, cx);
        });
    });

    assert!(
        picked.lock().expect("锁").is_empty(),
        "连路径都不该问：拦在选路径之前（否则用户白选一次）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("正在取全部数据") && message.contains("等它结束再导出"),
        "要说明是谁在占着通道，实际是：{message}"
    );

    // 收尾：取全部自己跑完，导出一事无成
    wait_for_all_pending(cx, &panel);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        6,
        "取全部没被导出的这次尝试打断"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).export_pending_for_test()),
        0,
        "导出没有留下在途状态"
    );
}

/// 【第 73 批】导出正在抓全量时问「要不要取全部」：不弹浮层，直接说清是谁在取
///
/// 反向那一格。导出抓全量期间也置 `fetching_more`，所以这条同时钉住判据顺序：
/// `pending_export` 必须判在 `fetching_more` 之前，否则提示会变成「正在取下一段」，
/// 用户不知道那次取段其实是自己刚点的导出在跑。
#[gpui_kit::test]
fn asking_for_all_while_the_export_is_fetching_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_three_segment_runner("select n from t");
    let path = export_temp_path("pends", "csv");
    let _picked = attach_export_picker(&shared, Some(path.clone()));
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(
        cx,
        &panel,
        "select n from t",
        execution::ResultPlacement::Replace,
    );

    // 导出并抓全量：首段之后还有下一段，于是置 `pending_export` 并发一段
    cx.update(|_window, cx| {
        panel.update(cx, |host, cx| {
            host.export_active_result_all(ExportFormat::Csv, cx);
        });
    });
    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "前置：导出占着取段（它自己发的第一段，offset 是已抓到的行数）"
    );

    // 这时按键（与状态栏那颗钮同一条）：只该给一句提示
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::AskFetchAll, cx);
            host.drain_result_actions(window, cx);
        });
    });

    assert!(
        !cx.update(|_window, cx| panel.read(cx).fetch_all_confirm_for_test(cx)),
        "不该弹浮层：问了也没有「继续」可点（那一下照样会被 fetch_all 拒）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("导出正在取数"),
        "要说清是导出在取，而不是含糊的「正在取下一段」，实际是：{message}"
    );

    // 收尾：导出把全量抓完、落盘，取全部没被启动
    wait_for_all_pending(cx, &panel);
    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS), (4, execution::SEGMENT_ROWS)],
        "导出自己跨轮抓完了；取全部一次都没发"
    );
}

/// 【第 75 批】`CTRL+SHIFT+G` 跳列全链：键位 → 面板浮层（真挂到渲染树上）→ 列名提交 →
/// 选中该列，**行不动**（DBeaver `setCurrentAttribute` 之后 `scrollToRow(CURRENT)`）。
///
/// 列映射要点：`set_selected_cell` 收的是**组件列**（0 是行号槽），所以「note」这一列
/// （第 2 个数据列）落到组件列 2；用 1 会选到 id，用例就是钉这一格。
#[gpui_kit::test]
fn the_goto_column_key_opens_the_overlay_and_selects_the_named_column(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    // 当前行 = 第 1 行、当前列 = 第 1 个数据列（组件列 1）
    cx.update(|_window, app| grid.update(app, |state, cx| state.set_selected_cell(0, 1, cx)));

    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-shift-g");
    cx.run_until_parked();

    assert!(
        cx.update(|_window, app| panel
            .read(app)
            .result_panel
            .read(app)
            .goto_column_for_test(app)),
        "键位应打开浮层"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-goto-column-go"),
        "浮层要真的挂到面板渲染树上"
    );

    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            let result_panel = p.result_panel.clone();
            result_panel.update(cx, |p, cx| {
                p.set_goto_column_text_for_test("note", window, cx)
            });
        });
    });
    // 走浮层「跳转」钮那条端口：与 Enter 同一终点（宿主 dispatch 调 submit）
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_action_queue
                .request(result_panel::ports::ResultRequest::SubmitGotoColumn, cx);
            host.drain_result_actions(window, cx);
        });
    });

    assert_eq!(
        cx.update(|_window, app| grid.read(app).selected_cell()),
        Some((0, 2)),
        "「note」是第 2 个数据列 → 组件列 2；行保持 0"
    );
    assert!(
        !cx.update(|_window, app| panel
            .read(app)
            .result_panel
            .read(app)
            .goto_column_for_test(app)),
        "提交后浮层要收"
    );
}

/// 【第 75 批】还没有结果时按 `CTRL+SHIFT+G`：不弹浮层，只留一句提示
/// （DBeaver 那边没有列也就没有对话框；本仓键位层统一「不可用要说话」）。
///
/// 这里直接调 action 的处理方法而不再 `simulate_keystrokes`：没有结果时结果区整块
/// 不渲染，网格不在焦点路径上，键位根本走不到（键位全链由上面那条用例钉）。
#[gpui_kit::test]
fn the_goto_column_key_without_a_result_only_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.on_goto_column(&crate::commands::GotoColumn, window, cx);
        });
    });
    cx.run_until_parked();

    assert!(
        !cx.update(|_window, app| panel
            .read(app)
            .result_panel
            .read(app)
            .goto_column_for_test(app)),
        "没列可跳就不该开浮层"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("还没有列可跳"),
        "键位不可用要留提示，实际是：{message}"
    );
}

/// 【第 76 批】`CTRL+ALT+C` 选列全链：键位 → 面板方法 → 组件整列选中。
///
/// 当前格放在「note」（第 2 个数据列 → 组件列 2）上，用例钉「选列不会左移到行号槽」
/// （`set_selected_col` 收组件列，0 是行号槽）。
#[gpui_kit::test]
fn the_select_column_key_selects_the_whole_column(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    cx.update(|_window, app| grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx)));

    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-alt-c");
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_window, app| grid.read(app).selected_col()),
        Some(2),
        "「note」整列选中（组件列 2，不是行号槽也不是 id）"
    );
}

/// 【第 76 批】`CTRL+ALT+R` 选行全链：键位 → 组件整行选中 + 状态行写 1 基行号。
/// 状态那口正是 `row_target_state` 的输入，行级三钮 / 邻行复制都靠它。
#[gpui_kit::test]
fn the_select_row_key_selects_the_whole_row_and_marks_the_status_line(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    cx.update(|_window, app| grid.update(app, |state, cx| state.set_selected_cell(0, 1, cx)));

    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-alt-r");
    cx.run_until_parked();

    let (component_row, status_row) = cx.update(|_window, app| {
        let status_row = panel
            .read(app)
            .result_panel
            .read(app)
            .status()
            .and_then(|s| s.selected_row);
        (grid.read(app).selected_row(), status_row)
    });
    assert_eq!(component_row, Some(0), "整行选中");
    assert_eq!(status_row, Some(1), "状态行写 1 基行号（行级动作读这口）");
}

/// 【第 76 批】还没有结果时按 `CTRL+ALT+C` / `CTRL+ALT+R`：不动作，各留一句提示
/// （与第 75 批跳列同口径：键位没有「灰」这一态，不可用要说出来）。
///
/// 没有结果时网格不在焦点路径上，直接调 action 的处理方法。
#[gpui_kit::test]
fn the_select_column_and_row_keys_without_a_result_only_say_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.on_select_current_column(&crate::commands::SelectCurrentColumn, window, cx);
        });
    });
    cx.run_until_parked();
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("选列"),
        "选列不可用要留提示，实际是：{message}"
    );

    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.on_select_current_row(&crate::commands::SelectCurrentRow, window, cx);
        });
    });
    cx.run_until_parked();
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("选行"),
        "选行不可用要留提示，实际是：{message}"
    );
}

/// 【第 77 批】`CTRL+ALT+SHIFT+T` 全链：键位（面板 context）→ 焦点进筛选框 →
/// 再按一次焦点回网格（DBeaver `switchFilterFocus` 的双向语义）。
#[gpui_kit::test]
fn the_filter_focus_key_moves_between_the_filter_box_and_the_grid(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    let filter_focus = cx.update(|_w, app| panel.read(app).result_panel.read(app).filter_focus_handle(app));

    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-alt-shift-t");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        Some(filter_focus.clone()),
        "第一次按：焦点进筛选框"
    );

    cx.simulate_keystrokes("ctrl-alt-shift-t");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        Some(grid_focus),
        "第二次按（焦点在筛选框里）：交回网格"
    );
}

/// 【第 77 批】还没有结果集时按 `CTRL+ALT+SHIFT+T`：
///
/// 结果区在本仓是**有内容才渲染**（`host/render.rs:221/241` 的 `result_has_content`），
/// 所以无结果时面板子树根本不在树上，`editor-result-panel` 上下文也不在焦点路径里
/// ——键位不命中、焦点不动。这与上游口径一致：`resultset.focus.filter` 的
/// `enabledWhen` 要求 `resultset.part` 存在（命令定义 `plugin.xml:249`、handler
/// `plugin.xml:545`），没有结果集时这条命令就是禁用的。上游
/// `switchFilterFocus:689-701` 自己只判 `filtersPanel`，不看有没有数据；差别只在
/// “部件存不存在”，本仓由「面板渲不渲染」自然承担。
#[gpui_kit::test]
fn the_filter_focus_key_is_inert_without_a_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    assert!(
        !cx.update(|_w, app| panel.read(app).result_panel.read(app).has_content()),
        "这条用例的前提是还没有结果"
    );

    let before = cx.update(|window, cx| window.focused(cx));
    cx.simulate_keystrokes("ctrl-alt-shift-t");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        before,
        "没有结果时焦点不动（与上游 enabledWhen 一致）"
    );
}

/// 【第 78 批】`ALT+SHIFT+H` / `ALT+SHIFT+T` 全链（`CMD_HIDE_COLUMNS` /
/// `CMD_SHOW_COLUMNS`，`plugin.xml:1096/1097`；`SpreadsheetCommandHandler:174-212`）：
/// 没有隐藏列时按「显示」只留提示 → 按「隐藏」把当前列藏掉 → 再按「显示」放回来。
#[gpui_kit::test]
fn the_hide_and_show_column_keys_round_trip(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    // 组件列 2 = 数据列 1（`note`）
    cx.update(|_window, app| grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx)));
    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));

    // 本来没有隐藏列 → `ALT+SHIFT+T` 只留一句提示
    cx.simulate_keystrokes("alt-shift-t");
    cx.run_until_parked();
    assert!(
        cx.update(|_w, app| panel
            .read(app)
            .result_panel
            .read(app)
            .column_hidden()
            .is_empty()),
        "没有隐藏列时什么也不变"
    );
    let message = cx
        .update(|_w, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("没有隐藏的列"),
        "要说出来，实际是：{message}"
    );

    // `ALT+SHIFT+H`：藏掉当前列
    cx.simulate_keystrokes("alt-shift-h");
    cx.run_until_parked();
    let hidden = cx.update(|_w, app| {
        panel
            .read(app)
            .result_panel
            .read(app)
            .column_hidden()
    });
    assert_eq!(hidden, vec!["note".to_string()], "当前列进隐藏清单");

    // 焦点在网格里；`ALT+SHIFT+T` 把隐藏列放回来
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("alt-shift-t");
    cx.run_until_parked();
    assert!(
        cx.update(|_w, app| panel
            .read(app)
            .result_panel
            .read(app)
            .column_hidden()
            .is_empty()),
        "所有隐藏列都回来了"
    );
}

/// 【第 79 批】`ALT+SHIFT+C` 复制列名全链（`CMD_COPY_COLUMN_NAMES`，`plugin.xml:1095`；
/// `ResultSetHandlerMain:333-375`）：选中某格 → 拷那一列；藏掉当前列（清选后）
/// → 退化成全部可见列，隐藏的列不出现。
#[gpui_kit::test]
fn alt_shift_c_copies_the_column_names(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    // 组件列 2 = 数据列 1（`note`）
    cx.update(|_window, app| grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx)));
    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));

    let clipboard = |cx: &mut VisualTestContext| {
        cx.update(|_window, app| app.read_from_clipboard().and_then(|item| item.text()))
    };
    let message = |cx: &mut VisualTestContext| {
        cx.update(|_window, cx| panel.read(cx).message.clone())
            .unwrap_or_default()
    };

    // 当前格在 `note`：只拷那一列，不加分隔符
    cx.simulate_keystrokes("alt-shift-c");
    cx.run_until_parked();
    assert_eq!(clipboard(cx).as_deref(), Some("note"), "当前格所在列");
    assert!(message(cx).contains("已复制列名"), "实际是：{}", message(cx));

    // 藏掉 `note`（当前格在最右列 → 越界清选），再按就是全部可见列
    cx.simulate_keystrokes("alt-shift-h");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_w, app| panel.read(app).result_panel.read(app).column_hidden()),
        vec!["note".to_string()],
        "前提：这一列真的藏了"
    );
    // 藏掉最右一列时 `hide_current_column` 会清选（面板层用例
    // `hiding_the_rightmost_column_clears_the_current_cell` 钉住；面板字段不跨 crate 暴露），
    // 于是下面这一按走的是「全部可见列」那条路。

    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("alt-shift-c");
    cx.run_until_parked();
    assert_eq!(clipboard(cx).as_deref(), Some("n"), "隐藏列不进全量");
    assert!(message(cx).contains("已复制列名"), "实际是：{}", message(cx));
}

/// 【第 80 批】`ALT+SHIFT+F` 打开筛选设置浮层（`CMD_FILTER_EDIT_SETTINGS`，
/// `plugin.xml:1094`；handler `:570` → `ResultSetHandlerMain:546-549` →
/// `ResultSetViewer.showFilterSettingsDialog()` `:644-646`，直接开对话框、不判条件）。
///
/// 【第 81 批】`ESC` 收起这条浮层，焦点在排序输入框里也成立（原理见
/// `escape_dismisses_every_result_overlay` 的说明）；鼠标路径是标题栏那颗 ✕
/// （`editor-result-filter-settings-close`），两条出口都只入队 `CloseFilterSettings`。
#[gpui_kit::test]
fn alt_shift_f_opens_the_filter_settings_overlay(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));

    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_none(),
        "前提：还没打开"
    );

    cx.simulate_keystrokes("alt-shift-f");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_some(),
        "按一下就要把浮层摆出来"
    );

    // 【第 81 批】`ESC` 收起：焦点在排序框里也成立——`input` 自己的 `escape` binding
    // 命中后 `cx.propagate()`，面板层的 `DismissOverlay` 接住并冒泡到面板根
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_none(),
        "焦点在排序框时 ESC 也要收起浮层"
    );

    // 再开一次，用 ✕ 关（键位与鼠标各一条出口，落点都是 CloseFilterSettings）
    cx.simulate_keystrokes("alt-shift-f");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let close = cx
        .debug_bounds("editor-result-filter-settings-close")
        .expect("关闭钮要画出来");
    cx.simulate_click(close.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_none(),
        "点 ✕ 之后浮层收起"
    );
}

/// 【第 81 批】`ESC` 收起四个结果集浮层（SQL 预览 / 取全部确认 / 筛选设置 / 跳列）。
///
/// **为什么既不是键位、也不是遮罩的 `on_key_down`**：三条通道实测都不可用。
/// ① 遮罩 `on_key_down`：`ESC` 落到焦点节点后要先过 binding 与 action 派发，
/// `on_key_down` 排在它们之后（`gpui-pre-0.3.11/src/window.rs:6493` 在 binding 循环
/// 之后），焦点在输入框里时压根走不到；② 任何带 context 的 binding（网格层 / 面板层）：
/// 焦点一旦交给 `InputState`，`Window::context_stack()` 退化成空（本用例实测 `[]`，
/// 对照网格焦点是 `[Root, editor, editor-result-panel, editor-result-grid, DataTable]`），
/// 连面板层已绑好的 `Ctrl+Shift+G` 也匹配不到；③ 全局（`context: None`）binding 能命中，
/// 但会先截走网格内联编辑与侧栏那批靠 `on_key_down` 收 `ESC` 的处理（`grid/edit.rs`）。
/// 唯一不依赖焦点 / context 的通道是 app 级 keystroke interceptor
/// （`ResultPanel::new` 的 `intercept_keystrokes`，`window.rs:6335-6352` 在 binding 之前跑）。
/// 它只在真有浮层开着时才 `stop_propagation()`，无浮层时让路。
///
/// 本仓 `ESC` 不是 DBeaver 的 `cell.reset`（`plugin.xml:1081`）：网格的 `ESC` 保留组件
/// 语义，只有浮层按模态惯例收起，关哪一个由 `ResultPanel::dismiss_overlay` 判。
#[gpui_kit::test]
fn escape_dismisses_every_result_overlay(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    // ① 筛选设置：焦点被交给排序输入框（`open_filter_settings` 里 `state.focus`）
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("alt-shift-f");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_some(),
        "前提：筛选设置浮层开着"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_none(),
        "焦点在排序框时 ESC 也要收起筛选设置浮层"
    );

    // ② 跳列：焦点被交给跳列输入框
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-shift-g");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-goto-column-card").is_some(),
        "前提：跳列浮层开着"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-goto-column-card").is_none(),
        "焦点在跳列输入框时 ESC 也要收起浮层"
    );

    // ③ SQL 预览：没有输入框焦点（打开时不移交焦点），焦点仍在网格——
    //    这条证明面板层键位不依赖浮层内部焦点
    cx.update(|window, cx| {
        panel.update(cx, |host, cx| {
            host.result_panel
                .update(cx, |p, cx| p.open_sql_dialog(window, cx));
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-sql-dialog-card").is_some(),
        "前提：SQL 预览浮层开着"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-sql-dialog-card").is_none(),
        "焦点不在浮层里时 ESC 也要收起 SQL 预览"
    );

    // ④ 取全部确认：同样没有输入源，焦点留在网格
    cx.update(|_window, cx| {
        panel.update(cx, |host, cx| {
            host.result_panel.update(cx, |p, cx| p.open_fetch_all_confirm(cx));
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-fetch-all-confirm-card").is_some(),
        "前提：取全部确认浮层开着"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-fetch-all-confirm-card").is_none(),
        "ESC 要收起取全部确认浮层"
    );
}

/// 【第 82 批】`F7` / `CTRL+7`：右侧辅助栏整组收起 / 展开
/// （DBeaver `CMD_TOGGLE_PANELS`，`plugin.xml:1084/1085`；`ResultSetHandlerMain:167-169`
/// → `ResultSetViewer.showPanels(!isPanelsVisible(), true, true)`，`:1478-1518`）。
///
/// 断言分三层：① 权威状态 `panels_visible()`（= 右缘竖条与右栏内容都在）；
/// ② 右栏内容真的没了 / 回来了（`editor-result-right-tabs` 只在 `show_value_sidebar`
/// 为真时才挂进树，收起后取不到 bounds）；③ 两条键位落同一个动作——`F7` 收，
/// `CTRL+7` 再放。本仓鼠标等价入口是竖条顶部那颗 `panelsButton`
/// （`editor-result-edge-panels` → `ResultRequest::SetPanelsVisible`），同一份状态。
#[gpui_kit::test]
fn f7_and_ctrl_7_toggle_the_result_panels(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let grid = cx.update(|_window, app| panel.read(app).grid_for_test());
    let grid_focus = cx.update(|_window, app| grid.read(app).focus_handle(app));
    cx.update(|window, cx| window.focus(&grid_focus, cx));

    let panels_visible = |cx: &mut VisualTestContext| {
        cx.update(|_w, app| {
            panel
                .read(app)
                .result_panel
                .read(app)
                .panels_visible()
        })
    };

    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(panels_visible(cx), "前提：出厂是展开的");
    assert!(
        cx.debug_bounds("editor-result-right-tabs").is_some(),
        "前提：右栏内容摆着"
    );

    // `F7`：整组收起（右缘竖条与右栏内容一起消失）
    cx.simulate_keystrokes("f7");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!panels_visible(cx), "F7 之后面板收起");
    assert!(
        cx.debug_bounds("editor-result-right-tabs").is_none(),
        "右栏内容要跟着消失"
    );

    // `CTRL+7`：同一条动作反向（展开时顺带补回默认页集合）
    cx.update(|window, cx| window.focus(&grid_focus, cx));
    cx.simulate_keystrokes("ctrl-7");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(panels_visible(cx), "CTRL+7 之后面板展开");
    assert!(
        cx.debug_bounds("editor-result-right-tabs").is_some(),
        "右栏内容要回来"
    );
}
