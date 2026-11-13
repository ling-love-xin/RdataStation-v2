use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use futures_util::FutureExt as _;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{DriverError, ErrorCode};
use crate::frame::{FrameReader, FrameWriter, KIND_JSON};
use crate::meta::{MetaObject, MetaObjectDetail, MetaSchema};
use crate::page::{ExecuteRequest, Page, SessionInfo};

const PROTOCOL_VERSION: u64 = 1;
const DEFAULT_MAX_ROWS: usize = 1000;

/// 一个数据库驱动需要实现的能力面。协议层（会话、帧、错误码）全在 `serve` 里。
#[async_trait]
pub trait Driver: Send + Sync + 'static {
    type Session: Send + 'static;

    fn driver_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn identifier_quote(&self) -> Option<&'static str> {
        Some("\"")
    }

    async fn open(&self, params: &Value) -> Result<(Self::Session, SessionInfo), DriverError>;
    async fn close(&self, session: Self::Session);
    async fn ping(&self, session: &mut Self::Session) -> Result<(), DriverError>;
    async fn execute(&self, session: &mut Self::Session, req: ExecuteRequest) -> Result<Page, DriverError>;

    async fn begin(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
        Err(DriverError::unsupported("该驱动未实现事务"))
    }
    async fn commit(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
        Err(DriverError::unsupported("该驱动未实现事务"))
    }
    async fn rollback(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
        Err(DriverError::unsupported("该驱动未实现事务"))
    }

    async fn catalogs(&self, _session: &mut Self::Session) -> Result<Vec<String>, DriverError> {
        Ok(Vec::new())
    }
    async fn schemas(&self, _session: &mut Self::Session) -> Result<Vec<MetaSchema>, DriverError> {
        Ok(Vec::new())
    }
    async fn objects(
        &self,
        _session: &mut Self::Session,
        _schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        Ok(Vec::new())
    }
    async fn object_detail(
        &self,
        _session: &mut Self::Session,
        _schema: &str,
        _name: &str,
    ) -> Result<MetaObjectDetail, DriverError> {
        Err(DriverError::unsupported("该驱动未实现对象详情"))
    }

    /// 例程源码（`meta.routine_source`）。
    ///
    /// 默认 `Ok(None)`：**没有源码是合法答案**，不是错误 —— 驱动把「查不到」与
    /// 「不支持」都表达成 `null`，界面据此把源码区留空，不需要报错。
    /// 真去查了但失败（断连、权限）才该 `Err`，让用户看见原因。
    async fn routine_source(
        &self,
        _session: &mut Self::Session,
        _schema: &str,
        _name: &str,
    ) -> Result<Option<String>, DriverError> {
        Ok(None)
    }
}

/// 类型擦除后的会话（`Box<dyn Any + Send>`），多驱动进程用它装各驱动的 `Session`。
///
/// 只有一个驱动时也会经过它（`serve` 就是 `serve_many` 的单驱动特例），
/// 这样两条路径的会话管理代码**只有一份**，不会出现「单驱动版能用、多驱动版忘了改」。
pub struct AnySession(Box<dyn std::any::Any + Send>);

impl AnySession {
    fn new<S: Send + 'static>(session: S) -> Self {
        Self(Box::new(session))
    }

    fn downcast_mut<S: 'static>(&mut self) -> Option<&mut S> {
        self.0.downcast_mut::<S>()
    }

    fn into_inner<S: 'static>(self) -> Option<S> {
        self.0.downcast::<S>().ok().map(|boxed| *boxed)
    }
}

/// 类型擦除后的驱动。`impl<D: Driver> SidecarDriver for D`，业务侧不用手写这个 trait。
///
/// 为什么要擦：sidecar 进程按**插件**起（一个 `plugin.toml` 一个可执行文件），而一个插件
/// 可以贡献多个 driver id（例如 sqlx 轨一个进程供 `mysql` 与 `postgres`）。宿主协议本来
/// 就是按 `session.open` 里的 `driver_id` 路由的，`initialize` 也回 `driver_ids` 列表，
/// 多驱动是同一条协议的自然形态；只有 `Driver` trait 的 `type Session` 让「一个进程一个
/// 驱动」成了事实上的限制。
pub trait SidecarDriver: Send + Sync + 'static {
    fn driver_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn identifier_quote(&self) -> Option<&'static str>;

    fn open<'a>(&'a self, params: &'a Value) -> BoxFuture<'a, Result<(AnySession, SessionInfo), DriverError>>;
    fn close<'a>(&'a self, session: AnySession) -> BoxFuture<'a, ()>;
    fn ping<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>>;
    fn execute<'a>(
        &'a self,
        session: &'a mut AnySession,
        req: ExecuteRequest,
    ) -> BoxFuture<'a, Result<Page, DriverError>>;

    fn begin<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>>;
    fn commit<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>>;
    fn rollback<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>>;

    fn catalogs<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<Vec<String>, DriverError>>;
    fn schemas<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<Vec<MetaSchema>, DriverError>>;
    fn objects<'a>(
        &'a self,
        session: &'a mut AnySession,
        schema: String,
    ) -> BoxFuture<'a, Result<Vec<MetaObject>, DriverError>>;
    fn object_detail<'a>(
        &'a self,
        session: &'a mut AnySession,
        schema: String,
        name: String,
    ) -> BoxFuture<'a, Result<MetaObjectDetail, DriverError>>;
    fn routine_source<'a>(
        &'a self,
        session: &'a mut AnySession,
        schema: String,
        name: String,
    ) -> BoxFuture<'a, Result<Option<String>, DriverError>>;
}

/// 把任意 [`Driver`] 擦进 [`SidecarDriver`]。
pub fn as_sidecar<D: Driver>(driver: D) -> Box<dyn SidecarDriver> {
    Box::new(driver)
}

impl<D: Driver> SidecarDriver for D {
    fn driver_id(&self) -> &'static str {
        Driver::driver_id(self)
    }

    fn display_name(&self) -> &'static str {
        Driver::display_name(self)
    }

    fn identifier_quote(&self) -> Option<&'static str> {
        Driver::identifier_quote(self)
    }

    fn open<'a>(&'a self, params: &'a Value) -> BoxFuture<'a, Result<(AnySession, SessionInfo), DriverError>> {
        Box::pin(async move {
            let (session, info) = Driver::open(self, params).await?;
            Ok((AnySession::new(session), info))
        })
    }

    fn close<'a>(&'a self, session: AnySession) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Some(session) = session.into_inner::<D::Session>() {
                Driver::close(self, session).await;
            }
        })
    }

    fn ping<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "session.ping")?;
            Driver::ping(self, session).await
        })
    }

    fn execute<'a>(
        &'a self,
        session: &'a mut AnySession,
        req: ExecuteRequest,
    ) -> BoxFuture<'a, Result<Page, DriverError>> {
        Box::pin(async move {
            let session =
                session_of::<D::Session>(session, Driver::driver_id(self), "query.execute")?;
            Driver::execute(self, session, req).await
        })
    }

    fn begin<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "tx.begin")?;
            Driver::begin(self, session).await
        })
    }

    fn commit<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "tx.commit")?;
            Driver::commit(self, session).await
        })
    }

    fn rollback<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "tx.rollback")?;
            Driver::rollback(self, session).await
        })
    }

    fn catalogs<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<Vec<String>, DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "meta.catalogs")?;
            Driver::catalogs(self, session).await
        })
    }

    fn schemas<'a>(&'a self, session: &'a mut AnySession) -> BoxFuture<'a, Result<Vec<MetaSchema>, DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "meta.schemas")?;
            Driver::schemas(self, session).await
        })
    }

    fn objects<'a>(
        &'a self,
        session: &'a mut AnySession,
        schema: String,
    ) -> BoxFuture<'a, Result<Vec<MetaObject>, DriverError>> {
        Box::pin(async move {
            let session = session_of::<D::Session>(session, Driver::driver_id(self), "meta.objects")?;
            Driver::objects(self, session, &schema).await
        })
    }

    fn object_detail<'a>(
        &'a self,
        session: &'a mut AnySession,
        schema: String,
        name: String,
    ) -> BoxFuture<'a, Result<MetaObjectDetail, DriverError>> {
        Box::pin(async move {
            let session =
                session_of::<D::Session>(session, Driver::driver_id(self), "meta.object_detail")?;
            Driver::object_detail(self, session, &schema, &name).await
        })
    }

    fn routine_source<'a>(
        &'a self,
        session: &'a mut AnySession,
        schema: String,
        name: String,
    ) -> BoxFuture<'a, Result<Option<String>, DriverError>> {
        Box::pin(async move {
            let session =
                session_of::<D::Session>(session, Driver::driver_id(self), "meta.routine_source")?;
            Driver::routine_source(self, session, &schema, &name).await
        })
    }
}

/// 会话类型对不上 = 请求路由错了驱动（同一个进程里有多个驱动时的编程错误），按内部错误报。
fn session_of<'s, S: 'static>(
    session: &'s mut AnySession,
    driver_id: &str,
    method: &str,
) -> Result<&'s mut S, DriverError> {
    session.downcast_mut::<S>().ok_or_else(|| {
        DriverError::internal(format!(
            "{method} 的会话类型与驱动 {driver_id} 不匹配（会话被路由到了别的驱动）"
        ))
    })
}

struct SessionEntry {
    /// `Server::drivers` 的下标 —— 会话记住自己属于哪个驱动，路由不靠猜。
    driver: usize,
    session: AnySession,
    info: SessionInfo,
}

struct Server {
    drivers: Vec<Box<dyn SidecarDriver>>,
    sessions: HashMap<String, SessionEntry>,
    cancelled: HashSet<String>,
}

/// 跑**单驱动** sidecar 主循环（既有插件用这条路径，行为不变）。
///
/// 返回 `Ok(())` 只表示 stdin 干净 EOF（宿主已退出）。
pub async fn serve<D, R, W>(driver: D, reader: R, writer: W) -> Result<(), String>
where
    D: Driver,
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    serve_many(vec![as_sidecar(driver)], reader, writer).await
}

/// 跑**多驱动** sidecar 主循环：一个进程按 `driver_id` 供多个数据库。
///
/// 这是 `serve` 的推广形态 —— `serve` 就是「一个驱动」的特例，会话管理代码只有一份。
pub async fn serve_many<R, W>(
    drivers: Vec<Box<dyn SidecarDriver>>,
    reader: R,
    writer: W,
) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if drivers.is_empty() {
        return Err("至少要有 1 个驱动".to_string());
    }
    {
        // 同一进程里两个驱动撞同一个 id，路由就没法有确定答案 —— 起进程时就拒绝。
        let mut seen = HashSet::new();
        for driver in &drivers {
            if !seen.insert(driver.driver_id()) {
                return Err(format!("同一个进程里有两个驱动都声称是 {}", driver.driver_id()));
            }
        }
    }

    let mut reader = FrameReader::new(reader);
    let mut writer = FrameWriter::new(writer);
    let mut server = Server {
        drivers,
        sessions: HashMap::new(),
        cancelled: HashSet::new(),
    };

    loop {
        let frame = match reader.read_frame().await {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                server.close_all().await;
                return Ok(());
            }
            Err(err) => {
                return Err(format!("读取宿主帧失败：{err}"));
            }
        };
        if frame.kind != KIND_JSON {
            return Err(format!("宿主发来非 JSON 帧（kind=0x{:02x}）", frame.kind));
        }
        let request: Value = match serde_json::from_slice(&frame.payload) {
            Ok(value) => value,
            Err(err) => {
                send_error(&mut writer, Value::Null, -32700, &format!("parse error: {err}"), None)
                    .await
                    .map_err(|e| format!("写错误响应失败：{e}"))?;
                continue;
            }
        };
        // 宿主发来的响应（没有 method）与通知直接忽略。
        let Some(method) = request.get("method").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let has_id = !id.is_null();

        // panic 隔离：驱动里任何一处 `unwrap` 踩炸，只把这个请求变成
        // -32603 内部错误，进程继续服务下一条请求。
        //
        // 不隔离的代价是实测过的：2026-11-12 MSSQL 真机 `meta.object_detail`
        // 里 `sys.columns.precision` 是 tinyint 却按 `i16` 取，tiberius 的
        // `Row::get` 内部 unwrap panic，进程 exit 101 —— 宿主只看到
        // 「帧 I/O 失败：early eof」，同一实例上所有会话一起断，且没法定位
        // 是哪个对象、哪一列炸的。
        //
        // `AssertUnwindSafe` 是必须的：`dispatch` 借了 `&mut self` 与
        // `&mut writer`，跨 panic 边界时编译器无法证明安全。panic 之后
        // 会话表可能停在半更新状态，这是「宁可让一个会话看病、也不让整条
        // 进程死」的取舍；真出问题时 stderr 仍留有默认 panic hook 打印的
        // 位置与原值，可用于修根因。
        let outcome = AssertUnwindSafe(server.dispatch(&method, &params, &mut writer))
            .catch_unwind()
            .await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(panic) => Err(DriverError::internal(format!(
                "{method} 触发驱动内部 panic（已隔离，进程继续）：{}",
                panic_message(panic.as_ref())
            ))),
        };
        match outcome {
            Ok(Dispatch::Result(result)) => {
                if has_id {
                    send_result(&mut writer, id, result)
                        .await
                        .map_err(|e| format!("写响应失败：{e}"))?;
                }
            }
            Ok(Dispatch::Shutdown(result)) => {
                if has_id {
                    send_result(&mut writer, id, result)
                        .await
                        .map_err(|e| format!("写关机响应失败：{e}"))?;
                }
                server.close_all().await;
                return Ok(());
            }
            Ok(Dispatch::Arrow { head, chunks }) => {
                if has_id {
                    let envelope = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": head,
                    });
                    let payload = serde_json::to_vec(&envelope).map_err(|e| format!("序列化响应失败：{e}"))?;
                    writer
                        .write_json_with_arrow(&payload, &chunks)
                        .await
                        .map_err(|e| format!("写附件响应失败：{e}"))?;
                }
            }
            Err(err) => {
                if has_id {
                    let data = err.to_json()["data"].clone();
                    send_error(&mut writer, id, err.code.code(), &err.message, Some(data))
                        .await
                        .map_err(|e| format!("写错误响应失败：{e}"))?;
                }
            }
        }
    }
}

/// 把 `catch_unwind` 接住的载荷翻成一句可进日志 / 错误消息的话。
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "panic 载荷不是字符串".to_string()
}

enum Dispatch {
    Result(Value),
    Arrow { head: Value, chunks: Vec<Vec<u8>> },
    Shutdown(Value),
}

/// 写响应 / 写错误是**自由函数**而不是 `Server` 的方法：它们要在 `serve` 主循环里被
/// `await`，若挂成 `&self` 方法，`&Server` 就会跨 await 存活 —— 而 `Server` 里装着
/// 各驱动的会话（`Box<dyn Any + Send>`，天然不 `Sync`），一条 `&Server` 跨 await 就要求
/// `Server: Sync`，多驱动支持会被迫给所有驱动的 `Session` 加 `Sync` 约束（Oracle /
/// MSSQL 的会话未必满足）。自由函数只借 `writer`，`Server` 只需要 `Send`。
async fn send_result<W: AsyncWrite + Unpin>(
    writer: &mut FrameWriter<W>,
    id: Value,
    result: Value,
) -> Result<(), std::io::Error> {
    let payload = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
    .map_err(std::io::Error::other)?;
    writer.write_frame(KIND_JSON, &payload).await
}

async fn send_error<W: AsyncWrite + Unpin>(
    writer: &mut FrameWriter<W>,
    id: Value,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Result<(), std::io::Error> {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    let payload = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    }))
    .map_err(std::io::Error::other)?;
    writer.write_frame(KIND_JSON, &payload).await
}

impl Server {
    async fn close_all(&mut self) {
        let sessions = std::mem::take(&mut self.sessions);
        for (_, entry) in sessions {
            let driver = &*self.drivers[entry.driver];
            driver.close(entry.session).await;
        }
    }

    /// 取「会话 + 它所属的驱动」。两个字段分别借，绕开 `&mut self` 的整借冲突。
    fn routed<'s>(&'s mut self, session_id: &str) -> Result<(&'s dyn SidecarDriver, &'s mut SessionEntry), DriverError> {
        let drivers = &self.drivers;
        let sessions = &mut self.sessions;
        let entry = sessions.get_mut(session_id).ok_or_else(|| {
            DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
        })?;
        let driver: &dyn SidecarDriver = &*drivers[entry.driver];
        Ok((driver, entry))
    }

    async fn dispatch<W: AsyncWrite + Unpin>(
        &mut self,
        method: &str,
        params: &Value,
        _writer: &mut FrameWriter<W>,
    ) -> Result<Dispatch, DriverError> {
        match method {
            "initialize" => {
                let requested = params.get("protocol").cloned().unwrap_or(Value::Null);
                let ok = requested.as_u64() == Some(PROTOCOL_VERSION)
                    || requested.as_str().map(|s| s == "1" || s == "rds-driver/1").unwrap_or(false);
                if !ok {
                    return Err(DriverError::new(
                        ErrorCode::ProtocolVersionMismatch,
                        format!(
                            "协议版本不匹配：宿主 protocol={requested}，本驱动 protocol={PROTOCOL_VERSION}"
                        ),
                    ));
                }
                let ids: Vec<&'static str> = self.drivers.iter().map(|d| d.driver_id()).collect();
                Ok(Dispatch::Result(json!({
                    "protocol": PROTOCOL_VERSION,
                    "driver_ids": ids,
                    "runtime": { "kind": "rust" },
                })))
            }
            "ping" => Ok(Dispatch::Result(json!({ "pong": true }))),
            "shutdown" => Ok(Dispatch::Shutdown(json!({ "ok": true }))),
            "driver.describe" => {
                let session_id = require_str(params, "session_id")?;
                let (driver, entry) = self.routed(&session_id)?;
                let descriptor = json!({
                    "driver_id": driver.driver_id(),
                    "display_name": driver.display_name(),
                    "server_version": entry.info.server_version.clone(),
                    "capabilities": entry.info.capabilities_json(),
                    "identifier_quote": driver.identifier_quote(),
                    "default_schema": entry.info.default_schema.clone(),
                });
                Ok(Dispatch::Result(descriptor))
            }
            "session.open" => {
                let session_id = require_str(params, "session_id")?;
                let driver_id = require_str(params, "driver_id")?;
                let idx = self
                    .drivers
                    .iter()
                    .position(|d| d.driver_id() == driver_id)
                    .ok_or_else(|| {
                        DriverError::new(
                            ErrorCode::DriverNotSupported,
                            format!("驱动 id 不匹配：请求 {driver_id}，本进程提供不了"),
                        )
                    })?;
                let conn = params.get("params").cloned().unwrap_or_else(|| json!({}));
                let (session, info) = self.drivers[idx].open(&conn).await?;
                let version = info.server_version.clone();
                self.sessions.insert(
                    session_id.clone(),
                    SessionEntry {
                        driver: idx,
                        session,
                        info,
                    },
                );
                Ok(Dispatch::Result(json!({
                    "session_id": session_id,
                    "server_version": version,
                })))
            }
            "session.close" => {
                let session_id = require_str(params, "session_id")?;
                let Some(entry) = self.sessions.remove(&session_id) else {
                    return Err(DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}")));
                };
                let driver = &*self.drivers[entry.driver];
                driver.close(entry.session).await;
                Ok(Dispatch::Result(json!({})))
            }
            "session.ping" => {
                let session_id = require_str(params, "session_id")?;
                let (driver, entry) = self.routed(&session_id)?;
                driver.ping(&mut entry.session).await?;
                Ok(Dispatch::Result(json!({})))
            }
            "query.execute" => {
                let session_id = require_str(params, "session_id")?;
                let request_id = require_str(params, "request_id")?;
                self.cancelled.remove(&request_id);
                let sql = require_str(params, "sql")?;
                let max_rows = params
                    .get("max_rows")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
                    .filter(|v| *v > 0)
                    .unwrap_or(DEFAULT_MAX_ROWS);
                let bind_params = match params.get("params") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(items)) => items.clone(),
                    Some(_) => {
                        return Err(DriverError::protocol("query.execute 的 params 必须是数组"));
                    }
                };
                let (driver, entry) = self.routed(&session_id)?;
                let page = driver
                    .execute(
                        &mut entry.session,
                        ExecuteRequest {
                            sql,
                            params: bind_params,
                            max_rows,
                        },
                    )
                    .await?;
                if self.cancelled.remove(&request_id) {
                    return Err(DriverError::new(ErrorCode::Cancelled, "查询已取消"));
                }
                Ok(self.encode_page(page)?)
            }
            "query.fetch" => Err(DriverError::new(
                ErrorCode::DriverNotSupported,
                "当前驱动未提供服务端游标；请用 max_rows 分页重查",
            )),
            "query.cancel" => {
                let request_id = require_str(params, "request_id")?;
                self.cancelled.insert(request_id);
                Ok(Dispatch::Result(json!({})))
            }
            "tx.begin" | "tx.commit" | "tx.rollback" => {
                let session_id = require_str(params, "session_id")?;
                let (driver, entry) = self.routed(&session_id)?;
                match method {
                    "tx.begin" => driver.begin(&mut entry.session).await?,
                    "tx.commit" => driver.commit(&mut entry.session).await?,
                    _ => driver.rollback(&mut entry.session).await?,
                }
                Ok(Dispatch::Result(json!({})))
            }
            "meta.catalogs" => {
                let session_id = require_str(params, "session_id")?;
                let (driver, entry) = self.routed(&session_id)?;
                let catalogs = driver.catalogs(&mut entry.session).await?;
                Ok(Dispatch::Result(json!({ "catalogs": catalogs })))
            }
            "meta.schemas" => {
                let session_id = require_str(params, "session_id")?;
                let (driver, entry) = self.routed(&session_id)?;
                let schemas = driver.schemas(&mut entry.session).await?;
                Ok(Dispatch::Result(json!({ "schemas": schemas })))
            }
            "meta.objects" => {
                let session_id = require_str(params, "session_id")?;
                let schema = optional_str(params, "schema")
                    .or_else(|| optional_str(params, "catalog"))
                    .unwrap_or_default();
                let (driver, entry) = self.routed(&session_id)?;
                let objects = driver.objects(&mut entry.session, schema).await?;
                Ok(Dispatch::Result(json!({ "objects": objects })))
            }
            "meta.object_detail" => {
                let session_id = require_str(params, "session_id")?;
                let schema = optional_str(params, "schema")
                    .or_else(|| optional_str(params, "catalog"))
                    .unwrap_or_default();
                let name = optional_str(params, "object")
                    .or_else(|| optional_str(params, "name"))
                    .ok_or_else(|| DriverError::protocol("meta.object_detail 缺少 object/name"))?;
                let (driver, entry) = self.routed(&session_id)?;
                let detail = driver.object_detail(&mut entry.session, schema, name).await?;
                Ok(Dispatch::Result(serde_json::to_value(detail).unwrap_or_else(|_| json!({}))))
            }
            "meta.routine_source" => {
                let session_id = require_str(params, "session_id")?;
                let schema = optional_str(params, "schema")
                    .or_else(|| optional_str(params, "catalog"))
                    .unwrap_or_default();
                let name = optional_str(params, "object")
                    .or_else(|| optional_str(params, "name"))
                    .ok_or_else(|| DriverError::protocol("meta.routine_source 缺少 object/name"))?;
                let (driver, entry) = self.routed(&session_id)?;
                // 拿不到就是 null（合法答案），不是错误 —— 之前这里被硬编码成 null，
                // 于是所有 Rust 驱动的源码面板永远是空的，自检也绿着。
                Ok(Dispatch::Result(match driver.routine_source(&mut entry.session, schema, name).await? {
                    Some(source) => Value::String(source),
                    None => Value::Null,
                }))
            }
            other => Err(DriverError::protocol(format!("未实现的方法：{other}"))),
        }
    }

    fn encode_page(&self, page: Page) -> Result<Dispatch, DriverError> {
        let columns: Vec<Value> = page.columns.iter().map(|c| c.to_wire()).collect();
        let mut head = json!({
            "columns": columns,
            "row_count": page.row_count(),
            "has_more": page.has_more,
            "truncated": page.has_more,
            "notices": page.notices,
        });
        if let Some(affected) = page.affected_rows {
            head["affected_rows"] = json!(affected);
        }
        if let Some(cursor_id) = &page.cursor_id {
            head["cursor_id"] = json!(cursor_id);
        }

        if page.should_inline() {
            head["rows"] = json!(page.inline_rows());
            return Ok(Dispatch::Result(head));
        }
        let chunks = page
            .arrow_chunks()
            .map_err(|e| DriverError::new(ErrorCode::ResourceLimit, e))?;
        head["attachments"] = json!([{
            "id": 0,
            "kind": "arrow-ipc-stream",
            "frames": chunks.len(),
        }]);
        Ok(Dispatch::Arrow { head, chunks })
    }
}

fn require_str(params: &Value, key: &str) -> Result<String, DriverError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| DriverError::protocol(format!("缺少字符串参数：{key}")))
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use arrow::datatypes::{DataType, TimeUnit};
    use arrow::ipc::reader::StreamReader;
    use async_trait::async_trait;

    use super::*;
    use crate::meta::{MetaColumn, MetaObjectDetail};
    use crate::page::{Canonical, ColumnDef};

    struct FakeDriver;

    struct FakeSession {
        rows: usize,
    }

    #[async_trait]
    impl Driver for FakeDriver {
        type Session = FakeSession;

        fn driver_id(&self) -> &'static str {
            "fake"
        }
        fn display_name(&self) -> &'static str {
            "Fake Driver"
        }
        async fn open(&self, params: &Value) -> Result<(Self::Session, SessionInfo), DriverError> {
            if params.get("fail").and_then(Value::as_bool) == Some(true) {
                return Err(DriverError::connect("测试用连接失败"));
            }
            let rows = params.get("rows").and_then(Value::as_u64).unwrap_or(0) as usize;
            let mut caps = BTreeMap::new();
            caps.insert("schemas".to_string(), true);
            caps.insert("transactions".to_string(), true);
            caps.insert("cancel".to_string(), true);
            caps.insert("arrow".to_string(), true);
            Ok((
                FakeSession { rows },
                SessionInfo {
                    server_version: Some("FakeDB 1.0".to_string()),
                    default_schema: Some("public".to_string()),
                    capabilities: caps,
                },
            ))
        }
        async fn close(&self, _session: Self::Session) {}
        async fn ping(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
            Ok(())
        }
        async fn execute(&self, session: &mut Self::Session, req: ExecuteRequest) -> Result<Page, DriverError> {
            if req.sql == "FAIL" {
                return Err(DriverError::sql("boom").with_sqlstate("42000"));
            }
            let columns = vec![
                ColumnDef::new("id", "NUMBER", false, Canonical::Integer),
                ColumnDef::new("name", "VARCHAR2", true, Canonical::Text),
                ColumnDef::new("ts", "TIMESTAMP", true, Canonical::Timestamp),
            ];
            if req.sql == "DML" {
                return Ok(Page::affected(3));
            }
            let count = session.rows.min(req.max_rows);
            let rows = (0..count)
                .map(|i| vec![json!(i), json!(format!("row-{i}")), json!("2026-01-02T03:04:05.123456Z")])
                .collect();
            Ok(Page::rows(columns, rows))
        }
        async fn begin(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
            Ok(())
        }
        async fn commit(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
            Ok(())
        }
        async fn rollback(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
            Ok(())
        }
        async fn schemas(&self, _session: &mut Self::Session) -> Result<Vec<MetaSchema>, DriverError> {
            Ok(vec![MetaSchema { name: "public".to_string(), comment: None }])
        }
        async fn objects(&self, _session: &mut Self::Session, schema: &str) -> Result<Vec<MetaObject>, DriverError> {
            Ok(vec![MetaObject {
                name: "t1".to_string(),
                kind: "table".to_string(),
                comment: None,
                parent: Some(schema.to_string()),
            }])
        }
        async fn object_detail(
            &self,
            _session: &mut Self::Session,
            _schema: &str,
            name: &str,
        ) -> Result<MetaObjectDetail, DriverError> {
            // 测试用：模拟「某列类型按错 Rust 类型取 → unwrap 踩炸」。
            // 真机上的同款事故见 `a_panicking_request_is_isolated_*` 的注释。
            if name == "PANIC" {
                panic!("测试用 panic：对象名是 PANIC");
            }
            Ok(MetaObjectDetail::new(
                name,
                "table",
                vec![MetaColumn {
                    name: "id".to_string(),
                    type_raw: "NUMBER".to_string(),
                    canonical: Some("INTEGER".to_string()),
                    nullable: Some(false),
                    is_pk: Some(true),
                    is_fk: None,
                    default: None,
                    format: None,
                    comment: None,
                }],
            ))
        }

        // 测试用：`P_WITH_SRC` 拿得到源码，其它例程一律 null（模拟加密对象 / 无权限）。
        async fn routine_source(
            &self,
            _session: &mut Self::Session,
            schema: &str,
            name: &str,
        ) -> Result<Option<String>, DriverError> {
            if name == "P_WITH_SRC" {
                Ok(Some(format!("CREATE OR REPLACE PROCEDURE {schema}.{name} IS BEGIN NULL; END;")))
            } else {
                Ok(None)
            }
        }
    }

    async fn request_json(writer: &mut FrameWriter<tokio::io::WriteHalf<tokio::io::DuplexStream>>, id: u64, method: &str, params: Value) {
        let payload = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }))
        .unwrap();
        writer.write_frame(KIND_JSON, &payload).await.unwrap();
    }

    async fn read_json(reader: &mut FrameReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>) -> Value {
        let frame = reader.read_frame().await.unwrap().expect("响应帧");
        serde_json::from_slice(&frame.payload).unwrap()
    }

    #[tokio::test]
    async fn initialize_ping_and_eof_exit() {
        let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move { serve(FakeDriver, server_rx, server_tx).await });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(&mut writer, 1, "initialize", json!({"protocol": 1})).await;
        let init = read_json(&mut reader).await;
        assert_eq!(init["result"]["protocol"], 1);
        assert_eq!(init["result"]["driver_ids"][0], "fake");

        request_json(&mut writer, 2, "ping", json!({})).await;
        let pong = read_json(&mut reader).await;
        assert_eq!(pong["result"]["pong"], true);

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn routine_source_returns_text_or_null_but_never_a_stub() {
        let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move { serve(FakeDriver, server_rx, server_tx).await });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(&mut writer, 1, "initialize", json!({"protocol": 1})).await;
        let _ = read_json(&mut reader).await;
        request_json(
            &mut writer,
            2,
            "session.open",
            json!({"session_id": "s1", "driver_id": "fake"}),
        )
        .await;
        let open = read_json(&mut reader).await;
        assert_eq!(open["result"]["session_id"], "s1");

        // 驱动真给了源码就该原样回传字符串（不是 null，也不是包一层对象）。
        request_json(
            &mut writer,
            3,
            "meta.routine_source",
            json!({"session_id": "s1", "schema": "demo", "object": "P_WITH_SRC"}),
        )
        .await;
        let with_src = read_json(&mut reader).await;
        let text = with_src["result"]
            .as_str()
            .expect("源码应当是字符串而不是对象/数组");
        assert!(text.contains("CREATE OR REPLACE PROCEDURE demo.P_WITH_SRC"));

        // 驱动说没有 → null 是合法答案，不能变成错误。
        request_json(
            &mut writer,
            4,
            "meta.routine_source",
            json!({"session_id": "s1", "schema": "demo", "object": "P_NO_SRC"}),
        )
        .await;
        let no_src = read_json(&mut reader).await;
        assert!(no_src.get("error").is_none(), "拿不到源码不算错误");
        assert!(no_src["result"].is_null(), "拿不到源码就是 null");

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn version_mismatch_and_unknown_method_use_protocol_errors() {
        let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move { serve(FakeDriver, server_rx, server_tx).await });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(&mut writer, 1, "initialize", json!({"protocol": 2})).await;
        let resp = read_json(&mut reader).await;
        assert_eq!(resp["error"]["code"], -32007);
        assert!(resp["error"]["message"].as_str().unwrap().contains("1"));

        request_json(&mut writer, 2, "nope.method", json!({})).await;
        let resp = read_json(&mut reader).await;
        assert_eq!(resp["error"]["code"], -32001);

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn connect_failure_and_session_not_found() {
        let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move { serve(FakeDriver, server_rx, server_tx).await });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(&mut writer, 1, "session.open", json!({
            "session_id": "s1", "driver_id": "fake", "params": {"fail": true}
        })).await;
        let resp = read_json(&mut reader).await;
        assert_eq!(resp["error"]["code"], -32009);

        request_json(&mut writer, 2, "query.execute", json!({
            "session_id": "missing", "request_id": "r1", "sql": "SELECT 1", "max_rows": 10
        })).await;
        let resp = read_json(&mut reader).await;
        assert_eq!(resp["error"]["code"], -32002);

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn small_page_inline_and_large_page_arrow() {
        let (server_io, client_io) = tokio::io::duplex(8 * 1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move { serve(FakeDriver, server_rx, server_tx).await });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(&mut writer, 1, "session.open", json!({
            "session_id": "s1", "driver_id": "fake", "params": {}
        })).await;
        let _ = read_json(&mut reader).await;

        request_json(&mut writer, 2, "query.execute", json!({
            "session_id": "s1", "request_id": "r1", "sql": "SELECT", "max_rows": 50
        })).await;
        let page = read_json(&mut reader).await;
        assert_eq!(page["result"]["row_count"], 0);
        assert_eq!(page["result"]["rows"].as_array().unwrap().len(), 0);
        assert_eq!(page["result"]["columns"].as_array().unwrap().len(), 3);

        // 大结果：改会话行数，另一个会话。
        request_json(&mut writer, 3, "session.open", json!({
            "session_id": "s2", "driver_id": "fake", "params": {"rows": 3000}
        })).await;
        let _ = read_json(&mut reader).await;
        request_json(&mut writer, 4, "query.execute", json!({
            "session_id": "s2", "request_id": "r2", "sql": "SELECT", "max_rows": 3000
        })).await;
        let head = read_json(&mut reader).await;
        let result = &head["result"];
        assert_eq!(result["row_count"], 3000, "响应头={head}");
        assert!(result.get("rows").is_none());
        let frames = result["attachments"][0]["frames"].as_u64().unwrap() as usize;
        let mut bytes = Vec::new();
        for _ in 0..frames {
            let frame = reader.read_frame().await.unwrap().expect("附件帧");
            assert_eq!(frame.kind, crate::frame::KIND_ARROW);
            bytes.extend_from_slice(&frame.payload);
        }
        let ipc = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
        let schema = ipc.schema();
        let batch = Arc::new(ipc.into_iter().next().unwrap().unwrap());
        assert_eq!(batch.num_rows(), 3000);
        assert_eq!(schema.field(0).data_type(), &DataType::Int64);
        assert_eq!(
            schema.field(2).data_type(),
            &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
        );

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }

    /// 一个请求把驱动踩炸，只该让这个请求失败，不该把进程带走。
    ///
    /// 这条钉的是真机上发生过的事故：MSSQL 的 `meta.object_detail` 把
    /// `sys.columns.precision`（tinyint）按 `i16` 取，tiberius 内部 unwrap
    /// panic，进程 exit 101；宿主只看到「帧 I/O 失败：early eof」，同一实例
    /// 上所有会话一起断，而且完全不知道是哪个对象、哪一列炸的。
    #[tokio::test]
    async fn a_panicking_request_is_isolated_and_the_process_keeps_serving() {
        let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move { serve(FakeDriver, server_rx, server_tx).await });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(
            &mut writer,
            1,
            "session.open",
            json!({"session_id": "s1", "driver_id": "fake", "params": {"rows": 3}}),
        )
        .await;
        let opened = read_json(&mut reader).await;
        assert!(opened.get("result").is_some(), "开会话应成功：{opened}");

        // 这一条会让驱动 panic。
        request_json(
            &mut writer,
            2,
            "meta.object_detail",
            json!({"session_id": "s1", "schema": "public", "object": "PANIC"}),
        )
        .await;
        let panicked = read_json(&mut reader).await;
        assert_eq!(panicked["id"], 2);
        assert_eq!(panicked["error"]["code"], -32603, "panic 应转成内部错误：{panicked}");
        let message = panicked["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("meta.object_detail"), "错误消息要点名方法：{message}");
        assert!(message.contains("panic"), "错误消息要说清是 panic：{message}");

        // 进程还活着，同一条连接上的下一条请求照常。
        request_json(&mut writer, 3, "ping", json!({})).await;
        let pong = read_json(&mut reader).await;
        assert_eq!(pong["id"], 3);
        assert_eq!(pong["result"]["pong"], true, "panic 之后进程应继续服务：{pong}");

        // 会话也仍然可用：换个正常对象就能查到详情。
        request_json(
            &mut writer,
            4,
            "meta.object_detail",
            json!({"session_id": "s1", "schema": "public", "object": "t1"}),
        )
        .await;
        let detail = read_json(&mut reader).await;
        assert_eq!(detail["id"], 4);
        assert_eq!(detail["result"]["object"]["name"], "t1", "会话应仍可用：{detail}");

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }

    /// 另一个驱动：会话类型与 `FakeSession` 完全不同，用来钉「按 driver_id 路由」。
    struct OtherDriver;

    struct OtherSession {
        tag: String,
    }

    #[async_trait]
    impl Driver for OtherDriver {
        type Session = OtherSession;

        fn driver_id(&self) -> &'static str {
            "other"
        }
        fn display_name(&self) -> &'static str {
            "Other Driver"
        }
        async fn open(&self, _params: &Value) -> Result<(Self::Session, SessionInfo), DriverError> {
            Ok((
                OtherSession { tag: "from-other".to_string() },
                SessionInfo {
                    server_version: Some("OtherDB 2.0".to_string()),
                    default_schema: None,
                    capabilities: BTreeMap::new(),
                },
            ))
        }
        async fn close(&self, _session: Self::Session) {}
        async fn ping(&self, _session: &mut Self::Session) -> Result<(), DriverError> {
            Ok(())
        }
        async fn execute(&self, session: &mut Self::Session, _req: ExecuteRequest) -> Result<Page, DriverError> {
            Ok(Page::rows(
                vec![ColumnDef::new("who", "TEXT", false, Canonical::Text)],
                vec![vec![json!(session.tag)]],
            ))
        }
    }

    /// 一个进程供两个驱动：`initialize` 回两个 id，会话按 `session.open` 的
    /// `driver_id` 各进各的类型擦除槽位，互不串线。
    #[tokio::test]
    async fn two_drivers_in_one_process_route_by_driver_id() {
        let (server_io, client_io) = tokio::io::duplex(1024 * 1024);
        let (server_rx, server_tx) = tokio::io::split(server_io);
        let (client_rx, client_tx) = tokio::io::split(client_io);
        let server = tokio::spawn(async move {
            serve_many(
                vec![as_sidecar(FakeDriver), as_sidecar(OtherDriver)],
                server_rx,
                server_tx,
            )
            .await
        });
        let mut writer = FrameWriter::new(client_tx);
        let mut reader = FrameReader::new(client_rx);

        request_json(&mut writer, 1, "initialize", json!({"protocol": 1})).await;
        let init = read_json(&mut reader).await;
        let ids: Vec<&str> = init["result"]["driver_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["fake", "other"], "初始化应报出全部驱动 id：{init}");

        // 两个会话：一个 fake（有 rows 参数）、一个 other。
        request_json(&mut writer, 2, "session.open", json!({
            "session_id": "s-fake", "driver_id": "fake", "params": {"rows": 2}
        })).await;
        let ok = read_json(&mut reader).await;
        assert!(ok.get("result").is_some(), "fake 会话应能打开：{ok}");

        request_json(&mut writer, 3, "session.open", json!({
            "session_id": "s-other", "driver_id": "other", "params": {}
        })).await;
        let ok = read_json(&mut reader).await;
        assert!(ok.get("result").is_some(), "other 会话应能打开：{ok}");

        // 描述各报各的版本与能力，说明 describe 也是按会话路由的。
        request_json(&mut writer, 4, "driver.describe", json!({"session_id": "s-fake"})).await;
        let fake_desc = read_json(&mut reader).await;
        assert_eq!(fake_desc["result"]["driver_id"], "fake");
        assert_eq!(fake_desc["result"]["server_version"], "FakeDB 1.0");

        request_json(&mut writer, 5, "driver.describe", json!({"session_id": "s-other"})).await;
        let other_desc = read_json(&mut reader).await;
        assert_eq!(other_desc["result"]["driver_id"], "other");
        assert_eq!(other_desc["result"]["server_version"], "OtherDB 2.0");

        // 两个会话各执行一条：数据来自各自驱动，说明会话槽位没串。
        request_json(&mut writer, 6, "query.execute", json!({
            "session_id": "s-fake", "request_id": "r1", "sql": "SELECT", "max_rows": 10
        })).await;
        let fake_page = read_json(&mut reader).await;
        assert_eq!(fake_page["result"]["row_count"], 2);
        assert_eq!(fake_page["result"]["columns"][0]["name"], "id");

        request_json(&mut writer, 7, "query.execute", json!({
            "session_id": "s-other", "request_id": "r2", "sql": "SELECT", "max_rows": 10
        })).await;
        let other_page = read_json(&mut reader).await;
        assert_eq!(other_page["result"]["columns"][0]["name"], "who");
        assert_eq!(other_page["result"]["rows"][0]["who"], "from-other");

        // 不认识的 id：明确报驱动不支持，不是「随便挑一个」。
        request_json(&mut writer, 8, "session.open", json!({
            "session_id": "s-nope", "driver_id": "nope", "params": {}
        })).await;
        let nope = read_json(&mut reader).await;
        assert_eq!(nope["error"]["code"], -32001, "未知驱动应报 driver_not_supported：{nope}");

        drop(writer);
        drop(reader);
        server.await.unwrap().unwrap();
    }
}
