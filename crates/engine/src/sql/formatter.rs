//! SQL 格式化
//!
//! 走 sqlglot-rust 的 AST 生成器（`generate_pretty`），**不是 Rust 的 Debug 打印**。
//! （v2 迁移期的实现是 `format!("{:?}", statement)`，输出不是 SQL —— 属"四个假底座"之一，
//! 其旧测试只断言"非空"，因此没有拦住。）
//!
//! ## 取舍
//!
//! - **多语句脚本**：`parse_statements_with_comments` 逐条生成，以 `;\n\n` 连接并补尾分号
//!   （编辑器面向脚本，单条 `parse` 会把多语句判为解析失败）。
//! - **解析失败原样返回**：编辑中的文本（未写完）不能因为格式化成坏内容，也不报错。
//! - **注释**：AST 携带 comments 且生成器会输出前导注释；**行内 / 尾随注释可能丢失**，
//!   这是 sqlglot-rust 生成器的能力边界（真机核对后若不可接受，再考虑自研缩进器）。
//! - **不改变语义**：只做排版；方言相关的重写属 `transpiler`，不在这里做。

use sqlglot_rust::{generate_pretty, parse_statements_with_comments, Dialect};

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

/// 格式化 SQL（多语句脚本安全；解析失败原样返回）
pub fn format(sql: &str, dialect: SqlDialect) -> String {
    let inner = to_inner_dialect(dialect);

    match parse_statements_with_comments(sql, inner) {
        Ok(statements) if !statements.is_empty() => {
            let mut bodies: Vec<String> = Vec::with_capacity(statements.len());
            for statement in &statements {
                // 生成器是否带尾分号未在文档中承诺：统一裁掉后再拼接，避免出现 `;;`
                let body = generate_pretty(statement, inner);
                let body = body.trim().trim_end_matches(';').trim().to_string();
                if !body.is_empty() {
                    bodies.push(body);
                }
            }
            if bodies.is_empty() {
                return sql.to_string();
            }

            let mut out = bodies.join(";\n\n");
            out.push(';');
            out
        }
        // 空输入 / 解析失败（文本可能还没写完）：原样返回，不静默改写用户内容
        Ok(_) | Err(_) => sql.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlglot_rust::parse_statements_with_comments as parse_many;

    #[test]
    fn formats_as_sql_not_a_debug_dump() {
        let out = format("select a,b from t where x=1", SqlDialect::Ansi);
        assert!(out.contains("SELECT"), "{out}");
        assert!(out.contains("FROM"), "{out}");
        // 关键回归：绝不能是 Rust Debug 打印（迁移期实现即 `format!("{:?}")`）
        assert!(!out.contains("Select("), "{out}");
        assert!(!out.contains('{'), "{out}");
    }

    #[test]
    fn formatted_output_can_be_reparsed() {
        let sql = "select id, count(*) as n from orders group by id order by n desc limit 10";
        let out = format(sql, SqlDialect::Ansi);
        assert!(
            parse_many(&out, Dialect::Ansi).is_ok(),
            "格式化结果必须能再次解析：{out}"
        );
    }

    #[test]
    fn multi_statement_script_keeps_every_statement() {
        let out = format("select 1; select 2;", SqlDialect::Ansi);
        assert_eq!(out.matches("SELECT").count(), 2, "{out}");
        assert!(out.ends_with(';'), "{out}");
        assert!(!out.contains(";;"), "不应出现连续分号：{out}");
    }

    #[test]
    fn invalid_sql_is_returned_unchanged() {
        let sql = "NOT VALID SQL";
        assert_eq!(format(sql, SqlDialect::Ansi), sql);
    }

    #[test]
    fn empty_and_blank_inputs_are_returned_unchanged() {
        assert_eq!(format("", SqlDialect::Ansi), "");
        assert_eq!(format("   \n", SqlDialect::Ansi), "   \n");
    }

    #[test]
    fn leading_comment_is_not_dropped() {
        let out = format("-- 说明\nselect 1", SqlDialect::Ansi);
        assert!(out.contains("说明"), "前导注释不应被丢弃：{out}");
    }

    #[test]
    fn dialect_parameter_is_accepted() {
        // 各方言走同一实现（生成器按方言；测试只保证不 panic 且仍是 SQL）
        for dialect in [
            SqlDialect::Ansi,
            SqlDialect::Mysql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::Duckdb,
        ] {
            let out = format("select 1", dialect);
            assert!(out.contains("SELECT"), "{dialect:?} → {out}");
        }
    }
}
