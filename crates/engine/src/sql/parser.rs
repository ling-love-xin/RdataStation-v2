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

/// 一条 SELECT 的投影列来源（P1 字段注释的血缘；**与输出列一一对应**）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionSource {
    /// 实体列引用：`[schema.]table.column`（单表 FROM 时裸列名已归位到表）
    Column { table: Option<String>, column: String },
    /// `SELECT *` / `table.*`：来源列需按表列集展开（table 为限定表名，可能为 None）
    Star { table: Option<String> },
    /// 复杂表达式 / 字面量 / 函数：无来源列，注释如实无
    Expression,
}

/// 解析一条 SELECT 的投影列来源（P1 字段注释血缘；非 SELECT / 解析失败 = `Err`）
///
/// 口径（与 DBeaver 的 JDBC 一致）：
/// - `Expr::Column` 直接给来源列；**单表 FROM 时一律归位到真实表名**（`schema.表` 或裸名；
///   限定符是别名也一样归位——单表只有一张表，别名/表名指向同一张，NavCache 只认真实名）；
/// - `SELECT *` 按 FROM 表展开（子查询 / CTE 派生表没有 schema 对应列，调用方按无来源处理）；
/// - 多表 join：限定列保留 SQL 原文限定符（别名**未解析**——解析留给调用方，DBeaver 同口径），
///   **无限定**列无法确定属哪张表，留 `None`；
/// - 复杂表达式（`a+b` / `count(*)` / 字面量）如实给 `Expression`。
pub fn projection_sources(sql: &str, dialect: SqlDialect) -> Result<Vec<ProjectionSource>, String> {
    let inner = to_inner_dialect(dialect);
    let stmt = parse(sql, inner).map_err(|e| e.to_string())?;
    let sqlglot_rust::ast::Statement::Select(select) = stmt else {
        return Err("不是 SELECT 语句".to_string());
    };
    // FROM 是**单表**（无 join / 子查询 / 表函数）时，裸列名归位到这张表的**真实名**；
    // 多表 join 无法确定无限定列属哪张，留 None（DBeaver 对 join 的无限定列同样无法归位）
    let from_table = if select.joins.is_empty() {
        single_from_table(select.from.as_ref())
    } else {
        None
    };
    Ok(select
        .columns
        .iter()
        .map(|item| match item {
            sqlglot_rust::ast::SelectItem::Wildcard => ProjectionSource::Star {
                table: from_table.clone(),
            },
            sqlglot_rust::ast::SelectItem::QualifiedWildcard { table } => ProjectionSource::Star {
                table: Some(table.clone()),
            },
            sqlglot_rust::ast::SelectItem::Expr { expr, .. } => match expr {
                sqlglot_rust::ast::Expr::Column { table, name, .. } => {
                    ProjectionSource::Column {
                        // 单表 FROM：限定符是别名也一样归位到真实表（NavCache 只认真实名）
                        table: from_table
                            .clone()
                            .or_else(|| table.clone()),
                        column: name.clone(),
                    }
                }
                _ => ProjectionSource::Expression,
            },
        })
        .collect())
}

/// FROM 单表的**真实**限定名（`catalog.schema.table` → `schema.table`；非单表 = `None`）
///
/// 不用别名：这张表是要到 NavCache 查列元数据的，缓存键是 `schema.表` 真实名。
/// （join 场景的别名解析留给调用方——多表时限定符本身就表达归属，见 [`projection_sources`]。）
fn single_from_table(from: Option<&sqlglot_rust::ast::FromClause>) -> Option<String> {
    let sqlglot_rust::ast::FromClause { source } = from?;
    let sqlglot_rust::ast::TableSource::Table(table) = source else {
        return None;
    };
    match (&table.schema, &table.catalog) {
        (Some(schema), _) => Some(format!("{}.{}", schema, table.name)),
        (None, Some(catalog)) => Some(format!("{}.{}", catalog, table.name)),
        (None, None) => Some(table.name.clone()),
    }
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

    // ---------- P1 投影列来源 ----------

    #[test]
    fn projection_sources_qualified_and_bare_columns() {
        let sources = projection_sources(
            "SELECT o.id, name, o.amount FROM orders o",
            SqlDialect::Ansi,
        )
        .expect("解析成功");
        assert_eq!(
            sources,
            vec![
                // 单表 FROM：限定符是别名也归位到**真实表名**（NavCache 只认真实名）
                ProjectionSource::Column {
                    table: Some("orders".to_string()),
                    column: "id".to_string(),
                },
                ProjectionSource::Column {
                    table: Some("orders".to_string()),
                    column: "name".to_string(),
                },
                ProjectionSource::Column {
                    table: Some("orders".to_string()),
                    column: "amount".to_string(),
                },
            ]
        );
    }

    #[test]
    fn projection_sources_schema_qualified_and_star() {
        let sources = projection_sources(
            "SELECT *, public.orders.id FROM public.orders",
            SqlDialect::Ansi,
        )
        .expect("解析成功");
        assert_eq!(
            sources,
            vec![
                ProjectionSource::Star {
                    table: Some("public.orders".to_string()),
                },
                ProjectionSource::Column {
                    table: Some("public.orders".to_string()),
                    column: "id".to_string(),
                },
            ]
        );
    }

    #[test]
    fn projection_sources_expression_and_alias() {
        let sources = projection_sources(
            "SELECT count(*) AS cnt, a + b AS sum, 1 AS one FROM t",
            SqlDialect::Ansi,
        )
        .expect("解析成功");
        assert_eq!(
            sources,
            vec![
                ProjectionSource::Expression,
                ProjectionSource::Expression,
                ProjectionSource::Expression,
            ]
        );
    }

    #[test]
    fn projection_sources_join_bare_column_stays_unknown() {
        let sources = projection_sources(
            "SELECT t1.id, t2.name, status FROM t1 JOIN t2 ON t1.id = t2.id",
            SqlDialect::Ansi,
        )
        .expect("解析成功");
        assert_eq!(
            sources,
            vec![
                ProjectionSource::Column {
                    table: Some("t1".to_string()),
                    column: "id".to_string(),
                },
                ProjectionSource::Column {
                    table: Some("t2".to_string()),
                    column: "name".to_string(),
                },
                // join 的无限定列无法归位（DBeaver 同口径）
                ProjectionSource::Column {
                    table: None,
                    column: "status".to_string(),
                },
            ]
        );
    }

    #[test]
    fn projection_sources_non_select_is_err() {
        assert!(projection_sources("INSERT INTO t VALUES (1)", SqlDialect::Ansi).is_err());
    }
}
