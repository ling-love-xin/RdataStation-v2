//! DuckDB 数据库驱动实现
//!
//! 使用 `duckdb-rs`（官方 Rust 绑定）实现 `Database` trait。
//! DuckDB 是嵌入式分析型数据库，专为 OLAP 场景优化。
//!
//! ## 关键约束
//! - DuckDB 不支持 schema 层级 — list_schemas 返回空 vec
//! - Statement 必须先执行 `query([])` 才能访问 column metadata
//! - 多进程同时打开同一 .duckdb 文件可能导致锁冲突

use std::sync::Arc;
use std::sync::Mutex;

use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use duckdb::Connection;

use crate::core::driver::traits::MetadataBrowser;
use crate::core::driver::utils::escape_sql_string;
use crate::core::driver::{ColumnDetail, DataSourceMeta, Database, IndexDetail, Transaction};
use crate::core::error::{CoreError, DatabaseError};
use crate::core::models::{ArrowBatch, QueryResult, Value};

/// DuckDB 数据库连接
///
/// 封装 `duckdb::Connection`，以 `Arc<Mutex<Connection>>` 管理线程安全访问。
/// DuckDB 是嵌入式分析型数据库，专为 OLAP 场景优化，支持外部数据库注册。
///
/// # 字段
/// * `conn` - 由 Arc + Mutex 保护的 duckdb-rs 连接
/// * `server_version` - DuckDB 版本号
pub struct DuckDbDatabase {
    conn: Arc<Mutex<Connection>>,
    server_version: Option<String>,
}

impl DuckDbDatabase {
    pub fn new(url: &str) -> Result<Self, CoreError> {
        let path = if url.starts_with("duckdb://") {
            url.trim_start_matches("duckdb://")
        } else {
            url
        };

        // 确保父目录存在
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::database(DatabaseError::Driver {
                        db_type: "duckdb".to_string(),
                        operation: "create_directory".to_string(),
                        source: e.to_string(),
                    })
                })?;
            }
        }

        let conn = Connection::open(path).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        let server_version = conn
            .query_row("PRAGMA version", [], |row| row.get::<_, String>(0))
            .ok();
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            server_version,
        })
    }

    pub fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            server_version: None,
        }
    }
}

fn is_read_only_sql(sql: &str) -> bool {
    let sql_upper = sql.trim_start().to_uppercase();
    sql_upper.starts_with("SELECT")
        || sql_upper.starts_with("SHOW")
        || sql_upper.starts_with("DESCRIBE")
        || sql_upper.starts_with("EXPLAIN")
        || sql_upper.starts_with("PRAGMA")
}

#[async_trait::async_trait]
/// Database trait 实现：DuckDB
///
/// 核心查询通过 `duckdb::Connection::prepare()` 执行，
/// 然后将结果集转换为 Arrow `RecordBatch`。
impl Database for DuckDbDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let conn = Arc::clone(&self.conn);
        let sql_owned = sql.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "duckdb".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            let mut stmt = conn.prepare(&sql_owned).map_err(|e| {
                CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
            })?;

            let row_data: Vec<Vec<duckdb::types::Value>>;

            {
                let mut rows = stmt.query([]).map_err(|e| {
                    CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                })?;

                let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
                while let Some(row) = rows.next().map_err(|e| {
                    CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                })? {
                    let mut values: Vec<duckdb::types::Value> = Vec::new();
                    for i in 0.. {
                        match row.get::<usize, duckdb::types::Value>(i) {
                            Ok(v) => values.push(v),
                            Err(_) => break,
                        }
                    }
                    data.push(values);
                }
                row_data = data;
            }

            let column_count: usize = if let Some(first) = row_data.first() {
                first.len()
            } else {
                stmt.column_count()
            };

            let columns: Vec<String> = if column_count > 0 {
                (0..column_count)
                    .map(|i| {
                        stmt.column_name(i)
                            .map_or("unknown".to_string(), |v| v.to_string())
                    })
                    .collect()
            } else {
                Vec::new()
            };

            let _is_read_only = is_read_only_sql(&sql_owned);
            let row_count = row_data.len();

            let batch = if row_count > 0 {
                duckdb_rows_to_arrow(&columns, &row_data)?
            } else {
                return Ok(QueryResult {
                    columns,
                    batches: vec![],
                    ..Default::default()
                });
            };

            Ok(QueryResult {
                columns,
                batches: vec![batch],
                ..Default::default()
            })
        })
        .await
        .map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "spawn_blocking".to_string(),
                source: e.to_string(),
            })
        })?
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<QueryResult, CoreError> {
        let conn = Arc::clone(&self.conn);
        let sql_owned = sql.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "duckdb".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            let mut stmt = conn.prepare(&sql_owned).map_err(|e| {
                CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
            })?;

            let column_count = stmt.column_count();
            let columns: Vec<String> = if column_count > 0 {
                (0..column_count)
                    .map(|i| {
                        stmt.column_name(i)
                            .map(|n| n.to_string())
                            .unwrap_or_else(|_| format!("column_{}", i))
                    })
                    .collect()
            } else {
                Vec::new()
            };

            let duckdb_params: Vec<duckdb::types::Value> = params
                .iter()
                .map(|v| match v {
                    Value::Null => duckdb::types::Value::Null,
                    Value::Bool(b) => duckdb::types::Value::Boolean(*b),
                    Value::Int(i) => duckdb::types::Value::BigInt(*i),
                    Value::Float(f) => duckdb::types::Value::Double(*f),
                    Value::Text(s) => duckdb::types::Value::Text(s.clone()),
                    Value::Bytes(b) => duckdb::types::Value::Blob(b.clone()),
                })
                .collect();

            let params_slice: Vec<&dyn duckdb::ToSql> = duckdb_params
                .iter()
                .map(|v| v as &dyn duckdb::ToSql)
                .collect();

            let row_data: Vec<Vec<duckdb::types::Value>>;
            {
                let mut rows = stmt.query(params_slice.as_slice()).map_err(|e| {
                    CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                })?;

                let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
                while let Some(row) = rows.next().map_err(|e| {
                    CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                })? {
                    let mut values: Vec<duckdb::types::Value> = Vec::new();
                    let col_count = if column_count > 0 {
                        column_count
                    } else {
                        row.as_ref().column_count()
                    };
                    for i in 0..col_count {
                        match row.get::<usize, duckdb::types::Value>(i) {
                            Ok(v) => values.push(v),
                            Err(_) => values.push(duckdb::types::Value::Null),
                        }
                    }
                    data.push(values);
                }
                row_data = data;
            }

            let _is_read_only = is_read_only_sql(&sql_owned);
            let row_count = row_data.len();

            let columns = if columns.is_empty() && column_count > 0 {
                (0..column_count).map(|i| format!("column_{}", i)).collect()
            } else {
                columns
            };

            let batch = if row_count > 0 {
                duckdb_rows_to_arrow(&columns, &row_data)?
            } else {
                return Ok(QueryResult {
                    columns,
                    batches: vec![],
                    ..Default::default()
                });
            };

            Ok(QueryResult {
                columns,
                batches: vec![batch],
                ..Default::default()
            })
        })
        .await
        .map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "spawn_blocking".to_string(),
                source: e.to_string(),
            })
        })?
    }

    async fn query_with_cancel(
        &self,
        sql: &str,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<QueryResult, CoreError> {
        let conn = Arc::clone(&self.conn);
        let sql_owned = sql.to_string();
        let sql_for_error = sql.to_string();

        tokio::select! {
            result = tokio::task::spawn_blocking(move || {
                let conn = conn.lock().map_err(|e| CoreError::database(DatabaseError::Driver {
                    db_type: "duckdb".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                }))?;

                let mut stmt = conn.prepare(&sql_owned)
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

                let row_data: Vec<Vec<duckdb::types::Value>>;

                {
                    let mut rows = stmt.query([]).map_err(|e| {
                        CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
                    })?;

                    let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
                    while let Ok(Some(row)) = rows.next() {
                        let mut values: Vec<duckdb::types::Value> = Vec::new();
                        for i in 0.. {
                            match row.get::<usize, duckdb::types::Value>(i) {
                                Ok(v) => values.push(v),
                                Err(_) => break,
                            }
                        }
                        data.push(values);
                    }
                    row_data = data;
                }

                let column_count: usize = if let Some(first) = row_data.first() {
                    first.len()
                } else {
                    stmt.column_count()
                };

                let columns: Vec<String> = if column_count > 0 {
                    (0..column_count)
                        .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                        .collect()
                } else {
                    Vec::new()
                };

                let _is_read_only = is_read_only_sql(&sql_owned);
                let row_count = row_data.len();

                let batch = if row_count > 0 {
                    duckdb_rows_to_arrow(&columns, &row_data)?
                } else {
                    return Ok(QueryResult {
                        columns,
                        batches: vec![],
                        ..Default::default()
                    });
                };

                Ok(QueryResult {
                    columns,
                    batches: vec![batch],
                    ..Default::default()
                })
            }) => {
                result.map_err(|e| CoreError::database(DatabaseError::query(
                    &sql_for_error,
                    format!("Task panicked: {}", e),
                )))?
            }
            _ = cancel_token.cancelled() => {
                Err(CoreError::database(DatabaseError::Query {
                    sql: sql_for_error,
                    reason: "Query cancelled".to_string(),
                    position: None,
                }))
            }
        }
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        conn.execute("BEGIN TRANSACTION", []).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;

        Ok(Box::new(DuckDbTransaction::new(Arc::clone(&self.conn))))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::duckdb()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;
        conn.execute("SELECT 1", [])
            .map_err(|e| CoreError::database(DatabaseError::query("SELECT 1", e.to_string())))?;
        Ok(())
    }

    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        Ok(vec!["main".to_string()])
    }

    async fn list_tables(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<crate::core::driver::SchemaObject>, CoreError> {
        let nodes = self.get_tables("main", "main").await?;
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
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let detail = self.get_table_detail("main", "main", table).await?;
        Ok(detail.columns)
    }

    /// 列举表的所有索引
    ///
    /// DuckDB 通过 duckdb_indexes() 表函数获取索引信息。
    /// 索引列名从 expression 字段解析（如 "CREATE INDEX idx ON t (col1, col2)"）。
    async fn list_indexes(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        // duckdb_indexes() 返回: index_name, is_unique, is_primary, index_type, expression
        let mut stmt = conn
            .prepare(
                "SELECT index_name, is_unique, is_primary, index_type, expression
                 FROM duckdb_indexes()
                 WHERE table_name = ?",
            )
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_indexes", e.to_string()))
            })?;

        let rows = stmt
            .query_map([table], |row| {
                Ok((
                    row.get::<_, String>(0)?,         // index_name
                    row.get::<_, bool>(1)?,           // is_unique
                    row.get::<_, bool>(2)?,           // is_primary
                    row.get::<_, Option<String>>(3)?, // index_type
                    row.get::<_, Option<String>>(4)?, // expression
                ))
            })
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_indexes", e.to_string()))
            })?;

        let mut indexes = Vec::new();
        for row in rows {
            match row {
                Ok((name, is_unique, is_primary, index_type, expression)) => {
                    // 从 expression 解析列名：格式 "(col1, col2)" 或 "(col1)"
                    let column_names: Vec<String> = expression
                        .as_deref()
                        .and_then(|expr| {
                            let start = expr.find('(')?;
                            let end = expr.rfind(')')?;
                            if end > start {
                                Some(
                                    expr[start + 1..end]
                                        .split(',')
                                        .map(|s| s.trim().trim_matches('"').to_string())
                                        .collect(),
                                )
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default();

                    indexes.push(IndexDetail {
                        name,
                        table_name: table.to_string(),
                        column_names,
                        is_unique,
                        is_primary,
                        index_type,
                        comment: None,
                    });
                }
                Err(e) => {
                    tracing::warn!("DuckDB list_indexes row error: {}", e);
                }
            }
        }

        Ok(indexes)
    }

    async fn register_external_database(
        &self,
        name: &str,
        driver: &str,
        connection_string: &str,
    ) -> Result<(), CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let sql = format!(
            "ATTACH '{}' AS {} (TYPE '{}')",
            connection_string, name, driver
        );
        conn.execute(&sql, []).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "register_external_database".to_string(),
                source: e.to_string(),
            })
        })?;

        Ok(())
    }

    async fn create_external_table(
        &self,
        external_db_name: &str,
        schema_name: &str,
        table_name: &str,
        external_table_name: &str,
    ) -> Result<(), CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let sql = format!(
            "CREATE EXTERNAL TABLE {}.{} AS SELECT * FROM {}.{}",
            schema_name, table_name, external_db_name, external_table_name
        );
        conn.execute(&sql, []).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "create_external_table".to_string(),
                source: e.to_string(),
            })
        })?;

        Ok(())
    }

    fn as_metadata_browser(&self) -> Option<&dyn crate::core::driver::MetadataBrowser> {
        Some(self)
    }
}

/// DuckDB 事务句柄
///
/// 通过 `Arc<Mutex<Connection>>` 共享连接，支持 begin/commit/rollback。
/// `committed` 标记用于 Drop 时判断是否需要自动回滚。
pub struct DuckDbTransaction {
    conn: Arc<Mutex<Connection>>,
    committed: bool,
}

impl DuckDbTransaction {
    fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            conn,
            committed: false,
        }
    }
}

#[async_trait::async_trait]
impl Transaction for DuckDbTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let row_data: Vec<Vec<duckdb::types::Value>>;

        {
            let mut rows = stmt
                .query([])
                .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

            let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
            while let Ok(Some(row)) = rows.next() {
                let mut values: Vec<duckdb::types::Value> = Vec::new();
                for i in 0.. {
                    match row.get::<usize, duckdb::types::Value>(i) {
                        Ok(v) => values.push(v),
                        Err(_) => break,
                    }
                }
                data.push(values);
            }
            row_data = data;
        }

        let column_count: usize = if let Some(first) = row_data.first() {
            first.len()
        } else {
            stmt.column_count()
        };

        let columns: Vec<String> = if column_count > 0 {
            (0..column_count)
                .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                .collect()
        } else {
            Vec::new()
        };

        let sql_upper = sql.trim_start().to_uppercase();
        let _is_read_only = sql_upper.starts_with("SELECT")
            || sql_upper.starts_with("SHOW")
            || sql_upper.starts_with("DESCRIBE");
        let row_count = row_data.len();

        let batch = if row_count > 0 {
            duckdb_rows_to_arrow(&columns, &row_data)?
        } else {
            return Ok(QueryResult {
                columns,
                batches: vec![],
                ..Default::default()
            });
        };

        Ok(QueryResult {
            columns,
            batches: vec![batch],
            ..Default::default()
        })
    }

    async fn commit(&mut self) -> Result<(), CoreError> {
        if !self.committed {
            let conn = self.conn.lock().map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "duckdb".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            conn.execute("COMMIT", []).map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "duckdb".to_string(),
                    operation: "commit".to_string(),
                    source: e.to_string(),
                })
            })?;

            self.committed = true;
        }
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CoreError> {
        if !self.committed {
            let conn = self.conn.lock().map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "duckdb".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            if let Err(e) = conn.execute("ROLLBACK", []) {
                tracing::warn!("DuckDB transaction rollback error: {}", e);
            }
            self.committed = true;
        }
        Ok(())
    }
}

/// 将 DuckDB 行转换为 Arrow 批处理
pub fn duckdb_rows_to_arrow(
    columns: &[String],
    rows: &[Vec<duckdb::types::Value>],
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

        // 遍历所有行确定最宽类型（0=Null, 1=Bool, 2=Int64, 3=Float64, 4=Blob, 5=Text）
        let mut detected_rank: u8 = 0;

        for row in rows {
            if let Some(value) = row.get(col_idx) {
                match value {
                    duckdb::types::Value::Null => {
                        string_values.push(None);
                        int_values.push(None);
                        float_values.push(None);
                        bool_values.push(None);
                        binary_values.push(None);
                    }
                    duckdb::types::Value::Boolean(b) => {
                        if detected_rank < 1 {
                            detected_rank = 1;
                        }
                        bool_values.push(Some(*b));
                    }
                    duckdb::types::Value::TinyInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::SmallInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::Int(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::BigInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i));
                    }
                    duckdb::types::Value::UTinyInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::USmallInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::UInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::UBigInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::HugeInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::Float(f) => {
                        if detected_rank < 3 {
                            detected_rank = 3;
                        }
                        float_values.push(Some(*f as f64));
                    }
                    duckdb::types::Value::Double(f) => {
                        if detected_rank < 3 {
                            detected_rank = 3;
                        }
                        float_values.push(Some(*f));
                    }
                    duckdb::types::Value::Text(s) => {
                        detected_rank = 5; // Text 为最宽类型
                        string_values.push(Some(s.clone()));
                    }
                    duckdb::types::Value::Blob(b) => {
                        if detected_rank < 4 {
                            detected_rank = 4;
                        }
                        binary_values.push(Some(b.clone()));
                    }
                    _ => {
                        string_values.push(None);
                    }
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
            db_type: "duckdb".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[async_trait::async_trait]
impl crate::core::driver::MetadataBrowser for DuckDbDatabase {
    async fn get_catalogs(&self) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
        Ok(vec![crate::core::driver::NodeInfo {
            name: "main".to_string(),
            kind: crate::core::driver::SchemaObjectKind::Catalog,
            icon: Some("database".to_string()),
            comment: None,
        }])
    }

    async fn get_schemas(
        &self,
        _catalog: &str,
    ) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
        Ok(vec![crate::core::driver::NodeInfo {
            name: "main".to_string(),
            kind: crate::core::driver::SchemaObjectKind::Schema,
            icon: Some("schema".to_string()),
            comment: None,
        }])
    }

    async fn get_tables(
        &self,
        _catalog: &str,
        _schema: &str,
    ) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
        let result = self.query("SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = 'main' ORDER BY table_name").await?;
        let nodes: Vec<crate::core::driver::NodeInfo> = (0..result.total_rows())
            .filter_map(|row_idx| {
                result.batches.iter().find_map(|batch| {
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
                            Some(crate::core::driver::NodeInfo {
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
        _catalog: &str,
        _schema: &str,
        table: &str,
    ) -> Result<crate::core::driver::NodeDetail, CoreError> {
        let safe_table = escape_sql_string(table);
        let sql = format!("SELECT column_name, data_type, is_nullable, column_default FROM information_schema.columns WHERE table_schema = 'main' AND table_name = '{}' ORDER BY ordinal_position", safe_table);
        let result = self.query(&sql).await?;
        let columns: Vec<crate::core::driver::ColumnDetail> = (0..result.total_rows())
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
                        let default = batch
                            .column(3)
                            .as_any()
                            .downcast_ref::<StringArray>()?
                            .value(row_idx);
                        Some(crate::core::driver::ColumnDetail {
                            name: col_name.to_string(),
                            data_type: data_type.to_string(),
                            nullable,
                            is_primary_key: false,
                            is_foreign_key: false,
                            default_value: if default.is_empty() {
                                None
                            } else {
                                Some(default.to_string())
                            },
                            comment: None,
                            extra: std::collections::HashMap::new(),
                        })
                    } else {
                        None
                    }
                })
            })
            .collect();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::Database;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_db_path() -> String {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("rd_duckdb_test_{}", id));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("test.duckdb");
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().to_string()
    }

    fn db() -> Result<DuckDbDatabase, CoreError> {
        let path = unique_db_path();
        DuckDbDatabase::new(&path)
    }

    #[test]
    fn test_connect() -> Result<(), CoreError> {
        DuckDbDatabase::new(&unique_db_path())?;
        Ok(())
    }

    #[tokio::test]
    async fn test_query_select_one() -> Result<(), CoreError> {
        let db = db()?;
        let result = db.query("SELECT 1 AS val").await?;
        assert_eq!(result.columns, vec!["val"]);
        Ok(())
    }

    #[tokio::test]
    async fn test_crud_roundtrip() -> Result<(), CoreError> {
        let db = db()?;

        db.query("CREATE TABLE IF NOT EXISTS _rd_test (id INTEGER, name VARCHAR, value DOUBLE)")
            .await?;

        db.query("INSERT INTO _rd_test VALUES (1, 'hello', 3.14)")
            .await?;

        let result = db
            .query("SELECT id, name, value FROM _rd_test WHERE id = 1")
            .await?;
        assert_eq!(result.columns, vec!["id", "name", "value"]);

        db.query("DROP TABLE IF EXISTS _rd_test").await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_error_handling() -> Result<(), CoreError> {
        let db = db()?;
        let result = db.query("SELECT * FROM _non_existent_table_rd").await;
        assert!(result.is_err(), "Expected error for non-existent table");
        Ok(())
    }

    #[tokio::test]
    async fn test_list_tables() -> Result<(), CoreError> {
        let db = db()?;
        let tables = db.list_tables("main", None).await;
        assert!(tables.is_ok(), "list_tables failed: {:?}", tables.err());
        Ok(())
    }

    #[tokio::test]
    async fn test_meta() -> Result<(), CoreError> {
        let db = db()?;
        let meta = db.meta();
        assert!(meta.supports_arrow);
        assert!(meta.supports_federated);
        Ok(())
    }

    #[tokio::test]
    async fn test_is_read_only_flag() -> Result<(), CoreError> {
        let db = db()?;
        let result = db.query("SELECT 1").await?;
        assert_eq!(result.is_read_only, Some(true));
        Ok(())
    }
}
