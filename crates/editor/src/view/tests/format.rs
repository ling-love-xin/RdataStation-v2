//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::rc::Rc;

use gpui_kit::{
    Focusable as _, TestAppContext, VisualTestContext,
};

use crate::execution::{self};
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::{
    bind_editor_keys, open_panel, result_tabs, run_statement,
    shared_with_document, shared_with_runner, wait_for_all_pending,
    wait_for_result,
};
use super::dialogs::{FakeConnections, option};

// ===== B10：格式化（`Ctrl+Shift+F` / 工具栏「⇤ 格式化」）=====
//
// 分两扇：**能力**按模式分层（文本模式不解析 SQL 就不该有格式化），**行为**看真按键——
// 格式化改的是用户手上的文本，所以“改哪、没改哪、为什么没改”都要有回执。

/// 工具栏按能力分层：格式化只在 SQL 模式出现
#[gpui_kit::test]
fn the_toolbar_offers_format_only_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\format.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let format_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-format").is_some()
    };
    assert!(format_present(cx), "SQL 模式应当有「格式化」");

    shared.update(|service| service.set_mode(&id, EditorMode::General));
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!format_present(cx), "文本模式不解析 SQL，不得出现格式化");
}

// ===== 【B8 余项】工具栏「⟲ 历史」=====
//
// 历史面板本体（右 Dock `HistoryView` + 重放）在 B8 已落地；这里补的是原型 §2.2
// 文档级那个入口。能力分层与格式化同档（SQL 模式才放），点击走「打开历史面板」端口。

/// 工具栏按能力分层：历史只在 SQL 模式出现（与格式化同档，文档级常驻）
#[gpui_kit::test]
fn the_toolbar_offers_history_only_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\history.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let history_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-history").is_some()
    };
    assert!(history_present(cx), "SQL 模式应当有「历史」按钮");

    shared.update(|service| service.set_mode(&id, EditorMode::General));
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!history_present(cx), "文本模式不放文档级 SQL 按钮");
}

/// 注入「打开历史面板」端口后可从 shared 读出（宿主装配期接线；渲染路径只读）
#[gpui_kit::test]
fn open_history_port_is_readable_after_attach(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, _id) = shared_with_document(r"D:\sql\history.sql", "select 1;");
    assert!(
        shared.open_history_port().is_none(),
        "未注入时端口应缺席（按钮置灰）"
    );
    shared.attach_open_history(std::rc::Rc::new(|_cx: &mut gpui_kit::App| {}));
    assert!(
        shared.open_history_port().is_some(),
        "注入后端口应可读（按钮可点）"
    );
}

/// `Ctrl+Shift+F`：真按键 → 文本真被排版、状态栏报“改了几条”
#[gpui_kit::test]
fn ctrl_shift_f_formats_the_document_and_reports_the_count(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id) = shared_with_document(r"D:\sql\format.sql", "select a,b from t where x=1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-shift-f");

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(text.contains("FROM"), "关键字应当大写：{text:?}");
    assert!(text.contains('\n'), "格式化后应当分行：{text:?}");
    // 文档内容跟着走（否则保存下去的还是旧文本）
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).document_content_for_test()),
        Some(text.clone()),
        "服务层必须拿到格式化后的文本"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("格式化要有回执");
    assert!(message.contains("已格式化 1 条语句"), "{message}");
}

/// 没写完的语句**原样保留**，并在状态栏里如实报出来（不是“失败”，也不是悄悄丢掉）
#[gpui_kit::test]
fn a_half_written_statement_is_kept_verbatim_and_reported(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // 第二句缺右括号：解析不了——用户正打到一半就按了格式化
    let (shared, id) = shared_with_document(r"D:\sql\half.sql", "select a,b from t;\nselect (1");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.format_document(window, cx)));

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        text.ends_with("select (1"),
        "解析不了的语句要一个字节不动：{text:?}"
    );
    assert!(text.contains("FROM"), "能格式化的那句照常排：{text:?}");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("要说清楚哪条没动");
    assert!(message.contains("解析不了"), "{message}");
}

/// 有选区只格式化选区：**选区外一个字节不动**（别顺手把用户的草稿也排了）
#[gpui_kit::test]
fn formatting_a_selection_leaves_the_rest_untouched(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let original = "select a,b from t;\n\n-- 下面是草稿\nselect   c   from   u";
    let (shared, id) = shared_with_document(r"D:\sql\selection.sql", original);
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 只选中第一句（含分号：`select a,b from t;` 共 18 字节）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_selection_for_test(0..18, cx));
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.format_document(window, cx)));

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(text.contains("SELECT"), "选中的那句要排版：{text:?}");
    assert!(text.contains("FROM"), "选中的那句要排版：{text:?}");
    assert!(
        text.contains("-- 下面是草稿"),
        "选区外的注释不得被动：{text:?}"
    );
    assert!(
        text.contains("select   c   from   u"),
        "选区外的语句要原样保留：{text:?}"
    );
    // 选区跟着结果走（继续改还能接着改选中的那段）
    let range = cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx));
    assert!(!range.is_empty(), "格式化后选区应当还在");
}

/// 只读文档拒绝格式化，并给出原因
#[gpui_kit::test]
fn formatting_a_readonly_document_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(
            OpenRequest::untitled("select a,b from t", EditorMode::Sql)
                .with_read_only(crate::model::ReadOnly::editor_only()),
        )
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.format_document(window, cx)));

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "select a,b from t",
        "只读文档不得被改"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("只读"), "{message}");
}

/// 文本模式拒绝格式化（记事本不解析 SQL）
#[gpui_kit::test]
fn formatting_in_text_mode_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\notes.txt", "select a,b from t");
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.format_document(window, cx)));

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "select a,b from t",
        "文本模式不得被排版"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("文本模式"), "{message}");
}

// ===== B10：方言转译（⋯ 更多 ▾ ▸ 转译为）=====
//
// 转译是**破坏性**动作（改用户手上的文本），所以三道闸都要看得见：
// 只读拒绝 · 文本模式拒绝 · **源方言不明拒绝**（方向错不得）；真翻成功则文本、
// 服务层、状态栏三处都有据可查。

/// 工具栏按能力分层：分析组（执行计划 / 方言转换 / 模板）只在 SQL 模式出现
/// （原型 §2.2：分析组常驻；文本模式按能力表不解析 SQL，不摆）
#[gpui_kit::test]
fn the_toolbar_offers_the_analysis_group_only_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\more.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let analysis_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-explain").is_some()
            || cx.debug_bounds("editor-translate").is_some()
            || cx.debug_bounds("editor-template").is_some()
    };
    assert!(analysis_present(cx), "SQL 模式应当有分析组（执行计划/方言转换/模板）");

    shared.update(|service| service.set_mode(&id, EditorMode::General));
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!analysis_present(cx), "文本模式不解析 SQL，不得出现分析组");
}

/// 绑定 MySQL 连接 → 转译为 PostgreSQL：文本真被翻、服务层拿到新文本、状态栏报条数
#[gpui_kit::test]
fn transpiling_rewrites_the_text_and_reports_the_count(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tr.sql", "SELECT `a` FROM `t`;");
    let port = Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )]));
    shared.attach_connections(port);
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.transpile_document(engine::sql::SqlDialect::Postgres, window, cx)
        });
    });

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        text.contains("\"a\"") && text.contains("\"t\""),
        "选中的 MySQL 反引号应当换成 PG 的双引号：{text:?}"
    );
    assert!(!text.contains('`'), "反引号不该留在 PG 文本里：{text:?}");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).document_content_for_test()),
        Some(text.clone()),
        "服务层必须拿到转译后的文本（否则保存下去的还是旧的）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("转译要有回执");
    assert!(message.contains("已转译为 PostgreSQL"), "{message}");
    assert!(message.contains("1 条语句"), "{message}");
}

/// 未绑定连接：**拒绝**（不拿 Ansi 乱翻），原因可读
#[gpui_kit::test]
fn transpiling_without_a_connection_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tr.sql", "SELECT `a` FROM `t`;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.transpile_document(engine::sql::SqlDialect::Postgres, window, cx)
        });
    });

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "SELECT `a` FROM `t`;",
        "源方言不明时不得改文本"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("未绑定连接"), "{message}");
}

/// 只读文档拒绝转译
#[gpui_kit::test]
fn transpiling_a_readonly_document_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(
            OpenRequest::untitled("SELECT `a` FROM `t`", EditorMode::Sql)
                .with_read_only(crate::model::ReadOnly::editor_only()),
        )
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.transpile_document(engine::sql::SqlDialect::Postgres, window, cx)
        });
    });

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "SELECT `a` FROM `t`",
        "只读文档不得被改"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("只读"), "{message}");
}

/// 文本模式拒绝转译（记事本不解析 SQL）
#[gpui_kit::test]
fn transpiling_in_text_mode_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\notes.txt", "SELECT `a` FROM `t`");
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.transpile_document(engine::sql::SqlDialect::Postgres, window, cx)
        });
    });

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "SELECT `a` FROM `t`",
        "文本模式不得被转译"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("文本模式"), "{message}");
}

// ===== B10：执行计划（⋯ 更多 ▾ → 执行计划）=====
//
// 计划是**源库（或 DuckDB）自己的 EXPLAIN**，所以三件事都要看得见：前缀按方言生成、
// 结果落**新结果集**且贴「执行计划」标题、不抢用户正在看的那份。

/// 执行计划：带方言前缀、落新结果集、贴标题、不抢选中
#[gpui_kit::test]
fn the_execution_plan_lands_in_its_own_labelled_result_set(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("select a from t;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });

    // 先有一份普通结果（对照：计划不抢它）
    run_statement(
        cx,
        &panel,
        "select a from t",
        execution::ResultPlacement::Replace,
    );
    wait_for_result(cx, &panel);
    assert_eq!(result_tabs(cx, &panel).len(), 1);

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.explain_current(cx)));
    // 回执要在**提交那一刻**看：回填之后状态栏会被结果区的摘要接管（那是另一回事）
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("提交要有回执");
    assert!(message.contains("已提交执行计划"), "{message}");
    wait_for_all_pending(cx, &panel);

    let sql = seen.lock().expect("锁").last().cloned().unwrap_or_default();
    assert_eq!(sql, "EXPLAIN select a from t", "MySQL 方言的 EXPLAIN 前缀");
    let tabs = result_tabs(cx, &panel);
    assert_eq!(tabs.len(), 2, "计划该落成一份新结果集：{tabs:?}");
    assert_eq!(tabs[1].0, "执行计划", "标签要说清这是什么：{tabs:?}");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test(cx)),
        1,
        "计划是新结果集，执行后切到它（2026-10-27：每次执行切到最新）"
    );
}

/// 前缀按**连接的方言**生成：SQLite 要 `EXPLAIN QUERY PLAN`（裸 EXPLAIN 是 VM 指令）
#[gpui_kit::test]
fn the_plan_prefix_follows_the_connection_dialect(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    let mut sqlite = option("S_local", "S", "local");
    sqlite.db_type = "sqlite".to_string();
    shared.attach_connections(Rc::new(FakeConnections::new(vec![sqlite])));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("S_local".to_string()), cx)
        });
    });

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.explain_current(cx)));
    wait_for_all_pending(cx, &panel);

    let sql = seen.lock().expect("锁").last().cloned().unwrap_or_default();
    assert_eq!(sql, "EXPLAIN QUERY PLAN select 1", "SQLite 的计划树");
}

/// 未绑定连接：不猜源方言，拒绝并说明
#[gpui_kit::test]
fn explain_without_a_connection_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.explain_current(cx)));

    assert!(
        seen.lock().expect("锁").is_empty(),
        "源方言不明时不得发出任何语句"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("未绑定连接"), "{message}");
}

/// 文本模式拒绝执行计划（记事本不与数据库通信）
#[gpui_kit::test]
fn explain_in_text_mode_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::General);
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.explain_current(cx)));

    assert!(seen.lock().expect("锁").is_empty(), "文本模式不得发语句");
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("拒绝要留原因");
    assert!(message.contains("文本模式"), "{message}");
}

