//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    TestAppContext,
};

use crate::execution::{self, QueryData, QueryRunner};
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::open_panel;
use super::dialogs::{FakeConnections, option};

// ===== 【2d】分析笔记本：窗口级测试 =====

/// 假执行器：只实现 `run_cell`（单元执行走这一条），记录请求并回成功结论
pub(crate) struct NotebookRunner {
    pub(crate) seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl QueryRunner for NotebookRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: crate::channel::ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        Ok(QueryData::default())
    }
    fn run_cell(
        &self,
        request: &crate::cell::CellRequest,
    ) -> Result<crate::cell::CellOutcome, String> {
        self.seen.lock().unwrap().push(format!("{}:{}", request.session_id, request.cell_id));
        Ok(crate::cell::CellOutcome {
            sql: request.sql.clone(),
            elapsed_ms: 3,
            affected_rows: None,
            truncated: false,
            result: Ok(QueryData {
                columns: vec!["n".to_string()],
                rows: vec![vec!["1".to_string()]],
                ..Default::default()
            }),
        })
    }
}

/// 空态引导：分析模式的空笔记不是空白，而是引导卡（两个新增入口真在）
#[gpui_kit::test]
fn analysis_mode_shows_the_empty_state_guide(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(NotebookRunner {
        seen: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 层真渲染：空态引导卡的两个入口按钮都在（headless 弹层不可靠，用 debug_bounds）
    assert!(
        cx.update(|_window, cx| panel.read(cx).notebook_state().cells.is_empty()),
        "新建笔记 = 空单元"
    );
    assert!(cx.debug_bounds("editor-nb-empty-sql").is_some(), "空态要有 SQL 入口");
    assert!(cx.debug_bounds("editor-nb-empty-text").is_some(), "空态要有文本入口");

    // 走面板方法（与按钮回调同一条路）：加一个 SQL 单元 → 空态消失、卡片出现
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).notebook_state().cells.len()),
        1
    );
    assert!(cx.debug_bounds("editor-nb-add-cell").is_some(), "列表尾有插入入口");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 单元动作（上移 / 下移 / 复制 / 删除 / 转类型）全真实生效
#[gpui_kit::test]
fn notebook_cell_actions_are_real(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 加三个单元：SQL / 文本 / SQL
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Markdown, window, cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells.len(), 3);
    let first = cells[0].id.clone();
    let second = cells[1].id.clone();
    assert_eq!(cells[1].language, crate::cell::CellLanguage::Markdown);

    // 转类型：文本 → SQL（正文原样，类型翻转）
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_convert_cell(&id, &second, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells[1].language, crate::cell::CellLanguage::Sql, "Markdown 转 SQL");

    // 复制第一个：副本插在后面，id 不同、正文原样
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_duplicate_cell(&id, &first, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells.len(), 4);
    assert_eq!(cells[1].id, format!("{first}-copy"), "副本紧跟原单元");
    assert_eq!(cells[1].body, cells[0].body);

    // 上移第三个：与第二个换位
    let third = cells[2].id.clone();
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_move_cell(&id, &third, -1, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells[1].id, third, "上移一位");

    // 删除第二个（原来的 third）：列表变短、id 消失
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_delete_cell(&id, &third, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells.len(), 3);
    assert!(!cells.iter().any(|cell| cell.id == third));
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 运行 SQL 单元：走 `run_cell` 假执行器 → 回执经泵落到单元输出（内联 Table 投影）
#[gpui_kit::test]
fn running_a_sql_cell_applies_the_inline_output(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(NotebookRunner {
        seen: seen.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 加 SQL 单元 + 一个文本单元
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Markdown, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    let sql_id = cells[0].id.clone();
    let text_id = cells[1].id.clone();

    // 运行 SQL 单元：提交成功 → 状态 Running
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &sql_id, cx));
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells[0].state, crate::cell::CellState::Running);

    // 泵回执：Done + 内联 Table 输出（1 行 × 1 列）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let done = loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_cell_notes(cx);
                panel.notebook_state().cells[0].state == crate::cell::CellState::Done
            })
        });
        if done || std::time::Instant::now() > deadline {
            break done;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(done, "回执要落到单元上（Done）");
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        &[format!("default:{sql_id}")],
        "执行器收到会话 + 单元 id"
    );
    match &cells[0].outputs[0] {
        crate::cell::Output::Table(entry) => {
            assert_eq!(entry.rows.len(), 1);
            assert_eq!(entry.columns, vec!["n".to_string()]);
            assert_eq!(entry.lineage.as_deref(), Some("单元执行"));
        }
        other => panic!("SQL 单元成功回执要拼成内联 Table，实际：{other:?}"),
    }

    // 文本单元不可执行：如实拒绝并给原因（不假装能跑）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &text_id, cx));
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells[1].state, crate::cell::CellState::Idle, "文本单元不可执行");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 【T2d-5】打开 `.rdsnote`：正文 JSON 解析进笔记本持有器（单元回来、输出清空）
#[gpui_kit::test]
fn opening_an_rdsnote_populates_the_notebook(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // 先造一份 .rdsnote 内容（与 persist 测试同一形状：会话 + 两个单元）
    let json = r##"{
      "version": 1,
      "session": { "id": "default", "default_connection": null, "env": null },
      "cells": [
        { "id": "c1", "kind": "sql", "body": "select 1;" },
        { "id": "c2", "kind": "text", "body": "# 备注" }
      ]
    }"##;
    let dir = std::env::temp_dir().join(format!("rds_editor_rdsnote_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("note.rdsnote");
    std::fs::write(&path, json).expect("写盘");

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Analysis)
        .expect("打开")
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 文档正文确为 .rdsnote JSON（服务层没丢内容）
    let doc_content = shared.service().find(&id).map(|doc| doc.content().to_string());
    assert!(doc_content.as_deref().unwrap_or("").contains("\"version\": 1"), "服务层要留着 JSON 正文");

    // 笔记本持有器：单元按文件回来（输出 / 状态不跟随）
    let state = cx.update(|_window, cx| panel.read(cx).notebook_state());
    assert_eq!(state.cells.len(), 2);
    assert_eq!(state.cells[0].id, "c1");
    assert_eq!(state.cells[0].language, crate::cell::CellLanguage::Sql);
    assert_eq!(state.cells[0].body, "select 1;");
    assert_eq!(state.cells[1].body, "# 备注");
    assert!(state.cells.iter().all(|cell| cell.outputs.is_empty()), "输出不跟随文件");

    // 层真渲染：两个单元卡片都画出来了（▶ 按钮按单元 id 存在）
    assert!(
        cx.debug_bounds("editor-nb-run-c1").is_some() || cx.debug_bounds("editor-nb-menu-c1").is_some(),
        "SQL 单元卡片渲染出来（按 id 找得到）"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 【#14】多输出渲染：一个单元带「文本 + Markdown + 图片 + HTML 表格」四条输出，
/// 渲染后每一条都独立成块（业务键 id 逐个可找）；类型转化正确性由 media.rs 单测负责，
/// 这里只验「输出序列真的画出来了」
#[gpui_kit::test]
fn analysis_cell_renders_multiple_output_blocks(cx: &mut TestAppContext) {
    use base64::Engine as _;
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 塞一个带四条输出的单元（顺序 = 到达顺序；图片用真实 1×1 PNG，
    // 渲染期解码不 panic）
    let mut state = crate::cell::NotebookState::new();
    let mut cell = crate::cell::Cell::new("c1", crate::cell::CellLanguage::Sql, "1+1");
    cell.mark_done(crate::cell::Output::Text("2\n".to_string()));
    cell.mark_done(crate::cell::Output::Markdown("**加粗** 正文".to_string()));
    cell.mark_done(crate::cell::Output::Image {
        bytes: base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")
            .expect("1×1 PNG base64"),
        mime: "image/png".to_string(),
    });
    cell.mark_done(crate::cell::Output::TableData {
        columns: vec!["a".to_string(), "b".to_string()],
        rows: vec![vec!["1".to_string(), "2".to_string()]],
    });
    state.cells = vec![cell];
    shared.set_notebook_state(&id, state);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 四条输出块按序渲染（try_find：不在树里返回 None，隐藏/被裁也算 in-tree）
    for ix in 0..4 {
        assert!(
            cx.update(|window, _| window.try_find(format!("editor-nb-output-{ix}"))).is_some(),
            "输出块 {ix} 应渲染出来"
        );
    }
    // 状态栏信息：单元 Done（渲染路径读的是共享状态）
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells[0].state, crate::cell::CellState::Done);
    assert_eq!(cells[0].outputs.len(), 4);
}

/// 【#16】右下角指示器：可执行单元有执行源按钮（默认连接），Markdown 单元没有
#[gpui_kit::test]
fn cell_footer_shows_source_button_only_for_executable_cells(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_connections(std::rc::Rc::new(FakeConnections::new(vec![
        option("P_orders", "P", "orders"),
        option("G_analytics", "G", "analytics"),
    ])));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // SQL 单元 + Markdown 单元各一
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Markdown, window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    let sql_id = cells[0].id.clone();
    let md_id = cells[1].id.clone();

    // 可执行单元：右下角有执行源按钮；Markdown 单元没有
    assert!(
        cx.update(|window, _| window.try_find(format!("editor-nb-source-{sql_id}")))
            .is_some(),
        "SQL 单元要渲染执行源按钮"
    );
    assert!(
        cx.update(|window, _| window.try_find(format!("editor-nb-source-{md_id}")))
            .is_none(),
        "Markdown 单元没有执行源按钮"
    );
}

/// 【#16】切换执行源：面板方法写 shared，同值重设 = 无操作
#[gpui_kit::test]
fn switching_execution_source_persists_to_shared(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_connections(std::rc::Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    let cell_id = cells[0].id.clone();
    assert_eq!(cells[0].execution_source, None, "新单元执行源 = 会话默认");

    // 切到指定连接：shared 落库
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_set_execution_source(
                &id,
                &cell_id,
                Some(crate::cell::ExecutionSource::Connection("P_orders".to_string())),
                cx,
            );
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(
        cells[0].execution_source,
        Some(crate::cell::ExecutionSource::Connection("P_orders".to_string()))
    );

    // 回会话默认
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_set_execution_source(&id, &cell_id, None, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells[0].execution_source, None, "可以切回默认");
}

/// 【#17】SQL 单元的执行连接：**执行源指定优先**，否则文档默认绑定
#[gpui_kit::test]
fn cell_connection_prefers_execution_source_over_document_default(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    // bind_connection 要先 ensure_connected（连接端口注入才有建连能力）
    shared.attach_connections(std::rc::Rc::new(FakeConnections::new(vec![
        option("P_A", "P", "a"),
        option("P_B", "P", "b"),
    ])));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 文档绑定 P_A（默认）；加 SQL 单元
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_A".to_string()), cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    let cell_id = cells[0].id.clone();
    let request = crate::cell::CellRequest {
        session_id: "default".to_string(),
        cell_id: cell_id.clone(),
        sql: "select 1".to_string(),
        conn_id: None,
    };
    // 未指定执行源 → 文档默认
    assert_eq!(shared.cell_connection_for(&id, &request).as_deref(), Some("P_A"));

    // 指定执行源 → 它优先
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_set_execution_source(
                &id,
                &cell_id,
                Some(crate::cell::ExecutionSource::Connection("P_B".to_string())),
                cx,
            );
        });
    });
    let request = crate::cell::CellRequest {
        conn_id: Some("P_B".to_string()),
        ..request
    };
    assert_eq!(shared.cell_connection_for(&id, &request).as_deref(), Some("P_B"));
}

/// 【#17】真内核链路（shared 层，不依赖 pump）：启动 → 执行 → 事件轮询 → mime 分发
///
/// `#[ignore]`：需要本机 ipykernel；跑法见 rds-kernel 的集成测试。
#[gpui_kit::test]
#[ignore = "需要本机 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
fn python_cell_runs_via_real_kernel_and_dispatches_media(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    // #20：注册表化后真实命令 = RDS_KERNEL_PYTHON 指向的解释器（与 rds-kernel 集成测试同口径）
    let python = std::env::var_os("RDS_KERNEL_PYTHON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("python"));
    shared
        .ensure_kernel("python3", &python.to_string_lossy())
        .expect("内核启动（RDS_KERNEL_PYTHON 是否指向带 ipykernel 的解释器？）");
    shared
        .kernel_execute("python3", "1 + 1")
        .expect("提交执行");

    // 轮询事件直到 ExecuteReply（真时钟；内核回执通常 <2s）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut outputs: Vec<crate::cell::Output> = Vec::new();
    let mut status: Option<String> = None;
    loop {
        for event in shared.kernel_drain("python3") {
            match event {
                rds_kernel::KernelEvent::Display(media) => {
                    outputs.extend(crate::media::media_to_outputs(&media));
                }
                rds_kernel::KernelEvent::Stream { text, .. } => {
                    outputs.push(crate::cell::Output::Text(text));
                }
                rds_kernel::KernelEvent::ExecuteReply { status: s, .. } => {
                    status = Some(s);
                }
                rds_kernel::KernelEvent::Error { ename, evalue, .. } => {
                    outputs.push(crate::cell::Output::Error(format!("{ename}: {evalue}")));
                }
                rds_kernel::KernelEvent::Failed(reason) => panic!("内核通道失败：{reason}"),
                rds_kernel::KernelEvent::Status(_) => {}
            }
        }
        if status.is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "内核 20s 未回执（outputs={outputs:?}）"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // `1 + 1` 的富输出：text/plain "2" → 文本输出；执行回复 ok
    assert_eq!(status.as_deref(), Some("ok"));
    assert!(
        outputs.iter().any(|o| matches!(o, crate::cell::Output::Text(t) if t.trim() == "2")),
        "text/plain 结果要分发成文本输出：{outputs:?}"
    );
    shared.kernel_shutdown("python3");
}

