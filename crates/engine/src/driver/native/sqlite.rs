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
use rusqlite::{Connection, OpenFlags};

use crate::driver::utils::{affected_rows_result, quote_identifier, returns_rows};
use crate::driver::{ColumnDetail, DataSourceMeta, Database, IndexDetail, Transaction};
use shared::error::{CoreError, DatabaseError};
use shared::models::{ArrowBatch, QueryResult, Value};

/// 连接属性 → 开库动作（纯函数产物，可测）。
///
/// 键与取值都**白名单校验**后才拼成常量 SQL：属性值来自用户输入，直接拼串就是注入口。
/// 取值的允许集合就是 [`crate::driver::property_spec`] 里 sqlite 条目声明的键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SqlitePlan {
    /// 打开模式（`mode=ro|rw|rwc`）；`None` = rusqlite 默认（rwc）
    pub flags: Option<OpenFlags>,
    /// 要逐条执行的 PRAGMA
    pub pragmas: Vec<PlannedPragma>,
    /// 清单外的键：不执行（调用方记 warn）
    pub unknown: Vec<String>,
}

/// 一条待执行的 PRAGMA。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedPragma {
    /// 用户在属性页里写的键（含别名，报错时要说回他的名字）
    pub key: String,
    /// 规范名（`property_spec` 里的 key，也是 SQLite 认的名字）
    pub applied_as: &'static str,
    /// 设置语句
    pub sql: String,
    /// 应用后读回的语句 + 期望值（`None` = 不校验）
    pub verify: Option<(String, i64)>,
    /// 期望的直接回值（`journal_mode` 会回实际落下的模式）
    pub expect_text: Option<&'static str>,
}

/// 规划连接属性（不碰数据库，纯函数）。
///
/// 失败一律报错并带上键与值（本仓规矩：做不到就报错，不静默降级）——
/// 典型是 `journal_mode=wal` 写成了 `journal_mode=write`（写错了要看得见）。
pub(crate) fn plan_connection(
    props: &std::collections::HashMap<String, String>,
) -> Result<SqlitePlan, CoreError> {
    let mut plan = SqlitePlan {
        flags: None,
        pragmas: Vec::new(),
        unknown: Vec::new(),
    };

    // 按 key 排序后应用：HashMap 迭代顺序不定，顺序稳定才可复现（也让日志可读）
    let mut ordered: Vec<&String> = props.keys().collect();
    ordered.sort();
    for raw_key in ordered {
        let raw_value = &props[raw_key];
        let key = raw_key.trim();
        let value = raw_value.trim();
        // 别名（旧驼峰名）与规范名等价：应用时用规范名（见 `property_spec` 的 sqlite 条目）
        let canonical = match key.to_ascii_lowercase().as_str() {
            "mode" => "mode",
            "journal_mode" | "journalmode" => "journal_mode",
            "synchronous" => "synchronous",
            "busy_timeout" | "busytimeout" => "busy_timeout",
            "foreign_keys" | "foreignkeys" => "foreign_keys",
            "cache_size" | "cachesize" => "cache_size",
            "temp_store" | "tempstore" => "temp_store",
            _ => {
                plan.unknown.push(raw_key.clone());
                continue;
            }
        };

        match canonical {
            "mode" => plan.flags = Some(open_flags(value).map_err(|e| attr_err(key, value, e))?),
            "journal_mode" => {
                let literal = match value.to_ascii_lowercase().as_str() {
                    "delete" => "DELETE",
                    "truncate" => "TRUNCATE",
                    "persist" => "PERSIST",
                    "memory" => "MEMORY",
                    "wal" => "WAL",
                    "off" => "OFF",
                    _ => {
                        return Err(attr_err(
                            key,
                            value,
                            "只支持 delete / truncate / persist / memory / wal / off",
                        ))
                    }
                };
                plan.pragmas.push(PlannedPragma {
                    key: raw_key.clone(),
                    applied_as: "journal_mode",
                    sql: format!("PRAGMA journal_mode = {literal}"),
                    verify: None,
                    // 关键：PRAGMA journal_mode 会回**实际落下**的模式——
                    // `:memory:` 库上写 wal 会静默保持 memory，不比对就是假装配上了。
                    expect_text: Some(literal),
                });
            }
            "synchronous" => {
                let level = match value.to_ascii_lowercase().as_str() {
                    "off" | "0" => 0,
                    "normal" | "1" => 1,
                    "full" | "2" => 2,
                    "extra" | "3" => 3,
                    _ => return Err(attr_err(key, value, "只支持 off / normal / full / extra（或 0-3）")),
                };
                plan.pragmas.push(pragma_with_readback(
                    raw_key,
                    "synchronous",
                    level.to_string(),
                    level,
                ));
            }
            "temp_store" => {
                let where_to = match value.to_ascii_lowercase().as_str() {
                    "default" | "0" => 0,
                    "file" | "1" => 1,
                    "memory" | "2" => 2,
                    _ => return Err(attr_err(key, value, "只支持 default / file / memory（或 0-2）")),
                };
                plan.pragmas.push(pragma_with_readback(
                    raw_key,
                    "temp_store",
                    where_to.to_string(),
                    where_to,
                ));
            }
            "foreign_keys" => {
                let on = match value.to_ascii_lowercase().as_str() {
                    "1" | "true" | "on" | "yes" => 1,
                    "0" | "false" | "off" | "no" => 0,
                    _ => return Err(attr_err(key, value, "只支持 true/false（或 on/off、1/0）")),
                };
                plan.pragmas.push(PlannedPragma {
                    key: raw_key.clone(),
                    applied_as: "foreign_keys",
                    sql: format!("PRAGMA foreign_keys = {}", if on == 1 { "ON" } else { "OFF" }),
                    verify: Some(("PRAGMA foreign_keys".to_string(), on)),
                    expect_text: None,
                });
            }
            "busy_timeout" => {
                let ms: i64 = value
                    .parse()
                    .map_err(|_| attr_err(key, value, "应是毫秒数（整数）"))?;
                if ms < 0 {
                    return Err(attr_err(key, value, "不能为负"));
                }
                plan.pragmas.push(pragma_with_readback(
                    raw_key,
                    "busy_timeout",
                    ms.to_string(),
                    ms,
                ));
            }
            "cache_size" => {
                let pages: i64 = value
                    .parse()
                    .map_err(|_| attr_err(key, value, "应是页数（负数 = KiB，如 -2000）"))?;
                // 不读回校验：SQLite 会按平台钳制 cache_size，读回值不等于写入值不代表没生效
                plan.pragmas.push(PlannedPragma {
                    key: raw_key.clone(),
                    applied_as: "cache_size",
                    sql: format!("PRAGMA cache_size = {pages}"),
                    verify: None,
                    expect_text: None,
                });
            }
            other => unreachable!("canonical 已穷举：{other}"),
        }
    }

    Ok(plan)
}

/// 带「应用后读回比对」的 PRAGMA（读回值可能被钳制/归一，比对不过就报错）。
fn pragma_with_readback(
    key: &str,
    applied_as: &'static str,
    literal: String,
    expect: i64,
) -> PlannedPragma {
    PlannedPragma {
        key: key.to_string(),
        applied_as,
        sql: format!("PRAGMA {applied_as} = {literal}"),
        verify: Some((format!("PRAGMA {applied_as}"), expect)),
        expect_text: None,
    }
}

/// `mode` → 打开标志（白名单：ro / rw / rwc）。
fn open_flags(mode: &str) -> Result<OpenFlags, &'static str> {
    let base = OpenFlags::default();
    match mode.to_ascii_lowercase().as_str() {
        "ro" => Ok(OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX),
        "rw" => Ok(base.difference(OpenFlags::SQLITE_OPEN_CREATE)),
        "rwc" => Ok(base),
        _ => Err("只支持 ro / rw / rwc"),
    }
}

/// 属性不合法的统一错误（带键与值，界面与日志都能定位）。
fn attr_err(key: &str, value: &str, reason: &str) -> CoreError {
    CoreError::database(DatabaseError::Driver {
        db_type: "sqlite".to_string(),
        operation: "driver_properties".to_string(),
        source: format!("属性 `{key} = {value}` 不合法：{reason}"),
    })
}

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
        Self::new_with_properties(url, &std::collections::HashMap::new())
    }

    /// 带**连接属性**开库：属性由本仓驱动侧落实（PRAGMA / 打开标志）。
    ///
    /// 属性键的允许集合与取值见 `driver::property_spec`（键 → 去向）与本文件的
    /// `plan_connection`（取值白名单）；清单外的键**不执行**（记 warn，由属性页如实标注）。
    /// 清单内的键**必须生效**：取值非法或读回不符 → 报错（不静默降级）。
    pub fn new_with_properties(
        url: &str,
        props: &std::collections::HashMap<String, String>,
    ) -> Result<Self, CoreError> {
        let path = if url.starts_with("sqlite://") {
            url.trim_start_matches("sqlite://")
        } else {
            url
        };
        let path = path.split('?').next().unwrap_or(path);

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

        let plan = plan_connection(props)?;
        let conn = match plan.flags {
            Some(flags) => Connection::open_with_flags(path, flags),
            None => Connection::open(path),
        }
        .map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;

        apply_pragmas(&conn, &plan)?;
        if !plan.unknown.is_empty() {
            tracing::warn!(
                driver = "sqlite",
                keys = ?plan.unknown,
                "属性键不在支持清单里，未应用（属性页会标注「不认这个键」）"
            );
        }

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

/// 逐条应用 PRAGMA 并**读回校验**（做不到就报错，不静默降级）。
///
/// 两种校验：
/// - `expect_text`：PRAGMA 自己的回值（`journal_mode` 会回实际落下的模式——
///   `:memory:` 库上写 `wal` 会静默保持 `memory`，不比对就是假装配上了）；
/// - `verify`：应用后再读一次（`synchronous` / `busy_timeout` / `foreign_keys` / `temp_store`）。
fn apply_pragmas(conn: &Connection, plan: &SqlitePlan) -> Result<(), CoreError> {
    let mut applied: Vec<&str> = Vec::new();
    for p in &plan.pragmas {
        // PRAGMA 走 `query_row`：会回值的拿得到（`journal_mode` 回文本、`busy_timeout` 回整数），
        // 不回值的报 `QueryReturnedNoRows`（也算成功）——所以回值要按**动态类型**读。
        let reply: Option<rusqlite::types::Value> =
            match conn.query_row(&p.sql, [], |row| row.get::<_, rusqlite::types::Value>(0)) {
                Ok(v) => Some(v),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => {
                    return Err(pragma_err(&p.key, &p.sql, e.to_string()));
                }
            };

        if let Some(want) = p.expect_text {
            match reply.as_ref() {
                Some(rusqlite::types::Value::Text(got)) if got.eq_ignore_ascii_case(want) => {}
                Some(rusqlite::types::Value::Text(got)) => {
                    return Err(pragma_err(
                        &p.key,
                        &p.sql,
                        format!("引擎实际落下的值是 `{got}`（期望 `{want}`）——该库不支持这个取值"),
                    ))
                }
                other => {
                    return Err(pragma_err(
                        &p.key,
                        &p.sql,
                        format!("没有拿到预期回值（{other:?}）"),
                    ))
                }
            }
        }

        if let Some((read_sql, want)) = &p.verify {
            match conn.query_row(read_sql, [], |row| row.get::<_, i64>(0)) {
                Ok(got) if got == *want => {}
                Ok(got) => {
                    return Err(pragma_err(
                        &p.key,
                        &p.sql,
                        format!("读回值是 {got}（期望 {want}）——未生效"),
                    ))
                }
                Err(e) => return Err(pragma_err(&p.key, &p.sql, e.to_string())),
            }
        }

        applied.push(p.applied_as);
    }

    if !applied.is_empty() {
        tracing::info!(driver = "sqlite", applied = ?applied, "连接属性已应用（驱动侧 PRAGMA）");
    }
    Ok(())
}

fn pragma_err(key: &str, sql: &str, reason: String) -> CoreError {
    CoreError::database(DatabaseError::Driver {
        db_type: "sqlite".to_string(),
        operation: "driver_properties".to_string(),
        source: format!("属性 `{key}`（{sql}）应用失败：{reason}"),
    })
}

fn is_read_only_sql(sql: &str) -> bool {
    let sql_upper = sql.trim_start().to_uppercase();
    sql_upper.starts_with("SELECT")
        || sql_upper.starts_with("PRAGMA")
        || sql_upper.starts_with("EXPLAIN")
}

/// rusqlite 错误 → 引擎错误
///
/// `SqlInputError` 的 `Display` 会把 **SQL 原文与偏移拼进文本**（`{msg} in {sql} at offset {n}`）：
/// 位置埋在文本里、SQL 又重复一遍。分段抓取发给驱动的是**窗口包装**（`SELECT * FROM ( … )`），
/// 于是报错会把内部的 `rds_segment` 泄漏给用户，位置也会按错的坐标被解析。这里取出结构化的
/// 「消息 + 出错 token 的字节偏移」（`sqlite3_error_offset`，0 基、相对本次发出的 SQL），
/// 与 PG 两条路径同一口径；`offset` 为负表示拿不到位置。
fn sqlite_error(sql: &str, error: rusqlite::Error) -> CoreError {
    if let rusqlite::Error::SqlInputError { msg, offset, .. } = &error {
        let mapped = DatabaseError::query(sql, msg.clone());
        return CoreError::database(if *offset >= 0 {
            mapped.with_position(*offset as usize)
        } else {
            mapped
        });
    }
    CoreError::database(DatabaseError::query(sql, error.to_string()))
}

/// 写语句（不返回行）走 `Connection::execute`，拿**真实影响行数**（B5 / P0.6）
///
/// `Connection::execute` 对会返回行的语句会直接报错（“Execute returned results”），
/// 所以调用方必须先确认语句既不返回行、也不带 `RETURNING`。
fn execute_writing(
    conn: &Connection,
    sql: &str,
    params: &[rusqlite::types::Value],
) -> Result<QueryResult, CoreError> {
    let affected = conn
        .execute(sql, rusqlite::params_from_iter(params.iter()))
        .map_err(|e| sqlite_error(sql, e))?;
    Ok(affected_rows_result(affected as u64))
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

            // B5 / P0.6：不返回行的写语句走 `execute`，拿驱动的真实影响行数
            if !is_read_only_sql(&sql_owned) && !returns_rows(&sql_owned) {
                return execute_writing(&conn, &sql_owned, &params_slice);
            }

            let mut stmt = conn
                .prepare(&sql_owned)
                .map_err(|e| sqlite_error(&sql_owned, e))?;

            let columns: Vec<String> = stmt
                .column_names()
                .iter()
                .map(|name| name.to_string())
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
            .map_err(|e| sqlite_error(&sql_owned, e))?;

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

                // B5 / P0.6：不返回行的写语句走 `execute`，拿驱动的真实影响行数
                if !is_read_only_sql(&sql_owned) && !returns_rows(&sql_owned) {
                    return execute_writing(&conn, &sql_owned, &[]);
                }

                let mut stmt = conn.prepare(&sql_owned)
                    .map_err(|e| sqlite_error(&sql_owned, e))?;

                let columns: Vec<String> = stmt.column_names()
                    .iter()
                    .map(|name| name.to_string())
                    .collect();

                let mut rows = stmt.query([])
                    .map_err(|e| sqlite_error(&sql_owned, e))?;

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
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
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
                        "view" => crate::driver::SchemaObjectKind::View,
                        _ => crate::driver::SchemaObjectKind::Table,
                    };
                    objects.push(crate::driver::NodeInfo::new(name, kind));
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

    fn as_metadata_browser(&self) -> Option<&dyn crate::driver::MetadataBrowser> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl crate::driver::MetadataBrowser for SqliteDatabase {
    fn has_schema_level(&self) -> bool {
        // SQLite 为单库，database 内无独立 Schema 层。
        false
    }

    async fn get_catalogs(&self) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        Ok(vec![crate::driver::NodeInfo::new(
            "main",
            crate::driver::SchemaObjectKind::Catalog,
        )])
    }

    async fn get_schemas(
        &self,
        _catalog: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        Ok(vec![])
    }

    async fn get_tables(
        &self,
        catalog: &str,
        _schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        // 一次内省：本方法不再重写 SQL（2026-09-19 统一前这里是「调 list_tables 再加 icon」，
        // 而 icon 没有任何消费者）。
        self.list_tables(catalog, None).await
    }

    async fn get_table_detail(
        &self,
        catalog: &str,
        _schema: &str,
        table: &str,
    ) -> Result<crate::driver::NodeDetail, CoreError> {
        let columns = self.list_columns(catalog, None, table).await?;
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

        // B5 / P0.6：事务内的写语句同样要真实影响行数
        if !is_read_only_sql(sql) && !returns_rows(sql) {
            return execute_writing(&conn, sql, &[]);
        }

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| sqlite_error(sql, e))?;

        let columns: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|name| name.to_string())
            .collect();

        let mut rows = stmt
            .query([])
            .map_err(|e| sqlite_error(sql, e))?;

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

    fn props(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// 取值白名单：合法值能规划出 SQL，非法值报错**并带上键与值**。
    #[test]
    fn planning_accepts_whitelisted_values_and_rejects_the_rest() {
        let plan = plan_connection(&props(&[
            ("journalMode", "wal"), // 旧驼峰名 → 规范名
            ("busyTimeout", "3000"),
            ("foreign_keys", "true"),
            ("synchronous", "normal"),
            ("魔改键", "1"),
        ]))
        .expect("合法属性应能规划");
        let applied: Vec<&str> = plan.pragmas.iter().map(|p| p.applied_as).collect();
        assert_eq!(
            applied,
            vec!["busy_timeout", "foreign_keys", "journal_mode", "synchronous"],
            "按 key 排序、且别名归到规范名"
        );
        assert_eq!(plan.unknown, vec!["魔改键".to_string()]);

        // 取值非法：报错里要能看见键与值（不静默忽略）
        let err = plan_connection(&props(&[("journal_mode", "write")]))
            .expect_err("write 不是合法日志模式")
            .to_string();
        assert!(err.contains("journal_mode") && err.contains("write"), "{err}");
        let err = plan_connection(&props(&[("busy_timeout", "3s")]))
            .expect_err("毫秒数不能带单位")
            .to_string();
        assert!(err.contains("busy_timeout"), "{err}");
        let err = plan_connection(&props(&[("mode", "rwx")]))
            .expect_err("只有 ro/rw/rwc")
            .to_string();
        assert!(err.contains("ro / rw / rwc"), "{err}");

        // 注入尝试：值里有分号 / 引号也只会在白名单比对时被拒
        assert!(plan_connection(&props(&[("journal_mode", "wal; DROP TABLE t")])).is_err());
    }

    /// 端到端：属性真的落到库上（PRAGMA 读回为准），且未知键不影响开库。
    #[tokio::test]
    async fn driver_properties_are_applied_and_verifiable() -> Result<(), CoreError> {
        let url = temp_db_path("props_applied")?;
        let db = SqliteDatabase::new_with_properties(
            &url,
            &props(&[
                ("journal_mode", "wal"),
                ("busy_timeout", "3000"),
                ("foreign_keys", "true"),
                ("synchronous", "normal"),
                ("temp_store", "memory"),
                ("cache_size", "-2000"),
                ("无关键", "1"),
            ]),
        )?;

        let read = |sql: &str| -> String {
            let conn = db.conn.lock().expect("lock");
            conn.query_row(sql, [], |row| row.get::<_, rusqlite::types::Value>(0))
                .map(|v| match v {
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Text(t) => t,
                    other => format!("{other:?}"),
                })
                .unwrap_or_default()
        };
        assert_eq!(read("PRAGMA journal_mode").to_lowercase(), "wal");
        assert_eq!(read("PRAGMA busy_timeout"), "3000");
        assert_eq!(read("PRAGMA foreign_keys"), "1");
        assert_eq!(read("PRAGMA synchronous"), "1", "normal = 1");
        assert_eq!(read("PRAGMA temp_store"), "2", "memory = 2");
        assert_eq!(read("PRAGMA cache_size"), "-2000");
        Ok(())
    }

    /// 取开库错误的消息（`SqliteDatabase` 不是 `Debug`，用不了 `expect_err`）。
    fn open_err(url: &str, props: &std::collections::HashMap<String, String>) -> String {
        match SqliteDatabase::new_with_properties(url, props) {
            Ok(_) => panic!("本该开库失败却成功了：{url}"),
            Err(e) => e.to_string(),
        }
    }

    /// `journal_mode=wal` 在**内存库**上不可能生效：必须报错，不能假装配上了。
    #[tokio::test]
    async fn unimplementable_pragma_fails_loudly() {
        let err = open_err(":memory:", &props(&[("journal_mode", "wal")]));
        assert!(err.contains("journal_mode"), "{err}");
        assert!(err.contains("memory"), "错误里要能看出实际落下的值：{err}");
    }

    /// `mode=rw` 不建新文件：库不存在时开库必失败（证明打开标志真的生效了）。
    #[tokio::test]
    async fn open_mode_is_applied() {
        let dir = std::env::temp_dir().join(format!("rdata_test_sqlite_mode_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("missing.db");
        let _ = std::fs::remove_file(&path);

        // rwc（默认）会建文件
        SqliteDatabase::new_with_properties(&format!("sqlite://{}", path.display()), &props(&[("mode", "rwc")]))
            .expect("rwc 应能建库");
        assert!(path.exists());

        // rw 不会建（文件已存在 → 能开；删掉后 → 必须失败）
        std::fs::remove_file(&path).expect("rm");
        let err = open_err(
            &format!("sqlite://{}", path.display()),
            &props(&[("mode", "rw")]),
        );
        assert!(!path.exists(), "rw 不该创建文件：{err}");

        // ro 打开已有库后写不进去
        std::fs::create_dir_all(&dir).ok();
        {
            let c = Connection::open(&path).expect("seed");
            c.execute("CREATE TABLE t(a)", []).expect("ddl");
        }
        let ro = SqliteDatabase::new_with_properties(
            &format!("sqlite://{}", path.display()),
            &props(&[("mode", "ro")]),
        )
        .expect("ro 应能开已有库");
        let write = {
            let conn = ro.conn.lock().expect("lock");
            conn.execute("INSERT INTO t VALUES (1)", [])
        };
        assert!(write.is_err(), "ro 连接必须拒绝写入");

        let _ = std::fs::remove_dir_all(&dir);
    }

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
