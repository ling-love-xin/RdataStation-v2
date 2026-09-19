//! 测试用 sidecar 靶子 —— **不是产品组件**（`[[bin]]` 只为 `cargo test` 存在）
//!
//! 它只实现宿主协议的**最小对端**，够用来把「起进程 → 握手 → 调用 → 收摊」这条链路
//! 真的跑在进程边界上（见 `crates/plugin/tests/spawn_real_process.rs`）。
//!
//! # 为什么手写帧而不是复用 `rds_plugin::sidecar::proto`
//!
//! 两侧共用一份帧编解码，`total_len` 的读法错得**一致**就永远测不出来。这里按
//! `dev-plan` §4.2.1 的白纸黑字重写一遍（4 字节大端整帧长 + 1 字节种类），
//! 相当于第二份独立解码器 —— 跨语言互通本来就是硬门槛（§6.1，pyarrow / arrow-go 各写各的）。
//!
//! # 开关（都由宿主侧的测试传进来）
//!
//! - `--ignore-eof`：读到 stdin EOF 也不退（用来验证「强杀兜底」这一路）
//! - `--exit-ms=<n>`：启动 n 毫秒后自行退出，退出码 3（用来验证「没调用也会发现它死了」）
//! - 请求侧开关：`session.open` 的**连接参数**给 `{"fail": true}`（即 `params.params.fail`）
//!   会回一个错误（验证「开会话失败要把会话如实撤销」；错误码表里**还没有「连接失败」
//!   这一码** —— 先用 -32003 顶上）
//!
//! # 它故意不做什么
//!
//! 不做真实驱动、不做 Arrow 附件、不解析 SQL。`query.execute` 与 3000 行 Arrow
//! 是**验收靶子**（PostgreSQL 包一层 sidecar）的活；验收清单见 `dev-plan` §5 P1。

use std::io::{self, ErrorKind, Read, Write};
use std::time::Duration;

/// 帧种类：JSON-RPC 控制面（与 `proto.rs` 的 `KIND_JSON` 同一个值）。
const KIND_JSON: u8 = 0x01;
/// 协议版本（与 `proto.rs` 的 `PROTOCOL_VERSION` 同一个值）。
const PROTOCOL_VERSION: u64 = 1;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ignore_eof = args.iter().any(|a| a == "--ignore-eof");
    if let Some(ms) = args
        .iter()
        .find_map(|a| a.strip_prefix("--exit-ms="))
        .and_then(|v| v.parse::<u64>().ok())
    {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            std::process::exit(3);
        });
    }

    eprintln!("fixture 启动 pid={}", std::process::id());

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    loop {
        match read_frame(&mut input) {
            Ok(Some(payload)) => {
                if let Some(reply) = handle(&payload) {
                    if let Err(e) = write_frame(&mut output, &reply) {
                        eprintln!("fixture: 写帧失败 {e}");
                        return;
                    }
                }
            }
            // stdin EOF = 宿主没了（dev-plan §4.2.1）：正常对端必须立刻退
            Ok(None) => {
                if ignore_eof {
                    eprintln!("fixture: 收到 EOF 但按 --ignore-eof 赖着不走");
                    loop {
                        std::thread::sleep(Duration::from_secs(3600));
                    }
                }
                eprintln!("fixture: 收到 EOF（宿主已走），退出");
                return;
            }
            Err(e) => {
                eprintln!("fixture: 读帧失败 {e}");
                return;
            }
        }
    }
}

/// 处理一个请求帧；返回 `None` = 这次不回（通知、或故意挂着）。
fn handle(payload: &[u8]) -> Option<Vec<u8>> {
    let message: serde_json::Value = match serde_json::from_slice(payload) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("fixture: 载荷不是 JSON：{e}");
            return None;
        }
    };

    let method = message
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let params = message
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let Some(id) = message.get("id").cloned() else {
        eprintln!("fixture: 收到通知 {method}，忽略");
        return None;
    };

    match method {
        // 直接死掉，不回：用来验证「在飞调用会被交还」与「拿得到退出码」
        "crash" => std::process::exit(3),
        // 永不回：用来验证宿主的超时 + 放弃（连接本身不该受影响）
        "hold" => {
            eprintln!("fixture: {method} 挂着不回（等宿主超时）");
            None
        }
        "initialize" => Some(reply(
            id,
            serde_json::json!({
                "protocol": PROTOCOL_VERSION,
                "driver_ids": ["fixture"],
                "runtime": {
                    "name": "rds-sidecar-fixture",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )),
        "ping" => Some(reply(id, serde_json::json!({ "pong": true }))),
        "echo" => Some(reply(id, params)),
        // 会话：**宿主发号，靶子回声**（口径见 dev-plan §4.2.2 附录：一个会话只维护一张 id 表）
        "session.open" => {
            // 连接参数在嵌套的那一层（`{session_id, driver_id, params}`），与真实协议同形 ——
            // 不要图省事读顶层
            if params
                .get("params")
                .and_then(|p| p.get("fail"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                Some(error(id, -32003, "fixture：连接失败（故意）"))
            } else {
                Some(reply(
                    id,
                    serde_json::json!({
                        "session_id": params.get("session_id").cloned().unwrap_or(serde_json::Value::Null),
                        "driver_id": params.get("driver_id").cloned().unwrap_or(serde_json::Value::Null),
                        "server_version": "fixture-1",
                    }),
                ))
            }
        }
        "session.close" => Some(reply(id, serde_json::json!({ "closed": true }))),
        other => Some(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("未知方法 {other}") },
            }))
            .expect("自己拼的 JSON 一定能序列化"),
        ),
    }
}

fn reply(id: serde_json::Value, result: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
    .expect("自己拼的 JSON 一定能序列化")
}

fn error(id: serde_json::Value, code: i32, message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    }))
    .expect("自己拼的 JSON 一定能序列化")
}

/// 读一帧；`Ok(None)` = 管道到头了（EOF）。
///
/// 手写这一段的用意见文件头：这是协议的第二份独立实现。
fn read_frame(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 5];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    // `total_len` **含头 5 字节**（dev-plan §4.2.1 定死的那一处）
    let total = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let kind = header[4];
    if kind != KIND_JSON {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("fixture 只认 JSON 帧，收到 kind=0x{kind:02x}"),
        ));
    }
    let payload_len = total.checked_sub(5).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("帧长 {total} 连头都装不下（total_len 是含头的整帧长度）"),
        )
    })?;

    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// 写一帧（先头后载荷）。
fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let total = (5 + payload.len()) as u32;
    writer.write_all(&total.to_be_bytes())?;
    writer.write_all(&[KIND_JSON])?;
    writer.write_all(payload)?;
    writer.flush()
}
