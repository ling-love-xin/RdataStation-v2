//! LSP 传输层：`Content-Length` 头帧编解码（纯函数，可离线断言）
//!
//! ## 与插件分帧的区别（别混）
//!
//! - 插件 D5 分帧（`crates/plugin`）：4B 大端整帧长 + 1B 种类 + 内容，面向数据库驱动。
//! - **LSP 分帧**：HTTP 风格头 + 空行 + JSON 正文——
//!   `Content-Length: <字节数>\r\n\r\n<JSON>`。面向语言服务，是行业标准。
//!
//! 本模块只做**字节↔消息**这一层：不管 JSON-RPC 语义（那是 `client` 的事）。
//!
//! ## 边界与纪律
//!
//! - 编解码是**纯函数**：输入字节，输出消息/剩余缓冲，不碰进程、不碰 I/O。
//! - 不猜测长度：头里声明多少就等多少；头畸形（缺 `Content-Length`、非数字、
//!   负数）直接报错，**不试图修复**——LSP server 是外部程序，协议畸形宁可报错。
//! - 一次可能读回**多条**消息（server 一口气发了若干条），也允许**半条**（还没凑够）；
//!   两种都由调用方按"剩余缓冲"继续喂。

use std::fmt;

/// 一条 LSP 消息 = 头之后那段 JSON 正文（**字节**，未解析；解析归 `client`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspMessage {
    /// 头里声明的正文长度（字节）
    pub content_length: usize,
    /// 正文（JSON，未解析）
    pub body: Vec<u8>,
}

/// 帧解析错误（对端协议畸形时给可读原因；`body` 过长也在此列，避免无界增长）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// 头里没有 `Content-Length`
    MissingHeader,
    /// `Content-Length` 不是合法的非负十进制整数
    InvalidLength(String),
    /// 头声明长度超出上限（防恶意/损坏的巨型帧）
    LengthTooLarge { declared: usize, limit: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::MissingHeader => write!(f, "LSP 头缺少 Content-Length"),
            FrameError::InvalidLength(raw) => {
                write!(f, "Content-Length 不是合法数字：{raw:?}")
            }
            FrameError::LengthTooLarge { declared, limit } => {
                write!(f, "Content-Length {declared} 超过上限 {limit}")
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// 单条正文的字节数上限（LSP 消息是 JSON；防御性上限，正常诊断/补全远小于此）
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

/// 把一条正文编码成完整帧（`Content-Length: N\r\n\r\n{body}`）
pub fn encode(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + body.len());
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out
}

/// 从缓冲里解析**尽可能多**的完整帧；返回（帧列表，剩余缓冲）。
///
/// - 缓冲不足一个头 → `Ok(([], buf))`（还没凑够，继续等）
/// - 头完整但正文没到齐 → `Ok(([], buf))`（同样继续等）
/// - 头畸形 / 长度越界 → `Err(FrameError)`
/// - 恰好/超过一帧 → 取出一帧或多帧，剩余留在 `buf`
///
/// `buf` 会被就地消费（取剩余部分），调用方应把返回值里的剩余缓冲存回去。
pub fn parse(mut buf: &[u8], limit: usize) -> Result<(Vec<LspMessage>, &[u8]), FrameError> {
    let mut messages = Vec::new();
    loop {
        // 找头结束（\r\n\r\n）
        let header_end = match find_header_end(buf) {
            Some(pos) => pos,
            None => return Ok((messages, buf)),
        };
        let header = &buf[..header_end];
        let content_length = parse_length(header)?;
        if content_length > limit {
            return Err(FrameError::LengthTooLarge {
                declared: content_length,
                limit,
            });
        }
        // 正文起始 = 头结束 + 4（\r\n\r\n）
        let body_start = header_end + 4;
        let body_end = body_start + content_length;
        if body_end > buf.len() {
            // 正文没到齐：整条留下，等更多字节
            return Ok((messages, buf));
        }
        let body = buf[body_start..body_end].to_vec();
        messages.push(LspMessage {
            content_length,
            body,
        });
        buf = &buf[body_end..];
    }
}

/// 找 `\r\n\r\n` 的位置（返回它**起始**处的下标）
fn find_header_end(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// 从头文本里解析 `Content-Length: N`
fn parse_length(header: &[u8]) -> Result<usize, FrameError> {
    let text = String::from_utf8_lossy(header);
    let value = text
        .lines()
        .find_map(|line| {
            let line = line.trim();
            let lower = line.to_ascii_lowercase();
            lower.strip_prefix("content-length:")?.trim().to_owned().into()
        })
        .ok_or(FrameError::MissingHeader)?;
    // 只认十进制非负整数；`12abc` / `-1` / 空串一律拒绝
    let digits_ok = !value.is_empty()
        && value.chars().all(|c| c.is_ascii_digit())
        && !(value.len() > 1 && value.starts_with('0'));
    if !digits_ok {
        return Err(FrameError::InvalidLength(value));
    }
    value
        .parse::<usize>()
        .map_err(|_| FrameError::InvalidLength(value))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{FrameError, LspMessage, MAX_BODY_BYTES, encode, parse};

    fn msg(id: &str) -> (Vec<u8>, LspMessage) {
        let body = format!(r#"{{"jsonrpc":"2.0","id":{id}}}"#).into_bytes();
        let body_len = body.len();
        (encode(&body), LspMessage { content_length: body_len, body })
    }

    #[test]
    fn encode_produces_content_length_header() {
        let body = br#"{"jsonrpc":"2.0","method":"initialize"}"#;
        let frame = encode(body);
        let head = String::from_utf8_lossy(&frame[..32.min(frame.len())]).to_string();
        assert!(head.starts_with("Content-Length: "), "{head:?}");
        assert!(frame.ends_with(body), "正文要原样跟在头后面");
    }

    #[test]
    fn a_complete_frame_round_trips() {
        let (frame, expected) = msg("1");
        let (messages, rest) = parse(&frame, MAX_BODY_BYTES).expect("完整帧应解析成功");
        assert_eq!(messages, vec![expected]);
        assert!(rest.is_empty(), "恰好一帧时剩余为空");
    }

    #[test]
    fn multiple_frames_in_one_buffer_are_all_extracted() {
        let (f1, m1) = msg("1");
        let (f2, m2) = msg("2");
        let mut joined = f1.clone();
        joined.extend_from_slice(&f2);
        let (messages, rest) = parse(&joined, MAX_BODY_BYTES).expect("两条帧应解析成功");
        assert_eq!(messages, vec![m1, m2]);
        assert!(rest.is_empty());
    }

    #[test]
    fn a_half_frame_returns_nothing_and_keeps_the_buffer() {
        let (frame, _) = msg("1");
        let half = &frame[..frame.len() - 3]; // 截掉正文尾部 → 正文没到齐
        let (messages, rest) = parse(half, MAX_BODY_BYTES).expect("半条帧不是错误");
        assert!(messages.is_empty());
        assert_eq!(rest, half, "没凑够就原样留下");
    }

    #[test]
    fn feed_more_bytes_until_complete() {
        let (frame, expected) = msg("7");
        // 先给前 10 字节，再给剩余：两次加起来才是一条完整帧
        let (m1, rest1) = parse(&frame[..10], MAX_BODY_BYTES).expect("不足一帧不是错误");
        assert!(m1.is_empty());
        let mut rest = rest1.to_vec();
        rest.extend_from_slice(&frame[10..]);
        let (messages, rest2) = parse(&rest, MAX_BODY_BYTES).expect("补足后应解析出完整帧");
        assert_eq!(messages, vec![expected]);
        assert!(rest2.is_empty());
    }

    #[test]
    fn header_without_content_length_is_rejected() {
        let bogus = b"X-Ignored: 1\r\n\r\n{}";
        match parse(bogus, MAX_BODY_BYTES) {
            Err(FrameError::MissingHeader) => {}
            other => panic!("应当报 MissingHeader，得到 {other:?}"),
        }
    }

    #[test]
    fn non_numeric_or_negative_length_is_rejected() {
        for raw in ["abc", "-1", "", "12abc"] {
            let header = format!("Content-Length: {raw}\r\n\r\n{{}}").into_bytes();
            match parse(&header, MAX_BODY_BYTES) {
                Err(FrameError::InvalidLength(_)) => {}
                other => panic!("{raw:?} 应当报 InvalidLength，得到 {other:?}"),
            }
        }
    }

    #[test]
    fn oversized_length_is_rejected_without_allocation() {
        let header = format!("Content-Length: 99999999999\r\n\r\n").into_bytes();
        match parse(&header, 1024) {
            Err(FrameError::LengthTooLarge { .. }) => {}
            other => panic!("超限长度应当被拒，得到 {other:?}"),
        }
    }

    #[test]
    fn trailing_bytes_after_a_frame_are_kept_for_the_next_parse() {
        let (f1, _) = msg("1");
        let (f2, _) = msg("2");
        // 第一条完整 + 第二条的**一半**
        let mut joined = f1.clone();
        joined.extend_from_slice(&f2[..f2.len() - 2]);
        let (messages, rest) = parse(&joined, MAX_BODY_BYTES).expect("首帧完整 + 尾帧半条");
        assert_eq!(messages.len(), 1, "只有第一条完整");
        assert!(!rest.is_empty(), "半条尾巴要留下");
    }
}
