//! 错误文本 / 协议字段 → 「数据库指认的对象」（`shared::error::ErrorLocation`）。
//!
//! ## 为什么要这一层
//!
//! 用户看到的 `UNIQUE constraint failed: orders.tag` 里其实有可操作的信息（表 `orders`、
//! 列 `tag`），但出口是一整段文本：界面照原样糊上去，人得自己从里面认名字。取法照 dbflux
//! 的 `ErrorLocation`（MIT，`dbflux_core::core::error_formatter`）——把对象名**结构化**
//! 带出来。dbflux 那边 PG 走协议字段、MSSQL 走消息解析；我们两边都要：PG 有字段，其它三家
//! 只有文本。
//!
//! ## PG 与其它三家处理方式不同（这是真机逼出来的）
//!
//! * **PostgreSQL**：协议层就有字段（`PgDatabaseError` / `DbError` 的 schema / table /
//!   column / constraint），**与语言无关**。服务端 locale 是中文时消息文本也是中文
//!   （真机实测 `字段 "nope" 不存在` / `关系 "t" 不存在`），靠文本解析会当场失效 ——
//!   所以 PG **只走字段**，不解析文本。
//! * **MySQL / SQLite / DuckDB**：协议层没有这些字段，只有消息文本，所以按**真机抄下来的
//!   样本**解析（见各函数文档里的样本，2026-09-21 采集）。
//!
//! ## 口径（三家一致）
//!
//! * **拿不到就不填**：宁可 `None`，也不猜一个名字出来 —— 猜错的名字比没有更坏。
//! * **复合键只给表、不给列**：列字段是一个名字，多列硬塞必然错（`UNIQUE constraint
//!   failed: t.a, t.b` 只填表）。
//! * **只填「出错对象」**：外键冲突填子表（FK 所在的那张），被引用的父表不填 —— 我们的
//!   结构里没有第二个表的位置。
//! * **表达式不当名字**：SQLite 的 `CHECK constraint failed: <表达式>`（没命名时它打的是
//!   表达式）只在**裸标识符**时才当约束名，否则不填。

use shared::error::ErrorLocation;

/// PostgreSQL（sqlx 驱动）：从协议字段取，**不解析文本**。
pub fn postgres_from_sqlx(err: &sqlx::Error) -> Option<ErrorLocation> {
    let pg = err
        .as_database_error()?
        .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()?;
    from_pg_fields(pg.schema(), pg.table(), pg.column(), pg.constraint())
}

/// PostgreSQL（Official 驱动）：同上，走 `tokio_postgres::DbError`。
pub fn postgres_from_tokio(err: &tokio_postgres::Error) -> Option<ErrorLocation> {
    let db = err.as_db_error()?;
    from_pg_fields(db.schema(), db.table(), db.column(), db.constraint())
}

fn from_pg_fields(
    schema: Option<&str>,
    table: Option<&str>,
    column: Option<&str>,
    constraint: Option<&str>,
) -> Option<ErrorLocation> {
    let loc = ErrorLocation {
        schema: owned(schema),
        table: owned(table),
        column: owned(column),
        constraint: owned(constraint),
    };
    (!loc.is_empty()).then_some(loc)
}

/// MySQL（两个驱动同一套文案）。真机样本（9.7.2，2026-09-21）：
///
/// ```text
/// 1062 (23000): Duplicate entry 'dup' for key 'rds_err_probe_my.tag'
/// 1062 (23000): Duplicate entry '1' for key 'rds_err_probe_my.PRIMARY'
/// 1048 (23000): Column 'tag' cannot be null
/// 1364 (HY000): Field 'tag' doesn't have a default value
/// 1054 (42S22): Unknown column 'nope' in 'field list'
/// 3819 (HY000): Check constraint 'rds_err_probe_my_chk_1' is violated.
/// 1146 (42S02): Table 'mysql.rds_no_such_table_xyz' doesn't exist
/// 1452 (23000): Cannot add or update a child row: a foreign key constraint fails
///   (`mysql`.`rds_err_probe_myn`, CONSTRAINT `rds_err_probe_myn_ibfk_1`
///    FOREIGN KEY (`parent_id`) REFERENCES `rds_err_probe_myn_p` (`id`))
/// ```
///
/// 注：`Duplicate entry` 的 `for key` 给的是**索引名**（主键时是 `tbl.PRIMARY`），
/// 复合唯一索引报的是索引名而不是「哪几列」，所以那里只填表与约束。
pub fn mysql(message: &str) -> Option<ErrorLocation> {
    // 外键：子表（限定名）+ 约束名 + 「单列时」的列名
    if let Some(at) = message.find("a foreign key constraint fails (") {
        let rest = &message[at..];
        let mut loc = ErrorLocation::new();
        if let Some(from) = rest.find('`') {
            let to = rest[from..].find(',').map_or(rest.len(), |i| i + from);
            let (schema, table) = split_qualified(&rest[from..to]);
            loc.schema = schema;
            loc.table = non_empty(table);
        }
        if let Some(name) = between(rest, 0, "CONSTRAINT `", "`") {
            loc.constraint = non_empty(name.to_string());
        }
        // 复合外键的列列表有多个 —— 按口径只给表与约束
        if let Some(cols) = between(rest, 0, "FOREIGN KEY (", ")") {
            let cols = split_list(cols);
            if cols.len() == 1 {
                loc.column = cols.into_iter().next();
            }
        }
        if !loc.is_empty() {
            return Some(loc);
        }
    }

    // `for key 'tbl.index'` —— 表与约束（索引名）
    if let Some(key) = between(message, 0, "for key '", "'") {
        let (table, constraint) = match key.rsplit_once('.') {
            Some((table, constraint)) => {
                (non_empty(unquote(table)), non_empty(unquote(constraint)))
            }
            None => (None, non_empty(unquote(key))),
        };
        let loc = ErrorLocation {
            table,
            constraint,
            ..ErrorLocation::new()
        };
        if !loc.is_empty() {
            return Some(loc);
        }
    }

    // 列：`Field 'x' …` / `Column 'x' …` / `Unknown column 'x' …` / `… for column 'x' …`
    for marker in [
        "Field '",
        "Column '",
        "Unknown column '",
        "Data too long for column '",
        "Out of range value for column '",
    ] {
        if let Some(name) = between(message, 0, marker, "'") {
            if let Some(column) = non_empty(unquote(name)) {
                return Some(ErrorLocation::new().with_column(column));
            }
        }
    }

    // 表：`Table 'db.tbl' doesn't exist` / `Unknown table 'tbl' in …`
    for marker in ["Table '", "Unknown table '"] {
        if let Some(name) = between(message, 0, marker, "'") {
            let (schema, table) = split_qualified(name);
            let loc = ErrorLocation {
                schema,
                table: non_empty(table),
                ..ErrorLocation::new()
            };
            if !loc.is_empty() {
                return Some(loc);
            }
        }
    }

    // `Check constraint 'name' is violated.`
    if let Some(name) = between(message, 0, "Check constraint '", "'") {
        if let Some(constraint) = non_empty(unquote(name)) {
            return Some(ErrorLocation::new().with_constraint(constraint));
        }
    }

    None
}

/// SQLite。真机样本（`T.fossil`，2026-09-21）：
///
/// ```text
/// UNIQUE constraint failed: rds_err_probe_sq.tag
/// UNIQUE constraint failed: rds_err_probe_sq.code, rds_err_probe_sq.code2
/// NOT NULL constraint failed: rds_err_probe_sq.tag
/// table rds_err_probe_sq has no column named nope
/// CHECK constraint failed: code IS NULL OR code <> 'bad'
/// FOREIGN KEY constraint failed                        ← 不点名（拿不到就是拿不到）
/// no such column: nope / no such table: rds_no_such_table_xyz
/// duplicate column name: x / ambiguous column name: x
/// ```
pub fn sqlite(message: &str) -> Option<ErrorLocation> {
    // `UNIQUE constraint failed: t.c` / `NOT NULL constraint failed: t.c`（`t` 是表，`c` 是列）
    for marker in ["UNIQUE constraint failed: ", "NOT NULL constraint failed: "] {
        if let Some(body) = tail_after(message, marker) {
            let cols = split_list(body);
            let mut loc = ErrorLocation::new();
            if let Some(first) = cols.first() {
                // 这里是 `表.列`（不是 `schema.表`）
                let (table, _) = first.split_once('.').unwrap_or((first.as_str(), ""));
                loc.table = non_empty(table.to_string());
            }
            if cols.len() == 1 {
                if let Some((_, column)) = cols[0].split_once('.') {
                    loc.column = non_empty(column.to_string());
                }
            }
            if !loc.is_empty() {
                return Some(loc);
            }
        }
    }

    // `table t has no column named c`
    if let Some(body) = tail_after(message, "table ") {
        if let Some((table, column)) = body.split_once(" has no column named ") {
            return Some(
                ErrorLocation::new()
                    .with_table(table.trim().to_string())
                    .with_column(column.trim().to_string()),
            );
        }
    }

    // `CHECK constraint failed: X` —— 有名字时是裸标识符，没名字时 SQLite 打的是**表达式**，
    // 表达式当约束名写出去是错的（`code IS NULL OR …`），所以只认裸标识符。
    if let Some(body) = tail_after(message, "CHECK constraint failed: ") {
        let body = body.trim();
        if is_bare_identifier(body) {
            return Some(ErrorLocation::new().with_constraint(body.to_string()));
        }
    }

    // `no such column: c`（可能带表前缀）/ `no such table: t`
    if let Some(body) = tail_after(message, "no such column: ") {
        let mut loc = ErrorLocation::new();
        match body.trim().split_once('.') {
            Some((table, column)) => {
                loc.table = non_empty(table.to_string());
                loc.column = non_empty(column.to_string());
            }
            None => loc.column = non_empty(body.trim().to_string()),
        }
        if !loc.is_empty() {
            return Some(loc);
        }
    }
    if let Some(body) = tail_after(message, "no such table: ") {
        if let Some(table) = non_empty(body.trim().to_string()) {
            return Some(ErrorLocation::new().with_table(table));
        }
    }
    for marker in ["duplicate column name: ", "ambiguous column name: "] {
        if let Some(body) = tail_after(message, marker) {
            if let Some(column) = non_empty(body.trim().to_string()) {
                return Some(ErrorLocation::new().with_column(column));
            }
        }
    }

    None
}

/// DuckDB。真机样本（`D:\data\123`，2026-09-21）：
///
/// ```text
/// Constraint Error: NOT NULL constraint failed: rds_err_probe_dd.tag
/// Constraint Error: Duplicate key "id: 1" violates primary key constraint.
/// Constraint Error: Duplicate key "tag: dup" violates unique constraint.        ← 唯一也不点名
/// Constraint Error: CHECK constraint failed on table rds_err_probe_dd with expression CHECK(...)
/// Constraint Error: Violates foreign key constraint because key "id: 999" does not exist
///                   in the referenced table                                 ← 不点表名
/// Binder Error: Table "rds_err_probe_dd" does not have a column with name "nope"
/// Binder Error: Referenced column "nope" not found in FROM clause!
/// Catalog Error: Table with name rds_no_such_table_xyz does not exist!
/// ```
///
/// `Duplicate key "id: 1"` 的引号里是「列: 值」的列表 —— 单列时能拿到列名（复合键只给表，
/// 而这里连表名都没有，所以复合键时什么都不填）。外键冲突**不点名**（既没表也没列）。
pub fn duckdb(message: &str) -> Option<ErrorLocation> {
    // `NOT NULL constraint failed: t.c`（与 SQLite 同形状）
    if let Some(body) = tail_after(message, "NOT NULL constraint failed: ") {
        if let Some((table, column)) = body.trim().split_once('.') {
            if !table.is_empty() && !column.is_empty() {
                return Some(ErrorLocation::new().with_table(table).with_column(column));
            }
        }
    }

    // `Duplicate key "col: value" violates …`
    if let Some(body) = tail_after(message, "Duplicate key \"") {
        if let Some(keys) = body.split('"').next() {
            let cols: Vec<String> = keys
                .split(',')
                .filter_map(|pair| pair.split(':').next())
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if cols.len() == 1 {
                return Some(ErrorLocation::new().with_column(cols[0].clone()));
            }
        }
    }

    // `CHECK constraint failed on table t with expression …`（名字不给，只给表）
    if let Some(table) = between(
        message,
        0,
        "CHECK constraint failed on table ",
        " with expression",
    ) {
        if let Some(table) = non_empty(table.trim().to_string()) {
            return Some(ErrorLocation::new().with_table(table));
        }
    }

    // `Table "t" does not have a column with name "c"` / `… named "c"`
    if message.contains("does not have a column") {
        let table = between(message, 0, "Table \"", "\"");
        let column = between(message, 0, " name \"", "\"");
        let loc = ErrorLocation {
            table: table.and_then(|t| non_empty(t.to_string())),
            column: column.and_then(|c| non_empty(c.to_string())),
            ..ErrorLocation::new()
        };
        if !loc.is_empty() {
            return Some(loc);
        }
    }

    // `Referenced column "c" not found in FROM clause!`
    if let Some(column) = between(message, 0, "Referenced column \"", "\"") {
        if let Some(column) = non_empty(column.to_string()) {
            return Some(ErrorLocation::new().with_column(column));
        }
    }

    // `Table with name t does not exist!` / `Table "t" does not exist`
    if let Some(table) = between(message, 0, "Table with name ", " does not exist") {
        if let Some(table) = non_empty(table.trim().to_string()) {
            return Some(ErrorLocation::new().with_table(table));
        }
    }
    if let Some(table) = between(message, 0, "Table \"", "\" does not exist") {
        if let Some(table) = non_empty(table.to_string()) {
            return Some(ErrorLocation::new().with_table(table));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn owned(value: Option<&str>) -> Option<String> {
    value.filter(|s| !s.is_empty()).map(str::to_string)
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// 去掉标识符外面的引号：`` `x` `` / `"x"` / `'x'` / `[x]`。
fn unquote(raw: &str) -> String {
    let t = raw.trim();
    let bytes = t.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if matches!(
            (first, last),
            (b'`', b'`') | (b'"', b'"') | (b'\'', b'\'') | (b'[', b']')
        ) {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

/// `schema.tbl` → `(Some(schema), tbl)`；`tbl` → `(None, tbl)`。
///
/// **逐段**去引号：`` `mysql`.`tbl` `` 整串剥一对会剥成 `` mysql`.`tbl ``（表的尾引号被
/// 当成了配对），于是 schema 变成 `` mysql` ``。（这条是单测抓出来的。）
fn split_qualified(raw: &str) -> (Option<String>, String) {
    let t = raw.trim();
    match t.rsplit_once('.') {
        Some((head, tail)) => (non_empty(unquote(head)), unquote(tail)),
        None => (None, unquote(t)),
    }
}

/// `marker` 之后的内容（到字符串结尾）。
fn tail_after<'a>(haystack: &'a str, marker: &str) -> Option<&'a str> {
    let at = haystack.find(marker)? + marker.len();
    // rusqlite 的 `Display` 会把 ` in {sql} at offset {n}` 拼在消息后面 —— 那不是消息的
    // 一部分。真机踩到过：列名被读成 `nope in SELECT nope FROM t at offset 7`。
    let rest = &haystack[at..];
    let end = rest
        .find(" in ")
        .into_iter()
        .chain(rest.find(" at offset "))
        .min()
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `open`..`close` 之间的内容（从 `start` 起找 `open`）。
fn between<'a>(haystack: &'a str, start: usize, open: &str, close: &str) -> Option<&'a str> {
    let from = haystack[start..].find(open)? + start + open.len();
    let to = haystack[from..].find(close)? + from;
    Some(&haystack[from..to])
}

/// 逗号切分并去掉引号（列名列表）。
fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(unquote)
        .filter(|s| !s.is_empty())
        .collect()
}

/// 裸标识符（`ck_name` 这类）；SQLite 的 CHECK 表达式不会是它。
fn is_bare_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && text.chars().next().is_some_and(|c| !c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(
        schema: Option<&str>,
        table: Option<&str>,
        column: Option<&str>,
        constraint: Option<&str>,
    ) -> ErrorLocation {
        ErrorLocation {
            schema: schema.map(str::to_string),
            table: table.map(str::to_string),
            column: column.map(str::to_string),
            constraint: constraint.map(str::to_string),
        }
    }

    /// MySQL：样本逐字抄自 2026-09-21 真机（9.7.2；两个驱动的文案同一套 ——
    /// sqlx 那种前面多一段 `error returned from database: `，Official 那种包一层
    /// `Server error: `…``，解析按「在文本里找形态」，前缀不影响）。
    #[test]
    fn mysql_reads_real_samples() {
        let cases: &[(&str, ErrorLocation)] = &[
            (
                "error returned from database: 1062 (23000): Duplicate entry 'dup' for key 'rds_err_probe_my.tag'",
                loc(None, Some("rds_err_probe_my"), None, Some("tag")),
            ),
            (
                "error returned from database: 1062 (23000): Duplicate entry '1' for key 'rds_err_probe_my.PRIMARY'",
                loc(None, Some("rds_err_probe_my"), None, Some("PRIMARY")),
            ),
            (
                "Server error: `ERROR 1048 (23000): Column 'tag' cannot be null'",
                loc(None, None, Some("tag"), None),
            ),
            (
                "error returned from database: 1364 (HY000): Field 'tag' doesn't have a default value",
                loc(None, None, Some("tag"), None),
            ),
            (
                "error returned from database: 1054 (42S22): Unknown column 'nope' in 'field list'",
                loc(None, None, Some("nope"), None),
            ),
            (
                "error returned from database: 3819 (HY000): Check constraint 'rds_err_probe_my_chk_1' is violated.",
                loc(None, None, None, Some("rds_err_probe_my_chk_1")),
            ),
            (
                "error returned from database: 1146 (42S02): Table 'mysql.rds_no_such_table_xyz' doesn't exist",
                loc(Some("mysql"), Some("rds_no_such_table_xyz"), None, None),
            ),
            (
                "Server error: `ERROR 1452 (23000): Cannot add or update a child row: a foreign key constraint fails (`mysql`.`rds_err_probe_myn`, CONSTRAINT `rds_err_probe_myn_ibfk_1` FOREIGN KEY (`parent_id`) REFERENCES `rds_err_probe_myn_p` (`id`))'",
                loc(
                    Some("mysql"),
                    Some("rds_err_probe_myn"),
                    Some("parent_id"),
                    Some("rds_err_probe_myn_ibfk_1"),
                ),
            ),
            // 复合外键：**构造样本**（真机那次是单列外键）。按口径只给表与约束，不给列。
            (
                "Server error: `ERROR 1452 (23000): Cannot add or update a child row: a foreign key constraint fails (`db`.`child`, CONSTRAINT `fk_two` FOREIGN KEY (`a`, `b`) REFERENCES `parent` (`x`, `y`))'",
                loc(Some("db"), Some("child"), None, Some("fk_two")),
            ),
            // 语法错：没有对象可指认
            (
                "error returned from database: 1064 (42000): You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near 'SELEC 1' at line 1",
                loc(None, None, None, None),
            ),
        ];
        for (msg, want) in cases {
            let got = mysql(msg);
            if want.is_empty() {
                assert_eq!(got, None, "样本不该给出位置：{msg}");
            } else {
                assert_eq!(got.as_ref(), Some(want), "样本：{msg}");
            }
        }
    }

    /// SQLite：样本逐字抄自 2026-09-21 真机（`T.fossil`）。
    #[test]
    fn sqlite_reads_real_samples() {
        let cases: &[(&str, ErrorLocation)] = &[
            (
                "UNIQUE constraint failed: rds_err_probe_sq.tag",
                loc(None, Some("rds_err_probe_sq"), Some("tag"), None),
            ),
            // 复合唯一：只给表（列字段塞不下两个名字）
            (
                "UNIQUE constraint failed: rds_err_probe_sq.code, rds_err_probe_sq.code2",
                loc(None, Some("rds_err_probe_sq"), None, None),
            ),
            (
                "NOT NULL constraint failed: rds_err_probe_sq.tag",
                loc(None, Some("rds_err_probe_sq"), Some("tag"), None),
            ),
            (
                "table rds_err_probe_sq has no column named nope",
                loc(None, Some("rds_err_probe_sq"), Some("nope"), None),
            ),
            (
                "no such column: nope",
                loc(None, None, Some("nope"), None),
            ),
            (
                "no such table: rds_no_such_table_xyz",
                loc(None, Some("rds_no_such_table_xyz"), None, None),
            ),
            ("duplicate column name: c2", loc(None, None, Some("c2"), None)),
            ("ambiguous column name: id", loc(None, None, Some("id"), None)),
            // 没命名的 CHECK：SQLite 打的是**表达式**，不当约束名
            (
                "CHECK constraint failed: code IS NULL OR code <> 'bad'",
                loc(None, None, None, None),
            ),
            // 命名的 CHECK：**构造样本**（按 SQLite 文档：有名字时打名字）
            (
                "CHECK constraint failed: ck_positive",
                loc(None, None, None, Some("ck_positive")),
            ),
            // 外键：SQLite 不点名
            ("FOREIGN KEY constraint failed", loc(None, None, None, None)),
        ];
        for (msg, want) in cases {
            let got = sqlite(msg);
            if want.is_empty() {
                assert_eq!(got, None, "样本不该给出位置：{msg}");
            } else {
                assert_eq!(got.as_ref(), Some(want), "样本：{msg}");
            }
        }
    }

    /// DuckDB：样本逐字抄自 2026-09-21 真机（`D:\data\123`）。
    #[test]
    fn duckdb_reads_real_samples() {
        let cases: &[(&str, ErrorLocation)] = &[
            (
                "Constraint Error: NOT NULL constraint failed: rds_err_probe_dd.tag",
                loc(None, Some("rds_err_probe_dd"), Some("tag"), None),
            ),
            (
                "Constraint Error: Duplicate key \"id: 1\" violates primary key constraint.",
                loc(None, None, Some("id"), None),
            ),
            (
                "Constraint Error: Duplicate key \"tag: dup\" violates unique constraint.",
                loc(None, None, Some("tag"), None),
            ),
            // 复合键：键列表有两个 → 什么都不给（连表名都没有）
            (
                "Constraint Error: Duplicate key \"code: cu, code2: x\" violates unique constraint.",
                loc(None, None, None, None),
            ),
            (
                "Constraint Error: CHECK constraint failed on table rds_err_probe_dd with expression CHECK(((code IS NULL) OR (code != 'bad')))",
                loc(None, Some("rds_err_probe_dd"), None, None),
            ),
            // 外键：DuckDB 不点名（表与列都没有）
            (
                "Constraint Error: Violates foreign key constraint because key \"id: 999\" does not exist in the referenced table",
                loc(None, None, None, None),
            ),
            (
                "Binder Error: Table \"rds_err_probe_dd\" does not have a column with name \"nope\"",
                loc(None, Some("rds_err_probe_dd"), Some("nope"), None),
            ),
            (
                "Binder Error: Referenced column \"nope\" not found in FROM clause!",
                loc(None, None, Some("nope"), None),
            ),
            (
                "Catalog Error: Table with name rds_no_such_table_xyz does not exist!",
                loc(None, Some("rds_no_such_table_xyz"), None, None),
            ),
            (
                "Parser Error: syntax error at or near \"SELEC\"",
                loc(None, None, None, None),
            ),
        ];
        for (msg, want) in cases {
            let got = duckdb(msg);
            if want.is_empty() {
                assert_eq!(got, None, "样本不该给出位置：{msg}");
            } else {
                assert_eq!(got.as_ref(), Some(want), "样本：{msg}");
            }
        }
    }

    /// 展示文案：对象在前、约束在后；空位置不给话。
    #[test]
    fn display_text_is_human_readable() {
        assert_eq!(
            loc(Some("public"), Some("orders"), Some("amount"), None).display_text(),
            Some("public.orders.amount".to_string())
        );
        assert_eq!(
            loc(None, Some("orders"), Some("tag"), Some("uq_tag")).display_text(),
            Some("orders.tag，约束 uq_tag".to_string())
        );
        assert_eq!(
            loc(None, None, None, Some("fk_parent")).display_text(),
            Some("约束 fk_parent".to_string())
        );
        assert_eq!(loc(None, None, None, None).display_text(), None);
    }
}
