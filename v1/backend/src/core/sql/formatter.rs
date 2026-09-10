use sqlglot_rust::{parse, Dialect};

use super::engine::SqlDialect;

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

pub fn format(sql: &str, dialect: SqlDialect) -> String {
    let inner = to_inner_dialect(dialect);

    match parse(sql, inner) {
        Ok(statement) => format!("{:?}", statement),
        Err(_) => sql.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_select() {
        let result = format("SELECT * FROM users", SqlDialect::Ansi);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_format_invalid_graceful() {
        let result = format("NOT VALID SQL", SqlDialect::Ansi);
        assert_eq!(result, "NOT VALID SQL");
    }
}
