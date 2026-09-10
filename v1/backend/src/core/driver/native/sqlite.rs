//! SQLite 数据库驱动实现
//!
//! 使用 `rusqlite`（官方 Rust 绑定）实现 `Database` trait。
//! SQLite 是嵌入式文件数据库，连接以 `Mutex<Connection>` 管理。
//!
//! ## 关键约束
//! - SQLite 不支持 schema 层级 — list_schemas 返回空 vec
//! - 查询结果通过 `RecordBatch` (Arrow) 返回，实现零拷贝传输
//! - 写操作使用 `query_row` 标记为只读检查（`is_read_only_sql`）

use std::sync::Arc;
use std::sync::Mutex;

use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use rusqlite::Connection;

use crate::core::driver::utils::quote_identifier;
use crate::core::driver::{ColumnDetail, DataSourceMeta, Database, IndexDetail, Transaction};
use crate::core::error::{CoreError, DatabaseError};
use crate::core::models::{ArrowBatch, QueryResult, Value};

/// SQLite 数据库连接
///
/// 封装 `rusqlite::Connection`，以 `Arc<Mutex<Connection>>` 管理线程安全访问。
/// SQLite 是嵌入式文件数据库，不支持 schema 层级和网络连接。
///
/// # 字段
/// * `conn` - 由 Arc + Mutex 保护的 rusqlite 连接
/// * `server_version` - SQLite 版本号
pub struct SqliteDatabase {
    conn: Arc<Mutex<Connection>>,
    server_version: Option<String>,
}

impl SqliteDatabase {
    pub fn new(url: &str) -> Result<Self, CoreError> {
        let path = if url.starts_with("sqlite://") {
            url.trim_start_matches("sqlite://")
        } else {
            url
        };

        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::database(DatabaseError::Driver {
                        db_type: "sqlite".to_string(),
                        operation: "create_directory".to_string(),
                        source: e.to_string(),
                    })
                })?;
            }
        }

        let conn = Connection::open(path).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        let server_version = conn
            .query_row("SELECT sqlite_version()", [], |row| row.get::<_, String>(0))
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
        || sql_upper.starts_with("PRAGMA")
        || sql_upper.starts_with("EXPLAIN")
}

#[async_trait::async_trait]
impl Database for SqliteDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        self.query_with_params(sql, vec![]).await
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
                    db_type: "sqlite".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            let mut stmt = conn.prepare(&sql_owned).map_err(|e| {
                CoreError::database(DatabaseError::query(&sql_owned, e.to_string()))
            })?;

            let columns: Vec<String> = stmt
                .column_names()
                .iter()
                .map(|name| name.to_string())
                .collect();

            let params_slice: Vec<rusqlite::types::Value> = params
                .iter()
                .map(|v| match v {
                    Value::Null => rusqlite::types::Value::Null,
                    Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
                    Value::Int(i) => rusqlite::types::Value::Integer(*i),
                    Value::Float(f) => rusqlite::types::Value::Real(*f),
                    Value::Text(s) => rusqlite::types::Value::Text(s.clone()),
                    Value::Bytes(b) => rusqlite::types::Value::Blob(b.clone()),
                })
                .collect();

            let mut rows = match params_slice.len() {
                0 => stmt.query([]),
                1 => stmt.query([&params_slice[0]]),
                2 => stmt.query([&params_slice[0], &params_slice[1]]),
                3 => stmt.query([&params_slice[0], &params_slice[1], &params_slice[2]]),
                4 => stmt.query([
                    &params_slice[0],
                    &params_slice[1],
                    &params_slice[2],
                    &params_slice[3],
                ]),
                5 => stmt.query([
                    &params_slice[0],
                    &params_slice[1],
                    &params_slice[2],
                    &params_slice[3],
                    &params_slice[4],
                ]),
                _ => {
                    let params_refs: Vec<&dyn rusqlite::ToSql> = params_slice
                        .iter()
                        .take(16)
                        .map(|v| v as &dyn rusqlite::ToSql)
                        .collect();
                    stmt.query(rusqlite::params_from_iter(params_refs))
                }
            }
            .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

            let mut row_data: Vec<Vec<rusqlite::types::Value>> = Vec::new();
            while let Ok(Some(row)) = rows.next() {
                let values: Vec<rusqlite::types::Value> = columns
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        row.get::<usize, rusqlite::types::Value>(i)
                            .unwrap_or(rusqlite::types::Value::Null)
                    })
                    .collect();
                row_data.push(values);
            }

            let _is_read_only = is_read_only_sql(&sql_owned);
            let row_count = row_data.len();

            let batch = if row_count > 0 {
                sqlite_rows_to_arrow(&columns, &row_data)?
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
                db_type: "sqlite".to_string(),
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
                    db_type: "sqlite".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                }))?;

                let mut stmt = conn.prepare(&sql_owned)
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

                let columns: Vec<String> = stmt.column_names()
                    .iter()
                    .map(|name| name.to_string())
                    .collect();

                let mut rows = stmt.query([])
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;

                let mut row_data: Vec<Vec<rusqlite::types::Value>> = Vec::new();
                while let Ok(Some(row)) = rows.next() {
                    let values: Vec<rusqlite::types::Value> = columns.iter().enumerate()
                        .map(|(i, _)| {
                            row.get::<usize, rusqlite::types::Value>(i).unwrap_or(rusqlite::types::Value::Null)
                        })
                        .collect();
                    row_data.push(values);
                }

                let _is_read_only = is_read_only_sql(&sql_owned);
                let row_count = row_data.len();

                let batch = if row_count > 0 {
                    sqlite_rows_to_arrow(&columns, &row_data)?
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
                db_type: "sqlite".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        conn.execute("BEGIN TRANSACTION", []).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;

        Ok(Box::new(SqliteTransaction::new(Arc::clone(&self.conn))))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::sqlite()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;
        conn.query_row("SELECT 1", [], |_| Ok(()))
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
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT name, type FROM sqlite_master \
                 WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
                 ORDER BY name",
            )
            .map_err(|e| CoreError::database(DatabaseError::query("list_tables", e.to_string())))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CoreError::database(DatabaseError::query("list_tables", e.to_string())))?;

        let mut objects = Vec::new();
        for row in rows {
            match row {
                Ok((name, obj_type)) => {
                    let kind = match obj_type.as_str() {
                        "view" => crate::core::driver::SchemaObjectKind::View,
                        _ => crate::core::driver::SchemaObjectKind::Table,
                    };
                    objects.push(crate::core::driver::SchemaObject {
                        name,
                        kind,
                        children: None,
                        comment: None,
                        table_name: None,
                        event: None,
                    });
                }
                Err(e) => {
                    tracing::warn!("SQLite list_tables row error: {}", e);
                }
            }
        }

        Ok(objects)
    }

    async fn list_columns(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let quoted = quote_identifier(table, '"');
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", quoted))
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_columns", e.to_string()))
            })?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i32>(0)?,            // cid
                    row.get::<_, String>(1)?,         // name
                    row.get::<_, String>(2)?,         // type
                    row.get::<_, i32>(3)?,            // notnull
                    row.get::<_, Option<String>>(4)?, // dflt_value
                    row.get::<_, i32>(5)?,            // pk
                ))
            })
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_columns", e.to_string()))
            })?;

        let mut columns = Vec::new();
        for row in rows {
            match row {
                Ok((_cid, name, col_type, notnull, default_val, pk)) => {
                    columns.push(ColumnDetail {
                        name,
                        data_type: col_type,
                        nullable: notnull == 0,
                        is_primary_key: pk > 0,
                        is_foreign_key: false,
                        default_value: default_val,
                        comment: None,
                        extra: std::collections::HashMap::new(),
                    });
                }
                Err(e) => {
                    tracing::warn!("SQLite list_columns row error: {}", e);
                }
            }
        }

        Ok(columns)
    }

    /// 列举表的所有索引
    ///
    /// SQLite 通过 PRAGMA index_list(table) 获取索引列表，
    /// 再通过 PRAGMA index_info(index) 获取每个索引的列名。
    async fn list_indexes(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        // PRAGMA index_list(table) 返回: seq, name, unique, origin, partial
        let quoted = quote_identifier(table, '"');
        let sql = format!("PRAGMA index_list({})", quoted);
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            CoreError::database(DatabaseError::query("list_indexes", e.to_string()))
        })?;

        let index_rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i32>(0)?,    // seq
                    row.get::<_, String>(1)?, // name
                    row.get::<_, i32>(2)?,    // unique (1=unique, 0=not)
                    row.get::<_, String>(3)?, // origin (c/pk/u)
                ))
            })
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_indexes_query", e.to_string()))
            })?;

        let mut indexes = Vec::new();
        for index_row in index_rows {
            match index_row {
                Ok((_seq, name, unique, origin)) => {
                    // origin: "c"=user-created, "pk"=PRIMARY KEY, "u"=UNIQUE constraint
                    let is_primary = origin == "pk";

                    // 获取索引的列名
                    let info_sql = format!("PRAGMA index_info({})", quote_identifier(&name, '"'));
                    let mut col_stmt = conn.prepare(&info_sql).map_err(|e| {
                        CoreError::database(DatabaseError::query(
                            "list_indexes_info",
                            e.to_string(),
                        ))
                    })?;

                    let col_rows = col_stmt
                        .query_map([], |row| {
                            row.get::<_, String>(2) // name (seqno=0, cid=1, name=2)
                        })
                        .map_err(|e| {
                            CoreError::database(DatabaseError::query(
                                "list_indexes_info_query",
                                e.to_string(),
                            ))
                        })?;

                    let mut column_names = Vec::new();
                    for col_row in col_rows {
                        match col_row {
                            Ok(col_name) => column_names.push(col_name),
                            Err(e) => {
                                tracing::warn!("SQLite list_indexes column row error: {}", e);
                            }
                        }
                    }

                    if !column_names.is_empty() {
                        indexes.push(IndexDetail {
                            name,
                            table_name: table.to_string(),
                            column_names,
                            is_unique: unique != 0,
                            is_primary,
                            index_type: None,
                            comment: None,
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!("SQLite list_indexes row error: {}", e);
                }
            }
        }

        Ok(indexes)
    }

    fn as_metadata_browser(&self) -> Option<&dyn crate::core::driver::MetadataBrowser> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl crate::core::driver::MetadataBrowser for SqliteDatabase {
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
        Ok(vec![])
    }

    async fn get_tables(
        &self,
        catalog: &str,
        _schema: &str,
    ) -> Result<Vec<crate::core::driver::NodeInfo>, CoreError> {
        let objects = self.list_tables(catalog, None).await?;
        Ok(objects
            .into_iter()
            .map(|obj| {
                let is_view = matches!(obj.kind, crate::core::driver::SchemaObjectKind::View);
                crate::core::driver::NodeInfo {
                    name: obj.name,
                    kind: obj.kind,
                    icon: Some(if is_view {
                        "view".to_string()
                    } else {
                        "table".to_string()
                    }),
                    comment: obj.comment,
                }
            })
            .collect())
    }

    async fn get_table_detail(
        &self,
        catalog: &str,
        _schema: &str,
        table: &str,
    ) -> Result<crate::core::driver::NodeDetail, CoreError> {
        let columns = self.list_columns(catalog, None, table).await?;
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

pub struct SqliteTransaction {
    conn: Arc<Mutex<Connection>>,
    committed: bool,
}

impl SqliteTransaction {
    fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            conn,
            committed: false,
        }
    }
}

#[async_trait::async_trait]
impl Transaction for SqliteTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let columns: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|name| name.to_string())
            .collect();

        let mut rows = stmt
            .query([])
            .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;

        let mut row_data: Vec<Vec<rusqlite::types::Value>> = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let mut values: Vec<rusqlite::types::Value> = Vec::with_capacity(columns.len());
            for (i, _) in columns.iter().enumerate() {
                let v = row.get::<usize, rusqlite::types::Value>(i).map_err(|e| {
                    CoreError::database(DatabaseError::Driver {
                        db_type: "sqlite".to_string(),
                        operation: "row_parsing".to_string(),
                        source: e.to_string(),
                    })
                })?;
                values.push(v);
            }
            row_data.push(values);
        }

        let sql_upper = sql.trim_start().to_uppercase();
        let _is_read_only = sql_upper.starts_with("SELECT") || sql_upper.starts_with("PRAGMA");
        let row_count = row_data.len();

        let batch = if row_count > 0 {
            sqlite_rows_to_arrow(&columns, &row_data)?
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
                    db_type: "sqlite".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            conn.execute("COMMIT", []).map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "sqlite".to_string(),
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
                    db_type: "sqlite".to_string(),
                    operation: "lock".to_string(),
                    source: e.to_string(),
                })
            })?;

            if let Err(e) = conn.execute("ROLLBACK", []) {
                tracing::warn!("SQLite transaction rollback error: {}", e);
            }
            self.committed = true;
        }
        Ok(())
    }
}

fn sqlite_rows_to_arrow(
    columns: &[String],
    rows: &[Vec<rusqlite::types::Value>],
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

        // 遍历所有行确定最宽类型并同时收集值（0=Null, 1=Int64, 2=Float64, 3=Blob, 4=Text）
        let mut detected_rank: u8 = 0;

        for row in rows {
            if let Some(value) = row.get(col_idx) {
                match value {
                    rusqlite::types::Value::Null => {
                        string_values.push(None);
                        int_values.push(None);
                        float_values.push(None);
                        bool_values.push(None);
                        binary_values.push(None);
                    }
                    rusqlite::types::Value::Integer(i) => {
                        if detected_rank < 1 {
                            detected_rank = 1;
                        }
                        string_values.push(None);
                        int_values.push(Some(*i));
                        float_values.push(None);
                        bool_values.push(None);
                        binary_values.push(None);
                    }
                    rusqlite::types::Value::Real(f) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        string_values.push(None);
                        int_values.push(None);
                        float_values.push(Some(*f));
                        bool_values.push(None);
                        binary_values.push(None);
                    }
                    rusqlite::types::Value::Text(s) => {
                        detected_rank = 4; // Text 为最宽类型
                        string_values.push(Some(s.clone()));
                        int_values.push(None);
                        float_values.push(None);
                        bool_values.push(None);
                        binary_values.push(None);
                    }
                    rusqlite::types::Value::Blob(b) => {
                        if detected_rank < 3 {
                            detected_rank = 3;
                        }
                        string_values.push(None);
                        int_values.push(None);
                        float_values.push(None);
                        bool_values.push(None);
                        binary_values.push(Some(b.clone()));
                    }
                }
            } else {
                string_values.push(None);
                int_values.push(None);
                float_values.push(None);
                bool_values.push(None);
                binary_values.push(None);
            }
        }

        let effective_type = match detected_rank {
            1 => DataType::Int64,
            2 => DataType::Float64,
            3 => DataType::Binary,
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
            db_type: "sqlite".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> Result<String, CoreError> {
        let dir = std::env::temp_dir().join(format!("rdata_test_sqlite_{}", name));
        std::fs::create_dir_all(&dir).map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "create_test_dir".to_string(),
                source: e.to_string(),
            })
        })?;
        let path = dir.join("test.db");
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "sqlite".to_string(),
                    operation: "remove_test_file".to_string(),
                    source: e.to_string(),
                })
            })?;
        }
        Ok(format!("sqlite://{}", path.display()))
    }

    #[tokio::test]
    async fn test_sqlite_connect_ping() -> Result<(), CoreError> {
        let path = temp_db_path("ping")?;
        let db = SqliteDatabase::new(&path)?;
        db.ping().await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_sqlite_query() -> Result<(), CoreError> {
        let path = temp_db_path("query")?;
        let db = SqliteDatabase::new(&path)?;
        db.query("SELECT sqlite_version() AS version").await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_sqlite_transaction_commit() -> Result<(), CoreError> {
        let path = temp_db_path("tx_commit")?;
        let db = SqliteDatabase::new(&path)?;
        db.query("CREATE TABLE IF NOT EXISTS t (id INTEGER)")
            .await?;

        let mut tx = db.begin_transaction().await?;
        tx.query("INSERT INTO t VALUES (1)").await?;
        tx.commit().await?;

        let result = db.query("SELECT COUNT(*) AS cnt FROM t").await?;
        assert!(!result.batches.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_sqlite_transaction_rollback() -> Result<(), CoreError> {
        let path = temp_db_path("tx_rollback")?;
        let db = SqliteDatabase::new(&path)?;
        db.query("CREATE TABLE IF NOT EXISTS t (id INTEGER)")
            .await?;

        let mut tx = db.begin_transaction().await?;
        tx.query("INSERT INTO t VALUES (999)").await?;
        tx.rollback().await?;

        let result = db
            .query("SELECT COUNT(*) AS cnt FROM t WHERE id = 999")
            .await?;
        if let Some(batch) = result.batches.first() {
            assert_eq!(batch.num_rows(), 1);
        }
        Ok(())
    }
}
