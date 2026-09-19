//! 测试用 sidecar 靶子 —— **不是产品组件**（`[[bin]]` 只为 `cargo test` 存在）
//!
//! 它实现宿主协议的**最小可用对端**：握手 / 会话 / 描述 / 查询（内联 JSON 与 Arrow 附件两条路）
//! / 取消 / 崩溃 / EOF 自退。够用来把「起进程 → 握手 → 开会话 → 查询 → 收摊」整条链路
//! 真的跑在进程边界上（见 `crates/plugin/tests/` 下那三个集成测试文件）。
//!
//! # 为什么手写帧而不是复用 `rds_plugin::sidecar::proto`
//!
//! 两侧共用一份帧编解码，`total_len` 的读法错得**一致**就永远测不出来。这里按 `dev-plan`
//! §4.2.1 的白纸黑字重写一遍（4 字节大端整帧长 + 1 字节种类），相当于第二份独立解码器 ——
//! 跨语言互通本来就是硬门槛（§6.1，pyarrow / arrow-go 各写各的）。Arrow 那边同理：
//! **用 arrow crate 真的编出 IPC 流**，而不是端一坨字节冒充。
//!
//! # 开关（都由宿主侧的测试传进来）
//!
//! - `--ignore-eof`：读到 stdin EOF 也不退（验证「强杀兜底」这一路）
//! - `--exit-ms=<n>`：启动 n 毫秒后自行退出，退出码 3（验证「没调用也会发现它死了」）
//! - `session.open` 的连接参数给 `{"fail": true}` → 回 `-32009 connect_failed`
//!   （验证「开会话失败要如实撤销」；P1 时这一码还没有，先用 -32003 顶的）
//! - `--force-inline`：大结果也走内联 JSON（内联只是**线格式**，消费方不该看出区别）
//! - `--cap-na=<键>`：把某个能力报成 `false`（键取自 dev-plan §4.4）。连带把对应的方法也
//!   如实回 `-32006`：`schemas` → `meta.schemas`、`routines` → `meta.routine_source`、
//!   `cancel` → `query.cancel`；`views` / `sequences` / `triggers` 则是**不报**那类对象
//!   （驱动看不到与没有，在驱动侧本来就是同一件事）
//!
//! `sql` 是脚本化的（不连任何真库）：
//!
//! - `select rows=<n>` → 造 n 行五列结果（`id` / `name` / `amount` / `flag` / `ts`）
//! - `select hold_ms=<n>` → 慢查询：`n` 毫秒后回；期间收到 `query.cancel` 就回 `-32004`
//! - `select fail` → `-32003`（带 `sqlstate`，模拟真驱动）
//! - 其他 `select …` → 一行结果，`notices` 里带回收到的 SQL（测试用它确认 SQL 过了边界）
//!
//! `meta.*` 是一份写死的假 schema（`main` / `public` + `sales`），覆盖导航能摆的全部类别
//! （表 / 视图 / 物化视图 / 过程 / 函数 / 序列 / 触发器）+ 一个**摆不下**的 `domain` ——
//! 后者用来验证宿主「认不出就跳过并留痕」，而不是把它画成表。
//!
//! 真库（PostgreSQL）那条靶子在 `dev-plan` §5 P1 的实机验收清单里 —— 那一步要真库在场，
//! 进不了这一层自动化。

use std::collections::HashSet;
use std::io::{self, ErrorKind, Read, Stdout, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arrow::array::{
    BooleanArray, Float64Array, Int64Array, LargeStringArray, TimestampMicrosecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;
use serde_json::{Value, json};

/// 帧种类：JSON-RPC 控制面（与 `proto.rs` 的 `KIND_JSON` 同一个值）。
const KIND_JSON: u8 = 0x01;
/// 帧种类：Arrow IPC 数据面（与 `proto.rs` 的 `KIND_ARROW` 同一个值）。
const KIND_ARROW: u8 = 0x02;
/// 协议版本（与 `proto.rs` 的 `PROTOCOL_VERSION` 同一个值）。
const PROTOCOL_VERSION: u64 = 1;
/// 内联 JSON 的行数 / 字节阈值（与 `proto.rs` 同一组值，§4.2.1.2）。
const INLINE_JSON_MAX_ROWS: usize = 200;
const INLINE_JSON_MAX_BYTES: usize = 256 * 1024;
/// Arrow 附件切成多大的帧（故意远小于 64 MiB 上限：让重组真的发生）。
const ARROW_FRAME_CHUNK: usize = 32 * 1024;

/// 一个出口帧。
struct OutFrame {
    kind: u8,
    payload: Vec<u8>,
}

impl OutFrame {
    fn json(payload: Vec<u8>) -> Self {
        Self {
            kind: KIND_JSON,
            payload,
        }
    }

    fn arrow(payload: Vec<u8>) -> Self {
        Self {
            kind: KIND_ARROW,
            payload,
        }
    }
}

/// 靶子的全部状态。
///
/// 出口是**共享且上锁**的：慢查询要在后台线程里回帧，而帧不能交错写
/// （交错了整条流就错位了）—— 锁的粒度就是「一帧」。
struct Fixture {
    /// 大结果也走内联 JSON（只影响线格式；测试用它验证「承载方式对消费方不可见」）。
    force_inline: bool,
    /// `Stdout` 而不是 `StdoutLock`：锁不能跨线程共享（慢查询的回帧在后台线程里）。
    out: Arc<Mutex<Stdout>>,
    /// 被 `query.cancel` 标记过的 request id。
    cancelled: Arc<Mutex<HashSet<String>>>,
    /// 报成 `false` 的能力键（`--cap-na=` 给的）。
    cap_na: Vec<String>,
}

impl Fixture {
    fn send(&self, frames: &[OutFrame]) -> io::Result<()> {
        // 两把锁都要：我们的锁保证「帧与帧不交错」，stdout 自己的锁防同进程的其他写者
        let stdout = self.out.lock().expect("出口锁中毒");
        let mut out = stdout.lock();
        for frame in frames {
            let total = (5 + frame.payload.len()) as u32;
            out.write_all(&total.to_be_bytes())?;
            out.write_all(&[frame.kind])?;
            out.write_all(&frame.payload)?;
        }
        out.flush()
    }

    fn is_cancelled(&self, request_id: &str) -> bool {
        self.cancelled
            .lock()
            .expect("取消表锁中毒")
            .contains(request_id)
    }

    /// 这个能力报成不支持吗（测试用它验证「门控落到了真实的拒绝上」）。
    fn lacks(&self, key: &str) -> bool {
        self.cap_na.iter().any(|k| k == key)
    }
}

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

    let force_inline = args.iter().any(|a| a == "--force-inline");
    let cap_na: Vec<String> = args
        .iter()
        .filter_map(|a| a.strip_prefix("--cap-na="))
        .map(str::to_string)
        .collect();
    let fixture = Fixture {
        force_inline,
        cap_na,
        out: Arc::new(Mutex::new(io::stdout())),
        cancelled: Arc::new(Mutex::new(HashSet::new())),
    };

    let stdin = io::stdin();
    let mut input = stdin.lock();

    loop {
        match read_frame(&mut input) {
            Ok(Some((KIND_JSON, payload))) => {
                if let Some(frames) = handle(&fixture, &payload) {
                    if let Err(e) = fixture.send(&frames) {
                        eprintln!("fixture: 写帧失败 {e}");
                        return;
                    }
                }
            }
            Ok(Some((kind, _))) => {
                eprintln!("fixture: 只认 JSON 帧，收到 kind=0x{kind:02x}");
                return;
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
fn handle(fixture: &Fixture, payload: &[u8]) -> Option<Vec<OutFrame>> {
    let message: Value = match serde_json::from_slice(payload) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("fixture: 载荷不是 JSON：{e}");
            return None;
        }
    };

    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
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
        "initialize" => Some(vec![OutFrame::json(reply(
            id,
            json!({
                "protocol": PROTOCOL_VERSION,
                "driver_ids": ["fixture"],
                "runtime": {
                    "name": "rds-sidecar-fixture",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ))]),
        "ping" => Some(vec![OutFrame::json(reply(id, json!({ "pong": true })))]),
        "echo" => Some(vec![OutFrame::json(reply(id, params))]),

        // 会话：**宿主发号，靶子回声**（口径见 dev-plan §4.2.2 附录：一个会话只维护一张 id 表）
        "session.open" => {
            // 连接参数在嵌套的那一层（`{session_id, driver_id, params}`），与真实协议同形
            if nested(&params, "fail")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                // 「连接失败」有一等码了（P2 补的 -32009）：连不上与 SQL 写错必须分得开
                Some(vec![OutFrame::json(error(
                    id,
                    -32009,
                    "fixture：连接失败（故意）",
                ))])
            } else {
                Some(vec![OutFrame::json(reply(
                    id,
                    json!({
                        "session_id": params.get("session_id").cloned().unwrap_or(Value::Null),
                        "driver_id": params.get("driver_id").cloned().unwrap_or(Value::Null),
                        "server_version": "fixture-1",
                    }),
                ))])
            }
        }
        "session.close" => Some(vec![OutFrame::json(reply(id, json!({ "closed": true })))]),
        "session.ping" => Some(vec![OutFrame::json(reply(id, json!({ "alive": true })))]),

        "driver.describe" => {
            let driver_id = params
                .get("driver_id")
                .and_then(Value::as_str)
                .unwrap_or("fixture")
                .to_string();
            Some(vec![OutFrame::json(reply(
                id,
                json!({
                    "id": driver_id,
                    "display_name": "Fixture Driver",
                    "server_version": "fixture-1",
                    "capabilities": capabilities(fixture),
                    "identifier_quote": "\"",
                    "default_schema": "public",
                }),
            ))])
        }

        "meta.catalogs" => Some(vec![OutFrame::json(reply(
            id,
            json!({ "catalogs": ["main"] }),
        ))]),

        "meta.schemas" => {
            if fixture.lacks("schemas") {
                Some(vec![OutFrame::json(error(
                    id,
                    -32006,
                    "fixture：这个驱动没声明 schemas 能力",
                ))])
            } else {
                Some(vec![OutFrame::json(reply(
                    id,
                    json!({ "schemas": [
                        { "name": "public", "comment": "默认 schema" },
                        { "name": "sales" },
                    ] }),
                ))])
            }
        }

        // 一次给全、含 kind：宿主按 kind 分流到五个文件夹（口径见 meta.rs 模块文档）
        "meta.objects" => {
            let schema = params.get("schema").and_then(Value::as_str).unwrap_or("public");
            let mut objects = meta_objects(schema);
            // 没声明的能力：**不报**那一类（驱动「看不到」与「没有」是同一件事）
            for (key, kinds) in [
                ("views", &["view", "materialized_view"][..]),
                ("routines", &["procedure", "function"][..]),
                ("sequences", &["sequence"][..]),
                ("triggers", &["trigger"][..]),
            ] {
                if fixture.lacks(key) {
                    objects.retain(|o| !kinds.contains(&o["kind"].as_str().unwrap_or_default()));
                }
            }
            Some(vec![OutFrame::json(reply(id, json!({ "objects": objects })))])
        }

        "meta.object_detail" => {
            let schema = params.get("schema").and_then(Value::as_str).unwrap_or("public");
            let object = params.get("object").and_then(Value::as_str).unwrap_or_default();
            match (meta_object(schema, object), meta_columns(object)) {
                (Some(object), Some(columns)) => Some(vec![OutFrame::json(reply(
                    id,
                    json!({
                        "object": object,
                        "columns": columns,
                        "indexes": 3,
                        "row_count": 12000,
                    }),
                ))]),
                _ => Some(vec![OutFrame::json(error(
                    id,
                    -32003,
                    &format!("fixture：没有对象 {object}"),
                ))]),
            }
        }

        "meta.routine_source" => {
            if fixture.lacks("routines") {
                Some(vec![OutFrame::json(error(
                    id,
                    -32006,
                    "fixture：这个驱动没声明 routines 能力",
                ))])
            } else {
                // 形状只有两种：字符串，或 null（驱动查不到这个例程）
                match params.get("name").and_then(Value::as_str) {
                    Some("order_total") => Some(vec![OutFrame::json(reply(
                        id,
                        json!("CREATE FUNCTION order_total(orders.id) RETURNS numeric AS 'SELECT …'"),
                    ))]),
                    _ => Some(vec![OutFrame::json(reply(id, Value::Null))]),
                }
            }
        }

        "query.cancel" => {
            if fixture.lacks("cancel") {
                return Some(vec![OutFrame::json(error(
                    id,
                    -32006,
                    "fixture：这个驱动没声明 cancel 能力",
                ))]);
            }
            let request_id = params
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            eprintln!("fixture: 收到取消 request_id={request_id}");
            fixture
                .cancelled
                .lock()
                .expect("取消表锁中毒")
                .insert(request_id);
            Some(vec![OutFrame::json(reply(id, json!({ "accepted": true })))])
        }

        "query.execute" => execute(fixture, id, &params),

        other => Some(vec![OutFrame::json(error(
            id,
            -32601,
            &format!("未知方法 {other}"),
        ))]),
    }
}

/// `query.execute`：脚本化 SQL + 内联/Arrow 两条承载方式。
fn execute(fixture: &Fixture, id: Value, params: &Value) -> Option<Vec<OutFrame>> {
    let sql = params
        .get("sql")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let request_id = params
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or("req-0")
        .to_string();

    // `select fail` → SQL 层的错（带 sqlstate，模拟真驱动）
    if sql.contains("fail") {
        return Some(vec![OutFrame::json(error_with_data(
            id,
            -32003,
            "fixture：SQL 出错（故意）",
            json!({ "code": "42P01", "sqlstate": "42P01", "position": 15 }),
        ))]);
    }

    // `select hold_ms=<n>` → 慢查询：**不阻塞读循环**（否则收不到取消），到点在后台线程里回：
    // 被取消过 → -32004；没被取消 → 一行结果
    if let Some(ms) = marker_number(&sql, "hold_ms").map(|ms| ms as u64) {
        let worker = Fixture {
            force_inline: fixture.force_inline,
            out: Arc::clone(&fixture.out),
            cancelled: Arc::clone(&fixture.cancelled),
            cap_na: fixture.cap_na.clone(),
        };
        std::thread::spawn(move || {
            // 每 10ms 醒一次看有没有被取消：真驱动会把取消下到库里，靶子用轮询模拟。
            // （一次性 sleep 完再检查也能过语义，但那样「取消」其实没有效果 —— 等它跑完才回。）
            let deadline = std::time::Instant::now() + Duration::from_millis(ms);
            while std::time::Instant::now() < deadline {
                if worker.is_cancelled(&request_id) {
                    let cancelled =
                        vec![OutFrame::json(error(id, -32004, "fixture：查询已被取消"))];
                    if let Err(e) = worker.send(&cancelled) {
                        eprintln!("fixture: 取消回帧失败 {e}");
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let frames = vec![OutFrame::json(build_page(1, None).json_reply(id))];
            if let Err(e) = worker.send(&frames) {
                eprintln!("fixture: 慢查询回帧失败 {e}");
            }
        });
        return None;
    }

    // `select rows=<n>` → 造 n 行；其他 `select …` → 一行回声
    let rows = marker_number(&sql, "rows").unwrap_or(1);
    Some(build_page(rows, Some(&sql)).frames(id, fixture.force_inline))
}

fn nested<'a>(params: &'a Value, key: &str) -> Option<&'a Value> {
    params.get("params").and_then(|inner| inner.get(key))
}

fn marker_number(sql: &str, key: &str) -> Option<usize> {
    let needle = format!("{key}=");
    let at = sql.find(&needle)? + needle.len();
    let digits: String = sql[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 一页结果：五列（覆盖 §4.2.4 的兼容子集：`LargeUtf8` + `Timestamp(us, UTC)`）。
struct Page {
    rows: usize,
    sql_echo: Option<String>,
}

impl Page {
    /// 列定义（响应 `columns` 那一份）。
    fn columns(&self) -> Value {
        json!([
            { "name": "id",     "type_raw": "bigint",         "nullable": false },
            { "name": "name",   "type_raw": "varchar(64)",    "nullable": true  },
            { "name": "amount", "type_raw": "numeric(38,10)", "nullable": true  },
            { "name": "flag",   "type_raw": "boolean",        "nullable": true  },
            { "name": "ts",     "type_raw": "timestamptz",    "nullable": true  },
        ])
    }

    /// 内联 JSON 行（小结果走这条；大结果走 Arrow）。
    fn json_rows(&self) -> Vec<Value> {
        (1..=self.rows)
            .map(|i| {
                json!({
                    "id": i as i64,
                    "name": format!("row-{i}"),
                    "amount": i as f64 * 1.5,
                    "flag": i % 2 == 0,
                    // 内联 JSON 没有时间类型：用 RFC3339 字符串（口径写进 dev-plan §4.2.2 附录）
                    "ts": "2023-11-14T22:13:20.000001Z",
                })
            })
            .collect()
    }

    /// Arrow IPC 流（一整条自洽的 stream，自带 schema 与 `rds.*` metadata）。
    fn arrow_stream(&self) -> Vec<u8> {
        let schema = Arc::new(result_schema());
        let batch = self.record_batch(Arc::clone(&schema));
        let mut writer = StreamWriter::try_new(Vec::new(), &schema).expect("建 Arrow writer");
        writer.write(&batch).expect("写 Arrow 批");
        writer.finish().expect("收 Arrow 流");
        writer.into_inner().expect("取 Arrow 字节")
    }

    fn record_batch(&self, schema: Arc<Schema>) -> RecordBatch {
        let ids: Int64Array = (1..=self.rows as i64).collect();
        let names: LargeStringArray = (1..=self.rows).map(|i| Some(format!("row-{i}"))).collect();
        let amounts: Float64Array = (1..=self.rows).map(|i| Some(i as f64 * 1.5)).collect();
        let flags: BooleanArray = (1..=self.rows).map(|i| Some(i % 2 == 0)).collect();
        let ts: TimestampMicrosecondArray = (1..=self.rows as i64)
            .map(|i| Some(1_700_000_000_000_000 + i))
            .collect();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(ids),
                Arc::new(names),
                Arc::new(amounts),
                Arc::new(flags),
                Arc::new(ts.with_timezone("UTC")),
            ],
        )
        .expect("装 RecordBatch")
    }

    /// 这一页要发的帧：先 JSON 头，可能再跟 Arrow 附件帧。
    fn frames(&self, id: Value, force_inline: bool) -> Vec<OutFrame> {
        let inline_bytes = serde_json::to_vec(&self.json_rows())
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        let inline = force_inline
            || (self.rows <= INLINE_JSON_MAX_ROWS && inline_bytes <= INLINE_JSON_MAX_BYTES);

        if inline {
            return vec![OutFrame::json(self.json_reply(id))];
        }

        let stream = self.arrow_stream();
        let frames = stream.len().div_ceil(ARROW_FRAME_CHUNK);
        let mut out = vec![OutFrame::json(
            serde_json::to_vec(&self.envelope(id, Some(frames as u64)))
                .expect("自己拼的 JSON 一定能序列化"),
        )];
        for chunk in stream.chunks(ARROW_FRAME_CHUNK) {
            out.push(OutFrame::arrow(chunk.to_vec()));
        }
        out
    }

    fn json_reply(&self, id: Value) -> Vec<u8> {
        serde_json::to_vec(&self.envelope_with_rows(id)).expect("自己拼的 JSON 一定能序列化")
    }

    /// 响应信封（内联形态：带 `rows`）。
    fn envelope_with_rows(&self, id: Value) -> Value {
        let mut body = self.envelope(id, None);
        body["result"]["rows"] = Value::Array(self.json_rows());
        body
    }

    /// 响应信封（数据面另行给出：`attachments` 或 `rows`）。
    fn envelope(&self, id: Value, attachment_frames: Option<u64>) -> Value {
        let mut result = json!({
            "columns": self.columns(),
            "row_count": self.rows,
            "affected_rows": Value::Null,
            "has_more": false,
            "cursor_id": Value::Null,
            "truncated": false,
            "notices": [],
        });
        if let Some(sql) = &self.sql_echo {
            result["notices"] = json!([format!("fixture 收到 SQL：{sql}")]);
        }
        if let Some(frames) = attachment_frames {
            result["attachments"] =
                json!([{ "id": 0, "kind": "arrow-ipc-stream", "frames": frames }]);
        }
        json!({ "jsonrpc": "2.0", "id": id, "result": result })
    }
}

fn build_page(rows: usize, sql: Option<&str>) -> Page {
    Page {
        rows,
        sql_echo: sql.map(str::to_string),
    }
}

/// 结果列的 schema：`rds.*` metadata 就是类型映射的落点（dev-plan §4.5.1）。
fn result_schema() -> Schema {
    let field =
        |name: &str, data_type: DataType, type_raw: &str, canonical: &str, nullable: bool| {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("rds.type_raw".to_string(), type_raw.to_string());
            metadata.insert("rds.canonical".to_string(), canonical.to_string());
            metadata.insert("rds.nullable".to_string(), nullable.to_string());
            if canonical == "DECIMAL" {
                metadata.insert("rds.format".to_string(), "decimal(scale=10)".to_string());
            }
            if name == "id" {
                metadata.insert("rds.is_pk".to_string(), "true".to_string());
            }
            Field::new(name, data_type, nullable).with_metadata(metadata)
        };

    Schema::new(vec![
        field("id", DataType::Int64, "bigint", "BIGINT", false),
        field("name", DataType::LargeUtf8, "varchar(64)", "VARCHAR", true),
        field(
            "amount",
            DataType::Float64,
            "numeric(38,10)",
            "DECIMAL",
            true,
        ),
        field("flag", DataType::Boolean, "boolean", "BOOLEAN", true),
        field(
            "ts",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            "timestamptz",
            "TIMESTAMP",
            true,
        ),
    ])
}

fn reply(id: Value, result: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
    .expect("自己拼的 JSON 一定能序列化")
}

fn error(id: Value, code: i32, message: &str) -> Vec<u8> {
    error_with_data(id, code, message, Value::Null)
}

fn error_with_data(id: Value, code: i32, message: &str, data: Value) -> Vec<u8> {
    let mut error = json!({ "code": code, "message": message });
    if !data.is_null() {
        error["data"] = data;
    }
    serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    }))
    .expect("自己拼的 JSON 一定能序列化")
}

/// 读一帧；`Ok(None)` = 管道到头了（EOF）。
///
/// 手写这一段的用意见文件头：这是协议的第二份独立实现。
/// 能力表（dev-plan §4.4 的形状）：`--cap-na=<键>` 把某一个报成 false。
///
/// `streaming` 恒为 false —— 与真实驱动一致：本仓 `Database` trait 上没有流式接口。
fn capabilities(fixture: &Fixture) -> Value {
    let mut map = serde_json::Map::new();
    for key in [
        "schemas",
        "tables",
        "views",
        "materialized_views",
        "routines",
        "sequences",
        "triggers",
        "indexes",
        "constraints",
        "comments",
        "transactions",
        "cancel",
        "cursor",
        "explain",
        "affected_rows",
    ] {
        map.insert(key.to_string(), json!(!fixture.lacks(key)));
    }
    map.insert("streaming".to_string(), json!(false));
    Value::Object(map)
}

/// 假 schema 里的对象：(名字, 类别, 注释, 所属表)。
fn meta_objects(schema: &str) -> Vec<Value> {
    let objects: &[(&str, &str, Option<&str>, Option<&str>)] = match schema {
        "public" => &[
            ("orders", "table", Some("订单"), None),
            ("customers", "table", None, None),
            ("recent_orders", "view", Some("近期订单"), None),
            ("mv_daily", "materialized_view", None, None),
            ("refresh_orders", "procedure", None, None),
            ("order_total", "function", None, None),
            ("orders_seq", "sequence", None, None),
            ("trg_orders_audit", "trigger", None, Some("orders")),
            // 导航摆不下的类别：宿主该跳过它并留一条日志
            ("order_state", "domain", None, None),
        ],
        "sales" => &[("deals", "table", None, None)],
        _ => &[],
    };
    objects
        .iter()
        .map(|(name, kind, comment, parent)| {
            let mut object = json!({ "name": name, "kind": kind });
            if let Some(comment) = comment {
                object["comment"] = json!(comment);
            }
            if let Some(parent) = parent {
                object["parent"] = json!(parent);
            }
            object
        })
        .collect()
}

/// 单个对象的线格式（`meta.object_detail` 要把它回声给宿主）。
fn meta_object(schema: &str, object: &str) -> Option<Value> {
    meta_objects(schema)
        .into_iter()
        .find(|o| o["name"].as_str() == Some(object))
}

/// 列定义：(名字, 原始类型, 归一化类型, 可空, 主键, 格式化提示, 注释)。
fn meta_columns(object: &str) -> Option<Vec<Value>> {
    let columns: &[(&str, &str, &str, bool, bool, Option<&str>, Option<&str>)] = match object {
        "orders" => &[
            ("id", "bigint", "BIGINT", false, true, None, Some("主键")),
            ("customer_id", "bigint", "BIGINT", false, false, None, None),
            (
                "amount",
                "numeric(38,10)",
                "DECIMAL",
                true,
                false,
                Some("decimal(scale=10)"),
                Some("订单金额"),
            ),
            (
                "created_at",
                "timestamp with time zone",
                "TIMESTAMP",
                false,
                false,
                None,
                None,
            ),
            ("note", "text", "TEXT", true, false, None, None),
        ],
        "recent_orders" | "mv_daily" => &[
            ("id", "bigint", "BIGINT", false, true, None, None),
            ("amount", "numeric(38,10)", "DECIMAL", true, false, None, None),
        ],
        "deals" => &[
            ("id", "bigint", "BIGINT", false, true, None, None),
            ("name", "text", "TEXT", true, false, None, None),
        ],
        _ => return None,
    };
    Some(
        columns
            .iter()
            .map(|(name, raw, canonical, nullable, is_pk, format, comment)| {
                let mut column = json!({
                    "name": name,
                    "type_raw": raw,
                    "canonical": canonical,
                    "nullable": nullable,
                    "is_pk": is_pk,
                });
                if let Some(format) = format {
                    column["format"] = json!(format);
                }
                if let Some(comment) = comment {
                    column["comment"] = json!(comment);
                }
                column
            })
            .collect(),
    )
}

fn read_frame(reader: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 5];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    // `total_len` **含头 5 字节**（dev-plan §4.2.1 定死的那一处）
    let total = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let kind = header[4];
    let payload_len = total.checked_sub(5).ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("帧长 {total} 连头都装不下（total_len 是含头的整帧长度）"),
        )
    })?;

    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;
    Ok(Some((kind, payload)))
}
