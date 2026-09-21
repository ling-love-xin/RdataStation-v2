//! MySQL 官方 Rust 驱动实现（基于 mysql_async 0.34）
//!
//! 与 sqlx 驱动的区别：
//! - mysql_async 是 MySQL 官方维护的纯 Rust 异步驱动
//! - 支持更多 MySQL 特定功能（协议压缩、原生认证插件等）
//! - 与 sqlx 驱动并存，用户可在连接时选择驱动

use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::driver::traits::MetadataBrowser;
use crate::driver::utils::{affected_rows_result, returns_rows};
use crate::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, IndexDetail, NodeDetail, NodeInfo,
    PoolStatus, SchemaObjectKind, Transaction,
};
use crate::driver::utils::MY_LIST_CONSTRAINTS_SQL;
use shared::error::{ConnectionError, CoreError, DatabaseError};
use shared::models::{ArrowBatch, QueryResult, Value};

// ============================================================================
// TLS 连接器构造
// ============================================================================

/// 按 TLS 请求构造 mysql_async 的 `SslOpts`（纯函数，可测）。
///
/// 依据（`mysql_async 0.37` + `native-tls` 后端，改前复读 `src/opts/mod.rs` 与
/// `src/opts/native_tls_opts.rs`）：
/// - 证书链 / 主机名校验是两个 `danger_*` 开关，默认**都是开校验**——
///   「加密不校验」必须显式关掉（与 URL 上的 `verify_ca=false` 同一口径）；
/// - CA 走 `with_root_certs`——注意其元素类型 `PathOrBuf` **未在 crate 根导出**
///   （`mod opts` 是私有的），但类型推断可用：`vec![PathBuf::from(ca).into()]` 能解析到
///   `From<PathBuf> for PathOrBuf<'static>`（已实测可编译）；
/// - **客户端证书只接受 PKCS#12 存档**（`ClientIdentity::new(pkcs12)`），
///   PEM 证书 + 私钥两件套在该后端下无法表达 → 明确报错并引导用 sqlx 驱动，不静默丢弃。
pub(crate) fn mysql_async_ssl_opts(
    req: &connection::config::TlsRequest,
) -> Result<mysql_async::SslOpts, CoreError> {
    let mut opts = mysql_async::SslOpts::default()
        .with_danger_accept_invalid_certs(!req.verifies_chain())
        .with_danger_skip_domain_validation(!req.verifies_hostname());

    if let Some(ca) = req.ca_path() {
        opts = opts.with_root_certs(vec![std::path::PathBuf::from(ca).into()]);
    }

    match (req.client_cert_path(), req.client_key_path()) {
        (None, None) => {}
        (Some(cert), None) if is_pkcs12_archive(cert) => {
            opts = opts.with_client_identity(Some(mysql_async::ClientIdentity::new(
                std::path::PathBuf::from(cert).into(),
            )));
        }
        (Some(cert), None) => {
            return Err(tls_unsupported(format!(
                "客户端证书 `{cert}` 不是 PKCS#12 存档（.p12 / .pfx）：\
                 mysql_async 的 native-tls 后端只接受 PKCS#12 客户端身份。\
                 请改用「MySQL (sqlx)」驱动（它直接接受 PEM 证书 + 私钥）"
            )))
        }
        (Some(_), Some(_)) => {
            return Err(tls_unsupported(
                "mysql_async 的 native-tls 后端不接受 PEM 证书 + 私钥两件套\
                 （只接受 PKCS#12 存档）：请改用「MySQL (sqlx)」驱动，\
                 或把证书导出为 .p12/.pfx 后只填「客户端证书」一栏"
                    .to_string(),
            ))
        }
        (None, Some(_)) => {
            return Err(tls_unsupported(
                "填了客户端私钥但没有客户端证书：两者需成对（或只给 PKCS#12 存档）".to_string(),
            ))
        }
    }

    Ok(opts)
}

/// 是否像 PKCS#12 存档（按扩展名，大小写不敏感）。
fn is_pkcs12_archive(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower.ends_with(".p12") || lower.ends_with(".pfx")
}

fn tls_unsupported(reason: String) -> CoreError {
    CoreError::connection(ConnectionError::NotSupported(format!(
        "mysql_native: {reason}"
    )))
}

// ============================================================================
// MySQL Native Database 结构体
// ============================================================================

/// MySQL 官方驱动数据库实例
///
/// 封装 `mysql_async::Pool` 连接池，通过 `Database` trait 提供统一的查询/执行接口。
/// 元数据浏览通过 `MetadataBrowser` trait 实现，查询 `information_schema`。
pub struct MySqlNativeDatabase {
    pool: mysql_async::Pool,
    server_version: Option<String>,
    max_connections: usize,
    min_connections: usize,
}

impl MySqlNativeDatabase {
    /// 从连接 URL 创建新的 MySQL 数据库实例
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        Self::new_with_tls(url, None).await
    }

    /// 从连接 URL + 结构化 TLS 请求创建实例。
    ///
    /// URL 上的 `require_ssl` / `verify_ca` / `verify_identity` 只能表达「要不要加密」，
    /// **CA 与客户端证书必须走这里**（`SslOpts`）。
    pub async fn new_with_tls(
        url: &str,
        tls: Option<&connection::config::TlsRequest>,
    ) -> Result<Self, CoreError> {
        let opts = mysql_async::Opts::from_url(url).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "parse_url".to_string(),
                source: e.to_string(),
            })
        })?;
        let mut builder = mysql_async::OptsBuilder::from_opts(opts);
        if let Some(req) = tls {
            builder = builder.ssl_opts(Some(mysql_async_ssl_opts(req)?));
        }
        let pool = mysql_async::Pool::new(builder);
        // 验证连接并获取版本号
        let mut conn = pool.get_conn().await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        let server_version = {
            use mysql_async::prelude::Queryable;
            let mut result = conn.query_iter("SELECT VERSION()").await.map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql_native".to_string(),
                    operation: "version".to_string(),
                    source: e.to_string(),
                })
            })?;
            let rows: Vec<mysql_async::Row> = result.collect().await.map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql_native".to_string(),
                    operation: "version".to_string(),
                    source: e.to_string(),
                })
            })?;
            rows.first().and_then(|r| {
                let val: mysql_async::Value = r.get(0).unwrap_or(mysql_async::Value::NULL);
                match val {
                    mysql_async::Value::Bytes(b) => String::from_utf8(b).ok(),
                    _ => None,
                }
            })
        };
        Ok(Self {
            pool,
            server_version,
            max_connections: 10,
            min_connections: 0,
        })
    }

    /// 从现有连接池创建实例
    pub fn from_pool(pool: mysql_async::Pool) -> Self {
        Self {
            pool,
            server_version: None,
            max_connections: 10,
            min_connections: 0,
        }
    }

    /// 从现有连接池及配置创建实例
    pub fn from_pool_with_config(
        pool: mysql_async::Pool,
        max_connections: usize,
        min_connections: usize,
    ) -> Self {
        Self {
            pool,
            server_version: None,
            max_connections,
            min_connections,
        }
    }
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

/// 写语句（不返回行）跑 `exec_drop`，再从连接上读**真实影响行数**（B5 / P0.6）
///
/// `exec_iter` 路径对 DML 只能给出「无结果集」这个事实，拿不到计数；
/// `exec_drop` 会丢弃结果集，随后 `Conn::affected_rows()` 给出服务器报的行数。
/// mysql_async 错误 → 引擎错误；**带上数据库指认的对象**（表 / 列 / 约束）。
///
/// MySQL 的协议层没有这些字段（那是 PG 才有的），所以从消息文本里认 ——
/// 形态与样本见 `crate::driver::error_location::mysql`。拿不到就是 `None`，
/// 界面不写一句空话。
fn mysql_native_query_error(sql: &str, error: &mysql_async::Error) -> CoreError {
    let message = error.to_string();
    let location = crate::driver::error_location::mysql(&message);
    CoreError::database(DatabaseError::query(sql, message).with_location(location))
}

async fn execute_writing(
    conn: &mut mysql_async::Conn,
    sql: &str,
    params: mysql_async::Params,
) -> Result<QueryResult, CoreError> {
    use mysql_async::prelude::Queryable;
    conn.exec_drop(sql, params)
        .await
        .map_err(|e| mysql_native_query_error(sql, &e))?;
    Ok(affected_rows_result(conn.affected_rows()))
}

// ============================================================================
// Arrow 转换
// ============================================================================

/// 将 mysql_async Row 转换为 Arrow 批处理
fn mysql_native_rows_to_arrow(
    columns: &[String],
    rows: &[mysql_async::Row],
) -> Result<ArrowBatch, CoreError> {
    let num_rows = rows.len();
    let num_cols = columns.len();
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(num_cols);

    for col_idx in 0..num_cols {
        let mut string_values: Vec<Option<String>> = Vec::with_capacity(num_rows);
        let mut int_values: Vec<Option<i64>> = Vec::with_capacity(num_rows);
        let mut float_values: Vec<Option<f64>> = Vec::with_capacity(num_rows);
        let mut bool_values: Vec<Option<bool>> = Vec::with_capacity(num_rows);
        let mut binary_values: Vec<Option<Vec<u8>>> = Vec::with_capacity(num_rows);

        // 遍历所有行确定最宽类型（0=Null, 1=Bool, 2=Int64, 3=Float64, 4=Binary, 5=Utf8）
        let mut detected_rank: u8 = 0;

        for row in rows {
            let val: mysql_async::Value = row.get(col_idx).unwrap_or(mysql_async::Value::NULL);
            let row_rank = match &val {
                mysql_async::Value::NULL => 0,
                mysql_async::Value::Int(_) | mysql_async::Value::UInt(_) => {
                    // Check if it could be a bool (0/1)
                    match &val {
                        mysql_async::Value::Int(0) | mysql_async::Value::Int(1)
                            if detected_rank < 1 =>
                        {
                            1
                        }
                        _ => 2,
                    }
                }
                mysql_async::Value::Float(_) | mysql_async::Value::Double(_) => 3,
                mysql_async::Value::Bytes(b) => {
                    if std::str::from_utf8(b).is_ok() {
                        5
                    } else {
                        4
                    }
                }
                mysql_async::Value::Date(..) | mysql_async::Value::Time(..) => 5,
            };
            if row_rank > detected_rank {
                detected_rank = row_rank;
            }
            if detected_rank == 5 {
                break; // Utf8 为最宽类型
            }
        }

        // If the only non-null values are 0 and 1, treat as bool
        let effective_type = match detected_rank {
            1 => DataType::Boolean,
            2 => DataType::Int64,
            3 => DataType::Float64,
            4 => DataType::Binary,
            _ => DataType::Utf8,
        };

        for row in rows {
            let val: mysql_async::Value = row.get(col_idx).unwrap_or(mysql_async::Value::NULL);
            match effective_type {
                DataType::Boolean => {
                    let b = match &val {
                        mysql_async::Value::Int(1) => Some(true),
                        mysql_async::Value::Int(0) => Some(false),
                        mysql_async::Value::NULL => None,
                        _ => None,
                    };
                    bool_values.push(b);
                }
                DataType::Int64 => {
                    let i = match &val {
                        mysql_async::Value::Int(v) => Some(*v),
                        mysql_async::Value::UInt(v) => Some(*v as i64),
                        mysql_async::Value::NULL => None,
                        _ => None,
                    };
                    int_values.push(i);
                }
                DataType::Float64 => {
                    let f = match &val {
                        mysql_async::Value::Float(v) => Some(*v as f64),
                        mysql_async::Value::Double(v) => Some(*v),
                        mysql_async::Value::NULL => None,
                        _ => None,
                    };
                    float_values.push(f);
                }
                DataType::Binary => {
                    let b = match &val {
                        mysql_async::Value::Bytes(v) => Some(v.clone()),
                        mysql_async::Value::NULL => None,
                        _ => None,
                    };
                    binary_values.push(b);
                }
                _ => {
                    let s = match &val {
                        mysql_async::Value::NULL => None,
                        mysql_async::Value::Bytes(b) => {
                            Some(String::from_utf8_lossy(b).to_string())
                        }
                        mysql_async::Value::Int(v) => Some(v.to_string()),
                        mysql_async::Value::UInt(v) => Some(v.to_string()),
                        mysql_async::Value::Float(v) => Some(v.to_string()),
                        mysql_async::Value::Double(v) => Some(v.to_string()),
                        mysql_async::Value::Date(y, m, d, h, min, s, _us) => Some(format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                            y, m, d, h, min, s
                        )),
                        mysql_async::Value::Time(neg, days, hours, minutes, seconds, _us) => {
                            let days = *days as i64;
                            let hours = *hours as i64;
                            let minutes = *minutes as i64;
                            let seconds = *seconds as i64;
                            let total = if *neg {
                                -(days * 86400 + hours * 3600 + minutes * 60 + seconds)
                            } else {
                                days * 86400 + hours * 3600 + minutes * 60 + seconds
                            };
                            Some(total.to_string())
                        }
                    };
                    string_values.push(s);
                }
            }
        }

        let array: ArrayRef = match effective_type {
            DataType::Boolean => Arc::new(BooleanArray::from(bool_values)),
            DataType::Int64 => Arc::new(Int64Array::from(int_values)),
            DataType::Float64 => Arc::new(Float64Array::from(float_values)),
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
            db_type: "mysql_native".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

// ============================================================================
// 辅助: 从 QueryResult 解析 NodeInfo 列表
// ============================================================================

/// 单列名字结果集 → 对象列表（catalog / schema / 例程等只在内省里露名字的类别）。
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

// ============================================================================
// build_query_result 辅助函数
// ============================================================================

fn build_query_result(
    columns: &[String],
    rows: &[mysql_async::Row],
    _is_read_only: bool,
) -> Result<QueryResult, CoreError> {
    if rows.is_empty() {
        return Ok(QueryResult {
            columns: columns.to_vec(),
            batches: vec![],
            ..Default::default()
        });
    }
    let batch = mysql_native_rows_to_arrow(columns, rows)?;
    Ok(QueryResult {
        columns: columns.to_vec(),
        batches: vec![batch],
        ..Default::default()
    })
}

// ============================================================================
// Database trait 实现
// ============================================================================

#[async_trait::async_trait]
impl Database for MySqlNativeDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        use mysql_async::prelude::Queryable;
        let is_read_only = is_read_only_sql(sql);
        let mut conn = self.pool.get_conn().await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "acquire".to_string(),
                source: e.to_string(),
            })
        })?;

        // B5 / P0.6：不返回结果集的写语句走 `exec_drop`，拿驱动的真实影响行数
        if !is_read_only && !returns_rows(sql) {
            return execute_writing(&mut conn, sql, mysql_async::Params::Empty).await;
        }

        let mut result = conn
            .query_iter(sql)
            .await
            .map_err(|e| mysql_native_query_error(sql, &e))?;

        let columns: Vec<String> = result
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();

        let rows: Vec<mysql_async::Row> = result
            .collect()
            .await
            .map_err(|e| mysql_native_query_error(sql, &e))?;

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<QueryResult, CoreError> {
        use mysql_async::prelude::Queryable;
        let is_read_only = is_read_only_sql(sql);
        let mut conn = self.pool.get_conn().await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "acquire".to_string(),
                source: e.to_string(),
            })
        })?;

        let mysql_params: Vec<mysql_async::Value> = params
            .iter()
            .map(|v| match v {
                Value::Null => mysql_async::Value::NULL,
                Value::Bool(b) => mysql_async::Value::Int(if *b { 1 } else { 0 }),
                Value::Int(i) => mysql_async::Value::Int(*i),
                Value::Float(f) => mysql_async::Value::Double(*f),
                Value::Text(s) => mysql_async::Value::Bytes(s.as_bytes().to_vec()),
                Value::Bytes(b) => mysql_async::Value::Bytes(b.clone()),
            })
            .collect();

        // B5 / P0.6：带参数的写语句同样给真实影响行数
        if !is_read_only && !returns_rows(sql) {
            return execute_writing(
                &mut conn,
                sql,
                mysql_async::Params::Positional(mysql_params),
            )
            .await;
        }

        let params_ref: Vec<&mysql_async::Value> = mysql_params.iter().collect();

        let mut result = conn
            .exec_iter(sql, params_ref)
            .await
            .map_err(|e| mysql_native_query_error(sql, &e))?;

        let columns: Vec<String> = result
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();

        let rows: Vec<mysql_async::Row> = result
            .collect()
            .await
            .map_err(|e| mysql_native_query_error(sql, &e))?;

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_cancel(
        &self,
        sql: &str,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<QueryResult, CoreError> {
        let sql_owned = sql.to_string();
        let sql_for_cancel = sql.to_string();
        let pool = self.pool.clone();

        tokio::select! {
            result = async move {
                use mysql_async::prelude::Queryable;
                let is_read_only = is_read_only_sql(&sql_owned);
                let mut conn = pool.get_conn().await.map_err(|e| {
                    CoreError::database(DatabaseError::Driver {
                        db_type: "mysql_native".to_string(),
                        operation: "acquire".to_string(),
                        source: e.to_string(),
                    })
                })?;

                // B5 / P0.6：不返回结果集的写语句走 `exec_drop`，拿驱动的真实影响行数
                if !is_read_only && !returns_rows(&sql_owned) {
                    return execute_writing(
                        &mut conn,
                        &sql_owned,
                        mysql_async::Params::Empty,
                    )
                    .await;
                }

                let mut query_result = conn.query_iter(&sql_owned).await.map_err(|e| {
                    mysql_native_query_error(&sql_owned, &e)
                })?;

                let columns: Vec<String> = query_result
                    .columns_ref()
                    .iter()
                    .map(|c| c.name_str().to_string())
                    .collect();

                let rows: Vec<mysql_async::Row> = query_result.collect().await.map_err(|e| {
                    mysql_native_query_error(&sql_owned, &e)
                })?;

                build_query_result(&columns, &rows, is_read_only)
            } => result,
            _ = cancel_token.cancelled() => {
                Err(CoreError::database(DatabaseError::Query {
                    sql: sql_for_cancel,
                    reason: "Query cancelled".to_string(),
                    position: None,
                    location: None,
                }))
            }
        }
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError> {
        use mysql_async::prelude::Queryable;
        let mut conn = self.pool.get_conn().await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "acquire".to_string(),
                source: e.to_string(),
            })
        })?;
        conn.query_drop("START TRANSACTION").await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;

        Ok(Box::new(MySqlNativeTransaction {
            conn: Mutex::new(Some(conn)),
        }))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::mysql_native()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        use mysql_async::prelude::Queryable;
        let mut conn = self.pool.get_conn().await.map_err(|e| {
            CoreError::connection(ConnectionError::Other {
                conn_id: "mysql_native".to_string(),
                reason: format!("Ping failed: {}", e),
            })
        })?;
        conn.ping().await.map_err(|e| {
            CoreError::connection(ConnectionError::Other {
                conn_id: "mysql_native".to_string(),
                reason: format!("Ping failed: {}", e),
            })
        })
    }

    async fn pool_status(&self) -> Option<PoolStatus> {
        // mysql_async Pool 不直接暴露连接数统计，返回固定值
        Some(PoolStatus {
            size: self.max_connections,
            idle: 0,
            active: 0,
            waiting: 0,
            max_connections: self.max_connections,
            min_connections: self.min_connections,
        })
    }

    /// 列举约束 —— **与 `mysql`（sqlx）共用同一份 SQL**（`utils::MY_LIST_CONSTRAINTS_SQL`）。
    ///
    /// 本方法此前没实现 → 落到 trait 默认空实现 → 属性面板的「约束」分区对官方 MySQL
    /// 驱动同样恒为空。
    async fn list_constraints(
        &self,
        _catalog: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        let sql = MY_LIST_CONSTRAINTS_SQL;
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(schema.unwrap_or_default().to_string()),
                    Value::Text(table.to_string()),
                ],
            )
            .await?;

        let mut out: Vec<ConstraintDetail> = Vec::new();
        let Some(batch) = result.batches.first() else {
            return Ok(out);
        };
        let col = |name: &str| {
            batch
                .column_by_name(name)
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>())
        };
        let (Some(cname), Some(ctype), Some(cols), Some(ref_table), Some(ref_cols), Some(upd), Some(del)) = (
            col("cname"),
            col("ctype"),
            col("cols"),
            col("ref_table"),
            col("ref_cols"),
            col("upd"),
            col("del"),
        ) else {
            return Ok(out);
        };

        let split = |text: &str| -> Vec<String> {
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };

        for row in 0..batch.num_rows() {
            let ref_table = ref_table.value(row);
            out.push(ConstraintDetail {
                name: cname.value(row).to_string(),
                table_name: table.to_string(),
                constraint_type: ctype.value(row).to_string(),
                column_names: split(cols.value(row)),
                referenced_table: (!ref_table.is_empty()).then(|| ref_table.to_string()),
                referenced_columns: split(ref_cols.value(row)),
                update_rule: (!upd.value(row).is_empty()).then(|| upd.value(row).to_string()),
                delete_rule: (!del.value(row).is_empty()).then(|| del.value(row).to_string()),
            });
        }
        Ok(out)
    }

    /// 源版 DDL：`SHOW CREATE TABLE`（对视图也有效 —— 返回的列名是 `Create View`）。
    async fn get_table_ddl(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Option<String>, CoreError> {
        use crate::driver::utils::quote_identifier;
        let sql = format!(
            "SHOW CREATE TABLE {}.{}",
            quote_identifier(catalog, '`'),
            quote_identifier(table, '`')
        );
        let result = self.query(&sql).await?;
        Ok(crate::driver::utils::create_statement(&result))
    }

    fn as_metadata_browser(&self) -> Option<&dyn MetadataBrowser> {
        Some(self)
    }

    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        self.get_catalogs()
            .await
            .map(|nodes| nodes.into_iter().map(|n| n.name).collect())
    }

    async fn list_tables(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        // MySQL 的 database 即 schema（见 `has_schema_level`）。
        self.get_tables(catalog, catalog).await
    }

    async fn list_columns(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let detail = self.get_table_detail(catalog, catalog, table).await?;
        Ok(detail.columns)
    }

    async fn list_procedures(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let sql = "\
            SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE' \
             ORDER BY ROUTINE_NAME";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Procedure))
    }

    async fn list_functions(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let sql = "\
            SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION' \
             ORDER BY ROUTINE_NAME";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Function))
    }

    async fn get_routine_source(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        name: &str,
        kind: SchemaObjectKind,
    ) -> Result<Option<String>, CoreError> {
        let stmt_type = match kind {
            SchemaObjectKind::Procedure => "PROCEDURE",
            SchemaObjectKind::Function => "FUNCTION",
            _ => return Ok(None),
        };
        let esc_catalog = catalog.replace('`', "``");
        let esc_name = name.replace('`', "``");
        let sql = format!("SHOW CREATE {} `{}`.`{}`", stmt_type, esc_catalog, esc_name);
        let result = self.query(&sql).await?;
        Ok(crate::driver::utils::create_statement(&result))
    }
}

// ============================================================================
// Transaction 实现
// ============================================================================

/// MySQL 原生事务句柄
///
/// 封装 `mysql_async::Conn` 并保持事务状态。
/// Drop 时连接归还池（mysql_async 会自动回滚未提交的事务）。
pub struct MySqlNativeTransaction {
    conn: Mutex<Option<mysql_async::Conn>>,
}

#[async_trait::async_trait]
impl Transaction for MySqlNativeTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        use mysql_async::prelude::Queryable;
        let mut guard = self.conn.lock().await;
        if let Some(ref mut conn) = *guard {
            let is_read_only = is_read_only_sql(sql);

            // B5 / P0.6：事务内的写语句同样要真实影响行数
            if !is_read_only && !returns_rows(sql) {
                return execute_writing(conn, sql, mysql_async::Params::Empty).await;
            }

            let mut result = conn
                .query_iter(sql)
                .await
                .map_err(|e| mysql_native_query_error(sql, &e))?;

            let columns: Vec<String> = result
                .columns_ref()
                .iter()
                .map(|c| c.name_str().to_string())
                .collect();

            let rows: Vec<mysql_async::Row> = result
                .collect()
                .await
                .map_err(|e| mysql_native_query_error(sql, &e))?;

            build_query_result(&columns, &rows, is_read_only)
        } else {
            Err(CoreError::database(DatabaseError::Driver {
                db_type: "mysql_native".to_string(),
                operation: "query".to_string(),
                source: "Transaction already closed".to_string(),
            }))
        }
    }

    async fn commit(&mut self) -> Result<(), CoreError> {
        use mysql_async::prelude::Queryable;
        let mut guard = self.conn.lock().await;
        if let Some(ref mut conn) = *guard {
            conn.query_drop("COMMIT").await.map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql_native".to_string(),
                    operation: "commit".to_string(),
                    source: e.to_string(),
                })
            })?;
        }
        // Return conn to pool by dropping
        *guard = None;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CoreError> {
        use mysql_async::prelude::Queryable;
        let mut guard = self.conn.lock().await;
        if let Some(ref mut conn) = *guard {
            if let Err(e) = conn.query_drop("ROLLBACK").await {
                tracing::warn!("MySQL native transaction rollback error: {}", e);
            }
        }
        *guard = None;
        Ok(())
    }
}

// ============================================================================
// MetadataBrowser trait 实现
// ============================================================================

#[async_trait::async_trait]
impl MetadataBrowser for MySqlNativeDatabase {
    fn has_schema_level(&self) -> bool {
        false
    }

    async fn get_catalogs(&self) -> Result<Vec<NodeInfo>, CoreError> {
        let result = self
            .query("SELECT schema_name FROM information_schema.schemata ORDER BY schema_name")
            .await?;
        Ok(rows_to_node_info(&result, SchemaObjectKind::Catalog))
    }

    async fn get_schemas(&self, _catalog: &str) -> Result<Vec<NodeInfo>, CoreError> {
        // MySQL 的 database 即 schema，无独立 Schema 层（见 `has_schema_level`）。
        Ok(vec![])
    }

    async fn get_tables(&self, catalog: &str, _schema: &str) -> Result<Vec<NodeInfo>, CoreError> {
        use arrow::array::StringArray;
        let sql = "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = ? ORDER BY table_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
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
        _schema: &str,
        table: &str,
    ) -> Result<NodeDetail, CoreError> {
        let sql = crate::driver::utils::MY_TABLE_DETAIL_SQL;
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(catalog.to_string()),
                    Value::Text(table.to_string()),
                ],
            )
            .await?;
        let columns = crate::driver::utils::columns_from_detail_rows(&result);

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Database;

    const MYSQL_URL: &str = "mysql://root:root@localhost:3306/";

    /// TLS 请求 → `SslOpts`（纯函数，无需真机）：CA 落到 root_certs，
    /// PKCS#12 落到 client_identity，PEM 两件套（native-tls 后端不支持）**报错不静默丢弃**。
    #[test]
    fn tls_request_becomes_ssl_opts() {
        use connection::config::{SslConfig, TlsRequest};
        use connection::url_params::SslMode;

        let req = TlsRequest::new(
            SslMode::VerifyCa,
            SslConfig {
                verify_server_cert: true,
                ca_cert_path: Some("D:/certs/ca.pem".to_string()),
                client_cert_path: Some("D:/certs/client.p12".to_string()),
                client_key_path: None,
                ..Default::default()
            },
        );
        let opts = mysql_async_ssl_opts(&req).expect("CA + PKCS#12 应可构造");
        // `PathOrBuf` 未在 crate 根导出（不可命名），但方法可调用
        assert_eq!(opts.root_certs().len(), 1);
        assert!(opts.client_identity().is_some());

        // 不带任何 TLS 请求 → 只用 URL 上的三个布尔
        let plain = TlsRequest::new(SslMode::Disable, SslConfig::default());
        let opts = mysql_async_ssl_opts(&plain).expect("空请求也可构造");
        assert_eq!(opts.root_certs().len(), 0);
        assert!(opts.client_identity().is_none());

        // PEM 证书 + 私钥：native-tls 后端无法表达 → 可见错误（引导用 sqlx 驱动）
        let pem = TlsRequest::new(
            SslMode::Require,
            SslConfig {
                verify_server_cert: false,
                client_cert_path: Some("D:/certs/client.pem".to_string()),
                client_key_path: Some("D:/certs/client.key".to_string()),
                ..Default::default()
            },
        );
        let err = mysql_async_ssl_opts(&pem).expect_err("PEM 两件套应报错");
        assert!(err.to_string().contains("PKCS#12"), "{err}");

        // 只有私钥没有证书：同样报错
        let key_only = TlsRequest::new(
            SslMode::Require,
            SslConfig {
                verify_server_cert: false,
                client_key_path: Some("D:/certs/client.key".to_string()),
                ..Default::default()
            },
        );
        assert!(mysql_async_ssl_opts(&key_only).is_err());
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_connect() {
        let db = MySqlNativeDatabase::new(MYSQL_URL).await;
        assert!(db.is_ok(), "Failed to connect to MySQL: {:?}", db.err());
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_query_select_one() -> Result<(), CoreError> {
        let db = MySqlNativeDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT 1 AS val").await?;
        assert_eq!(result.columns, vec!["val"]);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_meta() -> Result<(), CoreError> {
        let db = MySqlNativeDatabase::new(MYSQL_URL).await?;
        let meta = db.meta();
        assert!(meta.supports_transaction);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_is_read_only_flag() -> Result<(), CoreError> {
        let db = MySqlNativeDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT 1").await?;
        assert_eq!(result.is_read_only, Some(true));
        Ok(())
    }
}
