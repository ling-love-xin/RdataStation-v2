//! LSP 客户端核心（JSON-RPC 2.0 over stdio，方案 a 的宿主侧桥）
//!
//! ## 定位
//!
//! gpui-kit 编辑器自带的是 **LSP provider 接口 + `lsp-types` 数据模型**，不含 LSP client
//! （无 JSON-RPC / 进程 / initialize / publishDiagnostics 推送）。数据源与桥接逻辑由宿主提供。
//! 本模块就是那个"数据源"：一个走 **LSP 标准 Content-Length 分帧**（`frame`）的最小 client。
//!
//! ## 线程与生命周期（对齐 `scan.rs` 的成熟模式，不引 tokio）
//!
//! - 一条**读线程**负责从对端读帧并分发：响应 → 请求表里的 oneshot；通知 → 事件通道。
//! - 写侧由调用方（provider 的 background 任务）持锁直写；请求用"请求表 + oneshot + 超时"，
//!   与 `crates/plugin/src/sidecar/conn.rs::call` 同一套思想。
//! - **停机**：`shutdown()` 发 `shutdown` + `exit` 通知并关闭写侧 → 对端 stdin 见 EOF 自己退；
//!   读线程在 EOF 时退出；`Drop` 置停机标志等读线程收工（不靠引用计数——`scan.rs` 踩过的坑）。
//!
//! ## 可测性
//!
//! I/O 抽象成 [`LspReader`] / [`LspWriter`]：生产实现包 `std::process` 的 stdin/stdout；测试用**内存对端**
//! （一个预置响应的假 server），不真起进程。协议面（握手 / 请求响应关联 / 诊断推送）全部可单测。

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::lsp::frame;

/// LSP 对端（server）的**读侧**：读线程独占（阻塞到凑够一条；EOF → `Ok(None)`）
pub trait LspReader: Send + 'static {
    fn read_frame(&mut self) -> io::Result<Option<Vec<u8>>>;
}

/// LSP 对端（server）的**写侧**：由 client 内部锁共享（请求与通知都会写）
pub trait LspWriter: Send + 'static {
    fn write_frame(&mut self, bytes: &[u8]) -> io::Result<()>;
}

/// 进程 stdio 的生产实现（stdin = 写侧，stdout = 读侧）
pub struct ProcessIo {
    stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
    read_limit: usize,
}

impl ProcessIo {
    pub fn new(stdin: std::process::ChildStdin, stdout: std::process::ChildStdout) -> Self {
        Self {
            stdin,
            stdout,
            read_limit: frame::MAX_BODY_BYTES,
        }
    }
}

impl LspWriter for ProcessIo {
    fn write_frame(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stdin.write_all(bytes)?;
        self.stdin.flush()
    }
}

impl LspReader for ProcessIo {
    fn read_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        // LSP 帧：先读头到 \r\n\r\n，再按 Content-Length 读正文。逐字节攒头即可
        // （头本身很小；正文按声明长度一次读齐）。
        let mut header = Vec::with_capacity(128);
        loop {
            let mut byte = [0u8; 1];
            match self.stdout.read(&mut byte)? {
                0 => return Ok(None), // EOF：对端收工
                _ => {
                    header.push(byte[0]);
                    if header.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        let content_length = parse_header_length(&header)?;
        if content_length > self.read_limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Content-Length {content_length} 超过上限 {}", self.read_limit),
            ));
        }
        let mut body = vec![0u8; content_length];
        self.stdout.read_exact(&mut body)?;
        Ok(Some(body))
    }
}

/// 从头字节里解析 `Content-Length`（帧头；畸形给 `io::Error::InvalidData`）
fn parse_header_length(header: &[u8]) -> io::Result<usize> {
    let text = String::from_utf8_lossy(header);
    let value = text
        .lines()
        .find_map(|line| {
            let lower = line.trim().to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")?
                .trim()
                .to_owned()
                .into()
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "缺少 Content-Length"))?;
    value
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Content-Length 非法"))
}

/// 一条从对端收到的**通知**（无 id 的消息；`method` + `params`）
#[derive(Debug, Clone, PartialEq)]
pub struct LspNotification {
    pub method: String,
    pub params: Value,
}

/// 客户端事件（宿主轮询的出口；与 `scan.rs` 的 `try_recv` 同口径）
#[derive(Debug, Clone, PartialEq)]
pub enum LspEvent {
    /// `textDocument/publishDiagnostics`：`params = { uri, diagnostics }`
    Diagnostics(Value),
    /// 其它通知（原样透传，宿主按需忽略/处理）
    Notification(LspNotification),
    /// 读线程遇到 I/O 错误或协议畸形（对端已不可用；宿主应重启/报告）
    IoError(String),
}

/// 一次请求的结论
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// 对端关闭/连接已停（请求不会再有响应）
    Closed,
    /// 等待超时（请求已放弃；对端之后才回会被当作杂散响应丢弃）
    Timeout { after: Duration },
    /// 对端返回 JSON-RPC error 对象
    Rpc { code: i64, message: String },
    /// 本侧编解码/协议错误
    Protocol(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Closed => write!(f, "LSP 对端已关闭"),
            CallError::Timeout { after } => write!(f, "LSP 请求超时（{after:?}）"),
            CallError::Rpc { code, message } => write!(f, "LSP 错误 {code}: {message}"),
            CallError::Protocol(why) => write!(f, "LSP 协议错误：{why}"),
        }
    }
}

impl std::error::Error for CallError {}

/// 请求表：id → 等待中的 oneshot（响应/错误回填）
type Pending = HashMap<u64, Sender<Result<Value, CallError>>>;

/// 客户端共享状态（读线程 + 写侧共用）
struct Shared {
    pending: Mutex<Pending>,
    /// 停机标志（读线程与 Drop 的判据）
    shutdown: AtomicU64,
    /// 唤醒读线程的信号（停机时 notify）
    signal: Condvar,
    /// 停机等待锁（配合 Condvar）
    slot: Mutex<()>,
}

/// LSP 客户端：进程连接 + 请求/响应关联 + 通知分发
///
/// **Send + Sync**：宿主会在多个线程间共享同一个连接（provider 的 background 任务里
/// `call`、主线程轮询事件、多个文档共用一个 server）。事件接收端由 `new` 返回、宿主持有。
pub struct LspClient {
    shared: Arc<Shared>,
    writer: Mutex<Box<dyn LspWriter>>,
    _reader: Option<thread::JoinHandle<()>>,
    next_id: AtomicU64,
    /// 已初始化标志（initialize 成功前拒绝其它请求；Arc 共享，用原子）
    initialized: AtomicBool,
}

/// 客户端可克隆句柄（宿主在多个文档间共享同一个 server 连接）
pub type LspHandle = Arc<LspClient>;

impl LspClient {
    /// 用一条已建立的连接起客户端：读侧交给读线程，写侧由内部锁共享。
    ///
    /// 返回 `(客户端句柄, 事件接收端)`：`publishDiagnostics` 等通知从接收端读
    /// （`Sender` 留在客户端内部，保证 `LspClient` 是 `Send + Sync`）。
    ///
    /// 生产：`reader`/`writer` 是同一个 `ProcessIo` 的两半；测试：内存对端两半。
    pub fn new(reader: Box<dyn LspReader>, writer: Box<dyn LspWriter>) -> (Arc<Self>, Receiver<LspEvent>) {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending::new()),
            shutdown: AtomicU64::new(0),
            signal: Condvar::new(),
            slot: Mutex::new(()),
        });
        let (event_tx, events) = mpsc::channel::<LspEvent>();
        let reader_shared = Arc::clone(&shared);
        let reader = thread::Builder::new()
            .name("rds-editor-lsp-read".to_string())
            .spawn(move || reader_loop(&reader_shared, reader, &event_tx))
            .ok();
        let client = Arc::new(Self {
            shared,
            writer: Mutex::new(writer),
            _reader: reader,
            next_id: AtomicU64::new(1),
            initialized: AtomicBool::new(false),
        });
        (client, events)
    }

    /// 握手：`initialize`（LSP 标准），成功后置 initialized。
    ///
    /// `root_uri` 给 server 一个工作区根（无则给 `None`）。
    pub fn initialize(&self, root_uri: Option<&str>, timeout: Duration) -> Result<Value, CallError> {
        let params = json!({
            "processId": null,
            "rootUri": root_uri,
            "capabilities": {},
            "clientInfo": { "name": "rds-editor", "version": env!("CARGO_PKG_VERSION") },
        });
        let result = self.call("initialize", params, timeout)?;
        self.initialized.store(true, Ordering::SeqCst);
        // LSP 规定：initialize 响应后必须发 `initialized` 通知
        let _ = self.notify("initialized", json!({}));
        Ok(result)
    }

    /// 发一个请求并等响应（阻塞到响应/错误/超时）。
    ///
    /// 线程模型：从 **background 线程**调用（provider 的 `Task` 在 `cx.background_spawn` 里）；
    /// 读线程把响应写进请求表的 oneshot，本方法等它。
    pub fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let bytes = frame::encode(&serde_json::to_vec(&body).map_err(|e| CallError::Protocol(e.to_string()))?);
        let (tx, rx) = mpsc::channel::<Result<Value, CallError>>();
        {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            pending.insert(id, tx);
        }
        {
            let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = writer.write_frame(&bytes) {
                // 写失败：把请求表里自己那条摘掉，报 Closed
                let _ = self.shared.pending.lock().map(|mut p| p.remove(&id));
                return Err(CallError::Protocol(format!("写帧失败：{e}")));
            }
        }
        match rx.recv_timeout(timeout) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 超时放弃：摘掉请求表里自己那条（对端之后才回 → 当作杂散响应丢弃）
                let _ = self.shared.pending.lock().map(|mut p| p.remove(&id));
                Err(CallError::Timeout { after: timeout })
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(CallError::Closed),
        }
    }

    /// 发一个通知（不期待响应）
    pub fn notify(&self, method: &str, params: Value) -> Result<(), CallError> {
        let body = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let bytes = frame::encode(&serde_json::to_vec(&body).map_err(|e| CallError::Protocol(e.to_string()))?);
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        writer
            .write_frame(&bytes)
            .map_err(|e| CallError::Protocol(format!("写帧失败：{e}")))
    }

    /// 通知对端打开某文档（`didOpen`；`version` 自增，LSP 约定）
    pub fn did_open(&self, uri: &str, language_id: &str, text: &str) -> Result<(), CallError> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": text,
                }
            }),
        )
    }

    /// 通知对端全文已变（`didChange` 整篇替换；version 自增）
    pub fn did_change(&self, uri: &str, version: i64, text: &str) -> Result<(), CallError> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        )
    }

    /// 是否已握手（未握手时补全/诊断请求应被宿主拒绝）
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    /// 停机：发 `shutdown` 请求（尽力）→ `exit` 通知 → 关写侧 → 等读线程收工
    ///
    /// 真正的回收靠"关闭写侧 → 对端 stdin 见 EOF → 自己退"；读线程在 EOF 时退出。
    pub fn shutdown(&self) {
        let _ = self.call("shutdown", json!(null), Duration::from_secs(2));
        let _ = self.notify("exit", json!(null));
        // 释放写侧（drop ChildStdin → 对端见 EOF）
        if let Ok(mut writer) = self.writer.lock() {
            *writer = Box::new(NullWriter);
        }
        self.stop_reader();
    }
}

/// 停机用空写侧（丢写侧 = 对端 EOF 的约定）
struct NullWriter;

impl LspWriter for NullWriter {
    fn write_frame(&mut self, _bytes: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "已停机"))
    }
}

impl Drop for LspClient {
    /// 停机判据是标志（`scan.rs` 踩过的坑：引用计数到不了 1，join 会死等）
    fn drop(&mut self) {
        self.stop_reader();
    }
}

impl LspClient {
    fn stop_reader(&self) {
        self.shared.shutdown.fetch_add(1, Ordering::SeqCst);
        {
            let _guard = self
                .shared
                .slot
                .lock()
                .unwrap_or_else(|e| e.into_inner());
        }
        self.shared.signal.notify_all();
    }
}

/// 读线程：读帧 → 分发（响应 → 请求表 oneshot；通知 → 事件通道）
fn reader_loop(shared: &Arc<Shared>, mut reader: Box<dyn LspReader>, event_tx: &Sender<LspEvent>) {
    loop {
        // 停机检查（等待锁拿不到也算停机信号——避免忙等）
        if shared.shutdown.load(Ordering::SeqCst) != 0 {
            return;
        }
        match reader.read_frame() {
            Ok(Some(bytes)) => dispatch_frame(shared, &bytes, event_tx),
            Ok(None) => {
                // 对端 EOF：把还在等响应的请求全部失败
                fail_pending(shared, CallError::Closed);
                return;
            }
            Err(e) => {
                let _ = event_tx.send(LspEvent::IoError(e.to_string()));
                fail_pending(shared, CallError::Closed);
                return;
            }
        }
    }
}

/// 把一帧正文分发给对应入口
fn dispatch_frame(shared: &Arc<Shared>, bytes: &[u8], event_tx: &Sender<LspEvent>) {
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => {
            let _ = event_tx.send(LspEvent::IoError("LSP 正文不是合法 JSON".to_string()));
            return;
        }
    };
    // 响应带 id；通知带 method
    if value.get("id").is_some() {
        let id = value["id"].as_u64().unwrap_or(0);
        let outcome = if let Some(error) = value.get("error") {
            Err(CallError::Rpc {
                code: error["code"].as_i64().unwrap_or(0),
                message: error["message"].as_str().unwrap_or("").to_string(),
            })
        } else {
            Ok(value.get("result").cloned().unwrap_or(Value::Null))
        };
        let delivered = shared
            .pending
            .lock()
            .map(|mut p| p.remove(&id))
            .ok()
            .flatten();
        if let Some(tx) = delivered {
            let _ = tx.send(outcome);
        }
        // 已超时放弃的响应：无接收者，静默丢弃
    } else if let Some(method) = value.get("method").and_then(Value::as_str) {
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let event = if method == "textDocument/publishDiagnostics" {
            LspEvent::Diagnostics(params)
        } else {
            LspEvent::Notification(LspNotification {
                method: method.to_string(),
                params,
            })
        };
        let _ = event_tx.send(event);
    } else {
        let _ = event_tx.send(LspEvent::IoError("LSP 帧既无 id 也无 method".to_string()));
    }
}

/// 对端不可用时：让所有等待中的请求立刻失败（收摊不吞错）
fn fail_pending(shared: &Arc<Shared>, err: CallError) {
    if let Ok(mut pending) = shared.pending.lock() {
        let drained: Vec<_> = pending.drain().collect();
        for (_, tx) in drained {
            let _ = tx.send(Err(err.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::io;
    use std::sync::mpsc::{Receiver, Sender, channel};

    use super::{CallError, LspEvent, LspNotification, LspReader, LspWriter};

    /// 客户端侧读：从 channel 读（server 写来的帧），**解析头、只回 JSON 正文**
    /// （与 `ProcessIo::read_frame` 同语义：`LspReader` 的产物是正文不是整帧）
    struct MemoryReader {
        read_rx: Receiver<Vec<u8>>,
    }

    impl LspReader for MemoryReader {
        fn read_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
            match self.read_rx.recv() {
                Ok(bytes) => {
                    // 测试通道里存的是完整帧（含 Content-Length 头）；抽正文
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
        fn write_frame(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.write_tx
                .send(bytes.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "对端已关"))
        }
    }

    /// 从帧字节里抽出 JSON 正文（测试辅助：跳过 Content-Length 头）
    fn body_of(bytes: &[u8]) -> serde_json::Value {
        let text = String::from_utf8_lossy(bytes);
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
        serde_json::from_str(body).unwrap()
    }

    /// 建一对内存通道 + 客户端：
    /// - `recv_rx`（返回第一项）= 测试收 client 发来的请求帧；
    /// - `reply_tx`（返回第二项）= 测试回帧给 client（响应 / 通知）；
    /// - `client`（返回第三项）= 客户端句柄；
    /// - `events_rx`（返回第四项）= 客户端事件（诊断 / 通知）。
    fn harness(
    ) -> (
        Receiver<Vec<u8>>,
        Sender<Vec<u8>>,
        std::sync::Arc<super::LspClient>,
        Receiver<super::LspEvent>,
    ) {
        // client → server（写侧）：writer 发，server_read 收
        let (client_write, server_read) = channel::<Vec<u8>>();
        // server → client（读侧）：reply_tx 发，client_read 收
        let (reply_tx, client_read) = channel::<Vec<u8>>();
        let reader = MemoryReader { read_rx: client_read };
        let writer = MemoryWriter { write_tx: client_write };
        let (client, events_rx) = super::LspClient::new(Box::new(reader), Box::new(writer));
        (server_read, reply_tx, client, events_rx)
    }

    #[test]
    fn initialize_handshake_round_trips() {
        let (recv_rx, reply_tx, client, _events_rx) = harness();

        // client 的 initialize 是阻塞调用：放后台线程发，主线程读请求、回响应
        let client_for_thread = client.clone();
        let init_handle = std::thread::spawn(move || {
            client_for_thread.initialize(None, std::time::Duration::from_secs(2))
        });

        // server 侧：等收到 initialize，回 result
        let first = recv_rx.recv().expect("应收到 initialize 请求");
        let req = body_of(&first);
        assert_eq!(req["method"], "initialize");
        assert_eq!(req["jsonrpc"], "2.0");
        let id = req["id"].as_u64().unwrap();
        // 回响应（完整帧）
        let resp_body = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0", "id": id,
            "result": { "capabilities": {} }
        })).unwrap();
        reply_tx.send(crate::lsp::frame::encode(&resp_body)).unwrap();

        let result = init_handle.join().expect("initialize 线程应结束").expect("握手应成功");
        assert_eq!(result["capabilities"], serde_json::json!({}));
        assert!(client.is_initialized());

        // 握手后应收到 initialized 通知（server 侧收到）
        let notify = recv_rx.recv().expect("应收到 initialized 通知");
        let notify = body_of(&notify);
        assert_eq!(notify["method"], "initialized");
        assert!(notify.get("id").is_none(), "通知不该带 id");
    }

    /// 从 server 侧通道读下一帧，断言 method；跳过 `initialized` 等握手期通知
    /// （`initialize` 成功后 client 会主动发 `initialized` 通知，它先进通道）
    fn next_request(recv_rx: &Receiver<Vec<u8>>, want: &str) -> u64 {
        loop {
            let frame = recv_rx.recv().expect("应收到请求");
            let req = body_of(&frame);
            if let Some(method) = req["method"].as_str() {
                if method == "initialized" {
                    continue; // 握手期通知，跳过
                }
                assert_eq!(method, want, "方法名应匹配");
            }
            return req["id"].as_u64().expect("请求应带 id");
        }
    }

    #[test]
    fn completion_request_gets_its_response() {
        let (recv_rx, reply_tx, client, _events_rx) = harness();

        // initialize 握手（后台线程）
        let init_handle = std::thread::spawn({
            let client = client.clone();
            move || client.initialize(None, std::time::Duration::from_secs(2))
        });
        let id = next_request(&recv_rx, "initialize");
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } });
        reply_tx.send(crate::lsp::frame::encode(&serde_json::to_vec(&resp).unwrap())).unwrap();
        init_handle.join().unwrap().expect("握手应成功");

        // completion 请求放后台线程；主线程读到请求后回响应
        let call_handle = std::thread::spawn({
            let client = client.clone();
            move || {
                client.call(
                    "textDocument/completion",
                    serde_json::json!({
                        "textDocument": { "uri": "file:///a.sql" },
                        "position": { "line": 0, "character": 3 },
                    }),
                    std::time::Duration::from_secs(2),
                )
            }
        });
        let req_id = next_request(&recv_rx, "textDocument/completion");
        let completion_reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "isIncomplete": false,
                "items": [{ "label": "orders" }]
            }
        });
        reply_tx.send(crate::lsp::frame::encode(&serde_json::to_vec(&completion_reply).unwrap())).unwrap();

        let result = call_handle.join().unwrap().expect("completion 应成功");
        assert_eq!(result["items"][0]["label"], "orders");
    }

    #[test]
    fn publish_diagnostics_becomes_an_event() {
        let (_recv_rx, reply_tx, _client, events_rx) = harness();

        // server 主动推一条 publishDiagnostics 通知（无需请求）
        let notify = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///a.sql",
                "diagnostics": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } }, "message": "bad" }]
            }
        });
        reply_tx.send(crate::lsp::frame::encode(&serde_json::to_vec(&notify).unwrap())).unwrap();

        let event = poll_event(&events_rx, 50);
        match event {
            Some(LspEvent::Diagnostics(params)) => {
                assert_eq!(params["uri"], "file:///a.sql");
                assert_eq!(params["diagnostics"][0]["message"], "bad");
            }
            other => panic!("应当收到 Diagnostics 事件，得到 {other:?}"),
        }
    }

    #[test]
    fn server_error_object_is_surfaced_as_rpc_error() {
        let (recv_rx, reply_tx, client, _events_rx) = harness();

        // initialize 先成功（后台线程）
        let init_handle = std::thread::spawn({
            let client = client.clone();
            move || client.initialize(None, std::time::Duration::from_secs(2))
        });
        let id = next_request(&recv_rx, "initialize");
        let resp = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {} } });
        reply_tx.send(crate::lsp::frame::encode(&serde_json::to_vec(&resp).unwrap())).unwrap();
        init_handle.join().unwrap().unwrap();

        // 发 completion（后台线程）；主线程读到请求后回 error
        let outcome_handle = std::thread::spawn({
            let client = client.clone();
            move || {
                client.call(
                    "textDocument/completion",
                    serde_json::json!({}),
                    std::time::Duration::from_secs(2),
                )
            }
        });
        let req_id = next_request(&recv_rx, "textDocument/completion");
        let err_resp = serde_json::json!({
            "jsonrpc": "2.0", "id": req_id,
            "error": { "code": -32601, "message": "method not found" }
        });
        reply_tx.send(crate::lsp::frame::encode(&serde_json::to_vec(&err_resp).unwrap())).unwrap();

        match outcome_handle.join().unwrap() {
            Err(CallError::Rpc { code, message }) => {
                assert_eq!(code, -32601);
                assert_eq!(message, "method not found");
            }
            other => panic!("应当收到 Rpc 错误，得到 {other:?}"),
        }
    }

    #[test]
    fn other_notifications_are_passed_through() {
        let (_recv_rx, reply_tx, _client, events_rx) = harness();

        let notify = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "hi" }
        });
        reply_tx.send(crate::lsp::frame::encode(&serde_json::to_vec(&notify).unwrap())).unwrap();

        match poll_event(&events_rx, 50) {
            Some(LspEvent::Notification(LspNotification { method, params })) => {
                assert_eq!(method, "window/logMessage");
                assert_eq!(params["message"], "hi");
            }
            other => panic!("应当收到 Notification 事件，得到 {other:?}"),
        }
    }

    /// 轮询客户端事件（最多等 `ms` 毫秒）
    fn poll_event(events: &Receiver<super::LspEvent>, ms: u64) -> Option<super::LspEvent> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        loop {
            if let Ok(event) = events.try_recv() {
                return Some(event);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
