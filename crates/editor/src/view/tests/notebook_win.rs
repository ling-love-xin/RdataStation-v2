//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use gpui_kit::TestAppContext;
use gpui_kit::test::TestWindowExt as _;

use crate::execution::{self, QueryData, QueryRunner};
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::dialogs::{FakeConnections, option};
use super::open_panel;

// ===== 【2d】分析笔记本：窗口级测试 =====

/// 拼 `'static` debug 选择器（`debug_bounds` 要求静态借用；测试进程生命周期内泄漏无妨）
fn cell_selector(pattern: &str, id: &str) -> &'static str {
    Box::leak(format!("{pattern}{id}").into_boxed_str())
}

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
        self.seen
            .lock()
            .unwrap()
            .push(format!("{}:{}", request.session_id, request.cell_id));
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
    assert!(
        cx.debug_bounds("editor-nb-empty-sql").is_some(),
        "空态要有 SQL 入口"
    );
    assert!(
        cx.debug_bounds("editor-nb-empty-text").is_some(),
        "空态要有文本入口"
    );

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
    // 2026-11-09 拍板语言面收敛后，底部插入行是三个快捷入口（SQL / Python / 文本）
    assert!(
        cx.debug_bounds("editor-nb-add-sql").is_some(),
        "列表尾有 SQL 插入入口"
    );
    assert!(
        cx.debug_bounds("editor-nb-add-python").is_some(),
        "列表尾有 Python 插入入口"
    );
    assert!(
        cx.debug_bounds("editor-nb-add-text").is_some(),
        "列表尾有文本插入入口"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 【语言面收敛（2026-11-09 拍板：Python / Markdown / SQL）】Python 单元有创建入口：
/// 空态引导卡带「＋ Python 单元」，走面板生产入口加出来的是带编辑器实例的 Python 单元
#[gpui_kit::test]
fn python_cells_have_a_creation_entry(cx: &mut TestAppContext) {
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

    // 空态第三个入口在（此前 Python 单元没有任何创建入口）
    assert!(
        cx.debug_bounds("editor-nb-empty-python").is_some(),
        "空态要有 Python 入口"
    );

    // 与按钮回调同一条路：加一个 Python 单元 → 语言正确 + 编辑器实例就位
    //（语言服务会话依赖宿主端口，未注入 = 如实没有，不在本用例范围）
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].language, crate::cell::CellLanguage::Python);
    let cell_id = cells[0].id.clone();
    assert!(
        cx.update(|_window, cx| {
            panel.read(cx).cell_editors.contains_key(&cell_id)
        }),
        "Python 单元要有自己的内核编辑器实例"
    );
}

/// 【Markdown 预览】切到渲染态出 TextView 预览、切回源码态消失；非 Markdown 单元无操作
#[gpui_kit::test]
fn markdown_cells_can_toggle_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Markdown, window, cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    let md_id = cells
        .iter()
        .find(|cell| cell.language.is_markdown())
        .expect("有 Markdown 单元")
        .id
        .clone();
    let sql_id = cells
        .iter()
        .find(|cell| !cell.language.is_markdown())
        .expect("有 SQL 单元")
        .id
        .clone();

    // 预览前：渲染态不在（编辑器在不另断——`editor-nb-cell-source-*` 的 id+test_support
    // 观察路径在 gpui-fast 下已登记失真，见升级报告 §7 第 5 条；渲染分支由代码路径覆盖）
    let preview_sel = cell_selector("editor-nb-md-preview-", &md_id);
    assert!(cx.debug_bounds(preview_sel).is_none());

    // 与按钮回调同一条路：开预览 → 渲染态出现；面板状态入集
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_toggle_cell_preview(&id, &md_id, cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds(preview_sel).is_some());
    assert!(
        cx.update(|_window, cx| panel.read(cx).cell_markdown_preview.contains(&md_id)),
        "面板状态入集"
    );

    // SQL 单元走同一入口 = 无操作（预览是 Markdown 专属）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_toggle_cell_preview(&id, &sql_id, cx)
        })
    });
    assert!(
        cx.update(|_window, cx| !panel.read(cx).cell_markdown_preview.contains(&sql_id)),
        "SQL 单元不受预览切换影响"
    );

    // 切回源码态：预览消失、集合清空（编辑器回来的渲染分支由 else 侧代码路径覆盖）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_toggle_cell_preview(&id, &md_id, cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds(preview_sel).is_none());
    assert!(
        cx.update(|_window, cx| !panel.read(cx).cell_markdown_preview.contains(&md_id)),
        "集合清空"
    );
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
    assert_eq!(
        cells[1].language,
        crate::cell::CellLanguage::Sql,
        "Markdown 转 SQL"
    );

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

/// 【B2 / B3】复制出来的单元 id 必须唯一；转类型 / 删除要清掉该单元的旧错误线
///
/// 区分性证明（两条断言在旧实现下都会红）：
/// - 旧实现两次复制都产出 `<id>-copy`；同 id 会让 `ensure_cell_editor` 见 id 已存在直接
///   return，第二个副本没有编辑器实例、两个单元共用正文；
/// - 旧实现的 `cell_error_marks` 只在重跑与回执成功时清，转换与删除都不碰它——SQL 单元
///   失败（有波浪线）→ 转 Markdown → 转回 SQL，波浪线原地复活。
#[gpui_kit::test]
fn cell_copy_ids_are_unique_and_convert_or_delete_clears_error_marks(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
        });
    });
    let first = cx.update(|_window, cx| panel.read(cx).notebook_state().cells[0].id.clone());

    // 连点两次「复制单元」
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_duplicate_cell(&id, &first, window, cx);
            panel.notebook_duplicate_cell(&id, &first, window, cx);
        });
    });
    let ids: Vec<String> = cx.update(|_window, cx| {
        panel
            .read(cx)
            .notebook_state()
            .cells
            .iter()
            .map(|cell| cell.id.clone())
            .collect()
    });
    assert_eq!(ids.len(), 3, "两个副本都进列表");
    assert_eq!(ids[0], first);
    assert_eq!(
        ids[1],
        format!("{first}-copy-2"),
        "第二个副本紧跟原单元，id 必须再退一档（{ids:?}）"
    );
    assert_eq!(ids[2], format!("{first}-copy"), "第一个副本被挤到后面（{ids:?}）");

    // 造一条执行错误位点（仿真「本单元跑失败、正文有波浪线」）
    let mark = crate::view::cell_diagnostics::ExecMark {
        site: crate::diagnostics::ErrorSite {
            offset: 0,
            len: 0,
            line: 1,
            column: 1,
            token: None,
        },
        reason: "syntax error near `wheree`".to_string(),
    };
    cx.update(|_window, cx| {
        panel.update(cx, |panel, _cx| {
            panel.cell_error_marks.insert(first.clone(), mark.clone());
        });
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).cell_error_marks.len()),
        1,
        "错误位点先落进去"
    );

    // 转类型（SQL → Markdown）：输出已清空、没再跑过，旧错误线不能留
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_convert_cell(&id, &first, window, cx);
        });
    });
    assert!(
        !cx.update(|_window, cx| panel
            .read(cx)
            .cell_error_marks
            .contains_key(&first)),
        "转类型后旧错误线不能留（否则转回 SQL 会复活）"
    );

    // 再插一条 → 删除该单元 → 同样不能留
    cx.update(|_window, cx| {
        panel.update(cx, |panel, _cx| {
            panel.cell_error_marks.insert(first.clone(), mark);
        });
    });
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_delete_cell(&id, &first, window, cx);
        });
    });
    assert!(
        !cx.update(|_window, cx| panel
            .read(cx)
            .cell_error_marks
            .contains_key(&first)),
        "删除单元后它的错误线不能留"
    );
}

/// 运行 SQL 单元：走 `run_cell` 假执行器 → 回执经泵落到单元输出（内联 Table 投影）
#[gpui_kit::test]
fn running_a_sql_cell_applies_the_inline_output(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(NotebookRunner { seen: seen.clone() }));
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
    assert_eq!(
        cells[1].state,
        crate::cell::CellState::Idle,
        "文本单元不可执行"
    );
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
    let doc_content = shared
        .service()
        .find(&id)
        .map(|doc| doc.content().to_string());
    assert!(
        doc_content
            .as_deref()
            .unwrap_or("")
            .contains("\"version\": 1"),
        "服务层要留着 JSON 正文"
    );

    // 笔记本持有器：单元按文件回来（输出 / 状态不跟随）
    let state = cx.update(|_window, cx| panel.read(cx).notebook_state());
    assert_eq!(state.cells.len(), 2);
    assert_eq!(state.cells[0].id, "c1");
    assert_eq!(state.cells[0].language, crate::cell::CellLanguage::Sql);
    assert_eq!(state.cells[0].body, "select 1;");
    assert_eq!(state.cells[1].body, "# 备注");
    assert!(
        state.cells.iter().all(|cell| cell.outputs.is_empty()),
        "输出不跟随文件"
    );

    // 层真渲染：两个单元卡片都画出来了（▶ 按钮按单元 id 存在）
    assert!(
        cx.debug_bounds("editor-nb-run-c1").is_some()
            || cx.debug_bounds("editor-nb-menu-c1").is_some(),
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
    // 0.9 起手动 `draw` 只重放保留帧：窗口不脏时面板不重建（输出块登记不进观测表）。
    // 先 `refresh` 把窗口置脏再出帧（等价于 `TestWindowExt::render_frame`）。
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });

    // 四条输出块按序渲染（try_find：不在树里返回 None，隐藏/被裁也算 in-tree）
    for ix in 0..4 {
        assert!(
            cx.update(|window, _| window.try_find(format!("editor-nb-output-{ix}")))
                .is_some(),
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
                Some(crate::cell::ExecutionSource::Connection(
                    "P_orders".to_string(),
                )),
                cx,
            );
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(
        cells[0].execution_source,
        Some(crate::cell::ExecutionSource::Connection(
            "P_orders".to_string()
        ))
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
    assert_eq!(
        shared.cell_connection_for(&id, &request).as_deref(),
        Some("P_A")
    );

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
    assert_eq!(
        shared.cell_connection_for(&id, &request).as_deref(),
        Some("P_B")
    );
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
    let msg_id = shared
        .kernel_execute("python3", "1 + 1")
        .expect("提交执行");

    // 轮询事件直到 ExecuteReply（真时钟；内核回执通常 <2s）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut outputs: Vec<crate::cell::Output> = Vec::new();
    let mut status: Option<String> = None;
    loop {
        for envelope in shared.kernel_drain("python3") {
            // 【R8】带归属的事件必须属于本次 execute_request
            if let Some(id) = envelope.msg_id.as_ref() {
                assert_eq!(id, &msg_id, "事件归属错误：应属于本次执行");
            }
            match envelope.event {
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
                // `1 + 1` 不请求输入：真收到说明内核行为异常，但这条用例只看输出与回执
                rds_kernel::KernelEvent::InputRequest { .. } => {}
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
        outputs
            .iter()
            .any(|o| matches!(o, crate::cell::Output::Text(t) if t.trim() == "2")),
        "text/plain 结果要分发成文本输出：{outputs:?}"
    );
    shared.kernel_shutdown("python3");
}

/// 等泵把等待态送进面板（30s 上限）：`(提示串, 是否密码, 单元 id, 内核 id)`
fn wait_pending_input(
    cx: &mut gpui_kit::VisualTestContext,
    panel: &gpui_kit::Entity<crate::view::host::EditorHostPanel>,
) -> Option<(String, bool, String, String)> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        let pending = cx.update(|_window, cx| {
            panel.read(cx).nb_pending.as_ref().map(|request| {
                (
                    request.prompt.clone(),
                    request.password,
                    request.cell_id.clone(),
                    request.kernel_id.clone(),
                )
            })
        });
        if pending.is_some() {
            return pending;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "30s 内没等到输入请求：泵没把 `InputRequest` 送进面板（消息栏：{:?}）",
            cx.update(|_window, cx| panel.read(cx).message.clone())
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 注册表快照：只一个 ready 的 Python 内核（命令 = `RDS_KERNEL_PYTHON` 指向的解释器）
struct OnePython(rds_kernel::KernelSpec);

impl crate::kernel::KernelsPort for OnePython {
    fn list(&self) -> Vec<rds_kernel::KernelSpec> {
        vec![self.0.clone()]
    }
}

/// 真机用例共用的 Python 内核描述（`RDS_KERNEL_PYTHON` 缺省时退回 `python`）
fn python_spec() -> rds_kernel::KernelSpec {
    let python = std::env::var_os("RDS_KERNEL_PYTHON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("python"));
    let command = python.to_string_lossy().to_string();
    rds_kernel::KernelSpec {
        id: "python3".to_string(),
        display_name: "Python 3".to_string(),
        language: rds_kernel::KernelLanguage::Python,
        command: command.clone(),
        ready: true,
        note: None,
        launch: rds_kernel::KernelLaunch::ipykernel(&command),
    }
}

/// 等单元收口（30s 上限），返回最终输出
fn wait_cell_settled(
    cx: &mut gpui_kit::VisualTestContext,
    shared: &EditorShared,
    id: &crate::model::DocumentId,
) -> Vec<crate::cell::Output> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        let cells = shared.notebook_cells(id);
        if cells[0].state == crate::cell::CellState::Done {
            return cells[0].outputs.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "30s 没收口（状态 {:?}，输出 {:?}）",
            cells[0].state,
            cells[0].outputs
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 【R3】输入整链（panel 层）：真内核 → 泵（唤醒）→ `drain_kernel_events` → 等待态 →
/// 回送端口 → 内核继续跑完、单元收口
///
/// 这是此前唯一没有证据的一段：`InputRequest` 的归属在 kernel 层真机用例里比过
/// `msg_id`，提取逻辑有 `orchestrate::pending_input` 的无 GPUI 单测，但「事件真被泵
/// 送进面板、面板据此置等待态、回送后执行继续」没有任何用例。泵若不起 / 不唤醒 /
/// 不 drain，这条等不到提示串，必红。
///
/// 两相：普通 `input()`（不遮挡，提交后清空）与 `getpass()`（遮挡，`password=true`
/// 已由 kernel 层 `getpass_reports_password_flag` 在真内核上取证）。
#[gpui_kit::test]
#[ignore = "需要本机 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
fn notebook_input_round_trips_through_the_pump(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // 真内核在自己的线程里唤醒泵（`Notify::notify_one`）——测试调度器默认把「非测试线程
    // 上发生调度」判成非确定性并 panic（`gpui-pre-scheduler::TestScheduler::assert_correct_thread`）。
    // `allow_parking()` 会置位 `parking_allowed_once`，该检查随即放行，`run_until_parked`
    // 也不再因「有未完成任务」panic。这条用例验证的正是跨线程 push，故必须开。
    cx.executor().allow_parking();

    let spec = python_spec();

    let shared = EditorShared::new();
    shared.attach_kernels(std::rc::Rc::new(OnePython(spec)));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 一个 Python 单元：先请求输入，再把拿到的值打出来
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx);
        });
    });
    let mut cells = shared.notebook_cells(&id);
    let cell_id = cells[0].id.clone();
    cells[0].body = "x = input('name? ')\nprint('got=' + x)".to_string();
    shared.set_notebook_cells(&id, cells);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx));
        window.draw(cx).clear(cx);
    });

    // 输入框当前遮挡态（`InputState::is_masked` 在 `presentation()` 上）
    let masked = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_window, cx| {
            panel.read(cx).nb_input.read(cx).presentation().is_masked()
        })
    };
    // 把值敲进输入框再从框里读出来提交——与「发送」按钮 / 回车同一条路径
    let send = |cx: &mut gpui_kit::VisualTestContext, value: &str| {
        cx.update(|window, cx| {
            let input = panel.read(cx).nb_input.clone();
            input.update(cx, |state, cx| {
                state.set_value(value.to_string(), window, cx);
            });
        });
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                let value = panel.nb_input.read(cx).value().to_string();
                panel.submit_notebook_input(value, cx);
            });
        });
    };

    // ---------- 相一：普通 `input()` ----------

    // 跑这个单元：controller 启动序列 → 提交拿 msg_id → 起泵
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &cell_id, cx));
    });
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert_eq!(
        shared.notebook_cells(&id)[0].state,
        crate::cell::CellState::Running,
        "提交成功要标 Running（消息栏：{message:?}）"
    );

    let (prompt, password, on_cell, on_kernel) =
        wait_pending_input(cx, &panel).expect("等到了输入请求");
    assert_eq!(prompt, "name? ", "提示串要原样（前端照此画输入行）");
    assert!(!password, "`input()` 不是密码输入");
    assert_eq!(on_cell, cell_id, "等待态要挂在正在跑的那个单元上");
    assert_eq!(on_kernel, "python3", "回送要打到本次执行的内核");
    assert!(!masked(cx), "普通请求不遮挡");

    send(cx, "doubao");
    let outputs = wait_cell_settled(cx, &shared, &id);
    let printed = outputs
        .iter()
        .any(|o| matches!(o, crate::cell::Output::Text(t) if t.contains("got=doubao")));
    assert!(printed, "回送的值要真的进了内核：{outputs:?}");

    // 收口即撤等待态（否则输入行会留在已完成的卡片上）
    let still_pending = cx.update(|_window, cx| panel.read(cx).nb_pending.is_some());
    assert!(!still_pending, "收口后等待态要撤掉");

    // 提交成功后清空：`set_value` 走 `cx.defer` + 窗口，这里回读状态（外观另说）
    cx.run_until_parked();
    let leftover = cx.update(|_window, cx| panel.read(cx).nb_input.read(cx).value().to_string());
    assert!(leftover.is_empty(), "提交后输入框要清空（残留：{leftover:?}）");

    // ---------- 相二：`getpass()`（同一内核、同一单元，换正文再跑） ----------

    let mut cells = shared.notebook_cells(&id);
    cells[0].body = "import getpass\nprint('pw=' + getpass.getpass('pw? '))".to_string();
    shared.set_notebook_cells(&id, cells);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &cell_id, cx));
    });
    let (prompt, password, _, _) =
        wait_pending_input(cx, &panel).expect("`getpass()` 也要出等待态");
    assert!(prompt.contains("pw?"), "提示串要原样：{prompt:?}");
    assert!(password, "`getpass()` 必须是密码请求");
    assert!(masked(cx), "密码请求要遮挡回显");

    send(cx, "doubao");
    let outputs = wait_cell_settled(cx, &shared, &id);
    let printed = outputs
        .iter()
        .any(|o| matches!(o, crate::cell::Output::Text(t) if t.contains("pw=doubao")));
    assert!(printed, "`getpass()` 的值也要真的进内核：{outputs:?}");
    assert!(!masked(cx), "收口后不得继续遮挡（否则下一条普通请求会延续遮挡）");

    shared.kernel_shutdown("python3");
}

/// 【R2】首屏延迟：热内核上「提交 → 首个输出落进单元」必须远早于 2s 兜底
///
/// 泵的循环是 `drain → select!(notified(), timer(2s))`（`ensure_kernel_pump`）。
/// `timer(2s)` 只是「唤醒线断了」时的逃生阀：正常情况下醒来的唯一原因是内核线程
/// `Notify::notify_one`。所以这条断言是可判别的——真走兜底节拍必然 ≥ 2s，
/// 唤醒线接上了则是几十毫秒级。若哪天 waker 没注册 / `kernel_wake` 取不到 / notify 丢，
/// 本用例会红，而两相用例（30s 上限）照样绿：它只证明「最终能到」，不证明「没在等兜底」。
///
/// 计时只取**热跑**（第二个单元）：冷跑含内核启动（~1s），不属于泵的口径。
#[gpui_kit::test]
#[ignore = "需要本机 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
fn notebook_first_output_beats_the_escape_valve(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // 真内核在自己的线程上唤醒泵（`Notify::notify_one` → waker → `schedule_local`），
    // 测试调度器默认判其非确定性（见两相用例的说明）。
    cx.executor().allow_parking();

    let shared = EditorShared::new();
    shared.attach_kernels(std::rc::Rc::new(OnePython(python_spec())));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx);
        });
    });
    let mut cells = shared.notebook_cells(&id);
    let cell_id = cells[0].id.clone();
    cells[0].body = "print('warm')".to_string();
    shared.set_notebook_cells(&id, cells);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx));
        window.draw(cx).clear(cx);
    });

    // 冷跑：只为把内核拉起来，耗时不计（`mark_running` 会清空输出，热跑从空开始）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &cell_id, cx));
    });
    let outputs = wait_cell_settled(cx, &shared, &id);
    let warm = outputs
        .iter()
        .any(|o| matches!(o, crate::cell::Output::Text(t) if t.contains("warm")));
    assert!(warm, "冷跑要先出结果才谈得上热跑：{outputs:?}");

    // 热跑：换正文，从提交那一刻计时到首个输出出现
    let mut cells = shared.notebook_cells(&id);
    cells[0].body = "print('ping')".to_string();
    shared.set_notebook_cells(&id, cells);

    let started = std::time::Instant::now();
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &cell_id, cx));
    });
    let elapsed = loop {
        cx.run_until_parked();
        if !shared.notebook_cells(&id)[0].outputs.is_empty() {
            break started.elapsed();
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "10s 内没有首个输出（状态 {:?}，消息栏：{:?}）",
            shared.notebook_cells(&id)[0].state,
            cx.update(|_window, cx| panel.read(cx).message.clone())
        );
        // 5ms 轮询：给「首个输出落地」一个独立于泵节拍的观测粒度
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    println!("热跑首屏延迟：{elapsed:?}");

    // 兜底节拍是 2s：只要触发它，这个上限必红。留 1.5s 余量给本机抖动（实测见日志）。
    assert!(
        elapsed < std::time::Duration::from_millis(1500),
        "首个输出等了 {elapsed:?}，落在 2s 兜底节拍的量级——唤醒线大概率断了"
    );

    // 收口也走唤醒（不是兜底）：整段热跑同样应远小于 2s
    let outputs = wait_cell_settled(cx, &shared, &id);
    let ping = outputs
        .iter()
        .any(|o| matches!(o, crate::cell::Output::Text(t) if t.contains("ping")));
    assert!(ping, "热跑输出要落进单元：{outputs:?}");

    shared.kernel_shutdown("python3");
}

/// 【R3】删掉正在等输入的单元 → 内核不再被那一问卡住，且能继续执行
///
/// 轮次 16 的单元用例只钉了面板侧（等待态撤掉、行撤掉）。这一步补的是另一半：阻塞真的
/// 被解开了。
///
/// 平台分叉（都有证据）：ipykernel 在 Windows 上不发 SIGINT
/// （`kernelbase.py` 的 `_send_interrupt_children`：`if os.name == "nt"` 直接返回），
/// 所以面板在 Windows 上用**关闭内核**兜底（`request_interrupt` 的 windows 分支），
/// 下次运行按需重启；其它平台发 `InterruptRequest`（kernel 侧
/// `interrupt_unblocks_a_waiting_input` 已在真内核上取证）。
///
/// 判据与平台无关：删掉之后**新提交一次执行必须能跑完**。中断 / 关闭若没真正生效，内核
/// 仍停在 `input()` 上，新执行排在它后面永远不会出结果（`wait_cell_settled` 上限 30s 会红）。
#[gpui_kit::test]
#[ignore = "需要本机 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
fn notebook_deleting_a_waiting_cell_interrupts_the_kernel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.executor().allow_parking();

    let shared = EditorShared::new();
    shared.attach_kernels(std::rc::Rc::new(OnePython(python_spec())));
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx);
        });
    });
    let mut cells = shared.notebook_cells(&id);
    let cell_id = cells[0].id.clone();
    cells[0].body = "input('name? ')".to_string();
    shared.set_notebook_cells(&id, cells);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx));
        window.draw(cx).clear(cx);
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &cell_id, cx));
    });
    let (prompt, _, on_cell, _) = wait_pending_input(cx, &panel).expect("等到了输入请求");
    assert_eq!(prompt, "name? ");
    assert_eq!(on_cell, cell_id);
    assert_eq!(
        shared.notebook_cells(&id)[0].state,
        crate::cell::CellState::Running,
        "前提：单元在内核上阻塞着"
    );

    // 删单元：面板侧要立刻撤等待态、并在消息栏说明原因
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_delete_cell(&id, &cell_id, window, cx);
        });
    });
    assert!(
        cx.update(|_window, cx| panel.read(cx).nb_pending.is_none()),
        "删除后等待态必须撤掉"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .unwrap_or_default();
    assert!(
        message.contains("已中断"),
        "消息栏要如实说明中断原因：{message:?}"
    );

    // 1) 阻嗇被解开：内核若仍卡在 `input()`，下面这个新执行只会排在它后面永远不出结果。
    // 注意不能直接 `shared.kernel_drain`：面板的泵（`ensure_kernel_pump`）会先取走事件，
    // 所以判据只能落在「内核是否重新可执行」这个可观测结果上。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx);
        });
    });
    let new_cell = shared.notebook_cells(&id)[0].id.clone();
    let mut cells = shared.notebook_cells(&id);
    cells[0].body = "1 + 1".to_string();
    shared.set_notebook_cells(&id, cells);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx));
        window.draw(cx).clear(cx);
    });
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, &new_cell, cx));
    });
    assert_eq!(
        shared.notebook_cells(&id)[0].state,
        crate::cell::CellState::Running,
        "删除那个单元后要能提交新执行（消息栏：{:?}）",
        cx.update(|_window, cx| panel.read(cx).message.clone())
    );
    // `wait_cell_settled` 的上限是 30s：中断没抵达时内核永远停在 `input()`，这里必红
    let outputs = wait_cell_settled(cx, &shared, &id);
    assert!(
        outputs
            .iter()
            .any(|o| matches!(o, crate::cell::Output::Text(t) if t.trim() == "2")),
        "中断后内核必须还能跑出新结果（不是通道已废）：{outputs:?}，消息栏：{:?}",
        cx.update(|_window, cx| panel.read(cx).message.clone())
    );

    shared.kernel_shutdown("python3");
}
