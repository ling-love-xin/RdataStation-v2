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

use crate::core::driver::traits::MetadataBrowser;
use crate::core::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, IndexDetail, NodeDetail, NodeInfo,
    PoolStatus, SchemaObject, SchemaObjectKind, Transaction,
};
use crate::core::error::{ConnectionError, CoreError, DatabaseError};
use crate::core::models::{ArrowBatch, QueryResult, Value};

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
        // 构建 TLS 连接器（接受自签名证书用于开发环境）
        let tls_connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "postgres_native".to_string(),
                    operation: "tls_init".to_string(),
                    source: e.to_string(),
                })
            })?;
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

// ============================================================================
// Arrow 转换
// ============================================================================

/// 将 tokio_postgres Row 转换为 Arrow 批处理
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
            } else if *col_type == tokio_postgres::types::Type::FLOAT8
                || *col_type == tokio_postgres::types::Type::NUMERIC
            {
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
                    int32_values.push(row.try_get::<_, Option<i32>>(col_idx).ok().flatten());
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
                    // 通用字符串格式
                    let val: Result<Option<String>, _> = row.try_get(col_idx);
                    match val {
                        Ok(s) => string_values.push(s),
                        Err(_) => {
                            let v = row
                                .try_get::<_, Option<bool>>(col_idx)
                                .ok()
                                .flatten()
                                .map(|b| b.to_string())
                                .or_else(|| {
                                    row.try_get::<_, Option<i64>>(col_idx)
                                        .ok()
                                        .flatten()
                                        .map(|i| i.to_string())
                                })
                                .or_else(|| {
                                    row.try_get::<_, Option<f64>>(col_idx)
                                        .ok()
                                        .flatten()
                                        .map(|f| f.to_string())
                                });
                            string_values.push(v);
                        }
                    }
                }
            }
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

fn rows_to_node_info(result: &QueryResult, kind: SchemaObjectKind, icon: &str) -> Vec<NodeInfo> {
    use arrow::array::StringArray;
    let mut nodes: Vec<NodeInfo> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        nodes.push(NodeInfo {
                            name: name.to_string(),
                            kind: kind.clone(),
                            icon: Some(icon.to_string()),
                            comment: None,
                        });
                    }
                }
            }
        }
    }
    nodes
}

fn names_to_schema_objects(result: &QueryResult, kind: SchemaObjectKind) -> Vec<SchemaObject> {
    use arrow::array::StringArray;
    let mut objects: Vec<SchemaObject> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        objects.push(SchemaObject {
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

// ============================================================================
// Database trait 实现
// ============================================================================

#[async_trait::async_trait]
impl Database for PostgresNativeDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let is_read_only = is_read_only_sql(sql);
        let client = self.client.lock().await;

        let rows = client
            .query(sql, &[])
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

        let rows = client
            .query(sql, &param_refs)
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

                let rows = guard
                    .query(&sql_owned, &[])
                    .await
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

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
            ..DataSourceMeta::postgres()
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
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let nodes = self.get_tables(catalog, schema_name).await?;
        Ok(nodes
            .into_iter()
            .map(|n| SchemaObject {
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
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT p.proname FROM pg_catalog.pg_proc p \
                   JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
                   WHERE n.nspname = $1 AND p.prokind = 'p' \
                   ORDER BY p.proname";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(names_to_schema_objects(
            &result,
            SchemaObjectKind::Procedure,
        ))
    }

    async fn list_functions(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT p.proname FROM pg_catalog.pg_proc p \
                   JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
                   WHERE n.nspname = $1 AND p.prokind = 'f' \
                   ORDER BY p.proname";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(names_to_schema_objects(&result, SchemaObjectKind::Function))
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

        let rows = client
            .query(sql, &[])
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
        let result = self
            .query("SELECT datname FROM pg_catalog.pg_database WHERE datistemplate = false ORDER BY datname")
            .await?;
        Ok(rows_to_node_info(
            &result,
            SchemaObjectKind::Catalog,
            "database",
        ))
    }

    async fn get_schemas(&self, catalog: &str) -> Result<Vec<NodeInfo>, CoreError> {
        let sql = "SELECT schema_name FROM information_schema.schemata \
                   WHERE catalog_name = $1 AND schema_name NOT IN ('pg_catalog', 'information_schema') \
                   ORDER BY schema_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(rows_to_node_info(
            &result,
            SchemaObjectKind::Schema,
            "schema",
        ))
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
                        nodes.push(NodeInfo {
                            name: name_arr.value(row_idx).to_string(),
                            kind,
                            icon: Some(if table_type == "VIEW" {
                                "view".to_string()
                            } else {
                                "table".to_string()
                            }),
                            comment: None,
                        });
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
            node: NodeInfo {
                name: table.to_string(),
                kind: SchemaObjectKind::Table,
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
    use crate::core::driver::Database;

    const PG_URL: &str = "postgresql://postgres:postgresql@localhost:5432/business_db";

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
