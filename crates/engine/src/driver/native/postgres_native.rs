//! PostgreSQL 官方 Rust 驱动实现（基于 tokio-postgres 0.7）
//!
//! 与 sqlx 驱动的区别：
//! - tokio-postgres 是 PostgreSQL 官方维护的异步驱动
//! - 支持 Pipeline 模式、COPY 协议、LISTEN/NOTIFY 等原生活功能
//! - 与 sqlx 驱动并存，用户可在连接时选择驱动

use arrow::array::{
    ArrayRef, BinaryArray, BooleanArray, Float32Array, Float64Array, Int32Array, Int64Array,
    StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::driver::traits::MetadataBrowser;
use crate::driver::utils::{affected_rows_result, byte_offset_for_char, returns_rows};
use crate::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, IndexDetail, NodeDetail, NodeInfo,
    PoolStatus, SchemaObjectKind, Transaction,
};
use shared::error::{ConnectionError, CoreError, DatabaseError};
use shared::models::{ArrowBatch, QueryResult, Value};

// ============================================================================
// PostgreSQL Native Database 结构体
// ============================================================================

/// PostgreSQL 官方驱动数据库实例
///
/// 封装 `tokio_postgres::Client`，通过 `Database` trait 提供统一的查询/执行接口。
/// 使用单连接模式（非连接池），适合轻量级使用场景。
/// 元数据浏览通过 `MetadataBrowser` trait 实现，查询 `information_schema` 和 `pg_catalog`。
pub struct PostgresNativeDatabase {
    client: Arc<Mutex<tokio_postgres::Client>>,
    server_version: Option<String>,
}

impl PostgresNativeDatabase {
    /// 从连接 URL 创建新的 PostgreSQL 数据库实例
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        Self::new_with_tls(url, None).await
    }

    /// 从连接 URL + 结构化 TLS 请求创建实例。
    ///
    /// tokio-postgres 的连接串里只有 `sslmode=disable|prefer|require`（无 verify 档、
    /// 无证书参数），所以 **验证书链 / 主机名与证书文件都在这里的连接器上落实**：
    /// `verify-ca` → 校链不校主机名，`verify-full` → 两者都校。
    pub async fn new_with_tls(
        url: &str,
        tls_request: Option<&connection::config::TlsRequest>,
    ) -> Result<Self, CoreError> {
        let tls_connector = pg_tls_connector(tls_request)?;
        let tls = postgres_native_tls::MakeTlsConnector::new(tls_connector);

        let (client, connection) = tokio_postgres::connect(url, tls).await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "postgres_native".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;

        // 在后台任务中驱动连接事件循环
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(target: "postgres_native", "Connection error: {}", e);
            }
        });

        // 获取服务器版本
        let server_version = {
            let rows = client
                .query("SELECT version()", &[])
                .await
                .ok()
                .and_then(|rows| rows.first().map(|r| r.get::<_, String>(0)));
            rows
        };

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            server_version,
        })
    }

    /// 从现有 Client 创建实例
    pub fn from_client(client: tokio_postgres::Client) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            server_version: None,
        }
    }

    /// 从现有 Client 及版本信息创建实例
    pub fn from_client_with_version(
        client: tokio_postgres::Client,
        server_version: Option<String>,
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            server_version,
        }
    }
}

// ============================================================================
// TLS 连接器构造
// ============================================================================

/// 校验策略（纯函数）：`(校验证书链, 校验主机名)`。
///
/// 无请求时保持历史行为（不校验，接受自签）；`verify-ca` / `verify-full` 才开校验，
/// 两者都是「用户显式要验证书」的意图，不允许静默降级。
pub(crate) fn pg_tls_policy(
    req: Option<&connection::config::TlsRequest>,
) -> (bool, bool) {
    match req {
        None => (false, false),
        Some(r) => (r.verifies_chain(), r.verifies_hostname()),
    }
}

/// 按 TLS 请求构造 native-tls 连接器（证书文件在这里读，读不到就明确报错）。
pub(crate) fn pg_tls_connector(
    req: Option<&connection::config::TlsRequest>,
) -> Result<native_tls::TlsConnector, CoreError> {
    let (verify_chain, verify_hostname) = pg_tls_policy(req);
    let mut builder = native_tls::TlsConnector::builder();
    builder
        .danger_accept_invalid_certs(!verify_chain)
        .danger_accept_invalid_hostnames(!verify_hostname);

    if let Some(r) = req {
        if let Some(ca) = r.ca_path() {
            let bytes = std::fs::read(ca)
                .map_err(|e| tls_config_err(format!("读取 CA 证书失败（{ca}）：{e}")))?;
            let cert = native_tls::Certificate::from_pem(&bytes)
                .or_else(|_| native_tls::Certificate::from_der(&bytes))
                .map_err(|e| {
                    tls_config_err(format!("CA 证书既不是有效 PEM 也不是 DER（{ca}）：{e}"))
                })?;
            builder.add_root_certificate(cert);
        }

        match (r.client_cert_path(), r.client_key_path()) {
            (None, None) => {}
            (Some(cert), Some(key)) => {
                let cert_pem = std::fs::read(cert)
                    .map_err(|e| tls_config_err(format!("读取客户端证书失败（{cert}）：{e}")))?;
                let key_pem = std::fs::read(key)
                    .map_err(|e| tls_config_err(format!("读取客户端私钥失败（{key}）：{e}")))?;
                let identity = native_tls::Identity::from_pkcs8(&cert_pem, &key_pem)
                    .map_err(|e| {
                        tls_config_err(format!(
                            "客户端证书与私钥无法组成 PKCS#8 身份（{cert} + {key}）：{e}"
                        ))
                    })?;
                builder.identity(identity);
            }
            (Some(cert), None) => {
                return Err(tls_config_err(format!(
                    "客户端证书 {cert} 缺少对应私钥：两件套需成对填写"
                )))
            }
            (None, Some(key)) => {
                return Err(tls_config_err(format!(
                    "客户端私钥 {key} 缺少对应证书：两件套需成对填写"
                )))
            }
        }
    }

    builder
        .build()
        .map_err(|e| tls_config_err(format!("TLS 连接器构造失败：{e}")))
}

fn tls_config_err(reason: String) -> CoreError {
    CoreError::connection(ConnectionError::InvalidConfig {
        conn_id: "postgres_native".to_string(),
        reason,
    })
}

// ============================================================================
// SQL 工具函数
// ============================================================================

fn is_read_only_sql(sql: &str) -> bool {
    let sql_upper = sql.trim_start().to_uppercase();
    sql_upper.starts_with("SELECT")
        || sql_upper.starts_with("SHOW")
        || sql_upper.starts_with("DESCRIBE")
        || sql_upper.starts_with("EXPLAIN")
        || sql_upper.starts_with("SET")
}

/// 写语句（不返回行）走 `Client::execute`，拿**真实影响行数**（B5 / P0.6）
///
/// `execute` 会丢弃结果集，只回报 `CommandComplete` 里的行数；
/// 带 `RETURNING` 的语句要走 `query`，否则数据会被丢掉。
async fn execute_writing(
    client: &tokio_postgres::Client,
    sql: &str,
) -> Result<QueryResult, CoreError> {
    let affected = client
        .execute(sql, &[])
        .await
        .map_err(|e| query_error(sql, e))?;
    Ok(affected_rows_result(affected))
}

/// tokio-postgres 错误 → 引擎错误（**把服务器说的话原样带出来**，B6）
///
/// `tokio_postgres::Error` 的 `Display` 只有 “db error” 这种看不出所以然的话；真正的消息
/// （+ `DETAIL` / `HINT`）在 `as_db_error()` 里。错误文本是用户唯一能看到的线索，
/// 不能只剩 “db error”。
///
/// 位置同理：`ErrorPosition::Original` 是 **1 基的字符位置**（PG 协议定义），
/// 换算成 SQL 的**字节偏移**给上层用；`Internal` 说的是服务器自己生成的语句，与我们
/// 的 SQL 无关，不能用。
fn query_error(sql: &str, error: tokio_postgres::Error) -> CoreError {
    let Some(db) = error.as_db_error() else {
        // 不是数据库报的错（IO / 连接断了）：原样带上
        return CoreError::database(DatabaseError::query(sql, error.to_string()));
    };

    let mut reason = db.message().to_string();
    if let Some(detail) = db.detail() {
        reason.push_str(&format!("（{detail}）"));
    }
    if let Some(hint) = db.hint() {
        reason.push_str(&format!("；建议：{hint}"));
    }

    let mut mapped = DatabaseError::query(sql, reason);
    if let Some(tokio_postgres::error::ErrorPosition::Original(position)) = db.position()
        && let Some(offset) = byte_offset_for_char(sql, *position as usize)
    {
        mapped = mapped.with_position(offset);
    }
    CoreError::database(mapped)
}

// ============================================================================
// Arrow 转换
// ============================================================================

/// 将 tokio_postgres Row 转换为 Arrow 批处理
/// 把一个单元格解成展示文本（tokio-postgres 版，对应 sqlx 侧的 `sqlx_cell_as_text!`）。
///
/// 顺序与 sqlx 侧同口径：`String` → 时间族 → UUID → JSON → 字节 lossy。
/// `timestamptz` 只能按带时区解、`timestamp` 只能按无时区解，所以两个都要试。
///
/// **NUMERIC 不在这条链里**：`postgres-types` 0.2.13 没有任何 decimal feature，
/// `NUMERIC` 当前**没有** `FromSql` 实现 —— 要么等上游加，要么自己按 wire 格式写一个。
/// 在那之前它只能交出 NULL，并由调用方的 warn 留痕。
fn pg_native_cell_as_text(row: &tokio_postgres::Row, idx: usize) -> Option<String> {
    if let Ok(Some(v)) = row.try_get::<_, Option<String>>(idx) {
        return Some(v);
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(idx) {
        return Some(v.format("%Y-%m-%d %H:%M:%S%.f+00").to_string());
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<chrono::NaiveDateTime>>(idx) {
        return Some(v.format("%Y-%m-%d %H:%M:%S%.f").to_string());
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<chrono::NaiveDate>>(idx) {
        return Some(v.format("%Y-%m-%d").to_string());
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<chrono::NaiveTime>>(idx) {
        return Some(v.format("%H:%M:%S%.f").to_string());
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<uuid::Uuid>>(idx) {
        return Some(v.to_string());
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<serde_json::Value>>(idx) {
        return Some(v.to_string());
    }
    if let Ok(Some(v)) = row.try_get::<_, Option<Vec<u8>>>(idx) {
        return Some(String::from_utf8_lossy(&v).into_owned());
    }
    None
}

fn postgres_native_rows_to_arrow(
    columns: &[String],
    rows: &[tokio_postgres::Row],
) -> Result<ArrowBatch, CoreError> {
    let num_rows = rows.len();
    let num_cols = columns.len();
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(num_cols);

    for col_idx in 0..num_cols {
        let mut string_values: Vec<Option<String>> = Vec::with_capacity(num_rows);
        let mut int32_values: Vec<Option<i32>> = Vec::with_capacity(num_rows);
        let mut int64_values: Vec<Option<i64>> = Vec::with_capacity(num_rows);
        let mut float32_values: Vec<Option<f32>> = Vec::with_capacity(num_rows);
        let mut float64_values: Vec<Option<f64>> = Vec::with_capacity(num_rows);
        let mut bool_values: Vec<Option<bool>> = Vec::with_capacity(num_rows);
        let mut binary_values: Vec<Option<Vec<u8>>> = Vec::with_capacity(num_rows);

        // 遍历所有行确定最宽类型（0=Null, 1=Bool, 2=Int32, 3=Int64, 4=Float32, 5=Float64, 6=Binary, 7=Utf8）
        let mut detected_rank: u8 = 0;

        for row in rows {
            let col_type = row.columns()[col_idx].type_();

            let row_rank = if *col_type == tokio_postgres::types::Type::BOOL {
                1
            } else if *col_type == tokio_postgres::types::Type::INT2
                || *col_type == tokio_postgres::types::Type::INT4
            {
                2
            } else if *col_type == tokio_postgres::types::Type::INT8 {
                3
            } else if *col_type == tokio_postgres::types::Type::FLOAT4 {
                4
            } else if *col_type == tokio_postgres::types::Type::FLOAT8 {
                5
            } else if *col_type == tokio_postgres::types::Type::BYTEA {
                6
            } else {
                7 // 其他类型 → Utf8
            };

            if row_rank > detected_rank {
                detected_rank = row_rank;
            }
            if detected_rank == 7 {
                break; // Utf8 为最宽类型
            }
        }

        let effective_type = match detected_rank {
            1 => DataType::Boolean,
            2 => DataType::Int32,
            3 => DataType::Int64,
            4 => DataType::Float32,
            5 => DataType::Float64,
            6 => DataType::Binary,
            _ => DataType::Utf8,
        };

        // 按确定的类型收集值
        for row in rows {
            match effective_type {
                DataType::Boolean => {
                    bool_values.push(row.try_get::<_, Option<bool>>(col_idx).ok().flatten());
                }
                DataType::Int32 => {
                    // `int2` 只能按 `i16` 解（tokio-postgres 不放宽），加宽进 Int32 列
                    let value = row
                        .try_get::<_, Option<i32>>(col_idx)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            row.try_get::<_, Option<i16>>(col_idx)
                                .ok()
                                .flatten()
                                .map(i32::from)
                        });
                    int32_values.push(value);
                }
                DataType::Int64 => {
                    int64_values.push(row.try_get::<_, Option<i64>>(col_idx).ok().flatten());
                }
                DataType::Float32 => {
                    float32_values.push(row.try_get::<_, Option<f32>>(col_idx).ok().flatten());
                }
                DataType::Float64 => {
                    float64_values.push(row.try_get::<_, Option<f64>>(col_idx).ok().flatten());
                }
                DataType::Binary => {
                    binary_values.push(row.try_get::<_, Option<Vec<u8>>>(col_idx).ok().flatten());
                }
                _ => {
                    // 通用文本链：真文本列先命中；时间 / UUID / JSON 由 `pg_native_cell_as_text`
                    // 解成文本。2026-09-21 之前这里只试 `String` 加几个标量，于是
                    // timestamptz / date / time / jsonb / uuid 全部静默变 NULL。
                    string_values.push(pg_native_cell_as_text(row, col_idx));
                }
            }
        }

        // 解不出来 ≠ 值为 NULL：整列取不到任何值、而库里确实有行时留痕。
        // （tokio-postgres 的 `Row` 没有公开的 is_null，所以这里按「整列全空」判定；
        //  数值列全都是真 NULL 时会多报一条 —— 宁可多报，不可静默。）
        if effective_type == DataType::Utf8
            && !rows.is_empty()
            && string_values.iter().all(Option::is_none)
        {
            tracing::warn!(
                driver = "postgres_native",
                column = %columns[col_idx],
                declared_type = %rows[0].columns()[col_idx].type_().name(),
                total_rows = num_rows,
                "整列没能取出任何值（该类型当前没有解码器，或整列都是 SQL NULL）——\
                 出口层目前不区分这两者，界面会显示为空"
            );
        }

        let array: ArrayRef = match effective_type {
            DataType::Boolean => Arc::new(BooleanArray::from(bool_values)),
            DataType::Int32 => Arc::new(Int32Array::from(int32_values)),
            DataType::Int64 => Arc::new(Int64Array::from(int64_values)),
            DataType::Float32 => Arc::new(Float32Array::from(float32_values)),
            DataType::Float64 => Arc::new(Float64Array::from(float64_values)),
            DataType::Binary => {
                let refs: Vec<Option<&[u8]>> = binary_values
                    .iter()
                    .map(|opt| opt.as_ref().map(|v| v.as_slice()))
                    .collect();
                Arc::new(BinaryArray::from(refs))
            }
            _ => Arc::new(StringArray::from(string_values)),
        };

        arrays.push(array);
    }

    let fields: Vec<Field> = columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let data_type = arrays[i].data_type().clone();
            Field::new(name, data_type, true)
        })
        .collect();

    let schema = Arc::new(Schema::new(fields));
    RecordBatch::try_new(schema, arrays).map_err(|e| {
        CoreError::database(DatabaseError::Driver {
            db_type: "postgres_native".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

// ============================================================================
// 辅助函数
// ============================================================================

fn build_query_result(
    columns: &[String],
    rows: &[tokio_postgres::Row],
    _is_read_only: bool,
) -> Result<QueryResult, CoreError> {
    let batch = postgres_native_rows_to_arrow(columns, rows)?;
    Ok(QueryResult {
        columns: columns.to_vec(),
        batches: vec![batch],
        ..Default::default()
    })
}

fn rows_to_node_info(result: &QueryResult, kind: SchemaObjectKind) -> Vec<NodeInfo> {
    use arrow::array::StringArray;
    let mut nodes: Vec<NodeInfo> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        nodes.push(NodeInfo::new(name.to_string(), kind.clone()));
                    }
                }
            }
        }
    }
    nodes
}

/// 触发器结果集 → 对象列表（第 2 列是所属表，进 `parent_name`）。
fn names_to_trigger_nodes(result: &QueryResult) -> Vec<NodeInfo> {
    use arrow::array::{Array, StringArray};
    let mut nodes: Vec<NodeInfo> = Vec::new();
    for row_idx in 0..result.total_rows() {
        let Some(batch) = result.batches.first() else {
            break;
        };
        if row_idx >= batch.num_rows() {
            continue;
        }
        let Some(name_arr) = batch.column(0).as_any().downcast_ref::<StringArray>() else {
            continue;
        };
        let name = name_arr.value(row_idx);
        if name.is_empty() {
            continue;
        }
        let mut node = NodeInfo::new(name.to_string(), SchemaObjectKind::Trigger);
        if let Some(table_arr) = batch.column(1).as_any().downcast_ref::<StringArray>() {
            if !table_arr.is_null(row_idx) {
                node = node.with_parent(table_arr.value(row_idx));
            }
        }
        nodes.push(node);
    }
    nodes
}

// ============================================================================
// Database trait 实现
// ============================================================================

#[async_trait::async_trait]
impl Database for PostgresNativeDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let is_read_only = is_read_only_sql(sql);
        let client = self.client.lock().await;

        // B5 / P0.6：不返回结果集的写语句走 `execute`，拿驱动的真实影响行数
        if !is_read_only && !returns_rows(sql) {
            return execute_writing(&client, sql).await;
        }

        let rows = client
            .query(sql, &[])
            .await
            .map_err(|e| query_error(sql, e))?;

        if rows.is_empty() {
            return Ok(QueryResult {
                columns: vec![],
                batches: vec![],
                ..Default::default()
            });
        }

        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<QueryResult, CoreError> {
        let is_read_only = is_read_only_sql(sql);
        let client = self.client.lock().await;

        let pg_params: Vec<Box<dyn tokio_postgres::types::ToSql + Sync + Send>> = params
            .iter()
            .map(|v| -> Box<dyn tokio_postgres::types::ToSql + Sync + Send> {
                match v {
                    Value::Null => Box::new(None::<String>),
                    Value::Bool(b) => Box::new(*b),
                    Value::Int(i) => Box::new(*i),
                    Value::Float(f) => Box::new(*f),
                    Value::Text(s) => Box::new(s.clone()),
                    Value::Bytes(b) => Box::new(b.clone()),
                }
            })
            .collect();

        let param_refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = pg_params
            .iter()
            .map(|p| p.as_ref() as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect();

        // B5 / P0.6：带参数的写语句同样给真实影响行数
        if !is_read_only && !returns_rows(sql) {
            let affected = client.execute(sql, &param_refs).await.map_err(|e| {
                CoreError::database(DatabaseError::query(sql, e.to_string()))
            })?;
            return Ok(affected_rows_result(affected));
        }

        let rows = client
            .query(sql, &param_refs)
            .await
            .map_err(|e| query_error(sql, e))?;

        if rows.is_empty() {
            return Ok(QueryResult {
                columns: vec![],
                batches: vec![],
                ..Default::default()
            });
        }

        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_cancel(
        &self,
        sql: &str,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<QueryResult, CoreError> {
        let sql_owned = sql.to_string();
        let sql_for_cancel = sql.to_string();
        let client = self.client.clone();

        tokio::select! {
            result = async move {
                let is_read_only = is_read_only_sql(&sql_owned);
                let guard = client.lock().await;

                // B5 / P0.6：不返回结果集的写语句走 `execute`，拿驱动的真实影响行数
                if !is_read_only && !returns_rows(&sql_owned) {
                    return execute_writing(&guard, &sql_owned).await;
                }

                let rows = guard
                    .query(&sql_owned, &[])
                    .await
                    .map_err(|e| query_error(&sql_owned, e))?;

                if rows.is_empty() {
                    return Ok(QueryResult {
                        columns: vec![],
                        batches: vec![],
                        ..Default::default()
                    });
                }

                let columns: Vec<String> = rows[0]
                    .columns()
                    .iter()
                    .map(|c| c.name().to_string())
                    .collect();

                build_query_result(&columns, &rows, is_read_only)
            } => result,
            _ = cancel_token.cancelled() => {
                Err(CoreError::database(DatabaseError::Query {
                    sql: sql_for_cancel,
                    reason: "Query cancelled".to_string(),
                    position: None,
                }))
            }
        }
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError> {
        let client = self.client.lock().await;
        client.batch_execute("BEGIN").await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "postgres_native".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;
        // Drop the lock so transaction can use the client
        drop(client);

        Ok(Box::new(PostgresNativeTransaction {
            client: self.client.clone(),
            active: true,
        }))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::postgres_native()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        let client = self.client.lock().await;
        client.query_one("SELECT 1", &[]).await.map_err(|e| {
            CoreError::connection(ConnectionError::Other {
                conn_id: "postgres_native".to_string(),
                reason: format!("Ping failed: {}", e),
            })
        })?;
        Ok(())
    }

    async fn pool_status(&self) -> Option<PoolStatus> {
        Some(PoolStatus {
            size: 1,
            idle: 0,
            active: 1,
            waiting: 0,
            max_connections: 1,
            min_connections: 1,
        })
    }

    fn as_metadata_browser(&self) -> Option<&dyn MetadataBrowser> {
        Some(self)
    }

    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        self.get_catalogs()
            .await
            .map(|nodes| nodes.into_iter().map(|n| n.name).collect())
    }

    async fn list_schemas(&self, catalog: &str) -> Result<Vec<String>, CoreError> {
        self.get_schemas(catalog)
            .await
            .map(|nodes| nodes.into_iter().map(|n| n.name).collect())
    }

    async fn list_tables(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        self.get_tables(catalog, schema_name).await
    }

    async fn list_columns(
        &self,
        _catalog: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let detail = self.get_table_detail(_catalog, schema_name, table).await?;
        Ok(detail.columns)
    }

    async fn list_procedures(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT p.proname FROM pg_catalog.pg_proc p \
                   JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
                   WHERE n.nspname = $1 AND p.prokind = 'p' \
                   ORDER BY p.proname";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Procedure))
    }

    async fn list_functions(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT p.proname FROM pg_catalog.pg_proc p \
                   JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
                   WHERE n.nspname = $1 AND p.prokind = 'f' \
                   ORDER BY p.proname";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Function))
    }

    async fn list_sequences(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        self.get_sequences(catalog, schema.unwrap_or("public"))
            .await
    }

    async fn list_triggers(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        self.get_triggers(catalog, schema.unwrap_or("public"))
            .await
    }

    async fn get_routine_source(
        &self,
        _catalog: &str,
        schema: Option<&str>,
        name: &str,
        kind: SchemaObjectKind,
    ) -> Result<Option<String>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let prokind = match kind {
            SchemaObjectKind::Procedure => "p",
            SchemaObjectKind::Function => "f",
            _ => return Ok(None),
        };
        let sql = "\
            SELECT pg_get_functiondef(p.oid) \
             FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
             WHERE n.nspname = $1 AND p.proname = $2 AND p.prokind = $3";
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(schema_name.to_string()),
                    Value::Text(name.to_string()),
                    Value::Text(prokind.to_string()),
                ],
            )
            .await?;
        if let Some(batch) = result.batches.first() {
            if batch.num_rows() > 0 {
                if let Some(col) = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<arrow::array::StringArray>()
                {
                    return Ok(Some(col.value(0).to_string()));
                }
            }
        }
        Ok(None)
    }
}

// ============================================================================
// Transaction 实现
// ============================================================================

/// PostgreSQL 本地事务句柄
///
/// 与数据库共享同一个 `tokio_postgres::Client`，通过 BEGIN/COMMIT/ROLLBACK
/// 管理事务生命周期。Drop 时若事务仍活跃则自动回滚。
pub struct PostgresNativeTransaction {
    client: Arc<Mutex<tokio_postgres::Client>>,
    active: bool,
}

#[async_trait::async_trait]
impl Transaction for PostgresNativeTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        if !self.active {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "postgres_native".to_string(),
                operation: "query".to_string(),
                source: "Transaction already closed".to_string(),
            }));
        }
        let is_read_only = is_read_only_sql(sql);
        let client = self.client.lock().await;

        // B5 / P0.6：事务内的写语句同样要真实影响行数
        if !is_read_only && !returns_rows(sql) {
            return execute_writing(&client, sql).await;
        }

        let rows = client
            .query(sql, &[])
            .await
            .map_err(|e| query_error(sql, e))?;

        if rows.is_empty() {
            return Ok(QueryResult {
                columns: vec![],
                batches: vec![],
                ..Default::default()
            });
        }

        let columns: Vec<String> = rows[0]
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect();

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn commit(&mut self) -> Result<(), CoreError> {
        if !self.active {
            return Ok(());
        }
        let client = self.client.lock().await;
        client.batch_execute("COMMIT").await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "postgres_native".to_string(),
                operation: "commit".to_string(),
                source: e.to_string(),
            })
        })?;
        self.active = false;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CoreError> {
        if !self.active {
            return Ok(());
        }
        let client = self.client.lock().await;
        if let Err(e) = client.batch_execute("ROLLBACK").await {
            tracing::warn!("PostgreSQL native transaction rollback error: {}", e);
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for PostgresNativeTransaction {
    fn drop(&mut self) {
        // If still active on drop, we can't do async cleanup here.
        // The ROLLBACK will happen naturally when PostgreSQL detects
        // the transaction is no longer in use, or it may be left open.
        // In practice, the connection will be reused and any uncommitted
        // changes won't be visible.
        if self.active {
            tracing::warn!(
                "PostgresNativeTransaction dropped without commit/rollback; \
                 transaction may remain open until connection reuse"
            );
        }
    }
}

// ============================================================================
// MetadataBrowser trait 实现
// ============================================================================

#[async_trait::async_trait]
impl MetadataBrowser for PostgresNativeDatabase {
    async fn get_catalogs(&self) -> Result<Vec<NodeInfo>, CoreError> {
        // 同 sqlx 驱动：一条连接只绑定一个库，只返回当前库（避免假节点）。
        let result = self
            .query("SELECT current_database()::text AS datname")
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Catalog))
    }

    async fn get_schemas(&self, catalog: &str) -> Result<Vec<NodeInfo>, CoreError> {
        let sql = "SELECT schema_name FROM information_schema.schemata \
                   WHERE catalog_name = $1 AND schema_name NOT IN ('pg_catalog', 'information_schema') \
                   ORDER BY schema_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Schema))
    }

    async fn get_tables(&self, catalog: &str, schema: &str) -> Result<Vec<NodeInfo>, CoreError> {
        use arrow::array::StringArray;
        let sql = "SELECT table_name, table_type FROM information_schema.tables \
                   WHERE table_catalog = $1 AND table_schema = $2 ORDER BY table_name";
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(catalog.to_string()),
                    Value::Text(schema.to_string()),
                ],
            )
            .await?;
        let mut nodes: Vec<NodeInfo> = Vec::new();
        for row_idx in 0..result.total_rows() {
            if let Some(batch) = result.batches.first() {
                if row_idx < batch.num_rows() {
                    if let (Some(name_arr), Some(type_arr)) = (
                        batch.column(0).as_any().downcast_ref::<StringArray>(),
                        batch.column(1).as_any().downcast_ref::<StringArray>(),
                    ) {
                        let table_type = type_arr.value(row_idx);
                        let kind = if table_type == "VIEW" {
                            SchemaObjectKind::View
                        } else {
                            SchemaObjectKind::Table
                        };
                        nodes.push(NodeInfo::new(
                            name_arr.value(row_idx).to_string(),
                            kind,
                        ));
                    }
                }
            }
        }
        Ok(nodes)
    }

    async fn get_table_detail(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<NodeDetail, CoreError> {
        use arrow::array::StringArray;
        let sql = "\
            SELECT column_name, data_type, is_nullable, \
             CASE WHEN column_name IN (SELECT kcu.column_name FROM information_schema.table_constraints tc \
             JOIN information_schema.key_column_usage kcu ON tc.constraint_name = kcu.constraint_name \
             WHERE tc.table_schema = $2 AND tc.table_name = $3 AND tc.constraint_type = 'PRIMARY KEY') \
             THEN 'PRI' ELSE '' END AS column_key, \
             column_default, \
             COALESCE(col_description((SELECT oid FROM pg_class WHERE relname = $3), ordinal_position), '') AS column_comment \
             FROM information_schema.columns \
             WHERE table_catalog = $1 AND table_schema = $2 AND table_name = $3 \
             ORDER BY ordinal_position";
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(catalog.to_string()),
                    Value::Text(schema.to_string()),
                    Value::Text(table.to_string()),
                ],
            )
            .await?;
        let mut columns: Vec<ColumnDetail> = Vec::new();
        for row_idx in 0..result.total_rows() {
            if let Some(batch) = result.batches.first() {
                if row_idx < batch.num_rows() {
                    let col_name = batch
                        .column(0)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .map_or("", |a| a.value(row_idx));
                    let data_type = batch
                        .column(1)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .map_or("", |a| a.value(row_idx));
                    let nullable = batch
                        .column(2)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .is_some_and(|a| a.value(row_idx) == "YES");
                    let pk = batch
                        .column(3)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .map_or("", |a| a.value(row_idx));
                    let default = batch
                        .column(4)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .map_or("", |a| a.value(row_idx));
                    let comment = batch
                        .column(5)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .map_or("", |a| a.value(row_idx));
                    columns.push(ColumnDetail {
                        name: col_name.to_string(),
                        data_type: data_type.to_string(),
                        nullable,
                        is_primary_key: pk == "PRI",
                        is_foreign_key: false,
                        default_value: if default.is_empty() {
                            None
                        } else {
                            Some(default.to_string())
                        },
                        comment: if comment.is_empty() {
                            None
                        } else {
                            Some(comment.to_string())
                        },
                        extra: std::collections::HashMap::new(),
                    });
                }
            }
        }

        Ok(NodeDetail {
            node: NodeInfo::new(table, SchemaObjectKind::Table),
            columns,
            index_count: None,
            row_count_estimate: None,
        })
    }

    async fn get_indexes(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        self.list_indexes(catalog, Some(schema), table).await
    }

    async fn get_constraints(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        self.list_constraints(catalog, Some(schema), table).await
    }

    /// 序列与触发器在 PostgreSQL 走**浏览器层**（而不是 `Database::list_*` 回退）：
    /// 回退路径要先让 `get_sequences` 返回空才算数，而 trait 默认实现就是空——
    /// 那是“未支持”与“真的没有”分辨不清的写法。这里直接给真实实现。
    async fn get_sequences(&self, _catalog: &str, schema: &str) -> Result<Vec<NodeInfo>, CoreError> {
        let sql = "SELECT sequence_name FROM information_schema.sequences \
                   WHERE sequence_schema = $1 ORDER BY sequence_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema.to_string())])
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Sequence))
    }

    async fn get_triggers(&self, _catalog: &str, schema: &str) -> Result<Vec<NodeInfo>, CoreError> {
        // 只取「名字 + 所属表」：触发事件（event_manipulation）当前没有展示位，不查。
        let sql = "SELECT trigger_name, event_object_table \
                   FROM information_schema.triggers \
                   WHERE trigger_schema = $1 ORDER BY trigger_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema.to_string())])
            .await?;
        Ok(names_to_trigger_nodes(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Database;

    const PG_URL: &str = "postgresql://postgres:postgresql@localhost:5432/business_db";

    /// 校验策略（纯函数）：无请求 = 历史行为（不校验）；verify 档才开校验，
    /// `verify-ca` 不校主机名、`verify-full` 两者都校。
    #[test]
    fn tls_policy_follows_the_request() {
        use connection::config::{SslConfig, TlsRequest};
        use connection::url_params::SslMode;

        assert_eq!(pg_tls_policy(None), (false, false));

        let require = TlsRequest::new(
            SslMode::Require,
            SslConfig {
                verify_server_cert: false,
                ..Default::default()
            },
        );
        assert_eq!(pg_tls_policy(Some(&require)), (false, false), "require 只管加密");

        let ca = TlsRequest::new(
            SslMode::VerifyCa,
            SslConfig {
                verify_server_cert: true,
                ..Default::default()
            },
        );
        assert_eq!(pg_tls_policy(Some(&ca)), (true, false));

        let full = TlsRequest::new(
            SslMode::VerifyFull,
            SslConfig {
                verify_server_cert: true,
                ..Default::default()
            },
        );
        assert_eq!(pg_tls_policy(Some(&full)), (true, true));
    }

    /// 证书文件读不到 / 两件套不齐 → **可见错误**（不静默变成不校验）。
    #[test]
    fn broken_cert_material_is_a_visible_error() {
        use connection::config::{SslConfig, TlsRequest};
        use connection::url_params::SslMode;

        let missing = std::env::temp_dir().join("rds_no_such_ca_file_9f3a.pem");
        let _ = std::fs::remove_file(&missing);
        let req = TlsRequest::new(
            SslMode::VerifyCa,
            SslConfig {
                verify_server_cert: true,
                ca_cert_path: Some(missing.to_string_lossy().to_string()),
                ..Default::default()
            },
        );
        let err = pg_tls_connector(Some(&req)).expect_err("缺文件应报错");
        assert!(err.to_string().contains("读取 CA 证书失败"), "{err}");

        let half = TlsRequest::new(
            SslMode::Require,
            SslConfig {
                verify_server_cert: false,
                client_cert_path: Some("D:/certs/only-cert.pem".to_string()),
                ..Default::default()
            },
        );
        let err = pg_tls_connector(Some(&half)).expect_err("缺私钥应报错");
        assert!(err.to_string().contains("缺少对应私钥"), "{err}");

        // 不提供任何请求：旧行为（不校验）仍然可构造连接器
        assert!(pg_tls_connector(None).is_ok());
    }

    /// 触发器结果集 → 对象列表：第 2 列（所属表）进 `parent_name`，NULL 时不填。
    /// 与 sqlx 驱动那份是各自的实现，所以两边各钉一遍。
    #[test]
    fn trigger_rows_carry_their_table_into_parent_name() {
        let schema = std::sync::Arc::new(Schema::new(vec![
            Field::new("trigger_name", DataType::Utf8, false),
            Field::new("event_object_table", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                std::sync::Arc::new(StringArray::from(vec!["audit_trg", "orphan_trg"])),
                std::sync::Arc::new(StringArray::from(vec![Some("orders"), None])),
            ],
        )
        .expect("构造批次");
        let result = QueryResult {
            columns: vec![
                "trigger_name".to_string(),
                "event_object_table".to_string(),
            ],
            batches: vec![batch],
            ..Default::default()
        };

        let nodes = names_to_trigger_nodes(&result);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "audit_trg");
        assert_eq!(nodes[0].parent_name.as_deref(), Some("orders"));
        assert_eq!(nodes[1].parent_name, None, "NULL 所属表不该被填成空串");
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_connect() {
        let db = PostgresNativeDatabase::new(PG_URL).await;
        assert!(db.is_ok(), "连接 PostgreSQL 失败: {:?}", db.err());
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_query_select_one() -> Result<(), CoreError> {
        let db = PostgresNativeDatabase::new(PG_URL).await?;
        let result = db.query("SELECT 1 AS val").await?;
        assert_eq!(result.columns, vec!["val"]);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_meta() -> Result<(), CoreError> {
        let db = PostgresNativeDatabase::new(PG_URL).await?;
        let meta = db.meta();
        assert!(meta.supports_transaction);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_is_read_only_flag() -> Result<(), CoreError> {
        let db = PostgresNativeDatabase::new(PG_URL).await?;
        let result = db.query("SELECT 1").await?;
        assert_eq!(result.is_read_only, Some(true));
        Ok(())
    }
}
