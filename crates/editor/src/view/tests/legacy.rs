//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use gpui_kit::TestAppContext;
use gpui_kit::test::TestWindowExt as _;

use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::{open_panel, shared_with_document};

// ===== 【遗留 #10】单元内联编辑 =====

/// 一份带两个单元的 `.rdsnote`（SQL + 文本）
fn analysis_note_two_cells() -> &'static str {
    r###"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
        {"id":"c1","kind":"sql","body":"select 1;"},
        {"id":"c2","kind":"text","body":"## 备注"}]}"###
}

/// 打开分析文档 → 面板为每个单元建内核编辑器实例（编辑能力随文档就位；
/// 渲染据此走内核，不再是纯文本）
#[gpui_kit::test]
fn opening_an_analysis_document_builds_a_cell_editor_per_cell(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\a.rdsnote",
            analysis_note_two_cells(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    // 单元编辑器在事件路径建（生产=on_added_to；测试窗口没挂 Dock，手动走同一入口）
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let mut ids = cx.update(|_window, cx| {
        panel
            .read(cx)
            .cell_editors
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    });
    ids.sort();
    assert_eq!(
        ids,
        vec!["c1".to_string(), "c2".to_string()],
        "每个单元都要有内核编辑器实例（键 = 稳定 cell_id；HashMap 迭代顺序不保证，先排序再比）"
    );

    // 渲染层：单元源以内核呈现（带业务键 id 的包装元素在树里，可被 headless 观察）
    let rendered = cx.update(|window, _cx| window.try_find("editor-nb-cell-source-c1").is_some());
    assert!(rendered, "单元源在渲染树里是内核编辑器（不是纯文本回退）");
}

/// 【15.1 第 4 条】分析模式的逐单元补全：能力表的 `completion` 位要与**单元自己的内核**对上
///
/// SQL 单元必须真的装上 SQL 目录补全 provider（不然能力位就是空头支票：主编辑器的
/// provider 在笔记模式下没人调用）；文本单元不装（它不吃 SQL 候选）。
#[gpui_kit::test]
fn sql_cells_of_a_note_get_the_completion_provider(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\d.rdsnote",
            analysis_note_two_cells(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));

    let provider_of = |cx: &mut gpui_kit::VisualTestContext, cell_id: &str| {
        cx.update(|_window, cx| {
            let editor = panel
                .read(cx)
                .cell_editors
                .get(cell_id)
                .cloned()
                .expect("单元编辑器实例要存在");
            editor.update(cx, |state, _| state.lsp_mut().completion_provider.is_some())
        })
    };

    assert!(
        provider_of(cx, "c1"),
        "SQL 单元的补全 provider 要装在自己的内核上（能力表 completion=true 的落地点）"
    );
    assert!(
        !provider_of(cx, "c2"),
        "文本单元不吃 SQL 目录补全，装上会给错候选"
    );
}

/// 新增 / 复制 / 删除单元时，编辑器实例与单元序列保持同步（创建 / 移除随生命周期）
#[gpui_kit::test]
fn cell_editors_follow_add_duplicate_and_delete(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\b.rdsnote",
            analysis_note_two_cells(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 新增文本单元 → 编辑器随之建出
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Markdown, window, cx)
        });
    });
    let keys = cx.update(|_window, cx| {
        panel
            .read(cx)
            .cell_editors
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(keys.len() == 3, "新增后三个单元都有编辑器：{keys:?}");

    // 复制 c1 → 副本 c1-copy 有独立编辑器
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_duplicate_cell(&id, "c1", window, cx)
        });
    });
    let keys = cx.update(|_window, cx| {
        panel
            .read(cx)
            .cell_editors
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(
        keys.contains(&"c1-copy".to_string()),
        "副本要有自己的编辑器：{keys:?}"
    );

    // 删除 c1-copy → 编辑器与订阅一并移除
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_delete_cell(&id, "c1-copy", window, cx)
        });
    });
    let keys = cx.update(|_window, cx| {
        panel
            .read(cx)
            .cell_editors
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(
        !keys.contains(&"c1-copy".to_string()),
        "删除后编辑器要移除（订阅句柄一并释放）：{keys:?}"
    );
}

/// 回写回路：在单元的内核编辑器里真打字 → Change 事件 → 面板订阅 → 单元正文落回
/// shared（下次运行 / 落盘读的都是编辑后的正文）
#[gpui_kit::test]
fn typing_in_a_cell_editor_writes_back_to_the_notebook_body(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\c.rdsnote",
            r#"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
                {"id":"c1","kind":"sql","body":"select 1;"}]}"#,
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 聚焦第一个单元的内核编辑器（点带业务键的包装元素；点击即聚焦）
    cx.update(|window, cx| {
        window.click("editor-nb-cell-source-c1", cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 真打一个字符：内核发出 Change → 面板订阅回写 shared 的单元正文
    //（VisualTestContext 的单参版自动打到当前窗口 / 当前聚焦）
    cx.simulate_input("x");

    let body = cx.update(|_window, _cx| shared.notebook_cells(&id)[0].body.clone());
    assert_ne!(body, "select 1;", "击键要写回笔记本正文（当前：{body}）");
    assert!(body.contains('x'), "正文要包含刚打进去的字符：{body}");
}

/// 通用模式打开 `.md` 文档：默认源码编辑器；工具栏有「预览」切换；
/// 切预览 → 渲染视图（TextView::markdown）出现；再切回 → 编辑器恢复。
#[gpui_kit::test]
fn markdown_document_toggles_between_source_and_rendered_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\notes.md", "# 标题\n\n- 列表项\n");
    // .md 走通用模式（与 SQL 文档区分）；`sync_mode` 让面板按记忆模式渲染
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 初始：源码编辑器态（预览根不出现）；工具栏有切换按钮
    assert!(
        cx.debug_bounds("editor-markdown-preview-toggle").is_some(),
        "Markdown 文档工具栏应有预览切换按钮"
    );
    assert!(
        cx.update(|window, _| window.try_find("markdown-preview-root"))
            .is_none(),
        "初始应为源码编辑器态（预览根不出现）"
    );

    // 切到预览：渲染视图出现
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_markdown_preview(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, _| window.try_find("markdown-preview-root"))
            .is_some(),
        "预览态应有渲染视图根"
    );

    // 再切回：源码编辑器恢复
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_markdown_preview(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, _| window.try_find("markdown-preview-root"))
            .is_none(),
        "切回源码态后预览根应消失"
    );
}

/// 非 Markdown 文档（如 `.py`）不出现预览切换按钮（切换只对 Markdown 语义有意义）
#[gpui_kit::test]
fn non_markdown_documents_have_no_preview_toggle(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\py\run.py", "print('hi')");
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-markdown-preview-toggle").is_none(),
        "非 Markdown 文档不出现预览切换"
    );
}

// ===== 【15.14】逐单元诊断 =====

/// 一份带三个单元的 `.rdsnote`：好 SQL / 坏 SQL（未闭合字符串）/ 文本
fn analysis_note_with_bad_sql() -> &'static str {
    r###"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
        {"id":"c1","kind":"sql","body":"select 1;"},
        {"id":"c2","kind":"sql","body":"select 'abc"},
        {"id":"c3","kind":"text","body":"## 备注"}]}"###
}

/// 【15.14】诊断必须挂在**单元自己的内核**上，且逐单元独立重建
///
/// 三件事一起钉：
/// 1. 打开即诊断（单元一出现就带波浪线，不必等敲键）；
/// 2. 单元之间互不影响（坏的那条只脏自己，好 SQL 与文本单元干净）；
/// 3. 正文变化后重算（打一个未闭合引号 → 该单元诊断出现）。
///
/// 第 3 条同时覆盖 `cx.defer` 那条路径：Change 由单元编辑器自己发出，回调里不能重入
/// update 本实体，重建被推迟到本次更新之后——这里靠 `run_until_parked` 把推迟的那步跑掉。
#[gpui_kit::test]
fn cell_diagnostics_are_per_cell_and_rebuild_on_typing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\diag.rdsnote",
            analysis_note_with_bad_sql(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let count_of = |cx: &mut gpui_kit::VisualTestContext, cell_id: &str| {
        let editor = cx.update(|_window, cx| {
            panel
                .read(cx)
                .cell_editors
                .get(cell_id)
                .cloned()
                .expect("单元编辑器实例要存在")
        });
        cx.update(|_window, cx| {
            editor.update(cx, |state, _| {
                state.diagnostics().map(|set| set.len()).unwrap_or(0)
            })
        })
    };

    assert_eq!(count_of(cx, "c1"), 0, "好 SQL 不该有诊断");
    assert!(
        count_of(cx, "c2") >= 1,
        "未闭合字符串的单元一打开就该有诊断（首帧重建）"
    );
    assert_eq!(count_of(cx, "c3"), 0, "文本单元不吃 SQL 诊断");

    // 点进 c1 敲一个未闭合的块注释开头（内核不会为它自动补字符）：词法层报「块注释没有闭合」
    cx.update(|window, cx| window.click("editor-nb-cell-source-c1", cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_input("/");
    cx.simulate_input("*");
    cx.run_until_parked();

    let body = cx.update(|_window, _cx| shared.notebook_cells(&id)[0].body.clone());
    let count = count_of(cx, "c1");
    assert!(
        count >= 1,
        "正文变化后该单元自己的诊断要重建（正文：{body:?}，诊断数：{count}）"
    );
    assert_eq!(count_of(cx, "c3"), 0, "重建只作用于本单元，别的单元不受牵连");
}

// ===== 【15.15】单元执行错误进单元诊断 =====

/// 假执行器：SQL 里带 `wheree` → 回「带位置的解析错误」（PG 口径 `at position N`）；
/// 否则成功。两种结论都要能造，才能钉住"重跑成功后旧错误必须清掉"。
struct CellErrorRunner;

impl crate::execution::QueryRunner for CellErrorRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: crate::channel::ExecChannel,
        _sql: &str,
        _options: crate::execution::RunOptions,
    ) -> Result<crate::execution::QueryData, String> {
        Ok(crate::execution::QueryData::default())
    }

    fn run_cell(
        &self,
        request: &crate::cell::CellRequest,
    ) -> Result<crate::cell::CellOutcome, String> {
        let sql = request.sql.clone();
        let result = match sql.find("wheree") {
            Some(position) => Err(format!(
                "Query failed at position {position}: syntax error at or near \"wheree\""
            )),
            None => Ok(crate::execution::QueryData {
                columns: vec!["n".to_string()],
                rows: vec![vec!["1".to_string()]],
                ..Default::default()
            }),
        };
        Ok(crate::cell::CellOutcome {
            sql,
            elapsed_ms: 2,
            affected_rows: None,
            truncated: false,
            result,
        })
    }
}

/// 一份两单元的 `.rdsnote`：c1 的 SQL 执行必然失败（词法干净），c2 正常
fn analysis_note_with_a_failing_sql() -> &'static str {
    r###"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
        {"id":"c1","kind":"sql","body":"select * from t wheree 1 = 1;"},
        {"id":"c2","kind":"sql","body":"select 1;"}]}"###
}

/// 【15.15】执行错误落在**该单元自己**的诊断上，重跑成功后清掉
///
/// 三件事：① 跑之前干净（词法层对这段 SQL 无话可说）；② 失败回执到了以后，
/// 该单元出现来源「执行」的诊断，位点来自驱动原文的 `at position N`；
/// ③ 改好 SQL 重跑成功 → 旧执行错误被清掉（与词法诊断同在一条重建流程里）。
#[gpui_kit::test]
fn a_failing_cell_run_marks_its_own_cell_and_a_successful_rerun_clears_it(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(CellErrorRunner));
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\exec_diag.rdsnote",
            analysis_note_with_a_failing_sql(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 诊断来源列表（`Some("执行")` 就是执行错误那份，便于与词法层区分）
    let sources_of = |cx: &mut gpui_kit::VisualTestContext, cell_id: &str| {
        let editor = cx.update(|_window, cx| {
            panel
                .read(cx)
                .cell_editors
                .get(cell_id)
                .cloned()
                .expect("单元编辑器实例要存在")
        });
        cx.update(|_window, cx| {
            editor.update(cx, |state, _| {
                state
                    .diagnostics()
                    .map(|set| {
                        set.range(0..usize::MAX)
                            .map(|entry| entry.diagnostic.source.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        })
    };

    // ① 跑之前：词法干净，没有执行错误
    assert!(
        sources_of(cx, "c1").is_empty(),
        "还没跑过的单元不该有诊断（词法层对这段 SQL 无话可说）"
    );
    assert_eq!(sources_of(cx, "c2"), vec![], "另一个单元干净");

    // ② 跑一次（必然失败）：等到回执落地，再断言该单元出现「执行」诊断
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, "c1", cx));
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let finished = loop {
        let finished = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_cell_notes(cx);
                let cell = panel
                    .notebook_state()
                    .cells
                    .into_iter()
                    .find(|cell| cell.id == "c1")
                    .expect("c1 还在");
                cell.state == crate::cell::CellState::Done
                    || cell.state == crate::cell::CellState::Error
            })
        });
        if finished || std::time::Instant::now() > deadline {
            break finished;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(finished, "回执要落到单元上（Done / Error）");
    let sources = sources_of(cx, "c1");
    assert!(
        sources.iter().any(|source| source.as_ref().is_some_and(|s| s.as_str() == "执行")),
        "执行失败的位点要进该单元自己的诊断（实际来源：{sources:?}）"
    );
    assert_eq!(sources_of(cx, "c2"), vec![], "别的单元不被牵连");

    // ③ 改好 SQL 重跑（成功）→ 旧执行错误清掉
    let mut state = cx.update(|_window, _cx| shared.notebook_state(&id));
    state.cells[0].body = "select * from t where 1 = 1;".to_string();
    let cells = state.cells;
    cx.update(|_window, _cx| shared.set_notebook_cells(&id, cells));
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.notebook_run_cell(&id, "c1", cx));
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let finished = loop {
        let finished = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_cell_notes(cx);
                panel
                    .notebook_state()
                    .cells
                    .into_iter()
                    .find(|cell| cell.id == "c1")
                    .is_some_and(|cell| cell.state == crate::cell::CellState::Done)
            })
        });
        if finished || std::time::Instant::now() > deadline {
            break finished;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(finished, "重跑要成功收口（Done）");
    assert_eq!(
        sources_of(cx, "c1"),
        vec![],
        "重跑成功后上一次的执行错误必须清掉（不留过期波浪线）"
    );
}

// ===== 【§16.1 A2】目录快照变更 → 不必等敲键 =====

/// 一份两单元的 `.rdsnote`：c1 引用表 `t_old`（词法干净），c2 不引用任何表
fn analysis_note_referencing_a_table() -> &'static str {
    r###"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
        {"id":"c1","kind":"sql","body":"select * from t_old;"},
        {"id":"c2","kind":"sql","body":"select 1;"}]}"###
}

/// 目录快照（只给一张表的目录就够：语义段只关心表/列在不在）
fn catalog_with_one_table(name: &str) -> std::sync::Arc<crate::completion::Catalog> {
    std::sync::Arc::new(crate::completion::Catalog {
        objects: vec![crate::completion::Candidate {
            label: name.to_string(),
            kind: crate::completion::CandidateKind::Table,
            detail: None,
        }],
        columns: vec![(
            name.to_string(),
            "id".to_string(),
            Some("INTEGER".to_string()),
        )],
        truncated: false,
    })
}

/// 【A2】目录快照一变（切连接 / 切 Schema），已有单元的语义诊断跟着刷新
///
/// 区分性：旧实现只在「敲键 / 执行」时重建，这里**一次都不敲**——只换快照 + 渲一帧，
/// 若没有 render 权威同步点那条重建，c1 会一直挂着旧目录的结论（这里会红）。
#[gpui_kit::test]
fn a_new_catalog_snapshot_refreshes_cell_diagnostics_without_typing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\catalog_rev.rdsnote",
            analysis_note_referencing_a_table(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    // 目录里有 t_old：c1 的引用能解析，语义段无话可说
    shared.set_catalog_snapshot(&id, Some(catalog_with_one_table("t_old")));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    // 有 `unknown_table` 这条 code 的诊断数（语义层专属，词法层不产这个 code）
    let unknown_tables_of = |cx: &mut gpui_kit::VisualTestContext, cell_id: &str| {
        let editor = cx.update(|_window, cx| {
            panel
                .read(cx)
                .cell_editors
                .get(cell_id)
                .cloned()
                .expect("单元编辑器实例要存在")
        });
        cx.update(|_window, cx| {
            editor.update(cx, |state, _| {
                state
                    .diagnostics()
                    .map(|set| {
                        set.range(0..usize::MAX)
                            .filter(|entry| {
                                entry
                                    .diagnostic
                                    .code
                                    .as_ref()
                                    .is_some_and(|code| code.as_ref() == "unknown_table")
                            })
                            .count()
                    })
                    .unwrap_or(0)
            })
        })
    };

    assert_eq!(
        unknown_tables_of(cx, "c1"),
        0,
        "目录里有 t_old：不该报未知表"
    );
    assert_eq!(unknown_tables_of(cx, "c2"), 0, "c2 不引用表，本就干净");

    // 换成不含 t_old 的目录（等价于切到了别的连接 / Schema）：不敲键，只让面板重绘一帧
    // （真实入口里「切连接」的那次点击本身就会 `cx.notify()`，这里如实模拟那次重绘）
    shared.set_catalog_snapshot(&id, Some(catalog_with_one_table("t_new")));
    cx.update(|_window, cx| panel.update(cx, |_panel, cx| cx.notify()));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    assert!(
        unknown_tables_of(cx, "c1") >= 1,
        "目录换成不含 t_old 的那一份后，c1 的未知表诊断要在不敲键的情况下出现"
    );
    assert_eq!(
        unknown_tables_of(cx, "c2"),
        0,
        "整批重建只针对引用了表的单元，c2 依旧干净"
    );
}
