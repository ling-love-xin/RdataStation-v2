//! MySQL 的 sqlx 实现（插件侧）。语义对齐内核 `crates/driver/src/native/mysql.rs`：
//! 单连接、懒重建的会话模型由宿主 `concurrency="serial"` 保证（一个会话一条连接，
//! 请求串行），这里不需要池。
//!
//! 与内核的差别只在**出口协议**：结果集走 `Page`（小结果内联 JSON / 大结果 Arrow），
//! 不再产 `QueryResult`。

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
use sqlx::{Column as _, Connection as _, MySqlConnection, Row as _, TypeInfo as _};

use crate::config::{redact_url, ConnConfig};
use crate::values::{cell_as_text, cell_is_null, map_error, mysql_canonical, returns_rows};

/// 系统库不进导航（与内核同口径：`information_schema` / `mysql` /
/// `performance_schema` / `sys`）。MySQL 的 database 即 schema，所以过滤在这里。
const SYSTEM_SCHEMAS: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];

/// 列出某个 database 下的对象。这条 SQL 内核也是就地写的（不在 `rds-driver-sql` 里），
/// 因为只有它一条、没有第二个实现要用（`mysql_native` 走的是它自己的查询）。
const MY_LIST_TABLES_SQL: &str =
    "SELECT table_name, table_type FROM information_schema.tables \
     WHERE table_schema = ? ORDER BY table_name";

const MY_LIST_CATALOGS_SQL: &str =
    "SELECT schema_name FROM information_schema.schemata ORDER BY schema_name";

pub struct MysqlDriver;

pub struct MysqlSession {
    conn: MySqlConnection,
    query_timeout: Duration,
}

#[async_trait]
impl Driver for MysqlDriver {
    type Session = MysqlSession;

    fn driver_id(&self) -> &'static str {
        "mysql"
    }

    fn display_name(&self) -> &'static str {
        "MySQL (sqlx)"
    }

    fn identifier_quote(&self) -> Option<&'static str> {
        Some("`")
    }

    async fn open(&self, params: &Value) -> Result<(Self::Session, SessionInfo), DriverError> {
        let config = ConnConfig::parse(params).map_err(DriverError::connect)?;
        let url = config.mysql_url().map_err(DriverError::connect)?;
        let connect_timeout = config.connect_timeout();
        let query_timeout = config.query_timeout();

        let mut conn = match tokio::time::timeout(connect_timeout, MySqlConnection::connect(&url))
            .await
        {
            Ok(Ok(conn)) => conn,
            Ok(Err(err)) => {
                return Err(DriverError::connect(format!(
                    "连接 MySQL 失败（{}）：{err}",
                    redact_url(&url)
                )));
            }
            Err(_) => {
                return Err(DriverError::connect(format!(
                    "连接 MySQL 超时（{} 秒无响应）：{}",
                    connect_timeout.as_secs(),
                    redact_url(&url)
                )));
            }
        };

        // 版本探测同时是「这条连接真的能用」的验证：内核在 `SELECT VERSION()` 处
        // **不吞错**，这里的失败算连接失败（不是查询失败）。
        let version: String =
            match tokio::time::timeout(query_timeout, sqlx::query_scalar("SELECT VERSION()").fetch_one(&mut conn)).await
            {
                Ok(Ok(version)) => version,
                Ok(Err(err)) => {
                    return Err(DriverError::connect(format!(
                        "连接 MySQL 后 `SELECT VERSION()` 失败：{err}"
                    )));
                }
                Err(_) => {
                    return Err(DriverError::connect(format!(
                        "连接 MySQL 后 `SELECT VERSION()` 超时（{} 秒无响应）",
                        query_timeout.as_secs()
                    )));
                }
            };

        let default_schema = config
            .database
            .clone()
            .map(|database| database.trim().to_string())
            .filter(|database| !database.is_empty());

        Ok((
            MysqlSession { conn, query_timeout },
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
        exec_statement(&mut session.conn, "START TRANSACTION").await
    }

    async fn commit(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        exec_statement(&mut session.conn, "COMMIT").await
    }

    async fn rollback(&self, session: &mut Self::Session) -> Result<(), DriverError> {
        exec_statement(&mut session.conn, "ROLLBACK").await
    }

    async fn catalogs(&self, session: &mut Self::Session) -> Result<Vec<String>, DriverError> {
        let names: Vec<String> = sqlx::query_scalar(MY_LIST_CATALOGS_SQL)
            .fetch_all(&mut session.conn)
            .await
            .map_err(map_error)?;
        Ok(names
            .into_iter()
            .filter(|name| !SYSTEM_SCHEMAS.contains(&name.as_str()))
            .collect())
    }

    /// MySQL 的 database 即 schema，没有独立的 Schema 层级 → 返回空（与内核同口径：
    /// 回退成 catalog 列表会让导航出现同名重复层）。
    async fn schemas(&self, _session: &mut Self::Session) -> Result<Vec<MetaSchema>, DriverError> {
        Ok(Vec::new())
    }

    async fn objects(
        &self,
        session: &mut Self::Session,
        schema: &str,
    ) -> Result<Vec<MetaObject>, DriverError> {
        let rows = sqlx::query(MY_LIST_TABLES_SQL)
            .bind(schema)
            .fetch_all(&mut session.conn)
            .await
            .map_err(map_error)?;
        Ok(rows
            .iter()
            .map(|row| {
                let name = text_of(row, 0);
                let table_type = text_of(row, 1).to_ascii_uppercase();
                MetaObject {
                    name,
                    kind: if table_type == "VIEW" { "view" } else { "table" }.to_string(),
                    comment: None,
                    parent: Some(schema.to_string()),
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
        // 列序契约由 `rds-driver-sql` 的 `MY_TABLE_DETAIL_SQL` 头注释钉住
        // （0..=12：列名/类型/可空/序号/主键/默认/注释/引用表/引用列/外键/标识/生成方式/排序规则）。
        let rows = sqlx::query(meta_sql::MY_TABLE_DETAIL_SQL)
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
                    canonical: Some(mysql_canonical(&type_raw).as_str().to_string()),
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

/// 一条请求的执行主体（外层统一套 `query_timeout`）。
async fn run_query(session: &mut MysqlSession, req: ExecuteRequest) -> Result<Page, DriverError> {
    let sql = req.sql;
    if !returns_rows(&sql) {
        // DML / DDL：批读拿不到 `rows_affected`，必须走 execute。
        // `AssertSqlSafe` 是 sqlx 0.9 对「运行时拼出来的 SQL」的显式放行（内核同写法）：
        // 语句来自用户的 SQL 编辑器，本来就不是编译期字面量。
        let query = bind_params(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), &req.params)?;
        let result = query
            .execute(&mut session.conn)
            .await
            .map_err(map_error)?;
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
        // 多取一行只为了知道「还有没有下一行」：截断必须是显式的 `has_more`，
        // 不能让界面上多出来的行凭空消失（那看着像「查询只返回了 1000 行」）。
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

/// 事务语句只有三条字面量，签名钉成 `&'static str`：sqlx 0.9 的静态 SQL 检查
/// 要的就是这个生命周期，动态拼的语句走 `AssertSqlSafe`（见 `run_query`）。
async fn exec_statement(conn: &mut MySqlConnection, sql: &'static str) -> Result<(), DriverError> {
    sqlx::query(sql)
        .execute(&mut *conn)
        .await
        .map_err(map_error)?;
    Ok(())
}

fn columns_of(row: &sqlx::mysql::MySqlRow) -> Vec<ColumnDef> {
    row.columns()
        .iter()
        .map(|col| {
            let type_raw = col.type_info().name().to_string();
            ColumnDef::new(
                col.name(),
                type_raw.clone(),
                true,
                mysql_canonical(&type_raw),
            )
        })
        .collect()
}

fn decode_row(row: &sqlx::mysql::MySqlRow, columns: &[ColumnDef]) -> Vec<Value> {
    (0..columns.len())
        .map(|idx| {
            if cell_is_null(row, idx) {
                return Value::Null;
            }
            match cell_as_text!(row, idx) {
                Some(text) => Value::String(text),
                // 解不出来**不能**折成 NULL（那样「库里有值」在界面上就是空白格）：
                // 显式给一个可读的占位，类型名带上，用户知道该报哪个驱动 bug。
                None => Value::String(format!("<无法解码：{}>", columns[idx].type_raw)),
            }
        })
        .collect()
}

/// 绑定参数。与内核 `native/mysql.rs` 的 `Value` → `bind` 映射同口径
/// （null / bool / int / float / text / bytes）；JSON 数组与对象不支持 ——
/// 宿主 UI 不会产生这两类，真给了就明确报错，而不是悄悄拼成字符串。
fn bind_params<'q>(
    mut query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    params: &'q [Value],
) -> Result<sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>, DriverError> {
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

fn text_of(row: &sqlx::mysql::MySqlRow, idx: usize) -> String {
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

/// 能力面如实声明（`driver.describe` 的 `capabilities` 才是真能力，清单可以撒谎）。
///
/// **不声明**的（当前 sidecar 协议里没有通道，声明了就是骗宿主）：
/// * `meta_index_detail` / `meta_constraint_detail`：`rust-core` 的
///   `list_indexes` / `list_constraints` 直接回「不支持」，协议只给索引**个数**；
/// * `meta_ddl_source`：没有 `get_table_ddl` 协议方法；
/// * `meta_triggers` / `meta_routine_source` / `meta_sequences`：没有对应协议方法。
///
/// 属性面板拿这些键判「置灰 + 原因」，所以缺席是**正确的输入**，不是功能缺失。
fn capabilities() -> BTreeMap<String, bool> {
    let mut caps = BTreeMap::new();
    caps.insert("tree".to_string(), true);
    caps.insert("health_check".to_string(), true);
    caps.insert("transactions".to_string(), true);
    caps.insert("sql_autocomplete".to_string(), true);
    // DDL 由宿主 `sql_gen::create_table_ddl` 合成（不依赖驱动的 `SHOW CREATE`）。
    caps.insert("meta_ddl_compose".to_string(), true);
    // 取消只能置标记、不真中断在飞查询（协议不扩，见计划 R1）。
    caps.insert("cancel".to_string(), false);
    caps
}
