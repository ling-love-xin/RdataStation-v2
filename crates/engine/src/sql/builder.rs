use sqlglot_rust::ast::{
    ColumnDef as SqlglotColumnDef, CreateTableStatement, DataType, DropTableStatement, Expr,
    InsertSource, InsertStatement, QuoteStyle, Statement, TableRef,
};
use sqlglot_rust::builder::{select, select_all};
use sqlglot_rust::{generate, Dialect};

use super::engine::{AlterOperation, ColumnDefInfo, SqlDialect};

#[allow(dead_code)]
fn to_inner_dialect(dialect: SqlDialect) -> Dialect {
    match dialect {
        SqlDialect::Ansi => Dialect::Ansi,
        SqlDialect::Mysql => Dialect::Mysql,
        SqlDialect::Postgres => Dialect::Postgres,
        SqlDialect::Sqlite => Dialect::Sqlite,
        SqlDialect::Duckdb => Dialect::DuckDb,
        SqlDialect::MsSQL => Dialect::Tsql,
        SqlDialect::Oracle => Dialect::Oracle,
        SqlDialect::Snowflake => Dialect::Snowflake,
        SqlDialect::BigQuery => Dialect::BigQuery,
        SqlDialect::Redshift => Dialect::Redshift,
    }
}

fn make_table_ref(name: &str) -> TableRef {
    qualified_table_ref(None, None, name)
}

/// 三段式表名（catalog / schema / table）。
///
/// 跨库写入用：把目标 DuckDB 文件库 `ATTACH ... AS <catalog>` 后，
/// 表以 `<catalog>.<schema>.<table>` 定位（见 [`build_insert_select`]）。
/// 限定名必须是**三段都写**的形式，不要用字符串拼 `a.b.c` 当单名（会被当成一个标识符）。
#[derive(Debug, Clone, Copy)]
pub struct QualifiedTable<'a> {
    /// `ATTACH ... AS <catalog>` 里的别名
    pub catalog: &'a str,
    /// 目标库内的 schema（DuckDB 文件库是 `main`）
    pub schema: &'a str,
    /// 表名
    pub table: &'a str,
}

fn qualified_table_ref(catalog: Option<&str>, schema: Option<&str>, name: &str) -> TableRef {
    // 注意：sqlglot 只给**表名**加引号，catalog / schema 原样输出，
    // 所以这两个只能传安全标识符（本项目固定用 `rds_mock_sink` + `main`）。
    TableRef {
        catalog: catalog.map(str::to_string),
        schema: schema.map(str::to_string),
        name: name.to_string(),
        alias: None,
        name_quote_style: QuoteStyle::DoubleQuote,
        // 0.10 新增：别名引号风格。此处无别名，取默认（不加引号）
        alias_quote_style: QuoteStyle::None,
    }
}

/// 标识符加双引号（内部的 `"` 翻倍）。
///
/// 列名走 `mock::sanitize_identifier`，允许非 ASCII（中文列名）与数字开头，
/// 这类名字**不加引号在 SQL 里解析不过**（临时表建表时也是带引号的）。
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn parse_data_type(dt: &str) -> DataType {
    let upper = dt.to_uppercase();
    match upper.as_str() {
        "INT" | "INTEGER" => DataType::Int,
        "BIGINT" => DataType::BigInt,
        "SMALLINT" => DataType::SmallInt,
        "TINYINT" => DataType::TinyInt,
        "FLOAT" => DataType::Float,
        "DOUBLE" => DataType::Double,
        "DECIMAL" => DataType::Decimal {
            precision: None,
            scale: None,
        },
        "BOOLEAN" | "BOOL" => DataType::Boolean,
        "VARCHAR" => DataType::Varchar(None),
        "TEXT" => DataType::Text,
        "DATE" => DataType::Date,
        "TIMESTAMP" => DataType::Timestamp {
            precision: None,
            with_tz: false,
        },
        "UUID" => DataType::Varchar(None),
        "BLOB" | "BYTEA" => DataType::Binary(None),
        other => {
            if let Some(inner) = other.strip_prefix("VARCHAR(") {
                let len = extract_param(inner);
                DataType::Varchar(len)
            } else if let Some(inner) = other.strip_prefix("DECIMAL(") {
                let (precision, scale) = extract_two_params(inner);
                DataType::Decimal { precision, scale }
            } else {
                DataType::Varchar(None)
            }
        }
    }
}

fn extract_param(s: &str) -> Option<u32> {
    s.trim_end_matches(')').trim().parse().ok()
}

fn extract_two_params(s: &str) -> (Option<u32>, Option<u32>) {
    let inner = s.trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').collect();
    let precision = parts.first().and_then(|p| p.trim().parse().ok());
    let scale = parts.get(1).and_then(|p| p.trim().parse().ok());
    (precision, scale)
}

/// 列定义转换（`build_create_table` 与跨库版本共用同一份）。
fn column_defs(columns: &[ColumnDefInfo]) -> Vec<SqlglotColumnDef> {
    columns
        .iter()
        .map(|c| SqlglotColumnDef {
            name: c.name.clone(),
            data_type: parse_data_type(&c.data_type),
            nullable: if c.nullable { None } else { Some(false) },
            default: None,
            primary_key: false,
            unique: c.unique,
            auto_increment: false,
            collation: None,
            comment: None,
        })
        .collect()
}

fn create_table_sql(
    table: TableRef,
    columns: Vec<SqlglotColumnDef>,
    if_not_exists: bool,
) -> String {
    let stmt = Statement::CreateTable(CreateTableStatement {
        comments: vec![],
        if_not_exists,
        temporary: false,
        table,
        columns,
        constraints: vec![],
        as_select: None,
    });
    generate(&stmt, Dialect::DuckDb)
}

pub fn build_create_table(table: &str, columns: &[ColumnDefInfo], if_not_exists: bool) -> String {
    create_table_sql(make_table_ref(table), column_defs(columns), if_not_exists)
}

/// 跨库版本：`CREATE TABLE "<catalog>"."<schema>"."<table>" (...)`（建的是 `ATTACH` 进来的文件库）。
pub fn build_create_table_in(
    target: &QualifiedTable,
    columns: &[ColumnDefInfo],
    if_not_exists: bool,
) -> String {
    create_table_sql(
        qualified_table_ref(Some(target.catalog), Some(target.schema), target.table),
        column_defs(columns),
        if_not_exists,
    )
}

fn drop_table_sql(table: TableRef, if_exists: bool) -> String {
    let stmt = Statement::DropTable(DropTableStatement {
        comments: vec![],
        if_exists,
        table,
        cascade: false,
    });
    generate(&stmt, Dialect::DuckDb)
}

pub fn build_drop_table(table: &str, if_exists: bool) -> String {
    drop_table_sql(make_table_ref(table), if_exists)
}

/// 跨库版本：删 `ATTACH` 进来的文件库里的表（写入失败时回滚刚建的表，见 `mock::write_temp_table_to_database`）。
pub fn build_drop_table_in(target: &QualifiedTable, if_exists: bool) -> String {
    drop_table_sql(
        qualified_table_ref(Some(target.catalog), Some(target.schema), target.table),
        if_exists,
    )
}

/// 生成 `ATTACH '<path>' AS "<alias>"`（DuckDB 专有）。
///
/// sqlglot 的 AST 里没有 `ATTACH`（与 `COPY` 同例），所以这里手拼：
/// 路径按 SQL 字面量规则转义单引号（Windows 反斜杠在 DuckDB 单引号串里是字面量，不需转义）。
pub fn build_attach_database(path: &str, alias: &str) -> String {
    format!(
        "ATTACH '{}' AS {}",
        path.replace('\'', "''"),
        quote_identifier(alias)
    )
}

/// 生成 `DETACH "<alias>"`（与 [`build_attach_database`] 成对）。
pub fn build_detach_database(alias: &str) -> String {
    format!("DETACH {}", quote_identifier(alias))
}

/// 生成 `INSERT INTO "<catalog>"."<schema>"."<table>" (<cols>) SELECT <cols> FROM <source>`。
///
/// 用于**跨库直写**：数据全程在 DuckDB 内部流动，不经过 Rust 字符串（对比 `INSERT ... VALUES` 文本中转）。
/// 列清单同时出现在插入列与选择列上：目标表多出的列走默认值，与旧文本路径语义一致。
pub fn build_insert_select(
    target: &QualifiedTable,
    source_table: &str,
    columns: &[String],
) -> String {
    let quoted: Vec<String> = columns.iter().map(|c| quote_identifier(c)).collect();
    // 先建 SELECT（它借用 `quoted`），再把 `quoted` 移进插入列清单
    let query = {
        let refs: Vec<&str> = quoted.iter().map(String::as_str).collect();
        select(&refs).from(source_table).build()
    };
    let stmt = Statement::Insert(InsertStatement {
        comments: vec![],
        table: qualified_table_ref(Some(target.catalog), Some(target.schema), target.table),
        columns: quoted,
        source: InsertSource::Query(Box::new(query)),
        on_conflict: None,
        returning: vec![],
    });
    generate(&stmt, Dialect::DuckDb)
}

/// 生成 `CREATE TABLE {table} AS SELECT * FROM {source_table}`。
///
/// `source_table` 是源**表名**，不是 SELECT 语句（历史参数名 `select_sql` 有误导性，
/// 曾导致调用方传入整条 SELECT 而生成非法 SQL）。
pub fn build_create_table_as_select(table: &str, source_table: &str) -> String {
    let stmt = Statement::CreateTable(CreateTableStatement {
        comments: vec![],
        if_not_exists: false,
        temporary: false,
        table: make_table_ref(table),
        columns: vec![],
        constraints: vec![],
        as_select: Some(Box::new(select_all().from(source_table).build())),
    });
    generate(&stmt, Dialect::DuckDb)
}

pub fn build_insert(table: &str, columns: &[String], values: &[Vec<String>]) -> String {
    let all_values: Vec<Vec<Expr>> = values
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| {
                    if v == "NULL" {
                        Expr::Null
                    } else {
                        Expr::StringLiteral(v.clone())
                    }
                })
                .collect()
        })
        .collect();

    let stmt = Statement::Insert(InsertStatement {
        comments: vec![],
        table: make_table_ref(table),
        columns: columns.to_vec(),
        source: InsertSource::Values(all_values),
        on_conflict: None,
        returning: vec![],
    });

    generate(&stmt, Dialect::DuckDb)
}

pub fn build_select_all(table: &str, limit: Option<i64>) -> String {
    let mut builder = select_all().from(table);
    if let Some(n) = limit {
        builder = builder.limit(n);
    }
    generate(&builder.build(), Dialect::DuckDb)
}

pub fn build_select(table: &str, columns: &[&str], limit: Option<i64>) -> String {
    let mut builder = select(columns).from(table);
    if let Some(n) = limit {
        builder = builder.limit(n);
    }
    generate(&builder.build(), Dialect::DuckDb)
}

pub fn build_alter_table(table: &str, operations: &[AlterOperation]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for op in operations {
        match op {
            AlterOperation::AddColumn(col) => {
                parts.push(format!("ADD COLUMN \"{}\" {}", col.name, col.data_type));
            }
            AlterOperation::DropColumn(name) => {
                parts.push(format!("DROP COLUMN \"{}\"", name));
            }
            AlterOperation::RenameColumn { old_name, new_name } => {
                parts.push(format!(
                    "RENAME COLUMN \"{}\" TO \"{}\"",
                    old_name, new_name
                ));
            }
            AlterOperation::ModifyColumn(col) => {
                parts.push(format!("MODIFY COLUMN \"{}\" {}", col.name, col.data_type));
            }
        }
    }

    format!("ALTER TABLE \"{}\" {}", table, parts.join(", "))
}

pub fn build_create_index(name: &str, table: &str, columns: &[String], unique: bool) -> String {
    let unique_str = if unique { "UNIQUE " } else { "" };
    let cols = columns
        .iter()
        .map(|c| format!("\"{}\"", c))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE {}INDEX \"{}\" ON \"{}\" ({})",
        unique_str, name, table, cols
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固化生成结果（含标识符加引号的方式）
    ///
    /// sqlglot-rust 升级会动到引号/风格处理（0.10 起 `TableRef` 新增 `alias_quote_style`），
    /// 这里钉住当前输出，依赖行为一变更就会失败
    #[test]
    fn test_generated_sql_is_pinned() {
        let cols = vec![ColumnDefInfo {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            unique: true,
            nullable: false,
        }];
        assert_eq!(
            build_create_table("users", &cols, false),
            "CREATE TABLE \"users\" (id INT UNIQUE NOT NULL)"
        );
        assert_eq!(
            build_drop_table("users", true),
            "DROP TABLE IF EXISTS \"users\""
        );
        assert_eq!(
            build_select_all("users", Some(10)),
            "SELECT * FROM users LIMIT 10"
        );
        assert_eq!(
            build_insert(
                "users",
                &["id".to_string(), "name".to_string()],
                &[vec!["1".to_string(), "Alice".to_string()]]
            ),
            "INSERT INTO \"users\" (id, name) VALUES ('1', 'Alice')"
        );
    }

    /// 跳库直写的四个构造器（ATTACH / DETACH / 限定名建表与删表 / INSERT SELECT）。
    ///
    /// 列名与表名一律带引号：`sanitize_identifier` 允许中文列名与数字开头，
    /// 不加引号在 SQL 里解析不过。
    #[test]
    fn test_cross_database_sql_is_pinned() {
        let target = QualifiedTable {
            catalog: "rds_mock_sink",
            schema: "main",
            table: "orders",
        };
        assert_eq!(
            build_attach_database("D:\\data\\analytics.duckdb", "rds_mock_sink"),
            "ATTACH 'D:\\data\\analytics.duckdb' AS \"rds_mock_sink\""
        );
        assert_eq!(
            build_detach_database("rds_mock_sink"),
            "DETACH \"rds_mock_sink\""
        );
        let cols = vec![ColumnDefInfo {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            unique: false,
            nullable: true,
        }];
        assert_eq!(
            build_create_table_in(&target, &cols, false),
            "CREATE TABLE rds_mock_sink.main.\"orders\" (id INT)"
        );
        assert_eq!(
            build_drop_table_in(&target, true),
            "DROP TABLE IF EXISTS rds_mock_sink.main.\"orders\""
        );
        assert_eq!(
            build_insert_select(
                &target,
                "temp_mock_orders",
                &["id".to_string(), "金额".to_string()]
            ),
            "INSERT INTO rds_mock_sink.main.\"orders\" (\"id\", \"金额\") SELECT \"id\", \"金额\" FROM temp_mock_orders"
        );
    }

    /// 路径里的单引号按 SQL 字面量规则翻倍（否则会拼出语法错的语句）。
    #[test]
    fn test_attach_escapes_single_quote_in_path() {
        assert_eq!(
            build_attach_database("/tmp/o'brien/analytics.duckdb", "sink"),
            "ATTACH '/tmp/o''brien/analytics.duckdb' AS \"sink\""
        );
    }

    #[test]
    fn test_build_create_table() {
        let cols = vec![ColumnDefInfo {
            name: "id".to_string(),
            data_type: "INT".to_string(),
            unique: true,
            nullable: false,
        }];
        let sql = build_create_table("users", &cols, false);
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("users"));
        assert!(sql.contains("id"));
    }

    #[test]
    fn test_build_drop_table() {
        let sql = build_drop_table("users", true);
        assert!(sql.contains("DROP TABLE"));
        assert!(sql.contains("IF EXISTS"));
    }

    #[test]
    fn test_build_select_all() {
        let sql = build_select_all("users", Some(10));
        assert!(sql.contains("SELECT"));
        assert!(sql.contains("users"));
        assert!(sql.contains("LIMIT"));
    }

    #[test]
    fn test_build_insert() {
        let sql = build_insert(
            "users",
            &["id".to_string(), "name".to_string()],
            &[vec!["1".to_string(), "Alice".to_string()]],
        );
        assert!(sql.contains("INSERT"));
        assert!(sql.contains("users"));
    }
}
