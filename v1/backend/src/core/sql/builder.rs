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
    TableRef {
        catalog: None,
        schema: None,
        name: name.to_string(),
        alias: None,
        name_quote_style: QuoteStyle::DoubleQuote,
    }
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

pub fn build_create_table(table: &str, columns: &[ColumnDefInfo], if_not_exists: bool) -> String {
    let col_defs: Vec<SqlglotColumnDef> = columns
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
        .collect();

    let stmt = Statement::CreateTable(CreateTableStatement {
        comments: vec![],
        if_not_exists,
        temporary: false,
        table: make_table_ref(table),
        columns: col_defs,
        constraints: vec![],
        as_select: None,
    });

    generate(&stmt, Dialect::DuckDb)
}

pub fn build_drop_table(table: &str, if_exists: bool) -> String {
    let stmt = Statement::DropTable(DropTableStatement {
        comments: vec![],
        if_exists,
        table: make_table_ref(table),
        cascade: false,
    });
    generate(&stmt, Dialect::DuckDb)
}

pub fn build_create_table_as_select(table: &str, select_sql: &str) -> String {
    let stmt = Statement::CreateTable(CreateTableStatement {
        comments: vec![],
        if_not_exists: false,
        temporary: false,
        table: make_table_ref(table),
        columns: vec![],
        constraints: vec![],
        as_select: Some(Box::new(select_all().from(select_sql).build())),
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
