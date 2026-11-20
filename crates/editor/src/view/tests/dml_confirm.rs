//! 写操作确认（执行语义矩阵第 8 行 · 原型 ⑨）
//!
//! headless 下鼠标点击到不了对话框层（本仓既有口径），所以流程是：
//! 执行 → 断言「没入队、挂起成立」→ 直调确认（与按钮回调同一函数）→ 断言「入队且带确认标记」。

use std::sync::Arc;
use std::sync::Mutex;

use gpui_kit::TestAppContext;

use crate::channel::ExecChannel;
use crate::execution::{QueryData, QueryRunner};
use crate::model::EditorMode;
use crate::view::host::EditorHostPanel;

use super::{open_panel, shared_with_document};

/// 假执行器：只对 UPDATE / DELETE 报「需要确认」，并记录每次执行的选项
struct DmlRunner {
    options: Arc<Mutex<Vec<crate::execution::RunOptions>>>,
}

impl QueryRunner for DmlRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        options: crate::execution::RunOptions,
    ) -> Result<QueryData, String> {
        self.options.lock().expect("锁").push(options);
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 1,
            truncated: false,
            affected_rows: Some(1),
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }

    fn dml_confirm(
        &self,
        _connection: Option<&str>,
        sql: &str,
    ) -> Option<engine::services::policy_guard::DmlConfirm> {
        // 模拟引擎：环境配了「DML 需确认」，只盯 UPDATE / DELETE
        let upper = sql.trim_start().to_ascii_uppercase();
        let statement = if upper.starts_with("UPDATE") {
            "UPDATE"
        } else if upper.starts_with("DELETE") {
            "DELETE"
        } else {
            return None;
        };
        Some(engine::services::policy_guard::DmlConfirm {
            environment: "生产".to_string(),
            statement,
            preview: sql.trim().to_string(),
        })
    }
}

fn shared_with_dml_runner(
    content: &str,
) -> (
    crate::shared::EditorShared,
    crate::model::DocumentId,
    Arc<Mutex<Vec<crate::execution::RunOptions>>>,
) {
    let (shared, id) = shared_with_document("dml-confirm.sql", content);
    let options = Arc::new(Mutex::new(Vec::new()));
    shared.attach_runner(Arc::new(DmlRunner {
        options: options.clone(),
    }));
    (shared, id, options)
}

/// 等作业真的跑到执行器上（worker 在另一条线程）
fn wait_for_options(
    cx: &mut TestAppContext,
    panel: &gpui_kit::Entity<EditorHostPanel>,
    expect: usize,
    options: &Arc<Mutex<Vec<crate::execution::RunOptions>>>,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        cx.update(|cx| {
            panel.update(cx, |panel, cx| panel.drain_exec_results(cx));
        });
        if options.lock().expect("锁").len() >= expect {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "作业迟迟没跑到执行器上（已 {expect} 条）"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// UPDATE 先挂起等确认：不点「执行」不入队；取消只清挂起
#[gpui_kit::test]
fn update_waits_for_the_dml_confirm(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, options) = shared_with_dml_runner("update t set a = 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let pending = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute_preferring_selection(cx);
            panel.pending_dml_confirm_for_test()
        })
    });
    assert!(pending, "UPDATE 在配置了写确认的环境里必须先挂起");
    assert!(
        options.lock().expect("锁").is_empty(),
        "确认前不该有任何作业入队"
    );

    // 取消：只清挂起，仍然不跑
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.cancel_dml_confirm(cx));
    });
    assert!(
        !cx.update(|_window, cx| panel.read(cx).pending_dml_confirm_for_test()),
        "取消后不该还挂着"
    );
    assert!(options.lock().expect("锁").is_empty(), "取消后仍然不入队");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 确认后真的入队，且选项里带 `dml_confirmed = true`（引擎据此放行）
#[gpui_kit::test]
fn confirmed_update_runs_with_the_flag(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, options) = shared_with_dml_runner("update t set a = 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx));
    });
    let finished = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.finish_dml_confirm(cx))
    });
    assert!(finished, "确认后应当提交成功");
    wait_for_options(cx, &panel, 1, &options);
    assert!(
        !cx.update(|_window, cx| panel.read(cx).pending_dml_confirm_for_test()),
        "确认后挂起要清掉（不能再弹第二次）"
    );
    assert!(
        options.lock().expect("锁")[0].dml_confirmed,
        "确认后的执行必须带 dml_confirmed，否则引擎会再拒一次"
    );
}

/// 反例：INSERT / SELECT 不弹（确认只覆盖 UPDATE / DELETE）
#[gpui_kit::test]
fn insert_and_select_do_not_ask(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    for content in ["select 1;", "insert into t values (1);"] {
        let (shared, id, options) = shared_with_dml_runner(content);
        let (panel, cx) = open_panel(cx, &shared, &id);
        let pending = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.execute_preferring_selection(cx);
                panel.pending_dml_confirm_for_test()
            })
        });
        assert!(!pending, "「{content}」不该弹写确认");
        wait_for_options(cx, &panel, 1, &options);
        assert!(
            !options.lock().expect("锁")[0].dml_confirmed,
            "没走过确认的执行不应带确认标记"
        );
    }
}

#[allow(dead_code)]
fn _assert_types(_: EditorMode, _: &EditorHostPanel) {}
