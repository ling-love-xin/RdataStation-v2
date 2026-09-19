//! sidecar stdio 连接：把 [`super::proto`] 的分帧与 [`super::router`] 的附件语义
//! 接成「方法调用 → 结果」的异步客户端（D5 的落点）。
//!
//! ```text
//! caller ──Call──▶ ┌─ driver 任务 ────────────────┐ ──Frame──▶ sidecar
//!                  │ Router（附件语义 / 错位判定） │
//! caller ◀─reply── │ 在飞表（id → oneshot）       │ ◀─Frame── reader 任务
//!                  └───────────────────────────────┘
//!                          │
//!                          └──ConnEvent──▶ 事件订阅方（通知 / 错位 / 断线）
//! ```
//!
//! ## 三个设计选择（都不是随手定的）
//!
//! 1. **在飞状态只有一份**：`Router` 与「id → oneshot」都归 driver 任务独占。
//!    两边各存一份"谁在等什么"是这类客户端最经典的走味源头 —— 迟早对不上。
//! 2. **读侧单独一个任务**：`tokio::select!` 会取消没被选中的分支，而
//!    `read_frame` **不是**取消安全的（读了一半的帧会丢）。所以让读侧只管读、把帧发到
//!    通道，driver 只 select 通道（通道 `recv` 是取消安全的）。
//! 3. **超时要显式放弃**：超时后调用方发 `Abandon`，driver 把它从在飞表里摘掉；
//!    之后姗姗来迟的响应会被 `Router` 报成 `Issue`（不静默吞）。
//!
//! 传输本身就是一对字节流，所以这个类型**同时**是"宿主连 sidecar"和"sidecar 连宿主"
//! 的客户端 —— 靶子（P1 用 PostgreSQL 包一层）可以直接拿它写测试。

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::proto::{
    Frame, FrameIoError, PROTOCOL_VERSION, RpcErrorCode, check_protocol_version, read_frame,
    write_frame,
};
use super::router::{Attachment, Router, RouterEvent};

/// 一次调用的完整结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallOutcome {
    /// JSON 结果体（`result`）。
    pub result: Value,
    /// 附件（已按声明收齐的 Arrow IPC 流）。
    ///
    /// ⚠️ **空不代表"没有数据"**：小结果按 §4.2.1.2 的阈值走 `result.rows` 内联，
    /// 只有大结果才走附件。
    pub attachments: Vec<Attachment>,
}

/// 调用失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// 对端返回了 JSON-RPC 错误（错误码已识别，见 `dev-plan` §4.2.2）。
    Rpc {
        code: RpcErrorCode,
        message: String,
        data: Option<Value>,
    },
    /// 对端返回的错误码我们不认识（对端比我们新）。
    RpcUnknownCode { raw_code: i32, message: String },
    /// 超时 —— **该请求已被放弃**；迟到的响应会变成 [`ConnEvent::Issue`]。
    Timeout { method: String, after: Duration },
    /// 连接断了（对端退出 / 管道关闭）。
    Disconnected { reason: String },
    /// 帧层失败（协议错或 I/O 错）。
    Frame { detail: String },
    /// 连接已关闭（driver 任务退出）。
    Closed,
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rpc { code, message, .. } => write!(f, "RPC 错误 {code}：{message}"),
            Self::RpcUnknownCode { raw_code, message } => {
                write!(f, "RPC 错误（未知码 {raw_code}）：{message}")
            }
            Self::Timeout { method, after } => {
                write!(f, "调用 {method} 超时（{:?}）", after)
            }
            Self::Disconnected { reason } => write!(f, "连接已断开：{reason}"),
            Self::Frame { detail } => write!(f, "帧层失败：{detail}"),
            Self::Closed => write!(f, "连接已关闭"),
        }
    }
}

impl std::error::Error for CallError {}

/// 连接上的异步事件（不属于任何一次调用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnEvent {
    /// 通知（插件 → 宿主）：`log` / `progress` 等。
    Notification { method: String, params: Value },
    /// 流层面的不对：错位、未知种类、超时后迟到的响应……
    Issue { detail: String },
    /// 连接断开（在飞调用会各自收到 [`CallError::Disconnected`]）。
    Disconnected {
        reason: String,
        abandoned_ids: Vec<u64>,
    },
}

/// 事件订阅端。
#[derive(Debug)]
pub struct ConnEvents {
    rx: mpsc::UnboundedReceiver<ConnEvent>,
}

impl ConnEvents {
    /// 取下一个事件（连接彻底结束且事件耗尽时返回 `None`）。
    pub async fn recv(&mut self) -> Option<ConnEvent> {
        self.rx.recv().await
    }

    /// 非阻塞取（用于"顺手看看有没有新事件"）。
    pub fn try_recv(&mut self) -> Option<ConnEvent> {
        self.rx.try_recv().ok()
    }
}

enum DriverCommand {
    Call {
        id: u64,
        frame: Frame,
        reply: oneshot::Sender<Result<CallOutcome, CallError>>,
    },
    Abandon {
        id: u64,
    },
    Shutdown,
}

/// stdio 连接。
pub struct SidecarConn {
    cmd_tx: mpsc::Sender<DriverCommand>,
    next_id: AtomicU64,
    /// driver 任务的句柄（只用于「别在它还在跑时把进程忘了」这类诊断；不等它）。
    driver: JoinHandle<()>,
}

impl Drop for SidecarConn {
    fn drop(&mut self) {
        // 连接被丢掉 = 写侧随之关闭：对端会看到 stdin EOF 并自行退出（不是“当垃圾扫掉”）。
        self.driver.abort();
    }
}

impl SidecarConn {
    /// 在一对字节流上起一条连接（读侧与写侧各一个半）。
    ///
    /// 宿主侧是 `(child.stdout, child.stdin)`；测试里用 `tokio::io::duplex` 即可。
    pub fn spawn<R, W>(reader: R, writer: W) -> (Self, ConnEvents)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (frame_tx, frame_rx) = mpsc::unbounded_channel::<Result<Frame, FrameIoError>>();
        let (event_tx, event_rx) = mpsc::unbounded_channel::<ConnEvent>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);

        // 读侧：只负责把字节变成帧（自己独占 reader，所以不受 select 取消影响）
        tokio::spawn(async move {
            let mut reader = reader;
            loop {
                match read_frame(&mut reader).await {
                    Ok(frame) => {
                        if frame_tx.send(Ok(frame)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = frame_tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        let driver = tokio::spawn(driver_loop(writer, frame_rx, cmd_rx, event_tx));

        (
            Self {
                cmd_tx,
                next_id: AtomicU64::new(0),
                driver,
            },
            ConnEvents { rx: event_rx },
        )
    }

    /// 发起一次调用。
    ///
    /// `timeout` 到点后本方法返回 [`CallError::Timeout`]，并**放弃**该请求
    /// （对端若之后才回，会作为 [`ConnEvent::Issue`] 报出来）。
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<CallOutcome, CallError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let payload = serde_json::to_vec(&body).map_err(|e| CallError::Frame {
            detail: e.to_string(),
        })?;

        let (reply, rx) = oneshot::channel();
        self.cmd_tx
            .send(DriverCommand::Call {
                id,
                frame: Frame::json(payload),
                reply,
            })
            .await
            .map_err(|_| CallError::Closed)?;

        match tokio::time::timeout(timeout, rx).await {
            // driver 把结果递回来了
            Ok(Ok(outcome)) => outcome,
            // driver 丢了 reply（收摊时不该发生：收摊会逐个失败）
            Ok(Err(_)) => Err(CallError::Closed),
            Err(_) => {
                let _ = self.cmd_tx.send(DriverCommand::Abandon { id }).await;
                Err(CallError::Timeout {
                    method: method.to_string(),
                    after: timeout,
                })
            }
        }
    }

    /// 握手：`initialize` + 版本闸（§4.2.2 / §4.2.3）。
    ///
    /// 对端报的协议版本不一致时**当场拒绝** —— 否则会一路跑到某个字段对不上才炸，
    /// 那时候已经很难判断是谁的错。
    pub async fn initialize(
        &self,
        host_name: &str,
        host_version: &str,
    ) -> Result<CallOutcome, CallError> {
        let outcome = self
            .call(
                "initialize",
                json!({
                    "protocol": PROTOCOL_VERSION,
                    "host": { "name": host_name, "version": host_version },
                }),
                Duration::from_secs(15),
            )
            .await?;

        let peer = outcome
            .result
            .get("protocol")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        if let Err((ours, theirs)) = check_protocol_version(peer) {
            return Err(CallError::Rpc {
                code: RpcErrorCode::ProtocolVersionMismatch,
                message: format!("协议版本不匹配：宿主 {ours}，插件 {theirs}"),
                data: None,
            });
        }
        Ok(outcome)
    }

    /// 通知对端退出（写一帧 `shutdown` 命令后收摊）。
    ///
    /// 只发命令、**不等 driver 任务结束**：真正的回收是异步的，在飞调用会立刻拿到
    /// [`CallError::Closed`]。宿主侧真正靠得住的是「丢掉写侧 → 对端 stdin 见 EOF → 它自己退」
    /// 那条约定（`dev-plan` §4.2.1），进程的等待与强杀属于 `manager` 的职责。
    pub async fn shutdown(&self) {
        let _ = self.cmd_tx.send(DriverCommand::Shutdown).await;
    }
}

type Pending = HashMap<u64, oneshot::Sender<Result<CallOutcome, CallError>>>;

async fn driver_loop<W>(
    writer: W,
    mut frame_rx: mpsc::UnboundedReceiver<Result<Frame, FrameIoError>>,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    event_tx: mpsc::UnboundedSender<ConnEvent>,
) where
    W: AsyncWrite + Unpin,
{
    let mut writer = writer;
    let mut router = Router::new();
    let mut pending: Pending = HashMap::new();

    loop {
        tokio::select! {
            // ① 调用方要发东西
            cmd = cmd_rx.recv() => match cmd {
                Some(DriverCommand::Call { id, frame, reply }) => {
                    router.register(id);
                    pending.insert(id, reply);
                    if let Err(e) = write_frame(&mut writer, &frame).await {
                        let reason = format!("写帧失败：{e}");
                        if let Some(reply) = pending.remove(&id) {
                            let _ = reply.send(Err(CallError::Frame { detail: reason.clone() }));
                        }
                        close(&mut pending, &event_tx, reason, CallError::Closed);
                        return;
                    }
                }
                Some(DriverCommand::Abandon { id }) => {
                    // 超时/取消：两边都摘掉，之后迟到的响应由 Router 报成 Issue
                    pending.remove(&id);
                    router.abandon(id);
                }
                Some(DriverCommand::Shutdown) | None => {
                    close(&mut pending, &event_tx, "本地主动关闭".to_string(), CallError::Closed);
                    return;
                }
            },

            // ② 读侧送来一帧
            incoming = frame_rx.recv() => match incoming {
                Some(Ok(frame)) => {
                    if !dispatch(router.on_frame(frame), &mut pending, &event_tx) {
                        let reason = "对端断开".to_string();
                        close(&mut pending, &event_tx, reason, CallError::Closed);
                        return;
                    }
                }
                Some(Err(e)) => {
                    let reason = e.to_string();
                    // 先让 Router 把在飞的（含扣着附件的）逐个交还
                    dispatch(router.on_disconnected(&reason), &mut pending, &event_tx);
                    close(&mut pending, &event_tx, reason, CallError::Closed);
                    return;
                }
                None => {
                    let reason = "读侧结束".to_string();
                    dispatch(router.on_disconnected(&reason), &mut pending, &event_tx);
                    close(&mut pending, &event_tx, reason, CallError::Closed);
                    return;
                }
            },
        }
    }
}

/// 分发路由事件；返回 `false` 表示连接已结束（调用方应收摊）。
fn dispatch(
    events: Vec<RouterEvent>,
    pending: &mut Pending,
    event_tx: &mpsc::UnboundedSender<ConnEvent>,
) -> bool {
    let mut alive = true;
    for event in events {
        match event {
            RouterEvent::Response {
                id,
                result,
                attachments,
            } => {
                // 表里没有 = 超时已放弃：那条已经返回给调用方了，这里不重复报
                if let Some(reply) = pending.remove(&id) {
                    let _ = reply.send(Ok(CallOutcome {
                        result,
                        attachments,
                    }));
                }
            }
            RouterEvent::Error {
                id,
                code,
                message,
                data,
            } => {
                if let Some(reply) = pending.remove(&id) {
                    let _ = reply.send(Err(CallError::Rpc {
                        code,
                        message,
                        data,
                    }));
                }
            }
            RouterEvent::UnknownError {
                id,
                raw_code,
                message,
            } => {
                if let Some(reply) = pending.remove(&id) {
                    let _ = reply.send(Err(CallError::RpcUnknownCode { raw_code, message }));
                }
            }
            RouterEvent::Notification { method, params } => {
                let _ = event_tx.send(ConnEvent::Notification { method, params });
            }
            RouterEvent::Issue { detail } => {
                let _ = event_tx.send(ConnEvent::Issue { detail });
            }
            RouterEvent::Disconnected {
                reason,
                abandoned_ids,
            } => {
                let _ = event_tx.send(ConnEvent::Disconnected {
                    reason,
                    abandoned_ids,
                });
                alive = false;
            }
        }
    }
    alive
}

/// 收摊：把还在等的调用逐个失败掉（**一个都不能留下**，否则调用方永远挂在那儿）。
fn close(
    pending: &mut Pending,
    event_tx: &mpsc::UnboundedSender<ConnEvent>,
    reason: String,
    error: CallError,
) {
    for (_, reply) in pending.drain() {
        let _ = reply.send(Err(error.clone()));
    }
    let _ = event_tx.send(ConnEvent::Disconnected {
        reason,
        abandoned_ids: Vec::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::proto::FrameKind;
    use crate::sidecar::router::encode_response_with_arrow;
    use tokio::io::{AsyncRead, AsyncWrite};

    fn reply_frame(id: u64, result: Value) -> Frame {
        Frame::json(
            serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": id, "result": result })).unwrap(),
        )
    }

    fn error_frame(id: u64, code: i32, message: &str) -> Frame {
        Frame::json(
            serde_json::to_vec(&json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": code, "message": message }
            }))
            .unwrap(),
        )
    }

    /// 起一条连接 + 一个"对端"（用闭包处理每个请求帧）。
    fn harness<F>(on_request: F) -> (SidecarConn, ConnEvents, JoinHandle<()>)
    where
        F: Fn(Value, u64) -> Vec<Frame> + Send + 'static,
    {
        let (host, peer) = tokio::io::duplex(1 << 20);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, events) = SidecarConn::spawn(host_r, host_w);

        let (mut peer_r, mut peer_w) = tokio::io::split(peer);
        let side = tokio::spawn(async move {
            while let Ok(frame) = read_frame(&mut peer_r).await {
                if frame.kind != FrameKind::Json {
                    break;
                }
                let Ok(msg) = serde_json::from_slice::<Value>(&frame.payload) else {
                    break;
                };
                let id = msg.get("id").and_then(Value::as_u64).unwrap_or(0);
                for out in on_request(msg, id) {
                    if write_frame(&mut peer_w, &out).await.is_err() {
                        return;
                    }
                }
            }
        });
        (conn, events, side)
    }

    fn method_of(msg: &Value) -> String {
        msg.get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    #[tokio::test]
    async fn call_returns_the_result() {
        let (conn, _events, _side) = harness(|msg, id| {
            assert_eq!(method_of(&msg), "ping");
            vec![reply_frame(id, json!({ "pong": true }))]
        });

        let out = conn
            .call("ping", json!({}), Duration::from_secs(5))
            .await
            .expect("调用应成功");
        assert_eq!(out.result["pong"], true);
        assert!(out.attachments.is_empty());
    }

    /// 并发调用：**按 id 对应**，不是按顺序对应（对端故意反着回）。
    #[tokio::test]
    async fn concurrent_calls_are_correlated_by_id() {
        let (host, peer) = tokio::io::duplex(1 << 20);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, _events) = SidecarConn::spawn(host_r, host_w);
        let (mut pr, mut pw) = tokio::io::split(peer);

        // 对端：收满两个请求，然后**反着回**
        tokio::spawn(async move {
            let mut seen = Vec::new();
            while seen.len() < 2 {
                let Ok(frame) = read_frame(&mut pr).await else {
                    return;
                };
                let msg: Value = serde_json::from_slice(&frame.payload).unwrap();
                let id = msg.get("id").and_then(Value::as_u64).unwrap_or(0);
                seen.push((id, method_of(&msg)));
            }
            seen.reverse();
            for (id, method) in seen {
                if write_frame(&mut pw, &reply_frame(id, json!({ "method": method })))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });

        let (a, b) = tokio::join!(
            conn.call("first", json!({}), Duration::from_secs(5)),
            conn.call("second", json!({}), Duration::from_secs(5)),
        );
        assert_eq!(a.expect("first").result["method"], "first");
        assert_eq!(b.expect("second").result["method"], "second");
    }

    /// 附件要跟着响应一起交到调用方手里（协议层与路由层的对接点）。
    #[tokio::test]
    async fn attachments_reach_the_caller() {
        let stream: Vec<u8> = (0..300u32).map(|i| (i % 97) as u8).collect();
        let expected = stream.clone();

        let (conn, _events, _side) = harness(move |_msg, id| {
            encode_response_with_arrow(
                &json!({ "jsonrpc": "2.0", "id": id, "result": { "row_count": 300 } }),
                &stream,
                64, // 故意切得很碎：5 个帧以上
            )
            .expect("切帧")
        });

        let out = conn
            .call("query.execute", json!({}), Duration::from_secs(5))
            .await
            .expect("调用应成功");
        assert_eq!(out.attachments.len(), 1);
        assert_eq!(
            out.attachments[0].payload, expected,
            "拼回的 Arrow 流应逐字节一致"
        );
    }

    /// 超时：返回 Timeout，并把该请求放弃；迟到的响应变成 `Issue`。
    #[tokio::test]
    async fn timeout_abandons_the_request_and_late_response_becomes_an_issue() {
        let (host, peer) = tokio::io::duplex(1 << 20);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, mut events) = SidecarConn::spawn(host_r, host_w);
        let (mut pr, mut pw) = tokio::io::split(peer);

        // 对端：把响应拖到调用方超时之后（用异步 sleep；测试是单线程运行时，
        // 阻塞式 sleep 会把定时器一起卡住）
        tokio::spawn(async move {
            let Ok(frame) = read_frame(&mut pr).await else {
                return;
            };
            let msg: Value = serde_json::from_slice(&frame.payload).unwrap();
            let id = msg.get("id").and_then(Value::as_u64).unwrap_or(0);
            tokio::time::sleep(Duration::from_millis(120)).await;
            let _ = write_frame(&mut pw, &reply_frame(id, json!({ "late": true }))).await;
        });

        let err = conn
            .call("query.execute", json!({}), Duration::from_millis(30))
            .await
            .unwrap_err();
        assert!(matches!(err, CallError::Timeout { .. }), "{err:?}");

        // 迟到的那条要能被看见（不能静默吞）
        let mut saw_issue = false;
        for _ in 0..20 {
            if let Some(ConnEvent::Issue { detail }) = events.recv().await {
                assert!(detail.contains("未登记"), "{detail}");
                saw_issue = true;
                break;
            }
        }
        assert!(saw_issue, "迟到的响应应报成 Issue");
    }

    #[tokio::test]
    async fn notifications_are_forwarded_as_events() {
        let (conn, mut events, _side) = harness(|_msg, id| {
            vec![
                Frame::json(
                    serde_json::to_vec(&json!({
                        "jsonrpc": "2.0", "method": "progress",
                        "params": { "phase": "fetch", "done": 1, "total": 2 }
                    }))
                    .unwrap(),
                ),
                reply_frame(id, json!({ "ok": true })),
            ]
        });

        conn.call("query.execute", json!({}), Duration::from_secs(5))
            .await
            .expect("调用应成功");

        let event = events.recv().await.expect("应有事件");
        match event {
            ConnEvent::Notification { method, params } => {
                assert_eq!(method, "progress");
                assert_eq!(params["done"], 1);
            }
            other => panic!("期望 Notification，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn rpc_errors_carry_typed_codes() {
        let (conn, _events, _side) = harness(|_msg, id| vec![error_frame(id, -32004, "cancelled")]);

        let err = conn
            .call("query.execute", json!({}), Duration::from_secs(5))
            .await
            .unwrap_err();
        match err {
            CallError::Rpc { code, message, .. } => {
                assert_eq!(code, RpcErrorCode::Cancelled);
                assert_eq!(message, "cancelled");
            }
            other => panic!("期望 Rpc，得到 {other:?}"),
        }
    }

    /// 对端在读请求的过程中退出：在飞调用必须立刻失败（不能挂到超时）。
    #[tokio::test]
    async fn peer_exit_fails_in_flight_calls() {
        let (host, peer) = tokio::io::duplex(4096);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, mut events) = SidecarConn::spawn(host_r, host_w);

        // 对端读一帧就退出（drop 掉读写两半）
        let (mut peer_r, peer_w) = tokio::io::split(peer);
        tokio::spawn(async move {
            let _ = read_frame(&mut peer_r).await;
            drop(peer_w);
            drop(peer_r);
        });

        let err = conn
            .call("ping", json!({}), Duration::from_secs(10))
            .await
            .unwrap_err();
        assert!(
            matches!(err, CallError::Disconnected { .. } | CallError::Closed),
            "对端退出应立刻失败，而不是等到超时：{err:?}"
        );

        let event = events.recv().await.expect("应有断线事件");
        assert!(matches!(event, ConnEvent::Disconnected { .. }), "{event:?}");
    }

    /// `initialize` 的版本闸：对端报别的版本要在握手期就拒绝，并说清两边版本号。
    #[tokio::test]
    async fn initialize_rejects_a_protocol_version_mismatch() {
        let (conn, _events, _side) = harness(|_msg, id| {
            vec![reply_frame(
                id,
                json!({ "protocol": PROTOCOL_VERSION + 1, "driver_ids": [] }),
            )]
        });

        let err = conn.initialize("RdataStation", "0.1.0").await.unwrap_err();
        match err {
            CallError::Rpc { code, message, .. } => {
                assert_eq!(code, RpcErrorCode::ProtocolVersionMismatch);
                assert!(
                    message.contains(&(PROTOCOL_VERSION + 1).to_string()),
                    "{message}"
                );
            }
            other => panic!("期望版本不匹配错误，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn initialize_accepts_a_matching_version() {
        let (conn, _events, _side) = harness(|msg, id| {
            assert_eq!(method_of(&msg), "initialize");
            vec![reply_frame(
                id,
                json!({ "protocol": PROTOCOL_VERSION, "driver_ids": ["oracle-jdbc"] }),
            )]
        });

        let out = conn
            .initialize("RdataStation", "0.1.0")
            .await
            .expect("同版本应通过");
        assert_eq!(out.result["driver_ids"][0], "oracle-jdbc");
    }

    /// 主动关闭：在飞调用拿到 `Closed`，事件里也有一条断线。
    #[tokio::test]
    async fn shutdown_fails_in_flight_calls() {
        // 对端故意不回，好让调用一直挂着
        let (conn, mut events, _side) = harness(|_msg, _id| Vec::new());
        let conn = std::sync::Arc::new(conn);

        let calling = {
            let conn = conn.clone();
            tokio::spawn(async move {
                conn.call("query.execute", json!({}), Duration::from_secs(30))
                    .await
            })
        };

        // 等请求真的发出去了再关
        tokio::time::sleep(Duration::from_millis(40)).await;
        conn.shutdown().await;

        let err = calling.await.expect("任务应结束").unwrap_err();
        assert!(
            matches!(err, CallError::Closed | CallError::Disconnected { .. }),
            "{err:?}"
        );

        let mut saw_disconnect = false;
        for _ in 0..20 {
            match events.recv().await {
                Some(ConnEvent::Disconnected { .. }) => {
                    saw_disconnect = true;
                    break;
                }
                Some(_) => continue,
                None => break,
            }
        }
        assert!(saw_disconnect, "关闭时应有断线事件");
    }
}
