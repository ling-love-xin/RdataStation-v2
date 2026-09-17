//! 执行计划（EXPLAIN）：按方言生成前缀
//!
//! ## 为什么是“按方言生成”而不是一律加 `EXPLAIN`
//!
//! 各家语法不同：SQLite 要 `EXPLAIN QUERY PLAN`（裸 `EXPLAIN` 给的是**虚拟机指令流**，
//! 不是计划树）；SQL Server 要先 `SET SHOWPLAN_ALL ON` 之类的**会话开关**；Oracle 是
//! `EXPLAIN PLAN FOR …` 之后再查 `DBMS_XPLAN` 视图的**两步**动作。后两者都不是“加个前缀”
//! 能表达的，所以这里**如实返回 `None`**（调用方给一句“暂不支持”），而不是硬拼一条跑不通的语句。
//!
//! ## 权威来源
//!
//! 计划一律来自**源库自己的 EXPLAIN**（或加速 / 联邦档下 **DuckDB 的 EXPLAIN**）——
//! 本地 sqlglot 的 `plan`（`planner` 模块）只是它自建的离线计划，**不等于**数据库真实计划，
//! 只在没有连接时作为降级展示（见架构 §12 #20）。

use super::engine::SqlDialect;

/// 把一条语句包成“取执行计划”的语句；该方言不支持时返回 `None`
///
/// 输入里的首尾空白与**尾分号**会被裁掉（`EXPLAIN SELECT 1;;` 是语法错误）。
pub fn explain_sql(dialect: SqlDialect, sql: &str) -> Option<String> {
    let body = sql.trim().trim_end_matches(';').trim();
    if body.is_empty() {
        return None;
    }
    match dialect {
        SqlDialect::Mysql | SqlDialect::Postgres | SqlDialect::Duckdb => {
            Some(format!("EXPLAIN {body}"))
        }
        // SQLite 的计划树要 QUERY PLAN；裸 EXPLAIN 是 VM 指令（对用户没有意义）
        SqlDialect::Sqlite => Some(format!("EXPLAIN QUERY PLAN {body}")),
        // SQL Server（SHOWPLAN 会话开关）与 Oracle（EXPLAIN PLAN FOR + 查视图）都是多步动作，
        // 先不假装能做
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::explain_sql;
    use crate::sql::SqlDialect;

    #[test]
    fn the_prefix_follows_the_dialect() {
        assert_eq!(
            explain_sql(SqlDialect::Mysql, "SELECT 1").as_deref(),
            Some("EXPLAIN SELECT 1")
        );
        assert_eq!(
            explain_sql(SqlDialect::Postgres, "SELECT 1").as_deref(),
            Some("EXPLAIN SELECT 1")
        );
        assert_eq!(
            explain_sql(SqlDialect::Duckdb, "SELECT 1").as_deref(),
            Some("EXPLAIN SELECT 1")
        );
        // SQLite 的计划树要 QUERY PLAN
        assert_eq!(
            explain_sql(SqlDialect::Sqlite, "SELECT 1").as_deref(),
            Some("EXPLAIN QUERY PLAN SELECT 1")
        );
    }

    #[test]
    fn dialects_that_need_more_than_a_prefix_are_refused() {
        for dialect in [SqlDialect::MsSQL, SqlDialect::Oracle, SqlDialect::Ansi] {
            assert!(
                explain_sql(dialect, "SELECT 1").is_none(),
                "{dialect:?} 不是加个前缀能表达的，不该硬拼"
            );
        }
    }

    #[test]
    fn trailing_semicolons_and_whitespace_are_trimmed() {
        assert_eq!(
            explain_sql(SqlDialect::Mysql, "  SELECT 1;  ").as_deref(),
            Some("EXPLAIN SELECT 1")
        );
        assert_eq!(
            explain_sql(SqlDialect::Mysql, "SELECT 1;;").as_deref(),
            Some("EXPLAIN SELECT 1"),
            "尾分号全裁掉（`EXPLAIN SELECT 1;;` 是语法错误）"
        );
    }

    #[test]
    fn nothing_to_explain_returns_none() {
        assert!(explain_sql(SqlDialect::Mysql, "").is_none());
        assert!(explain_sql(SqlDialect::Mysql, "  ; ").is_none());
    }
}
