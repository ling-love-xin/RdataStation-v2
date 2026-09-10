use sqlglot_rust::{transpile as sqlglot_transpile, Dialect};

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

pub fn transpile(sql: &str, source: SqlDialect, target: SqlDialect) -> Result<String, String> {
    let src = to_inner_dialect(source);
    let tgt = to_inner_dialect(target);

    sqlglot_transpile(sql, src, tgt).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_mysql_to_postgres() {
        let result = transpile("SELECT NOW()", SqlDialect::Mysql, SqlDialect::Postgres);
        assert!(result.is_ok());
    }

    #[test]
    fn test_transpile_same_dialect() {
        let result = transpile("SELECT 1", SqlDialect::Ansi, SqlDialect::Ansi);
        assert!(result.is_ok());
    }
}
