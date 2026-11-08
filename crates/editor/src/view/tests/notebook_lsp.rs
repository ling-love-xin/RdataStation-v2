//! 【A1】分析笔记的逐单元语言服务：窗口级用例（2026-11-09）
//!
//! 覆盖 §15.17 的验收口径：Python 单元装补全 provider、SQL / 文本单元不装；服务端
//! `publishDiagnostics` 按单元 uri 进该单元诊断，且正文一变（重建一次）后仍在。
//!
//! 假端口只在**单元级**给会话（与 `WorkbenchLsp::cell_service` 同口径），不碰真实
//! `jedi-language-server`——本用例验的是面板接线，不是协议（协议有 `lsp_probe*.py` 实测）。
//! 注意：不用 `use super::*` 通配（gpui `test` 宏会递归），依赖显式列举。

use std::cell::RefCell;

use gpui_kit::TestAppContext;

use crate::lsp::{
    LspClient, LspEvent, LspHandle, LspPort, LspPortHandle, LspReader, LspSession, LspWriter,
};
use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::open_panel;

/// 对端不可达的读侧（只为能造出 `LspSession`；本用例不发请求、不读帧）
struct NullReader;

impl LspReader for NullReader {
    fn read_frame(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// 丢弃写侧的写（同上）
struct NullWriter;

impl LspWriter for NullWriter {
    fn write_frame(&mut self, _bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }
}

/// 假语言服务端口：**只对 Python 单元**给会话（单元 uri 与 `uri_for_cell` 同形）
struct CellFakeLsp {
    client: LspHandle,
    /// 待认领的事件（按 uri 过滤，别家暂存——与真端口同口径）
    events: RefCell<Vec<LspEvent>>,
}

impl CellFakeLsp {
    fn new() -> Self {
        let (client, _events) = LspClient::new(Box::new(NullReader), Box::new(NullWriter));
        Self {
            client,
            events: RefCell::new(Vec::new()),
        }
    }

    fn push_diagnostics(&self, uri: &str, message: &str) {
        self.events.borrow_mut().push(LspEvent::Diagnostics(
            serde_json::json!({
                "uri": uri,
                "diagnostics": [{
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 5 }
                    },
                    "message": message,
                    "severity": 1
                }]
            }),
        ));
    }
}

impl LspPort for CellFakeLsp {
    // 文档级服务：本用例只关心单元级，如实不给
    fn service(&self, _document: &crate::model::DocumentId) -> Option<LspSession> {
        None
    }

    fn drain_for(&self, _document: &crate::model::DocumentId) -> Vec<LspEvent> {
        Vec::new()
    }

    fn wants(&self, _document: &crate::model::DocumentId) -> bool {
        // 含 Python 单元的分析笔记「想要」服务（真实端口 `needs_service` 同一口径）
        true
    }

    fn cell_service(
        &self,
        document: &crate::model::DocumentId,
        cell_id: &str,
    ) -> Option<LspSession> {
        Some(LspSession {
            client: self.client.clone(),
            uri: format!("untitled:Rds-{}-{cell_id}", document.as_str()),
            language_id: "python".to_string(),
        })
    }

    fn drain_for_uri(&self, uri: &str) -> Vec<LspEvent> {
        let mut mine = Vec::new();
        let mut rest = Vec::new();
        {
            let mut all = self.events.borrow_mut();
            for event in all.drain(..) {
                let is_mine = match &event {
                    LspEvent::Diagnostics(value) => value
                        .get("uri")
                        .and_then(|one| one.as_str())
                        .is_some_and(|one| one == uri),
                    _ => true,
                };
                if is_mine {
                    mine.push(event);
                } else {
                    rest.push(event);
                }
            }
        }
        *self.events.borrow_mut() = rest;
        mine
    }
}

/// 读某单元编辑器上「语言服务」那一源的诊断消息（直接读内核集合，不靠渲染）
fn lsp_messages(
    panel: &gpui_kit::Entity<crate::view::host::EditorHostPanel>,
    cell_id: &str,
    cx: &mut gpui_kit::VisualTestContext,
) -> Vec<String> {
    cx.update(|_window, cx| {
        let panel = panel.read(cx);
        let Some(editor) = panel.cell_editors.get(cell_id).cloned() else {
            return Vec::new();
        };
        let state = editor.read(cx);
        state
            .diagnostics()
            .map(|set| {
                set.iter()
                    .filter(|entry| entry.diagnostic.source.as_deref() == Some("语言服务"))
                    .map(|entry| entry.diagnostic.message.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
}

/// 只有 Python 单元接上语言服务并装 provider；SQL 单元如实没有
#[gpui_kit::test]
fn only_python_cells_get_a_language_service_session(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let port = std::rc::Rc::new(CellFakeLsp::new());
    shared.attach_lsp(port as LspPortHandle);
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 一个 SQL 单元 + 一个 Python 单元
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Sql, window, cx);
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx);
        });
    });
    let cells = cx.update(|_window, cx| panel.read(cx).notebook_state().cells);
    assert_eq!(cells.len(), 2);
    let sql_id = cells[0].id.clone();
    let py_id = cells[1].id.clone();

    let (python, sql) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (
            panel
                .cell_lsp
                .get(&py_id)
                .map(|slot| slot.providers_installed),
            panel.cell_lsp.contains_key(&sql_id),
        )
    });
    assert_eq!(
        python,
        Some(true),
        "Python 单元应接上会话并装补全 / hover provider"
    );
    assert!(!sql, "SQL 单元不接语言服务（走本地目录补全）");
}

/// 服务端诊断进该单元，且正文变化重建一次后仍在（三源合并不许把语言服务那源清掉）
#[gpui_kit::test]
fn cell_lsp_diagnostics_land_in_the_cell_and_survive_a_rebuild(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let port = std::rc::Rc::new(CellFakeLsp::new());
    shared.attach_lsp(port.clone() as LspPortHandle);
    let id = shared
        .open(OpenRequest::untitled("", EditorMode::Analysis))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Python, window, cx);
        });
    });
    let py_id = cx
        .update(|_window, cx| panel.read(cx).notebook_state().cells[0].id.clone());

    // 端推送一条诊断（uri = 该单元），面板每拍收一次
    let uri = format!("untitled:Rds-{}-{py_id}", id.as_str());
    port.push_diagnostics(&uri, "未定义名称 os");
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.drain_cell_lsp(cx));
    });
    assert_eq!(
        lsp_messages(&panel, &py_id, cx),
        vec!["未定义名称 os".to_string()],
        "诊断应进该单元，源为「语言服务」"
    );

    // 「敲键」路径（Change 的 defer 目标）重建一次：语言服务那源不能被语法 / 执行擦掉
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.on_cell_text_changed(&py_id, cx));
    });
    assert_eq!(
        lsp_messages(&panel, &py_id, cx),
        vec!["未定义名称 os".to_string()],
        "重建后语言服务诊断应仍在（三源合并只 clear 一次）"
    );
}
