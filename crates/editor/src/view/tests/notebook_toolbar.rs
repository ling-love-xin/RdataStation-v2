//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::rc::Rc;

use gpui_kit::{
    TestAppContext, VisualTestContext,
};

use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::{
    open_panel,
    shared_with_document,
};
use super::dialogs::{FakeConnections, option};
use super::notebook_win::NotebookRunner;

// ===== 【2d-笔记级】分析模式工具栏：会话 ▾ / 全部运行 / 重启 =====
//
// 笔记级动作（1c）：分析模式没有 SQL 组的“执行当前语句”，而是三个笔记级按钮——
// 会话（绑定连接）、全部运行（顺序跑可执行单元）、重启（关闭活跃内核）。
// 与 SQL 组模型无关：会话绑定是笔记级事实，执行是顺序跑全部单元。

/// 三个笔记级按钮只在分析模式出现（SQL / 通用模式不摆）
#[gpui_kit::test]
fn notebook_toolbar_actions_appear_only_in_analysis_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\notebook.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let action_present = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds(selector).is_some()
    };

    // SQL 模式：不出现（SQL 有自己的执行组）
    assert!(!action_present(cx, "editor-notebook-session"), "SQL 模式不摆会话按钮");
    assert!(!action_present(cx, "editor-notebook-run-all"), "SQL 模式不摆全部运行");
    assert!(!action_present(cx, "editor-notebook-restart"), "SQL 模式不摆重启");

    // 分析模式：三个笔记级按钮出现
    shared.update(|service| service.set_mode(&id, EditorMode::Analysis));
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(action_present(cx, "editor-notebook-session"), "分析模式应有会话按钮");
    assert!(action_present(cx, "editor-notebook-run-all"), "分析模式应有全部运行按钮");
    assert!(action_present(cx, "editor-notebook-restart"), "分析模式应有重启按钮");

    // 通用模式：不出现
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!action_present(cx, "editor-notebook-session"), "通用模式不摆会话按钮");
    assert!(!action_present(cx, "editor-notebook-run-all"), "通用模式不摆全部运行");
    assert!(!action_present(cx, "editor-notebook-restart"), "通用模式不摆重启");
}

/// 会话菜单切连接 → 会话绑定更新 + 全部有输出单元标 stale（T2d-2）
#[gpui_kit::test]
fn notebook_switch_connection_updates_session_and_marks_stale(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\cells.sql", "select 1;");
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Analysis);
    });
    let (panel, cx) = open_panel(cx, &shared, &id);
    // 面板构造会按文档内容解析出空笔记；测试注入带输出的单元要在 open 之后
    // （构造期解析会覆盖注入，顺序反了就白注入）
    shared.set_notebook_state(
        &id,
        crate::cell::NotebookState {
            session: crate::session::Session::default_session(),
            cells: vec![crate::cell::Cell {
                id: "c1".to_string(),
                language: crate::cell::CellLanguage::Sql,
                execution_source: None,
                body: "select 1;".to_string(),
                state: crate::cell::CellState::Done,
                outputs: vec![crate::cell::Output::Text("1".to_string())],
            }],
        },
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 走真实入口：面板方法（与按钮回调同一函数）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_switch_connection(Some("P_orders".to_string()), cx)
        });
    });

    let (bound, stale) = cx.update(|_window, _cx| {
        let state = shared.notebook_state(&id);
        (
            state.session.default_connection.clone(),
            state.stale_count(),
        )
    });
    assert_eq!(bound.as_deref(), Some("P_orders"), "会话绑定要换成新连接");
    assert_eq!(stale, 1, "有输出的单元要标 stale");
}

/// 全部运行：串行队列跑所有可执行单元（SQL 走 run_cell 执行链），
/// Markdown / 空单元跳过；跑完一个自动接下一个。
#[gpui_kit::test]
fn notebook_run_all_runs_only_executable_cells(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(NotebookRunner {
        seen: seen.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    // 构造期会按空正文建空笔记；注入 4 个单元要在 open 之后
    shared.set_notebook_state(
        &id,
        crate::cell::NotebookState {
            session: crate::session::Session::default_session(),
            cells: vec![
                crate::cell::Cell::new("c1", crate::cell::CellLanguage::Sql, "select 1;"),
                crate::cell::Cell::new("c2", crate::cell::CellLanguage::Markdown, "# 备注"),
                crate::cell::Cell::new("c3", crate::cell::CellLanguage::Sql, "select 3;"),
                crate::cell::Cell::new("c4", crate::cell::CellLanguage::Sql, ""),
            ],
        },
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_all(cx));
    });

    // headless 不自动跑泵：手动收执行回执（与 exec pump 同一条 drain 路径），
    // 让串行队列逐步推进到两个 SQL 单元都跑完
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| panel.drain_cell_notes(cx));
            seen.lock().expect("锁").len() == 2
        });
        if done {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "单元执行迟迟没进执行链");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let seen = seen.lock().expect("锁").clone();
    assert_eq!(
        seen,
        vec!["default:c1".to_string(), "default:c3".to_string()],
        "只有可执行的 SQL 单元进执行链（按序串行），Markdown 和空单元跳过"
    );
}

/// 重启：没有活跃内核时如实提示（不假装重启了东西）
#[gpui_kit::test]
fn notebook_restart_without_active_kernels_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\cells.sql", "select 1;");
    shared.update(|service| service.set_mode(&id, EditorMode::Analysis));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_restart_kernel(cx));
    });

    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert_eq!(
        message.as_deref(),
        Some("没有活跃内核，无需重启"),
        "没有内核时不能假装重启成功"
    );
}

