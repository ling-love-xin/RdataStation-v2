//! PostgreSQL 的 sqlx 实现（插件侧）。语义对齐内核 `crates/driver/src/native/postgres.rs`：
//! 单连接会话、`ssl-mode` 归一、元数据 SQL 与 `mysql`/PG 同族共用一份
//! （`rds-driver-sql::meta_sql`）。
//!
//! PG 的这三条内省 SQL **不吃 catalog 参数**：一条连接只绑一个库，跨库浏览要靠
//! 「按库另开连接」（宿主 `catalog_is_connection_scoped` 负责）。

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::TryStreamExt as _;
use rds_driver_sidecar::{
    ColumnDef, Driver, DriverError, ErrorCode, ExecuteRequest, MetaColumn, MetaObject,
    MetaObjectDetail, MetaSchema, Page, SessionInfo,
};
use rds_driver_sql::meta_sql;
use serde_json::Value;
use sqlx::{Column as _, Connection as _, PgConnection, Row as _, TypeInfo as _};

use crate::config::{redact_url, ConnConfig};
use crate::values::{cell_as_text, cell_is_null, map_error, pg_canonical, returns_rows};

pub struct PostgresDriver;

pub struct PostgresSession {
    conn: PgConnection,
    query_timeout: Duration,
}

#[async_trait]
impl Driver for PostgresDriver {
    type Session = PostgresSession;

    fn driver_id(&self) -> &'static str {
        "postgres"
    }

    fn display_name(&self) -> &'static str {
        "PostgreSQL (sqlx)"
    }

    async fn open(&self, params: &Value) -> Result<(Self::Session, SessionInfo), DriverError> {
        let config = ConnConfig::parse(params).map_err(DriverError::connect)?;
        let url = config.postgres_url().map_err(DriverError::connect)?;
        let connect_timeout = config.connect_timeout();
        let query_timeout = config.query_timeout();

        let mut conn = match tokio::time::timeout(connect_timeout, PgConnection::connect(&url)).await
        {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => {
                return Err(DriverError::connect(format!(
                    "连接 PostgreSQL 失败（{}）：{err}",
                    redact_url(&url)
                )));
            }
            Err(_) => {
                return Err(DriverError::connect(format!(
                    "连接 PostgreSQL 超时（{} 秒无响应）：{}",
                    connect_timeout.as_secs(),
                    redact_url(&url)
                )));
            }
        };

        let version: String =
            match tokio::time::timeout(query_timeout, sqlx::query_scalar("SELECT version()").fetch_one(&mut conn)).await
            {
                Ok(Ok(version)) => version,
                Ok(Err(err)) => {
                    return Err(DriverError::connect(format!(
                        "连接 PostgreSQL 后 `SELECT version()` 失败：{err}"
                    )));
                }
                Err(_) => {
                    return Err(DriverError::connect(format!(
                        "连接 PostgreSQL 后 `SELECT version()` 超时（{} 秒无响应）",
                        query_timeout.as_secs()
                    )));
                }
            };

        // `current_schema()` 可能为 NULL（search_path 全空），此时 default_schema 留空。
        let default_schema: Option<String> = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&mut conn)
            .await
            .ok()
            .flatten();

        Ok((
            PostgresSession { conn, query_timeout },
            SessionInfo {
                server_version: Some(version),
                default_schema,
                capabilities: capabilities(),
            },
        ))
    }

    async fn close(&self, session: Self::Session) {
        let _ = session.conn.close().await;
    }

    async fn ping(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        let _: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&mut session.conn)
            .await
            .map_err(map_error)?;
        Ok(())
    }

    async fn execute(
        &self,
        session: &mut Self::Session,
        req: ExecuteRequest,
    ) -> Result<Page, DriverError> {
        let timeout = session.query_timeout;
        match tokio::time::timeout(timeout, run_query(session, req)).await {
            Ok(outcome) => outcome,
            Err(_) => Err(DriverError::new(
                ErrorCode::Timeout,
                format!("查询超时（{} 秒无响应）", timeout.as_secs()),
            )),
        }
    }

    async fn begin(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        exec_statement(&mut session.conn, "BEGIN").await
    }

    async fn commit(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        exec_statement(&mut session.conn, "COMMIT").await
    }

    async fn rollback(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        exec_statement(&mut session.conn, "ROLLBACK").await
    }

    /// 服务器上**可连接**的库（不是 `current_database()`）：导航里要看到别的库，
    /// 跨库内容由宿主按库另开连接去取。
    async fn catalogs(&self, session: &mut Self::Session) -> Result<Vec<String>, DriverError> {
        let names: Vec<String> = sqlx::query_scalar(meta_sql::PG_LIST_CATALOGS_SQL)
            .fetch_all(&mut session.conn)
            .await
            .map_err(map_error)?;
        Ok(names)
    }

    async fn schemas(&self, session: &mut Self::Session) -> Result<Vec<MetaSchema>, DriverError> {
        let names: Vec<String> = sqlx::query_scalar(meta_sql::PG_LIST_SCHEMAS_SQL)
            .fetch_all(&mut session.conn)
            .await
            .map_err(map_error)?;
        Ok(names
            .into_iter()
            .map(|name| MetaSchema {
                name,
                comment: None,
            })
            .collect())
    }

    async fn objects(
        &self,
        session: &mut Self::Session,
        schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        let rows = sqlx::query(meta_sql::PG_LIST_TABLES_SQL)
            .bind(schema)
            .fetch_all(&mut session.conn)
            .await
            .map_err(map_error)?;
        Ok(rows
            .iter()
            .map(|row| {
                let name = text_of(row, 0);
                let relkind = text_of(row, 1).to_ascii_uppercase();
                // SQL 侧已把 `v` / `m` 归成 VIEW，其余是 BASE TABLE；物化视图并入视图
                // 是本仓导航口径（见 `docs/architecture/references/README.md`）。
                let kind = if relkind == "VIEW" { "view" } else { "table" };
                // 第 3 列是分区/继承的父表（可能为 NULL）：子表挂在父表下，
                // 导航据此折叠；没有父表就是顶层对象。
                let parent = non_empty(text_of(row, 2)).unwrap_or_else(|| schema.to_string());
                MetaObject {
                    name,
                    kind: kind.to_string(),
                    comment: None,
                    parent: Some(parent),
                }
            })
            .collect())
    }

    async fn object_detail(
        &self,
        session: &mut Self::Session,
        schema: &str,
        name: &str,
    ) -> Result<MetaObjectDetail, DriverError> {
        let rows = sqlx::query(meta_sql::PG_TABLE_DETAIL_SQL)
            .bind(schema)
            .bind(name)
            .fetch_all(&mut session.conn)
            .await
            .map_err(map_error)?;
        let columns: Vec<MetaColumn> = rows
            .iter()
            .map(|row| {
                let type_raw = text_of(row, 1);
                let ref_table = text_of(row, 7);
                MetaColumn {
                    canonical: Some(pg_canonical(&type_raw).as_str().to_string()),
                    name: text_of(row, 0),
                    type_raw,
                    nullable: Some(text_of(row, 2) == "YES"),
                    is_pk: Some(text_of(row, 4) == "PRI"),
                    is_fk: Some(text_of(row, 9) == "1" || !ref_table.is_empty()),
                    default: non_empty(text_of(row, 5)),
                    format: None,
                    comment: non_empty(text_of(row, 6)),
                }
            })
            .collect();
        Ok(MetaObjectDetail::new(name, "table", columns))
    }
}

async fn run_query(session: &mut PostgresSession, req: ExecuteRequest) -> Result<Page, DriverError> {
    let sql = req.sql;
    if !returns_rows(&sql) {
        let query = bind_params(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), &req.params)?;
        let result = query.execute(&mut session.conn).await.map_err(map_error)?;
        return Ok(Page::affected(result.rows_affected()));
    }

    let max_rows = req.max_rows.max(1);
    let query = bind_params(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), &req.params)?;
    let mut stream = query.fetch(&mut session.conn);
    let mut columns: Option<Vec<ColumnDef>> = None;
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut has_more = false;

    while let Some(row) = stream.try_next().await.map_err(map_error)? {
        if columns.is_none() {
            columns = Some(columns_of(&row));
        }
        if rows.len() >= max_rows {
            has_more = true;
            break;
        }
        let cols = columns.as_ref().expect("刚填过列头");
        rows.push(decode_row(&row, cols));
    }

    let mut page = Page::rows(columns.unwrap_or_default(), rows);
    page.has_more = has_more;
    Ok(page)
}

/// 事务语句只有三条字面量，签名钉成 `&'static str`（同 mysql.rs 的理由）。
async fn exec_statement(conn: &mut PgConnection, sql: &'static str) -> Result<(), DriverError> {
    sqlx::query(sql)
        .execute(&mut *conn)
        .await
        .map_err(map_error)?;
    Ok(())
}

fn columns_of(row: &sqlx::postgres::PgRow) -> Vec<ColumnDef> {
    row.columns()
        .iter()
        .map(|col| {
            let type_raw = col.type_info().name().to_string();
            ColumnDef::new(col.name(), type_raw.clone(), true, pg_canonical(&type_raw))
        })
        .collect()
}

fn decode_row(row: &sqlx::postgres::PgRow, columns: &[ColumnDef]) -> Vec<Value> {
    (0..columns.len())
        .map(|idx| {
            if cell_is_null(row, idx) {
                return Value::Null;
            }
            match cell_as_text!(row, idx) {
                Some(text) => Value::String(text),
                None => Value::String(format!("<无法解码：{}>", columns[idx].type_raw)),
            }
        })
        .collect()
}

fn bind_params<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    params: &'q [Value],
) -> Result<sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>, DriverError> {
    for param in params {
        query = match param {
            Value::Null => query.bind(None::<&str>),
            Value::Bool(value) => query.bind(*value),
            Value::Number(number) => match number.as_i64() {
                Some(value) => query.bind(value),
                None => query.bind(number.as_f64().unwrap_or(0.0)),
            },
            Value::String(value) => query.bind(value.as_str()),
            other => {
                return Err(DriverError::unsupported(format!(
                    "参数只支持标量（null / bool / 数字 / 字符串），收到 {}",
                    json_kind(other)
                )));
            }
        };
    }
    Ok(query)
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn text_of(row: &sqlx::postgres::PgRow, idx: usize) -> String {
    row.try_get::<Option<String>, _>(idx)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

/// 能力面如实声明（同 `mysql.rs::capabilities` 的口径）。
///
/// 相比内核 `descriptors.rs` 的 PG 声明，这里**少了** `meta_index_detail` /
/// `meta_constraint_detail` / `meta_triggers` / `meta_routine_source` /
/// `meta_sequences` —— sidecar 协议没有这些通道（`rust-core` 直接回「不支持」），
/// 也**没有** `meta_ddl_source`（PG 本来就没有 `SHOW CREATE TABLE`，DDL 靠宿主合成）。
fn capabilities() -> BTreeMap<String, bool> {
    let mut caps = BTreeMap::new();
    caps.insert("tree".to_string(), true);
    caps.insert("health_check".to_string(), true);
    caps.insert("transactions".to_string(), true);
    caps.insert("sql_autocomplete".to_string(), true);
    caps.insert("schema_browser".to_string(), true);
    caps.insert("meta_ddl_compose".to_string(), true);
    caps.insert("cancel".to_string(), false);
    caps
}
