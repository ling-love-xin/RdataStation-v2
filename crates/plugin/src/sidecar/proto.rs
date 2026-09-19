//! sidecar 帧协议（D5：**stdio + 二进制分帧**）。
//!
//! 设计出处：`docs/architecture/plugin/plugin-dev-plan.md` §4.2。这里只做**传输层**：
//! 帧、种类字节、协议版本、内联阈值、错误码。**不定义 JSON-RPC 信封** ——
//! 信封在 `client` 里已有一份，两份定义正是参考实现踩过的坑（`Rdata-Sidecar` 的
//! `connectorapi/types.go` 与 `drivers/types.go` 逐字重复，见 dev-plan §3.5）。
//!
//! ```text
//! Frame = [u32 BE total_len][u8 kind][payload]
//!
//! total_len  = **含头 5 字节的整帧长度**（即 5 + payload.len()）
//! ```
//!
//! ## 为什么必须定死 `total_len` 的口径
//!
//! 设计文档写的是 `[u32 BE total_len][u8 kind][payload]`，但"total"含不含那 5 个字节
//! 没写。两种读法都能自洽，差 5 个字节就会把整条流解错位 —— 所以这里取**字面含义**
//! （整帧长度）并写进文档；读侧用 `total_len - 5` 得载荷长，写侧用 `5 + payload.len()` 检验。
//!
//! ## 为什么不把 Arrow 塞进 JSON
//!
//! 混在**字节流层**（本模块）而不是消息层：零额外编码、可分别 dump（JSON 帧直接可读，
//! Arrow 帧落 `.arrow` 文件就能用 `pyarrow` 打开）。base64 进 JSON 要付 +33% 体积、
//! 内存峰值双份、且不可流式（dev-plan §4.2.1.1）。

use std::fmt;

/// 帧头长度：4 字节大端长度 + 1 字节种类。
pub const FRAME_HEADER_LEN: usize = 5;

/// 单帧上限 64 MiB。
///
/// 取值参照 DBX 的 `stdio-framed`（5 字节头 / 64 MiB 上限，已在生产验证）。
/// **这个上限是防御性的**：长度前缀来自对端，不设限就是让对方用一个 `u32::MAX`
/// 让我们预分配 4 GiB。
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

/// 载荷上限（整帧上限扣掉头）。
pub const MAX_PAYLOAD_LEN: usize = MAX_FRAME_LEN - FRAME_HEADER_LEN;

/// 种类字节：JSON-RPC 2.0 消息（UTF-8）。
pub const KIND_JSON: u8 = 0x01;
/// 种类字节：Arrow IPC stream 分片（二进制）。
pub const KIND_ARROW: u8 = 0x02;

/// 协议版本。写进 `initialize` 握手与版本闸（dev-plan §4.2.3）。
///
/// **不兼容变更必须加**：对端版本不同就在握手期拒绝，而不是运行到一半才发现字段对不上。
pub const PROTOCOL_VERSION: u32 = 1;

/// 小结果走内联 JSON 的行数阈值（dev-plan §4.2.1.2）。
pub const INLINE_JSON_MAX_ROWS: usize = 200;
/// 小结果走内联 JSON 的字节阈值（dev-plan §4.2.1.2）。
pub const INLINE_JSON_MAX_BYTES: usize = 256 * 1024;

/// 帧种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameKind {
    /// JSON-RPC 控制面（方法调用 / 错误 / 进度）。
    Json,
    /// Arrow IPC 数据面（结果集分片）。
    ArrowIpc,
}

impl FrameKind {
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Json => KIND_JSON,
            Self::ArrowIpc => KIND_ARROW,
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            KIND_JSON => Some(Self::Json),
            KIND_ARROW => Some(Self::ArrowIpc),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::ArrowIpc => "arrow-ipc",
        }
    }
}

/// 帧层错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// 声明的整帧长度连头都装不下（< 5）。
    FrameTooShort { declared: u32 },
    /// 声明的整帧长度超过 [`MAX_FRAME_LEN`]。
    FrameTooLarge { declared: u32, max: usize },
    /// 未知的种类字节（对端比我们新，或流已错位）。
    UnknownKind(u8),
    /// 载荷超过 [`MAX_PAYLOAD_LEN`]（编码侧自检，不该发生）。
    PayloadTooLarge { len: usize, max: usize },
    /// 缓冲区长度与帧头声明的长度不一致（整帧解码时）。
    LengthMismatch { declared: u32, actual: usize },
    /// 构造帧时自身的 JSON 编码失败。
    ///
    /// 实践中不会发生（输入本来就是解析好的 `Value`），但**不 panic**：
    /// 一个能报错的返回面比一个可能把宿主拉下去的分支更划算。
    Serialization { detail: String },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooShort { declared } => write!(
                f,
                "帧头声明的长度 {declared} 不足一帧（至少要 {FRAME_HEADER_LEN} 字节）"
            ),
            Self::FrameTooLarge { declared, max } => {
                write!(f, "帧长 {declared} 超过上限 {max}")
            }
            Self::UnknownKind(b) => write!(f, "未知的帧种类字节 0x{b:02x}"),
            Self::PayloadTooLarge { len, max } => {
                write!(f, "载荷 {len} 字节超过上限 {max}")
            }
            Self::LengthMismatch { declared, actual } => {
                write!(f, "帧头声明 {declared} 字节，缓冲区实际 {actual} 字节")
            }
            Self::Serialization { detail } => write!(f, "帧载荷 JSON 编码失败：{detail}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// 一个帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn json(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            kind: FrameKind::Json,
            payload: payload.into(),
        }
    }

    pub fn arrow(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            kind: FrameKind::ArrowIpc,
            payload: payload.into(),
        }
    }

    /// 本帧编码后的总长度（含头）。
    pub fn total_len(&self) -> usize {
        FRAME_HEADER_LEN + self.payload.len()
    }

    /// 编码头部 5 字节。
    ///
    /// 单独给出是因为写侧应当**先写头再逐块写载荷**（大 Arrow 帧不必先拼一整个
    /// `Vec` 再写），读侧同理。
    pub fn encode_header(&self) -> Result<[u8; FRAME_HEADER_LEN], ProtocolError> {
        if self.payload.len() > MAX_PAYLOAD_LEN {
            return Err(ProtocolError::PayloadTooLarge {
                len: self.payload.len(),
                max: MAX_PAYLOAD_LEN,
            });
        }
        let total = self.total_len() as u32;
        let mut hdr = [0u8; FRAME_HEADER_LEN];
        hdr[..4].copy_from_slice(&total.to_be_bytes());
        hdr[4] = self.kind.to_byte();
        Ok(hdr)
    }

    /// 整帧编码（头 + 载荷）。
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let hdr = self.encode_header()?;
        let mut out = Vec::with_capacity(self.total_len());
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&self.payload);
        Ok(out)
    }

    /// 解帧头，返回（种类，载荷长度）。
    pub fn decode_header(hdr: [u8; FRAME_HEADER_LEN]) -> Result<(FrameKind, usize), ProtocolError> {
        let declared = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);

        if (declared as usize) < FRAME_HEADER_LEN {
            return Err(ProtocolError::FrameTooShort { declared });
        }
        if declared as usize > MAX_FRAME_LEN {
            return Err(ProtocolError::FrameTooLarge {
                declared,
                max: MAX_FRAME_LEN,
            });
        }
        let kind = FrameKind::from_byte(hdr[4]).ok_or(ProtocolError::UnknownKind(hdr[4]))?;
        Ok((kind, declared as usize - FRAME_HEADER_LEN))
    }

    /// 整帧解码（缓冲区必须**恰好**是一帧）。
    pub fn decode(buf: &[u8]) -> Result<Self, ProtocolError> {
        if buf.len() < FRAME_HEADER_LEN {
            return Err(ProtocolError::FrameTooShort {
                declared: buf.len() as u32,
            });
        }
        let hdr: [u8; FRAME_HEADER_LEN] = buf[..FRAME_HEADER_LEN].try_into().expect("长度已判");
        let (kind, payload_len) = Self::decode_header(hdr)?;
        let declared = (FRAME_HEADER_LEN + payload_len) as u32;
        if buf.len() != FRAME_HEADER_LEN + payload_len {
            return Err(ProtocolError::LengthMismatch {
                declared,
                actual: buf.len(),
            });
        }
        Ok(Self {
            kind,
            payload: buf[FRAME_HEADER_LEN..].to_vec(),
        })
    }
}

/// 增量分帧解码器：stdio 上读到的字节流不一定正好切在帧边界。
///
/// 用法：把每次读到的字节 `push` 进来，拿回本次能解出的帧（0 个或多个）。
#[derive(Debug)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    max_frame_len: usize,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            max_frame_len: MAX_FRAME_LEN,
        }
    }

    /// 更严格的本地上限（例如测试里验证越界拒绝）。
    pub fn with_max_frame_len(max_frame_len: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_frame_len,
        }
    }

    /// 缓冲区里尚未成帧的字节数（诊断用；正常空闲时应为 0）。
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// 喂入字节，返回本次解出的全部帧。
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, ProtocolError> {
        self.buf.extend_from_slice(bytes);

        let mut out = Vec::new();
        loop {
            if self.buf.len() < FRAME_HEADER_LEN {
                break;
            }
            let hdr: [u8; FRAME_HEADER_LEN] =
                self.buf[..FRAME_HEADER_LEN].try_into().expect("长度已判");
            let (kind, payload_len) = Frame::decode_header(hdr)?;
            let total = FRAME_HEADER_LEN + payload_len;

            // 本地更严的上限：越界立刻拒绝，不等把载荷收完
            if total > self.max_frame_len {
                return Err(ProtocolError::FrameTooLarge {
                    declared: total as u32,
                    max: self.max_frame_len,
                });
            }
            if self.buf.len() < total {
                break;
            }

            let payload = self.buf[FRAME_HEADER_LEN..total].to_vec();
            self.buf.drain(..total);
            out.push(Frame { kind, payload });
        }
        Ok(out)
    }
}

/// 帧 I/O 错误：底层 I/O 与协议错分开报（排查时这两种原因的处置完全不同）。
#[derive(Debug)]
pub enum FrameIoError {
    /// 管道/流本身的问题（对端关pipe、磁盘写入失败…）。
    Io(std::io::Error),
    /// 流还是好的，但字节不是合法帧（错位、版本不同、恶意长度）。
    Protocol(ProtocolError),
}

impl fmt::Display for FrameIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "帧 I/O 失败：{e}"),
            Self::Protocol(e) => write!(f, "帧协议错：{e}"),
        }
    }
}

impl std::error::Error for FrameIoError {}

impl From<std::io::Error> for FrameIoError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ProtocolError> for FrameIoError {
    fn from(e: ProtocolError) -> Self {
        Self::Protocol(e)
    }
}

impl From<FrameIoError> for super::SidecarError {
    fn from(e: FrameIoError) -> Self {
        match e {
            FrameIoError::Io(io) => Self::CommunicationError(format!("帧 I/O 失败：{io}")),
            FrameIoError::Protocol(p) => Self::CommunicationError(format!("帧协议错：{p}")),
        }
    }
}

/// 写一帧：**先写头再写载荷**。
///
/// 分两次写而不是先拼一整块 `Vec`：Arrow 分片可以很大（上限 64 MiB），
/// 拼一块就要多一份内存峰值，而 `write_all` 本来就能处理短写。
pub async fn write_frame<W>(writer: &mut W, frame: &Frame) -> Result<(), FrameIoError>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt as _;

    let header = frame.encode_header()?;
    writer.write_all(&header).await?;
    writer.write_all(&frame.payload).await?;
    writer.flush().await?;
    Ok(())
}

/// 从流上读一帧（上限取 [`MAX_FRAME_LEN`]）。
///
/// 顺序很重要：**先校验声明长度，再分配载荷缓冲**。反过来写，对端一个
/// `u32::MAX` 就能让我们先申请 4 GiB。
///
/// 对端在帧中途关管道 / 退进程 → 报 `Io(UnexpectedEof)`，调用方应把它当作
/// “sidecar 挂了”，而不是协议错。
pub async fn read_frame<R>(reader: &mut R) -> Result<Frame, FrameIoError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    read_frame_with_max(reader, MAX_FRAME_LEN).await
}

/// 同 [`read_frame`]，但用更严的本地上限（例如临时降级 / 单测）。
pub async fn read_frame_with_max<R>(
    reader: &mut R,
    max_frame_len: usize,
) -> Result<Frame, FrameIoError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt as _;

    let mut header = [0u8; FRAME_HEADER_LEN];
    reader.read_exact(&mut header).await?;
    let (kind, payload_len) = Frame::decode_header(header)?;

    let total = FRAME_HEADER_LEN + payload_len;
    if total > max_frame_len {
        return Err(ProtocolError::FrameTooLarge {
            declared: total as u32,
            max: max_frame_len,
        }
        .into());
    }

    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await?;
    Ok(Frame { kind, payload })
}

/// 小结果是否该走内联 JSON（而不是 Arrow 附件）。
///
/// 规则（dev-plan §4.2.1.2）：**两个阈值都满足**才内联；上限只影响线格式，
/// `ResultSet` 对消费方保持一致（`from_batches` / `from_json_rows`）。
pub fn should_inline_json(row_count: usize, byte_len: usize) -> bool {
    row_count <= INLINE_JSON_MAX_ROWS && byte_len <= INLINE_JSON_MAX_BYTES
}

/// 协议错误码（挂在 JSON-RPC `error.code` 上，dev-plan §4.2.2）。
///
/// ⚠️ **号段纪律**：参考实现（`Rdata-Sidecar`）按模块切段（JDBC -32000~-32019、
/// LSP -32020~-32039、Connectors -32040~-32059）。我们目前只有一个模块，
/// 但扩展宿主（JS）上线后要**另起一段**，不要挤进这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RpcErrorCode {
    DriverNotSupported,
    SessionNotFound,
    SqlError,
    Cancelled,
    Timeout,
    CapabilityDenied,
    ProtocolVersionMismatch,
    ResourceLimit,
}

impl RpcErrorCode {
    pub fn code(self) -> i32 {
        match self {
            Self::DriverNotSupported => -32001,
            Self::SessionNotFound => -32002,
            Self::SqlError => -32003,
            Self::Cancelled => -32004,
            Self::Timeout => -32005,
            Self::CapabilityDenied => -32006,
            Self::ProtocolVersionMismatch => -32007,
            Self::ResourceLimit => -32008,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::DriverNotSupported => "driver_not_supported",
            Self::SessionNotFound => "session_not_found",
            Self::SqlError => "sql_error",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::CapabilityDenied => "capability_denied",
            Self::ProtocolVersionMismatch => "protocol_version_mismatch",
            Self::ResourceLimit => "resource_limit",
        }
    }

    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            -32001 => Self::DriverNotSupported,
            -32002 => Self::SessionNotFound,
            -32003 => Self::SqlError,
            -32004 => Self::Cancelled,
            -32005 => Self::Timeout,
            -32006 => Self::CapabilityDenied,
            -32007 => Self::ProtocolVersionMismatch,
            -32008 => Self::ResourceLimit,
            _ => return None,
        })
    }
}

impl fmt::Display for RpcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name(), self.code())
    }
}

/// 握手期版本闸：版本不同就**拒绝**，并报出两个版本号（dev-plan §4.2.3 / 风险表）。
pub fn check_protocol_version(peer: u32) -> Result<(), (u32, u32)> {
    if peer == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err((PROTOCOL_VERSION, peer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 帧头口径：`total_len` = **含头 5 字节**的整帧长度。
    /// 这条断言就是把设计文档那处留白钉死 —— 差 5 字节会把整条流解错位。
    #[test]
    fn total_len_includes_the_header() {
        let f = Frame::json(b"{}");
        assert_eq!(f.payload.len(), 2);
        assert_eq!(f.total_len(), 2 + FRAME_HEADER_LEN);

        let bytes = f.encode().unwrap();
        assert_eq!(bytes.len(), FRAME_HEADER_LEN + 2);
        assert_eq!(&bytes[..4], &7u32.to_be_bytes());
        assert_eq!(bytes[4], KIND_JSON);
    }

    #[test]
    fn round_trip_both_kinds() {
        for f in [
            Frame::json(br#"{"jsonrpc":"2.0","id":1}"#.to_vec()),
            Frame::arrow([0u8, 1, 2, 3, 255].to_vec()),
        ] {
            let bytes = f.encode().unwrap();
            assert_eq!(Frame::decode(&bytes).unwrap(), f);
        }
    }

    /// stdio 上读到的字节流不会正好切在帧边界：逐字节喂也必须能解出来。
    #[test]
    fn decoder_reassembles_byte_by_byte() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&Frame::json(b"{\"id\":1}").encode().unwrap());
        stream.extend_from_slice(&Frame::arrow(b"\x00\x01\x02").encode().unwrap());

        let mut dec = FrameDecoder::new();
        let mut got = Vec::new();
        for b in &stream {
            got.extend(dec.push(&[*b]).unwrap());
        }
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].kind, FrameKind::Json);
        assert_eq!(got[0].payload, b"{\"id\":1}");
        assert_eq!(got[1].kind, FrameKind::ArrowIpc);
        assert_eq!(got[1].payload, b"\x00\x01\x02");
        assert_eq!(dec.pending(), 0, "解完后缓冲区该是空的");
    }

    /// 一次读到多帧（TCP/管道常见的合并读）要一次全解出来。
    #[test]
    fn decoder_handles_several_frames_in_one_push() {
        let mut stream = Vec::new();
        for i in 0..5u8 {
            stream.extend_from_slice(&Frame::json(vec![i; 3]).encode().unwrap());
        }
        let mut dec = FrameDecoder::new();
        let got = dec.push(&stream).unwrap();
        assert_eq!(got.len(), 5);
        assert_eq!(got[4].payload, vec![4u8; 3]);
    }

    /// 半帧不算错：载荷没到齐就该"继续等"，而不是报错或返回半帧。
    #[test]
    fn decoder_waits_for_the_whole_payload() {
        let bytes = Frame::json(b"hello world").encode().unwrap();
        let mut dec = FrameDecoder::new();
        assert!(dec.push(&bytes[..7]).unwrap().is_empty());
        assert_eq!(dec.pending(), 7);
        let got = dec.push(&bytes[7..]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].payload, b"hello world");
    }

    /// 长度前缀来自对端：不设限就是让对方用一个 u32::MAX 让我们预分配 4 GiB。
    #[test]
    fn oversized_frame_is_rejected_without_allocating() {
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&u32::MAX.to_be_bytes());
        hdr.push(KIND_JSON);

        let mut dec = FrameDecoder::new();
        let err = dec.push(&hdr).unwrap_err();
        assert_eq!(
            err,
            ProtocolError::FrameTooLarge {
                declared: u32::MAX,
                max: MAX_FRAME_LEN
            }
        );

        // 更严格的本地上限同样生效（不必等载荷）
        let mut dec = FrameDecoder::with_max_frame_len(8);
        assert!(matches!(
            dec.push(&Frame::json(vec![0u8; 10]).encode().unwrap())
                .unwrap_err(),
            ProtocolError::FrameTooLarge { max: 8, .. }
        ));
    }

    #[test]
    fn too_short_and_unknown_kind_are_distinguishable() {
        // 声明的总长连头都装不下
        let mut hdr = [0u8; FRAME_HEADER_LEN];
        hdr[..4].copy_from_slice(&3u32.to_be_bytes());
        hdr[4] = KIND_JSON;
        assert_eq!(
            Frame::decode_header(hdr),
            Err(ProtocolError::FrameTooShort { declared: 3 })
        );

        // 长度合法但种类不认识：不能当成 JSON 硬解
        let mut hdr = [0u8; FRAME_HEADER_LEN];
        hdr[..4].copy_from_slice(&5u32.to_be_bytes());
        hdr[4] = 0x7f;
        assert_eq!(
            Frame::decode_header(hdr),
            Err(ProtocolError::UnknownKind(0x7f))
        );
    }

    /// 整帧解码要求缓冲区恰好一帧 —— 多一个字节就是不匹配，不能"容错截断"。
    #[test]
    fn whole_frame_decode_requires_exact_length() {
        let bytes = Frame::json(b"abc").encode().unwrap();
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(matches!(
            Frame::decode(&extra).unwrap_err(),
            ProtocolError::LengthMismatch { .. }
        ));
    }

    /// 阈值规则：两个上限**都满足**才内联；边界值含在内。
    #[test]
    fn inline_json_threshold_is_conjunctive() {
        assert!(should_inline_json(0, 0));
        assert!(should_inline_json(
            INLINE_JSON_MAX_ROWS,
            INLINE_JSON_MAX_BYTES
        ));
        assert!(!should_inline_json(INLINE_JSON_MAX_ROWS + 1, 1), "超行数");
        assert!(
            !should_inline_json(1, INLINE_JSON_MAX_BYTES + 1),
            "超字节数（一行一个超大 blob 也要走附件）"
        );
    }

    #[test]
    fn error_codes_round_trip_and_do_not_collide() {
        let all = [
            RpcErrorCode::DriverNotSupported,
            RpcErrorCode::SessionNotFound,
            RpcErrorCode::SqlError,
            RpcErrorCode::Cancelled,
            RpcErrorCode::Timeout,
            RpcErrorCode::CapabilityDenied,
            RpcErrorCode::ProtocolVersionMismatch,
            RpcErrorCode::ResourceLimit,
        ];
        for (i, a) in all.iter().enumerate() {
            assert_eq!(RpcErrorCode::from_code(a.code()), Some(*a));
            for b in &all[i + 1..] {
                assert_ne!(a.code(), b.code(), "号段内必须唯一：{a} 与 {b}");
            }
        }
        assert_eq!(RpcErrorCode::from_code(-1), None);
    }

    /// 版本闸：不匹配要拒绝，并且两个版本号都能报出来（否则用户没法排查）。
    #[test]
    fn protocol_version_gate_reports_both_versions() {
        assert!(check_protocol_version(PROTOCOL_VERSION).is_ok());
        assert_eq!(
            check_protocol_version(PROTOCOL_VERSION + 1),
            Err((PROTOCOL_VERSION, PROTOCOL_VERSION + 1))
        );
    }

    // ==================== 流上读写（async） ====================

    /// 每次只吐 1 字节的读端：`AsyncRead` **允许**这样短读，
    /// 而 stdio / 管道上真的会出现（帧头跨两次读、载荷跨十次读都合法）。
    struct TrickleReader<R> {
        inner: R,
    }

    impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for TrickleReader<R> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let old = buf.filled().len();
            let cap_before = buf.remaining();
            let mut one = [0u8; 1];
            let mut small = tokio::io::ReadBuf::new(&mut one);
            let poll = std::pin::Pin::new(&mut self.inner).poll_read(cx, &mut small);
            if let std::task::Poll::Ready(Ok(())) = poll {
                let n = small.filled().len();
                if n == 0 {
                    return std::task::Poll::Ready(Ok(()));
                }
                buf.put_slice(&small.filled()[..n.min(cap_before)]);
            }
            let _ = old;
            poll
        }
    }

    #[tokio::test]
    async fn stream_round_trip() {
        let (mut a, mut b) = tokio::io::duplex(64);

        let outgoing = vec![
            Frame::json(br#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#.to_vec()),
            Frame::arrow(vec![0u8, 1, 2, 3, 4, 5]),
        ];
        let writer = tokio::spawn(async move {
            for f in &outgoing {
                write_frame(&mut a, f).await.expect("写帧");
            }
        });

        let first = read_frame(&mut b).await.expect("读第一帧");
        let second = read_frame(&mut b).await.expect("读第二帧");
        writer.await.expect("写侧任务");

        assert_eq!(first.kind, FrameKind::Json);
        assert!(first.payload.starts_with(b"{\"jsonrpc\""));
        assert_eq!(second.kind, FrameKind::ArrowIpc);
        assert_eq!(second.payload, vec![0, 1, 2, 3, 4, 5]);
    }

    /// 短读：把整条流切成每次 1 字节喂给 `read_frame`，两帧仍要完整解出。
    #[tokio::test]
    async fn stream_reader_survives_short_reads() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&Frame::json(b"hello").encode().unwrap());
        stream.extend_from_slice(&Frame::arrow(vec![9u8; 300]).encode().unwrap());

        let mut trickle = TrickleReader {
            inner: std::io::Cursor::new(stream),
        };
        let f1 = read_frame(&mut trickle).await.expect("第一帧");
        let f2 = read_frame(&mut trickle).await.expect("第二帧");

        assert_eq!(f1.payload, b"hello");
        assert_eq!(f2.kind, FrameKind::ArrowIpc);
        assert_eq!(f2.payload.len(), 300);
    }

    /// 对端在帧中途挂掉：报 I/O 错（不是协议错）—— 调用方据此判定“sidecar 挂了”。
    #[tokio::test]
    async fn truncated_stream_is_an_io_error() {
        let bytes = Frame::json(b"0123456789").encode().unwrap();
        let cut = bytes.len() - 3;
        let mut reader = std::io::Cursor::new(bytes[..cut].to_vec());

        let err = read_frame(&mut reader).await.unwrap_err();
        assert!(matches!(err, FrameIoError::Io(_)), "{err}");
    }

    /// 声明超大长度时**不能**先分配。两道上限分开验，因为它们报出的 `max` 不同：
    /// ① 超过协议全局上限（64 MiB）→ 报全局值；② 在全局内但超本地更严的上限 → 报本地值。
    #[tokio::test]
    async fn stream_reader_rejects_oversize_before_allocating() {
        // ① u32::MAX：全局上限先拦住（此时还没轮到本地的 1024）
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        bytes.push(KIND_JSON);
        let err = read_frame_with_max(&mut std::io::Cursor::new(bytes), 1024)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                FrameIoError::Protocol(ProtocolError::FrameTooLarge {
                    max: MAX_FRAME_LEN,
                    ..
                })
            ),
            "全局上限应先生效：{err}"
        );

        // ② 声明 2048 字节（小于全局、大于本地的 1024）：本地更严的上限生效
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2048u32.to_be_bytes());
        bytes.push(KIND_JSON);
        let err = read_frame_with_max(&mut std::io::Cursor::new(bytes), 1024)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                FrameIoError::Protocol(ProtocolError::FrameTooLarge { max: 1024, .. })
            ),
            "本地更严的上限应生效：{err}"
        );
    }
}
