//! LSP 宿主端口的**工作台实现**（插件原型 §14.2 第四 / 五刀）：按需起语言服务 → 文档 ↔ 会话 → 空闲回收
//!
//! ## 一句话
//!
//! editor 侧只声明端口契约（`editor::lsp::LspPort`：文档 → 会话 + 按 `uri` 认领事件 + `wants`），
//! **真正起 LSP server 的是宿主**。本模块就是那个宿主实现：按需探测 Python 环境、spawn
//! `python -m jedi_language_server`（v1 候选），后台完成 `initialize` 握手（不阻塞主线程）。
//!
//! ## 生命周期（第五刀：「状态栏 LSP 管理器」）
//!
//! **按需启动 + 空闲回收 + 手动启停覆盖**，三条互不冲突：
//!
//! - **按需启动**：装配时**不** spawn。有文档要服务（`service()` 判定「通用模式 + `.py`」）才
//!   `ensure_started()`；`spawn` 只要几毫秒，`initialize` 后台跑，就绪前 `service()` 如实返回
//!   `None`（面板据此保持轮询，就绪即接上）。
//! - **空闲回收**（[`reap_if_idle`]，视图 1 s 一拍）：**关掉最后一个需要服务的面板 → 停 server**。
//!   回收只停服务、不设 blocking，下次有文档要服务会自动重启。
//! - **手动启停**（[`start`] / [`stop`]，状态栏调用）：`stop` 置 blocking（手动停止），`start`
//!   清 blocking 并拉起。服务被停后，面板靠 [`LspPort::wants`] 继续轻轮询，**重启即自动重连**。
//! - **崩溃**：[`mark_dead`]——读线程断了（`IoError` / channel `Disconnected`）即广播死讯、摘
//!   句柄、置 blocking（待手动重启，避免崩溃热循环）。
//!
//! ## 关键决策
//!
//! - **v1 只服务 Python**（`.py` 文档，jedi-language-server）：SQL 模式不引专用 LSP（D11 拍板，
//!   诊断走本地 lint）；分析模式（`.rdsnote` 笔记本）暂不服务；Markdown 的 LSP 留后。没装
//!   jedi-language-server / 没有 Python → spawn 失败 = 如实无服务（记原因，不自动重试、不假装）。
//! - **宿主持 `EditorShared`**（Rc 句柄，克隆便宜）：`service(&DocumentId)` 只给 id（不含路径 /
//!   模式），宿主在文档集合里反查 `mode()` + `path()` 再判定语言——不改 editor 侧端口形状。
//! - **事件认领**（[`claim_events`]）：`publishDiagnostics` 按 `uri` 归本文档、其它文档暂存；
//!   `IoError` 是**全局死讯**（server 死了所有文档都受影响）→ 单独存 `dead`，每个 `drain_for`
//!   都广播，面板各自摘 provider / 停轮询，自然收敛。
//!
//! ## 无孤儿进程
//!
//! 面板关闭 / 换绑已发 `didClose`（editor 侧）。宿主侧 server 的停用走 [`WorkbenchLsp::teardown`] /
//! [`WorkbenchLsp::mark_dead`] → 丢后台线程跑 `LspClient::shutdown()`（发 `shutdown` + `exit`、
//! 关写侧），对端 stdin 见 EOF 自退；应用退出时的兜底靠 `LspClient::Drop` 关读线程 + 进程随
//! stdout EOF 自退。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use editor::lsp::{LspEvent, LspHandle, LspPort, LspSession};
use editor::model::{DocumentId, EditorMode};
use editor::shared::EditorShared;

use super::editor_env;

/// `initialize` 握手超时：Python 首次 import jedi 可能很慢（1~2 秒），给足余量
const INIT_TIMEOUT: Duration = Duration::from_secs(15);

/// 服务没能运行的原因（状态栏显示用；`None` = 没拦着，该起就会起）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspBlock {
    /// 用户手动停止
    Manual,
    /// 启动失败（Python 环境 / spawn）
    StartFailed(String),
    /// 运行中断开（读线程断 / 握手失败）
    Crashed(String),
}

/// 文档 → 语言 id（**纯函数，可单测**）
///
/// - SQL 模式：`None`（D11：SQL 不引专用 LSP，诊断走本地 lint）；
/// - 分析模式：`None`（`.rdsnote` 笔记本的 LSP 是后续，v1 不服务）；
/// - 通用模式：按文件扩展名推断（`.py` → python）；未命名（无路径）→ `None`（不猜）；
/// - 其它扩展名 → `None`（v1 只有 Python 一个 server，不假装服务别的语言）。
fn language_for(mode: EditorMode, path: Option<&Path>) -> Option<&'static str> {
    if mode != EditorMode::General {
        return None;
    }
    let ext = path?.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "py" | "python" => Some("python"),
        _ => None,
    }
}

/// 文档 → LSP uri（**纯函数，可单测**）
///
/// `DocumentId` 不含路径（`doc-<serial>` 纯序号），宿主拿不到磁盘路径时用未命名规则；
/// 每个文档独立 uri（`didOpen` 各自开），面板按它认领诊断。
fn uri_for(document: &DocumentId) -> String {
    format!("untitled:Rds-{}", document.as_str())
}

/// 把一批事件按 `uri` 认领：本文档的 `publishDiagnostics` 归 `claimed`，其它文档的暂存
/// （**不丢**：多面板共用一个 server，别家的事件等别家来取）；`IoError` 是全局死讯，交给
/// 调用方单独广播（不进 `claimed`，也不暂存给某一方）。其余通知忽略。
///
/// 返回 `(本文档的, 其余暂存的)`。
fn claim_events(uri: &str, events: VecDeque<LspEvent>) -> (Vec<LspEvent>, VecDeque<LspEvent>) {
    let mut claimed = Vec::new();
    let mut rest = VecDeque::new();
    for event in events {
        match &event {
            LspEvent::Diagnostics(params) => {
                let same = params.get("uri").and_then(|u| u.as_str()) == Some(uri);
                if same {
                    claimed.push(event);
                } else {
                    rest.push_back(event);
                }
            }
            // IoError：不进 claimed 也不暂存——由宿主存 `dead` 字段做广播（每个文档都该知道）
            LspEvent::IoError(_) | LspEvent::Notification(_) => {}
        }
    }
    (claimed, rest)
}

/// 工作台实现的 LSP 宿主端口（[`editor::lsp::LspPort`]）
///
/// 一个常驻 server 服务多份文档；`service` 反查文档集合定语言并按需拉起服务，`drain_for` 按 `uri` 认领。
pub struct WorkbenchLsp {
    /// editor 共享态（`service()` / `wants()` 里反查文档的 `mode()` / `path()`）
    editor: EditorShared,
    /// 已建立的 server 连接（按需 spawn；未起 / 已停 / 失败 = `None`）
    server: RefCell<Option<LspHandle>>,
    /// 事件接收端（`LspClient::new` 返回；与 `server` 同生共死）
    events: RefCell<Option<Receiver<LspEvent>>>,
    /// 暂存不属于当前询问文档的事件（多文档共享 server）
    pending: RefCell<VecDeque<LspEvent>>,
    /// 全局死讯（`IoError`；server 死后置 `Some`，每个 `drain_for` 都广播）
    dead: RefCell<Option<String>>,
    /// 拦住再启动的原因（手动停 / 启动失败 / 崩溃）：`Some` = 现在不启，`service()` 返回 `None`
    blocked: RefCell<Option<LspBlock>>,
    /// 后台握手是否已失败（init 线程置位；`service()` 据此收尾，避免卡在「启动中」）
    init_failed: Arc<AtomicBool>,
    /// 状态版本（启动 / 停止 / 崩溃 / 握手就绪都自增）：视图据此按需重绘状态栏指示
    epoch: Arc<AtomicU64>,
}

impl WorkbenchLsp {
    /// 建端口（**不 spawn**）：首次有文档要服务时由 `service()` 按需拉起
    pub fn new(editor: EditorShared) -> Rc<Self> {
        Rc::new(Self {
            editor,
            server: RefCell::new(None),
            events: RefCell::new(None),
            pending: RefCell::new(VecDeque::new()),
            dead: RefCell::new(None),
            blocked: RefCell::new(None),
            init_failed: Arc::new(AtomicBool::new(false)),
            epoch: Arc::new(AtomicU64::new(0)),
        })
    }

    /// 状态版本（视图轮询；变了就重绘状态栏指示）
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// 状态版本自增（内部状态变了就调一次）
    fn bump(&self) {
        let _ = self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// 服务是否已拉起（还没握手完成也算「在跑」）
    pub fn is_running(&self) -> bool {
        self.server.borrow().is_some()
    }

    /// 服务是否已完成 `initialize` 握手（补全 / 诊断可用）
    pub fn is_initialized(&self) -> bool {
        self.server
            .borrow()
            .as_ref()
            .is_some_and(|client| client.is_initialized())
    }

    /// 拦住再启动的原因（`None` = 没拦着；状态栏据此显示文案）
    pub fn block(&self) -> Option<LspBlock> {
        self.blocked.borrow().clone()
    }

    /// 当前**需要**语言服务的文档数（通用模式 + `.py`）；0 = 没有面板在用，可回收
    pub fn active_document_count(&self) -> usize {
        let docs = self.editor.service();
        docs.documents()
            .iter()
            .filter(|doc| language_for(doc.mode(), doc.path()).is_some())
            .count()
    }

    /// 手动启动（状态栏「启动」）：清拦住原因（含崩溃 / 启动失败）+ 拉起
    pub fn start(&self) {
        *self.blocked.borrow_mut() = None;
        *self.dead.borrow_mut() = None;
        self.ensure_started();
    }

    /// 手动停止（状态栏「停止」）：置拦住原因 + 停服务；面板保持轮询，点「启动」即自动重连
    pub fn stop(&self) {
        *self.blocked.borrow_mut() = Some(LspBlock::Manual);
        self.teardown();
    }

    /// 空闲回收：**关掉最后一个需要服务的面板 → 停 server**（不设拦住原因，下次需要会自动重启）
    pub fn reap_if_idle(&self) {
        if self.server.borrow().is_some() && self.active_document_count() == 0 {
            self.teardown();
        }
    }

    /// 按需拉起服务（已跑 / 已被拦住则不动）
    fn ensure_started(&self) {
        if self.blocked.borrow().is_some() || self.server.borrow().is_some() {
            return;
        }
        match spawn_server() {
            Ok((client, events)) => {
                let handle = client.clone();
                let failed = self.init_failed.clone();
                // 后台完成 `initialize` 握手（主线程不冻结）；`is_initialized()` 是原子读，
                // 就绪前 `service()` 如实返回 None。失败置共享标志，等 `service()` 收尾。
                std::thread::Builder::new()
                    .name("rds-workbench-lsp-init".to_string())
                    .spawn(move || {
                        if client.initialize(None, INIT_TIMEOUT).is_err() {
                            failed.store(true, Ordering::SeqCst);
                        }
                    })
                    .ok();
                *self.server.borrow_mut() = Some(handle);
                *self.events.borrow_mut() = Some(events);
                self.bump();
            }
            Err(reason) => {
                // 探测 / spawn 失败：如实记原因并拦住（不每拍重试、不假装）
                *self.blocked.borrow_mut() = Some(LspBlock::StartFailed(reason));
                self.bump();
            }
        }
    }

    /// 停服务（不设拦住原因）：手动停止 / 空闲回收共用
    fn teardown(&self) {
        self.take_server();
        self.pending.borrow_mut().clear();
        *self.dead.borrow_mut() = None;
        self.bump();
    }

    /// 服务退出（读线程断 / channel 断开 / 握手失败）：广播死讯 + 摘句柄 + 置拦住原因（待手动重启）
    ///
    /// **不自动重启**：`blocked` 让 `service()` 返回 `None`，面板保持轮询但不再接；用户在状态栏
    /// 点「启动」才清 `blocked` 重启——避免「崩溃 → 自动重启 → 再崩溃」的热循环。
    fn mark_dead(&self, reason: String) {
        *self.dead.borrow_mut() = Some(reason.clone());
        *self.blocked.borrow_mut() = Some(LspBlock::Crashed(reason));
        self.take_server();
        self.pending.borrow_mut().clear();
        self.bump();
    }

    /// 摘掉连接与事件端：`shutdown()` 含一次最长 2 s 的 RPC，丢后台线程别卡主线程
    fn take_server(&self) {
        if let Some(client) = self.server.borrow_mut().take() {
            std::thread::Builder::new()
                .name("rds-workbench-lsp-shutdown".to_string())
                .spawn(move || client.shutdown())
                .ok();
        }
        *self.events.borrow_mut() = None;
    }
}

impl LspPort for WorkbenchLsp {
    fn service(&self, document: &DocumentId) -> Option<LspSession> {
        // 文档不存在（已关）→ 无服务；模式 / 扩展名不支持 → 无服务（不猜）
        // `service()` 返回借用（Ref），必须 let 绑定撑住生命周期
        let language_id = {
            let docs = self.editor.service();
            let doc = docs.find(document)?;
            language_for(doc.mode(), doc.path())?.to_string()
        };
        // 后台握手失败 → 收尾（否则会卡在「启动中」：面板一直问、一直拿不到会话）
        if self.init_failed.swap(false, Ordering::SeqCst) {
            self.mark_dead("初始化握手失败".to_string());
            return None;
        }
        // 被拦住（手动停 / 启动失败 / 崩溃待重启）→ 无服务（如实）
        if self.blocked.borrow().is_some() {
            return None;
        }
        // 首次有文档要服务时按需拉起（就绪前返回 None，面板保持轮询，就绪即接上）
        self.ensure_started();
        let client = self.server.borrow().as_ref()?.clone();
        if !client.is_initialized() {
            return None;
        }
        Some(LspSession {
            client,
            uri: uri_for(document),
            language_id,
        })
    }

    fn drain_for(&self, document: &DocumentId) -> Vec<LspEvent> {
        // 先收新事件（不阻塞；读线程持续写进 channel），并入暂存
        let mut events = std::mem::take(&mut *self.pending.borrow_mut());
        let mut died: Option<String> = None;
        if let Some(rx) = self.events.borrow_mut().as_mut() {
            loop {
                match rx.try_recv() {
                    // 全局死讯：不进暂存，走 `mark_dead` 广播（每个文档都该知道）
                    Ok(LspEvent::IoError(reason)) => {
                        if died.is_none() {
                            died = Some(reason);
                        }
                    }
                    Ok(event) => events.push_back(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if died.is_none() {
                            died = Some("语言服务已退出".to_string());
                        }
                        break;
                    }
                }
            }
        }
        let (claimed, rest) = claim_events(&uri_for(document), events);
        if let Some(reason) = died {
            // 首次发现死讯：广播 + 摘句柄 + 拦住（待手动重启）；暂存的一并丢弃（server 没了）
            self.mark_dead(reason);
        } else {
            *self.pending.borrow_mut() = rest;
        }
        // 全局死讯广播：server 死后每个文档的 `drain_for` 都带上一份（面板据此摘 provider 停泵）
        let mut result = claimed;
        if let Some(dead) = self.dead.borrow().clone() {
            result.insert(0, LspEvent::IoError(dead));
        }
        result
    }

    fn wants(&self, document: &DocumentId) -> bool {
        // 「想要」只看文档本身该不该配服务，**不看服务启没启 / 是不是被停了**：
        // 面板据此在服务未起 / 被停 / 崩溃时保持轻轮询，重启后自动重连。
        let docs = self.editor.service();
        docs.find(document)
            .is_some_and(|doc| language_for(doc.mode(), doc.path()).is_some())
    }
}

/// 注入 LSP 端口（**生产装配调用一次**）并返回端口句柄（状态栏 LSP 管理器持有它做启停 / 回收）
///
/// 测试装配（`WorkbenchView::new_for_test`）**不调这个**：真 spawn 子进程在测试里是副作用，
/// editor 侧测试用假端口（`FakePort`）直接 `attach_lsp`，workbench 侧不测 LSP。
pub fn attach(editor: &EditorShared) -> Rc<WorkbenchLsp> {
    let port = WorkbenchLsp::new(editor.clone());
    editor.attach_lsp(port.clone());
    port
}

/// 探测 Python 环境并 spawn jedi-language-server，返回「连接 + 事件接收端」
///
/// stderr 丢 `Stdio::null()`：LSP server 的日志不读会填满管道卡死进程；v1 不要它的日志
/// （要排障时改成后台线程 drain）。
fn spawn_server() -> Result<(LspHandle, Receiver<LspEvent>), String> {
    let resolved = editor_env::resolve("python").map_err(|e| format!("Python 环境不可用：{e}"))?;
    let mut command = std::process::Command::new(&resolved.command);
    command
        .args(&resolved.args)
        .arg("-m")
        .arg("jedi_language_server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 LSP server（{}）失败：{e}", resolved.command))?;
    let stdin = child
        .stdin
        .take()
        .ok_or("无法获取子进程 stdin（spawn 后即被占用）")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("无法获取子进程 stdout（spawn 后即被占用）")?;
    // `LspClient` 只拿 stdin / stdout；`child` 句柄 drop 不杀进程（Windows 语义），
    // server 进程靠 `shutdown` / 关闭写侧（EOF）自行退出——无孤儿由编辑器侧 `Drop` 兜底。
    Ok(editor::lsp::LspClient::from_process(stdin, stdout))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::collections::VecDeque;
    use std::path::Path;

    use editor::lsp::LspEvent;
    use editor::model::{DocumentId, EditorMode};

    use super::{claim_events, language_for, uri_for};

    fn doc(id: &str) -> DocumentId {
        DocumentId::new(id)
    }

    #[test]
    fn language_for_is_none_outside_general_mode() {
        // SQL / 分析模式一律不配服务（D11 / 笔记本 v1）
        assert_eq!(language_for(EditorMode::Sql, Some(Path::new("a.py"))), None);
        assert_eq!(language_for(EditorMode::Analysis, Some(Path::new("a.py"))), None);
        // 未命名（无路径）不猜
        assert_eq!(language_for(EditorMode::General, None), None);
    }

    #[test]
    fn language_for_maps_python_by_extension() {
        assert_eq!(language_for(EditorMode::General, Some(Path::new("a.py"))), Some("python"));
        assert_eq!(language_for(EditorMode::General, Some(Path::new("x/a.PY"))), Some("python"));
        // 其它扩展名 / 非 python 语言 → 无服务（v1 只有 Python 一个 server）
        assert_eq!(language_for(EditorMode::General, Some(Path::new("a.md"))), None);
        assert_eq!(language_for(EditorMode::General, Some(Path::new("a.rs"))), None);
        assert_eq!(language_for(EditorMode::General, Some(Path::new("a.txt"))), None);
    }

    #[test]
    fn uri_for_is_stable_and_distinct() {
        assert_eq!(uri_for(&doc("doc-1")), "untitled:Rds-doc-1");
        assert_ne!(uri_for(&doc("doc-1")), uri_for(&doc("doc-2")));
    }

    #[test]
    fn claim_events_keeps_own_and_stashes_others() {
        let uri = uri_for(&doc("doc-1"));
        let events: VecDeque<LspEvent> = vec![
            LspEvent::Diagnostics(serde_json::json!({
                "uri": uri_for(&doc("doc-2")),
                "diagnostics": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "message": "别人的" }]
            })),
            LspEvent::Diagnostics(serde_json::json!({
                "uri": uri,
                "diagnostics": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "message": "我的" }]
            })),
            LspEvent::Notification(editor::lsp::LspNotification {
                method: "window/logMessage".to_string(),
                params: serde_json::json!({}),
            }),
            LspEvent::IoError("对端已关".to_string()),
        ]
        .into();

        let (claimed, rest) = claim_events(&uri, events);
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0], LspEvent::Diagnostics(serde_json::json!({
            "uri": uri,
            "diagnostics": [{ "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "message": "我的" }]
        })));
        // 别人的诊断暂存等别人来取；通知 / IoError 不进暂存（IoError 由宿主 `dead` 字段广播）
        assert_eq!(rest.len(), 1);
        assert!(matches!(&rest[0], LspEvent::Diagnostics(_)));
    }

    #[test]
    fn claim_events_handles_empty_input() {
        let uri = uri_for(&doc("doc-9"));
        let (claimed, rest) = claim_events(&uri, VecDeque::new());
        assert!(claimed.is_empty());
        assert!(rest.is_empty());
    }
}
