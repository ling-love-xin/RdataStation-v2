//! 附件（attachment）语义：**「响应 JSON 先到，Arrow 数据随后分片到」这条规则的唯一实现**。
//!
//! 设计出处：`docs/architecture/plugin/plugin-dev-plan.md` §4.2.1。
//!
//! ```text
//! [JSON 响应]  {"jsonrpc":"2.0","id":7,"result":{…
//!                  "attachments":[{"id":0,"kind":"arrow-ipc-stream","frames":2}]}}
//! [0x02 帧]    前端 4 MiB 的 Arrow IPC 字节
//! [0x02 帧]    后端 3 MiB（frames 数 = 该附件占几个帧）
//! ```
//!
//! ## 三条要点（都是"不写清就会踩"的）
//!
//! 1. **响应在附件收齐之前不算完成**：`Router` 会把响应扣住，直到它声明的那几个 `0x02`
//!    帧全部到齐再交付。否则上层会拿到一个"行数写着 1000、数据还没到"的半成品。
//! 2. **附件帧是同一段 Arrow IPC 流的连续分片**，所以按顺序拼回一个 `Vec<u8>` 才是完整流；
//!    它不是"一帧一行"或"一帧一个批次"。
//! 3. **任何错位都要如实报**：附件帧到了但没人声明要它、声明着附件却先来了 JSON 帧、
//!    未知的 `kind`、未知的错误码 —— 一律产出 `Issue`/`UnknownError`，**不 panic、也不静默丢**。
//!
//! ## 两个方向同一条规则
//!
//! * 宿主侧：`Router`（消费帧 → 事件）
//! * sidecar 侧：`encode_response_with_arrow`（把响应 + 一段 Arrow 流切成分帧）
//!
//! 两者互为逆运算，`split_then_route_reassembles_the_stream` 这条测试就是把它们对起来。

use serde_json::{Map, Value};

use super::proto::{FRAME_HEADER_LEN, MAX_FRAME_LEN, ProtocolError, RpcErrorCode};
use super::proto::{Frame, FrameKind};

/// 附件种类：目前只有一种（Arrow IPC stream）。
pub const KIND_ARROW_IPC_STREAM: &str = "arrow-ipc-stream";

/// 一个已收齐的附件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// 附件序号（`result.attachments[i].id`，用于响应里回指）。
    pub id: i64,
    /// 种类；目前只会是 [`KIND_ARROW_IPC_STREAM`]。
    pub kind: String,
    /// 拼好的完整载荷（多个 `0x02` 帧按顺序拼回，即完整 Arrow IPC 流）。
    pub payload: Vec<u8>,
}

/// 路由产出的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterEvent {
    /// 一个**收齐了附件**的响应。
    Response {
        id: u64,
        result: Value,
        attachments: Vec<Attachment>,
    },
    /// 错误响应（错误码已识别）。
    Error {
        id: u64,
        code: RpcErrorCode,
        message: String,
        data: Option<Value>,
    },
    /// 错误响应的码我们不认识（对端比我们新）—— 如实转交，不猜。
    UnknownError {
        id: u64,
        raw_code: i32,
        message: String,
    },
    /// 通知（无 `id`）：`log` / `progress` 等插件 → 宿主方向的消息。
    Notification { method: String, params: Value },
    /// 连接断了：把在飞请求一次性交还给调用方（逐个失败，不静默丢弃）。
    Disconnected {
        reason: String,
        abandoned_ids: Vec<u64>,
    },
    /// 流层面的不对（错位 / 未知种类 / 不认识的字段）—— 不 panic，但要能排查。
    Issue { detail: String },
}

/// 正在等待的附件声明。
#[derive(Debug, Clone)]
struct PendingAttachments {
    id: u64,
    result: Value,
    /// 逐项 `(id, kind, 还差几个帧)`；收满后转成 [`Attachment`]。
    specs: Vec<(i64, String, usize, Vec<u8>)>,
    /// 当前收到第几项。
    cursor: usize,
}

/// 附着在帧流上的状态机（**不碰 I/O**，所以能把每条错位路径都测到）。
#[derive(Debug, Default)]
pub struct Router {
    /// 已发出、还没收到响应的请求 id。
    in_flight: std::collections::BTreeSet<u64>,
    /// 扣住的响应（附件没收齐）。
    pending: Option<PendingAttachments>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个已发出的请求 id。
    ///
    /// 登记过的 id 才认作"我们的响应"；没登记的响应会变成 `Issue`（典型场景是
    /// 超时之后姗姗来迟的响应 —— 那时候调用方已经放弃了，不该再当正常响应交付）。
    pub fn register(&mut self, id: u64) {
        self.in_flight.insert(id);
    }

    /// 在飞请求数（诊断用）。
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// 调用方**放弃**一个在飞请求（超时 / 用户取消）。
    ///
    /// 放弃之后该 id 的迟到响应会被报成 `Issue`，而不是当正常响应交付 —— 这正是要的：
    /// 调用方已经不在了，静默吞掉会让“到底谁收到了”变得不可查。
    /// 若被放弃的正是那个扣在附件上的响应，一并丢掉（它的载荷已无意义）。
    pub fn abandon(&mut self, id: u64) {
        if self.pending.as_ref().is_some_and(|p| p.id == id) {
            self.pending = None;
        }
        self.in_flight.remove(&id);
    }

    /// 还差几个 `0x02` 帧才算把当前响应收完（0 = 没有扣住的响应）。
    pub fn awaiting_arrow_frames(&self) -> usize {
        match &self.pending {
            None => 0,
            Some(p) => p.specs[p.cursor..]
                .iter()
                .map(|(_, _, left, _)| *left)
                .sum(),
        }
    }

    /// 连接断了：把所有在飞请求交还调用方。
    pub fn on_disconnected(&mut self, reason: &str) -> Vec<RouterEvent> {
        let mut events = Vec::new();

        // 扣着的响应先单独说明（否则调用方只知道"这个 id 没了"，不知道为什么）
        if let Some(p) = self.pending.take() {
            let left: usize = p.specs[p.cursor..].iter().map(|(_, _, l, _)| *l).sum();
            events.push(RouterEvent::Issue {
                detail: format!("响应 {} 的附件还没收齐（还差 {} 帧）连接就断了", p.id, left),
            });
        }

        let abandoned: Vec<u64> = self.in_flight.iter().copied().collect();
        self.in_flight.clear();
        events.push(RouterEvent::Disconnected {
            reason: reason.to_string(),
            abandoned_ids: abandoned,
        });
        events
    }

    /// 消费一帧。
    pub fn on_frame(&mut self, frame: Frame) -> Vec<RouterEvent> {
        match frame.kind {
            FrameKind::Json => self.on_json_frame(&frame.payload),
            FrameKind::ArrowIpc => self.on_arrow_frame(frame.payload),
        }
    }

    fn on_json_frame(&mut self, payload: &[u8]) -> Vec<RouterEvent> {
        // 扣着响应没收齐时又来 JSON 帧 = 错位（附件帧被吞了 / 对端写乱了）
        if let Some(p) = &self.pending {
            return vec![RouterEvent::Issue {
                detail: format!(
                    "响应 {} 还在等 {} 个附件帧，却先收到 JSON 帧：流已错位",
                    p.id,
                    self.awaiting_arrow_frames()
                ),
            }];
        }

        let msg: Value = match serde_json::from_slice(payload) {
            Ok(v) => v,
            Err(e) => {
                return vec![RouterEvent::Issue {
                    detail: format!("JSON 帧不是合法 JSON：{e}"),
                }];
            }
        };

        let Some(obj) = msg.as_object() else {
            return vec![RouterEvent::Issue {
                detail: "JSON 帧的顶层不是对象".to_string(),
            }];
        };

        // 无 id → 通知（插件 → 宿主方向）
        let Some(id) = obj.get("id").and_then(Value::as_u64) else {
            let method = obj
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("<无 method>")
                .to_string();
            let params = obj.get("params").cloned().unwrap_or(Value::Null);
            return vec![RouterEvent::Notification { method, params }];
        };

        // 只校验"登记过"，**不在这里移除**：
        // 被附件扣住的响应，调用方还在等，它就该继续算在飞（否则断线时交还不回去）。
        if !self.in_flight.contains(&id) {
            return vec![RouterEvent::Issue {
                detail: format!("收到未登记（或已放弃）的响应 id={id}"),
            }];
        }

        if let Some(err) = obj.get("error") {
            let event = self.map_error(id, err);
            self.in_flight.remove(&id);
            return vec![event];
        }

        let result = obj.get("result").cloned().unwrap_or(Value::Null);

        // 声明了附件 → 扣住，等 0x02 帧
        match parse_attachments(&result) {
            Ok(Some(specs)) if !specs.is_empty() => {
                self.pending = Some(PendingAttachments {
                    id,
                    result,
                    specs,
                    cursor: 0,
                });
                // 全为 `frames: 0` 的声明要**立刻**交付：没有帧会到来，
                // 不在这里推进就是一个永远不会完成的响应。
                self.advance_completed().unwrap_or_default()
            }
            Ok(_) => {
                self.in_flight.remove(&id);
                vec![RouterEvent::Response {
                    id,
                    result,
                    attachments: Vec::new(),
                }]
            }
            Err(detail) => {
                self.in_flight.remove(&id);
                vec![
                    RouterEvent::Issue { detail },
                    RouterEvent::Response {
                        id,
                        result,
                        attachments: Vec::new(),
                    },
                ]
            }
        }
    }

    /// 推进「已收满」的附件项；全部收满就交付响应。
    ///
    /// 循环推进是为了 `frames: 0` 的项（空附件）不会卡住游标。
    fn advance_completed(&mut self) -> Option<Vec<RouterEvent>> {
        let p = self.pending.as_mut()?;
        while p.cursor < p.specs.len() && p.specs[p.cursor].2 == 0 {
            p.cursor += 1;
        }
        if p.cursor < p.specs.len() {
            return None;
        }

        let done = self.pending.take().expect("刚取过引用");
        self.in_flight.remove(&done.id);
        let attachments = done
            .specs
            .into_iter()
            .map(|(id, kind, _, payload)| Attachment { id, kind, payload })
            .collect();
        Some(vec![RouterEvent::Response {
            id: done.id,
            result: done.result,
            attachments,
        }])
    }

    fn on_arrow_frame(&mut self, payload: Vec<u8>) -> Vec<RouterEvent> {
        let Some(p) = self.pending.as_mut() else {
            return vec![RouterEvent::Issue {
                detail: format!(
                    "收到 {} 字节的附件帧，但没有任何响应声明要它：流已错位",
                    payload.len()
                ),
            }];
        };

        // 填进当前项
        let (_, _, left, buf) = &mut p.specs[p.cursor];
        *left -= 1;
        buf.extend_from_slice(&payload);

        self.advance_completed().unwrap_or_default()
    }

    fn map_error(&self, id: u64, err: &Value) -> RouterEvent {
        let raw_code = err.get("code").and_then(Value::as_i64).unwrap_or(0) as i32;
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("<无 message>")
            .to_string();
        let data = err.get("data").cloned();

        match RpcErrorCode::from_code(raw_code) {
            Some(code) => RouterEvent::Error {
                id,
                code,
                message,
                data,
            },
            None => RouterEvent::UnknownError {
                id,
                raw_code,
                message,
            },
        }
    }
}

/// 解析 `result.attachments`，返回 `(id, kind, frames, 空缓冲)`。
///
/// `Ok(None)` = 没有 `attachments` 字段；`Err` = 有但写坏了（调用方仍应把响应交付，
/// 但必须把这个 `Issue` 报出去）。
fn parse_attachments(result: &Value) -> Result<Option<Vec<(i64, String, usize, Vec<u8>)>>, String> {
    let Some(list) = result.get("attachments") else {
        return Ok(None);
    };
    let Some(arr) = list.as_array() else {
        return Err("result.attachments 不是数组".to_string());
    };

    let mut specs = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("attachments[{i}] 缺 id"))?;
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("attachments[{i}] 缺 kind"))?
            .to_string();
        if kind != KIND_ARROW_IPC_STREAM {
            return Err(format!(
                "attachments[{i}] 的种类 {kind:?} 不认识（只支持 {KIND_ARROW_IPC_STREAM}）"
            ));
        }
        let frames = item
            .get("frames")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("attachments[{i}] 缺 frames"))?;
        let frames = frames as usize;
        if frames > MAX_FRAME_LEN / FRAME_HEADER_LEN {
            return Err(format!("attachments[{i}] 声明 {frames} 个帧，数量不合理"));
        }
        specs.push((id, kind, frames, Vec::new()));
    }
    Ok(Some(specs))
}

/// **sidecar 侧**：把「响应 JSON + 一段 Arrow IPC 流」切成帧序列。
///
/// 它会覆写 `result.attachments`（只声明一个附件），帧数按 `chunk` 切出来 —— 这样
/// "声明几个帧"与"实际发几个帧"永远一致（手写这个数字早晚会错）。
///
/// `chunk` 为单个 `0x02` 帧的载荷上限；实际取 `min(chunk, MAX_PAYLOAD)`。
/// Arrow 流为空时声明 `frames: 0`（合法：空结果集也有 schema）。
pub fn encode_response_with_arrow(
    response: &Value,
    arrow_stream: &[u8],
    chunk: usize,
) -> Result<Vec<Frame>, ProtocolError> {
    let chunk = chunk.max(1).min(MAX_FRAME_LEN - FRAME_HEADER_LEN);

    let mut resp: Map<String, Value> = response.as_object().cloned().unwrap_or_else(|| Map::new());

    let mut result: Map<String, Value> = resp
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| Map::new());

    let pieces: Vec<&[u8]> = arrow_stream.chunks(chunk).collect();
    result.insert(
        "attachments".to_string(),
        serde_json::json!([{
            "id": 0,
            "kind": KIND_ARROW_IPC_STREAM,
            "frames": pieces.len(),
        }]),
    );
    resp.insert("result".to_string(), Value::Object(result));

    let mut frames = Vec::with_capacity(pieces.len() + 1);
    let body =
        serde_json::to_vec(&Value::Object(resp)).map_err(|e| ProtocolError::Serialization {
            detail: e.to_string(),
        })?;
    frames.push(Frame::json(body));
    for piece in pieces {
        frames.push(Frame::arrow(piece.to_vec()));
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response_with_one_attachment(id: u64, frames: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "row_count": 1000,
                "has_more": true,
                "attachments": [{"id": 0, "kind": KIND_ARROW_IPC_STREAM, "frames": frames}],
            }
        })
    }

    fn json_frame(v: &Value) -> Frame {
        Frame::json(serde_json::to_vec(v).unwrap())
    }

    #[test]
    fn response_without_attachments_is_delivered_immediately() {
        let mut r = Router::new();
        r.register(1);
        let events = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "id": 1, "result": {"row_count": 3}
        })));
        assert_eq!(events.len(), 1);
        match &events[0] {
            RouterEvent::Response {
                id,
                attachments,
                result,
            } => {
                assert_eq!(*id, 1);
                assert!(attachments.is_empty());
                assert_eq!(result["row_count"], 3);
            }
            other => panic!("期望 Response，得到 {other:?}"),
        }
        assert_eq!(r.in_flight(), 0);
    }

    /// 核心语义：**附件收齐之前不交付**。
    #[test]
    fn response_is_withheld_until_its_arrow_frames_arrive() {
        let mut r = Router::new();
        r.register(7);

        let ev = r.on_frame(json_frame(&response_with_one_attachment(7, 2)));
        assert!(ev.is_empty(), "不该提前交付：{ev:?}");
        assert_eq!(r.awaiting_arrow_frames(), 2);
        assert_eq!(r.in_flight(), 1, "此刻仍在飞");

        let ev = r.on_frame(Frame::arrow(vec![1, 2, 3]));
        assert!(ev.is_empty(), "只到了 1/2 帧");
        assert_eq!(r.awaiting_arrow_frames(), 1);

        let ev = r.on_frame(Frame::arrow(vec![4, 5]));
        assert_eq!(ev.len(), 1);
        match &ev[0] {
            RouterEvent::Response {
                id,
                attachments,
                result,
            } => {
                assert_eq!(*id, 7);
                assert_eq!(result["row_count"], 1000);
                assert_eq!(attachments.len(), 1);
                // 分片按顺序拼回一条完整的流
                assert_eq!(attachments[0].payload, vec![1, 2, 3, 4, 5]);
                assert_eq!(attachments[0].kind, KIND_ARROW_IPC_STREAM);
                assert_eq!(attachments[0].id, 0);
            }
            other => panic!("期望 Response，得到 {other:?}"),
        }
        assert_eq!(r.in_flight(), 0);
        assert_eq!(r.awaiting_arrow_frames(), 0);
    }

    /// `frames: 0` 也合法（空结果集仍有 schema）。
    #[test]
    fn zero_frame_attachment_completes_at_once() {
        let mut r = Router::new();
        r.register(3);
        let ev = r.on_frame(json_frame(&response_with_one_attachment(3, 0)));
        assert_eq!(ev.len(), 1);
        match &ev[0] {
            RouterEvent::Response { attachments, .. } => {
                assert_eq!(attachments.len(), 1);
                assert!(attachments[0].payload.is_empty());
            }
            other => panic!("期望 Response，得到 {other:?}"),
        }
    }

    /// 多个附件按声明顺序分别收满。
    #[test]
    fn multiple_attachments_are_filled_in_order() {
        let mut r = Router::new();
        r.register(9);
        let ev = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "id": 9,
            "result": {"attachments": [
                {"id": 0, "kind": KIND_ARROW_IPC_STREAM, "frames": 1},
                {"id": 1, "kind": KIND_ARROW_IPC_STREAM, "frames": 2},
            ]}
        })));
        assert!(ev.is_empty());
        assert_eq!(r.awaiting_arrow_frames(), 3);

        assert!(r.on_frame(Frame::arrow(b"a".to_vec())).is_empty());
        assert!(r.on_frame(Frame::arrow(b"b".to_vec())).is_empty());
        let ev = r.on_frame(Frame::arrow(b"c".to_vec()));
        match &ev[0] {
            RouterEvent::Response { attachments, .. } => {
                assert_eq!(attachments.len(), 2);
                assert_eq!(attachments[0].payload, b"a");
                assert_eq!(attachments[1].payload, b"bc");
            }
            other => panic!("期望 Response，得到 {other:?}"),
        }
    }

    /// 附件帧到了却没人声明要它 = 流错位（不能当成"顺便丢一帧"）。
    #[test]
    fn orphan_arrow_frame_is_reported_as_issue() {
        let mut r = Router::new();
        let ev = r.on_frame(Frame::arrow(vec![0u8; 8]));
        assert!(matches!(ev[0], RouterEvent::Issue { .. }), "{ev:?}");
    }

    /// 正等附件时先来 JSON 帧 = 附件被吞了，同样要报错位。
    #[test]
    fn json_frame_while_awaiting_attachments_is_an_issue() {
        let mut r = Router::new();
        r.register(1);
        assert!(
            r.on_frame(json_frame(&response_with_one_attachment(1, 2)))
                .is_empty()
        );
        let ev = r.on_frame(json_frame(
            &json!({"jsonrpc": "2.0", "id": 2, "result": {}}),
        ));
        assert!(
            matches!(&ev[0], RouterEvent::Issue { detail } if detail.contains("错位")),
            "{ev:?}"
        );
    }

    #[test]
    fn notifications_are_routed_without_touching_pending_state() {
        let mut r = Router::new();
        let ev = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "method": "progress",
            "params": {"phase": "fetch", "done": 3, "total": 10}
        })));
        match &ev[0] {
            RouterEvent::Notification { method, params } => {
                assert_eq!(method, "progress");
                assert_eq!(params["done"], 3);
            }
            other => panic!("期望 Notification，得到 {other:?}"),
        }
    }

    /// 超时之后姗姗来迟的响应：id 已不在在飞集合里 → 报 Issue，不当正常响应交付。
    #[test]
    fn late_response_after_giving_up_is_an_issue() {
        let mut r = Router::new();
        let ev = r.on_frame(json_frame(
            &json!({"jsonrpc": "2.0", "id": 42, "result": {}}),
        ));
        assert!(
            matches!(&ev[0], RouterEvent::Issue { detail } if detail.contains("未登记")),
            "{ev:?}"
        );
    }

    #[test]
    fn known_and_unknown_error_codes_are_distinguished() {
        let mut r = Router::new();

        r.register(1);
        let ev = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "id": 1,
            "error": {"code": -32004, "message": "cancelled", "data": {"request_id": "q-1"}}
        })));
        match &ev[0] {
            RouterEvent::Error {
                code,
                message,
                data,
                ..
            } => {
                assert_eq!(*code, RpcErrorCode::Cancelled);
                assert_eq!(message, "cancelled");
                assert_eq!(data.as_ref().unwrap()["request_id"], "q-1");
            }
            other => panic!("期望 Error，得到 {other:?}"),
        }

        r.register(2);
        let ev = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "id": 2,
            "error": {"code": -39999, "message": "来自更新的对端"}
        })));
        assert!(
            matches!(
                &ev[0],
                RouterEvent::UnknownError {
                    raw_code: -39999,
                    ..
                }
            ),
            "未知错误码要如实转交而不是丢掉：{ev:?}"
        );
    }

    #[test]
    fn malformed_json_frame_is_an_issue_not_a_panic() {
        let mut r = Router::new();
        let ev = r.on_frame(Frame::json(b"{ this is not json".to_vec()));
        assert!(matches!(ev[0], RouterEvent::Issue { .. }), "{ev:?}");
    }

    /// 声明写坏了（缺 kind）：既报 Issue，也**不吞掉**响应本体。
    #[test]
    fn broken_attachment_declaration_reports_issue_but_keeps_response() {
        let mut r = Router::new();
        r.register(5);
        let ev = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "id": 5,
            "result": {"row_count": 1, "attachments": [{"id": 0, "frames": 1}]}
        })));
        assert_eq!(ev.len(), 2, "{ev:?}");
        assert!(matches!(ev[0], RouterEvent::Issue { .. }));
        assert!(matches!(ev[1], RouterEvent::Response { .. }));
        assert_eq!(r.awaiting_arrow_frames(), 0, "不该扣住一个声明坏了的响应");
    }

    /// 连接死亡：在飞请求必须逐个交还（否则调用方永远等一个不会来的响应）。
    #[test]
    fn disconnect_fails_every_in_flight_request() {
        let mut r = Router::new();
        r.register(1);
        r.register(2);
        r.register(3);

        let ev = r.on_disconnected("宿主退出");
        let last = ev.last().unwrap();
        match last {
            RouterEvent::Disconnected {
                reason,
                abandoned_ids,
            } => {
                assert_eq!(reason, "宿主退出");
                assert_eq!(abandoned_ids, &vec![1, 2, 3]);
            }
            other => panic!("期望 Disconnected，得到 {other:?}"),
        }
        assert_eq!(r.in_flight(), 0);
    }

    /// 扣着附件时断线：既报"还差几帧"，也要把那个 id 交还。
    #[test]
    fn disconnect_during_attachment_stream_reports_and_releases() {
        let mut r = Router::new();
        r.register(8);
        assert!(
            r.on_frame(json_frame(&response_with_one_attachment(8, 3)))
                .is_empty()
        );

        let ev = r.on_disconnected("sidecar 进程退出");
        assert!(
            matches!(&ev[0], RouterEvent::Issue { detail } if detail.contains("还差 3 帧")),
            "{ev:?}"
        );
        match &ev[1] {
            RouterEvent::Disconnected { abandoned_ids, .. } => assert_eq!(abandoned_ids, &vec![8]),
            other => panic!("期望 Disconnected，得到 {other:?}"),
        }
    }

    /// **两个方向对起来**：sidecar 侧切帧 → 宿主侧路由 → 载荷与原流逐字节一致。
    #[test]
    fn split_then_route_reassembles_the_stream() {
        // 一段"像 Arrow"的字节（内容无关，协议层只当不透明载荷）
        let stream: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();

        for chunk in [1usize, 7, 256, 4096] {
            let frames = encode_response_with_arrow(
                &json!({"jsonrpc": "2.0", "id": 11, "result": {"row_count": 1000}}),
                &stream,
                chunk,
            )
            .unwrap();

            // 第一帧必须是 JSON，且声明了正确帧数
            assert_eq!(frames[0].kind, FrameKind::Json);
            let declared: usize = serde_json::from_slice::<Value>(&frames[0].payload).unwrap()
                ["result"]["attachments"][0]["frames"]
                .as_u64()
                .unwrap() as usize;
            assert_eq!(declared, frames.len() - 1, "chunk={chunk}");

            let mut r = Router::new();
            r.register(11);
            let mut events = Vec::new();
            for f in frames {
                events.extend(r.on_frame(f));
            }
            assert_eq!(events.len(), 1, "chunk={chunk}：{events:?}");
            match &events[0] {
                RouterEvent::Response { attachments, .. } => {
                    assert_eq!(
                        attachments[0].payload, stream,
                        "chunk={chunk} 拼回的流必须逐字节一致"
                    );
                }
                other => panic!("期望 Response，得到 {other:?}"),
            }
        }
    }

    /// 空 Arrow 流也要能切（空结果集）：声明 `frames: 0`，路由侧立刻完成。
    #[test]
    fn empty_arrow_stream_round_trips() {
        let frames = encode_response_with_arrow(
            &json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
            &[],
            1024,
        )
        .unwrap();
        assert_eq!(frames.len(), 1);

        let mut r = Router::new();
        r.register(1);
        let events = r.on_frame(frames.into_iter().next().unwrap());
        match &events[0] {
            RouterEvent::Response { attachments, .. } => {
                assert_eq!(attachments.len(), 1);
                assert!(attachments[0].payload.is_empty());
            }
            other => panic!("期望 Response，得到 {other:?}"),
        }
    }

    /// 超时的实现路径：先 `abandon`，之后到达的响应同样变成 `Issue`。
    #[test]
    fn abandoned_request_treats_a_late_response_as_an_issue() {
        let mut r = Router::new();
        r.register(42);
        assert_eq!(r.in_flight(), 1);
        r.abandon(42);
        assert_eq!(r.in_flight(), 0, "放弃后不该继续算在飞");

        let ev = r.on_frame(json_frame(&json!({
            "jsonrpc": "2.0", "id": 42, "result": {"late": true}
        })));
        assert!(matches!(&ev[0], RouterEvent::Issue { .. }), "{ev:?}");
    }

    /// 放弃的正好是“扣在附件上”的那个响应：扣住的包袱也要一并丢掉。
    #[test]
    fn abandoning_a_withheld_response_drops_its_pending_attachments() {
        let mut r = Router::new();
        r.register(7);
        assert!(
            r.on_frame(json_frame(&response_with_one_attachment(7, 3)))
                .is_empty()
        );
        assert_eq!(r.awaiting_arrow_frames(), 3);

        r.abandon(7);
        assert_eq!(r.awaiting_arrow_frames(), 0, "扣住的响应应被丢掉");

        // 之后来的附件帧就变成孤儿帧（错位）—— 不能拿它去拼已放弃的响应
        let ev = r.on_frame(Frame::arrow(b"late".to_vec()));
        assert!(matches!(&ev[0], RouterEvent::Issue { .. }), "{ev:?}");
    }
}
