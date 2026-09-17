//! 格式化（B10）：把「格式化什么、改成什么样、光标放哪」算清楚
//!
//! ## 为什么规划与落地分开
//!
//! 格式化有三个容易出错的地方，而它们**都能在没有窗口的情况下穷举**：
//!
//! 1. **格式化什么**：有选区就只格式化选区（用户指着那段），没选区才整篇；
//! 2. **哪几条没动**：引擎是逐条格式化的（[`engine::sql::SqlEngine::format_report`]），
//!    解析不了的语句**逐字保留**——这个数量必须能报给用户（“按了没反应”不可接受）；
//! 3. **光标去哪**：整篇格式化会重排每一行，光标得按「第几条语句里的第几个字节」
//!    映射过去，否则用户按一下格式化就被扔回文首。
//!
//! 落地那一步（面板里）只剩一次 `replace_all`（内核保留撤销历史）+ 恢复选区/光标。
//!
//! ## 为什么需要方言
//!
//! sqlglot 的生成器是**按方言**出文本的：拿 Ansi 去格式化 MySQL 脚本会把 `# 注释`
//! 改写成 `-- 注释`（实测）。所以调用方要给方言，而方言只能从**连接**来——
//! 编辑器自己不知道驱动是什么，那是宿主填在 [`crate::connection::ConnectionOption`] 里的。

use std::ops::Range;

use engine::sql::{SqlDialect, SqlEngine, split_statements};

/// 一次格式化要做的全部事情（纯计算，不碰窗口）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatPlan {
    /// 替换后的整篇文本
    pub text: String,
    /// 成功格式化的语句数
    pub formatted: usize,
    /// 解析失败、逐字保留的语句数（要报给用户）
    pub kept_verbatim: usize,
    /// 替换后该选中的区间（选区格式化时 = 那段新文本；整篇时 = `None`）
    pub selection: Option<Range<usize>>,
    /// 替换后光标该在哪（字节偏移）
    pub cursor: usize,
}

impl FormatPlan {
    /// 这次格式化会不会真的改动文本
    pub fn changes(&self, original: &str) -> bool {
        self.text != original
    }
}

/// 驱动类型 → sqlglot 方言
///
/// 认不出的驱动用 `Ansi`（**不是猜某个具体方言**：猜错会静默改写用户的注释与引号风格）。
pub fn dialect_of(db_type: &str) -> SqlDialect {
    let normalized = db_type.trim().to_ascii_lowercase();
    if normalized.starts_with("mysql") {
        SqlDialect::Mysql
    } else if normalized.starts_with("postgres") || normalized.starts_with("pg") {
        SqlDialect::Postgres
    } else if normalized.starts_with("sqlite") {
        SqlDialect::Sqlite
    } else if normalized.starts_with("duckdb") {
        SqlDialect::Duckdb
    } else if normalized.starts_with("mssql") || normalized.starts_with("sqlserver") {
        SqlDialect::MsSQL
    } else if normalized.starts_with("oracle") {
        SqlDialect::Oracle
    } else {
        SqlDialect::Ansi
    }
}

/// 规划一次格式化
///
/// `selection` 是当前选区（无选区时 `start == end`）。有选区**只格式化选区**：
/// 用户选了一段就是想只动那一段，顺手把整篇重排是越权。
pub fn plan(text: &str, selection: Range<usize>, dialect: SqlDialect) -> FormatPlan {
    let has_selection = selection.end > selection.start;
    if has_selection {
        let clamped = clamp_range(text, selection);
        let slice = &text[clamped.clone()];
        let report = SqlEngine::format_report(slice, dialect);
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..clamped.start]);
        out.push_str(&report.text);
        out.push_str(&text[clamped.end..]);
        let start = clamped.start;
        let end = start + report.text.len();
        return FormatPlan {
            text: out,
            formatted: report.formatted,
            kept_verbatim: report.kept_verbatim,
            selection: Some(start..end),
            cursor: end,
        };
    }

    let report = SqlEngine::format_report(text, dialect);
    let cursor = map_offset(text, &report.text, selection.start);
    FormatPlan {
        text: report.text,
        formatted: report.formatted,
        kept_verbatim: report.kept_verbatim,
        selection: None,
        cursor,
    }
}

/// 席位钳制（选区可能来自内核，理论上不会越界；越界也不能 panic）
fn clamp_range(text: &str, range: Range<usize>) -> Range<usize> {
    let len = text.len();
    let start = range.start.min(len);
    let end = range.end.min(len).max(start);
    start..end
}

/// 把光标从旧文本映射到新文本
///
/// 规则：**先找它落在第几条语句里，再按语句内的偏移映射**（格式化只重排语句内部，
/// 不会调换语句顺序）；落在语句之外的（注释 / 空行）或找不到对应语句时，钳到新文本长度。
pub fn map_offset(old: &str, new: &str, offset: usize) -> usize {
    let offset = offset.min(old.len());
    let old_spans = split_statements(old);
    let index = old_spans
        .iter()
        .position(|span| offset >= span.start && offset <= span.end);
    let Some(index) = index else {
        return offset.min(new.len());
    };
    let within = offset - old_spans[index].start;
    let new_spans = split_statements(new);
    match new_spans.get(index) {
        Some(span) => (span.start + within).min(span.end),
        None => offset.min(new.len()),
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{dialect_of, map_offset, plan};
    use engine::sql::SqlDialect;

    #[test]
    fn the_dialect_comes_from_the_driver() {
        assert_eq!(dialect_of("mysql_native"), SqlDialect::Mysql);
        assert_eq!(dialect_of("MySQL"), SqlDialect::Mysql);
        assert_eq!(dialect_of("postgres_native"), SqlDialect::Postgres);
        assert_eq!(dialect_of("sqlite"), SqlDialect::Sqlite);
        assert_eq!(dialect_of("duckdb"), SqlDialect::Duckdb);
        // 认不出不猜具体方言（猜错会静默改写注释与引号）
        assert_eq!(dialect_of("clickhouse"), SqlDialect::Ansi);
        assert_eq!(dialect_of(""), SqlDialect::Ansi);
    }

    #[test]
    fn a_selection_is_formatted_on_its_own() {
        let text = "select 1;\n\n-- 这段别动\nselect a,b from t where x=1\n";
        let selection = std::ops::Range {
            start: text.find("select a,b").expect("选区在"),
            end: text.find("where x=1").expect("选区在") + "where x=1".len(),
        };
        let plan = plan(text, selection.clone(), SqlDialect::Ansi);
        // 选区之前（含注释）逐字不动
        assert!(
            plan.text.starts_with("select 1;\n\n-- 这段别动\n"),
            "选区外的内容不该被碰：{:?}",
            plan.text
        );
        // 选区内部被格式化了（关键字大写、换行排开）——具体排版是 sqlglot 的事，
        // 这里只断言“真的重排了”与关键片段还在
        let selected = plan.selection.clone().expect("有选区");
        let formatted = &plan.text[selected.clone()];
        assert!(formatted.contains("SELECT"), "{formatted:?}");
        assert!(formatted.contains("FROM"), "{formatted:?}");
        assert!(formatted.contains("WHERE"), "{formatted:?}");
        assert!(
            formatted.contains('\n'),
            "格式化就该把语句排开（不是原样一长行）：{formatted:?}"
        );
        // 格式化之后那一段仍然被选中（用户能接着改）
        assert!(
            plan.text[selected].starts_with("SELECT"),
            "格式化结果应当保持选中"
        );
        assert_eq!(plan.formatted, 1);
        assert_eq!(plan.kept_verbatim, 0);
    }

    #[test]
    fn the_whole_document_is_formatted_and_the_cursor_stays_in_its_statement() {
        let text = "select 1;\nselect a,b from t where x=1;\nselect 3;";
        // 光标落在第二条语句的 `where` 上
        let offset = text.find("where").expect("找得到") + 2;
        let plan = plan(text, offset..offset, SqlDialect::Ansi);
        assert!(plan.text.contains("SELECT\n  1;"), "{:?}", plan.text);
        assert!(plan.selection.is_none(), "整篇格式化不选中任何东西");
        // 光标还在第二条语句的区间内
        let spans = engine::sql::split_statements(&plan.text);
        let second = spans.get(1).expect("第二条还在");
        assert!(
            plan.cursor >= second.start && plan.cursor <= second.end,
            "光标应当留在第二条语句里：cursor={} span={:?}",
            plan.cursor,
            second
        );
    }

    #[test]
    fn unparseable_statements_are_kept_and_counted() {
        let text = "select 1;\nselect (\n";
        let plan = plan(text, 0..0, SqlDialect::Ansi);
        assert_eq!(plan.formatted, 1);
        assert_eq!(plan.kept_verbatim, 1, "没写完的那句要如实报出来");
        assert!(plan.text.contains("select (\nselect") || plan.text.contains("select ("), "{:?}", plan.text);
    }

    #[test]
    fn an_already_formatted_document_is_left_alone() {
        let text = "SELECT\n  1;\n\nSELECT\n  2;\n";
        let plan = plan(text, 0..0, SqlDialect::Ansi);
        assert!(!plan.changes(text), "已经是格式化后的样子就不该改动：{:?}", plan.text);
    }

    #[test]
    fn mapping_falls_back_to_clamping_outside_statements() {
        let old = "select 1;\n\n";
        let new = "SELECT\n  1;\n";
        // 落在语句之外（结尾空行）→ 钳到新文本长度（不断言具体值，只断言不越界）
        let mapped = map_offset(old, new, old.len());
        assert!(mapped <= new.len(), "{mapped} 越过了新文本长度 {}", new.len());
        // 越界也不 panic：先钳到旧文本长度，再保证不越新文本
        let mapped = map_offset("select 1;", new, usize::MAX);
        assert!(mapped <= new.len(), "{mapped} 越过了新文本长度 {}", new.len());
    }
}
