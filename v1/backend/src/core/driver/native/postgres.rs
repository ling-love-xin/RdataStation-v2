use arrow::array::{
    Array, ArrayRef, BinaryArray, BooleanArray, Float32Array, Float64Array, Int32Array, Int64Array,
    StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use sqlx::{Column, Pool, Postgres, Row};
use std::sync::Arc;

use crate::core::driver::traits::MetadataBrowser;
use crate::core::driver::{
    ColumnDetail, DataSourceMeta, Database, PoolStatus, SchemaObject, SchemaObjectKind, Transaction,
};
use crate::core::error::{ConnectionError, CoreError, DatabaseError};
use crate::core::models::{ArrowBatch, QueryResult, Value};

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
        let rows = sqlx::query(sql)
            .fetch_all(&self.pool)
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

        build_query_result(&columns, &rows, read_only)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<crate::core::models::Value>,
    ) -> Result<QueryResult, CoreError> {
        let read_only = is_read_only_sql(sql);

        let mut query_builder = sqlx::query(sql);

        for param in &params {
            query_builder = match param {
                crate::core::models::Value::Null => query_builder.bind(None::<String>),
                crate::core::models::Value::Bool(v) => query_builder.bind(*v),
                crate::core::models::Value::Int(v) => query_builder.bind(*v),
                crate::core::models::Value::Float(v) => query_builder.bind(*v),
                crate::core::models::Value::Text(v) => query_builder.bind(v),
                crate::core::models::Value::Bytes(v) => query_builder.bind(v.clone()),
            };
        }

        let rows = query_builder
            .fetch_all(&self.pool)
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

                let rows = sqlx::query(&sql_owned)
                    .fetch_all(&pool)
                    .await
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

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
        catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let nodes = self.get_tables(catalog, schema_name).await?;
        Ok(nodes
            .into_iter()
            .map(|n| crate::core::driver::SchemaObject {
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

    async fn list_sequences(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT sequence_name FROM information_schema.sequences \
                   WHERE sequence_schema = $1 ORDER BY sequence_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(names_to_schema_objects(&result, SchemaObjectKind::Sequence))
    }

    async fn list_triggers(
        &self,
        _catalog: &str,
        schema: Option<&str>,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let schema_name = schema.unwrap_or("public");
        let sql = "SELECT trigger_name, event_object_table, event_manipulation \
                   FROM information_schema.triggers \
                   WHERE trigger_schema = $1 ORDER BY trigger_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(schema_name.to_string())])
            .await?;
        Ok(names_to_schema_objects(&result, SchemaObjectKind::Trigger))
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

    fn as_metadata_browser(&self) -> Option<&dyn crate::core::driver::MetadataBrowser> {
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

            let rows = sqlx::query(sql)
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

        for row in rows {
            use sqlx::Row;

            let row_rank = if let Ok(Some(_)) = row.try_get::<Option<bool>, _>(col_idx) {
                1 // Boolean
            } else if let Ok(Some(_)) = row.try_get::<Option<i32>, _>(col_idx) {
                2 // Int32
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
                    int32_values.push(row.try_get::<Option<i32>, _>(col_idx).ok().flatten());
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
                    string_values.push(row.try_get::<Option<String>, _>(col_idx).ok().flatten());
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
            db_type: "postgres".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[async_trait::async_trait]
impl crate::core::driver::MetadataBrowser for PostgresDatabase {
    async fn get_catalogs(&self) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
        let result = self.query("SELECT datname FROM pg_catalog.pg_database WHERE datistemplate = false ORDER BY datname").await?;
        Ok(rows_to_node_info(
            &result,
            crate::core::driver::SchemaObjectKind::Catalog,
            "database",
        ))
    }

    async fn get_schemas(
        &self,
        catalog: &str,
    ) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
        let sql = "SELECT schema_name FROM information_schema.schemata \
                   WHERE catalog_name = $1 AND schema_name NOT IN ('pg_catalog', 'information_schema') \
                   ORDER BY schema_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(rows_to_node_info(
            &result,
            crate::core::driver::SchemaObjectKind::Schema,
            "schema",
        ))
    }

    async fn get_tables(
        &self,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
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
        let mut nodes: Vec<crate::core::driver::NodeInfo> = Vec::new();
        for row_idx in 0..result.total_rows() {
            if let Some(batch) = result.batches.first() {
                if row_idx < batch.num_rows() {
                    if let (Some(name_arr), Some(type_arr)) = (
                        batch.column(0).as_any().downcast_ref::<StringArray>(),
                        batch.column(1).as_any().downcast_ref::<StringArray>(),
                    ) {
                        let table_type = type_arr.value(row_idx);
                        let kind = if table_type == "VIEW" {
                            crate::core::driver::SchemaObjectKind::View
                        } else {
                            crate::core::driver::SchemaObjectKind::Table
                        };
                        nodes.push(crate::core::driver::NodeInfo {
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
    ) -> Result<crate::core::driver::NodeDetail, CoreError> {
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
        let mut columns: Vec<crate::core::driver::ColumnDetail> = Vec::new();
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
                    columns.push(crate::core::driver::ColumnDetail {
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

        Ok(crate::core::driver::NodeDetail {
            node: crate::core::driver::NodeInfo {
                name: table.to_string(),
                kind: crate::core::driver::SchemaObjectKind::Table,
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
    ) -> Result<Vec<crate::core::driver::IndexDetail>, CoreError> {
        self.list_indexes(catalog, Some(schema), table).await
    }

    async fn get_constraints(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<crate::core::driver::ConstraintDetail>, CoreError> {
        self.list_constraints(catalog, Some(schema), table).await
    }
}

fn rows_to_node_info(
    result: &QueryResult,
    kind: crate::core::driver::SchemaObjectKind,
    icon: &str,
) -> Vec<crate::core::driver::NodeInfo> {
    let mut nodes: Vec<crate::core::driver::NodeInfo> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        nodes.push(crate::core::driver::NodeInfo {
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

fn names_to_schema_objects(
    result: &QueryResult,
    kind: crate::core::driver::SchemaObjectKind,
) -> Vec<crate::core::driver::SchemaObject> {
    let mut objects: Vec<crate::core::driver::SchemaObject> = Vec::new();
    let is_trigger = kind == crate::core::driver::SchemaObjectKind::Trigger;
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        let (table_name, event) = if is_trigger {
                            let tn = batch
                                .column(1)
                                .as_any()
                                .downcast_ref::<StringArray>()
                                .and_then(|a| {
                                    if a.is_null(row_idx) {
                                        None
                                    } else {
                                        Some(a.value(row_idx).to_string())
                                    }
                                });
                            let ev = batch
                                .column(2)
                                .as_any()
                                .downcast_ref::<StringArray>()
                                .and_then(|a| {
                                    if a.is_null(row_idx) {
                                        None
                                    } else {
                                        Some(a.value(row_idx).to_string())
                                    }
                                });
                            (tn, ev)
                        } else {
                            (None, None)
                        };
                        objects.push(crate::core::driver::SchemaObject {
                            name: name.to_string(),
                            kind: kind.clone(),
                            children: None,
                            comment: None,
                            table_name,
                            event,
                        });
                    }
                }
            }
        }
    }
    objects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::Database;

    const PG_URL: &str = "postgresql://postgres:postgresql@localhost:5432/business_db";

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
