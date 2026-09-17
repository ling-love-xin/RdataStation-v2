//! 方言转译
//!
//! ## 为什么不能直接调 sqlglot 的 `transpile`
//!
//! **实测（架构 §12 #19）**：`transpile("SELECT 1; SELECT 2;")` 返回 `Ok("SELECT 1")` ——
//! 第二条及以后的语句**静默消失**，调用方连 `Err` 都收不到。生产路径
//! `SqlEngine::transpile` 同样如此（`crates/engine/tests/sqlglot_capabilities.rs` 的
//! 第 8 组探针把它钉在报告里）。
//!
//! 所以脚本转译走 [`super::script::rewrite_statements`]：**先按词法切分，再逐条转译**，
//! 回填原位。这同时解决了另外两件事：一条解析不了不影响其它条；语句外的注释与空行原样保留。
//! 面向单条的旧接口 [`transpile`] 保留（`sql_parser_service::transpile_sql` 在用）。
//!
//! ## 判定“解析不了”的结果
//!
//! 逐条转译失败的语句**逐字保留**并计数（`kept_verbatim`）——界面必须能说清
//! “有几条没动”，否则同一个功能在用户眼里就是“按了没反应”。
//!
//! ## 注释
//!
//! 单条用 `transpile_with_comments`（不是 `transpile`）：实测不带注释的实现会把
//! **语句开头的前导注释吃掉**（`"-- 头注释\nSELECT 1"` → `"SELECT 1"`），
//! 静默吃注释比“不转译”严重得多。行内 / 尾随注释仍会让该条解析失败 → **原样保留**
//! （与格式化同口径）。

use sqlglot_rust::{Dialect, transpile as sqlglot_transpile, transpile_with_comments};

use super::engine::SqlDialect;
use super::script::rewrite_statements;

/// 转译的结果（给界面用：**翻了几条 / 哪几条没动**都要说得清）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspileReport {
    /// 转译后的整篇文本（转译不了的语句**逐字保留**）
    pub text: String,
    /// 成功转译的语句数
    pub transpiled: usize,
    /// 解析 / 转译失败、逐字保留的语句数（0 = 全篇都翻了）
    pub kept_verbatim: usize,
}

/// 脚本级方言转译（**先切分再逐条转译**，脚本一条都不丢）
pub fn transpile_with_report(
    sql: &str,
    source: SqlDialect,
    target: SqlDialect,
) -> TranspileReport {
    let src = to_inner_dialect(source);
    let tgt = to_inner_dialect(target);
    let rewritten = rewrite_statements(sql, |body| transpile_single(body, src, tgt));
    TranspileReport {
        text: rewritten.text,
        transpiled: rewritten.rewritten,
        kept_verbatim: rewritten.kept_verbatim,
    }
}

/// 转译一条语句（失败返回 `None`：调用方逐字保留）
///
/// 用 `transpile_with_comments`：**前导注释要跟着语句一起过去**（实测不带注释的
/// `transpile` 会把 `-- 头注释\nSELECT 1` 里的注释吃掉）。
fn transpile_single(body: &str, source: Dialect, target: Dialect) -> Option<String> {
    let out = transpile_with_comments(body, source, target).ok()?;
    // 生成器是否带尾分号未在文档中承诺：统一裁掉（分隔符由脚本骨架补 `;\n\n`）
    let text = out.trim().trim_end_matches(';').trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(text)
}

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

/// 转译单条 SQL（**旧接口，只吃单条**）
///
/// 多语句脚本请用 [`SqlEngine::transpile_report`]（脚本走整篇接口会静默丢语句）。
pub fn transpile(sql: &str, source: SqlDialect, target: SqlDialect) -> Result<String, String> {
    let src = to_inner_dialect(source);
    let tgt = to_inner_dialect(target);

    sqlglot_transpile(sql, src, tgt).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{transpile, transpile_with_report};
    use crate::sql::{SqlDialect, split_statements};

    /// **硬约束回归（架构 §12 #19）**：脚本转译**一条都不能丢**
    #[test]
    fn a_script_never_loses_statements() {
        let script = "SELECT 1; SELECT 2; SELECT 3;";
        let report = transpile_with_report(script, SqlDialect::Ansi, SqlDialect::Postgres);
        assert_eq!(report.transpiled, 3, "三条都该转译：{}", report.text);
        assert_eq!(report.kept_verbatim, 0);
        // 条数是**切分回去数的**——不是数关键字（那会自证）
        assert_eq!(
            split_statements(&report.text).len(),
            3,
            "转译后的脚本必须还是三条：{:?}",
            report.text
        );
        for needle in ["1", "2", "3"] {
            assert!(report.text.contains(needle), "{needle} 丢了：{}", report.text);
        }
    }

    /// 对照：**单条接口对脚本确实会截断**（把已知缺陷钉住，防止有人“简化”回去）
    #[test]
    fn the_single_statement_api_truncates_a_script() {
        let truncated = transpile("SELECT 1; SELECT 2;", SqlDialect::Ansi, SqlDialect::Postgres)
            .expect("单条接口对脚本也不报错——这正是它危险的地方");
        assert!(
            !truncated.contains('2'),
            "若哪天它不再截断了，这段注释与架构 §12 #19 都该更新：{truncated:?}"
        );
    }

    /// 真的翻了东西：MySQL 的反引号标识符在 PostgreSQL 下换成双引号
    #[test]
    fn identifiers_get_requoted_for_the_target_dialect() {
        let report = transpile_with_report(
            "SELECT `a` FROM `t` LIMIT 1",
            SqlDialect::Mysql,
            SqlDialect::Postgres,
        );
        assert_eq!(report.transpiled, 1);
        assert!(
            report.text.contains("\"a\"") && report.text.contains("\"t\""),
            "PG 目标该用双引号：{}",
            report.text
        );
        assert!(
            !report.text.contains('`'),
            "反引号不该留在 PG 文本里：{}",
            report.text
        );
    }

    /// 解析不了的语句逐字保留并计数（不静默丢、不报成成功）
    #[test]
    fn unparseable_statements_are_kept_and_counted() {
        let report = transpile_with_report(
            "SELECT 1;\nSELECT (\nSELECT 3;",
            SqlDialect::Ansi,
            SqlDialect::Postgres,
        );
        assert_eq!(report.transpiled, 1);
        assert_eq!(report.kept_verbatim, 1);
        assert!(
            report.text.contains("SELECT (\nSELECT 3"),
            "没写完的那条必须逐字在里面：{}",
            report.text
        );
    }

    /// 语句外的注释与空行原样保留（注释归用户）
    #[test]
    fn comments_and_blank_lines_survive() {
        let report = transpile_with_report(
            "-- 头注释\nSELECT `a` FROM `t`;\n\n-- 尾注释\n",
            SqlDialect::Mysql,
            SqlDialect::Postgres,
        );
        assert!(report.text.contains("-- 头注释"), "{}", report.text);
        assert!(report.text.contains("-- 尾注释"), "{}", report.text);
        assert!(report.text.contains("\"a\""), "{}", report.text);
    }

    #[test]
    fn empty_input_comes_back_unchanged() {
        let report = transpile_with_report("", SqlDialect::Ansi, SqlDialect::Postgres);
        assert_eq!(report.text, "");
        assert_eq!(report.transpiled, 0);
        assert_eq!(report.kept_verbatim, 0);
    }

    #[test]
    fn same_dialect_transpile_keeps_the_text_readable() {
        // 同方言不必“不变”，但必须仍是能解析的 SQL、且语句不丢
        let report = transpile_with_report(
            "SELECT a, b FROM t WHERE x = 1;",
            SqlDialect::Postgres,
            SqlDialect::Postgres,
        );
        assert_eq!(report.kept_verbatim, 0, "{}", report.text);
        assert!(report.text.contains("SELECT"), "{}", report.text);
    }

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
