use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
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
}

struct SessionEntry<S> {
    session: S,
    info: SessionInfo,
}

struct Server<D: Driver> {
    driver: D,
    sessions: HashMap<String, SessionEntry<D::Session>>,
    cancelled: HashSet<String>,
}

/// 跑 sidecar 主循环。返回 `Ok(())` 只表示 stdin 干净 EOF（宿主已退出）。
pub async fn serve<D, R, W>(driver: D, reader: R, writer: W) -> Result<(), String>
where
    D: Driver,
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = FrameReader::new(reader);
    let mut writer = FrameWriter::new(writer);
    let mut server = Server {
        driver,
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
                server
                    .send_error(&mut writer, Value::Null, -32700, &format!("parse error: {err}"), None)
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

        let outcome = server.dispatch(&method, &params, &mut writer).await;
        match outcome {
            Ok(Dispatch::Result(result)) => {
                if has_id {
                    server
                        .send_result(&mut writer, id, result)
                        .await
                        .map_err(|e| format!("写响应失败：{e}"))?;
                }
            }
            Ok(Dispatch::Shutdown(result)) => {
                if has_id {
                    server
                        .send_result(&mut writer, id, result)
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
                    server
                        .send_error(&mut writer, id, err.code.code(), &err.message, Some(data))
                        .await
                        .map_err(|e| format!("写错误响应失败：{e}"))?;
                }
            }
        }
    }
}

enum Dispatch {
    Result(Value),
    Arrow { head: Value, chunks: Vec<Vec<u8>> },
    Shutdown(Value),
}

impl<D: Driver> Server<D> {
    async fn close_all(&mut self) {
        for (_, entry) in self.sessions.drain() {
            self.driver.close(entry.session).await;
        }
    }

    async fn send_result<W: AsyncWrite + Unpin>(
        &self,
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
        &self,
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
                Ok(Dispatch::Result(json!({
                    "protocol": PROTOCOL_VERSION,
                    "driver_ids": [self.driver.driver_id()],
                    "runtime": { "kind": "rust" },
                })))
            }
            "ping" => Ok(Dispatch::Result(json!({ "pong": true }))),
            "shutdown" => Ok(Dispatch::Shutdown(json!({ "ok": true }))),
            "driver.describe" => {
                let session_id = require_str(params, "session_id")?;
                let entry = self.session(&session_id)?;
                let descriptor = json!({
                    "driver_id": self.driver.driver_id(),
                    "display_name": self.driver.display_name(),
                    "server_version": entry.info.server_version.clone(),
                    "capabilities": entry.info.capabilities_json(),
                    "identifier_quote": self.driver.identifier_quote(),
                    "default_schema": entry.info.default_schema.clone(),
                });
                Ok(Dispatch::Result(descriptor))
            }
            "session.open" => {
                let session_id = require_str(params, "session_id")?;
                let driver_id = require_str(params, "driver_id")?;
                if driver_id != self.driver.driver_id() {
                    return Err(DriverError::new(
                        ErrorCode::DriverNotSupported,
                        format!(
                            "驱动 id 不匹配：请求 {driver_id}，本进程只提供 {}",
                            self.driver.driver_id()
                        ),
                    ));
                }
                let conn = params.get("params").cloned().unwrap_or_else(|| json!({}));
                let (session, info) = self.driver.open(&conn).await?;
                let version = info.server_version.clone();
                self.sessions.insert(session_id.clone(), SessionEntry { session, info });
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
                self.driver.close(entry.session).await;
                Ok(Dispatch::Result(json!({})))
            }
            "session.ping" => {
                let session_id = require_str(params, "session_id")?;
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                self.driver.ping(&mut entry.session).await?;
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
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                let page = self
                    .driver
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
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                match method {
                    "tx.begin" => self.driver.begin(&mut entry.session).await?,
                    "tx.commit" => self.driver.commit(&mut entry.session).await?,
                    _ => self.driver.rollback(&mut entry.session).await?,
                }
                Ok(Dispatch::Result(json!({})))
            }
            "meta.catalogs" => {
                let session_id = require_str(params, "session_id")?;
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                let catalogs = self.driver.catalogs(&mut entry.session).await?;
                Ok(Dispatch::Result(json!({ "catalogs": catalogs })))
            }
            "meta.schemas" => {
                let session_id = require_str(params, "session_id")?;
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                let schemas = self.driver.schemas(&mut entry.session).await?;
                Ok(Dispatch::Result(json!({ "schemas": schemas })))
            }
            "meta.objects" => {
                let session_id = require_str(params, "session_id")?;
                let schema = optional_str(params, "schema")
                    .or_else(|| optional_str(params, "catalog"))
                    .unwrap_or_default();
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                let objects = self.driver.objects(&mut entry.session, &schema).await?;
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
                let entry = self.sessions.get_mut(&session_id).ok_or_else(|| {
                    DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
                })?;
                let detail = self.driver.object_detail(&mut entry.session, &schema, &name).await?;
                Ok(Dispatch::Result(serde_json::to_value(detail).unwrap_or_else(|_| json!({}))))
            }
            "meta.routine_source" => Ok(Dispatch::Result(Value::Null)),
            other => Err(DriverError::protocol(format!("未实现的方法：{other}"))),
        }
    }

    fn session(&self, session_id: &str) -> Result<&SessionEntry<D::Session>, DriverError> {
        self.sessions.get(session_id).ok_or_else(|| {
            DriverError::new(ErrorCode::SessionNotFound, format!("会话不存在：{session_id}"))
        })
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
}
