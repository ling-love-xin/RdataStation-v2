//! LSP 宿主端口与装配判定：文档 ↔ 语言服务 的注入点
//!
//! ## 一句话
//!
//! gpui-kit 编辑器给的是 LSP provider 接口 + `lsp-types` 数据模型；`client` 补了数据源；
//! [`bridge`] 把请求转发给语言服务。**本模块回答「哪个文档配哪个语言服务」**——
//! 这是宿主（workbench / 未来插件宿主）的事，editor 侧只声明端口契约，不写探测。
//!
//! ## 端口口径（与全仓 `CompletionPort` / `EnvPort` 同款）
//!
//! 宿主把「文档 → 语言服务」与「事件源」注进 [`LspPort`]；**未注入 = 无语言服务**（如实，
//! 不假装探测过）。editor 侧只管协议（`didOpen` / `didChange` / 事件折叠），服务从哪来、
//! 文档该不该配服务，都由宿主按插件声明决定。
//!
//! ## 「想要」与「就绪」是两件事（`wants` / `service`）
//!
//! `service()` 只在服务**就绪**时给会话；`wants()` 不管服务启没启都回「这个文档本该配服务」。
//! 面板用它保持轮询，服务被停 / 崩溃重启后能自动重连（见 `view::host::sync_lsp_session`）。
//!
//! ## 事件路由（`drain_for`）
//!
//! 一个 LSP server 常驻、可同时服务多份文档；诊断事件带 `uri`。多个面板共用一个端口时，
//! 由端口按文档归属**认领**（`drain_for`）——面板只拿自己那份，不抢别人的，也不丢别的文档的
//! 事件（其余暂存，等对应面板来取）。

use std::rc::Rc;

use lsp_types::Diagnostic;

use crate::model::{DocumentId, EditorMode};

use super::client::LspEvent;
use super::LspHandle;

/// 一个文档的语言服务会话（didOpen 的文档身份；client 是宿主共享的连接）
///
/// 可克隆（client 是 `Arc`）：面板存一份，provider 造一份，互不干扰。
#[derive(Clone)]
pub struct LspSession {
    /// 到语言服务的连接（与其它文档共享同一个 server）
    pub client: LspHandle,
    /// 本文档的 LSP URI（宿主按文档路径 / 未命名规则生成）
    pub uri: String,
    /// LSP 语言 id（`didOpen` 用；宿主按语言服务声明给）
    pub language_id: String,
}

/// LSP 宿主端口：宿主注入「文档 → 语言服务」与事件源；未注入 = 无语言服务（如实）
pub trait LspPort: 'static {
    /// 文档当前可用的语言服务；`None` = 没有（宿主决定：模式 / 插件声明 / 探测结果）
    fn service(&self, document: &DocumentId) -> Option<LspSession>;

    /// 取走**属于本文档**的待处理事件（宿主轮询调用；端口按 `uri` 认领，别的文档的暂存）
    fn drain_for(&self, document: &DocumentId) -> Vec<LspEvent>;

    /// 本文档**是否想要**语言服务（宿主按插件声明 / 模式 / 扩展名判定）
    ///
    /// 与 [`Self::service`] 的分工：`service()` 只在服务**就绪**时给会话；`wants()` 不管服务
    /// 启没启都如实回「这个文档本该配服务」。面板据此在「服务未起 / 被停 / 崩溃」时继续轻
    /// 轮询，服务一就绪就自动接上（**重启后自动重连**靠这条）。默认 `false`（未接端口 = 不要服务）。
    fn wants(&self, document: &DocumentId) -> bool {
        let _ = document;
        false
    }
}

/// 端口句柄（主线程共享；与 `CompletionHandle` / `EnvHandle` 同款）
pub type LspPortHandle = Rc<dyn LspPort>;

/// 按模式与可用性决定挂哪个补全 provider（**唯一开关处的纯逻辑**，可单测）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionChoice {
    /// 语言服务补全（通用 / 分析模式 + 服务可用）
    Lsp,
    /// 本地 SQL 目录补全（SQL 模式）
    Sql,
    /// 不挂（文本模式无服务 / 只读 / 大文件档位）
    None,
}

/// 纯逻辑（可测）：三选一
///
/// - SQL 模式 → 本地 SQL 目录补全（`sql_enabled` 已是「能力表 + 只读 + 档位」合判）；
/// - 非 SQL 模式 → 语言服务补全（`lsp_available` 由调用方算好：服务健康 + 可编辑 + 档位）；
/// - 都没有 → 不挂。
pub fn choose_completion(mode: EditorMode, lsp_available: bool, sql_enabled: bool) -> CompletionChoice {
    match mode {
        EditorMode::Sql => {
            if sql_enabled {
                CompletionChoice::Sql
            } else {
                CompletionChoice::None
            }
        }
        _ => {
            if lsp_available {
                CompletionChoice::Lsp
            } else {
                CompletionChoice::None
            }
        }
    }
}

/// 一批 LSP 事件折叠的结果（面板按它落面板状态）
#[derive(Debug, Clone, Default)]
pub struct LspEventFold {
    /// 本文档的最新诊断（`Some` = 收到过 publishDiagnostics；空列表 = 服务端清空；
    /// `None` = 这批没有）
    pub diagnostics: Option<Vec<Diagnostic>>,
    /// 服务是否健康（`false` = 收到过 IoError / 对端 EOF）
    pub alive: bool,
    /// IoError 文案（有才报）
    pub io_error: Option<String>,
}

/// 纯逻辑（可测）：把一批事件按本文档 `uri` 认领并折叠
///
/// - `publishDiagnostics`：只认 `params.uri` 与本文档一致的那条；同批多条 = 后者覆盖
///   （LSP 语义：每次发布替换上一次）；解析失败保留旧诊断，不崩。
/// - `IoError`：置不可用 + 记文案（调用方据它摘 provider、清标记、停轮询）。
/// - 其它通知：忽略。
pub fn fold_events(uri: &str, events: Vec<LspEvent>) -> LspEventFold {
    let mut fold = LspEventFold {
        diagnostics: None,
        alive: true,
        io_error: None,
    };
    for event in events {
        match event {
            LspEvent::Diagnostics(params) => {
                let same_doc = params.get("uri").and_then(|u| u.as_str()) == Some(uri);
                if !same_doc {
                    continue;
                }
                if let Ok(list) = super::bridge::diagnostics_from_params(&params) {
                    fold.diagnostics = Some(list);
                }
            }
            LspEvent::IoError(reason) => {
                fold.alive = false;
                fold.io_error = Some(reason);
            }
            LspEvent::Notification(_) => {}
        }
    }
    fold
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::cell::RefCell;
    use std::io;

    use crate::lsp::client::LspClient;

    use super::{LspEvent, LspPort, LspSession, choose_completion, fold_events};
    use crate::lsp::{LspReader, LspWriter};
    use crate::model::{DocumentId, EditorMode};

    /// 对端不可达的读侧（构造 LspClient 用；测试只验端口契约，不跑协议）
    struct NullReader;

    impl LspReader for NullReader {
        fn read_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    /// 丢弃写侧的写（同上）
    struct NullWriter;

    impl LspWriter for NullWriter {
        fn write_frame(&mut self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }

    /// 假端口：预置会话 + 待认领事件
    struct FakePort {
        session: Option<LspSession>,
        events: RefCell<Vec<LspEvent>>,
    }

    impl LspPort for FakePort {
        fn service(&self, _document: &DocumentId) -> Option<LspSession> {
            self.session.clone()
        }

        fn drain_for(&self, _document: &DocumentId) -> Vec<LspEvent> {
            std::mem::take(&mut *self.events.borrow_mut())
        }
    }

    #[test]
    fn choose_completion_prefers_local_sql_in_sql_mode() {
        // SQL 模式：有 LSP 也不挂（SQL 走本地目录补全，D11 已拍板不引 SQL 专用 LSP）
        assert_eq!(
            choose_completion(EditorMode::Sql, true, true),
            super::CompletionChoice::Sql
        );
        // SQL 模式能力关（只读 / 大文件档位）→ 不挂
        assert_eq!(
            choose_completion(EditorMode::Sql, true, false),
            super::CompletionChoice::None
        );
    }

    #[test]
    fn choose_completion_uses_lsp_outside_sql_mode() {
        // 通用 / 分析模式：服务可用 → LSP；不可用 → 不挂（不假装）
        assert_eq!(
            choose_completion(EditorMode::General, true, false),
            super::CompletionChoice::Lsp
        );
        assert_eq!(
            choose_completion(EditorMode::Analysis, true, false),
            super::CompletionChoice::Lsp
        );
        assert_eq!(
            choose_completion(EditorMode::General, false, false),
            super::CompletionChoice::None
        );
        assert_eq!(
            choose_completion(EditorMode::Analysis, false, false),
            super::CompletionChoice::None
        );
    }

    #[test]
    fn port_contract_serves_session_and_drains_per_document() {
        let (client, _events) =
            LspClient::new(Box::new(NullReader), Box::new(NullWriter));
        let session = LspSession {
            client,
            uri: "file:///a.py".to_string(),
            language_id: "python".to_string(),
        };
        let port = FakePort {
            session: Some(session.clone()),
            events: RefCell::new(Vec::new()),
        };
        let doc = DocumentId::new("doc-a");

        // 会话可克隆、可取回
        assert_eq!(port.service(&doc).unwrap().uri, "file:///a.py");
        assert_eq!(port.service(&doc).unwrap().language_id, "python");
        // 没有事件 → 空
        assert!(port.drain_for(&doc).is_empty());
    }

    #[test]
    fn fold_events_claims_only_own_document_diagnostics() {
        let uri = "file:///a.py";
        let events = vec![
            LspEvent::Diagnostics(serde_json::json!({
                "uri": "file:///other.py",
                "diagnostics": [
                    { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }, "message": "别的文档的" }
                ]
            })),
            LspEvent::Diagnostics(serde_json::json!({
                "uri": uri,
                "diagnostics": [
                    { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }, "message": "我的" },
                    { "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 6 } }, "message": "我的二" }
                ]
            })),
        ];
        let fold = fold_events(uri, events);
        assert!(fold.alive);
        assert!(fold.io_error.is_none());
        let list = fold.diagnostics.expect("应拿到本文档诊断");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].message, "我的");
    }

    #[test]
    fn fold_events_later_diagnostics_win_and_io_error_marks_dead() {
        let uri = "file:///a.py";
        let events = vec![
            LspEvent::Diagnostics(serde_json::json!({
                "uri": uri,
                "diagnostics": [
                    { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }, "message": "旧" }
                ]
            })),
            // 同文档再推一次 = 替换（LSP 语义）
            LspEvent::Diagnostics(serde_json::json!({
                "uri": uri,
                "diagnostics": [
                    { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }, "message": "新" }
                ]
            })),
            LspEvent::IoError("对端已关".to_string()),
        ];
        let fold = fold_events(uri, events);
        assert!(!fold.alive);
        assert_eq!(fold.io_error.as_deref(), Some("对端已关"));
        // 先应用了新诊断、再收到死讯：调用方按 io_error 优先清标记（这里 fold 如实两者都给）
        assert_eq!(fold.diagnostics.as_ref().unwrap()[0].message, "新");
    }

    #[test]
    fn fold_events_cleared_diagnostics_is_empty_list() {
        let uri = "file:///a.py";
        let events = vec![LspEvent::Diagnostics(serde_json::json!({
            "uri": uri,
            "diagnostics": []
        }))];
        let fold = fold_events(uri, events);
        assert!(fold.alive);
        assert_eq!(fold.diagnostics, Some(Vec::new()));
    }
}
