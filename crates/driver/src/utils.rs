use super::registry::DriverConnectionConfig;
use shared::error::{ConnectionError, CoreError};
use shared::models::QueryResult;

// `sqlx_cell_as_text!` / `sqlx_value_is_null` 已随内核 sqlx 实现一起删除
// （D3，2026-11-12）：它们只服务 sqlx 的两个后端，官方驱动各有自己的取文本路径
// （`mysql_native.rs` / `postgres_native.rs`）。等价逻辑现在在插件里
// （`tools/plugin-drivers/sqlx-rust/src/values.rs`），并且补了整数/浮点分支。

// ===== 元数据面：SQL 常量与纯函数已抽到 `crates/driver-sql`（与 sqlx 插件共用一份）=====
//
// 这里只 re-export：既有的 `utils::PG_LIST_TABLES_SQL` / `utils::split_csv` 等调用点
// 一行都不用改，同时保证「内核原生驱动」与「sqlx 插件二进制」拿到的是同一份文本。
// 抄第二份必然漂移（本仓 2026-09-21 刚把七份 `is_read_only_sql` 收敛成一份）。
pub use rds_driver_sql::codes::{pg_constraint_kind, pg_fk_action, split_csv};
pub use rds_driver_sql::meta_sql::{
    DUCK_TABLE_DETAIL_SQL, MY_LIST_CONSTRAINTS_SQL, MY_TABLE_DETAIL_SQL, PG_CURRENT_DATABASE_SQL,
    PG_LIST_CATALOGS_SQL, PG_LIST_CONSTRAINTS_SQL, PG_LIST_INDEXES_SQL, PG_LIST_SCHEMAS_SQL,
    PG_LIST_TABLES_SQL, PG_TABLE_DETAIL_SQL,
};

/// 按**约定列序**（见 [`PG_TABLE_DETAIL_SQL`]）把表详情的结果读成列详情。
///
/// 四个网络驱动 + DuckDB 的表详情都走这里：映射只写一份，`references` / `ordinal` 的
/// 语义就不会因为「抄到第三个驱动时改了半行」而分叉（本仓已经有过七份
/// `is_read_only_sql` 四种口径的先例）。
///
/// 行短于契约时缺的单元格按空串处理：不 panic、也不吞行。
pub fn columns_from_detail_rows(result: &QueryResult) -> Vec<crate::ColumnDetail> {
    batch_to_string_rows(result)
        .into_iter()
        .map(|r| {
            let cell = |i: usize| r.get(i).map_or("", |s| s.as_str());
            let ref_table = cell(7);
            // 「有没有外键」与「引到哪」分开读：复合外键的目标本查询不猜（见各 SQL
            // 的说明），但那一列**属于外键**这件事要如实标出来。
            let fk_flag = cell(9);
            crate::ColumnDetail {
                name: cell(0).to_string(),
                data_type: cell(1).to_string(),
                nullable: cell(2) == "YES",
                ordinal: cell(3).parse().unwrap_or(0),
                is_primary_key: cell(4) == "PRI",
                default_value: (!cell(5).is_empty()).then(|| cell(5).to_string()),
                comment: (!cell(6).is_empty()).then(|| cell(6).to_string()),
                is_foreign_key: fk_flag == "1" || !ref_table.is_empty(),
                references: (!ref_table.is_empty()).then(|| crate::ForeignKeyRef {
                    table: ref_table.to_string(),
                    column: cell(8).to_string(),
                }),
                // 列序契约 10..=12（2026-10-24）：标识 / 生成方式 / 排序规则。
                is_identity: cell(10) == "1",
                identity_generation: (!cell(11).is_empty()).then(|| cell(11).to_string()),
                collation: (!cell(12).is_empty()).then(|| cell(12).to_string()),
                extra: std::collections::HashMap::new(),
            }
        })
        .collect()
}

/// 结果批次读成 `Vec<Vec<String>>`（**每列都渲染成字符串**）。
///
/// 新补的内省方法一张表要取六七列，逐列 `downcast_ref::<StringArray>()` 会让方法体比 SQL
/// 还长。集中一次：字符串列直取，其余走 Arrow 的单值格式化（`ordinal_position` 这类整数列
/// 就不必在 SQL 里 `::text`），NULL 给空串。
///
/// **不 panic、也不吞行**：取不到值的单元格就是空串（调用方按 `is_empty` 判空）。
pub fn batch_to_string_rows(result: &QueryResult) -> Vec<Vec<String>> {
    let Some(batch) = result.batches.first() else {
        return Vec::new();
    };
    let cols = batch.num_columns();
    (0..batch.num_rows())
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let array = batch.column(col);
                    if array.is_null(row) {
                        String::new()
                    } else {
                        arrow::util::display::array_value_to_string(array, row).unwrap_or_default()
                    }
                })
                .collect()
        })
        .collect()
}

/// 从 `SHOW CREATE …` 的结果里取 DDL 原文（表 / 视图 / 过程 / 函数四类同一形状）。
///
/// **不能取固定列**：`SHOW CREATE PROCEDURE|FUNCTION` 的列序是（名称, sql_mode,
/// `Create …`, …）—— 列 1 是 `sql_mode` 而不是 DDL（旧实现恒取列 1，拿到的是 sql_mode）。
/// 所以按**列名** `Create …` 定位（`SHOW CREATE TABLE` 是 `Create Table`，
/// 视图是 `Create View`）。
///
/// 位置全名匹配不上时退回列 2（`SHOW CREATE TABLE` 的 DDL 列位置），拿不到就给 `None`。
pub fn create_statement(result: &QueryResult) -> Option<String> {
    let col_idx = result
        .columns
        .iter()
        .position(|c| c.starts_with("Create "))
        .unwrap_or(2);
    batch_to_string_rows(result)
        .into_iter()
        .next()
        .and_then(|row| row.get(col_idx).cloned())
        .filter(|s| !s.is_empty())
}

/// 换掉连接串里的**库名**（只动 path 段），其余原样保留。
///
/// ## 为什么不是「改 config 重建 URL」
///
/// PG 的跨库浏览要另开一条连到别的库的连接，而 `DriverConnectionConfig` 的 URL 走
/// `url_override`（`effective_url()` 的优先级最高）—— 里面可能带着连接参数：
/// LAN 直连注入的 `sslmode=disable`、`application_name`、字符集、`connect_timeout`…
/// 重建 URL 会把这些**静默丢掉**，于是「跨库那条连接」与主连接行为不一致
/// （连不上、TLS 行为不同），排查起来极隐蔽。只换 path 段就没有这个问题。
///
/// 支持形态：`scheme://[user[:pass]@]host[:port][/db]?query#fragment`。
/// **不支持** key=value 形式的 DSN（`host=… dbname=…`）：那不是本仓拼 URL 的口径
/// （`connection::url::build_connection_url` 只产出 URL 形式），遇到就返回 `None`，
/// 由调用方如实报错，不猜。
pub fn with_database_in_url(url: &str, database: &str) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let rest = &url[scheme_end..];
    // 查询串 / 片段原样保留
    let suffix_at = rest.find(['?', '#']).unwrap_or(rest.len());
    let (head, suffix) = rest.split_at(suffix_at);
    // 没有 path 段（`postgres://host:5432`）时补一个
    let authority = match head.find('/') {
        Some(slash) => &head[..slash],
        None => head,
    };
    if authority.is_empty() {
        return None;
    }
    let encoded = encode_path_segment(database);
    Some(format!(
        "{}{}/{}{}",
        &url[..scheme_end],
        authority,
        encoded,
        suffix
    ))
}

/// 取出连接串里的库名（`with_database_in_url` 的反向读法，只用于展示 / 判断）。
pub fn database_from_url(url: &str) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let rest = &url[scheme_end..];
    let head = match rest.find(['?', '#']) {
        Some(at) => &rest[..at],
        None => rest,
    };
    let slash = head.find('/')?;
    let db = head[slash + 1..].trim_end_matches('/');
    (!db.is_empty()).then(|| percent_decode(db))
}

/// path 段的百分号编码（RFC 3986 的 unreserved 之外一律编码；`/` 也编掉 —— 库名里不该有它）。
fn encode_path_segment(s: &str) -> String {
    connection::url::encode_userinfo(s)
}

/// 最小百分号解码（只处理 `%XX`；解不出来就原样保留那一截）。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 构建数据库连接URL
pub fn build_connection_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    match config.driver.as_str() {
        "mysql" => build_mysql_url(config),
        "postgres" => build_postgres_url(config),
        "sqlite" => build_sqlite_url(config),
        "duckdb" => build_duckdb_url(config),
        "clickhouse" => build_clickhouse_url(config),
        _ => Err(CoreError::connection(ConnectionError::DriverNotFound {
            driver: config.driver.clone(),
        })),
    }
}

/// 构建MySQL连接URL
fn build_mysql_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let host = config.host.as_deref().ok_or_else(|| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: config.name.clone().unwrap_or_else(|| "mysql".to_string()),
            reason: "Host is required".to_string(),
        })
    })?;

    let port = config.port.unwrap_or(3306);
    let username = config.username.as_deref().unwrap_or("root");
    let password = config.password.as_deref().unwrap_or("");
    let database = config.database.as_deref().unwrap_or("");

    Ok(format!(
        "mysql://{}:{}@{}:{}/{}",
        username, password, host, port, database
    ))
}

/// 构建PostgreSQL连接URL
fn build_postgres_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let host = config.host.as_deref().ok_or_else(|| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: config
                .name
                .clone()
                .unwrap_or_else(|| "postgres".to_string()),
            reason: "Host is required".to_string(),
        })
    })?;

    let port = config.port.unwrap_or(5432);
    let username = config.username.as_deref().unwrap_or("postgres");
    let password = config.password.as_deref().unwrap_or("");
    let database = config.database.as_deref().unwrap_or("postgres");

    Ok(format!(
        "postgres://{}:{}@{}:{}/{}",
        username, password, host, port, database
    ))
}

/// 构建SQLite连接URL
fn build_sqlite_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let database = config.database.as_deref().ok_or_else(|| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: config.name.clone().unwrap_or_else(|| "sqlite".to_string()),
            reason: "Database path is required".to_string(),
        })
    })?;

    Ok(format!("sqlite://{}", database))
}

/// 构建DuckDB连接URL
fn build_duckdb_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let database = config.database.as_deref().unwrap_or(":memory:");

    Ok(format!("duckdb://{}", database))
}

/// 构建ClickHouse连接URL
fn build_clickhouse_url(config: &DriverConnectionConfig) -> Result<String, CoreError> {
    let host = config.host.as_deref().ok_or_else(|| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: config
                .name
                .clone()
                .unwrap_or_else(|| "clickhouse".to_string()),
            reason: "Host is required".to_string(),
        })
    })?;

    let port = config.port.unwrap_or(9000);
    let username = config.username.as_deref().unwrap_or("default");
    let password = config.password.as_deref().unwrap_or("");
    let database = config.database.as_deref().unwrap_or("default");

    Ok(format!(
        "clickhouse://{}:{}@{}:{}/{}",
        username, password, host, port, database
    ))
}

/// 验证驱动配置
pub fn validate_driver_config(config: &DriverConnectionConfig) -> Result<(), CoreError> {
    match config.driver.as_str() {
        "mysql" | "postgres" | "clickhouse" => {
            // 验证网络数据库的必需字段
            if config.host.is_none() {
                return Err(CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| config.driver.clone()),
                    reason: "Host is required".to_string(),
                }));
            }
            if config.username.is_none() {
                return Err(CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| config.driver.clone()),
                    reason: "Username is required".to_string(),
                }));
            }
        }
        "sqlite" => {
            // 验证SQLite的必需字段
            if config.database.is_none() {
                return Err(CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: config.name.clone().unwrap_or_else(|| "sqlite".to_string()),
                    reason: "Database path is required".to_string(),
                }));
            }
        }
        "duckdb" => {
            // DuckDB不需要验证，默认使用内存数据库
        }
        _ => {
            return Err(CoreError::connection(ConnectionError::DriverNotFound {
                driver: config.driver.clone(),
            }));
        }
    }

    Ok(())
}

/// 排查/转义工具已下沉 `shared::utils::sql`（engine 瘦身第二刀前置小手术）：
/// persistence 与 driver 两侧共用，此处仅做兼容重导出。
pub use shared::utils::sql::{escape_sql_string, quote_identifier};

/// 排查标准 SQL 引号下的标识符，等同于 quote_identifier(input, '"')
pub fn escape_identifier(input: &str) -> String {
    quote_identifier(input, '"')
}

/// 写语句的结果：**没有结果集，只有影响行数**（B5 / P0.6）
///
/// 界面上 DML / DDL 成功时显示“影响 N 行”。`u32` 溢出按上限饱和——不假装是个精确值。
pub fn affected_rows_result(affected: u64) -> shared::models::QueryResult {
    shared::models::QueryResult {
        columns: Vec::new(),
        batches: Vec::new(),
        affected_rows: Some(affected.min(u64::from(u32::MAX)) as u32),
        is_read_only: Some(false),
        ..Default::default()
    }
}

/// 取语句的首个关键字（跳过前置注释与空白）
///
/// 只认 ASCII 标识符字符，返回小写；拿不到就返回空串。
fn first_keyword(sql: &str) -> String {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        if let Some(tail) = rest.strip_prefix("--") {
            rest = match tail.find('\n') {
                Some(idx) => &tail[idx + 1..],
                None => return String::new(),
            };
        } else if let Some(tail) = rest.strip_prefix("/*") {
            rest = match tail.find("*/") {
                Some(idx) => &tail[idx + 2..],
                None => return String::new(),
            };
        } else {
            break;
        }
    }

    rest.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// 语句里是否出现独立单词 `RETURNING`
///
/// 扫描时跳过字符串字面量、引用标识符与注释，所以 `returning_log`、
/// `'returning'`、`-- returning` 都不算子句。
fn has_returning_clause(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // 单引号字面量：`''` 是转义写法
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            // 引用标识符（`"a"` / `` `a` ``）：同样双写转义
            quote @ (b'"' | b'`') => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        if i + 1 < bytes.len() && bytes[i + 1] == quote {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            // 行注释
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // 块注释
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if bytes[start..i].eq_ignore_ascii_case(b"returning") {
                    return true;
                }
            }
            _ => i += 1,
        }
    }
    false
}

/// 语句自己会不会返回行
///
/// 驱动据它决定「走 `query` 取结果集」还是「走 `execute` 只取影响行数」。
/// 判定看两部分：
///
/// * **首个关键字**——`SELECT` / `WITH` / `VALUES` / `TABLE` / `SHOW` 等本身就会回行
///   （`WITH … SELECT` 是查询，不能因为不是 `SELECT` 开头就当成写语句）；
/// * **语句里有没有 `RETURNING`**——带 `RETURNING` 的写语句既影响行、又回行。
///
/// 两边都要顾及时驱动给不出「两者兼得」的结果：**保行**，用户至少看得见数据。
pub fn returns_rows(sql: &str) -> bool {
    const ROW_KEYWORDS: [&str; 10] = [
        "select",
        "with",
        "values",
        "table",
        "show",
        "describe",
        "desc",
        "explain",
        "pragma",
        "summarize",
    ];
    ROW_KEYWORDS.contains(&first_keyword(sql).as_str()) || has_returning_clause(sql)
}

/// SQL 方言族（只用于 [`is_read_only_sql`] 的口径表）。
///
/// 与 `SqlDialect`（语句解析的方言）不是一回事：那个决定「怎么切分 / 怎么归类语句」，
/// 这个只回答「这个驱动认哪些只读前缀」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlFlavor {
    /// SQLite（rusqlite）。
    Sqlite,
    /// DuckDB（duckdb-rs）。
    DuckDb,
    /// MySQL（sqlx 与 mysql_async 两条路径同族）。
    MySql,
    /// PostgreSQL（sqlx 与 pg_wire 两条路径同族）。
    PostgreSql,
}

/// 语句是否只读（不改数据）。
///
/// ## 为什么要有这一份
///
/// `!is_read_only && !returns_rows(sql)` 决定一条语句走哪条路：`execute`（写路径，
/// 拿真实影响行数）还是 `query`（取结果集）。判定错一格的后果不是报错，而是**行为悄悄变**：
/// `SET` 被当写语句执行会「影响 0 行」、`PRAGMA` 被当写语句执行会丢掉结果集。
///
/// 此前六个驱动各写一份、四种口径（见 `columns_from_detail_rows` 的注释）；
/// 抄到后来还漏过前缀——DuckDB 的直连路径少算了 `EXPLAIN` / `PRAGMA`。
/// 差异本身是**方言事实**而不是笔误，所以这里保留差异、只合并实现：
/// 四种口径并排，谁多了什么一眼看清。
///
/// ## 口径
///
/// * `select`：四家都算（`with` / `values` / `table` 由 [`returns_rows`] 认，不必列此）；
/// * `show` / `describe` / `desc`：MySQL / PG 的元信息语句，SQLite 没有；
/// * `explain`：四家都有；
/// * `pragma`：SQLite / DuckDB 的查询式语句（MySQL / PG 的对应物是 `SET`，见下）；
/// * `set`：MySQL / PG 的会话设置——**不改数据但不返回行**，少了它就会被当写语句执行。
///
/// 认**首个关键字**（跳过前置注释与空白，与 [`returns_rows`] 同一口径）：老实现走
/// `sql.trim_start().starts_with(..)`，`-- 注释\nSET ...` 会被判成写语句。
pub fn is_read_only_sql(sql: &str, flavor: SqlFlavor) -> bool {
    let head = first_keyword(sql);
    match flavor {
        SqlFlavor::Sqlite => matches!(head.as_str(), "select" | "pragma" | "explain"),
        SqlFlavor::DuckDb => matches!(
            head.as_str(),
            "select" | "show" | "describe" | "desc" | "explain" | "pragma"
        ),
        SqlFlavor::MySql | SqlFlavor::PostgreSql => matches!(
            head.as_str(),
            "select" | "show" | "describe" | "desc" | "explain" | "set"
        ),
    }
}

/// 能不能被包进 `SELECT * FROM ( … ) AS 别名` 里去（分段抓取的窗口包装）
///
/// 分段抓取靠“把原句套成子查询 + LIMIT/OFFSET”取数（见 `SqlService::window_sql`）。
/// 下面这些首关键字**不接受子查询上下文**，套上去就是语法错误——真机踩到过：
/// MySQL 上 `SELECT * FROM (\nEXPLAIN SELECT …\n) AS rds_segment LIMIT 1000 OFFSET 0`
/// 报 1064。
///
/// - `explain` / `show` / `describe` / `desc` / `summarize`：**元信息语句**（结果就几行，
///   本来也不需要分段）；
/// - `pragma` / `set` / `use` / `call` / `exec(ute)`：**会话控制 / 存储过程调用**
///   （在有些库里连“子查询里出现”都不合法）。
///
/// 拿不准的一律**按能包**算（`select` / `with` / `values` / `table` 等正常查询）；
/// 包不了的那些走 `execute_first_segment` 的“原路执行”分支（那段代码本就在）。
pub fn wrappable_in_subquery(sql: &str) -> bool {
    const UNWRAPPABLE: [&str; 11] = [
        "explain",
        "show",
        "describe",
        "desc",
        "summarize",
        "pragma",
        "set",
        "use",
        "call",
        "exec",
        "execute",
    ];
    !UNWRAPPABLE.contains(&first_keyword(sql).as_str())
}

/// 解析驱动ID
pub fn parse_driver_id(url: &str) -> Option<&str> {
    if url.starts_with("mysql://") {
        Some("mysql")
    } else if url.starts_with("postgres://") {
        Some("postgres")
    } else if url.starts_with("sqlite://") {
        Some("sqlite")
    } else if url.starts_with("duckdb://") {
        Some("duckdb")
    } else if url.starts_with("clickhouse://") {
        Some("clickhouse")
    } else {
        None
    }
}

/// 【T2c-1】驱动 id（`ConnectionInfo.db_type` 的串）→ 方言族（会话上下文映射用）
///
/// `db_type` 是驱动 id（`mysql_native` / `postgres_native` / `sqlite` / `duckdb` 等，
/// 见 `driver::factory`），按前缀归类——与 `SqlService::run_filtered` 的
/// `starts_with("mysql")` 同一口径。不认识的驱动（JDBC 等）返回 `None`：**宁可不注入
/// 会话上下文，也不发一句驱动不认的 `SET` 把执行搅黄**。
pub fn flavor_from_db_type(db_type: &str) -> Option<SqlFlavor> {
    let t = db_type.to_ascii_lowercase();
    if t.starts_with("postgres") {
        Some(SqlFlavor::PostgreSql)
    } else if t.starts_with("mysql") {
        Some(SqlFlavor::MySql)
    } else if t.starts_with("duckdb") {
        Some(SqlFlavor::DuckDb)
    } else if t.starts_with("sqlite") {
        Some(SqlFlavor::Sqlite)
    } else {
        None
    }
}

/// 【T2c-1】把所选 schema 映射成「会话上下文」语句（方言差异在此定死，纯函数可测）
///
/// 切 schema 真影响执行的落点：执行用户语句前，先在这条连接的会话里把未限定名的
/// 解析指向所选 schema。方言映射：
///
/// - PostgreSQL：`SET search_path TO "schema"`（会话级；引号内 `"` 转义为 `""`）；
/// - MySQL：`` USE `schema` ``（文本协议执行；`needs_text_protocol` 已认 `USE`，
///   事务内的 `USE` 由 `MySqlTransaction::query` 的文本协议路由兜底）；
/// - DuckDB：`USE "schema"`（切换默认 catalog/schema）；
/// - SQLite：无 schema 概念（文件即库）→ `None`，调用方不注入。
///
/// schema 名来自导航缓存的元数据（不是用户手敲的对抗性输入），但统一按**标识符**
/// 加引号 + 转义，避免名里带特殊字符（空格 / 点 / 引号）时拼出语法错。
pub fn schema_context_sql(schema: &str, flavor: SqlFlavor) -> Option<String> {
    let name = schema.trim();
    if name.is_empty() {
        return None;
    }
    match flavor {
        SqlFlavor::PostgreSql => Some(format!(
            "SET search_path TO \"{}\"",
            name.replace('"', "\"\"")
        )),
        SqlFlavor::MySql => Some(format!("USE `{}`", name.replace('`', "``"))),
        SqlFlavor::DuckDb => Some(format!("USE \"{}\"", name.replace('"', "\"\""))),
        SqlFlavor::Sqlite => None,
    }
}

/// 【T2c-1】把 schema 绑定**恢复成连接默认**的语句（解绑时用；纯函数可测）
///
/// 为什么需要它：`SET search_path` / `USE` 都是**会话级**状态，不解绑就会残留——
/// 用户点了「默认」之后，未限定名仍解析到上一个 schema（实测：解绑后本该报
/// 「表不存在」，却查到了旧 schema 里的表）。方言映射：
///
/// - PostgreSQL：`RESET search_path`（回到角色 / 库的默认值，不需要知道默认是什么）；
/// - MySQL：`` USE `库名` ``（MySQL 没有「恢复默认库」语句，默认库只能从 URL 的库段取）；
/// - DuckDB / SQLite：暂无 schema 与会话语义的对齐（不注入）。
pub fn default_schema_reset_sql(db_type: &str, url: &str) -> Option<String> {
    let flavor = flavor_from_db_type(db_type)?;
    match flavor {
        SqlFlavor::PostgreSql => Some("RESET search_path".to_string()),
        SqlFlavor::MySql => {
            let path = url.split(['?', '#']).next()?.split("://").nth(1)?;
            let db = path.split('/').nth(1)?.trim();
            if db.is_empty() {
                return None;
            }
            Some(format!("USE `{}`", db.replace('`', "``")))
        }
        SqlFlavor::DuckDb | SqlFlavor::Sqlite => None,
    }
}

/// 这句 SQL 是不是在改**会话的默认 schema**（`USE`）——驱动据此把握手/执行过的
/// 会话状态记下来，在换到池里另一条连接时补上（见 `mysql_native` 的 `session_sql`）。
pub fn is_session_schema_statement(sql: &str) -> bool {
    let t = sql.trim_start();
    let head = t.get(..4).unwrap_or("");
    head.eq_ignore_ascii_case("use ") || head.eq_ignore_ascii_case("use\t")
}

/// 1 基的**字符**位置（数据库常这么报）→ SQL 里的**字节**偏移
///
/// PG 协议的 `position` 字段就是「第 1 个字符是 1、按字符数」；而编辑器内核按字节收
/// 区间，所以两边要换算。越界（有的驱动给的数字超出语句长度）返回 `None`——
/// 宁可没有位置，也不要一个错位置。
pub fn byte_offset_for_char(sql: &str, one_based_char: usize) -> Option<usize> {
    if one_based_char == 0 {
        return None;
    }
    sql.char_indices()
        .nth(one_based_char - 1)
        .map(|(offset, _)| offset)
}

#[cfg(test)]
mod tests {
    use super::{affected_rows_result, returns_rows, wrappable_in_subquery};

    /// 【B10】能不能包进子查询：元信息 / 会话控制语句不行（真机踩到）
    #[test]
    fn meta_statements_are_not_wrappable_in_a_subquery() {
        for sql in [
            "EXPLAIN SELECT 1",
            "explain analyze select 1",
            "SHOW TABLES",
            "DESCRIBE t",
            "PRAGMA table_info(t)",
            "SET search_path TO public",
            "USE db",
        ] {
            assert!(!wrappable_in_subquery(sql), "不该能包：{sql}");
        }
        // 正常查询（含前面带注释的）照旧能包
        for sql in [
            "SELECT 1",
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "  values (1)",
            "-- 注释\nSELECT 1",
        ] {
            assert!(wrappable_in_subquery(sql), "该能包：{sql}");
        }
    }

    /// 写语句（无 RETURNING）不返回行——驱动才会走 `execute` 取影响行数
    #[test]
    fn writes_do_not_return_rows() {
        assert!(!returns_rows("INSERT INTO t VALUES (1)"));
        assert!(!returns_rows("  update t set a = 1"));
        assert!(!returns_rows("DELETE FROM t"));
        assert!(!returns_rows("CREATE TABLE t (a INT)"));
        assert!(!returns_rows("TRUNCATE t"));
        assert!(!returns_rows(""));
    }

    /// 查询语句都返回行
    #[test]
    fn reads_return_rows() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("  \n select 1"));
        assert!(returns_rows("VALUES (1)"));
        assert!(returns_rows("TABLE t"));
        assert!(returns_rows("SHOW TABLES"));
        assert!(returns_rows("EXPLAIN SELECT 1"));
    }

    /// `WITH …` 是查询，不能因为不是 `SELECT` 开头就当成写语句
    #[test]
    fn cte_stays_a_query() {
        assert!(returns_rows("WITH x AS (SELECT 1) SELECT * FROM x"));
        assert!(returns_rows(
            "with recursive f(n) as (select 1) select n from f"
        ));
    }

    /// 带 `RETURNING` 的写语句既影响行又回行，按「保行」处理
    #[test]
    fn returning_clause_keeps_rows() {
        assert!(returns_rows("INSERT INTO t (a) VALUES (1) RETURNING id"));
        assert!(returns_rows("UPDATE t SET a = 1 returning *"));
        assert!(returns_rows("DELETE FROM t RETURNING *"));
    }

    /// `RETURNING` 必须是个独立单词：标识符、字符串字面量、注释里的都不算
    #[test]
    fn returning_inside_identifiers_is_not_a_clause() {
        assert!(!returns_rows("INSERT INTO returning_log (a) VALUES (1)"));
        assert!(!returns_rows("INSERT INTO t (a) VALUES ('returning')"));
        assert!(!returns_rows(
            "INSERT INTO t (a) VALUES ('it''s returning')"
        ));
        assert!(!returns_rows(
            "INSERT INTO t (a) VALUES ('后 RETURNING 前')"
        ));
        assert!(!returns_rows("-- returning\nDELETE FROM t"));
        assert!(!returns_rows("/* returning */ DELETE FROM t"));
        assert!(returns_rows("INSERT INTO t (a) VALUES ('x') RETURNING id"));
    }

    /// 前置注释不参与首关键字判定
    #[test]
    fn leading_comments_are_skipped() {
        assert!(returns_rows("-- 备注\nSELECT 1"));
        assert!(returns_rows("/* 备注 */ SELECT 1"));
        assert!(!returns_rows("-- 备注\nDELETE FROM t"));
        assert!(!returns_rows("/* a */ /* b */ UPDATE t SET a = 1"));
        assert!(!returns_rows("-- 只有注释"));
    }

    /// 1 基字符位置 → 字节偏移（含多字节字符与越界）
    #[test]
    fn char_positions_convert_to_byte_offsets() {
        use super::byte_offset_for_char;

        assert_eq!(byte_offset_for_char("select 1", 1), Some(0));
        assert_eq!(byte_offset_for_char("select 1", 8), Some(7));
        // 越界返回 None，不钳到末尾（那样定位会指到错的地方）
        assert_eq!(byte_offset_for_char("select 1", 9), None);
        assert_eq!(byte_offset_for_char("select 1", 0), None);
        // 多字节：第 8 个**字符**是 `名`，字节偏移 7
        assert_eq!(byte_offset_for_char("select 名", 8), Some(7));
        assert_eq!(byte_offset_for_char("select 名", 9), None);
    }

    /// 影响行数超过 u32 上限时饱和，不假装是精确值
    #[test]
    fn affected_rows_saturates_at_u32() {
        let small = affected_rows_result(3);
        assert_eq!(small.affected_rows, Some(3));
        assert!(small.columns.is_empty());
        assert!(small.batches.is_empty());
        assert_eq!(small.is_read_only, Some(false));

        let huge = affected_rows_result(u64::from(u32::MAX) + 10);
        assert_eq!(huge.affected_rows, Some(u32::MAX));
    }
}

#[cfg(test)]
mod url_database_tests {
    use super::{database_from_url, with_database_in_url};

    /// 换库名只动 path 段：认证、端口、查询串**一个都不能丢**（丢了就是跨库连接连不上）。
    #[test]
    fn swapping_database_keeps_everything_else() {
        assert_eq!(
            with_database_in_url("postgres://u:p@h:5432/old?sslmode=disable&a=1", "new").unwrap(),
            "postgres://u:p@h:5432/new?sslmode=disable&a=1"
        );
        // 没有 path 段时补一个
        assert_eq!(
            with_database_in_url("postgres://u:p@h:5432", "new").unwrap(),
            "postgres://u:p@h:5432/new"
        );
        // 末尾多一个斜杠也认
        assert_eq!(
            with_database_in_url("postgres://h/old/", "new").unwrap(),
            "postgres://h/new"
        );
        // 库名里的特殊字符要编码（不然会把 URL 结构弄坏）
        assert_eq!(
            with_database_in_url("postgres://h/old", "a/b").unwrap(),
            "postgres://h/a%2Fb"
        );
        // 不是 URL 形式（DSN）→ 如实说不支持，不猜
        assert!(with_database_in_url("host=h dbname=old", "new").is_none());
        assert!(with_database_in_url("postgres://", "new").is_none());
    }

    /// 反向读库名（判断「是不是当前库」用）。
    #[test]
    fn reading_database_back() {
        assert_eq!(
            database_from_url("postgres://u:p@h:5432/warehouse?sslmode=disable").as_deref(),
            Some("warehouse")
        );
        assert_eq!(database_from_url("postgres://h").as_deref(), None);
        assert_eq!(database_from_url("postgres://h/").as_deref(), None);
        assert_eq!(
            database_from_url("postgres://h/a%2Fb").as_deref(),
            Some("a/b"),
            "写进去时的编码要能读回来"
        );
    }
}

#[cfg(test)]
mod read_only_tests {
    use super::{SqlFlavor, is_read_only_sql};

    /// 只读口径：四种方言并排钉住。
    ///
    /// 这些差异是**方言事实**（SQLite 没有 `SHOW` / `DESCRIBE`；`SET` 只有 MySQL / PG 有；
    /// `PRAGMA` 是 SQLite / DuckDB 的查询式语句），所以合表是错的、必须逐族断言。
    /// 判错的后果不是报错而是行为悄悄变：`SET` 走写路径会「影响 0 行」、
    /// `PRAGMA` 走写路径会丢掉结果集。
    #[test]
    fn read_only_prefixes_are_per_flavor() {
        const ALL: [SqlFlavor; 4] = [
            SqlFlavor::Sqlite,
            SqlFlavor::DuckDb,
            SqlFlavor::MySql,
            SqlFlavor::PostgreSql,
        ];
        // 四家都算只读：查询本身与 `EXPLAIN`。
        for flavor in ALL {
            assert!(is_read_only_sql("SELECT 1", flavor), "{flavor:?} 认 SELECT");
            assert!(
                is_read_only_sql("  explain select 1", flavor),
                "{flavor:?} 认 EXPLAIN"
            );
            assert!(
                !is_read_only_sql("INSERT INTO t VALUES (1)", flavor),
                "{flavor:?} 不该把 INSERT 当只读"
            );
        }
        // 方言差异。
        assert!(is_read_only_sql("SHOW TABLES", SqlFlavor::MySql));
        assert!(is_read_only_sql("DESCRIBE t", SqlFlavor::PostgreSql));
        assert!(!is_read_only_sql("SHOW TABLES", SqlFlavor::Sqlite));
        assert!(!is_read_only_sql(
            "SET search_path TO public",
            SqlFlavor::Sqlite
        ));
        assert!(is_read_only_sql(
            "SET search_path TO public",
            SqlFlavor::PostgreSql
        ));
        assert!(is_read_only_sql("SET @x = 1", SqlFlavor::MySql));
        assert!(is_read_only_sql("PRAGMA table_info(t)", SqlFlavor::Sqlite));
        assert!(is_read_only_sql("PRAGMA table_info(t)", SqlFlavor::DuckDb));
        assert!(!is_read_only_sql(
            "PRAGMA table_info(t)",
            SqlFlavor::PostgreSql
        ));
        // 前置注释要跳过（与 `returns_rows` 同一口径；老写法在这里会判成写语句）。
        assert!(is_read_only_sql(
            "-- 找个人\nSELECT 1",
            SqlFlavor::PostgreSql
        ));
        assert!(is_read_only_sql(
            "/* 注释 */ set search_path to public",
            SqlFlavor::MySql
        ));
        // 只有注释：认不出语句，按「不是只读」算（交给 `returns_rows` 与驱动自己报错）。
        assert!(!is_read_only_sql("-- 半句话", SqlFlavor::PostgreSql));
    }
}

#[cfg(test)]
mod schema_context_tests {
    use super::{
        SqlFlavor, default_schema_reset_sql, flavor_from_db_type, is_session_schema_statement,
        schema_context_sql,
    };

    /// 【T2c-1】方言归类：驱动 id 前缀 → 方言族；不认识的驱动不硬猜。
    #[test]
    fn flavor_from_db_type_classifies_known_drivers() {
        assert_eq!(
            flavor_from_db_type("postgres_native"),
            Some(SqlFlavor::PostgreSql)
        );
        assert_eq!(flavor_from_db_type("postgres"), Some(SqlFlavor::PostgreSql));
        assert_eq!(flavor_from_db_type("mysql_native"), Some(SqlFlavor::MySql));
        assert_eq!(flavor_from_db_type("mysql"), Some(SqlFlavor::MySql));
        assert_eq!(flavor_from_db_type("duckdb"), Some(SqlFlavor::DuckDb));
        assert_eq!(flavor_from_db_type("sqlite"), Some(SqlFlavor::Sqlite));
        // 大小写不敏感（与 `run_filtered` 的 starts_with 口径一致）。
        assert_eq!(flavor_from_db_type("MySQL_NATIVE"), Some(SqlFlavor::MySql));
        // 不认识的驱动（JDBC 等）：宁可不注入也不发一句不认的 SET。
        assert_eq!(flavor_from_db_type("jdbc:postgresql"), None);
        assert_eq!(flavor_from_db_type(""), None);
    }

    /// 【T2c-1】解绑要能把会话状态**恢复成连接默认**：绑定是会话级的，会残留。
    #[test]
    fn default_schema_reset_maps_per_flavor() {
        // PG：不需要知道默认是什么，`RESET` 回到角色 / 库的默认。
        assert_eq!(
            default_schema_reset_sql(
                "postgres_native",
                "postgres://u:p@h:5432/postgres?sslmode=disable"
            )
            .as_deref(),
            Some("RESET search_path")
        );
        // MySQL：没有「恢复默认库」语句，默认库取 URL 的库段（查询串要剥掉）。
        assert_eq!(
            default_schema_reset_sql("mysql_native", "mysql://root:pw@h:3306/mall?ssl-mode=DISABLED")
                .as_deref(),
            Some("USE `mall`")
        );
        assert_eq!(
            default_schema_reset_sql("mysql", "mysql://u:p@h:3306/db").as_deref(),
            Some("USE `db`")
        );
        // 没写库段 → 无从恢复，宁可不发也不要发一句错的。
        assert_eq!(default_schema_reset_sql("mysql_native", "mysql://u:p@h:3306"), None);
        // 文件型 / 不认识的驱动：不注入。
        assert_eq!(default_schema_reset_sql("sqlite", "sqlite://D:\\x.db"), None);
        assert_eq!(default_schema_reset_sql("jdbc:postgresql", "jdbc:postgresql://h/db"), None);
    }

    /// 【T2c-1】「改会话默认 schema」的语句判定：驱动据此记下会话状态。
    #[test]
    fn session_schema_statements_are_recognized() {
        assert!(is_session_schema_statement("USE `mall`"));
        assert!(is_session_schema_statement("  use mall"));
        assert!(is_session_schema_statement("USE\tmall"));
        // 前缀相近但不是 `USE`：不能误判（否则会把随便一句记成会话状态）。
        assert!(!is_session_schema_statement("USER"));
        assert!(!is_session_schema_statement("SELECT 'use x'"));
        assert!(!is_session_schema_statement("UPDATE t SET a = 1"));
    }

    /// 【T2c-1】会话上下文语句按方言映射；SQLite 无 schema 概念不注入。
    #[test]
    fn schema_context_sql_maps_per_flavor() {
        assert_eq!(
            schema_context_sql("sales", SqlFlavor::PostgreSql).as_deref(),
            Some("SET search_path TO \"sales\"")
        );
        assert_eq!(
            schema_context_sql("sales", SqlFlavor::MySql).as_deref(),
            Some("USE `sales`")
        );
        assert_eq!(
            schema_context_sql("sales", SqlFlavor::DuckDb).as_deref(),
            Some("USE \"sales\"")
        );
        // SQLite：文件即库，没有可切的 schema。
        assert_eq!(schema_context_sql("main", SqlFlavor::Sqlite), None);
    }

    /// 【T2c-1】标识符引号 + 转义：名里带特殊字符不能拼出语法错。
    #[test]
    fn schema_names_are_quoted_and_escaped() {
        // PG / DuckDB：双引号内 `"` → `""`。
        assert_eq!(
            schema_context_sql("a\"b", SqlFlavor::PostgreSql).as_deref(),
            Some("SET search_path TO \"a\"\"b\"")
        );
        // MySQL：反引号内 `` ` `` → ````。
        assert_eq!(
            schema_context_sql("a`b", SqlFlavor::MySql).as_deref(),
            Some("USE `a``b`")
        );
        // 空串 / 纯空白：不注入。
        assert_eq!(schema_context_sql("", SqlFlavor::PostgreSql), None);
        assert_eq!(schema_context_sql("   ", SqlFlavor::MySql), None);
        // 名里带空格 / 点：引号包住即可，不该被拆成语段。
        assert_eq!(
            schema_context_sql("my schema", SqlFlavor::PostgreSql).as_deref(),
            Some("SET search_path TO \"my schema\"")
        );
    }
}

#[cfg(test)]
mod columns_detail_tests {
    use super::columns_from_detail_rows;

    /// 列序契约 10..=12（2026-10-24 列字段增强）：标识 / 生成方式 / 排序规则映射。
    /// 行 0 模拟 PG 标识列（`attidentity='d'` → BY DEFAULT，默认 collation）；
    /// 行 1 模拟普通文本列（显式 `C` 排序规则）。
    #[test]
    fn detail_rows_map_identity_and_collation() {
        use arrow::array::StringArray;
        use arrow::datatypes::{DataType, Field, Schema};
        use shared::models::ArrowBatch;

        let values = [
            // name/type/nullable/ordinal/key/default/comment/ref_table/ref_col/fk/identity/generation/collation
            vec![
                "id",
                "bigint",
                "NO",
                "1",
                "PRI",
                "",
                "",
                "",
                "",
                "1",
                "1",
                "BY DEFAULT",
                "default",
            ],
            vec!["c", "text", "YES", "2", "", "", "", "", "", "", "", "", "C"],
        ];
        let schema = std::sync::Arc::new(Schema::new(
            (0..13)
                .map(|i| Field::new(format!("c{i}"), DataType::Utf8, true))
                .collect::<Vec<_>>(),
        ));
        let arrays: Vec<std::sync::Arc<dyn arrow::array::Array>> = (0..13)
            .map(|col| {
                std::sync::Arc::new(StringArray::from(
                    values.iter().map(|row| row[col]).collect::<Vec<_>>(),
                )) as std::sync::Arc<dyn arrow::array::Array>
            })
            .collect();
        let qr = shared::models::QueryResult {
            columns: (0..13).map(|i| format!("c{i}")).collect(),
            batches: vec![ArrowBatch::try_new(schema, arrays).expect("批")],
            ..Default::default()
        };

        let cols = columns_from_detail_rows(&qr);
        assert_eq!(cols.len(), 2);
        assert!(cols[0].is_identity, "attidentity 非空 → 标识");
        assert_eq!(cols[0].identity_generation.as_deref(), Some("BY DEFAULT"));
        assert_eq!(cols[0].collation.as_deref(), Some("default"));
        assert!(!cols[1].is_identity, "非标识列如实标记");
        assert_eq!(cols[1].identity_generation, None);
        assert_eq!(cols[1].collation.as_deref(), Some("C"));
    }

    /// MySQL 行：EXTRA 含 auto_increment → 标识 + AUTO_INCREMENT；无排序规则 → None。
    #[test]
    fn detail_rows_map_mysql_auto_increment() {
        use arrow::array::StringArray;
        use arrow::datatypes::{DataType, Field, Schema};
        use shared::models::ArrowBatch;

        let values = [vec![
            "id",
            "int",
            "NO",
            "1",
            "PRI",
            "",
            "",
            "",
            "",
            "1",
            "1",
            "AUTO_INCREMENT",
            "utf8mb4_0900_ai_ci",
        ]];
        let schema = std::sync::Arc::new(Schema::new(
            (0..13)
                .map(|i| Field::new(format!("c{i}"), DataType::Utf8, true))
                .collect::<Vec<_>>(),
        ));
        let arrays: Vec<std::sync::Arc<dyn arrow::array::Array>> = (0..13)
            .map(|col| {
                std::sync::Arc::new(StringArray::from(
                    values.iter().map(|row| row[col]).collect::<Vec<_>>(),
                )) as std::sync::Arc<dyn arrow::array::Array>
            })
            .collect();
        let qr = shared::models::QueryResult {
            columns: (0..13).map(|i| format!("c{i}")).collect(),
            batches: vec![ArrowBatch::try_new(schema, arrays).expect("批")],
            ..Default::default()
        };

        let cols = columns_from_detail_rows(&qr);
        assert!(cols[0].is_identity);
        assert_eq!(
            cols[0].identity_generation.as_deref(),
            Some("AUTO_INCREMENT")
        );
        assert_eq!(cols[0].collation.as_deref(), Some("utf8mb4_0900_ai_ci"));
    }

    /// 行短于契约（如旧缓存 / 非本模块来源）：缺的单元格按空串处理，不 panic、不吞行。
    #[test]
    fn short_rows_fill_empty_cells() {
        use arrow::array::StringArray;
        use arrow::datatypes::{DataType, Field, Schema};
        use shared::models::ArrowBatch;

        let schema = std::sync::Arc::new(Schema::new(vec![Field::new("c0", DataType::Utf8, true)]));
        let arrays: Vec<std::sync::Arc<dyn arrow::array::Array>> =
            vec![std::sync::Arc::new(StringArray::from(vec!["id"]))];
        let qr = shared::models::QueryResult {
            columns: vec!["c0".to_string()],
            batches: vec![ArrowBatch::try_new(schema, arrays).expect("批")],
            ..Default::default()
        };

        let cols = columns_from_detail_rows(&qr);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].name, "id");
        assert!(!cols[0].is_identity);
        assert_eq!(cols[0].collation, None);
    }
}
