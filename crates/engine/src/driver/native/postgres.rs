use arrow::array::{
    Array, ArrayRef, BinaryArray, BooleanArray, Float32Array, Float64Array, Int32Array, Int64Array,
    StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use sqlx::{Column, Pool, Postgres, Row, TypeInfo as _};
use std::sync::Arc;

use crate::driver::traits::MetadataBrowser;
use crate::driver::utils::{
    affected_rows_result, byte_offset_for_char, returns_rows, sqlx_value_is_null,
};
use crate::driver::{
    ColumnDetail, DataSourceMeta, Database, PoolStatus, SchemaObjectKind, Transaction,
};
use shared::error::{ConnectionError, CoreError, DatabaseError};
use shared::models::{ArrowBatch, QueryResult, Value};

/// PostgreSQL 数据库连接
///
/// 封装 `sqlx::Pool<Postgres>` 连接池，通过 `Database` trait 提供统一的查询/执行接口。
/// 支持 schema 层级浏览，通过 `MetadataBrowser` trait 查询 `information_schema` 和 `pg_catalog`。
///
/// # 字段
/// * `pool` - sqlx PostgreSQL 连接池
/// * `server_version` - PostgreSQL 服务器版本号，首次连接时获取并缓存
/// * `max_connections` / `min_connections` - 连接池上下限配置
pub struct PostgresDatabase {
    pool: Pool<Postgres>,
    server_version: Option<String>,
    max_connections: usize,
    min_connections: usize,
}

impl PostgresDatabase {
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        let pool = Pool::connect(url).await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "postgres".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        let server_version = sqlx::query_scalar::<_, String>("SELECT version()")
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

    pub fn from_pool(pool: Pool<Postgres>) -> Self {
        Self {
            pool,
            server_version: None,
            max_connections: 10,
            min_connections: 0,
        }
    }

    pub fn from_pool_with_version(pool: Pool<Postgres>, server_version: Option<String>) -> Self {
        Self {
            pool,
            server_version,
            max_connections: 10,
            min_connections: 0,
        }
    }

    pub fn from_pool_with_config(
        pool: Pool<Postgres>,
        server_version: Option<String>,
        max_connections: usize,
        min_connections: usize,
    ) -> Self {
        Self {
            pool,
            server_version,
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

fn build_query_result(
    columns: &[String],
    rows: &[sqlx::postgres::PgRow],
    _is_read_only: bool,
) -> Result<QueryResult, CoreError> {
    let batch = postgres_rows_to_arrow(columns, rows)?;
    Ok(QueryResult {
        columns: columns.to_vec(),
        batches: vec![batch],
        ..Default::default()
    })
}

#[async_trait::async_trait]
impl Database for PostgresDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let read_only = is_read_only_sql(sql);
        // B5 / P0.6：不返回结果集的写语句走 `execute`，拿驱动的**真实影响行数**
        if !read_only && !returns_rows(sql) {
            return execute_writing(&self.pool, sql).await;
        }
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&self.pool)
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

        build_query_result(&columns, &rows, read_only)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<shared::models::Value>,
    ) -> Result<QueryResult, CoreError> {
        let read_only = is_read_only_sql(sql);

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

        build_query_result(&columns, &rows, read_only)
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
                let read_only = is_read_only_sql(&sql_owned);
                // B5 / P0.6：写语句（无结果集）走 `execute` 拿影响行数
                if !read_only && !returns_rows(&sql_owned) {
                    return execute_writing(&pool, &sql_owned).await;
                }

                let rows = sqlx::query(sqlx::AssertSqlSafe(sql_owned.as_str()))
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| query_error(&sql_owned, e))?;

                if rows.is_empty() {
                    return Ok(QueryResult {
                        columns: vec![],
                        batches: vec![],
                        ..Default::default()
                    });
                }

                let columns: Vec<String> = rows[0].columns()
                    .iter()
                    .map(|c| c.name().to_string())
                    .collect();

                build_query_result(&columns, &rows, read_only)
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
                db_type: "postgres".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;
        Ok(Box::new(PostgresTransaction::new(tx)))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::postgres()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                CoreError::connection(ConnectionError::Other {
                    conn_id: "postgres".to_string(),
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

    async fn list_schemas(&self, catalog: &str) -> Result<Vec<String>, CoreError> {
        self.get_schemas(catalog)
            .await
            .map(|nodes| nodes.into_iter().map(|n| n.name).collect())
    }

    async fn list_tables(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        self.get_tables(_catalog, schema_name).await
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
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT p.proname FROM pg_catalog.pg_proc p \
                   JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
                   WHERE n.nspname = $1 AND p.prokind = 'p' \
                   ORDER BY p.proname";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
         Ok(names_to_nodes(
            &result,
            SchemaObjectKind::Procedure,
        ))
    }

    async fn list_functions(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT p.proname FROM pg_catalog.pg_proc p \
                   JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
                   WHERE n.nspname = $1 AND p.prokind = 'f' \
                   ORDER BY p.proname";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(names_to_nodes(&result, SchemaObjectKind::Function))
    }

    async fn list_sequences(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        self.get_sequences(catalog, schema.unwrap_or("public")).await
    }

    async fn list_triggers(
        &self,
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        self.get_triggers(catalog, schema.unwrap_or("public")).await
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

    fn as_metadata_browser(&self) -> Option<&dyn crate::driver::MetadataBrowser> {
        Some(self)
    }
}

/// PostgreSQL 事务句柄
///
/// 封装 `sqlx::Transaction`，支持 begin/commit/rollback。
/// Drop 时若未提交且未回滚则自动回滚，避免悬挂事务。
pub struct PostgresTransaction {
    tx: Option<sqlx::Transaction<'static, Postgres>>,
}

impl PostgresTransaction {
    fn new(tx: sqlx::Transaction<'static, Postgres>) -> Self {
        Self { tx: Some(tx) }
    }
}

#[async_trait::async_trait]
impl Transaction for PostgresTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        if let Some(ref mut tx) = self.tx {
            let read_only = is_read_only_sql(sql);
            // B5 / P0.6：事务内的写语句同样要真实影响行数
            if !read_only && !returns_rows(sql) {
                let result = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| query_error(sql, e))?;
                return Ok(affected_rows_result(result.rows_affected()));
            }

            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut **tx)
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

            build_query_result(&columns, &rows, read_only)
        } else {
            Err(CoreError::database(DatabaseError::Driver {
                db_type: "postgres".to_string(),
                operation: "query".to_string(),
                source: "Transaction already closed".to_string(),
            }))
        }
    }

    async fn commit(&mut self) -> Result<(), CoreError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "postgres".to_string(),
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
                tracing::warn!("PostgreSQL transaction rollback error: {}", e);
            }
        }
        Ok(())
    }
}

/// 写语句（不返回结果集）走 `execute`：拿驱动的**真实影响行数**（B5 / P0.6）
async fn execute_writing(pool: &Pool<Postgres>, sql: &str) -> Result<QueryResult, CoreError> {
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .map_err(|e| query_error(sql, e))?;
    Ok(affected_rows_result(result.rows_affected()))
}

/// sqlx 错误 → 引擎错误（**带上数据库报的真实位置**，B6 定位要用）
///
/// sqlx 的 `Display` 只给消息，还会把 PG 的**源码行号**写成 “at line N” 附在末尾——
/// 那不是 SQL 里的位置。真位置在结构化字段里：`PgErrorPosition::Original` 是
/// **1 基的字符位置**（PG 协议就这么定义），这里换算成 SQL 的**字节偏移**；
/// `Internal` 说的是服务器自己生成的语句，与我们的 SQL 无关，不能用。
///
/// 拿不到位置就退回原来的消息（不假装知道位置）。
fn query_error(sql: &str, error: sqlx::Error) -> CoreError {
    let mut mapped = DatabaseError::query(sql, error.to_string());
    if let Some(db) = error.as_database_error()
        && let Some(pg) = db.try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
        && let Some(sqlx::postgres::PgErrorPosition::Original(position)) = pg.position()
        && let Some(offset) = byte_offset_for_char(sql, position)
    {
        mapped = mapped.with_position(offset);
    }
    CoreError::database(mapped)
}

fn postgres_rows_to_arrow(
    columns: &[String],
    rows: &[sqlx::postgres::PgRow],
) -> Result<ArrowBatch, CoreError> {
    let num_rows = rows.len();
    let num_cols = columns.len();

    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(num_cols);

    for col_idx in 0..num_cols {
        let mut string_values: Vec<Option<String>> = Vec::with_capacity(num_rows);
        let mut int64_values: Vec<Option<i64>> = Vec::with_capacity(num_rows);
        let mut int32_values: Vec<Option<i32>> = Vec::with_capacity(num_rows);
        let mut float64_values: Vec<Option<f64>> = Vec::with_capacity(num_rows);
        let mut float32_values: Vec<Option<f32>> = Vec::with_capacity(num_rows);
        let mut bool_values: Vec<Option<bool>> = Vec::with_capacity(num_rows);
        let mut binary_values: Vec<Option<Vec<u8>>> = Vec::with_capacity(num_rows);

        // 遍历所有行确定最宽类型（0=Null, 1=Bool, 2=Int32, 3=Int64, 4=Float32, 5=Float64, 6=Binary, 7=Utf8）
        let mut detected_rank: u8 = 0;
        // 非空却解不出来的单元格数（列级聚合，填值循环后面留痕）
        let mut undecodable = 0usize;

        for row in rows {
            use sqlx::Row;

            let row_rank = if let Ok(Some(_)) = row.try_get::<Option<bool>, _>(col_idx) {
                1 // Boolean
            } else if let Ok(Some(_)) = row.try_get::<Option<i32>, _>(col_idx) {
                2 // Int32
            } else if let Ok(Some(_)) = row.try_get::<Option<i16>, _>(col_idx) {
                // PG `int2`：sqlx 的类型检查不放宽（i32 解不了 int2），探测表漏了它就会
                // 整列落进文本探测再失败 —— 静默 NULL。归 Int32 档，填值时加宽。
                2 // Int16 → 加宽进 Int32
            } else if let Ok(Some(_)) = row.try_get::<Option<i64>, _>(col_idx) {
                3 // Int64
            } else if let Ok(Some(_)) = row.try_get::<Option<f32>, _>(col_idx) {
                4 // Float32
            } else if let Ok(Some(_)) = row.try_get::<Option<f64>, _>(col_idx) {
                5 // Float64
            } else if let Ok(Some(_)) = row.try_get::<Option<Vec<u8>>, _>(col_idx) {
                6 // Binary
            } else if let Ok(Some(_)) = row.try_get::<Option<String>, _>(col_idx) {
                7 // Utf8
            } else {
                0 // NULL — 不影响类型推断
            };
            if row_rank > detected_rank {
                detected_rank = row_rank;
            }
            if detected_rank == 7 {
                break; // Utf8 为最宽类型，无需继续
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

        for row in rows {
            use sqlx::Row;

            match effective_type {
                DataType::Boolean => {
                    bool_values.push(row.try_get::<Option<bool>, _>(col_idx).ok().flatten());
                }
                DataType::Int32 => {
                    // `int2` 只能按 `i16` 解（sqlx 不放宽），加宽进 Int32 列
                    let value = row
                        .try_get::<Option<i32>, _>(col_idx)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            row.try_get::<Option<i16>, _>(col_idx)
                                .ok()
                                .flatten()
                                .map(i32::from)
                        });
                    int32_values.push(value);
                }
                DataType::Int64 => {
                    int64_values.push(row.try_get::<Option<i64>, _>(col_idx).ok().flatten());
                }
                DataType::Float32 => {
                    float32_values.push(row.try_get::<Option<f32>, _>(col_idx).ok().flatten());
                }
                DataType::Float64 => {
                    float64_values.push(row.try_get::<Option<f64>, _>(col_idx).ok().flatten());
                }
                DataType::Binary => {
                    binary_values.push(row.try_get::<Option<Vec<u8>>, _>(col_idx).ok().flatten());
                }
                _ => {
                    // 文本兜底：真文本列先命中；`numeric` / 时间 / UUID / JSON 由这条链解成
                    // **文本**（精度保住，见 `utils.rs::sqlx_cell_as_text`）。
                    // 2026-09-21 之前这里只试 `String`，于是这些列全部静默变 NULL。
                    let decoded = crate::sqlx_cell_as_text!(row, col_idx);
                    if decoded.is_none() && !sqlx_value_is_null(row, col_idx) {
                        undecodable += 1;
                    }
                    string_values.push(decoded);
                }
            }
        }

        // 解不出来 ≠ 值为 NULL：非空单元格必须留痕，否则用户看到的是一个没有来源的空白格。
        // 聚合到列级：一条 SQL 每个受影响列只报一行，不按行刷屏。
        if undecodable > 0 {
            let declared = rows
                .first()
                .map(|r| r.column(col_idx).type_info().name().to_string())
                .unwrap_or_default();
            tracing::warn!(
                driver = "postgres",
                column = %columns[col_idx],
                declared_type = %declared,
                undecodable_rows = undecodable,
                total_rows = num_rows,
                "列里有非空单元格无法解码，已按 NULL 交出（出口层目前不区分「解不出来」与「本来就是 NULL」）"
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
            db_type: "postgres".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[async_trait::async_trait]
impl crate::driver::MetadataBrowser for PostgresDatabase {
    async fn get_catalogs(&self) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        // PostgreSQL 一条连接只绑定一个数据库：`information_schema` 仅暴露**当前库**的
        // schema / 表，列出其它库只会得到无法展开的假节点。因此只返回当前库；
        // 跨库浏览（DBeaver 式：展开时另开一条连接）留待后续。
        let result = self
            .query("SELECT current_database()::text AS datname")
            .await?;
        Ok(rows_to_node_info(
            &result,
            crate::driver::SchemaObjectKind::Catalog,
        ))
    }

    async fn get_schemas(&self, catalog: &str) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let sql = "SELECT schema_name FROM information_schema.schemata \
                   WHERE catalog_name = $1 AND schema_name NOT IN ('pg_catalog', 'information_schema') \
                   ORDER BY schema_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(rows_to_node_info(
            &result,
            crate::driver::SchemaObjectKind::Schema,
        ))
    }

    async fn get_tables(
        &self,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
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
        let mut nodes: Vec<crate::driver::NodeInfo> = Vec::new();
        for row_idx in 0..result.total_rows() {
            if let Some(batch) = result.batches.first() {
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
                        nodes.push(crate::driver::NodeInfo::new(
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
    ) -> Result<crate::driver::NodeDetail, CoreError> {
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
        let mut columns: Vec<crate::driver::ColumnDetail> = Vec::new();
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
                    columns.push(crate::driver::ColumnDetail {
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

        Ok(crate::driver::NodeDetail {
            node: crate::driver::NodeInfo::new(table, crate::driver::SchemaObjectKind::Table),
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

    /// 序列与触发器在 PostgreSQL 走**浏览器层**（而不是 `Database::list_*` 回退）：
    /// 回退路径要先让 `get_sequences` 返回空才算数，而 trait 默认实现就是空——
    /// 那是“未支持”与“真的没有”分辨不清的写法。这里直接给真实实现。
    async fn get_sequences(
        &self,
        _catalog: &str,
        schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let sql = "SELECT sequence_name FROM information_schema.sequences \
                   WHERE sequence_schema = $1 ORDER BY sequence_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema.to_string())])
            .await?;
        Ok(names_to_nodes(
            &result,
            crate::driver::SchemaObjectKind::Sequence,
        ))
    }

    async fn get_triggers(
        &self,
        _catalog: &str,
        schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
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

fn rows_to_node_info(
    result: &QueryResult,
    kind: crate::driver::SchemaObjectKind,
) -> Vec<crate::driver::NodeInfo> {
    let mut nodes: Vec<crate::driver::NodeInfo> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        nodes.push(crate::driver::NodeInfo::new(name.to_string(), kind.clone()));
                    }
                }
            }
        }
    }
    nodes
}

/// 单列名字结果集 → 对象列表（例程 / 序列）。
fn names_to_nodes(
    result: &QueryResult,
    kind: crate::driver::SchemaObjectKind,
) -> Vec<crate::driver::NodeInfo> {
    rows_to_node_info(result, kind)
}

/// 触发器结果集 → 对象列表（第 2 列是所属表，进 `parent_name`）。
fn names_to_trigger_nodes(result: &QueryResult) -> Vec<crate::driver::NodeInfo> {
    let mut nodes: Vec<crate::driver::NodeInfo> = Vec::new();
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
        let mut node =
            crate::driver::NodeInfo::new(name.to_string(), crate::driver::SchemaObjectKind::Trigger);
        if let Some(table_arr) = batch.column(1).as_any().downcast_ref::<StringArray>() {
            if !table_arr.is_null(row_idx) {
                node = node.with_parent(table_arr.value(row_idx));
            }
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Database;

    const PG_URL: &str = "postgresql://postgres:postgresql@localhost:5432/business_db";

    /// 触发器结果集 → 对象列表：第 2 列（所属表）进 `parent_name`，NULL 时不填。
    ///
    /// 纯函数，不需要真机——而它是「属性面板能看到关联表」的起点：
    /// 2026-09-19 之前这一列查了却被上层丢掉，所以这里把它钉住。
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
        let db = PostgresDatabase::new(PG_URL).await;
        assert!(db.is_ok(), "连接 PostgreSQL 失败: {:?}", db.err());
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_query_select_one() -> Result<(), CoreError> {
        let db = PostgresDatabase::new(PG_URL).await?;
        let result = db.query("SELECT 1 AS val").await?;
        assert_eq!(result.columns, vec!["val"]);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_crud_roundtrip() -> Result<(), CoreError> {
        let db = PostgresDatabase::new(PG_URL).await?;

        db.query("CREATE TABLE IF NOT EXISTS _rd_test (id INTEGER PRIMARY KEY, name VARCHAR(100), value DOUBLE PRECISION)")
            .await?;

        db.query("INSERT INTO _rd_test (id, name, value) VALUES (1, 'hello', 3.14)")
            .await?;

        let result = db
            .query("SELECT id, name, value FROM _rd_test WHERE id = 1")
            .await?;
        assert_eq!(result.columns, vec!["id", "name", "value"]);

        db.query("DROP TABLE IF EXISTS _rd_test").await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_error_handling() -> Result<(), CoreError> {
        let db = PostgresDatabase::new(PG_URL).await?;
        let result = db.query("SELECT * FROM _non_existent_table_rd").await;
        assert!(result.is_err(), "应返回不存在的表错误");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_list_tables() -> Result<(), CoreError> {
        let db = PostgresDatabase::new(PG_URL).await?;
        let tables = db.list_tables("public", Some("public")).await;
        assert!(tables.is_ok(), "list_tables 失败: {:?}", tables.err());
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_meta() -> Result<(), CoreError> {
        let db = PostgresDatabase::new(PG_URL).await?;
        let meta = db.meta();
        assert!(meta.supports_transaction);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 PostgreSQL 服务"]
    async fn test_is_read_only_flag() -> Result<(), CoreError> {
        let db = PostgresDatabase::new(PG_URL).await?;
        let result = db.query("SELECT 1").await?;
        assert_eq!(result.is_read_only, Some(true));
        Ok(())
    }
}
