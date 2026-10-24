//! LSP 补全 / 诊断的 provider bridge：把 [`LspClient`] 接到 gpui-kit 编辑内核的 LSP 槽位上
//!
//! ## 一句话
//!
//! gpui-kit 的 [`CompletionProvider`] 只声明"给什么候选"，不问候选从哪来。本模块实现一个
//! **把请求转发给语言服务**的 provider：`completions()` 在后台线程走 [`LspClient::call`]
//! 发 `textDocument/completion`，把响应解析回 `CompletionResponse`；另提供一个把
//! `textDocument/publishDiagnostics` 通知翻译成 LSP 诊断列表的纯函数，供宿主合并进内核
//! 诊断（内核 `DiagnosticSet::push` 已有 `From<lsp_types::Diagnostic>`，见 vendor
//! `gpui-base` 的 `input/editor/diagnostics.rs`）。
//!
//! ## 与本地补全的关系（谁挂谁，宿主定）
//!
//! 内核的 `Lsp::completion_provider` 是**单槽** `Option`：LSP provider 与本地
//! [`crate::view::completion::SqlCompletionProvider`] **互斥**。宿主按模式决策
//! （SQL 模式走本地目录补全；通用 / 分析模式且语言服务可用时挂本 provider）——本模块
//! 不选边，只提供实现与 `install` / `uninstall` 挂摘点。
//!
//! ## Position 编码（已知限制）
//!
//! LSP 规范默认 `Position.character` 是 **UTF-16 码元**；gpui-kit 的 `RopeExt`
//! 用 **char 数（标量值）** 编码（vendor `gpui-base` 的 `input/base/rope_ext.rs` 的
//! `offset_to_position` / `position_to_offset`），往返一致但对含代理对的非 BMP 字符
//! （emoji 等）与规范有偏差。本 bridge **跟随内核的编码**、不另造一套——否则
//! "发出去的请求位置"与"收进来的诊断位置"会两套语义，比规范偏差更糟。

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui_kit::component::input::{
    CompletionProvider, DefinitionProvider, EditorState, HoverProvider, Rope, RopeExt as _,
};
use gpui_kit::*;
use lsp_types::{CompletionContext, CompletionResponse, Diagnostic, Hover, Position};

use super::client::LspClient;

/// 补全请求的超时：语言服务偶发慢启动 / 大文档全量索引，给足但别让补全长期挂着
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(3);

/// 把请求转发给语言服务的补全 provider（无状态：可随时建、可共享）
pub struct LspCompletionProvider {
    client: Arc<LspClient>,
    document_uri: String,
}

impl LspCompletionProvider {
    pub fn new(client: Arc<LspClient>, document_uri: impl Into<String>) -> Self {
        Self {
            client,
            document_uri: document_uri.into(),
        }
    }
}

impl CompletionProvider for LspCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let position = text.offset_to_position(offset);
        let client = Arc::clone(&self.client);
        let uri = self.document_uri.clone();
        // `LspClient::call` 是同步阻塞：放后台线程跑，别卡 UI
        cx.background_spawn(async move { request_completions(&client, &uri, position) })
    }

    /// 刚敲下一个标识符字符或 `.` 时才问（与本地 SQL 补全同一判据，两 provider 手感一致）
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        is_trigger_char(new_text)
    }
}

/// 纯逻辑（可测）：单字符触发判据（标识符字符或 `.`）
fn is_trigger_char(new_text: &str) -> bool {
    let mut chars = new_text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '.',
        _ => false,
    }
}

/// 纯逻辑（可测）：组 `textDocument/completion` 请求参数
fn build_completion_params(uri: &str, position: Position) -> serde_json::Value {
    serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": position.line, "character": position.character },
    })
}

/// 把请求转发给语言服务的 hover provider（无状态：可随时建、可共享）
///
/// hover 弹层由 gpui-base 编辑内核**自带**（鼠标停留 150ms 防抖 → 调 `hover()`
/// → 结果存 `HoverPopoverState` 自动渲染 + 随鼠标离开自动收起），这里只补"数据源"：
/// 与补全同构——后台线程走 `textDocument/hover`，把响应解析回 `Option<Hover>`。
pub struct LspHoverProvider {
    client: Arc<LspClient>,
    document_uri: String,
}

impl LspHoverProvider {
    pub fn new(client: Arc<LspClient>, document_uri: impl Into<String>) -> Self {
        Self {
            client,
            document_uri: document_uri.into(),
        }
    }
}

impl HoverProvider for LspHoverProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<Hover>>> {
        let position = text.offset_to_position(offset);
        let client = Arc::clone(&self.client);
        let uri = self.document_uri.clone();
        // `LspClient::call` 是同步阻塞：放后台线程跑，别卡 UI
        cx.background_spawn(async move { request_hover(&client, &uri, position) })
    }
}

/// hover 请求的超时：与补全同档（语言服务偶发慢启动）
const HOVER_TIMEOUT: Duration = Duration::from_secs(3);

/// 纯逻辑（可测）：组 `textDocument/hover` 请求参数（与补全共用同一位置编码）
fn build_hover_params(uri: &str, position: Position) -> serde_json::Value {
    build_completion_params(uri, position)
}

/// 纯逻辑（可测）：发 hover 请求并解析响应（**阻塞**；生产在后台线程跑）
///
/// 语言服务没有 hover 时回 `result: null`——`from_value(null)` 解出 `None`，
/// 内核收起弹层；有则解出 `Some(Hover)`（`contents` + 可选 `range`）。
fn request_hover(
    client: &LspClient,
    uri: &str,
    position: Position,
) -> Result<Option<Hover>> {
    let result = client.call(
        "textDocument/hover",
        build_hover_params(uri, position),
        HOVER_TIMEOUT,
    )?;
    Ok(serde_json::from_value(result)?)
}

/// 纯逻辑（可测）：发补全请求并解析响应（**阻塞**；生产在后台线程跑）
fn request_completions(
    client: &LspClient,
    uri: &str,
    position: Position,
) -> Result<CompletionResponse> {
    let result = client.call(
        "textDocument/completion",
        build_completion_params(uri, position),
        COMPLETION_TIMEOUT,
    )?;
    Ok(serde_json::from_value(result)?)
}

/// 把 `textDocument/publishDiagnostics` 通知的 params 翻译成 LSP 诊断列表
///
/// 只取 `diagnostics` 数组，不强解析整个 params（避免 `uri` 字段的 `Uri` 解析
/// 把合法通知误判成坏数据）；对端发空数组 = 清空当前标记，返回空列表即可。
pub fn diagnostics_from_params(params: &serde_json::Value) -> Result<Vec<Diagnostic>> {
    let diagnostics = params
        .get("diagnostics")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    Ok(serde_json::from_value(diagnostics)?)
}

/// 装到某个文档的编辑内核上（`EditorState::lsp_mut`）
pub fn install(state: &mut EditorState, provider: Rc<LspCompletionProvider>) {
    state.lsp_mut().completion_provider = Some(provider);
}

/// 摘掉（切回本地补全 / 语言服务不可用时）
pub fn uninstall(state: &mut EditorState) {
    state.lsp_mut().completion_provider = None;
}

/// 【#21】装 hover provider（`EditorState::lsp_mut`；与补全并列的独立槽位，互不影响）
///
/// 泛型：LSP 转发（[`LspHoverProvider`]）与 SQL 自研语义（[`super::sql_hover::SqlHoverProvider`]）
/// 都挂同一个槽位（内核槽位是 `Rc<dyn HoverProvider>`），宿主按模式决策挂谁。
pub fn install_hover<P: HoverProvider + 'static>(state: &mut EditorState, provider: Rc<P>) {
    state.lsp_mut().hover_provider = Some(provider);
}

/// 摘掉 hover（语言服务不可用时）
pub fn uninstall_hover(state: &mut EditorState) {
    state.lsp_mut().hover_provider = None;
}

/// 【#54 C6】装 definition provider（内核槽位 `Option<Rc<dyn DefinitionProvider>>`，
/// 与 hover 并列的独立槽位，互不影响）。泛型：SQL 自研语义（[`super::sql_definition`]）
/// 挂这个槽位；宿主按模式决策。
pub fn install_definition<P: DefinitionProvider + 'static>(state: &mut EditorState, provider: Rc<P>) {
    state.lsp_mut().definition_provider = Some(provider);
}

/// 摘掉 definition（语言服务不可用时）
pub fn uninstall_definition(state: &mut EditorState) {
    state.lsp_mut().definition_provider = None;
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::sync::mpsc::{Receiver, Sender, channel};

    use gpui_kit::component::input::{Rope, RopeExt as _};

    use super::{
        LspClient, build_completion_params, build_hover_params, diagnostics_from_params,
        is_trigger_char, request_completions, request_hover,
    };
    use crate::lsp::{LspReader, LspWriter};

    /// 客户端侧读：从 channel 读（server 写来的帧），**解析头、只回 JSON 正文**
    /// （与 `ProcessIo::read_frame` 同语义：`LspReader` 的产物是正文不是整帧）
    struct MemoryReader {
        read_rx: Receiver<Vec<u8>>,
    }

    impl LspReader for MemoryReader {
        fn read_frame(&mut self) -> std::io::Result<Option<Vec<u8>>> {
            match self.read_rx.recv() {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    match text.split("\r\n\r\n").nth(1) {
                        Some(body) => Ok(Some(body.as_bytes().to_vec())),
                        None => Ok(Some(bytes)),
                    }
                }
                Err(_) => Ok(None),
            }
        }
    }

    /// 客户端侧写：写进 channel（server 读）
    struct MemoryWriter {
        write_tx: Sender<Vec<u8>>,
    }

    impl LspWriter for MemoryWriter {
        fn write_frame(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.write_tx
                .send(bytes.to_vec())
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "对端已关"))
        }
    }

    /// 从帧字节里抽出 JSON 正文（测试辅助：跳过 Content-Length 头）
    fn body_of(bytes: &[u8]) -> serde_json::Value {
        let text = String::from_utf8_lossy(bytes);
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
        serde_json::from_str(body).unwrap()
    }

    /// 从 server 侧通道读下一帧，断言 method；跳过 `initialized` 等握手期通知。
    /// 返回 `(请求 id, 请求正文)`——一帧读一次给全，避免调用方再 `recv` 一次等死。
    fn next_request(recv_rx: &Receiver<Vec<u8>>, want: &str) -> (u64, serde_json::Value) {
        loop {
            let frame = recv_rx.recv().expect("应收到请求");
            let req = body_of(&frame);
            if let Some(method) = req["method"].as_str() {
                if method == "initialized" {
                    continue;
                }
                assert_eq!(method, want, "方法名应匹配");
            }
            return (
                req["id"].as_u64().expect("请求应带 id"),
                req,
            );
        }
    }

    /// 建一对内存通道 + 已握手的客户端：
    /// - `recv_rx`（返回第一项）= 测试收 client 发来的请求帧（writer 接到它的写侧）；
    /// - `reply_tx`（返回第二项）= 测试回帧给 client；
    /// - `client`（返回第三项）= 客户端句柄。
    fn harness() -> (Receiver<Vec<u8>>, Sender<Vec<u8>>, std::sync::Arc<LspClient>) {
        // client → server（写侧）：writer 发，recv_rx 收
        let (client_write, recv_rx) = channel::<Vec<u8>>();
        // server → client（读侧）：reply_tx 发，client_read 收
        let (reply_tx, client_read) = channel::<Vec<u8>>();
        let reader = MemoryReader { read_rx: client_read };
        let writer = MemoryWriter {
            write_tx: client_write,
        };
        let (client, _events) = LspClient::new(Box::new(reader), Box::new(writer));

        // 握手（后台线程发；主线程读请求、回响应）
        let client_for_thread = client.clone();
        let init_handle = std::thread::spawn(move || {
            client_for_thread.initialize(None, std::time::Duration::from_secs(2))
        });
        let id = next_request(&recv_rx, "initialize").0;
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } });
        reply_tx
            .send(crate::lsp::frame::encode(&serde_json::to_vec(&resp).unwrap()))
            .unwrap();
        init_handle.join().expect("握手线程应结束").expect("握手应成功");

        (recv_rx, reply_tx, client)
    }

    #[test]
    fn trigger_char_only_single_identifier_or_dot() {
        assert!(is_trigger_char("a"));
        assert!(is_trigger_char("_"));
        assert!(is_trigger_char("$"));
        assert!(is_trigger_char("."));
        assert!(is_trigger_char("中"));
        // 多字符 / 空白 / 标点不触发（退格 / 空格 / 粘贴由内核直接问但这里拒掉）
        assert!(!is_trigger_char("ab"));
        assert!(!is_trigger_char(" "));
        assert!(!is_trigger_char("("));
        assert!(!is_trigger_char(""));
    }

    #[test]
    fn completion_params_carry_uri_and_position() {
        let position = lsp_types::Position::new(2, 5);
        let params = build_completion_params("file:///a.sql", position);
        assert_eq!(params["textDocument"]["uri"], "file:///a.sql");
        assert_eq!(params["position"]["line"], 2);
        assert_eq!(params["position"]["character"], 5);
    }

    #[test]
    fn offset_to_position_matches_kernel_encoding() {
        // 与内核同一编码（char 数，非 UTF-16）：跟随它，别另造一套
        let rope = Rope::from_str("select *\nfrom orders\nwhere");
        let pos = rope.offset_to_position("select *\nfrom ".len());
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 5);
    }

    #[test]
    fn completion_request_round_trips_items() {
        let (recv_rx, reply_tx, client) = harness();
        let position = lsp_types::Position::new(1, 5);

        // 请求放后台线程（阻塞）；主线程读到请求后回响应
        let client_for_thread = client.clone();
        let handle = std::thread::spawn(move || {
            request_completions(&client_for_thread, "file:///a.sql", position)
        });

        let (req_id, req) = next_request(&recv_rx, "textDocument/completion");
        assert_eq!(req["params"]["textDocument"]["uri"], "file:///a.sql");
        assert_eq!(req["params"]["position"]["character"], 5);

        let reply = serde_json::json!({
            "jsonrpc": "2.0", "id": req_id,
            "result": {
                "isIncomplete": false,
                "items": [
                    { "label": "orders", "kind": 5 },
                    { "label": "order_items", "kind": 5 },
                ]
            }
        });
        reply_tx
            .send(crate::lsp::frame::encode(&serde_json::to_vec(&reply).unwrap()))
            .unwrap();

        let response = handle.join().expect("补全线程应结束").expect("补全应成功");
        // 带 `isIncomplete` 的响应反序列化成 `List`，不带则成 `Array`——两者都合法，都断言
        let items = match response {
            lsp_types::CompletionResponse::Array(items) => items,
            lsp_types::CompletionResponse::List(list) => list.items,
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "orders");
    }

    #[test]
    fn completion_request_surfaces_rpc_error() {
        let (recv_rx, reply_tx, client) = harness();

        let client_for_thread = client.clone();
        let handle = std::thread::spawn(move || {
            request_completions(
                &client_for_thread,
                "file:///a.sql",
                lsp_types::Position::new(0, 0),
            )
        });

        let (req_id, _req) = next_request(&recv_rx, "textDocument/completion");
        let err_reply = serde_json::json!({
            "jsonrpc": "2.0", "id": req_id,
            "error": { "code": -32601, "message": "method not found" }
        });
        reply_tx
            .send(crate::lsp::frame::encode(&serde_json::to_vec(&err_reply).unwrap()))
            .unwrap();

        let err = handle.join().expect("补全线程应结束").expect_err("应报错");
        let text = format!("{err:#}");
        assert!(text.contains("-32601"), "错误应带 RPC 码，得到 {text}");
    }

    #[test]
    fn diagnostics_params_parse_to_list() {
        let params = serde_json::json!({
            "uri": "file:///a.sql",
            "diagnostics": [
                {
                    "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } },
                    "severity": 1,
                    "message": "bad column",
                },
                {
                    "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 6 } },
                    "message": "warn",
                }
            ],
            "version": 1,
        });
        let list = diagnostics_from_params(&params).expect("应解析成功");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].message, "bad column");
        assert_eq!(list[0].severity, Some(lsp_types::DiagnosticSeverity::ERROR));
        assert_eq!(list[1].range.start.line, 1);
    }

    #[test]
    fn diagnostics_params_missing_array_is_empty() {
        let params = serde_json::json!({ "uri": "file:///a.sql" });
        let list = diagnostics_from_params(&params).expect("缺数组时应解析成空列表");
        assert!(list.is_empty());
    }

    #[test]
    fn hover_params_carry_uri_and_position() {
        let position = lsp_types::Position::new(3, 7);
        let params = build_hover_params("file:///b.py", position);
        assert_eq!(params["textDocument"]["uri"], "file:///b.py");
        assert_eq!(params["position"]["line"], 3);
        assert_eq!(params["position"]["character"], 7);
    }

    #[test]
    fn hover_request_round_trips_content() {
        let (recv_rx, reply_tx, client) = harness();
        let position = lsp_types::Position::new(0, 4);

        let client_for_thread = client.clone();
        let handle = std::thread::spawn(move || {
            request_hover(&client_for_thread, "file:///b.py", position)
        });

        let (req_id, req) = next_request(&recv_rx, "textDocument/hover");
        assert_eq!(req["params"]["textDocument"]["uri"], "file:///b.py");
        assert_eq!(req["params"]["position"]["character"], 4);

        let reply = serde_json::json!({
            "jsonrpc": "2.0", "id": req_id,
            "result": {
                "contents": { "kind": "markdown", "value": "```python\ndef f():\n```\n说明" },
                "range": { "start": { "line": 0, "character": 2 }, "end": { "line": 0, "character": 5 } }
            }
        });
        reply_tx
            .send(crate::lsp::frame::encode(&serde_json::to_vec(&reply).unwrap()))
            .unwrap();

        let hover = handle.join().expect("hover 线程应结束").expect("应成功").expect("应有内容");
        // HoverContents 无 Display：按变体取 Markup 的 value 断言
        let value = match &hover.contents {
            lsp_types::HoverContents::Markup(m) => m.value.clone(),
            _ => String::new(),
        };
        assert!(value.contains("def f()"));
        assert_eq!(hover.range.expect("应有 range").start.character, 2);
    }

    #[test]
    fn hover_request_null_result_is_none() {
        let (recv_rx, reply_tx, client) = harness();
        let position = lsp_types::Position::new(0, 0);

        let client_for_thread = client.clone();
        let handle = std::thread::spawn(move || {
            request_hover(&client_for_thread, "file:///b.py", position)
        });

        let (req_id, _req) = next_request(&recv_rx, "textDocument/hover");
        let reply = serde_json::json!({ "jsonrpc": "2.0", "id": req_id, "result": null });
        reply_tx
            .send(crate::lsp::frame::encode(&serde_json::to_vec(&reply).unwrap()))
            .unwrap();

        let hover = handle.join().expect("hover 线程应结束").expect("应成功");
        assert!(hover.is_none(), "null 结果应解析为无 hover");
    }

    #[test]
    fn diagnostics_params_rejects_garbage() {
        let params = serde_json::json!({ "diagnostics": "not-an-array" });
        assert!(diagnostics_from_params(&params).is_err());
    }
}
