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
use super::meta::{
    MetaObject, MetaObjectDetail, MetaObjectKind, MetaSchema, parse_catalogs,
    parse_object_detail, parse_objects, parse_routine_source, parse_schemas,
};
use super::proto::RpcErrorCode;
use super::supervisor::SidecarSupervisor;
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

    /// 转成 [`CallError`] 的对应形态（同 crate 的其它层也要用：supervisor 报的是 `CallError`）。
    pub(crate) fn from_call(method: &str, error: CallError) -> Self {
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

    /// 数据面统一成 Arrow 批（喂 DuckDB 的那条路，§4.5.2）。
    ///
    /// 内联 JSON 那条线格式在这里补成 \`RecordBatch\`：**承载方式对消费方不可见**
    /// （§4.2.1.2），所以这里不该有"内联就没批"这种分支。
    pub fn into_batches(self) -> Result<Vec<ArrowBatch>, DriverError> {
        let QueryPage { columns, data, .. } = self;
        normalize_batches(&columns, data)
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

    /// 元数据：catalog 清单（§4.2.2 `meta.catalogs`）。
    pub async fn meta_catalogs(&self) -> Result<Vec<String>, DriverError> {
        let result = self
            .call_meta("meta.catalogs", json!({ "session_id": self.session_id }))
            .await?;
        parse_catalogs(&result)
    }

    /// 元数据：某 catalog 下的 schema（`meta.schemas`）。
    pub async fn meta_schemas(&self, catalog: &str) -> Result<Vec<MetaSchema>, DriverError> {
        let result = self
            .call_meta(
                "meta.schemas",
                json!({ "session_id": self.session_id, "catalog": catalog }),
            )
            .await?;
        parse_schemas(&result)
    }

    /// 元数据：某 schema 下的**全部**对象，含类别（`meta.objects`）。
    ///
    /// 「一次给全」是硬口径（§4.2.2）：五个文件夹共用这一次内省，宿主按 `kind` 分流。
    /// 按文件夹分家的话，宿主要跑五遍内省 —— 而它们在内省层面本来就是一条查询。
    pub async fn meta_objects(
        &self,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        let result = self
            .call_meta(
                "meta.objects",
                json!({
                    "session_id": self.session_id,
                    "catalog": catalog,
                    "schema": schema,
                }),
            )
            .await?;
        parse_objects(&result)
    }

    /// 元数据：一个对象的详情（`meta.object_detail`）—— 列、索引数、行数估计。
    pub async fn meta_object_detail(
        &self,
        catalog: &str,
        schema: &str,
        object: &str,
    ) -> Result<MetaObjectDetail, DriverError> {
        let result = self
            .call_meta(
                "meta.object_detail",
                json!({
                    "session_id": self.session_id,
                    "catalog": catalog,
                    "schema": schema,
                    "object": object,
                }),
            )
            .await?;
        parse_object_detail(&result)
    }

    /// 元数据：例程（过程 / 函数）源码（`meta.routine_source`）；对端查不到返回 `None`。
    pub async fn meta_routine_source(
        &self,
        catalog: &str,
        schema: &str,
        name: &str,
    ) -> Result<Option<String>, DriverError> {
        let result = self
            .call_meta(
                "meta.routine_source",
                json!({
                    "session_id": self.session_id,
                    "catalog": catalog,
                    "schema": schema,
                    "name": name,
                }),
            )
            .await?;
        parse_routine_source(&result)
    }

    /// 元数据调用的公共部分。
    ///
    /// 一律用 [DESCRIBE_RPC_TIMEOUT]：内省该是快的，慢到超时说明对端那条内省查询有问题，
    /// 该如实报出来而不是让界面一直转圈。**不收附件** —— 元数据是控制面，
    /// 带 Arrow 附件说明对端把两条路搅在一起了（那是我们没法按语义解释的响应）。
    async fn call_meta(&self, method: &'static str, params: Value) -> Result<Value, DriverError> {
        let outcome = self
            .conn
            .call(method, params, DESCRIBE_RPC_TIMEOUT)
            .await
            .map_err(|e| DriverError::from_call(method, e))?;
        if !outcome.attachments.is_empty() {
            return Err(DriverError::Protocol {
                detail: format!(
                    "{method} 的响应带了 {} 个 Arrow 附件（元数据是控制面，不走数据面）",
                    outcome.attachments.len()
                ),
            });
        }
        Ok(outcome.result)
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

// ==================== 接引擎的 `Database` trait ====================
//
// 上面那层是「协议怎么说话」，这一层是「引擎怎么用它」：把一条会话包成
// `engine::driver::Database`，于是连接面板 / 编辑器 / 导航树那些既有路径不必知道
// sidecar 的存在。
//
// 三条口径：
//
// 1. **只填 `batches`**（架构 §12 #21）：`rows` / `total_rows` 两个字段是前端 JSON 契约，
//    驱动去填它们会得到"大结果有数、小结果无数"。内联 JSON 那条线格式在这里**补成
//    `RecordBatch`**（§4.2.1.2 的「承载方式对消费方不可见」），消费方只看一种表示。
// 2. 连接是**弱引用**：会话的生死归 supervisor，这个句柄只是借用；进程被收掉之后
//    这里明确报「连接已不可用」，而不是抱着一个陈旧进程不放。
// 3. 取消是**真取消**：把 `query.cancel` 递到驱动进程（它回 `-32004`），而不是把 future
//    丢掉 —— 丢掉只是这边不再等，对端那条查询还在跑（§3.2 列的 sidecar 优势之一）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use engine::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, DynDatabase, IndexDetail, NodeInfo,
    SchemaObjectKind, Transaction,
};
use shared::error::{CommonError, ConnectionError, CoreError, DatabaseError, PluginError};
use shared::models::{QueryResult, Value as SharedValue};

impl QueryPage {
    /// 翻成引擎的结果模型（**只填 `batches`**，见本节口径 1）。
    pub fn into_query_result(self) -> Result<QueryResult, DriverError> {
        let QueryPage {
            columns,
            affected_rows,
            data,
            ..
        } = self;

        let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        let types: Vec<String> = columns.iter().map(|c| c.type_raw.clone()).collect();
        let batches = normalize_batches(&columns, data)?;

        let total_rows = batches.iter().map(|b| b.num_rows() as u32).sum();
        Ok(QueryResult {
            columns: names,
            column_types: types,
            // 前端 JSON 契约的那两个字段由上层按 `batches` 现算（架构 §12 #21），驱动不填
            rows: Vec::new(),
            total_rows,
            batches,
            affected_rows: affected_rows.map(|n| n as u32),
            // 影响行数只有写语句才会报：拿它当"是否只读"的提示（不复制第六份 SQL 首词启发式）
            is_read_only: Some(affected_rows.is_none()),
        })
    }
}

/// 从一次 `meta.objects` 里挑出一类对象，翻成引擎的对象项。
///
/// 认不出的类别在 [`MetaObject::into_node_info`] 里被跳过并留痕（导航摆不下它）。
fn take_kind(objects: Vec<MetaObject>, want: fn(&MetaObjectKind) -> bool) -> Vec<NodeInfo> {
    objects
        .into_iter()
        .filter(|o| want(&o.kind))
        .filter_map(MetaObject::into_node_info)
        .collect()
}

/// 数据面归一：Arrow 附件原样拿走，内联 JSON 补成一个批。
fn normalize_batches(
    columns: &[ColumnInfo],
    data: PageData,
) -> Result<Vec<ArrowBatch>, DriverError> {
    match data {
        PageData::Arrow(batches) => Ok(batches),
        PageData::Rows(rows) => Ok(vec![json_rows_to_batch(columns, &rows)?]),
    }
}

/// 一条 sidecar 会话包成的引擎驱动。
pub struct SidecarDatabase {
    /// **弱引用**：会话的生死归 supervisor（口径 2）。
    conn: Weak<SidecarConn>,
    session_id: String,
    /// 驱动类型（引擎侧用它选图标与文案；与 `MissingDriver` 等同一个口径）。
    db_type: String,
    /// `driver.describe` 的返回：能力、服务端版本都从这里取（清单可以撒谎，跑起来的进程不会）。
    descriptor: DriverDescriptor,
    /// 请求 id 发号器（`query.cancel` 用它；见模块文档的口径）。
    next_request: AtomicU64,
    /// 谁负责关会话。
    ///
    /// 弱引用只是**借**连接，不负责收摊：句柄被丢掉时得有人把会话交还给内核，
    /// 否则那条会话会一直占着实例（串行规格下会把后续连接全排住）。
    /// 工厂路径会带上 supervisor（见 [`SidecarDatabase::owned_by`]）；
    /// 手工构造（测试 / 借用式用法）可以不带，由调用方自己安排收摊。
    owner: Option<Arc<Mutex<SidecarSupervisor>>>,
}

impl SidecarDatabase {
    pub fn new(
        conn: &Arc<SidecarConn>,
        session_id: impl Into<String>,
        db_type: impl Into<String>,
        descriptor: DriverDescriptor,
    ) -> Self {
        Self {
            conn: Arc::downgrade(conn),
            session_id: session_id.into(),
            db_type: db_type.into(),
            descriptor,
            next_request: AtomicU64::new(1),
            owner: None,
        }
    }

    /// 指定「谁负责关会话」：句柄被丢掉时把会话交还给内核（工厂路径都这么用）。
    pub fn owned_by(mut self, supervisor: Arc<Mutex<SidecarSupervisor>>) -> Self {
        self.owner = Some(supervisor);
        self
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn descriptor(&self) -> &DriverDescriptor {
        &self.descriptor
    }

    /// 交给引擎的形态（注册表只收 `Arc<dyn Database + Send + Sync>`）。
    pub fn into_dyn(self) -> DynDatabase {
        Arc::new(self)
    }

    fn connection(&self) -> Result<Arc<SidecarConn>, CoreError> {
        self.conn.upgrade().ok_or_else(|| {
            CoreError::connection(ConnectionError::Network {
                conn_id: self.session_id.clone(),
                reason: format!("驱动 {} 的连接已经收掉了（会话失效）", self.db_type),
            })
        })
    }

    fn next_request_id(&self) -> String {
        let n = self.next_request.fetch_add(1, Ordering::Relaxed);
        format!("{}-{n}", self.session_id)
    }

    async fn run(
        &self,
        sql: &str,
        params: Vec<Value>,
        request_id: String,
    ) -> Result<QueryResult, CoreError> {
        let request = QueryRequest::new(request_id, sql).with_params(params);
        let conn = self.connection()?;
        let page = SessionDriver::new(&conn, &self.session_id)
            .execute(&request)
            .await
            .map_err(|e| self.map_error(sql, e))?;

        // 对端顺带说的话（警告 / NOTICE）：P1 只落日志 —— `QueryResult` 没有承载位，
        // 等 P2.5 的 `ResultSet` 补上（那时它才有一个像样的去处）。
        if !page.notices.is_empty() {
            tracing::debug!(session_id = %self.session_id, notices = ?page.notices, "驱动侧提示");
        }

        page.into_query_result().map_err(|e| self.map_error(sql, e))
    }

    /// 「索引 / 约束明细还没接」的那句错话（两处用，措辞要一致）。
    fn no_index_detail(&self, what: &str) -> CoreError {
        CoreError::common(CommonError::not_supported(format!(
            "驱动 {} 的{what}明细还没接（P2 的 meta 面只给个数）",
            self.db_type
        )))
    }

    /// 线格式里的 schema 名。
    ///
    /// 没给 schema 时用 `driver.describe` 的 `default_schema`；两者都没有就传**空串** ——
    /// 空串是有含义的：「这个驱动没有 schema 层，用你自己的默认」（MySQL / SQLite / DuckDB 类，
    /// 见 dev-plan §4.4 的 `schemas` 能力）。宿主**不替驱动猜**一个 schema 名。
    fn wire_schema(&self, schema: Option<&str>) -> String {
        schema
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| self.descriptor.default_schema.clone())
            .unwrap_or_default()
    }

    /// 一次 `meta.objects`。
    ///
    /// 五个文件夹（表 / 视图 / 例程 / 序列 / 触发器）都从这里挑，因为协议规定它**一次给全**：
    /// 按文件夹分家就要跑五遍内省，而它们在内省层面本来就是一条查询。代价是展开一个 schema
    /// 会按文件夹各问一次（导航是逐文件夹收集的）；驱动侧那条内省该是快的，重复展开由
    /// 导航的 L1 / L2 缓存挡住。
    async fn objects_of(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<MetaObject>, CoreError> {
        let conn = self.connection()?;
        let schema = self.wire_schema(schema);
        SessionDriver::new(&conn, &self.session_id)
            .meta_objects(catalog, &schema)
            .await
            .map_err(|e| self.map_error("meta.objects", e))
    }

    /// 驱动错误 → 引擎错误域的映射。
    ///
    /// 词表是既有的（`shared::error`）：SQL 错进 `DatabaseError::Query`（带出错位置，
    /// 光标能跳过去），连接没了进 `ConnectionError`，能力缺失进 `CommonError::NotSupported`
    /// （UI 据此置灰），插件自身说话不算话进 `PluginError`。
    fn map_error(&self, sql: &str, error: DriverError) -> CoreError {
        let db_type = self.db_type.clone();
        match error {
            DriverError::Rpc {
                code,
                message,
                data,
            } => match code {
                RpcErrorCode::SqlError => {
                    let mut mapped = DatabaseError::query(sql, message);
                    // 协议给的是 **1 基字符位置**（各家数据库的口径），引擎要的是 0 基字节偏移
                    if let Some(position) = data
                        .as_ref()
                        .and_then(|d| d.get("position"))
                        .and_then(Value::as_u64)
                        && let Some(offset) =
                            engine::driver::utils::byte_offset_for_char(sql, position as usize)
                    {
                        mapped = mapped.with_position(offset);
                    }
                    CoreError::database(mapped)
                }
                RpcErrorCode::Cancelled => {
                    CoreError::database(DatabaseError::query(sql, "查询已被取消"))
                }
                RpcErrorCode::Timeout => CoreError::database(DatabaseError::query(
                    sql,
                    "驱动报超时（-32005）：语句超过对端设定的时限",
                )),
                RpcErrorCode::CapabilityDenied => CoreError::common(CommonError::not_supported(
                    format!("驱动 {db_type} 不支持该能力：{message}"),
                )),
                RpcErrorCode::DriverNotSupported => {
                    CoreError::connection(ConnectionError::DriverNotFound { driver: db_type })
                }
                RpcErrorCode::SessionNotFound => {
                    CoreError::connection(ConnectionError::NoActiveConnection)
                }
                // 连不上目标库（P2 补的码）：与「原本连着、现在断了」分开 ——
                // 前者是「没连上」，后者是「掉线」，界面上的动作不一样（改配置 vs 重连）。
                // 没连上的细因（拒连 / 口令 / TLS）由驱动写在 message 里：宿主**不按子串猜**
                // 分类（§3.5 参考实现就是靠 containsAny 猜的，那套我们不要）。
                RpcErrorCode::ConnectFailed => CoreError::connection(ConnectionError::Refused {
                    conn_id: self.session_id.clone(),
                    reason: format!("连不上 {db_type}：{message}"),
                }),
                RpcErrorCode::ProtocolVersionMismatch | RpcErrorCode::ResourceLimit => {
                    CoreError::database(DatabaseError::Driver {
                        db_type,
                        operation: "protocol".to_string(),
                        source: message,
                    })
                }
            },
            DriverError::UnknownCode { raw_code, message } => {
                CoreError::database(DatabaseError::Driver {
                    db_type,
                    operation: format!("rpc({raw_code})"),
                    source: message,
                })
            }
            DriverError::Timeout { method } => CoreError::database(DatabaseError::query(
                sql,
                format!("{method} 超时（该请求已放弃，对端若之后才回会报成 Issue）"),
            )),
            DriverError::Disconnected { reason } => {
                CoreError::connection(ConnectionError::Network {
                    conn_id: self.session_id.clone(),
                    reason,
                })
            }
            DriverError::Protocol { detail } => CoreError::plugin(PluginError::ExecutionFailed {
                plugin_id: db_type,
                function: "query.execute".to_string(),
                reason: detail,
            }),
        }
    }
}

#[async_trait::async_trait]
impl Database for SidecarDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        self.run(sql, Vec::new(), self.next_request_id()).await
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<SharedValue>,
    ) -> Result<QueryResult, CoreError> {
        let wire = params
            .iter()
            .map(shared_value_to_json)
            .collect::<Result<Vec<_>, _>>()?;
        self.run(sql, wire, self.next_request_id()).await
    }

    async fn query_with_cancel(
        &self,
        sql: &str,
        cancel_token: CancellationToken,
    ) -> Result<QueryResult, CoreError> {
        let request = QueryRequest::new(self.next_request_id(), sql);
        let conn = self.connection()?;
        let driver = SessionDriver::new(&conn, &self.session_id);

        // 真取消（口径 3）：令牌响了就把取消**递到驱动进程**，然后继续等这次调用以
        // `-32004` 收场 —— 不能把 future 丢掉，那只是这边不等了，对端还在跑。
        let execute = driver.execute(&request);
        tokio::pin!(execute);
        let page = tokio::select! {
            biased;
            result = &mut execute => result,
            _ = cancel_token.cancelled() => {
                if let Err(e) = driver.cancel(&request.request_id).await {
                    tracing::warn!(session_id = %self.session_id, error = %e, "取消没能递到驱动进程");
                }
                execute.await
            }
        };

        let page = page.map_err(|e| self.map_error(sql, e))?;
        page.into_query_result().map_err(|e| self.map_error(sql, e))
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError> {
        // `tx.begin/commit/rollback` 是 P2 的事（P1 的 RPC 面只到 `query.*`）。
        // **明确报不支持**，不静默退化成"没有事务"——那样用户会在库里留下一堆半截写入。
        Err(CoreError::common(CommonError::not_supported(format!(
            "驱动 {} 的事务桥还没接（tx.* 属 P2）",
            self.db_type
        ))))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.descriptor.server_version.clone(),
            supports_transaction: self.descriptor.supports("transactions"),
            supports_streaming: self.descriptor.supports("streaming"),
            // 数据面就是 Arrow —— 这一项与原生驱动（false）恰好相反，也是本方案的核心
            supports_arrow: true,
            // 联邦是宿主 DuckDB 的事，不是单个驱动的事
            supports_federated: false,
            supports_concurrent_write: self.descriptor.supports("concurrent_write"),
            is_in_memory: false,
        }
    }

    /* ===== 对象树能力（导航树 / 属性面板 / `#` 内容档）===== */

    /// 全部走 `meta.*`（P2 落地，§4.2.2.1）；错误按域映射，能力的门控在 P2 收尾那一刀接。

    /// 列举 catalog。
    ///
    /// 协议口径：**拿不出 catalog 概念的驱动把 schema 名当 catalog 回** —— 导航第一层不能是空的
    /// （MySQL / SQLite / DuckDB 的原生驱动就是这么做的）。
    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        let conn = self.connection()?;
        SessionDriver::new(&conn, &self.session_id)
            .meta_catalogs()
            .await
            .map_err(|e| self.map_error("meta.catalogs", e))
    }

    async fn list_schemas(&self, catalog: &str) -> Result<Vec<String>, CoreError> {
        let conn = self.connection()?;
        SessionDriver::new(&conn, &self.session_id)
            .meta_schemas(catalog)
            .await
            .map(|schemas| schemas.into_iter().map(|s| s.name).collect())
            .map_err(|e| self.map_error("meta.schemas", e))
    }

    /// 表与视图一次拿全：导航按 `kind == View` 把这一份结果分成两个文件夹
    /// （`NavigatorService::collect_objects`），所以这里**两类都要返回**。
    async fn list_tables(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(take_kind(self.objects_of(catalog, schema).await?, |k| {
            k.is_table_like()
        }))
    }

    /// 列（展开表 / 属性面板）。
    ///
    /// 属性面板显示的是**原始类型名**（`numeric(38,10)`），归一化类型在 `extra.canonical`
    /// 里 —— 「给用户看的」与「给程序判的」不互相迁就（§4.5.1）。
    async fn list_columns(
        &self,
        catalog: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let conn = self.connection()?;
        let schema = self.wire_schema(schema);
        SessionDriver::new(&conn, &self.session_id)
            .meta_object_detail(catalog, &schema, table)
            .await
            .map(|detail| {
                detail
                    .columns
                    .into_iter()
                    .map(|c| c.into_column_detail())
                    .collect()
            })
            .map_err(|e| self.map_error("meta.object_detail", e))
    }

    async fn list_procedures(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(take_kind(self.objects_of(catalog, schema).await?, |k| {
            matches!(k, MetaObjectKind::Procedure)
        }))
    }

    async fn list_functions(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(take_kind(self.objects_of(catalog, schema).await?, |k| {
            matches!(k, MetaObjectKind::Function)
        }))
    }

    async fn list_sequences(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(take_kind(self.objects_of(catalog, schema).await?, |k| {
            matches!(k, MetaObjectKind::Sequence)
        }))
    }

    /// 触发器（带所属表：`NodeInfo::parent_name`，属性面板与缓存都靠它）。
    async fn list_triggers(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(take_kind(self.objects_of(catalog, schema).await?, |k| {
            matches!(k, MetaObjectKind::Trigger)
        }))
    }

    /// 例程源码。`kind` 不往协议里带：名字是驱动的唯一标识，它自己清楚那是过程还是函数
    /// （原生驱动也把这一位当摆设）。
    async fn get_routine_source(
        &self,
        catalog: &str,
        schema: Option<&str>,
        name: &str,
        kind: SchemaObjectKind,
    ) -> Result<Option<String>, CoreError> {
        if !matches!(
            kind,
            SchemaObjectKind::Procedure | SchemaObjectKind::Function
        ) {
            // 表 / 视图没有源码可看：**不装作查到了**，也不白跑一趟 RPC
            return Ok(None);
        }
        let conn = self.connection()?;
        let schema = self.wire_schema(schema);
        SessionDriver::new(&conn, &self.session_id)
            .meta_routine_source(catalog, &schema, name)
            .await
            .map_err(|e| self.map_error("meta.routine_source", e))
    }

    /// 索引 / 约束的**明细**：协议面还没定（`meta.object_detail` 只给个数）。
    ///
    /// 如实报不支持，**不返回空** —— 「没有索引」与「没这个能力」在界面上的意思完全不同
    /// （§4.4 的不得静默退化）。明细面留给 P4（属性面板要列的时候一起定）。
    async fn list_indexes(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        Err(self.no_index_detail("索引"))
    }

    async fn list_constraints(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        Err(self.no_index_detail("约束"))
    }

    async fn ping(&self) -> Result<(), CoreError> {
        let conn = self.connection()?;
        SessionDriver::new(&conn, &self.session_id)
            .session_ping()
            .await
            .map_err(|e| self.map_error("session.ping", e))
    }
}

impl Drop for SidecarDatabase {
    fn drop(&mut self) {
        let Some(supervisor) = self.owner.take() else {
            return;
        };
        let session_id = self.session_id.clone();

        // `Drop` 不能 await：把交还交给运行时。没有运行时就说清楚它会怎样 ——
        // 不装作收过了（那种"看起来成功"最坏事）。
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    let mut supervisor = supervisor.lock().await;
                    if let Err(e) = supervisor
                        .close_session(&session_id, std::time::Instant::now())
                        .await
                    {
                        tracing::warn!(session_id, error = %e, "连接句柄释放时没能交还会话");
                    }
                });
            }
            Err(_) => tracing::warn!(
                session_id,
                "没有异步运行时，会话没能立刻交还（进程收摊时会一并收掉）"
            ),
        }
    }
}

/// `shared::Value` → 线格式的 JSON。
///
/// `Bytes` 与 `NaN` **明确拒绝**：线格式是 JSON，二进制要先定一种编码（base64？数组？），
/// 这个口径还没定 —— 与其塞个看起来能用的东西过去，不如当场报错（§12 的「不得静默退化」）。
fn shared_value_to_json(value: &SharedValue) -> Result<Value, CoreError> {
    Ok(match value {
        SharedValue::Null => Value::Null,
        SharedValue::Bool(b) => Value::Bool(*b),
        SharedValue::Int(i) => json!(i),
        SharedValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .ok_or_else(|| {
                CoreError::common(CommonError::invalid_argument(
                    "params",
                    "NaN / Infinity 不是合法的 JSON 数字",
                ))
            })?,
        SharedValue::Text(t) => Value::String(t.clone()),
        SharedValue::Bytes(_) => {
            return Err(CoreError::common(CommonError::not_supported(
                "二进制参数：P1 的线格式是 JSON，bytes 要先定一种编码",
            )));
        }
    })
}

/// 内联 JSON 行 → 一个 `RecordBatch`。
///
/// 类型按**值的形状**逐列推断：内联那条路只带 `type_raw` 文本，不带宽带类型信息；
/// 保真的类型映射走 Arrow 附件（schema 里带 `rds.*` metadata，§4.5.1）。
/// 同一列混排（既有数又有文本）一律退化成文本 —— 宁可显示成文本，也不要静默丢值。
fn json_rows_to_batch(columns: &[ColumnInfo], rows: &[Value]) -> Result<ArrowBatch, DriverError> {
    if columns.is_empty() {
        return Err(DriverError::Protocol {
            detail: "内联结果没有列定义".to_string(),
        });
    }

    use arrow::array::{Array, BooleanArray, Float64Array, Int64Array, LargeStringArray};
    use arrow::datatypes::{DataType, Field, Schema};

    let mut fields: Vec<Field> = Vec::with_capacity(columns.len());
    let mut arrays: Vec<Arc<dyn Array>> = Vec::with_capacity(columns.len());

    for (index, column) in columns.iter().enumerate() {
        let cells: Vec<Option<&Value>> = rows
            .iter()
            .map(|row| cell_of(row, index, &column.name))
            .collect::<Result<_, _>>()?;

        let (data_type, array): (DataType, Arc<dyn Array>) = match column_kind(&cells) {
            JsonColumnKind::Boolean => {
                let values: Vec<Option<bool>> =
                    cells.iter().map(|c| c.and_then(|v| v.as_bool())).collect();
                (DataType::Boolean, Arc::new(BooleanArray::from(values)))
            }
            JsonColumnKind::Int64 => {
                let values: Vec<Option<i64>> =
                    cells.iter().map(|c| c.and_then(Value::as_i64)).collect();
                (DataType::Int64, Arc::new(Int64Array::from(values)))
            }
            JsonColumnKind::Float64 => {
                let values: Vec<Option<f64>> =
                    cells.iter().map(|c| c.and_then(Value::as_f64)).collect();
                (DataType::Float64, Arc::new(Float64Array::from(values)))
            }
            JsonColumnKind::Text => {
                let values: Vec<Option<String>> =
                    cells.iter().map(|c| c.map(cell_to_text)).collect();
                (
                    DataType::LargeUtf8,
                    Arc::new(LargeStringArray::from(values)),
                )
            }
        };

        fields.push(Field::new(&column.name, data_type, column.nullable));
        arrays.push(array);
    }

    ArrowBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(|e| DriverError::Protocol {
        detail: format!("内联结果装不成 RecordBatch：{e}"),
    })
}

/// 取第 `index` 列的值；`None` = 该行没有这一列（或值为 null）。
///
/// **内联行必须是对象**（列名做键）：位置数组不是本协议的线格式，遇到了要当场说，
/// 而不是猜一个顺序继续跑。
fn cell_of<'a>(row: &'a Value, index: usize, name: &str) -> Result<Option<&'a Value>, DriverError> {
    match row {
        Value::Object(map) => Ok(map.get(name).filter(|value| !value.is_null())),
        other => Err(DriverError::Protocol {
            detail: format!(
                "内联行必须是对象（列名做键），第 {} 行给的是 {}",
                index + 1,
                match other {
                    Value::Array(_) => "数组".to_string(),
                    other => other.to_string(),
                }
            ),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonColumnKind {
    Int64,
    Float64,
    Boolean,
    Text,
}

/// 逐列推断内联 JSON 的类型（全 null 的列没有类型信息可依 → 文本）。
fn column_kind(cells: &[Option<&Value>]) -> JsonColumnKind {
    let (mut ints, mut numbers, mut bools, mut any) = (true, true, true, false);
    for cell in cells.iter().flatten() {
        any = true;
        match cell {
            Value::Bool(_) => {
                ints = false;
                numbers = false;
            }
            Value::Number(n) => {
                bools = false;
                if n.as_i64().is_none() && n.as_u64().is_none() {
                    ints = false;
                }
            }
            _ => {
                ints = false;
                numbers = false;
                bools = false;
            }
        }
    }

    match (any, bools, ints, numbers) {
        (false, ..) => JsonColumnKind::Text,
        (_, true, ..) => JsonColumnKind::Boolean,
        (_, _, true, _) => JsonColumnKind::Int64,
        (_, _, _, true) => JsonColumnKind::Float64,
        _ => JsonColumnKind::Text,
    }
}

/// 退化成文本时怎么显示：数字 / 布尔按字面量，嵌套结构按 JSON 串（§4.2.4 的口径）。
fn cell_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(_) | Value::Array(_) => value.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;

    fn columns(names: &[(&str, &str)]) -> Vec<ColumnInfo> {
        names
            .iter()
            .map(|(name, type_raw)| ColumnInfo {
                name: (*name).to_string(),
                type_raw: (*type_raw).to_string(),
                nullable: true,
            })
            .collect()
    }

    /// 内联那条路的分内事：把 JSON 行补成 `RecordBatch`，且列序 / 类型对得上。
    #[test]
    fn inline_rows_become_a_record_batch() {
        let columns = columns(&[
            ("id", "bigint"),
            ("name", "varchar(64)"),
            ("amount", "numeric(38,10)"),
            ("flag", "boolean"),
        ]);
        let rows = vec![
            json!({ "id": 1, "name": "row-1", "amount": 1.5, "flag": false }),
            json!({ "id": 2, "name": "row-2", "amount": 3.0, "flag": true }),
        ];

        let batch = json_rows_to_batch(&columns, &rows).expect("应当装得出批");
        assert_eq!(batch.num_rows(), 2);
        let schema = batch.schema();
        let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(names, vec!["id", "name", "amount", "flag"]);
        // 整数列就是 Int64（不是文本）：网格要按类型排 / 算，不能全是字符串
        assert_eq!(
            schema.field_with_name("id").unwrap().data_type(),
            &arrow::datatypes::DataType::Int64
        );
        assert_eq!(
            schema.field_with_name("name").unwrap().data_type(),
            &arrow::datatypes::DataType::LargeUtf8
        );
        assert_eq!(
            schema.field_with_name("amount").unwrap().data_type(),
            &arrow::datatypes::DataType::Float64
        );
        assert_eq!(
            schema.field_with_name("flag").unwrap().data_type(),
            &arrow::datatypes::DataType::Boolean
        );

        use arrow::array::{Array, Int64Array};
        let ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(ids.value(0), 1);
        assert_eq!(ids.value(1), 2);
    }

    /// 同一列混排 → 退化成文本（宁可显示成文本，也不要静默丢值）；全 null 列同理。
    #[test]
    fn mixed_and_all_null_columns_degrade_to_text() {
        let columns = columns(&[("mixed", "text"), ("empty", "text")]);
        let rows = vec![
            json!({ "mixed": 1, "empty": null }),
            json!({ "mixed": "一", "empty": null }),
        ];
        let batch = json_rows_to_batch(&columns, &rows).unwrap();
        assert_eq!(
            batch.schema().field_with_name("mixed").unwrap().data_type(),
            &arrow::datatypes::DataType::LargeUtf8
        );
        let mixed = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow::array::LargeStringArray>()
            .unwrap();
        assert_eq!(mixed.value(0), "1");
        assert_eq!(mixed.value(1), "一");
        // 全 null：装得出来，且都是 null
        assert!(batch.column(1).is_null(0) && batch.column(1).is_null(1));
    }

    /// 位置的数组行不是本协议的线格式：当场说，不要猜顺序。
    #[test]
    fn positional_rows_are_rejected() {
        let columns = columns(&[("id", "bigint")]);
        let rows = vec![json!([1, 2])];
        let error = json_rows_to_batch(&columns, &rows).unwrap_err();
        assert!(matches!(error, DriverError::Protocol { .. }), "{error:?}");
        assert!(error.to_string().contains("列名做键"), "{error}");
    }

    /// 缺列按 null 处理（对端少给一列不该整条结果作废），但列定义不能没有。
    #[test]
    fn a_missing_key_is_null_and_columns_are_required() {
        let columns = columns(&[("id", "bigint"), ("note", "text")]);
        let rows = vec![json!({ "id": 1 })];
        let batch = json_rows_to_batch(&columns, &rows).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert!(batch.column(1).is_null(0));

        assert!(matches!(
            json_rows_to_batch(&[], &rows),
            Err(DriverError::Protocol { .. })
        ));
    }

    /// 两条承载方式在 `QueryResult` 这一层必须看不出区别（§4.2.1.2）。
    #[test]
    fn both_wire_forms_land_in_batches() {
        use arrow::array::Int64Array;
        use arrow::datatypes::{DataType, Field, Schema};

        let inline = QueryPage {
            columns: columns(&[("id", "bigint")]),
            row_count: 1,
            affected_rows: None,
            has_more: false,
            cursor_id: None,
            truncated: false,
            data: PageData::Rows(vec![json!({ "id": 7 })]),
            notices: Vec::new(),
        }
        .into_query_result()
        .unwrap();

        let arrow_batch = ArrowBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, true)])),
            vec![Arc::new(Int64Array::from(vec![Some(7)]))],
        )
        .unwrap();
        let arrow = QueryPage {
            columns: columns(&[("id", "bigint")]),
            row_count: 1,
            affected_rows: None,
            has_more: false,
            cursor_id: None,
            truncated: false,
            data: PageData::Arrow(vec![arrow_batch]),
            notices: Vec::new(),
        }
        .into_query_result()
        .unwrap();

        for result in [&inline, &arrow] {
            assert_eq!(result.total_rows, 1);
            assert_eq!(result.batches.len(), 1);
            assert_eq!(result.columns, vec!["id".to_string()]);
            assert_eq!(result.column_types, vec!["bigint".to_string()]);
            // 驱动契约：只填 batches，`rows` 留空（架构 §12 #21）
            assert!(result.rows.is_empty());
            assert_eq!(result.is_read_only, Some(true));
        }
        assert_eq!(inline.total_rows(), arrow.total_rows());
    }

    /// 错误域映射：SQL 错带位置（光标能跳）、能力缺失说"不支持"、断线进连接域。
    #[tokio::test]
    async fn driver_errors_map_into_the_engine_error_domains() {
        let db = SidecarDatabase::new(&Arc::new(dummy_conn()), "s1", "fixture", descriptor());
        let sql = "select * from 无此表 where a = '它'";

        let sql_error = db.map_error(
            sql,
            DriverError::Rpc {
                code: RpcErrorCode::SqlError,
                message: "relation does not exist".into(),
                data: Some(json!({ "position": 15 })),
            },
        );
        match sql_error {
            CoreError::Database(DatabaseError::Query {
                position, reason, ..
            }) => {
                assert!(reason.contains("does not exist"), "{reason}");
                // 1 基**字符**位置 15 → 0 基**字节**偏移（中文占多字节，两者不同）
                assert_eq!(position, Some(14));
            }
            other => panic!("SQL 错应当进 DatabaseError::Query：{other:?}"),
        }

        let denied = db.map_error(
            sql,
            DriverError::Rpc {
                code: RpcErrorCode::CapabilityDenied,
                message: "驱动不支持事务".into(),
                data: None,
            },
        );
        assert!(
            matches!(denied, CoreError::Common(CommonError::NotSupported(_))),
            "{denied:?}"
        );

        let gone = db.map_error(
            sql,
            DriverError::Disconnected {
                reason: "对端退出".into(),
            },
        );
        assert!(
            matches!(gone, CoreError::Connection(ConnectionError::Network { .. })),
            "{gone:?}"
        );

        // 连不上库：进连接域，且**不是** Query（用户看到的该是「连不上」而不是「SQL 错了」）
        let refused = db.map_error(
            sql,
            DriverError::Rpc {
                code: RpcErrorCode::ConnectFailed,
                message: "password authentication failed".into(),
                data: None,
            },
        );
        match refused {
            CoreError::Connection(ConnectionError::Refused { reason, .. }) => {
                assert!(reason.contains("password"), "{reason}");
            }
            other => panic!("连不上库应当进 ConnectionError::Refused：{other:?}"),
        }

        let broken = db.map_error(
            sql,
            DriverError::Protocol {
                detail: "载荷不是合法 Arrow".into(),
            },
        );
        assert!(matches!(broken, CoreError::Plugin(_)), "{broken:?}");
    }

    /// 文件夹各挑各的类别，认不出的类别一律不落进树（导航摆不下它）。
    #[test]
    fn folders_take_their_own_kinds() {
        let object = |name: &str, kind: MetaObjectKind| MetaObject {
            name: name.to_string(),
            kind,
            comment: None,
            parent: None,
        };
        let objects = vec![
            object("orders", MetaObjectKind::Table),
            object("recent", MetaObjectKind::View),
            object("mv_daily", MetaObjectKind::MaterializedView),
            object("orders_seq", MetaObjectKind::Sequence),
            object("trg", MetaObjectKind::Trigger),
            object("order_state", MetaObjectKind::Other("domain".into())),
        ];
        let names = |nodes: Vec<NodeInfo>| -> Vec<String> {
            nodes.into_iter().map(|n| n.name).collect()
        };

        // 表与视图是**一次调用**的结果，导航自己按 kind 分成两个文件夹
        assert_eq!(
            names(take_kind(objects.clone(), MetaObjectKind::is_table_like)),
            vec!["orders", "recent", "mv_daily"]
        );
        assert_eq!(
            names(take_kind(objects.clone(), |k| matches!(
                k,
                MetaObjectKind::Sequence
            ))),
            vec!["orders_seq"]
        );
        // 认不出的类别不被归进任何文件夹
        for want in [
            MetaObjectKind::is_table_like as fn(&MetaObjectKind) -> bool,
            |k| matches!(k, MetaObjectKind::Sequence),
            |k| matches!(k, MetaObjectKind::Trigger),
            |k| matches!(k, MetaObjectKind::Procedure),
            |k| matches!(k, MetaObjectKind::Function),
        ] {
            let picked = names(take_kind(objects.clone(), want));
            assert!(!picked.iter().any(|n| n == "order_state"), "{picked:?}");
        }
    }

    /// schema 的取值顺序：显式给的 → descriptor 里的默认 → 空串（由驱动自己决定默认）。
    #[tokio::test]
    async fn schema_falls_back_to_the_drivers_default() {
        let mut db = SidecarDatabase::new(&Arc::new(dummy_conn()), "s1", "fixture", descriptor());
        assert_eq!(db.wire_schema(Some("sales")), "sales");
        assert_eq!(db.wire_schema(None), "public", "descriptor 给了 default_schema");
        assert_eq!(db.wire_schema(Some("")), "public", "空串按没给算");

        // 驱动自己没说默认 schema：传空串，别替它猜一个
        db.descriptor.default_schema = None;
        assert_eq!(db.wire_schema(None), "");
    }

    /// 二进制参数与 NaN 明确拒绝（口径没定就别猜）。
    #[test]
    fn unsupported_param_values_are_refused() {
        assert!(shared_value_to_json(&SharedValue::Bytes(vec![1, 2, 3])).is_err());
        assert!(shared_value_to_json(&SharedValue::Float(f64::NAN)).is_err());
        assert!(shared_value_to_json(&SharedValue::Int(3)).is_ok());
    }

    fn dummy_conn() -> SidecarConn {
        let (host, peer) = tokio::io::duplex(64);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, _events) = SidecarConn::spawn(host_r, host_w);
        drop(peer);
        conn
    }

    fn descriptor() -> DriverDescriptor {
        DriverDescriptor {
            driver_id: "fixture".into(),
            display_name: "Fixture".into(),
            server_version: Some("fixture-1".into()),
            capabilities: [("transactions".to_string(), true)].into_iter().collect(),
            identifier_quote: Some("\"".into()),
            default_schema: Some("public".into()),
        }
    }
}
