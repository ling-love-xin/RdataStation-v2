//! 结果集出口层：类型名 → `Canonical`、单元格 → 展示文本、语句是否返回行。
//!
//! ## 为什么单元格一律先解成文本
//!
//! sqlx 四个转换器都靠「按值试探」定列型，试探表只有 `bool / i32 / i64 / f32 / f64 /
//! Vec<u8> / String`；`numeric` · `timestamptz` · `date` · `time` · `uuid` · `jsonb`
//! 一个都不在表里 —— 试探全失败就整列声明 `Utf8`，解码再失败就给 `None`，
//! 出口层看来是「这格就是 NULL」，与真的 NULL 不可区分（内核 2026-09-21 真机实测
//! PG 19 列 / MySQL 14 列全部静默变 NULL）。
//!
//! 所以这里沿用内核 `sqlx_cell_as_text!` 的**同一条试探链**：先 `String`，再精确数值、
//! 时间族、UUID、JSON，最后把字节 lossy 成文本。区别只有一处：时间按**协议 `Page`
//! 能解析的写法**输出（`page::parse_timestamp_micros` 认 RFC3339 / `%Y-%m-%d %H:%M:%S%.f`
//! / `%Y-%m-%dT%H:%M:%S%.f`），内核那份进 Arrow 不经这两条解析，写成 `+00` 也行，
//! 这里写成 `+00` 会被判成 NULL —— 那是「时间列静默变 NULL」的另一种形态。
//!
//! **D2 期间与内核重复是已知取舍**：D3 删掉内核 sqlx 实现时，内核那份随之消失，
//! 不会长期并存两份。
//!
//! ## 精度口径（协议面缺口，R1 不扩协议）
//!
//! `DECIMAL` / `NUMERIC` / `MONEY` 映射到 **`Canonical::Text`**，原样保留
//! `BigDecimal::to_string()` 的每一位。协议白名单里没有 Decimal 物理类型，映射到
//! `Float` 会把 `numeric(38,10)` 静默舍成 f64 —— 那是「看起来像数据错误的显示错误」，
//! 比「这一列按文本显示」糟得多。协议若将来加 `DECIMAL` canonical（R2），这里改回数值。

use rds_driver_sidecar::Canonical;
use rds_driver_sidecar::DriverError;

/// 把一个单元格解成展示文本；`None` 表示**这个驱动确实解不出来**（自定义类型 /
/// 几何 / 网络地址…），调用方不能把它悄悄当 NULL。
macro_rules! cell_as_text {
    ($row:expr, $idx:expr) => {{
        use sqlx::Row as _;
        let row = $row;
        let idx = $idx;
        if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
            Some(v)
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::BigDecimal>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(idx) {
            // 整数族必须显式列出来。真机实测（2026-11-12）：`COUNT(*)` 是 BIGINT，
            // 试探链里没有整数分支时整格解不出来，出口层只能写「<无法解码：BIGINT>」——
            // 「库里有值、界面看不懂」。无符号在 MySQL 侧另有前置分支（见
            // `mysql_cell_as_text!`），公共链不能写 `u64`：sqlx-postgres 没有它的 `Type`。
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<i32>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<i16>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<i8>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<f32>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<bool>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) =
            row.try_get::<Option<sqlx::types::chrono::NaiveDateTime>, _>(idx)
        {
            Some(v.format("%Y-%m-%d %H:%M:%S%.f").to_string())
        } else if let Ok(Some(v)) =
            row.try_get::<Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, _>(idx)
        {
            // RFC3339 形态：`Page` 的 `parse_timestamp_micros` 先按 RFC3339 解析。
            Some(v.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::chrono::NaiveDate>, _>(idx) {
            Some(v.format("%Y-%m-%d").to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::chrono::NaiveTime>, _>(idx) {
            Some(v.format("%H:%M:%S%.f").to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::Uuid>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) =
            row.try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>(idx)
        {
            Some(v.0.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<Vec<u8>>, _>(idx) {
            Some(String::from_utf8_lossy(&v).into_owned())
        } else {
            None
        }
    }};
}

pub(crate) use cell_as_text;

/// MySQL 专用：`BIGINT UNSIGNED` 先按 `u64` 试一次，再走公共链。
///
/// 为什么不能塞进公共链：`sqlx-postgres` **没有** `u64`/`u32`/`u16`/`u8` 的 `Type`
/// 实现，公共链里写 `try_get::<Option<u64>>` 会直接编译失败
/// （`trait bound u64: sqlx::Type<Postgres> is not satisfied`）。所以无符号只在 MySQL
/// 侧开一条前置分支：`COUNT(*)`、`BIGINT UNSIGNED` 列在真机上就是这么解出来的。
macro_rules! mysql_cell_as_text {
    ($row:expr, $idx:expr) => {{
        use sqlx::Row as _;
        if let Ok(Some(v)) = $row.try_get::<Option<u64>, _>($idx) {
            Some(v.to_string())
        } else {
            $crate::values::cell_as_text!($row, $idx)
        }
    }};
}

pub(crate) use mysql_cell_as_text;

/// 该单元格在**协议层**是不是 NULL —— 与「类型解不出来」区分开。
pub(crate) fn cell_is_null<R: sqlx::Row>(row: &R, idx: usize) -> bool
where
    usize: sqlx::ColumnIndex<R>,
{
    use sqlx::ValueRef as _;
    row.try_get_raw(idx)
        .map(|value| value.is_null())
        .unwrap_or(true)
}

/// MySQL 的类型名 → 协议白名单里的逻辑类型。`type_info().name()` 给的是
/// `INT` / `BIGINT UNSIGNED` / `VARCHAR` / `DECIMAL` 这类大写基名。
pub fn mysql_canonical(raw: &str) -> Canonical {
    let upper = raw.trim().to_ascii_uppercase();
    let base = upper
        .split(|ch: char| ch == '(' || ch == ' ' || ch == '[')
        .next()
        .unwrap_or("");
    match base {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT" | "YEAR" => {
            Canonical::Integer
        }
        "FLOAT" | "DOUBLE" | "REAL" => Canonical::Float,
        // 精确十进制：走文本保住每一位（见模块头「精度口径」）。
        "DECIMAL" | "NUMERIC" => Canonical::Text,
        "BIT" | "BOOL" | "BOOLEAN" => Canonical::Boolean,
        "DATE" => Canonical::Date,
        "TIME" => Canonical::Time,
        "DATETIME" | "TIMESTAMP" => Canonical::Timestamp,
        "JSON" => Canonical::Json,
        "BINARY" | "VARBINARY" | "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            Canonical::Binary
        }
        _ => Canonical::Text,
    }
}

/// PostgreSQL 的类型名 → 逻辑类型。`PgTypeInfo::name()` 给的是 `INT4` / `NUMERIC` /
/// `TIMESTAMPTZ` / `BYTEA` / `_INT4`（数组）这类。
pub fn pg_canonical(raw: &str) -> Canonical {
    let upper = raw.trim().to_ascii_uppercase();
    match upper.as_str() {
        "INT2" | "INT4" | "INT8" | "OID" => Canonical::Integer,
        "FLOAT4" | "FLOAT8" => Canonical::Float,
        "NUMERIC" | "MONEY" => Canonical::Text,
        "BOOL" => Canonical::Boolean,
        "DATE" => Canonical::Date,
        "TIME" => Canonical::Time,
        "TIMESTAMP" | "TIMESTAMPTZ" => Canonical::Timestamp,
        "JSON" | "JSONB" => Canonical::Json,
        "BYTEA" => Canonical::Binary,
        // uuid / inet / cidr / macaddr / interval / 数组 / 几何 / 枚举 / 自定义类型：
        // 文本展示，不猜数值、不丢字。
        _ => Canonical::Text,
    }
}

/// sqlx 错误 → 协议错误：SQL 错误带 sqlstate（宿主按它区分「语法错 / 对象不存在」）。
///
/// `as_database_error` 只对服务端回的错成立；连接层错误（io / tls / closed）按
/// `sql_error` 报（宿主侧会看到 `-32003`，与内核 sqlx 驱动同口径）。
pub(crate) fn map_error(err: sqlx::Error) -> DriverError {
    let sqlstate = err
        .as_database_error()
        .and_then(|db| db.code())
        .map(|code| code.to_string());
    let message = err
        .as_database_error()
        .map(|db| db.message().to_string())
        .unwrap_or_else(|| err.to_string());
    let mut mapped = DriverError::sql(friendly_message(message));
    if let Some(state) = sqlstate {
        mapped = mapped.with_sqlstate(state);
    }
    mapped
}

/// PostgreSQL 服务端 `lc_messages` 不是 UTF-8 时，错误消息体是**服务端本地编码**，
/// sqlx 解不出来，只能报「Postgres returned a non-UTF-8 string for its error message」——
/// 真实原因（最常见就是认证失败）被整条吞掉，界面上只剩一句协议层抱怨。
///
/// 真机实测：`192.168.3.138:5432` 用错口令时就是这样（`sslmode=require` 才进得到认证阶段）。
/// 这里在保留原文的同时补一句「往哪看」，否则用户拿到的信息量为零。
pub(crate) fn friendly_message(message: String) -> String {
    if message.contains("non-UTF-8 string") {
        return format!(
            "{message}（PostgreSQL 服务端 lc_messages 不是 UTF-8，错误内容无法解码；\
             最常见的原因是认证失败，真实原因见服务端日志）"
        );
    }
    message
}

/// 这条语句会不会返回行集（决定走 fetch 还是走 execute 拿 `affected_rows`）。
/// 只是**入口分流**，判定保守偏「返回行」：判错的代价是多一次空行集（界面上是
/// 0 行 + 列头），而不是把 INSERT 的结果当查询展示。
pub fn returns_rows(sql: &str) -> bool {
    let keyword = first_keyword(sql);
    matches!(
        keyword.as_str(),
        "SELECT" | "WITH" | "SHOW" | "VALUES" | "EXPLAIN" | "DESCRIBE" | "DESC" | "TABLE"
            | "CALL"
    )
}

/// 取第一个「真」关键字：跳过空白、`--` 行注释、`/* */` 块注释与开头的 `(`。
fn first_keyword(sql: &str) -> String {
    let mut rest = sql.trim_start();
    loop {
        if let Some(stripped) = rest.strip_prefix("--") {
            rest = match stripped.find('\n') {
                Some(idx) => &stripped[idx + 1..],
                None => "",
            };
            rest = rest.trim_start();
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("/*") {
            rest = match stripped.find("*/") {
                Some(idx) => &stripped[idx + 2..],
                None => "",
            };
            rest = rest.trim_start();
            continue;
        }
        break;
    }
    let rest = rest.trim_start_matches('(').trim_start();
    let word: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    word.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_type_names_map_to_protocol_canonicals() {
        assert_eq!(mysql_canonical("INT"), Canonical::Integer);
        assert_eq!(mysql_canonical("BIGINT UNSIGNED"), Canonical::Integer);
        assert_eq!(mysql_canonical("TINYINT"), Canonical::Integer);
        assert_eq!(mysql_canonical("DOUBLE"), Canonical::Float);
        // 精确十进制走文本，不进浮点。
        assert_eq!(mysql_canonical("DECIMAL"), Canonical::Text);
        assert_eq!(mysql_canonical("DECIMAL(10,2)"), Canonical::Text);
        assert_eq!(mysql_canonical("DATETIME"), Canonical::Timestamp);
        // `DATE` 不能被 `DATETIME` 的 contains 逻辑吃掉。
        assert_eq!(mysql_canonical("DATE"), Canonical::Date);
        assert_eq!(mysql_canonical("TIME"), Canonical::Time);
        assert_eq!(mysql_canonical("JSON"), Canonical::Json);
        assert_eq!(mysql_canonical("LONGBLOB"), Canonical::Binary);
        assert_eq!(mysql_canonical("VARCHAR"), Canonical::Text);
        assert_eq!(mysql_canonical("BIT"), Canonical::Boolean);
    }

    #[test]
    fn pg_type_names_map_to_protocol_canonicals() {
        assert_eq!(pg_canonical("INT4"), Canonical::Integer);
        assert_eq!(pg_canonical("INT8"), Canonical::Integer);
        assert_eq!(pg_canonical("FLOAT8"), Canonical::Float);
        assert_eq!(pg_canonical("NUMERIC"), Canonical::Text, "numeric 保住精度");
        assert_eq!(pg_canonical("TIMESTAMP"), Canonical::Timestamp);
        assert_eq!(pg_canonical("TIMESTAMPTZ"), Canonical::Timestamp);
        assert_eq!(pg_canonical("BOOL"), Canonical::Boolean);
        assert_eq!(pg_canonical("BYTEA"), Canonical::Binary);
        assert_eq!(pg_canonical("JSONB"), Canonical::Json);
        // 数组 / uuid / inet：文本，不猜。
        assert_eq!(pg_canonical("_INT4"), Canonical::Text);
        assert_eq!(pg_canonical("UUID"), Canonical::Text);
        assert_eq!(pg_canonical("INET"), Canonical::Text);
    }

    /// 精确十进制的**尾零口径**：`numeric(p,4)` 的 `1234.5600` 必须原样出 `1234.5600`，
    /// 不能被抹成 `1234.56`（也不能舍成浮点）。
    ///
    /// 本条从内核 `engine/tests/type_decode_fidelity.rs` 迁来（D3 删掉内核 sqlx 实现后，
    /// 那条 `sqlx_postgres_decodes_...` 用例随之删除；官方驱动走 `dscale` 另有一份断言）。
    /// 这里钉的是「插件把 `BigDecimal` 转展示文本」这一步：sqlx-postgres 解出的
    /// `BigDecimal` 保留了列的小数位，`to_string()` 必须原样带出。
    #[test]
    fn numeric_keeps_trailing_zeros_as_text() {
        let v: sqlx::types::BigDecimal = "1234.5600".parse().expect("合法 numeric");
        assert_eq!(v.to_string(), "1234.5600", "尾零是列精度的一部分，不能抹掉");
        assert_eq!(pg_canonical("NUMERIC(10,4)"), Canonical::Text, "先按文本承载");
    }

    #[test]
    fn returns_rows_skips_comments_and_leading_parens() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("  with t as (select 1) select * from t"));
        assert!(returns_rows("-- 注释\nSELECT 1"));
        assert!(returns_rows("/* 注释 */ SELECT 1"));
        assert!(returns_rows("(SELECT 1)"));
        assert!(returns_rows("SHOW TABLES"));
        assert!(returns_rows("EXPLAIN SELECT 1"));
        assert!(!returns_rows("INSERT INTO t VALUES (1)"));
        assert!(!returns_rows("UPDATE t SET a = 1"));
        assert!(!returns_rows("CREATE TABLE t (id int)"));
        assert!(!returns_rows(""));
    }

    #[test]
    fn non_utf8_server_error_gets_a_hint_instead_of_a_bare_protocol_complaint() {
        let raw = "encountered unexpected or invalid data: Postgres protocol error \
                   (reading ErrorResponse): Postgres returned a non-UTF-8 string for its error message."
            .to_string();
        let friendly = friendly_message(raw);
        assert!(friendly.contains("lc_messages"), "要告诉用户往哪看：{friendly}");
        assert!(
            friendly.contains("认证失败"),
            "最常见的真实原因要写出来：{friendly}"
        );
        // 普通消息原样返回，不能给所有错误都加一行噪音。
        assert_eq!(
            friendly_message("relation \"x\" does not exist".to_string()),
            "relation \"x\" does not exist"
        );
    }
}
