use sqlglot_rust::{parse, Dialect};

use super::engine::{SqlDialect, SqlStatementType};

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

pub fn parse_and_route(sql: &str, dialect: SqlDialect) -> (SqlStatementType, String) {
    let inner = to_inner_dialect(dialect);

    match parse(sql, inner) {
        Ok(stmt) => {
            let stmt_type = classify_statement(&stmt);
            let normalized = format!("{:?}", stmt);
            (stmt_type, normalized)
        }
        Err(_) => (SqlStatementType::Unknown, sql.to_string()),
    }
}

pub fn validate(sql: &str, dialect: SqlDialect) -> Result<(), String> {
    let inner = to_inner_dialect(dialect);
    parse(sql, inner).map(|_| ()).map_err(|e| e.to_string())
}

fn classify_statement(stmt: &sqlglot_rust::ast::Statement) -> SqlStatementType {
    match stmt {
        sqlglot_rust::ast::Statement::Select(_) => SqlStatementType::Select,
        sqlglot_rust::ast::Statement::Insert(_) => SqlStatementType::Insert,
        sqlglot_rust::ast::Statement::Update(_) => SqlStatementType::Update,
        sqlglot_rust::ast::Statement::Delete(_) => SqlStatementType::Delete,
        sqlglot_rust::ast::Statement::CreateTable(_)
        | sqlglot_rust::ast::Statement::DropTable(_)
        | sqlglot_rust::ast::Statement::AlterTable(_)
        | sqlglot_rust::ast::Statement::CreateView(_)
        | sqlglot_rust::ast::Statement::DropView(_)
        | sqlglot_rust::ast::Statement::Truncate(_) => SqlStatementType::Ddl,
        _ => SqlStatementType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let (ty, _) = parse_and_route("SELECT * FROM users", SqlDialect::Ansi);
        assert_eq!(ty, SqlStatementType::Select);
    }

    #[test]
    fn test_parse_insert() {
        let (ty, _) = parse_and_route("INSERT INTO users VALUES (1)", SqlDialect::Ansi);
        assert_eq!(ty, SqlStatementType::Insert);
    }

    #[test]
    fn test_parse_update() {
        let (ty, _) = parse_and_route("UPDATE users SET name='a' WHERE id=1", SqlDialect::Ansi);
        assert_eq!(ty, SqlStatementType::Update);
    }

    #[test]
    fn test_parse_delete() {
        let (ty, _) = parse_and_route("DELETE FROM users WHERE id=1", SqlDialect::Ansi);
        assert_eq!(ty, SqlStatementType::Delete);
    }

    #[test]
    fn test_parse_ddl() {
        let (ty, _) = parse_and_route("CREATE TABLE t (id INT)", SqlDialect::Ansi);
        assert_eq!(ty, SqlStatementType::Ddl);
    }

    #[test]
    fn test_parse_invalid() {
        let (ty, sql) = parse_and_route("NOT A VALID SQL STATEMENT", SqlDialect::Ansi);
        assert_eq!(ty, SqlStatementType::Unknown);
        assert_eq!(sql, "NOT A VALID SQL STATEMENT");
    }

    #[test]
    fn test_validate_valid() {
        assert!(validate("SELECT 1", SqlDialect::Ansi).is_ok());
    }

    #[test]
    fn test_validate_invalid() {
        assert!(validate("SELECT FROM", SqlDialect::Ansi).is_err());
    }
}
