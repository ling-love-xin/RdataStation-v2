use shared::models::QueryResult;
use super::registry::DriverConnectionConfig;
use shared::error::{ConnectionError, CoreError};

/// 把一个**非文本族**的单元格解成展示文本：`numeric` / 时间 / UUID / JSON。
///
/// ## 为什么需要它
///
/// sqlx / tokio-postgres 的四个转换器都靠「按值试探」定列型，试探表只有
/// `bool / i32 / i64 / f32 / f64 / Vec<u8> / String`。`numeric` · `timestamptz` · `date` ·
/// `time` · `uuid` · `jsonb` **一个都不在表里**：试探全失败 → 整列声明 `Utf8` →
/// 解码时又只试 `String` → 再失败 → `.ok().flatten()` 给 `None` → **出口层看到的是
/// 「值就是 NULL」**，与「这格本来就是 NULL」不可区分。
///
/// 真机实测（PG 19 列 / MySQL 14 列）：金额 · 时间 · JSON · 数组 · inet · point · uuid
/// 全部静默变 NULL。
///
/// ## 口径
///
/// * **一律产出文本，不进浮点**：`numeric(10,2)` 走 `BigDecimal::to_string()` 原样保留精度。
///   金额过一遍 `f64` 会变成「看起来像数据错误的显示错误」。
/// * **顺序有意义**：先 `String`（真文本列最快命中）→ 精确数值 → 时间族
///   （`NaiveDateTime` → `DateTime<Utc>` → `NaiveDate` → `NaiveTime`：`timestamptz`
///   只能按带时区解、`timestamp` 只能按无时区解）→ UUID → JSON → 最后才把字节 lossy 成文本。
/// * **写成宏而不是函数**：`sqlx::Row::try_get` 可用的解码类型挂在 `R::Database` 上，
///   泛型函数里无法对「任意 R」成立。宏仍然只有一份定义。
///
/// 返回 `None` 表示这个驱动确实解不出来（自定义类型 / 几何 / 网络地址…）。调用方
/// **不能把它悄悄当 NULL**，要留痕（各转换器里的 `undecodable` 计数 + `warn`）。
#[macro_export]
macro_rules! sqlx_cell_as_text {
    ($row:expr, $idx:expr) => {{
        use sqlx::Row as _;
        let row = $row;
        let idx = $idx;
        if let Ok(Some(v)) = row.try_get::<Option<String>, _>(idx) {
            Some(v)
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::BigDecimal>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::chrono::NaiveDateTime>, _>(idx) {
            Some(v.format("%Y-%m-%d %H:%M:%S%.f").to_string())
        } else if let Ok(Some(v)) =
            row.try_get::<Option<sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>>, _>(idx)
        {
            Some(v.format("%Y-%m-%d %H:%M:%S%.f+00").to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::chrono::NaiveDate>, _>(idx) {
            Some(v.format("%Y-%m-%d").to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::chrono::NaiveTime>, _>(idx) {
            Some(v.format("%H:%M:%S%.f").to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::Uuid>, _>(idx) {
            Some(v.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<sqlx::types::Json<serde_json::Value>>, _>(idx) {
            Some(v.0.to_string())
        } else if let Ok(Some(v)) = row.try_get::<Option<Vec<u8>>, _>(idx) {
            Some(String::from_utf8_lossy(&v).into_owned())
        } else {
            None
        }
    }};
}

/// 该单元格在**协议层**是不是 NULL —— 与「类型解不出来」区分开。
///
/// 为什么需要区分：转换器解不出来时只能往数组里塞 `None`，而 `None` 与真实 SQL NULL
/// 在 Arrow 里长得一模一样。调用方据此决定要不要留痕（`tracing::warn!`），否则
/// 「库里明明有值、界面却是空白格」就成了一个没有来源的现象。
///
/// 泛型只要求 `Row + ColumnIndex`，不涉及 `Decode` / `Type` 约束，因此两种后端都成立。
pub fn sqlx_value_is_null<R: sqlx::Row>(row: &R, index: usize) -> bool
where
    usize: sqlx::ColumnIndex<R>,
{
    use sqlx::ValueRef as _;
    row.try_get_raw(index)
        .map(|value| value.is_null())
        .unwrap_or(true)
}

/// PostgreSQL：列出某表的列（两个 PG 驱动共用）。
///
/// **列序是本模块的契约**（三个 `*_TABLE_DETAIL_SQL` 必须一致，`columns_from_detail_rows` 按它取值）：
/// `0 列名 / 1 类型 / 2 可空(YES|NO) / 3 序号(1 基) / 4 主键标记(PRI|空) / 5 默认值 / 6 注释 / 7 引用表 / 8 引用列 / 9 外键标记(1|空，**含复合外键**)`。
///
/// 一次取齐属性面板要的东西：列名 / 类型 / 可空 / 主键标记 / 默认值 / 注释，
/// 外加 2026-09-21 补的 **`ordinal_position`** 与**单列外键的引用目标**
/// （`referential_constraints` 只含外键，所以不会把主键误认成引用；
/// 复合外键用 `COUNT(*) = 1` 挡掉 —— 那时的配对要按列序做，留给约束分区）。
pub const PG_TABLE_DETAIL_SQL: &str = "\
            SELECT c.column_name, c.data_type, c.is_nullable, c.ordinal_position, \
                   CASE WHEN c.column_name IN (SELECT kcu.column_name \
                             FROM information_schema.table_constraints tc \
                             JOIN information_schema.key_column_usage kcu \
                               ON tc.constraint_name = kcu.constraint_name \
                            WHERE tc.table_schema = $2 AND tc.table_name = $3 \
                              AND tc.constraint_type = 'PRIMARY KEY') \
                        THEN 'PRI' ELSE '' END AS column_key, \
                   c.column_default, \
                   COALESCE(col_description((SELECT oid FROM pg_class WHERE relname = $3), c.ordinal_position), '') AS column_comment, \
                   COALESCE((SELECT ccu.table_name \
                               FROM information_schema.referential_constraints rc \
                               JOIN information_schema.key_column_usage kcu \
                                 ON kcu.constraint_schema = rc.constraint_schema AND kcu.constraint_name = rc.constraint_name \
                               JOIN information_schema.constraint_column_usage ccu \
                                 ON ccu.constraint_schema = rc.unique_constraint_schema AND ccu.constraint_name = rc.unique_constraint_name \
                              WHERE kcu.table_schema = $2 AND kcu.table_name = $3 AND kcu.column_name = c.column_name \
                                AND (SELECT COUNT(*) FROM information_schema.key_column_usage k2 \
                                      WHERE k2.constraint_schema = kcu.constraint_schema \
                                        AND k2.constraint_name = kcu.constraint_name) = 1 \
                              LIMIT 1), '') AS ref_table, \
                   COALESCE((SELECT ccu.column_name \
                               FROM information_schema.referential_constraints rc \
                               JOIN information_schema.key_column_usage kcu \
                                 ON kcu.constraint_schema = rc.constraint_schema AND kcu.constraint_name = rc.constraint_name \
                               JOIN information_schema.constraint_column_usage ccu \
                                 ON ccu.constraint_schema = rc.unique_constraint_schema AND ccu.constraint_name = rc.unique_constraint_name \
                              WHERE kcu.table_schema = $2 AND kcu.table_name = $3 AND kcu.column_name = c.column_name \
                                AND (SELECT COUNT(*) FROM information_schema.key_column_usage k2 \
                                      WHERE k2.constraint_schema = kcu.constraint_schema \
                                        AND k2.constraint_name = kcu.constraint_name) = 1 \
                              LIMIT 1), '') AS ref_column, \
                   CASE WHEN EXISTS (SELECT 1 \
                                       FROM information_schema.table_constraints tc \
                                       JOIN information_schema.key_column_usage kcu \
                                         ON kcu.constraint_name = tc.constraint_name \
                                      WHERE tc.table_schema = $2 AND tc.table_name = $3 \
                                        AND tc.constraint_type = 'FOREIGN KEY' \
                                        AND kcu.column_name = c.column_name) \
                        THEN '1' ELSE '' END AS is_fk \
              FROM information_schema.columns c \
             WHERE c.table_catalog = $1 AND c.table_schema = $2 AND c.table_name = $3 \
             ORDER BY c.ordinal_position";

/// MySQL：列出某表的列（两个 MySQL 驱动共用）。
///
/// 列序契约见 [`PG_TABLE_DETAIL_SQL`]。
///
/// 与 PG 的差别：`key_column_usage` 自己就有 `referenced_table_name` /
/// `referenced_column_name`，不必绕 `referential_constraints` 那圈 JOIN；
/// 复合外键同样用「本约束只涉及一列」挡掉 —— **与 PG 同口径**，所以四个网络驱动
/// 交出来的 `references` 语义一致。
pub const MY_TABLE_DETAIL_SQL: &str = "\
            SELECT c.column_name, c.data_type, c.is_nullable, c.ordinal_position, c.column_key, \
                   c.column_default, c.column_comment, \
                   COALESCE((SELECT kcu.referenced_table_name \
                               FROM information_schema.key_column_usage kcu \
                              WHERE kcu.table_schema = c.table_schema AND kcu.table_name = c.table_name \
                                AND kcu.column_name = c.column_name AND kcu.referenced_table_name IS NOT NULL \
                                AND (SELECT COUNT(*) FROM information_schema.key_column_usage k2 \
                                      WHERE k2.constraint_schema = kcu.constraint_schema \
                                        AND k2.constraint_name = kcu.constraint_name) = 1 \
                              LIMIT 1), '') AS ref_table, \
                   COALESCE((SELECT kcu.referenced_column_name \
                               FROM information_schema.key_column_usage kcu \
                              WHERE kcu.table_schema = c.table_schema AND kcu.table_name = c.table_name \
                                AND kcu.column_name = c.column_name AND kcu.referenced_column_name IS NOT NULL \
                                AND (SELECT COUNT(*) FROM information_schema.key_column_usage k2 \
                                      WHERE k2.constraint_schema = kcu.constraint_schema \
                                        AND k2.constraint_name = kcu.constraint_name) = 1 \
                              LIMIT 1), '') AS ref_column, \
                   CASE WHEN EXISTS (SELECT 1 FROM information_schema.key_column_usage kcu \
                                      WHERE kcu.table_schema = c.table_schema \
                                        AND kcu.table_name = c.table_name \
                                        AND kcu.column_name = c.column_name \
                                        AND kcu.referenced_table_name IS NOT NULL) \
                        THEN '1' ELSE '' END AS is_fk \
              FROM information_schema.columns c \
             WHERE c.table_schema = ? AND c.table_name = ? \
             ORDER BY c.ordinal_position";

/// DuckDB：列出某表的列。
///
/// 列序契约见 [`PG_TABLE_DETAIL_SQL`]。三处 DuckDB 专有（都是真机探针问出来的）：
///
/// * 注释列在 `information_schema.columns` 里叫 **`COLUMN_COMMENT`**（不叫 `comment`，
///   写 `c.comment` 直接是 `Binder Error`）；
/// * 主键只存在于 `duckdb_constraints()`，且列名列是**列表**，所以用 `list_contains` 判在不在；
/// * 外键目标列也是列表，必须 `array_to_string` 化开（`row.get::<String>` 对列表列报
///   `Invalid column type List`）。
///
/// 复合外键用 `len(...) = 1` 挡掉（同 PG / MySQL 口径）。
pub const DUCK_TABLE_DETAIL_SQL: &str = "\
            SELECT c.column_name, c.data_type, c.is_nullable, c.ordinal_position, \
                   CASE WHEN EXISTS (SELECT 1 FROM duckdb_constraints() k \
                                      WHERE k.table_name = c.table_name \
                                        AND k.constraint_type = 'PRIMARY KEY' \
                                        AND list_contains(k.constraint_column_names, c.column_name)) \
                        THEN 'PRI' ELSE '' END AS column_key, \
                   c.column_default, COALESCE(c.column_comment, '') AS column_comment, \
                   COALESCE((SELECT k.referenced_table FROM duckdb_constraints() k \
                              WHERE k.table_name = c.table_name AND k.constraint_type = 'FOREIGN KEY' \
                                AND list_contains(k.constraint_column_names, c.column_name) \
                                AND len(k.constraint_column_names) = 1 LIMIT 1), '') AS ref_table, \
                   COALESCE((SELECT array_to_string(k.referenced_column_names, ',') \
                               FROM duckdb_constraints() k \
                              WHERE k.table_name = c.table_name AND k.constraint_type = 'FOREIGN KEY' \
                                AND list_contains(k.constraint_column_names, c.column_name) \
                                AND len(k.constraint_column_names) = 1 LIMIT 1), '') AS ref_column, \
                   CASE WHEN EXISTS (SELECT 1 FROM duckdb_constraints() k \
                                      WHERE k.table_name = c.table_name \
                                        AND k.constraint_type = 'FOREIGN KEY' \
                                        AND list_contains(k.constraint_column_names, c.column_name)) \
                        THEN '1' ELSE '' END AS is_fk \
              FROM information_schema.columns c \
             WHERE c.table_schema = 'main' AND c.table_name = ? \
             ORDER BY c.ordinal_position";

/// 按**约定列序**（见 [`PG_TABLE_DETAIL_SQL`]）把表详情的结果读成列详情。
///
/// 四个网络驱动 + DuckDB 的表详情都走这里：映射只写一份，`references` / `ordinal` 的
/// 语义就不会因为「抄到第三个驱动时改了半行」而分叉（本仓已经有过七份
/// `is_read_only_sql` 四种口径的先例）。
///
/// 行短于契约时缺的单元格按空串处理：不 panic、也不吞行。
pub fn columns_from_detail_rows(result: &QueryResult) -> Vec<crate::driver::ColumnDetail> {
    batch_to_string_rows(result)
        .into_iter()
        .map(|r| {
            let cell = |i: usize| r.get(i).map_or("", |s| s.as_str());
            let ref_table = cell(7);
            // 「有没有外键」与「引到哪」分开读：复合外键的目标本查询不猜（见各 SQL
            // 的说明），但那一列**属于外键**这件事要如实标出来。
            let fk_flag = cell(9);
            crate::driver::ColumnDetail {
                name: cell(0).to_string(),
                data_type: cell(1).to_string(),
                nullable: cell(2) == "YES",
                ordinal: cell(3).parse().unwrap_or(0),
                is_primary_key: cell(4) == "PRI",
                default_value: (!cell(5).is_empty()).then(|| cell(5).to_string()),
                comment: (!cell(6).is_empty()).then(|| cell(6).to_string()),
                is_foreign_key: fk_flag == "1" || !ref_table.is_empty(),
                references: (!ref_table.is_empty()).then(|| crate::driver::ForeignKeyRef {
                    table: ref_table.to_string(),
                    column: cell(8).to_string(),
                }),
                extra: std::collections::HashMap::new(),
            }
        })
        .collect()
}

/// MySQL：列出某表的约束（两个 MySQL 驱动共用，勿抄第二份）。
///
/// 三张 `information_schema` 表各管一段：`TABLE_CONSTRAINTS`（有哪些约束）→
/// `KEY_COLUMN_USAGE`（涉及哪些列、引用谁）→ `REFERENTIAL_CONSTRAINTS`（更新 / 删除
/// 规则）。`GROUP_CONCAT(... ORDER BY ORDINAL_POSITION)` 把组合约束的列按**声明顺序**
/// 拼回一个字符串 —— 组合主键的顺序有意义，不能靠聚合的默认顺序。
pub const MY_LIST_CONSTRAINTS_SQL: &str = "\
            SELECT tc.CONSTRAINT_NAME AS cname, tc.CONSTRAINT_TYPE AS ctype, \
                   COALESCE(GROUP_CONCAT(kcu.COLUMN_NAME ORDER BY kcu.ORDINAL_POSITION), '') AS cols, \
                   COALESCE(kcu.REFERENCED_TABLE_NAME, '') AS ref_table, \
                   COALESCE(GROUP_CONCAT(kcu.REFERENCED_COLUMN_NAME ORDER BY kcu.ORDINAL_POSITION), '') AS ref_cols, \
                   COALESCE(rc.UPDATE_RULE, '') AS upd, COALESCE(rc.DELETE_RULE, '') AS del \
              FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
              LEFT JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu \
                ON kcu.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
               AND kcu.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
               AND kcu.TABLE_SCHEMA = tc.TABLE_SCHEMA AND kcu.TABLE_NAME = tc.TABLE_NAME \
              LEFT JOIN INFORMATION_SCHEMA.REFERENTIAL_CONSTRAINTS rc \
                ON rc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA \
               AND rc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME \
             WHERE tc.TABLE_SCHEMA = ? AND tc.TABLE_NAME = ? \
             GROUP BY tc.CONSTRAINT_NAME, tc.CONSTRAINT_TYPE, kcu.REFERENCED_TABLE_NAME, \
                      rc.UPDATE_RULE, rc.DELETE_RULE \
             ORDER BY tc.CONSTRAINT_NAME";

/// PostgreSQL：列出某表的索引（两个 PG 驱动共用，勿抄第二份）。
///
/// 用 `pg_catalog` 而不是 `information_schema`：后者没有 `indisprimary` 与索引方法
/// （btree / hash / gin / gist / brin）。布尔列在 SQL 里 `::text` —— 批读路径按
/// `StringArray` 取，混一个 BooleanArray 会让整列 downcast 落空。
pub const PG_LIST_INDEXES_SQL: &str = "\
            SELECT i.relname AS index_name, \
                   ix.indisunique::text, ix.indisprimary::text, \
                   COALESCE(am.amname, '') AS method, \
                   COALESCE((SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                             FROM unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) \
                             JOIN pg_catalog.pg_attribute a \
                               ON a.attrelid = ix.indrelid AND a.attnum = k.attnum), '') AS cols \
              FROM pg_catalog.pg_index ix \
              JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid \
              JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid \
              JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace \
              LEFT JOIN pg_catalog.pg_am am ON am.oid = i.relam \
             WHERE n.nspname = $1 AND t.relname = $2 \
             ORDER BY i.relname";

/// PostgreSQL：列出某表的约束（两个 PG 驱动共用）。
///
/// `conkey` / `confkey` 是 `int2[]`，用 `unnest ... WITH ORDINALITY` 按**列序**拼回名字
/// （组合主键的顺序有意义，不能靠 `string_agg` 的默认顺序）。`confupdtype` /
/// `confdeltype` 的 `' '`（空格）表示没写 ON UPDATE/DELETE，由 `pg_fk_action` 归 None。
pub const PG_LIST_CONSTRAINTS_SQL: &str = "\
            SELECT c.conname, c.contype::text, \
                   COALESCE((SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                             FROM unnest(c.conkey) WITH ORDINALITY AS k(attnum, ord) \
                             JOIN pg_catalog.pg_attribute a \
                               ON a.attrelid = c.conrelid AND a.attnum = k.attnum), '') AS cols, \
                   COALESCE(rc.relname, '') AS ref_table, \
                   COALESCE((SELECT string_agg(a.attname, ',' ORDER BY k.ord) \
                             FROM unnest(c.confkey) WITH ORDINALITY AS k(attnum, ord) \
                             JOIN pg_catalog.pg_attribute a \
                               ON a.attrelid = c.confrelid AND a.attnum = k.attnum), '') AS ref_cols, \
                   c.confupdtype::text, c.confdeltype::text \
              FROM pg_catalog.pg_constraint c \
              JOIN pg_catalog.pg_class t ON t.oid = c.conrelid \
              JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace \
              LEFT JOIN pg_catalog.pg_class rc ON rc.oid = c.confrelid \
             WHERE n.nspname = $1 AND t.relname = $2 \
             ORDER BY c.conname";

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

/// 逗号分隔的列名 → `Vec<String>`（SQL 侧 `string_agg` 的产物）。
pub fn split_csv(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `pg_constraint.contype` → 展示用名称。
pub fn pg_constraint_kind(code: &str) -> String {
    match code {
        "p" => "PRIMARY KEY",
        "f" => "FOREIGN KEY",
        "u" => "UNIQUE",
        "c" => "CHECK",
        "x" => "EXCLUDE",
        other => other,
    }
    .to_string()
}

/// `confupdtype` / `confdeltype` → 规则名。
///
/// `' '`（空格）是 PG 的「没写 ON UPDATE/DELETE」——**不是空串**，漏了它每个外键
/// 都会多出一对空的规则行。
pub fn pg_fk_action(code: &str) -> Option<String> {
    match code {
        "a" => Some("NO ACTION".to_string()),
        "r" => Some("RESTRICT".to_string()),
        "c" => Some("CASCADE".to_string()),
        "n" => Some("SET NULL".to_string()),
        "d" => Some("SET DEFAULT".to_string()),
        _ => None,
    }
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
