//! 方言转译（B10 切片二）：把「翻成哪种方言、改哪一段、光标放哪」算清楚
//!
//! 与格式化（[`crate::format`]）同一套形状与同一条理由：**规划与落地分开**。
//! 转译有三个容易出错的地方，而它们都能在没有窗口的情况下穷举：
//!
//! 1. **转译什么**：有选区就只转译选区（用户指着那段去别的库跑），没选区才整篇；
//! 2. **哪几条没翻**：引擎是逐条转译的（[`engine::sql::SqlEngine::transpile_report`]），
//!    解析不了的语句**逐字保留**——数量必须报给用户；
//! 3. **光标去哪**：整篇转译会重排每一行，光标得按「第几条语句里的第几个字节」映射回去。
//!
//! ## 源方言必须来自连接
//!
//! 转译的方向是「源方言 → 目标方言」，**源方言错了输出就是错的**（sqlglot 会按错误的
//! 读法解释引号、函数与分页语法）。编辑器自己不知道驱动，所以源方言只从
//! [`crate::connection::ConnectionOption::db_type`] 来；未绑定连接时不猜——调用方
//! （面板）会拒绝并说明原因，这里不提供“默认 Ansi”这种静默降级。

use std::ops::Range;

use engine::sql::{SqlDialect, SqlEngine};

/// 可选的转译目标（菜单直接按这个顺序摆）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// sqlglot 方言
    pub dialect: SqlDialect,
    /// 菜单上的名字
    pub label: &'static str,
}

const fn target(dialect: SqlDialect, label: &'static str) -> Target {
    Target { dialect, label }
}

/// 全部目标（顺序即菜单顺序：常用库在前，标准 SQL 收尾）
pub const TARGETS: &[Target] = &[
    target(SqlDialect::Postgres, "PostgreSQL"),
    target(SqlDialect::Mysql, "MySQL"),
    target(SqlDialect::Sqlite, "SQLite"),
    target(SqlDialect::Duckdb, "DuckDB"),
    target(SqlDialect::MsSQL, "SQL Server"),
    target(SqlDialect::Oracle, "Oracle"),
    target(SqlDialect::Snowflake, "Snowflake"),
    target(SqlDialect::BigQuery, "BigQuery"),
    target(SqlDialect::Redshift, "Redshift"),
    target(SqlDialect::Ansi, "标准 SQL"),
];

/// 菜单里该摆的目标：**排除源方言自己**（“MySQL 转 MySQL”没有意义，那是格式化的活）
///
/// 排除而不是置灰：置灰要在行尾解释“因为你现在就是 MySQL”，而菜单里少一项
/// 本来就不会让人误以为功能坏了。
pub fn targets_for(source: SqlDialect) -> Vec<Target> {
    TARGETS
        .iter()
        .filter(|target| target.dialect != source)
        .copied()
        .collect()
}

/// 目标方言的显示名（状态栏文案用；找不到就给一个诚实的兑底）
pub fn label_of(dialect: SqlDialect) -> &'static str {
    TARGETS
        .iter()
        .find(|target| target.dialect == dialect)
        .map(|target| target.label)
        .unwrap_or("目标方言")
}

/// 一次方言转译要做的全部事情（纯计算，不碰窗口）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranspilePlan {
    /// 替换后的整篇文本
    pub text: String,
    /// 成功转译的语句数
    pub transpiled: usize,
    /// 解析不了、逐字保留的语句数（要报给用户）
    pub kept_verbatim: usize,
    /// 替换后该选中的区间（选区转译时 = 那段新文本；整篇时 = `None`）
    pub selection: Option<Range<usize>>,
    /// 替换后光标该在哪（字节偏移）
    pub cursor: usize,
}

impl TranspilePlan {
    /// 这次转译会不会真的改动文本
    pub fn changes(&self, original: &str) -> bool {
        self.text != original
    }
}

/// 规划一次转译
///
/// `selection` 是当前选区（无选区时 `start == end`）。有选区**只转译选区**。
pub fn plan(
    text: &str,
    selection: Range<usize>,
    source: SqlDialect,
    target: SqlDialect,
) -> TranspilePlan {
    let has_selection = selection.end > selection.start;
    if has_selection {
        let clamped = crate::format::clamp_range(text, selection);
        let slice = &text[clamped.clone()];
        let report = SqlEngine::transpile_report(slice, source, target);
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..clamped.start]);
        out.push_str(&report.text);
        out.push_str(&text[clamped.end..]);
        let start = clamped.start;
        let end = start + report.text.len();
        return TranspilePlan {
            text: out,
            transpiled: report.transpiled,
            kept_verbatim: report.kept_verbatim,
            selection: Some(start..end),
            cursor: end,
        };
    }

    let report = SqlEngine::transpile_report(text, source, target);
    let cursor = crate::format::map_offset(text, &report.text, selection.start);
    TranspilePlan {
        text: report.text,
        transpiled: report.transpiled,
        kept_verbatim: report.kept_verbatim,
        selection: None,
        cursor,
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{TARGETS, plan, targets_for};
    use engine::sql::{SqlDialect, split_statements};

    #[test]
    fn every_target_is_listed_once_and_the_source_is_never_offered() {
        // 目标表本身不含重复
        for (index, target) in TARGETS.iter().enumerate() {
            let later = TARGETS.iter().skip(index + 1);
            assert!(
                !later.clone().any(|other| other.dialect == target.dialect),
                "{:?} 重复了",
                target.dialect
            );
        }
        // 四个内置驱动都能找到目标（否则用户装上就能用、一点转译发现没有自己的库就怪了）
        for source in [
            SqlDialect::Mysql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::Duckdb,
        ] {
            let targets = targets_for(source);
            assert!(
                targets.iter().all(|target| target.dialect != source),
                "{source:?} 不该把自己作为目标"
            );
            assert_eq!(
                targets.len(),
                TARGETS.len() - 1,
                "{source:?} 只该少了它自己"
            );
        }
    }

    #[test]
    fn a_selection_is_transpiled_on_its_own() {
        let text = "SELECT `a` FROM `t`;\n\n-- 这段别动\nSELECT `b` FROM `u`\n";
        let selection = std::ops::Range {
            start: text.find("SELECT `b`").expect("选区在"),
            end: text.find("FROM `u`").expect("选区在") + "FROM `u`".len(),
        };
        let plan = plan(
            text,
            selection.clone(),
            SqlDialect::Mysql,
            SqlDialect::Postgres,
        );
        assert!(
            plan.text.starts_with("SELECT `a` FROM `t`;\n\n-- 这段别动\n"),
            "选区之外（含那句 MySQL 原文与注释）逐字不动：{:?}",
            plan.text
        );
        let selected = plan.selection.clone().expect("有选区");
        let transpiled = &plan.text[selected];
        assert!(transpiled.contains("\"b\""), "选区被翻了：{transpiled:?}");
        assert!(!transpiled.contains('`'), "选区里不该留反引号：{transpiled:?}");
        assert_eq!(plan.transpiled, 1);
        assert_eq!(plan.kept_verbatim, 0);
    }

    #[test]
    fn the_whole_document_is_transpiled_and_the_cursor_stays_in_its_statement() {
        let text = "SELECT 1;\nSELECT `a` FROM `t`;\nSELECT 3;";
        let offset = text.find("FROM").expect("找得到") + 2;
        let plan = plan(text, offset..offset, SqlDialect::Mysql, SqlDialect::Postgres);
        assert_eq!(plan.transpiled, 3, "{:?}", plan.text);
        assert!(plan.selection.is_none(), "整篇转译不选中任何东西");
        let spans = split_statements(&plan.text);
        let second = spans.get(1).expect("第二条还在");
        assert!(
            plan.cursor >= second.start && plan.cursor <= second.end,
            "光标应当留在第二条语句里：cursor={} span={:?}",
            plan.cursor,
            second
        );
    }

    #[test]
    fn a_script_never_loses_statements_here_either() {
        // 这条与引擎侧同源，但在**编辑器这一层**再钉一次：面板拿到的就是这份计划
        let text = "SELECT 1; SELECT 2; SELECT 3;";
        let plan = plan(text, 0..0, SqlDialect::Mysql, SqlDialect::Postgres);
        assert_eq!(split_statements(&plan.text).len(), 3, "{:?}", plan.text);
    }

    #[test]
    fn unparseable_statements_are_kept_and_counted() {
        let text = "SELECT 1;\nSELECT (\n";
        let plan = plan(text, 0..0, SqlDialect::Mysql, SqlDialect::Postgres);
        assert_eq!(plan.transpiled, 1);
        assert_eq!(plan.kept_verbatim, 1, "没写完的那句要如实报出来");
        assert!(plan.text.contains("SELECT ("), "{:?}", plan.text);
    }

    #[test]
    fn a_document_without_a_source_dialect_is_not_guessed_here() {
        // 这里只保证：把 Ansi 当源时也能工作（**谁来给源方言是面板的事**——
        // 未绑定连接时面板会拒绝，而不是悄悄用 Ansi 翻出一份错东西）
        let plan = plan("SELECT `a` FROM `t`", 0..0, SqlDialect::Ansi, SqlDialect::Postgres);
        assert!(!plan.text.is_empty());
    }
}
