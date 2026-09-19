//! 驱动桥：把一条会话变成能用的驱动（§4.2.2 的方法表）。
//!
//! 这一层只做「把 RPC 收发与线格式翻成 Rust 类型」这一件事：
//!
//! | 谁 | 管什么 |
//! | --- | --- |
//! | [`SidecarSupervisor`](super::supervisor::SidecarSupervisor) | 进程与会话的生死（起 / 收 / 判死 / 排队） |
//! | **本模块** | 会话之上的调用：`driver.describe` / `query.execute` / `query.fetch` / `query.cancel` / `session.ping` |
//! | `engine` 的 `Database` trait | 之后接上：把这里的类型翻成引擎能认的驱动（P1 收尾） |
//!
//! # 承载方式对消费方不可见
//!
//! 一页结果要么走**内联 JSON**（小结果，§4.2.1.2 的两个阈值都满足），要么走 **Arrow 附件**
//! （大结果）。这是**线格式**的选择，调用方只看 [`PageData`]：
//!
//! ```ignore
//! match page.data {
//!     PageData::Rows(rows) => …,     // 小结果
//!     PageData::Arrow(batches) => …, // 大结果，可直接喂 DuckDB / 网格
//! }
//! ```
//!
//! # 两处口径（dev-plan §4.2.2 附录）
//!
//! - `request_id` 由**宿主**生成并在 `query.execute` 里带下去（`query.cancel` 用它取消）；
//!   与 `session_id` 同一条理由：句柄是宿主的资源名。
//! - `type_raw` 同时出现在响应 `columns` 与 Arrow schema metadata（`rds.type_raw`）两处：
//!   这是 §4.2.1 / §4.5.1 两处契约的交叠，**两处不一致时以响应 `columns` 为准并记一条告警**
//!   （Arrow 那份多带 `rds.canonical` / `rds.format`，那些只从 Arrow 取）。

use std::io::Cursor;
use std::time::Duration;

use serde_json::{Value, json};

use super::conn::{CallError, SidecarConn};
use super::proto::RpcErrorCode;
use shared::arrow::ArrowBatch;

/// 查询类调用的传输超时。
///
/// **故意给得长**：查询长短该由 SQL 自己的 `timeout_ms` 与用户的「取消」决定，
/// 不该被线路层按一个拍脑袋的值掐断。
pub const EXECUTE_RPC_TIMEOUT: Duration = Duration::from_secs(600);
/// 元数据 / 描述类调用（应当很快）。
pub const DESCRIBE_RPC_TIMEOUT: Duration = Duration::from_secs(20);
/// 取消：对端只要把标记置上就回，给短一点也无妨，但**不能没有**（否则 UI 会卡住等）。
pub const CANCEL_RPC_TIMEOUT: Duration = Duration::from_secs(15);

/// 驱动桥的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverError {
    /// 对端报了错误（码已识别，见 §4.2.2）。
    Rpc {
        code: RpcErrorCode,
        message: String,
        data: Option<Value>,
    },
    /// 对端报的错误码我们不认识（对端比我们新）。
    UnknownCode { raw_code: i32, message: String },
    /// 调用没在时限内回来（**该请求已被放弃**，对端若之后才回会报成 Issue）。
    Timeout { method: String },
    /// 连接没了（进程崩溃 / 收摊）。
    Disconnected { reason: String },
    /// 对端的回复不符合契约（字段缺失 / 类型不对 / 载荷不是合法 Arrow）。
    Protocol { detail: String },
}

impl DriverError {
    /// 用户主动取消（`-32004`）。
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Rpc {
                code: RpcErrorCode::Cancelled,
                ..
            }
        )
    }

    /// 驱动不支持这个能力（`-32006`）——UI 应当**置灰并说明**，而不是当成「出错了」。
    pub fn is_capability_denied(&self) -> bool {
        matches!(
            self,
            Self::Rpc {
                code: RpcErrorCode::CapabilityDenied,
                ..
            }
        )
    }

    /// SQL 层面的错（`-32003`，带 `sqlstate` / `position`）。
    pub fn is_sql_error(&self) -> bool {
        matches!(
            self,
            Self::Rpc {
                code: RpcErrorCode::SqlError,
                ..
            }
        )
    }

    /// 转成 [`CallError`] 的对应形态（供既有的调用方复用）。
    fn from_call(method: &str, error: CallError) -> Self {
        match error {
            CallError::Rpc {
                code,
                message,
                data,
            } => Self::Rpc {
                code,
                message,
                data,
            },
            CallError::RpcUnknownCode { raw_code, message } => {
                Self::UnknownCode { raw_code, message }
            }
            CallError::Timeout { .. } => Self::Timeout {
                method: method.to_string(),
            },
            CallError::Disconnected { reason } => Self::Disconnected { reason },
            CallError::Closed => Self::Disconnected {
                reason: "连接已关闭".to_string(),
            },
            CallError::Frame { detail } => Self::Protocol { detail },
        }
    }
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rpc {
                code,
                message,
                data,
            } => match data {
                Some(data) => write!(f, "驱动报错 {code}：{message}（{data}）"),
                None => write!(f, "驱动报错 {code}：{message}"),
            },
            Self::UnknownCode { raw_code, message } => {
                write!(f, "驱动报错（未知码 {raw_code}）：{message}")
            }
            Self::Timeout { method } => write!(f, "{method} 超时（该请求已放弃）"),
            Self::Disconnected { reason } => write!(f, "连接已断开：{reason}"),
            Self::Protocol { detail } => write!(f, "对端回复不符合契约：{detail}"),
        }
    }
}

impl std::error::Error for DriverError {}

/// 一列的元信息（响应 `columns` 那一份；`canonical` / `format` 在 Arrow schema 里）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub type_raw: String,
    pub nullable: bool,
}

/// 驱动自述（`driver.describe`）。
///
/// ⚠️ §4.2.2 只给了方法名，没定返回体；P1 先定这几项（够 UI 门控与属性面板用）。
/// **加字段是兼容变更，改/删字段是破坏性变更**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverDescriptor {
    pub driver_id: String,
    pub display_name: String,
    /// 对端（真库）的版本，拿不到就是 `None`。
    pub server_version: Option<String>,
    /// 运行时能力：`schemas` / `views` / `transactions` / `cancel` / `cursor` … → 支不支持。
    ///
    /// 清单里也声明了一份（§4.4）；这里是**运行时**那一份，UI 以它为准（清单可以撒谎，
    /// 跑起来的进程没法撒谎）。
    pub capabilities: std::collections::BTreeMap<String, bool>,
    pub identifier_quote: Option<String>,
    pub default_schema: Option<String>,
}

impl DriverDescriptor {
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.get(capability).copied().unwrap_or(false)
    }
}

/// 一页结果的数据面。
#[derive(Debug, Clone)]
pub enum PageData {
    /// 小结果：内联 JSON 行（`rows` 字段）。
    Rows(Vec<Value>),
    /// 大结果：Arrow 批（已从 IPC 流解出来，可直接喂 DuckDB / 网格）。
    Arrow(Vec<ArrowBatch>),
}

impl PageData {
    /// 行数（按**实际载荷**数，不是对端声称的那个数）。
    pub fn row_count(&self) -> usize {
        match self {
            Self::Rows(rows) => rows.len(),
            Self::Arrow(batches) => batches.iter().map(|b| b.num_rows()).sum(),
        }
    }

    /// 内联 JSON 行（Arrow 载荷返回 `None`）。
    pub fn rows(&self) -> Option<&[Value]> {
        match self {
            Self::Rows(rows) => Some(rows),
            Self::Arrow(_) => None,
        }
    }

    /// Arrow 批（内联载荷返回 `None`）。
    pub fn batches(&self) -> Option<&[ArrowBatch]> {
        match self {
            Self::Arrow(batches) => Some(batches),
            Self::Rows(_) => None,
        }
    }
}

/// 一次查询的结果（`QueryPage`，§4.2.1 的响应体）。
#[derive(Debug, Clone)]
pub struct QueryPage {
    pub columns: Vec<ColumnInfo>,
    /// 对端声称的行数（与 [`PageData::row_count`] 不一致时会记一条告警）。
    pub row_count: u64,
    /// DML 的影响行数（`SELECT` 是 `None`）。
    pub affected_rows: Option<u64>,
    /// 还有没读完的数据（配 `cursor_id` 用 `query.fetch` 继续）。
    pub has_more: bool,
    pub cursor_id: Option<String>,
    /// 因 `max_rows` 截断了。
    pub truncated: bool,
    /// 数据面（内联 JSON 或 Arrow）。
    pub data: PageData,
    /// 对端顺带说的话（警告、`NOTICE` 之类）。
    pub notices: Vec<String>,
}

impl QueryPage {
    /// 实际载荷的行数。
    pub fn rows_here(&self) -> usize {
        self.data.row_count()
    }

    /// 拿走 Arrow 批（这是喂 DuckDB 的那条路，§4.5.2）。
    pub fn into_batches(self) -> Option<Vec<ArrowBatch>> {
        match self.data {
            PageData::Arrow(batches) => Some(batches),
            PageData::Rows(_) => None,
        }
    }
}

/// 查询请求。
#[derive(Debug, Clone)]
pub struct QueryRequest {
    /// 宿主生成的请求 id（`query.cancel` 用它；见模块文档的口径）。
    pub request_id: String,
    pub sql: String,
    /// 绑定参数（线格式是 JSON 数组，类型由对端按驱动规则解释）。
    pub params: Vec<Value>,
    /// 这一页最多要多少行。
    pub max_rows: u64,
    /// 交给对端的语句超时（毫秒）；`None` = 不设。
    pub timeout_ms: Option<u64>,
}

impl QueryRequest {
    pub fn new(request_id: impl Into<String>, sql: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            sql: sql.into(),
            params: Vec::new(),
            max_rows: 1000,
            timeout_ms: None,
        }
    }

    pub fn with_params(mut self, params: Vec<Value>) -> Self {
        self.params = params;
        self
    }

    pub fn with_max_rows(mut self, max_rows: u64) -> Self {
        self.max_rows = max_rows;
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// 一条会话上的驱动调用。
///
/// 借的是**连接**而不是 supervisor：会话的生死由 supervisor 管，这里只管
/// 「这个会话上发什么、回什么」——两者职责分开，借用关系也就简单了。
pub struct SessionDriver<'a> {
    conn: &'a SidecarConn,
    session_id: &'a str,
}

impl<'a> SessionDriver<'a> {
    pub fn new(conn: &'a SidecarConn, session_id: &'a str) -> Self {
        Self { conn, session_id }
    }

    pub fn session_id(&self) -> &str {
        self.session_id
    }

    /// 驱动自述（§4.2.2 `driver.describe`）。
    pub async fn describe(&self, driver_id: &str) -> Result<DriverDescriptor, DriverError> {
        let outcome = self
            .conn
            .call(
                "driver.describe",
                json!({ "driver_id": driver_id }),
                DESCRIBE_RPC_TIMEOUT,
            )
            .await
            .map_err(|e| DriverError::from_call("driver.describe", e))?;
        parse_descriptor(driver_id, &outcome.result)
    }

    /// 执行一条查询（§4.2.2 `query.execute`）。
    pub async fn execute(&self, request: &QueryRequest) -> Result<QueryPage, DriverError> {
        let params = json!({
            "session_id": self.session_id,
            "request_id": request.request_id,
            "sql": request.sql,
            "params": request.params,
            "max_rows": request.max_rows,
            "timeout_ms": request.timeout_ms,
        });
        let outcome = self
            .conn
            .call("query.execute", params, EXECUTE_RPC_TIMEOUT)
            .await
            .map_err(|e| DriverError::from_call("query.execute", e))?;
        parse_page(&outcome.result, &outcome.attachments)
    }

    /// 继续取下一页（§4.2.2 `query.fetch`）。
    pub async fn fetch(
        &self,
        cursor_id: &str,
        offset: u64,
        limit: u64,
    ) -> Result<QueryPage, DriverError> {
        let params = json!({
            "session_id": self.session_id,
            "cursor_id": cursor_id,
            "offset": offset,
            "limit": limit,
        });
        let outcome = self
            .conn
            .call("query.fetch", params, EXECUTE_RPC_TIMEOUT)
            .await
            .map_err(|e| DriverError::from_call("query.fetch", e))?;
        parse_page(&outcome.result, &outcome.attachments)
    }

    /// 取消一条在跑的查询（§4.2.2 `query.cancel`）。
    ///
    /// 取消是**对端的事**：宿主只负责把请求递过去；被取消的那次 `execute` 会以
    /// `-32004 cancelled` 回来（所以这里成功不代表那边已经停了）。
    pub async fn cancel(&self, request_id: &str) -> Result<(), DriverError> {
        let params = json!({ "session_id": self.session_id, "request_id": request_id });
        self.conn
            .call("query.cancel", params, CANCEL_RPC_TIMEOUT)
            .await
            .map(|_| ())
            .map_err(|e| DriverError::from_call("query.cancel", e))
    }

    /// 这条会话还活着吗（§4.2.2 `session.ping`；进程级探活是 supervisor 的 `ping()`）。
    pub async fn session_ping(&self) -> Result<(), DriverError> {
        let params = json!({ "session_id": self.session_id });
        self.conn
            .call("session.ping", params, DESCRIBE_RPC_TIMEOUT)
            .await
            .map(|_| ())
            .map_err(|e| DriverError::from_call("session.ping", e))
    }
}

fn parse_descriptor(driver_id: &str, result: &Value) -> Result<DriverDescriptor, DriverError> {
    let capabilities = result
        .get("capabilities")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
                .collect()
        })
        .unwrap_or_default();

    Ok(DriverDescriptor {
        driver_id: result
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(driver_id)
            .to_string(),
        display_name: result
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(driver_id)
            .to_string(),
        server_version: result
            .get("server_version")
            .and_then(Value::as_str)
            .map(str::to_string),
        capabilities,
        identifier_quote: result
            .get("identifier_quote")
            .and_then(Value::as_str)
            .map(str::to_string),
        default_schema: result
            .get("default_schema")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// 把响应的 `result` + 附件翻成 [`QueryPage`]。
fn parse_page(
    result: &Value,
    attachments: &[super::conn::Attachment],
) -> Result<QueryPage, DriverError> {
    let columns = result
        .get("columns")
        .and_then(Value::as_array)
        .map(|cols| {
            cols.iter()
                .filter_map(|col| {
                    Some(ColumnInfo {
                        name: col.get("name")?.as_str()?.to_string(),
                        type_raw: col
                            .get("type_raw")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string(),
                        nullable: col.get("nullable").and_then(Value::as_bool).unwrap_or(true),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // 数据面：声明了附件就走 Arrow；否则看内联 rows。两者都没有 = 空结果（只有当列也在时才合法）
    let data = if !attachments.is_empty() {
        let mut batches = Vec::new();
        for attachment in attachments {
            batches.extend(parse_arrow_stream(&attachment.payload)?);
        }
        PageData::Arrow(batches)
    } else {
        let rows = result
            .get("rows")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        PageData::Rows(rows)
    };

    if columns.is_empty() && result.get("columns").is_none() {
        return Err(DriverError::Protocol {
            detail: "响应里没有 columns（空结果集也要带 schema）".to_string(),
        });
    }

    let page = QueryPage {
        columns,
        row_count: result
            .get("row_count")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| data.row_count() as u64),
        affected_rows: result.get("affected_rows").and_then(Value::as_u64),
        has_more: result
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cursor_id: result
            .get("cursor_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        truncated: result
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        data,
        notices: result
            .get("notices")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    };

    // 对端声称的行数与实际载荷不一致：不阻断（上层照实际数据画），但要留痕 ——
    // 这种不一致正是"结果少了一半"这类 bug 的第一现场。
    let actual = page.rows_here() as u64;
    if page.row_count != actual {
        tracing::warn!(
            claimed = page.row_count,
            actual,
            "对端声称的行数与实际载荷对不上"
        );
    }
    Ok(page)
}

/// 解一条自洽的 Arrow IPC 流（§4.2.4：每页一条 stream，自带 schema）。
fn parse_arrow_stream(bytes: &[u8]) -> Result<Vec<ArrowBatch>, DriverError> {
    let reader =
        arrow::ipc::reader::StreamReader::try_new(Cursor::new(bytes), None).map_err(|e| {
            DriverError::Protocol {
                detail: format!("Arrow 流解不开：{e}"),
            }
        })?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DriverError::Protocol {
            detail: format!("Arrow 批读不出来：{e}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_data_counts_by_actual_payload() {
        let rows = PageData::Rows(vec![json!({ "a": 1 }), json!({ "a": 2 })]);
        assert_eq!(rows.row_count(), 2);
        assert!(rows.rows().is_some() && rows.batches().is_none());

        let empty = PageData::Arrow(Vec::new());
        assert_eq!(empty.row_count(), 0);
        assert!(empty.rows().is_none() && empty.batches().is_some());
    }

    /// 空结果集**也要有 schema**：只有 columns 没有 rows/附件是合法的，缺 columns 不是。
    #[test]
    fn an_empty_result_still_needs_columns() {
        let with_columns = json!({
            "columns": [{ "name": "id", "type_raw": "int8", "nullable": false }],
            "row_count": 0,
        });
        let page = parse_page(&with_columns, &[]).expect("空结果集应当合法");
        assert_eq!(page.rows_here(), 0);
        assert_eq!(page.columns.len(), 1);
        assert!(!page.has_more);

        let without_columns = json!({ "row_count": 0 });
        assert!(matches!(
            parse_page(&without_columns, &[]),
            Err(DriverError::Protocol { .. })
        ));
    }

    #[test]
    fn call_errors_map_to_typed_driver_errors() {
        let cancelled = DriverError::from_call(
            "query.execute",
            CallError::Rpc {
                code: RpcErrorCode::Cancelled,
                message: "用户取消".into(),
                data: None,
            },
        );
        assert!(cancelled.is_cancelled() && !cancelled.is_sql_error());

        let denied = DriverError::from_call(
            "query.execute",
            CallError::Rpc {
                code: RpcErrorCode::CapabilityDenied,
                message: "驱动不支持事务".into(),
                data: Some(json!({ "required": "transactions" })),
            },
        );
        assert!(denied.is_capability_denied());
        assert!(denied.to_string().contains("transactions"));

        let unknown = DriverError::from_call(
            "query.execute",
            CallError::RpcUnknownCode {
                raw_code: -32099,
                message: "对端比我们新".into(),
            },
        );
        assert!(matches!(unknown, DriverError::UnknownCode { raw_code, .. } if raw_code == -32099));

        let gone = DriverError::from_call(
            "query.execute",
            CallError::Disconnected {
                reason: "对端退出".into(),
            },
        );
        assert!(matches!(gone, DriverError::Disconnected { .. }));
    }

    #[test]
    fn descriptor_parses_runtime_capabilities() {
        let result = json!({
            "id": "mssql",
            "display_name": "SQL Server",
            "server_version": "16.0",
            "capabilities": { "transactions": true, "cursor": false },
            "identifier_quote": "\"",
            "default_schema": "dbo",
        });
        let descriptor = parse_descriptor("mssql", &result).unwrap();
        assert_eq!(descriptor.display_name, "SQL Server");
        assert_eq!(descriptor.server_version.as_deref(), Some("16.0"));
        assert!(descriptor.supports("transactions"));
        assert!(!descriptor.supports("cursor"));
        // 没声明的能力按**不支持**（不得静默退化）
        assert!(!descriptor.supports("streaming"));
        assert_eq!(descriptor.default_schema.as_deref(), Some("dbo"));
    }
}
