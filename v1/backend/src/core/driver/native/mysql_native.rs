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

use crate::core::driver::traits::MetadataBrowser;
use crate::core::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, IndexDetail, NodeDetail, NodeInfo,
    PoolStatus, SchemaObject, SchemaObjectKind, Transaction,
};
use crate::core::error::{ConnectionError, CoreError, DatabaseError};
use crate::core::models::{ArrowBatch, QueryResult, Value};

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
        let pool = mysql_async::Pool::new(url);
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

        let mut result = conn
            .query_iter(sql)
            .await
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let columns: Vec<String> = result
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();

        let rows: Vec<mysql_async::Row> = result
            .collect()
            .await
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

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

        let params_ref: Vec<&mysql_async::Value> = mysql_params.iter().collect();

        let mut result = conn
            .exec_iter(sql, params_ref)
            .await
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let columns: Vec<String> = result
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();

        let rows: Vec<mysql_async::Row> = result
            .collect()
            .await
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

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

                let mut query_result = conn.query_iter(&sql_owned).await.map_err(|e| {
                    CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                })?;

                let columns: Vec<String> = query_result
                    .columns_ref()
                    .iter()
                    .map(|c| c.name_str().to_string())
                    .collect();

                let rows: Vec<mysql_async::Row> = query_result.collect().await.map_err(|e| {
                    CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                })?;

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
            ..DataSourceMeta::mysql()
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
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let nodes = self.get_tables(catalog, catalog).await?;
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
        let sql = format!("SHOW CREATE {} `{}`.`{}`", stmt_type, esc_catalog, esc_name);
        let result = self.query(&sql).await?;
        if let Some(batch) = result.batches.first() {
            if batch.num_rows() > 0 {
                if let Some(col) = batch
                    .column(1)
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

            let mut result = conn
                .query_iter(sql)
                .await
                .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

            let columns: Vec<String> = result
                .columns_ref()
                .iter()
                .map(|c| c.name_str().to_string())
                .collect();

            let rows: Vec<mysql_async::Row> = result
                .collect()
                .await
                .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

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
    async fn get_catalogs(&self) -> Result<Vec<NodeInfo>, CoreError> {
        let result = self
            .query("SELECT schema_name FROM information_schema.schemata ORDER BY schema_name")
            .await?;
        Ok(rows_to_node_info(
            &result,
            SchemaObjectKind::Catalog,
            "database",
        ))
    }

    async fn get_schemas(&self, _catalog: &str) -> Result<Vec<NodeInfo>, CoreError> {
        self.get_catalogs().await
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
        _schema: &str,
        table: &str,
    ) -> Result<NodeDetail, CoreError> {
        use arrow::array::StringArray;
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

    const MYSQL_URL: &str = "mysql://root:root@localhost:3306/";

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
