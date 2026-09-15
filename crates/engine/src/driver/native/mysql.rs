use sqlx::{Column, MySql, Pool, Row, TypeInfo as _};

fn names_to_schema_objects(
    result: &QueryResult,
    kind: crate::driver::SchemaObjectKind,
) -> Vec<crate::driver::SchemaObject> {
    use arrow::array::StringArray;
    let mut objects: Vec<crate::driver::SchemaObject> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        objects.push(crate::driver::SchemaObject {
                            name: name.to_string(),
                            kind: kind.clone(),
                            children: None,
                            comment: None,
                            table_name: None,
                            event: None,
                        });
                    }
                }
            }
        }
    }
    objects
}
use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use crate::driver::traits::MetadataBrowser;
use crate::driver::{ColumnDetail, DataSourceMeta, Database, PoolStatus, Transaction};
use crate::driver::{IndexDetail, SchemaObject, SchemaObjectKind};
use shared::error::{ConnectionError, CoreError, DatabaseError};
use shared::models::{ArrowBatch, QueryResult, Value};

/// MySQL 数据库连接
///
/// 封装 `sqlx::Pool<MySql>` 连接池，通过 `Database` trait 提供统一的查询/执行接口。
/// 元数据浏览通过 `MetadataBrowser` trait 实现，查询 `information_schema`。
///
/// # 字段
/// * `pool` - sqlx MySQL 连接池，管理连接复用和生命周期
/// * `server_version` - MySQL 服务器版本号，首次连接时获取并缓存
pub struct MySqlDatabase {
    pool: Pool<MySql>,
    server_version: Option<String>,
    max_connections: usize,
    min_connections: usize,
}

impl MySqlDatabase {
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        // 对 URL 中的密码进行脱敏
        let masked_url = if let Some(at_pos) = url.find('@') {
            if let Some(colon_pos) = url[..at_pos].rfind(':') {
                format!("{}:****{}", &url[..colon_pos], &url[at_pos..])
            } else {
                url.to_string()
            }
        } else {
            url.to_string()
        };
        tracing::info!(
            "MySqlDatabase::new: url_has_creds={}, url_len={}, masked_url={}",
            url.contains('@'),
            url.len(),
            masked_url
        );

        // MySQL 8.0 默认使用 caching_sha2_password 认证插件，可能需要在 URL 中禁用 SSL
        // 或配置 TLS。添加 ssl-mode=disabled 以兼容本地开发环境。
        let effective_url = if url.contains("ssl-mode") || url.contains("ssl_mode") {
            url.to_string()
        } else {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{}{}ssl-mode=disabled", url, sep)
        };

        let pool = Pool::connect(&effective_url).await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        let server_version = sqlx::query_scalar::<_, String>("SELECT VERSION()")
            .fetch_one(&pool)
            .await
            .ok();
        Ok(Self {
            pool,
            server_version,
            max_connections: 10,
            min_connections: 0,
        })
    }

    pub fn from_pool(pool: Pool<MySql>) -> Self {
        Self {
            pool,
            server_version: None,
            max_connections: 10,
            min_connections: 0,
        }
    }

    pub fn from_pool_with_config(
        pool: Pool<MySql>,
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

fn is_read_only_sql(sql: &str) -> bool {
    let sql_upper = sql.trim_start().to_uppercase();
    sql_upper.starts_with("SELECT")
        || sql_upper.starts_with("SHOW")
        || sql_upper.starts_with("DESCRIBE")
        || sql_upper.starts_with("EXPLAIN")
        || sql_upper.starts_with("SET")
}

#[async_trait::async_trait]
impl Database for MySqlDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let is_read_only = is_read_only_sql(sql);

        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            vec![]
        };

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<shared::models::Value>,
    ) -> Result<QueryResult, CoreError> {
        let is_read_only = is_read_only_sql(sql);

        let mut query_builder = sqlx::query(sqlx::AssertSqlSafe(sql));

        for param in &params {
            query_builder = match param {
                shared::models::Value::Null => query_builder.bind(None::<String>),
                shared::models::Value::Bool(v) => query_builder.bind(*v),
                shared::models::Value::Int(v) => query_builder.bind(*v),
                shared::models::Value::Float(v) => query_builder.bind(*v),
                shared::models::Value::Text(v) => query_builder.bind(v),
                shared::models::Value::Bytes(v) => query_builder.bind(v.clone()),
            };
        }

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            vec![]
        };

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
                let is_read_only = is_read_only_sql(&sql_owned);

                let rows = sqlx::query(sqlx::AssertSqlSafe(sql_owned.as_str()))
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

                let columns: Vec<String> = if let Some(first) = rows.first() {
                    first.columns().iter().map(|c| c.name().to_string()).collect()
                } else {
                    vec![]
                };

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
        let tx = self.pool.begin().await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;
        Ok(Box::new(MySqlTransaction::new(tx)))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::mysql()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                CoreError::connection(ConnectionError::Other {
                    conn_id: "mysql".to_string(),
                    reason: format!("Ping failed: {}", e),
                })
            })?;
        Ok(())
    }

    async fn pool_status(&self) -> Option<PoolStatus> {
        let size = self.pool.size() as usize;
        let idle = self.pool.num_idle();
        Some(PoolStatus {
            size,
            idle,
            active: size.saturating_sub(idle),
            waiting: 0,
            max_connections: self.max_connections,
            min_connections: self.min_connections,
        })
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
    ) -> Result<Vec<crate::driver::SchemaObject>, CoreError> {
        let nodes = self.get_tables(catalog, catalog).await?;
        Ok(nodes
            .into_iter()
            .map(|n| crate::driver::SchemaObject {
                name: n.name,
                kind: n.kind,
                children: None,
                comment: n.comment,
                table_name: None,
                event: None,
            })
            .collect())
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

    async fn list_indexes(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        let sql = format!("SHOW INDEX FROM `{}`.`{}`", catalog, table);
        let result = self.query_with_params(&sql, vec![]).await?;

        let mut indexes: Vec<IndexDetail> = Vec::new();
        if let Some(batch) = result.batches.first() {
            let name_col = batch
                .column_by_name("Key_name")
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>());
            let col_col = batch
                .column_by_name("Column_name")
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>());
            let non_unique_col = batch
                .column_by_name("Non_unique")
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::Int64Array>());

            if let (Some(name_arr), Some(col_arr)) = (name_col, col_col) {
                let mut seen: std::collections::HashMap<String, IndexDetail> =
                    std::collections::HashMap::new();
                for row_idx in 0..batch.num_rows() {
                    let name = name_arr.value(row_idx).to_string();
                    let col_name = col_arr.value(row_idx).to_string();
                    let is_unique = non_unique_col
                        .map(|a| a.value(row_idx) == 0)
                        .unwrap_or(false);

                    seen.entry(name.clone())
                        .and_modify(|idx| {
                            idx.column_names.push(col_name.clone());
                        })
                        .or_insert(IndexDetail {
                            name,
                            table_name: table.to_string(),
                            column_names: vec![col_name],
                            is_unique,
                            is_primary: false,
                            index_type: None,
                            comment: None,
                        });
                }
                indexes = seen.into_values().collect();
            }
        }
        Ok(indexes)
    }

    async fn list_procedures(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let sql = "\
            SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE' \
             ORDER BY ROUTINE_NAME";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(names_to_schema_objects(
            &result,
            SchemaObjectKind::Procedure,
        ))
    }

    async fn list_functions(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let sql = "\
            SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION' \
             ORDER BY ROUTINE_NAME";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(names_to_schema_objects(&result, SchemaObjectKind::Function))
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
        let sql = format!("SHOW CREATE {} `{}`.`{}`", stmt_type, esc_catalog, esc_name,);
        let result = self.query(&sql).await?;
        // `SHOW CREATE PROCEDURE|FUNCTION` 的列序是（名称, sql_mode, Create …, …）：
        // 必须取名为 `Create …` 的那一列。旧实现恒取列 1，拿到的是 **sql_mode**
        // 而非 DDL（本路径此前无调用方，故一直未暴露）。
        let col_idx = result
            .columns
            .iter()
            .position(|c| c.starts_with("Create "))
            .unwrap_or(2);
        if let Some(batch) = result.batches.first() {
            if batch.num_rows() > 0 && col_idx < batch.num_columns() {
                if let Some(col) = batch
                    .column(col_idx)
                    .as_any()
                    .downcast_ref::<arrow::array::StringArray>()
                {
                    return Ok(Some(col.value(0).to_string()));
                }
            }
        }
        Ok(None)
    }

    fn as_metadata_browser(&self) -> Option<&dyn crate::driver::MetadataBrowser> {
        Some(self)
    }
}

/// MySQL 事务句柄
///
/// 封装 `sqlx::Transaction`，支持 begin/commit/rollback。
/// Drop 时若未提交且未回滚则自动回滚，避免悬挂事务。
pub struct MySqlTransaction {
    tx: Option<sqlx::Transaction<'static, MySql>>,
}

impl MySqlTransaction {
    fn new(tx: sqlx::Transaction<'static, MySql>) -> Self {
        Self { tx: Some(tx) }
    }
}

#[async_trait::async_trait]
impl Transaction for MySqlTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        if let Some(ref mut tx) = self.tx {
            let sql_upper = sql.trim_start().to_uppercase();
            let _is_read_only = sql_upper.starts_with("SELECT")
                || sql_upper.starts_with("SHOW")
                || sql_upper.starts_with("DESCRIBE");

            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut **tx)
                .await
                .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

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

            let batch = mysql_rows_to_arrow(&columns, &rows)?;

            Ok(QueryResult {
                columns,
                batches: vec![batch],
                ..Default::default()
            })
        } else {
            Err(CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "query".to_string(),
                source: "Transaction already closed".to_string(),
            }))
        }
    }

    async fn commit(&mut self) -> Result<(), CoreError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql".to_string(),
                    operation: "commit".to_string(),
                    source: e.to_string(),
                })
            })?;
        }
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CoreError> {
        if let Some(tx) = self.tx.take() {
            if let Err(e) = tx.rollback().await {
                tracing::warn!("MySQL transaction rollback error: {}", e);
            }
        }
        Ok(())
    }
}

fn build_query_result(
    columns: &[String],
    rows: &[sqlx::mysql::MySqlRow],
    _is_read_only: bool,
) -> Result<QueryResult, CoreError> {
    if rows.is_empty() {
        return Ok(QueryResult {
            columns: columns.to_vec(),
            batches: vec![],
            ..Default::default()
        });
    }
    let batch = mysql_rows_to_arrow(columns, rows)?;
    Ok(QueryResult {
        columns: columns.to_vec(),
        batches: vec![batch],
        ..Default::default()
    })
}

/// 将 MySQL 行转换为 Arrow 批处理
/// MySQL 声明类型名是否属于文本族（含 `ENUM` / `SET` / `JSON`）。
fn is_text_type_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("CHAR")
        || n.contains("TEXT")
        || n.contains("ENUM")
        || n.contains("SET")
        || n.contains("JSON")
}

/// 字节可否视为文本。
///
/// MySQL 协议层把 TEXT 与 BLOB 都报成 `BLOB`（sqlx 名 = `BLOB`），无法只凭声明名区分，
/// 故用「可否解码为 UTF-8」作为判据；真正二进制（不可解码）才落 `Binary`。
fn bytes_are_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

/// 由 MySQL 声明类型名直接判定**数值**排行（`None` = 不是数值族，交给原有探测）
///
/// 为什么需要：MySQL 的 `BOOL` / `BOOLEAN` 在协议层就是 `TINYINT(1)`，声明名往往只报
/// `TINYINT`，无法据此区分布尔与 1/0 数值列；而 `try_get::<bool>` 对任何 1/0 列都会成功。
/// 因此数值族**一律按数值处理**——宁可把布尔显示成 `1`/`0`，也不能把计数、标志位列
/// （如 `COUNT(*)`）显示成 `true`/`false`。
fn declared_numeric_rank(name: &str) -> Option<u8> {
    Some(match name {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT" | "YEAR" => 2,
        "FLOAT" | "DOUBLE" | "REAL" | "DECIMAL" | "NUMERIC" => 3,
        _ => return None,
    })
}

fn mysql_rows_to_arrow(
    columns: &[String],
    rows: &[sqlx::mysql::MySqlRow],
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

        // 先用**声明类型**判定字符串 / 二进制。
        // 为何必须：sqlx 的 `Vec<u8>` 在 MySQL 上对 VARCHAR/TEXT 也会解码成功，若只靠
        // `try_get` 探测会把字符串列误判为 `Binary`，导致下游 `StringArray` 下转型全部失败、
        // 元数据内省（catalog / schema / table / column）一律返回空。
        let declared = rows
            .first()
            .map(|r| r.column(col_idx).type_info().name().to_ascii_uppercase())
            .unwrap_or_default();
        let declared_text = is_text_type_name(&declared);
        // 不在按声明名强制二进制：MySQL 把 TEXT 列也报成协议类型 BLOB（sqlx 名 = "BLOB"），
        // 无法只凭声明名区分 TEXT 与真 BLOB；统一交给下面的**字节 UTF-8 判定**。
        //
        // 数值族则**直接按声明名定排行**：不能再靠 `try_get::<bool>` 盲探——MySQL 的
        // `COUNT(*)` 报 BIGINT UNSIGNED，而 sqlx 会把 1/0 成功解成 `bool`，于是 bool 优先的
        // 旧逻辑把计数列判成布尔（网格里 `COUNT(*)` 显示 true）。文本/二进制仍走原有路径。
        let mut detected_rank: u8 = if declared_text {
            5
        } else {
            declared_numeric_rank(&declared).unwrap_or(0)
        };

        if detected_rank == 0 {
            for row in rows {
                use sqlx::Row;

                let row_rank = if let Ok(Some(_)) = row.try_get::<Option<bool>, _>(col_idx) {
                    1 // Boolean
                } else if let Ok(Some(_)) = row.try_get::<Option<i64>, _>(col_idx) {
                    2 // Int64
                } else if let Ok(Some(_)) = row.try_get::<Option<f64>, _>(col_idx) {
                    3 // Float64
                } else if let Ok(Some(bytes)) = row.try_get::<Option<Vec<u8>>, _>(col_idx) {
                    // 字节可解码为 UTF-8 → 文本（MySQL 会把部分文本列（如 information_schema
                    // 的 `TABLE_TYPE`）报成 BINARY 字符集）；否则才是真正的二进制。
                    if bytes_are_text(&bytes) {
                        5 // Utf8
                    } else {
                        4 // Binary
                    }
                } else if let Ok(Some(_)) = row.try_get::<Option<String>, _>(col_idx) {
                    5 // Utf8
                } else {
                    0 // NULL — 不影响类型推断
                };
                if row_rank > detected_rank {
                    detected_rank = row_rank;
                }
                if detected_rank == 5 {
                    break; // Utf8 为最宽类型，无需继续
                }
            }
        }

        let effective_type = match detected_rank {
            1 => DataType::Boolean,
            2 => DataType::Int64,
            3 => DataType::Float64,
            4 => DataType::Binary,
            _ => DataType::Utf8,
        };

        for row in rows {
            use sqlx::Row;

            match effective_type {
                DataType::Boolean => {
                    bool_values.push(row.try_get::<Option<bool>, _>(col_idx).ok().flatten());
                }
                DataType::Int64 => {
                    // 无符号列（如 `COUNT(*)` 的 BIGINT UNSIGNED）用 `i64` 解码会失败，
                    // 旧写法把失败吞成 NULL → 结果集里“计数列全空”。回退按 `u64` 解码。
                    let value = row
                        .try_get::<Option<i64>, _>(col_idx)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            row.try_get::<Option<u64>, _>(col_idx)
                                .ok()
                                .flatten()
                                .map(|v| v.min(i64::MAX as u64) as i64)
                        });
                    int_values.push(value);
                }
                DataType::Float64 => {
                    float_values.push(row.try_get::<Option<f64>, _>(col_idx).ok().flatten());
                }
                DataType::Binary => {
                    binary_values.push(row.try_get::<Option<Vec<u8>>, _>(col_idx).ok().flatten());
                }
                _ => {
                    // Utf8：优先 `String` 解码；失败则回退字节 lossy——MySQL 会把部分**文本**
                    // 列报成 BINARY 字符集，sqlx 对它们拒绝 `String` 类型解码。
                    let v = row
                        .try_get::<Option<String>, _>(col_idx)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            row.try_get::<Option<Vec<u8>>, _>(col_idx)
                                .ok()
                                .flatten()
                                .map(|b| String::from_utf8_lossy(&b).into_owned())
                        });
                    string_values.push(v);
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
            db_type: "mysql".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[async_trait::async_trait]
impl crate::driver::MetadataBrowser for MySqlDatabase {
    fn has_schema_level(&self) -> bool {
        false
    }

    async fn get_catalogs(&self) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let result = self
            .query("SELECT schema_name FROM information_schema.schemata ORDER BY schema_name")
            .await?;
        let nodes: Vec<crate::driver::NodeInfo> = (0..result.total_rows())
            .filter_map(|row_idx| {
                result.batches.iter().find_map(|batch| {
                    if row_idx < batch.num_rows() {
                        batch
                            .column(0)
                            .as_any()
                            .downcast_ref::<StringArray>()
                            .map(|arr| crate::driver::NodeInfo {
                                name: arr.value(row_idx).to_string(),
                                kind: crate::driver::SchemaObjectKind::Catalog,
                                icon: Some("database".to_string()),
                                comment: None,
                            })
                    } else {
                        None
                    }
                })
            })
            .collect();
        Ok(nodes)
    }

    async fn get_schemas(&self, _catalog: &str) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        // MySQL 的 database 即 schema，没有独立的 Schema 层级（`has_schema_level` 为 false）。
        // 返回空而非回退为 catalog 列表，避免导航树出现同名重复层。
        Ok(vec![])
    }

    async fn get_tables(
        &self,
        catalog: &str,
        _schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let sql = "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = ? ORDER BY table_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        let nodes: Vec<crate::driver::NodeInfo> = (0..result.total_rows())
            .filter_map(|row_idx| {
                result.batches.iter().find_map(|batch| {
                    if row_idx < batch.num_rows() {
                        if let (Some(name_arr), Some(type_arr)) = (
                            batch.column(0).as_any().downcast_ref::<StringArray>(),
                            batch.column(1).as_any().downcast_ref::<StringArray>(),
                        ) {
                            let table_type = type_arr.value(row_idx);
                            let kind = if table_type == "VIEW" {
                                crate::driver::SchemaObjectKind::View
                            } else {
                                crate::driver::SchemaObjectKind::Table
                            };
                            Some(crate::driver::NodeInfo {
                                name: name_arr.value(row_idx).to_string(),
                                kind,
                                icon: Some(if table_type == "VIEW" {
                                    "view".to_string()
                                } else {
                                    "table".to_string()
                                }),
                                comment: None,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            })
            .collect();
        Ok(nodes)
    }

    async fn get_table_detail(
        &self,
        catalog: &str,
        _schema: &str,
        table: &str,
    ) -> Result<crate::driver::NodeDetail, CoreError> {
        let sql = "\
            SELECT column_name, data_type, is_nullable, column_key, column_default, column_comment \
             FROM information_schema.columns \
             WHERE table_schema = ? AND table_name = ? \
             ORDER BY ordinal_position";
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(catalog.to_string()),
                    Value::Text(table.to_string()),
                ],
            )
            .await?;
        let columns: Vec<crate::driver::ColumnDetail> = (0..result.total_rows())
            .filter_map(|row_idx| {
                result.batches.iter().find_map(|batch| {
                    if row_idx < batch.num_rows() {
                        let col_name = batch
                            .column(0)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx);
                        let data_type = batch
                            .column(1)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx);
                        let nullable = batch
                            .column(2)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx)
                            == "YES";
                        let pk = batch
                            .column(3)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx);
                        let default = batch
                            .column(4)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx);
                        let comment = batch
                            .column(5)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx);
                        Some(crate::driver::ColumnDetail {
                            name: col_name.to_string(),
                            data_type: data_type.to_string(),
                            nullable,
                            is_primary_key: pk == "PRI",
                            is_foreign_key: pk == "MUL",
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
                        })
                    } else {
                        None
                    }
                })
            })
            .collect();

        Ok(crate::driver::NodeDetail {
            node: crate::driver::NodeInfo {
                name: table.to_string(),
                kind: crate::driver::SchemaObjectKind::Table,
                icon: Some("table".to_string()),
                comment: None,
            },
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
    ) -> Result<Vec<crate::driver::IndexDetail>, CoreError> {
        self.list_indexes(catalog, Some(schema), table).await
    }

    async fn get_constraints(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<crate::driver::ConstraintDetail>, CoreError> {
        self.list_constraints(catalog, Some(schema), table).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_text_and_binary_classification() {
        // 文本族声明名（大小写无关）。
        for n in ["VARCHAR", "char", "TEXT", "LONGTEXT", "ENUM", "SET", "JSON"] {
            assert!(is_text_type_name(n), "{n} 应视为文本");
        }
        for n in ["BIGINT", "BLOB", "BINARY", "DOUBLE", "DATETIME"] {
            assert!(!is_text_type_name(n), "{n} 不应视为文本");
        }
        // MySQL 把 TEXT 与 BLOB 都报成 `BLOB`：靠字节可否解码为 UTF-8 区分。
        assert!(bytes_are_text(b"BASE TABLE"));
        assert!(!bytes_are_text(&[0xff, 0xfe, 0x00]));
    }
    use crate::driver::Database;

    const MYSQL_URL: &str = "mysql://root:root@localhost:3306/";

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_connect() {
        let db = MySqlDatabase::new(MYSQL_URL).await;
        assert!(db.is_ok(), "Failed to connect to MySQL: {:?}", db.err());
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_query_select_one() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT 1 AS val").await?;
        assert_eq!(result.columns, vec!["val"]);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_crud_roundtrip() -> Result<(), CoreError> {
        let setup_db = MySqlDatabase::new(MYSQL_URL).await?;
        setup_db
            .query("CREATE DATABASE IF NOT EXISTS _rd_test_db")
            .await?;

        let db_url = format!("{}_rd_test_db", MYSQL_URL);
        let db = MySqlDatabase::new(&db_url).await?;

        db.query("CREATE TABLE IF NOT EXISTS _rd_test (id INT PRIMARY KEY, name VARCHAR(100), value DOUBLE)")
            .await?;

        db.query("INSERT INTO _rd_test (id, name, value) VALUES (1, 'hello', 3.14)")
            .await?;

        let result = db
            .query("SELECT id, name, value FROM _rd_test WHERE id = 1")
            .await?;
        assert_eq!(result.columns, vec!["id", "name", "value"]);

        db.query("DROP TABLE IF EXISTS _rd_test").await?;
        setup_db
            .query("DROP DATABASE IF EXISTS _rd_test_db")
            .await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_error_handling() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT * FROM _non_existent_table_rd").await;
        assert!(result.is_err(), "应返回不存在的表错误");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_list_tables() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let tables = db.list_tables("mysql", None).await;
        assert!(tables.is_ok(), "list_tables 失败: {:?}", tables.err());
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_meta() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let meta = db.meta();
        assert!(meta.supports_transaction);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_is_read_only_flag() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT 1").await?;
        assert_eq!(result.is_read_only, Some(true));
        Ok(())
    }
}
