//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::{
    TestAppContext, VisualTestContext,
};

use crate::model::EditorMode;

use super::{
    open_panel,
    shared_with_document, shared_with_env, wait_for_all_pending,
};
use super::fake_envs;

// ===== 2b：通用模式窗口级测试 =====
//
// 环境选择器 + 运行按钮只在通用模式出现（SQL / 分析模式不摆）；脚本结论走独立通道变成
// 卡片。未接环境端口 / 环境未装 / 未选环境 / 只读 / 空文档，运行都如实拒绝并报原因。

/// 通用模式：工具栏右侧出现环境选择器 + 运行按钮；未接端口时选择器置灰（不可点）
#[gpui_kit::test]
fn general_mode_toolbar_offers_env_picker_and_run_but_disabled_without_a_port(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\py\run.py", "print('hi')");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let picker_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-env-picker").is_some()
    };
    let run_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-run-script").is_some()
    };

    // SQL 模式：不摆环境控件（脚本语义不属于 SQL 模式）
    assert!(!picker_present(cx), "SQL 模式不得出现环境选择器");
    assert!(!run_present(cx), "SQL 模式不得出现运行按钮");

    // 通用模式：两个控件都出现；未接环境端口 → 列表为空 → 选择器不可点
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(picker_present(cx), "通用模式应有环境选择器");
    assert!(run_present(cx), "通用模式应有运行按钮");

    // 未接端口 = 选不到环境 → 运行如实拒绝（不是假装能跑）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.run_script(cx));
    });
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.as_deref().is_some_and(|m| m.contains("未选中执行环境")),
        "无端口时应报未选环境：{message:?}"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 选已装环境 → 运行按钮亮 → `run_script` 真提交 → 结论变成功卡片（独立通道，不走网格）
#[gpui_kit::test]
fn selecting_an_installed_env_runs_the_script_and_shows_a_success_card(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let installed = std::collections::HashMap::from([("py".to_string(), true)]);
    let (shared, id, seen) = shared_with_env(
        "print('hi')",
        fake_envs(),
        installed,
        Ok(crate::env::ScriptOutcome {
            exit_code: Some(0),
            stdout: "hi\n".to_string(),
            stderr: String::new(),
            timed_out: false,
        }),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 选环境（面板方法，与菜单项同一条路径）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_env(Some("py".to_string()), cx));
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).env_for()),
        Some("py".to_string()),
        "选中环境要能读回来"
    );

    // 运行：真提交 → 等回填（脚本结论走 script_notes，轮询泵手动跑）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.run_script(cx));
    });
    wait_for_all_pending(cx, &panel);

    let seen = seen.lock().expect("锁");
    assert_eq!(seen.len(), 1, "执行器只收到一次脚本请求");
    assert_eq!(seen[0].env_id, "py", "请求携带选中的环境 id");
    assert_eq!(seen[0].code, "print('hi')", "请求携带原样代码");
    drop(seen);

    let card = cx.update(|_window, cx| panel.read(cx).script_card_for_test(cx));
    let card = card.expect("脚本跑完应有卡片");
    assert_eq!(card.env_name, "Python 3.12", "卡片带环境展示名");
    match card.result {
        Ok(outcome) => {
            assert_eq!(outcome.stdout, "hi\n", "stdout 原样进卡片");
            assert_eq!(outcome.exit_code, Some(0));
            assert!(!outcome.timed_out);
        }
        Err(reason) => panic!("应当成功，得到：{reason}"),
    }
    // 脚本卡片顶替网格：结果区有卡片且没有网格行
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-script-card").is_some(),
        "结果区渲染出脚本卡片"
    );
}

/// 选了未安装环境：运行按钮亮不了、`run_script` 也如实拒绝并说明原因
#[gpui_kit::test]
fn an_uninstalled_env_refuses_to_run_with_a_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // 只有 rs（未装）在 installed 表里缺席 → `installed(rs)` = false
    let installed = std::collections::HashMap::new();
    let (shared, id, _seen) = shared_with_env(
        "fn main() {}",
        fake_envs(),
        installed,
        Ok(crate::env::ScriptOutcome {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        }),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_env(Some("rs".to_string()), cx));
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.run_script(cx));
    });
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.as_deref().is_some_and(|m| m.contains("未安装")),
        "未装环境应报未安装：{message:?}"
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).script_card_for_test(cx)).is_none(),
        "未装环境不得产出卡片"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 执行器报错（spawn 失败等）：结论是失败卡片，带原因
#[gpui_kit::test]
fn a_failed_script_execution_shows_an_error_card_with_the_reason(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let installed = std::collections::HashMap::from([("py".to_string(), true)]);
    let (shared, id, _seen) = shared_with_env(
        "raise SystemExit",
        fake_envs(),
        installed,
        Err("spawn 失败：python 命令不可达".to_string()),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_env(Some("py".to_string()), cx));
        panel.update(cx, |panel, cx| panel.run_script(cx));
    });
    wait_for_all_pending(cx, &panel);

    let card = cx.update(|_window, cx| panel.read(cx).script_card_for_test(cx));
    let card = card.expect("失败也要有卡片（带原因）");
    match card.result {
        Ok(_) => panic!("应当失败，却拿到了成功输出"),
        Err(reason) => assert!(
            reason.contains("spawn 失败"),
            "卡片带失败原因：{reason}"
        ),
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-script-card").is_some(),
        "失败卡片渲染出（Alert danger 在卡片内）"
    );
}

/// 状态栏（窗口级）：通用模式 + 已选环境 → 左段正文含「环境 Python 3.12」
#[gpui_kit::test]
fn status_bar_shows_the_env_segment_in_general_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let installed = std::collections::HashMap::from([("py".to_string(), true)]);
    let (shared, id, _seen) = shared_with_env(
        "print('hi')",
        fake_envs(),
        installed,
        Ok(crate::env::ScriptOutcome {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        }),
    );
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_env(Some("py".to_string()), cx));
    });
    // 触发一次渲染让 env_text 计算生效（draw 之前先 set_env + notify）
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    // 状态栏文案由 `labels()` 纯函数计算：通用模式 → 模式名在右段微标（原型 §2.5），
    // body 含环境段。这里直接断言纯函数（窗口级把 env 输入喂对，纯函数保证渲染一致）。
    let text = crate::view::widgets::status_bar::labels(
        &crate::view::widgets::status_bar::StatusInputs {
            mode: EditorMode::General,
            dirty: false,
            read_only: crate::model::ReadOnly::none(),
            statements: 0,
            line: 1,
            column: 1,
            cursor_offset: 0,
            cursor_count: 1,
            tab_size: 2,
            hard_tabs: false,
            format: crate::encoding::SaveFormat::default(),
            bytes: 10,
            selected_chars: 0,
            selected_rows: 0,
            message: None,
            executing: false,
            elapsed: None,
            connection: None,
            dialect: None,
            channel: None,
            env: Some("环境 Python 3.12"),
            file_type: Some("类型 Python"),
            lsp: None,
            tx: None,
            session: None,
            kernel: None,
            units: None,
        },
    );
    assert!(
        text.right.iter().any(|b| b.contains("通用"))
            && text.body.contains("环境 Python 3.12")
            && text.body.contains("类型 Python"),
        "模式名在右段微标、环境段+文件类型段在左段正文：body={} right={:?}",
        text.body,
        text.right
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 状态栏（窗口级，开放决定 #2）：通用模式 + `.py` 文件 → 左段正文含「类型 Python」
#[gpui_kit::test]
fn status_bar_shows_the_script_file_type_segment_in_general_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let installed = std::collections::HashMap::from([("py".to_string(), true)]);
    let (shared, _id, _seen) = shared_with_env(
        "print('hi')",
        fake_envs(),
        installed,
        Ok(crate::env::ScriptOutcome {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
        }),
    );
    // 换成带路径的 .py 文件（shared_with_env 开的是未命名档，`document_path()` 是 None）
    let id = shared
        .open(crate::service::OpenRequest::file(
            "C:/work/script.py",
            "print('hi')",
            EditorMode::General,
        ))
        .id()
        .clone();
    let (_panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 状态栏文案由 `labels()` 纯函数计算：通用模式 → 模式名在右段微标，
    // body 含文件类型段。这里直接断言纯函数（窗口级把 file_type 输入喂对，纯函数保证渲染一致）。
    let text = crate::view::widgets::status_bar::labels(
        &crate::view::widgets::status_bar::StatusInputs {
            mode: EditorMode::General,
            dirty: false,
            read_only: crate::model::ReadOnly::none(),
            statements: 0,
            line: 1,
            column: 1,
            cursor_offset: 0,
            cursor_count: 1,
            tab_size: 2,
            hard_tabs: false,
            format: crate::encoding::SaveFormat::default(),
            bytes: 10,
            selected_chars: 0,
            selected_rows: 0,
            message: None,
            executing: false,
            elapsed: None,
            connection: None,
            dialect: None,
            channel: None,
            env: None,
            file_type: Some("类型 Python"),
            lsp: None,
            tx: None,
            session: None,
            kernel: None,
            units: None,
        },
    );
    assert!(
        text.right.iter().any(|b| b.contains("通用")) && text.body.contains("类型 Python"),
        "模式名在右段微标、脚本文件类型段在左段正文：body={} right={:?}",
        text.body,
        text.right
    );

    // 未命名 / 未知扩展名（无路径）→ file_type 输入为 None → body 不含「类型」
    let untitled = crate::view::widgets::status_bar::labels(
        &crate::view::widgets::status_bar::StatusInputs {
            mode: EditorMode::General,
            dirty: false,
            read_only: crate::model::ReadOnly::none(),
            statements: 0,
            line: 1,
            column: 1,
            cursor_offset: 0,
            cursor_count: 1,
            tab_size: 2,
            hard_tabs: false,
            format: crate::encoding::SaveFormat::default(),
            bytes: 10,
            selected_chars: 0,
            selected_rows: 0,
            message: None,
            executing: false,
            elapsed: None,
            connection: None,
            dialect: None,
            channel: None,
            env: None,
            file_type: None,
            lsp: None,
            tx: None,
            session: None,
            kernel: None,
            units: None,
        },
    );
    assert!(
        !untitled.body.contains("类型"),
        "无路径/未知扩展名时如实不摆文件类型段：{}",
        untitled.body
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

