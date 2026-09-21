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

use arrow::array::StringArray;
use duckdb::{AccessMode, Config, Connection};

use crate::driver::traits::MetadataBrowser;
use crate::driver::utils::{affected_rows_result, returns_rows};
use crate::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, IndexDetail, Transaction,
};
use shared::error::{CoreError, DatabaseError};
use shared::models::{QueryResult, Value};
use crate::duckdb::row_to_arrow::duckdb_rows_to_arrow;

/// 连接属性 → 开库动作（纯函数产物，可测）。
///
/// 键与取值都**白名单校验**后才拼成常量 SQL：属性值来自用户输入，直接拼串就是注入口。
/// 允许的键就是 [`crate::driver::property_spec`] 里 duckdb 条目声明的那些。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DuckDbPlan {
    /// 开库时定的模式（`access_mode`，运行期改不了）
    pub access_mode: Option<AccessMode>,
    /// 开库后逐条执行的 `SET`（用户值排在 `DuckDBManager::configure_connection` 之后 → 用户赢）
    pub settings: Vec<PlannedSetting>,
    /// 清单外的键：不执行（调用方记 warn）
    pub unknown: Vec<String>,
}

/// 一条待执行的 `SET`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedSetting {
    /// 用户写的键（含别名）
    pub key: String,
    /// 规范名（DuckDB 认的设置名）
    pub applied_as: &'static str,
    /// 设置语句
    pub sql: String,
    /// 应用后读回比对的值（字符串比对；`None` = 不校验）
    pub verify: Option<String>,
}

/// 规划连接属性（不碰数据库，纯函数）。
///
/// 取值非法 → 报错带键与值；清单外的键 → 记入 `unknown`（不执行）。
pub(crate) fn plan_connection(
    props: &std::collections::HashMap<String, String>,
) -> Result<DuckDbPlan, CoreError> {
    let mut plan = DuckDbPlan {
        access_mode: None,
        settings: Vec::new(),
        unknown: Vec::new(),
    };

    // 按 key 排序后应用：HashMap 迭代顺序不定，顺序稳定才可复现（也让日志可读）
    let mut ordered: Vec<&String> = props.keys().collect();
    ordered.sort();
    for raw_key in ordered {
        let raw_value = &props[raw_key];
        let key = raw_key.trim();
        let value = raw_value.trim();
        match key.to_ascii_lowercase().as_str() {
            "access_mode" | "accessmode" => {
                let mode = match value.to_ascii_lowercase().as_str() {
                    "automatic" | "auto" => AccessMode::Automatic,
                    "read_only" | "readonly" | "ro" => AccessMode::ReadOnly,
                    "read_write" | "readwrite" | "rw" => AccessMode::ReadWrite,
                    _ => {
                        return Err(attr_err(
                            key,
                            value,
                            "只支持 automatic / read_only / read_write",
                        ))
                    }
                };
                plan.access_mode = Some(mode);
            }
            "threads" => {
                let n: i64 = value.parse().map_err(|_| {
                    attr_err(key, value, "应是线程数（正整数）")
                })?;
                if n < 1 {
                    return Err(attr_err(key, value, "至少为 1"));
                }
                plan.settings.push(set_number(raw_key, "threads", n));
            }
            "memory_limit" | "memorylimit" => {
                let size = size_literal(value).map_err(|e| attr_err(key, value, e))?;
                plan.settings.push(PlannedSetting {
                    key: raw_key.clone(),
                    applied_as: "memory_limit",
                    sql: format!("SET memory_limit = '{size}'"),
                    // 读回是归一化后的文本（如 `256.0 MiB`），只校验“非空且不报错”
                    verify: None,
                });
            }
            "max_temp_directory_size" | "maxtempdirectorysize" => {
                let size = size_literal(value).map_err(|e| attr_err(key, value, e))?;
                plan.settings.push(PlannedSetting {
                    key: raw_key.clone(),
                    applied_as: "max_temp_directory_size",
                    sql: format!("SET max_temp_directory_size = '{size}'"),
                    verify: None,
                });
            }
            "temp_directory" | "tempdirectory" => {
                let dir = directory_literal(value).map_err(|e| attr_err(key, value, e))?;
                plan.settings.push(PlannedSetting {
                    key: raw_key.clone(),
                    applied_as: "temp_directory",
                    sql: format!("SET temp_directory = '{dir}'"),
                    verify: Some(value.to_string()),
                });
            }
            "preserve_insertion_order" | "preserveinsertionorder" => {
                let on = match value.to_ascii_lowercase().as_str() {
                    "1" | "true" | "on" | "yes" => true,
                    "0" | "false" | "off" | "no" => false,
                    _ => return Err(attr_err(key, value, "只支持 true/false（或 on/off、1/0）")),
                };
                plan.settings.push(PlannedSetting {
                    key: raw_key.clone(),
                    applied_as: "preserve_insertion_order",
                    sql: format!("SET preserve_insertion_order = {on}"),
                    verify: Some(if on { "true" } else { "false" }.to_string()),
                });
            }
            _ => plan.unknown.push(raw_key.clone()),
        }
    }

    Ok(plan)
}

fn set_number(key: &str, applied_as: &'static str, n: i64) -> PlannedSetting {
    PlannedSetting {
        key: key.to_string(),
        applied_as,
        sql: format!("SET {applied_as} = {n}"),
        verify: Some(n.to_string()),
    }
}

/// 尺寸字面量白名单（如 `1GB` / `512MB` / `1024`）——防止任意文本进 SQL。
fn size_literal(value: &str) -> Result<String, &'static str> {
    let v = value.trim();
    if v.is_empty() {
        return Err("不能为空");
    }
    let (num, unit) = v.split_at(
        v.find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(v.len()),
    );
    let ok_num = !num.is_empty()
        && num
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
        && num.chars().any(|c| c.is_ascii_digit());
    let ok_unit = matches!(
        unit.to_ascii_uppercase().as_str(),
        "" | "B" | "KB" | "MB" | "GB" | "TB" | "KIB" | "MIB" | "GIB" | "TIB"
    );
    if !ok_num || !ok_unit {
        return Err("应形如 1GB / 512MB / 1024");
    }
    Ok(v.to_string())
}

/// 目录字面量：转义单引号并**拒绝分号**（单语句执行下也不给拼接留余地）。
fn directory_literal(value: &str) -> Result<String, &'static str> {
    let v = value.trim();
    if v.is_empty() {
        return Err("不能为空");
    }
    if v.contains(';') {
        return Err("不能含分号");
    }
    if v.contains('\'') {
        return Err("不能含单引号");
    }
    Ok(v.to_string())
}

/// 属性不合法的统一错误（带键与值）。
fn attr_err(key: &str, value: &str, reason: &str) -> CoreError {
    CoreError::database(DatabaseError::Driver {
        db_type: "duckdb".to_string(),
        operation: "driver_properties".to_string(),
        source: format!("属性 `{key} = {value}` 不合法：{reason}"),
    })
}

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
        Self::new_with_properties(url, &std::collections::HashMap::new())
    }

    /// 带**连接属性**开库：属性由本仓驱动侧落实（开库配置 + `SET`）。
    ///
    /// 应用顺序有意如此：先按 `access_mode` 开库（运行期改不了）→ 跑
    /// `DuckDBManager::configure_connection`（扩展目录 / Secret 目录 / 内存闸等应用默认）→
    /// **最后**逐条 `SET` 用户的属性（所以 `memory_limit` / `temp_directory` 这类键能覆盖应用默认，
    /// 这正是属性页标注“会覆盖”的含义）。
    /// 清单内的键必须生效：取值非法或读回不符 → 报错（不静默降级）。
    pub fn new_with_properties(
        url: &str,
        props: &std::collections::HashMap<String, String>,
    ) -> Result<Self, CoreError> {
        let path = if url.starts_with("duckdb://") {
            url.trim_start_matches("duckdb://")
        } else {
            url
        };
        let path = path.split('?').next().unwrap_or(path);

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

        let plan = plan_connection(props)?;
        // `AccessMode` 不是 Copy：先取一份给开库配置，`plan` 后续还要用（applied 日志 / 未知键）
        let open_mode = plan.access_mode.clone();
        let conn = match open_mode {
            Some(mode) => {
                let described = mode.to_string();
                let config = Config::default().access_mode(mode).map_err(|e| {
                    attr_err("access_mode", &described, &e.to_string())
                })?;
                Connection::open_with_flags(path, config)
            }
            None => Connection::open(path),
        }
        .map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        // 与其它 DuckDB 连接同一套纪律（扩展目录 / 内存闸 / 溢写口 / 不静默联网）：
        // 不配的话，用户 SQL 里的 `read_parquet` 会把扩展静默下到 `~/.duckdb`（C 盘）
        crate::duckdb::DuckDBManager::configure_connection(&conn).map_err(|error| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "configure".to_string(),
                source: error.to_string(),
            })
        })?;
        // 用户属性排在应用默认之后：显式写的值赢（见函数头注释）
        apply_settings(&conn, &plan)?;
        if !plan.unknown.is_empty() {
            tracing::warn!(
                driver = "duckdb",
                keys = ?plan.unknown,
                "属性键不在支持清单里，未应用（属性页会标注「不认这个键」）"
            );
        }
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

/// 逐条应用 `SET` 并**读回校验**（做不到就报错，不静默降级）。
///
/// 读回用 `current_setting('名字')`：DuckDB 会把值归一化（`256MB` → `256.0 MiB`），
/// 所以只对**能精确比对**的键（整数 / 布尔 / 目录路径）校验；尺寸类不比对（写过即认）。
fn apply_settings(conn: &Connection, plan: &DuckDbPlan) -> Result<(), CoreError> {
    let mut applied: Vec<&str> = Vec::new();
    for s in &plan.settings {
        conn.execute(&s.sql, [])
            .map_err(|e| setting_err(&s.key, &s.sql, e.to_string()))?;

        if let Some(want) = &s.verify {
            // `current_setting` 的原生类型五花八门（threads → 整数、preserve_insertion_order → 布尔），
            // 统一 `CAST(... AS VARCHAR)` 再比对。
            let got = conn
                .query_row(
                    &format!(
                        "SELECT CAST(current_setting('{}') AS VARCHAR)",
                        s.applied_as
                    ),
                    [],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|e| setting_err(&s.key, &s.sql, e.to_string()))?;
            if !got.eq_ignore_ascii_case(want) {
                return Err(setting_err(
                    &s.key,
                    &s.sql,
                    format!("读回值是 `{got}`（期望 `{want}`）——未生效"),
                ));
            }
        }
        applied.push(s.applied_as);
    }

    if plan.access_mode.is_some() {
        applied.push("access_mode");
    }
    if !applied.is_empty() {
        tracing::info!(driver = "duckdb", applied = ?applied, "连接属性已应用（驱动侧 SET）");
    }
    Ok(())
}

fn setting_err(key: &str, sql: &str, reason: String) -> CoreError {
    CoreError::database(DatabaseError::Driver {
        db_type: "duckdb".to_string(),
        operation: "driver_properties".to_string(),
        source: format!("属性 `{key}`（{sql}）应用失败：{reason}"),
    })
}

/// DuckDB 的列表列以**文本**返回（`[a, b]` / `[]`）：拆方括号、去空白与可选引号。
///
/// 用于 `duckdb_constraints()` 的 `constraint_column_names` / `referenced_column_names`
/// —— 它们不是分隔字符串，而是列表的文本表示。
fn parse_duckdb_list(text: &str) -> Vec<String> {
    text.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches('"').to_string())
        .collect()
}

fn is_read_only_sql(sql: &str) -> bool {
    let sql_upper = sql.trim_start().to_uppercase();
    sql_upper.starts_with("SELECT")
        || sql_upper.starts_with("SHOW")
        || sql_upper.starts_with("DESCRIBE")
        || sql_upper.starts_with("EXPLAIN")
        || sql_upper.starts_with("PRAGMA")
}

/// 写语句（不返回行）走 `Statement::execute`，拿**真实影响行数**（B5 / P0.6）
///
/// `execute` 对会返回行的语句会直接报错（要调 `query`），所以调用方必须先确认
/// 语句既不返回行、也不带 `RETURNING`。
fn execute_writing(conn: &Connection, sql: &str) -> Result<QueryResult, CoreError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;
    let affected = stmt
        .execute([])
        .map_err(|e| CoreError::database(DatabaseError::query(sql, e.to_string())))?;
    Ok(affected_rows_result(affected as u64))
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

            // B5 / P0.6：不返回行的写语句走 `execute`，拿驱动的真实影响行数
            if !is_read_only_sql(&sql_owned) && !returns_rows(&sql_owned) {
                return execute_writing(&conn, &sql_owned);
            }

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

            let is_read_only = is_read_only_sql(&sql_owned);
            let row_count = row_data.len();

            let batch = if row_count > 0 {
                duckdb_rows_to_arrow(&columns, &row_data)?
            } else {
                return Ok(QueryResult {
                    columns,
                    batches: vec![],
                    is_read_only: Some(is_read_only),
                    ..Default::default()
                });
            };

            Ok(QueryResult {
                columns,
                batches: vec![batch],
                is_read_only: Some(is_read_only),
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

            // B5 / P0.6：带参数的写语句同样给真实影响行数
            if !is_read_only_sql(&sql_owned) && !returns_rows(&sql_owned) {
                let affected = stmt
                    .execute(params_slice.as_slice())
                    .map_err(|e| CoreError::database(DatabaseError::query(&sql_owned, e.to_string())))?;
                return Ok(affected_rows_result(affected as u64));
            }

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
                    // 行自己知道有几列（与执行前无关）
                    for i in 0..row.as_ref().column_count() {
                        match row.get::<usize, duckdb::types::Value>(i) {
                            Ok(v) => values.push(v),
                            Err(_) => values.push(duckdb::types::Value::Null),
                        }
                    }
                    data.push(values);
                }
                row_data = data;
            }

            // **列名要等执行完再取**：duckdb-rs 的 `column_count` / `column_name` 读的是
            // 「执行结果」上的 schema，执行前调用直接 panic（`The statement was not executed
            // yet`）—— 这一步以前排在 `stmt.query()` 之前，等于这条参数化查询路径一跑就炸
            // （`query()` 里是执行后才取的，所以从没暴露）。
            let column_count = if let Some(first) = row_data.first() {
                first.len()
            } else {
                stmt.column_count()
            };
            let columns: Vec<String> = (0..column_count)
                .map(|i| {
                    stmt.column_name(i)
                        .map(|n| n.to_string())
                        .unwrap_or_else(|_| format!("column_{}", i))
                })
                .collect();

            let is_read_only = is_read_only_sql(&sql_owned);
            let row_count = row_data.len();

            let batch = if row_count > 0 {
                duckdb_rows_to_arrow(&columns, &row_data)?
            } else {
                return Ok(QueryResult {
                    columns,
                    batches: vec![],
                    is_read_only: Some(is_read_only),
                    ..Default::default()
                });
            };

            Ok(QueryResult {
                columns,
                batches: vec![batch],
                is_read_only: Some(is_read_only),
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

                // B5 / P0.6：不返回行的写语句走 `execute`，拿驱动的真实影响行数
                if !is_read_only_sql(&sql_owned) && !returns_rows(&sql_owned) {
                    return execute_writing(&conn, &sql_owned);
                }

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

                let is_read_only = is_read_only_sql(&sql_owned);
                let row_count = row_data.len();

                let batch = if row_count > 0 {
                    duckdb_rows_to_arrow(&columns, &row_data)?
                } else {
                    return Ok(QueryResult {
                        columns,
                        batches: vec![],
                        is_read_only: Some(is_read_only),
                        ..Default::default()
                    });
                };

                Ok(QueryResult {
                    columns,
                    batches: vec![batch],
                    is_read_only: Some(is_read_only),
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
        conn.query_row("SELECT 1", [], |r| r.get::<_, i32>(0))
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
        // 与浏览器同一份实现：单库场景下 catalog / schema 都是 `main`。
        self.get_tables("main", "main").await
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
    /// DuckDB 通过 `duckdb_indexes()` 表函数获取索引。
    ///
    /// **2026-09-21 真机修正**：原实现查 `index_type` 与 `expression` 两列，而当前
    /// DuckDB（1.5.5）的 `duckdb_indexes()` **两列都不存在**（真实列是 `expressions`，
    /// 复数，且没有索引方法列）—— 于是查询每次都以
    /// `Binder Error: Referenced column "index_type" not found in FROM clause` 失败，
    /// 而属性面板把 `get_indexes` 的错误吞成空 → **DuckDB 的「索引」分区恒为空白，
    /// 且不留任何痕迹**。这类"错在 SQL 里、被上层静默吞掉"的缺陷只有真机套件能抓。
    ///
    /// `expressions` 是**列表**，必须在 SQL 里 `array_to_string`（duckdb-rs 的
    /// `row.get::<String>` 对 `List` 列直接报 `Invalid column type List`）。
    ///
    /// 注：DuckDB 的**主键 / 唯一**走 `duckdb_constraints()`，不在 `duckdb_indexes()` 里
    /// —— 所以这里列出的就是用户显式建的索引。
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

        // duckdb_indexes() 的真实列（1.5.5 实测）：index_name, is_unique, is_primary,
        // expressions（列表）…；**没有** index_type / expression
        let mut stmt = conn
            .prepare(
                "SELECT index_name, is_unique, is_primary,
                        COALESCE(array_to_string(expressions, ','), '')
                   FROM duckdb_indexes()
                  WHERE table_name = ?
                  ORDER BY index_name",
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
                    row.get::<_, Option<String>>(3)?, // expressions（已 array_to_string）
                ))
            })
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_indexes", e.to_string()))
            })?;

        let mut indexes = Vec::new();
        for row in rows {
            match row {
                Ok((name, is_unique, is_primary, expressions)) => {
                    // `expressions` 已经是 `col1,col2` 形态（`array_to_string` 的产物）。
                    // 索引表达式（如 `lower(name)`）原样保留 —— 那是用户写的东西，
                    // 不要在这里"洗"成列名。
                    let column_names: Vec<String> = expressions
                        .as_deref()
                        .map(|e| {
                            e.split(',')
                                .map(|s| s.trim().trim_matches('"').to_string())
                                .filter(|s| !s.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();

                    indexes.push(IndexDetail {
                        name,
                        table_name: table.to_string(),
                        column_names,
                        is_unique,
                        is_primary,
                        // 索引方法（btree / art…）当前 DuckDB 不暴露，如实留空
                        index_type: None,
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

    /// 列举约束（`duckdb_constraints()`）。
    ///
    /// 为什么必须补：本方法此前**没实现**，而 `MetadataBrowser::get_constraints` 是
    /// `self.list_constraints(...)` 的纯转发 → 落到 trait 默认空实现 →
    /// **属性面板的「约束」分区恒为空**（与已修的「序列 / 触发器被空实现遮蔽」同一形态）。
    ///
    /// DuckDB 有现成的 `duckdb_constraints()`，而且比 `information_schema` 更全：
    /// `constraint_name`（真名字，如 `t_id_pkey`）、`referenced_table` /
    /// `referenced_column_names`（外键目标）都是它独有的。
    ///
    /// 两点要注意：
    /// * 列表列（`constraint_column_names` / `referenced_column_names`）**以文本返回**
    ///   （`[a, b]` / `[]`），不是分隔字符串，所以要拆方括号；
    /// * `duckdb_constraints()` 把 `NOT NULL` 也算一类约束 —— **有意过滤掉**：
    ///   属性面板「列」分区里已有「非空」一列，再来一行约束是重复噪音。
    async fn list_constraints(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        let conn = self.conn.lock().map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "duckdb".to_string(),
                operation: "lock".to_string(),
                source: e.to_string(),
            })
        })?;

        let mut stmt = conn
            .prepare(
                // 列表列**必须在 SQL 里转成文本**：duckdb-rs 的 `row.get::<String>` 对
                // `List` 类型的列会直接报 `Invalid column type List`（探针实测）。
                // `array_to_string` 比 `CAST(... AS VARCHAR)` 更干净（不用再拆方括号）。
                "SELECT constraint_name, constraint_type,
                        COALESCE(array_to_string(constraint_column_names, ','), ''),
                        COALESCE(referenced_table, ''),
                        COALESCE(array_to_string(referenced_column_names, ','), '')
                   FROM duckdb_constraints()
                  WHERE table_name = ? AND constraint_type <> 'NOT NULL'
                  ORDER BY constraint_index",
            )
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_constraints", e.to_string()))
            })?;

        let rows = stmt
            .query_map([table], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?, // constraint_name
                    row.get::<_, String>(1)?,         // constraint_type
                    row.get::<_, Option<String>>(2)?, // constraint_column_names（已 array_to_string）
                    row.get::<_, String>(3)?,         // referenced_table
                    row.get::<_, Option<String>>(4)?, // referenced_column_names（已 array_to_string）
                ))
            })
            .map_err(|e| {
                CoreError::database(DatabaseError::query("list_constraints", e.to_string()))
            })?;

        let mut out: Vec<ConstraintDetail> = Vec::new();
        for row in rows {
            let (name, kind, cols, ref_table, ref_cols) = row.map_err(|e| {
                CoreError::database(DatabaseError::query("list_constraints_row", e.to_string()))
            })?;
            out.push(ConstraintDetail {
                name: name.unwrap_or_default(),
                table_name: table.to_string(),
                constraint_type: kind,
                column_names: parse_duckdb_list(&cols.unwrap_or_default()),
                referenced_table: (!ref_table.is_empty()).then_some(ref_table),
                referenced_columns: parse_duckdb_list(&ref_cols.unwrap_or_default()),
                // DuckDB 的外键规则目前不在 duckdb_constraints() 里 —— 如实留空，
                // 不猜一个 "NO ACTION" 出来
                update_rule: None,
                delete_rule: None,
            });
        }
        Ok(out)
    }

    fn as_metadata_browser(&self) -> Option<&dyn crate::driver::MetadataBrowser> {
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

        // B5 / P0.6：事务内的写语句同样要真实影响行数
        if !is_read_only_sql(sql) && !returns_rows(sql) {
            return execute_writing(&conn, sql);
        }

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
        let is_read_only = sql_upper.starts_with("SELECT")
            || sql_upper.starts_with("SHOW")
            || sql_upper.starts_with("DESCRIBE");
        let row_count = row_data.len();

        let batch = if row_count > 0 {
            duckdb_rows_to_arrow(&columns, &row_data)?
        } else {
            return Ok(QueryResult {
                columns,
                batches: vec![],
                is_read_only: Some(is_read_only),
                ..Default::default()
            });
        };

        Ok(QueryResult {
            columns,
            batches: vec![batch],
            is_read_only: Some(is_read_only),
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



#[async_trait::async_trait]
impl crate::driver::MetadataBrowser for DuckDbDatabase {
    fn has_schema_level(&self) -> bool {
        // 当前内省按固定 `main` schema 取表，未按传入 schema 区分；
        // 单库场景下跳过 Schema 层，避免 `main → main` 重复层。
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
        // DuckDB 不区分 catalog/schema 层级（见 `has_schema_level`）。
        Ok(vec![])
    }

    async fn get_tables(
        &self,
        _catalog: &str,
        _schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let result = self.query("SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = 'main' ORDER BY table_name").await?;
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
                            Some(crate::driver::NodeInfo::new(
                                name_arr.value(row_idx).to_string(),
                                kind,
                            ))
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
    ) -> Result<crate::driver::NodeDetail, CoreError> {
        // 列序契约见 `utils::PG_TABLE_DETAIL_SQL` 的文档；表名走参数绑定（不拼串）。
        let result = self
            .query_with_params(
                crate::driver::utils::DUCK_TABLE_DETAIL_SQL,
                vec![Value::Text(table.to_string())],
            )
            .await?;
        let columns = crate::driver::utils::columns_from_detail_rows(&result);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Database;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn props(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// 取开库错误的消息（`DuckDbDatabase` 不是 `Debug`，用不了 `expect_err`）。
    fn open_err(url: &str, props: &std::collections::HashMap<String, String>) -> String {
        match DuckDbDatabase::new_with_properties(url, props) {
            Ok(_) => panic!("本该开库失败却成功了：{url}"),
            Err(e) => e.to_string(),
        }
    }

    /// 取值白名单：合法值能规划出 `SET`，非法值报错并带上键与值。
    #[test]
    fn planning_accepts_whitelisted_values_and_rejects_the_rest() {
        let plan = plan_connection(&props(&[
            ("memoryLimit", "256MB"), // 旧驼峰名 → 规范名
            ("threads", "2"),
            ("preserveInsertionOrder", "false"),
            ("accessMode", "read_only"),
            ("无关键", "1"),
        ]))
        .expect("合法属性应能规划");
        let applied: Vec<&str> = plan.settings.iter().map(|s| s.applied_as).collect();
        assert_eq!(
            applied,
            vec!["memory_limit", "preserve_insertion_order", "threads"],
            "按 key 排序、别名归到规范名"
        );
        assert_eq!(plan.access_mode, Some(AccessMode::ReadOnly));
        assert_eq!(plan.unknown, vec!["无关键".to_string()]);

        // 非法值：报错要能看见键与值
        let err = plan_connection(&props(&[("threads", "many")]))
            .expect_err("线程数必须是整数")
            .to_string();
        assert!(err.contains("threads") && err.contains("many"), "{err}");
        let err = plan_connection(&props(&[("memory_limit", "lots")]))
            .expect_err("尺寸字面量必须白名单")
            .to_string();
        assert!(err.contains("1GB"), "{err}");
        let err = plan_connection(&props(&[("temp_directory", "C:\\tmp'; DROP TABLE t")]))
            .expect_err("目录不能带引号 / 分号")
            .to_string();
        assert!(err.contains("temp_directory"), "{err}");
        assert!(plan_connection(&props(&[("access_mode", "maybe")])).is_err());
    }

    /// 端到端：`SET` 真的落了（读回为准），且未知键不影响开库。
    #[tokio::test]
    async fn driver_properties_are_applied_and_verifiable() -> Result<(), CoreError> {
        let dir = std::env::temp_dir().join(format!(
            "rd_duckdb_props_{}",
            TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).ok();
        let spill = dir.join("spill");
        std::fs::create_dir_all(&spill).ok();

        let db = DuckDbDatabase::new_with_properties(
            &dir.join("props.duckdb").to_string_lossy(),
            &props(&[
                ("threads", "2"),
                ("temp_directory", &spill.to_string_lossy()),
                ("preserve_insertion_order", "false"),
                ("memory_limit", "256MB"),
                ("无关键", "1"),
            ]),
        )?;

        let read = |name: &str| -> String {
            let conn = db.conn.lock().expect("lock");
            conn.query_row(
                &format!("SELECT CAST(current_setting('{name}') AS VARCHAR)"),
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default()
        };
        assert_eq!(read("threads"), "2", "用户写的线程数应胜过应用默认");
        assert_eq!(read("preserve_insertion_order"), "false");
        assert_eq!(
            read("temp_directory"),
            spill.to_string_lossy().to_string(),
            "用户写的溢写目录应胜过应用默认"
        );
        assert!(
            read("memory_limit").to_lowercase().contains("mi"),
            "内存上限应已设置：{}",
            read("memory_limit")
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// `access_mode=read_only` 在开库时定：只读连接写不进去（证明它真的生效）。
    #[tokio::test]
    async fn read_only_access_mode_rejects_writes() -> Result<(), CoreError> {
        let dir = std::env::temp_dir().join(format!(
            "rd_duckdb_ro_{}",
            TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("ro.duckdb");
        let path_str = path.to_string_lossy().to_string();
        {
            let seed = DuckDbDatabase::new(&path_str)?;
            seed.query("CREATE TABLE t AS SELECT 1 AS a").await?;
        }

        let ro = DuckDbDatabase::new_with_properties(
            &path_str,
            &props(&[("access_mode", "read_only")]),
        )?;
        let ok = ro.query("SELECT a FROM t").await;
        assert!(ok.is_ok(), "只读连接应能查询：{ok:?}");
        let write = ro.query("INSERT INTO t VALUES (2)").await;
        assert!(write.is_err(), "只读连接必须拒绝写入：{write:?}");

        // 非法取值不静默忽略
        let err = open_err(&path_str, &props(&[("access_mode", "maybe")]));
        assert!(err.contains("access_mode"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

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
