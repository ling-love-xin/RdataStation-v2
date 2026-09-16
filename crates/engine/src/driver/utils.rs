use super::registry::DriverConnectionConfig;
use shared::error::{ConnectionError, CoreError};

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

/// 安全转义 SQL 字符串字面量中的单引号
///
/// 将输入中的 `'` 替换为 `''`，这是 ANSI SQL 标准的转义方式，
/// 适用于所有主流数据库（MySQL/PostgreSQL/SQLite/DuckDB）。
///
/// 同时移除了空字节 `\0`，防止字符串截断攻击。
///
/// # 用法
/// ```ignore
/// let sql = format!("WHERE name = '{}'", escape_sql_string(input));
/// ```
pub fn escape_sql_string(input: &str) -> String {
    input.replace('\'', "''").replace('\0', "")
}

/// 使用数据库方言对应的引号包裹标识符（表名/列名/数据库名）
///
/// 将引号字符在标识符内双写后，用该引号包裹整体。
///
/// | 数据库 | 引号 | 示例 |
/// |--------|------|------|
/// | MySQL | `` ` `` | `` `table``name` `` |
/// | PostgreSQL | `"` | `"table""name"` |
/// | SQLite | `"` | `"table""name"` |
/// | DuckDB | `"` | `"table""name"` |
///
/// # 用法
/// ```ignore
/// let sql = format!("PRAGMA table_info(\"{}\")", quote_identifier(table, '"'));
/// ```
pub fn quote_identifier(input: &str, quote_char: char) -> String {
    let escaped = input.replace(quote_char, &format!("{}{}", quote_char, quote_char));
    format!("{}{}{}", quote_char, escaped, quote_char)
}

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
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
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
        "select", "with", "values", "table", "show", "describe", "desc", "explain",
        "pragma", "summarize",
    ];
    ROW_KEYWORDS.contains(&first_keyword(sql).as_str()) || has_returning_clause(sql)
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

#[cfg(test)]
mod tests {
    use super::{affected_rows_result, returns_rows};

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
        assert!(!returns_rows("INSERT INTO t (a) VALUES ('it''s returning')"));
        assert!(!returns_rows("INSERT INTO t (a) VALUES ('后 RETURNING 前')"));
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
