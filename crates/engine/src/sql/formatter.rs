//! SQL 格式化
//!
//! 走 sqlglot-rust 的 AST 生成器（`generate_pretty`），**不是 Rust 的 Debug 打印**。
//! （v2 迁移期的实现是 `format!("{:?}", statement)`，输出不是 SQL —— 属"四个假底座"之一，
//! 其旧测试只断言"非空"，因此没有拦住。）
//!
//! ## 取舍
//!
//! - **多语句脚本**：走 `super::script::rewrite_statements`（切分 → 逐条格式化 → 原位回填），
//!   语句之间以 `;\n\n` 拉开；**一条没写完不影响其它条**。
//! - **解析失败原样返回**：编辑中的文本（未写完）不能因为格式化成坏内容，也不报错。
//! - **注释**：只有**语句前**的注释能通过解析——实测（`crates/engine/tests/sqlglot_capabilities.rs` 的
//!   「块注释位置与可解析性」）行内 / 尾随注释会让整条语句解析失败，而解析失败即**原样返回**，
//!   所以注释**不会丢**，代价是这类语句不被格式化（用户看到的是原样文本，不是被改写过的内容）。
//!   能解析时生成器只 emit 前导注释（`gen_statement` 每个分支 `gen_comments(&s.comments)`，
//!   `generator/sql_generator.rs:206-268`）。
//!   另注：`normalize_comment`（`:181-197`）会把非 MySQL 目标的 `#` 注释改写成 `--`（已实测；方言差异，非排版差异）。
//! - **不改变语义**：只做排版；方言相关的重写属 `transpiler`，不在这里做。

use std::ops::Range;

use sqlglot_rust::{Dialect, generate_pretty, parse_statements_with_comments};

use super::engine::SqlDialect;
use super::highlight::{TokenClass, highlight_spans};
use super::script::rewrite_statements;

/// 格式化选项（sqlglot pretty 输出固定，这些选项靠安全后处理实现）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCase {
    /// 保持 sqlglot 输出原样（默认）
    Preserve,
    /// 关键字统一大写（SELECT / FROM / WHERE）
    Uppercase,
    /// 关键字统一小写
    Lowercase,
}

impl Default for KeywordCase {
    fn default() -> Self {
        Self::Preserve
    }
}

/// SQL 格式化选项
///
/// `indent_width` 是缩进空格数（sqlglot pretty 固定 2 空格，这里按配置重排行首缩进）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    /// 关键字大小写
    pub keyword_case: KeywordCase,
    /// 缩进宽度（空格数；2 = sqlglot 原生）
    pub indent_width: u8,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::Preserve,
            indent_width: 2,
        }
    }
}

/// 格式化的结果（给界面用：**改了几条 / 哪几条没动**都要说得清）
///
/// 为什么不能只返回 `String`：解析失败时旧实现是**整体原样返回**，用户在界面上看到
/// 的是“按了没反应”——不知道是“本来就是格式化好的”还是“这句解析不了”。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatReport {
    /// 格式化后的整篇文本（未格式化的语句**逐字保留**）
    pub text: String,
    /// 成功格式化的语句数
    pub formatted: usize,
    /// 解析失败、逐字保留的语句数（0 = 全篇都格式化了）
    pub kept_verbatim: usize,
}

/// 逐条格式化（**语句为单位**，解析失败的那条逐字保留）
///
/// 与 [`format`] 的区别（后者保留是为了兼容旧的调用点）：
///
/// - 旧的 `parse_statements_with_comments` 是**整篇**解析：脚本里有一句没写完，全篇都不格式化；
/// - 这里走 [`super::script::rewrite_statements`]：词法切分拿到每条语句的**字节区间**，逐条
///   格式化再**回填原位**：区间外的内容（注释、空行、没写完的那句）一个字节都不动。
pub fn format_with_report(sql: &str, dialect: SqlDialect, options: FormatOptions) -> FormatReport {
    let inner = to_inner_dialect(dialect);
    let rewritten = rewrite_statements(sql, |body| format_single(body, inner, options));
    FormatReport {
        text: rewritten.text,
        formatted: rewritten.rewritten,
        kept_verbatim: rewritten.kept_verbatim,
    }
}

/// 单独格式化一条语句（内部用；失败返回 `None`）
fn format_single(body: &str, dialect: Dialect, options: FormatOptions) -> Option<String> {
    let statements = parse_statements_with_comments(body, dialect).ok()?;
    let mut bodies: Vec<String> = Vec::with_capacity(statements.len());
    for statement in &statements {
        let text = generate_pretty(statement, dialect);
        let text = text.trim().trim_end_matches(';').trim().to_string();
        if !text.is_empty() {
            bodies.push(text);
        }
    }
    if bodies.is_empty() {
        return None;
    }
    let joined = bodies.join(";\n\n");
    Some(apply_options(&joined, options))
}

/// 对**已格式化**的语句文本应用选项（字符串 / 注释区间不碰）
///
/// sqlglot pretty 输出固定（2 空格缩进、无大小写选项），选项在这里安全后处理：
///
/// - **关键字大小写**：复用高亮词法器（[`highlight_spans`]），只改写
///   `TokenClass::Keyword` 区间。标识符 / 字符串 / 注释天然排除（TokenType 语义判定，
///   不是文本猜测）——`select 'select'` 里的字符串不会被改。
/// - **缩进宽度**：只重排**行首**缩进（pretty 缩进都在行首、2 空格倍数）；行首落在
///   字符串 / 注释区间内的行不碰（多行字符串里的缩进是内容，不是排版）。
fn apply_options(text: &str, options: FormatOptions) -> String {
    if options.keyword_case == KeywordCase::Preserve && options.indent_width == 2 {
        return text.to_string();
    }
    let spans = highlight_spans(text);
    let protected: Vec<Range<usize>> = spans
        .iter()
        .filter(|s| matches!(s.class, TokenClass::String | TokenClass::Comment))
        .map(|s| s.start..s.end)
        .collect();

    let mut out = text.to_string();

    // 1) 关键字大小写（ASCII 改写同长，区间偏移不移动；从后往前保险）
    if options.keyword_case != KeywordCase::Preserve {
        let keyword_spans: Vec<Range<usize>> = spans
            .iter()
            .filter(|s| s.class == TokenClass::Keyword)
            .map(|s| s.start..s.end)
            .collect();
        for range in keyword_spans.into_iter().rev() {
            let (head, tail) = out.split_at(range.start);
            let (mid, rest) = tail.split_at(range.end - range.start);
            let rewritten = match options.keyword_case {
                KeywordCase::Uppercase => mid.to_ascii_uppercase(),
                KeywordCase::Lowercase => mid.to_ascii_lowercase(),
                KeywordCase::Preserve => mid.to_string(),
            };
            out = format!("{head}{rewritten}{rest}");
        }
    }

    // 2) 缩进宽度（逐行处理行首空格）
    if options.indent_width != 2 {
        let width = (options.indent_width.max(1)) as usize;
        let mut out2 = String::with_capacity(out.len());
        let mut offset = 0usize;
        for line in out.split_inclusive('\n') {
            let line_start = offset;
            offset += line.len();
            let stripped = line.trim_start_matches(' ');
            let indent_bytes = line.len() - stripped.len();
            let in_protected =
                protected.iter().any(|r| r.start <= line_start && line_start < r.end);
            if indent_bytes >= 2 && indent_bytes % 2 == 0 && !in_protected {
                out2.push_str(&" ".repeat(indent_bytes / 2 * width));
                out2.push_str(stripped);
            } else {
                out2.push_str(line);
            }
        }
        out = out2;
    }

    out
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
            // 注释保真：格式化不得静默丢弃用户注释。sqlglot 对**尾随行注释**
            // 已知可解析但 pretty 输出会丢掉（2026-10-29 实测行为变化）；
            // 输入输出注释标记数不一致 → 原样返回，保护用户内容。
            if comment_mark_count(sql) != comment_mark_count(&out) {
                return sql.to_string();
            }
            out
        }
        // 空输入 / 解析失败（文本可能还没写完）：原样返回，不静默改写用户内容
        Ok(_) | Err(_) => sql.to_string(),
    }
}

/// 注释标记计数（行注释 `--` / 块注释 `/*` / MySQL `#`）——格式化保真判据
fn comment_mark_count(text: &str) -> usize {
    text.matches("--").count() + text.matches("/*").count() + text.matches('#').count()
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

    /// 【B10】逐条报告：能格式化的格式化，解析不了的那条**逐字保留**（不当它不存在）
    #[test]
    fn the_report_formats_what_it_can_and_counts_what_it_kept() {
        // 第二条从 `select (` 一直占到它那个 `;`（词法切分只认分号）——它解析不了
        let sql = "select 1;\nselect (\nselect 3;";
        let report = format_with_report(sql, SqlDialect::Ansi, FormatOptions::default());
        assert_eq!(report.formatted, 1, "只有第一句能解析：{}", report.text);
        assert_eq!(report.kept_verbatim, 1, "没写完的那句要如实计数");
        assert!(
            report.text.contains("select (\nselect 3"),
            "没写完的那句必须**逐字**在里面：{}",
            report.text
        );
        assert!(report.text.contains("SELECT\n  1"), "{}", report.text);

        let clean = format_with_report("select 1;\nselect 3;", SqlDialect::Ansi, FormatOptions::default());
        assert_eq!(clean.kept_verbatim, 0);
        assert!(
            parse_many(&clean.text, Dialect::Ansi).is_ok(),
            "全篇可解析时，结果必须可再次解析：{}",
            clean.text
        );
    }

    /// 【格式化增强】关键字大小写选项：只改关键字，字符串 / 注释 / 标识符不动
    #[test]
    fn keyword_case_only_touches_keywords() {
        // 用**前导**注释（尾随注释会让生成器丢弃注释——模块头的已知边界，不用它测改写保护）
        let upper = format_with_report(
            "-- where\nselect id, 'select' as s from t where id = 1",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Uppercase,
                indent_width: 2,
            },
        );
        let text = &upper.text;
        assert!(text.contains("SELECT"), "{text}");
        assert!(text.contains("FROM"), "{text}");
        assert!(text.contains("WHERE"), "{text}");
        // 注释内容不能被改写（前导注释被生成器 emit，逐字保留）
        assert!(text.contains("-- where"), "{text}");
        // 字符串内容不能被改写
        assert!(text.contains("'select'"), "{text}");
        // 标识符（列 / 表名）不能被改写
        assert!(text.contains("id"), "{text}");

        let lower = format_with_report(
            "SELECT ID FROM T",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Lowercase,
                indent_width: 2,
            },
        );
        let text = &lower.text;
        assert!(text.contains("select"), "{text}");
        assert!(text.contains("from"), "{text}");
        assert!(text.contains("ID"), "标识符保持原样：{text}");
        assert!(text.contains("T"), "标识符保持原样：{text}");
    }

    /// 【格式化增强】缩进宽度选项：行首缩进按配置重排，字符串区间内的行不碰
    #[test]
    fn indent_width_reindents_lines_but_not_string_internals() {
        let wide = format_with_report(
            "select a, b from t where x = 1",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Preserve,
                indent_width: 4,
            },
        );
        let text = &wide.text;
        assert!(text.contains("\n    "), "缩进应为 4 空格：{text:?}");
        // 不存在「恰好 2 空格」的缩进行（`\n    ` 含子串 `\n  `，不能用 contains 反向断言）
        assert!(
            text.lines().all(|line| {
                let indent = line.len() - line.trim_start_matches(' ').len();
                indent == 0 || indent >= 4
            }),
            "不应再留 2 空格缩进：{text:?}"
        );

        // 多行字符串：字符串内部的换行 / 缩进是内容，不能动
        let quoted = format_with_report(
            "select 'a\n  b' as v",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Preserve,
                indent_width: 4,
            },
        );
        assert!(
            quoted.text.contains("'a\n  b'"),
            "字符串内容原样：{:?}",
            quoted.text
        );
    }

    /// 【格式化增强】kept_verbatim 语句不受选项影响（逐字保留语义高于选项）
    #[test]
    fn options_never_touch_kept_verbatim_statements() {
        let report = format_with_report(
            "select 1;\nselect (\nselect 2;",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Uppercase,
                indent_width: 4,
            },
        );
        assert!(report.text.contains("select (\nselect 2"), "{}", report.text);
    }

    /// 【B10】语句之间的空白规整成 `;` + 两个换行（读感），注释一字不动
    #[test]
    fn gaps_are_normalized_but_comments_are_left_alone() {
        let out = format_with_report("select 1; select 2;", SqlDialect::Ansi, FormatOptions::default()).text;
        assert!(out.contains(";\n\nSELECT"), "语句之间要拉开：{out:?}");

        let commented = "select 1; -- 这句得留着\nselect 2;";
        let out = format_with_report(commented, SqlDialect::Ansi, FormatOptions::default()).text;
        assert!(out.contains("-- 这句得留着"), "注释不能被格式化吃掉：{out}");

        // 幂等：格式化过的文本再格式化一遍不再变
        let once = format_with_report("select 1; select 2;", SqlDialect::Ansi, FormatOptions::default()).text;
        let twice = format_with_report(&once, SqlDialect::Ansi, FormatOptions::default()).text;
        assert_eq!(once, twice, "格式化应当是幂等的");
    }
}
