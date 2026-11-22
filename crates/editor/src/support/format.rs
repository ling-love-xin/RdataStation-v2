//! 格式化（B10）：把「格式化什么、改成什么样、光标放哪」算清楚
//!
//! ## 为什么规划与落地分开
//!
//! 格式化有三个容易出错的地方，而它们**都能在没有窗口的情况下穷举**：
//!
//! 1. **格式化什么**：有选区就格式化选区——选区先**吸附到完整语句**边界（与 DataGrip /
//!    VS Code 一致：用户指着 `where x=1` 也是想排包含它的那条 `SELECT`），没选区才整篇；
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

use engine::sql::{FormatOptions, SqlDialect, SqlEngine, split_statements};

/// 【格式化设置页】`SqlFormat` 落盘值 → [`FormatOptions`]（纯函数；设置异常回退引擎默认，
/// 不猜：未知档位走默认档，越界数值夹回合法区间）。
///
/// 两个消费方共用同一份口径：编辑器格式化（Ctrl+Shift+F / 保存时格式化，经
/// `view/host/format.rs::format_options`）与设置页实时预览（app 装配经
/// `settings::install_sql_format_preview` 注入）——改映射只动这里，两边永远一致。
///
/// 十二个旋钮里只有 `keyword_case` 与 `normalize_operators` 不走 lax 的排版能力：
/// 前者用 sqlglot 词法器判定大小写（lax 词表只有 85 个通用词），后者是本项目自己的
/// 后处理（lax 明确不规范化 token 间距），见 `crates/sql/src/formatter.rs` 模块头。
pub fn options_from_settings(prefs: &settings::model::SqlFormat) -> FormatOptions {
    use engine::sql::{ClauseStyle, CommaStyle, IndentChar, KeywordCase, NewLineKind};
    // 忽略标记为空会匹配任何注释（等于"全篇不排版"）。设置层已拒绝空值，
    // 这里兜住"配置文件被手工改坏"的情况：回退引擎默认标记，不用空串。
    let node_mark = prefs.ignore_node_comment_text.trim();
    let file_mark = prefs.ignore_file_comment_text.trim();
    FormatOptions {
        keyword_case: match prefs.keyword_case.as_str() {
            "upper" => KeywordCase::Uppercase,
            "lower" => KeywordCase::Lowercase,
            _ => KeywordCase::Preserve,
        },
        indent_char: if prefs.indent_char == "tab" {
            IndentChar::Tab
        } else {
            IndentChar::Space
        },
        indent_width: prefs.indent_width.clamp(2, 8) as u8,
        line_width: prefs.line_width.clamp(40, 200) as u16,
        clause_style: if prefs.clause_style == "fill" {
            ClauseStyle::Fill
        } else {
            ClauseStyle::Expanded
        },
        comma_style: if prefs.comma_style == "leading" {
            CommaStyle::Leading
        } else {
            CommaStyle::Trailing
        },
        new_line_kind: match prefs.new_line_kind.as_str() {
            "lf" => NewLineKind::Lf,
            "crlf" => NewLineKind::Crlf,
            _ => NewLineKind::Auto,
        },
        ignore_node_comment_text: if node_mark.is_empty() {
            FormatOptions::default().ignore_node_comment_text
        } else {
            node_mark.to_string()
        },
        ignore_file_comment_text: if file_mark.is_empty() {
            FormatOptions::default().ignore_file_comment_text
        } else {
            file_mark.to_string()
        },
        blank_lines_between: prefs.blank_lines_between.clamp(0, 2) as u8,
        normalize_operators: prefs.normalize_operators,
    }
}

/// 【格式化设置页】实时预览的固定示例（刻意覆盖全部旋钮的可见效果：多列 SELECT
/// 换行见逗号风格与缩进、JOIN/WHERE 见子句排版、字符串字面量见「内容不动」、
/// 两条语句见空行数）。按 ANSI 方言格式化（预览卡标题注明实际跟随绑定连接）。
pub const PREVIEW_SAMPLE_SQL: &str = "select u.id, u.name, o.total from users u join orders o on o.user_id = u.id where o.status = 'paid'; select count(*) from users";

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
/// **格式化**用这个口径（排版猜错只是难看）；**转译**要用 [`dialect_of_known`]
/// （转译猜错是把语句改成别的意思）。
pub fn dialect_of(db_type: &str) -> SqlDialect {
    dialect_of_known(db_type).unwrap_or(SqlDialect::Ansi)
}

/// 驱动类型 → sqlglot 方言；**认不出返回 `None`**
///
/// 【B10 转译用】转译的方向错不得：源方言错了，sqlglot 会按错误的读法解释引号、函数与
/// 分页语法，输出看着像 SQL 但意思变了。所以未绑定连接（空驱动名）或认不出的驱动
/// 一律 `None`，由调用方拒绝并给原因。
pub fn dialect_of_known(db_type: &str) -> Option<SqlDialect> {
    let normalized = db_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with("mysql") {
        Some(SqlDialect::Mysql)
    } else if normalized.starts_with("postgres") || normalized.starts_with("pg") {
        Some(SqlDialect::Postgres)
    } else if normalized.starts_with("sqlite") {
        Some(SqlDialect::Sqlite)
    } else if normalized.starts_with("duckdb") {
        Some(SqlDialect::Duckdb)
    } else if normalized.starts_with("mssql") || normalized.starts_with("sqlserver") {
        Some(SqlDialect::MsSQL)
    } else if normalized.starts_with("oracle") {
        Some(SqlDialect::Oracle)
    } else if normalized.starts_with("ansi")
        || normalized.starts_with("standard")
        || normalized.starts_with("generic")
    {
        Some(SqlDialect::Ansi)
    } else {
        None
    }
}

/// 规划一次格式化
///
/// `selection` 是当前选区（无选区时 `start == end`）。有选区时**只动这一片**：
/// 用户选了一段就是想只动那一段，顺手把整篇重排是越权。
///
/// 选区会先**吸附到完整语句**边界（见 [`expand_to_statements`]）：只选 `where x=1`
/// 时排的是包含它的那条语句。原因是排版引擎（`lax-sql`）只负责「把给它的这段排好」，
/// 它不知道语句边界；裸片段排出来会缺上下文，观感也不稳。
///
/// `options` 是格式化选项（行宽 / 缩进 / 子句风格 / 关键字大小写…），由设置注入。
pub fn plan(
    text: &str,
    selection: Range<usize>,
    dialect: SqlDialect,
    options: FormatOptions,
) -> FormatPlan {
    let has_selection = selection.end > selection.start;
    if has_selection {
        let clamped = expand_to_statements(text, clamp_range(text, selection));
        let slice = &text[clamped.clone()];
        let report = SqlEngine::format_report(slice, dialect, options);
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

    let report = SqlEngine::format_report(text, dialect, options);
    let cursor = map_offset(text, &report.text, selection.start);
    FormatPlan {
        text: report.text,
        formatted: report.formatted,
        kept_verbatim: report.kept_verbatim,
        selection: None,
        cursor,
    }
}

/// 选区钳制（选区可能来自内核，理论上不会越界；越界也不能 panic）
///
/// **只做边界钳制**：不判断选区是否切断字符串 / 注释 / 半个 token。那是
/// [`expand_to_statements`] 的职责——它把边界推到语句首尾，绝大多数“半截”随之消失。
pub(crate) fn clamp_range(text: &str, range: Range<usize>) -> Range<usize> {
    let len = text.len();
    let start = range.start.min(len);
    let end = range.end.min(len).max(start);
    start..end
}

/// 选区吸附：把落在语句内部的选区扩到**完整语句**边界
///
/// 规则：与选区**相交**的所有语句首尾相接（选到两句的中间就两句一起排）。
/// 没有相交时原样返回——选区落在语句之间的空白或纯注释区，那是用户真的只想选空白，
/// 替他扩大没有意义。
///
/// 用 `split_statements` 而不是自算：它已经处理好引号双写、`$$ … $$`、块注释与
/// 未闭合引号（见 `rds-sql/src/split.rs` 模块头），且保证区间落在字符边界上。
pub(crate) fn expand_to_statements(text: &str, range: Range<usize>) -> Range<usize> {
    if range.end <= range.start {
        return range;
    }
    let mut hit: Option<Range<usize>> = None;
    for span in split_statements(text) {
        // 相交（区间左闭右开）：`span.end` 不含分隔分号，所以“只选了分号”不会吸附
        if span.start < range.end && range.start < span.end {
            hit = Some(match hit {
                Some(prev) => prev.start.min(span.start)..prev.end.max(span.end),
                None => span.start..span.end,
            });
        }
    }
    hit.unwrap_or(range)
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
    use super::{dialect_of, dialect_of_known, expand_to_statements, map_offset, plan};
    use engine::sql::{FormatOptions, SqlDialect};

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

    /// 【B10 转译】认不出就是认不出——转译要用这个口径（见 `source_dialect`）
    #[test]
    fn the_strict_lookup_refuses_to_guess() {
        assert_eq!(dialect_of_known("mysql"), Some(SqlDialect::Mysql));
        assert_eq!(dialect_of_known("duckdb"), Some(SqlDialect::Duckdb));
        assert_eq!(dialect_of_known("standard-sql"), Some(SqlDialect::Ansi));
        assert_eq!(dialect_of_known("clickhouse"), None);
        assert_eq!(dialect_of_known(""), None, "未绑定连接 = 不知道源方言");
    }

    #[test]
    fn a_selection_is_formatted_on_its_own() {
        let text = "select 1;\n\n-- 这段别动\nselect a,b from t where x=1\n";
        let selection = std::ops::Range {
            start: text.find("select a,b").expect("选区在"),
            end: text.find("where x=1").expect("选区在") + "where x=1".len(),
        };
        let plan = plan(
            text,
            selection.clone(),
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        // 选区之前（含注释）逐字不动
        assert!(
            plan.text.starts_with("select 1;\n\n-- 这段别动\n"),
            "选区外的内容不该被碰：{:?}",
            plan.text
        );
        // 选区内部被排版（lax 只调空白，不碰大小写：原文小写就保持小写）
        let selected = plan.selection.clone().expect("有选区");
        let formatted = &plan.text[selected.clone()];
        // 选区吸附到语句起点，而 `split_statements` 把**语句前的注释**算作该语句的一部分
        // （`rds-sql/src/split.rs` 模块头“注释归属”），所以选区首行是那条注释。
        // 注释文本一字未改 —— lax 只动空白，`apply_options` 也不碰注释区间。
        assert!(
            formatted.starts_with("-- 这段别动"),
            "语句前导注释随语句一起吸附：{formatted:?}"
        );
        assert!(
            formatted.ends_with("select\n  a,\n  b\nfrom\n  t\nwhere\n  x=1"),
            "选区应被排版成子句分段：{formatted:?}"
        );
        // 格式化之后那一段仍然被选中（用户能接着改）
        assert!(
            plan.text[selected].contains("select\n  a,"),
            "格式化结果应当保持选中"
        );
        assert_eq!(plan.formatted, 1);
        assert_eq!(plan.kept_verbatim, 0);
    }

    /// 选区吸附：只选了语句内部的一小段，排的是**整条语句**（DataGrip / VS Code 口径）
    #[test]
    fn a_selection_inside_a_statement_expands_to_the_whole_statement() {
        let text = "select 1;\nselect a,b from t where x=1;\nselect 3;\n";
        // 只选 `select a,b`（第二条语句的前半截）
        let start = text.find("select a,b").expect("选区在");
        let plan = plan(
            text,
            start..start + "select a,b".len(),
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        // 第一条、第三条逐字不动（没被顺手重排）
        assert!(
            plan.text.starts_with("select 1;\n"),
            "选区外的第一条不该被碰：{:?}",
            plan.text
        );
        assert!(
            plan.text.ends_with("select 3;\n"),
            "选区外的第三条不该被碰：{:?}",
            plan.text
        );
        // 第二条被**整条**排版：`from` / `where` 都落到独行（只排前半截做不到这点）
        assert!(
            plan.text
                .contains("select\n  a,\n  b\nfrom\n  t\nwhere\n  x=1"),
            "应当排的是整条语句：{:?}",
            plan.text
        );
        assert_eq!(plan.formatted, 1, "只有一条语句被排：{:?}", plan.text);
    }

    /// 选区跨两条语句 → 两条一起吸附（首尾相接），第三条不碰
    #[test]
    fn a_selection_spanning_two_statements_expands_to_both() {
        let text = "select 1;\nselect a,b from t where x=1;\nselect c,d from u;\n";
        let start = text.find("where x=1").expect("选区在");
        let end = text.find("from u").expect("选区在") + "from u".len();
        let plan = plan(text, start..end, SqlDialect::Ansi, FormatOptions::default());
        assert!(
            plan.text.starts_with("select 1;\n"),
            "第一条不在选区里，不该被碰：{:?}",
            plan.text
        );
        assert!(
            plan.text
                .contains("select\n  a,\n  b\nfrom\n  t\nwhere\n  x=1"),
            "第二条应当被整条排版：{:?}",
            plan.text
        );
        assert!(
            plan.text.contains("select\n  c,\n  d\nfrom\n  u"),
            "第三条应当被整条排版：{:?}",
            plan.text
        );
        assert_eq!(plan.formatted, 2, "两条语句被排：{:?}", plan.text);
    }

    /// 选区落在语句之间的空白 → **不吸附**（用户真的只选了空白）
    #[test]
    fn a_selection_in_the_gap_between_statements_is_not_expanded() {
        let text = "select 1;\n\nselect 2;\n";
        let gap = text.find("\n\n").expect("有空行") + 1;
        assert_eq!(expand_to_statements(text, gap..gap + 1), gap..gap + 1);
        // 落到空白 → 排版不动任何东西（整篇等于原文）
        let plan = plan(
            text,
            gap..gap + 1,
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        assert!(!plan.changes(text), "选区是空白，不该改动：{:?}", plan.text);
    }

    /// 区间吸附本身：相交取首尾、不相交原样返回
    #[test]
    fn expand_takes_the_outermost_touching_statements() {
        let text = "select 1;\nselect 2;\n";
        // 完全在第一条内 → 第一条（不含分号，`end` 是分号前）
        assert_eq!(expand_to_statements(text, 3..4), 0..8);
        // 跨两条 → 合到首尾
        assert_eq!(expand_to_statements(text, 3..12), 0..18);
        // 只选分号 → 不与任何语句相交 → 原样
        assert_eq!(expand_to_statements(text, 8..9), 8..9);
    }

    #[test]
    fn the_whole_document_is_formatted_and_the_cursor_stays_in_its_statement() {
        let text = "select 1;\nselect a,b from t where x=1;\nselect 3;";
        // 光标落在第二条语句的 `where` 上
        let offset = text.find("where").expect("找得到") + 2;
        let plan = plan(
            text,
            offset..offset,
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        assert!(plan.text.contains("select\n  1;"), "{:?}", plan.text);
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

    /// **行为变化**：没写完的尾句也会被排版（lax 永不拒绝输入），不再有“解析失败保留”
    ///
    /// 旧实现用 sqlglot 解析，第二句缺右括号就整条跳过并计数；现在两句都会过一遍排版，
    /// `kept_verbatim` 只留作引擎内部异常时的兜底（正常输入恒 0）。
    #[test]
    fn incomplete_tail_is_typeset_instead_of_skipped() {
        let text = "select 1;\nselect (\n";
        let plan = plan(text, 0..0, SqlDialect::Ansi, FormatOptions::default());
        assert_eq!(plan.formatted, 2, "两句都会过排版：{:?}", plan.text);
        assert_eq!(
            plan.kept_verbatim, 0,
            "不再有“解析失败保留”：{:?}",
            plan.text
        );
        assert!(plan.changes(text), "整篇应当被重排：{:?}", plan.text);
        assert!(
            plan.text.contains('('),
            "未写完的内容不能丢：{:?}",
            plan.text
        );
    }

    #[test]
    fn an_already_formatted_document_is_left_alone() {
        let text = "SELECT\n  1;\n\nSELECT\n  2;\n";
        let plan = plan(text, 0..0, SqlDialect::Ansi, FormatOptions::default());
        assert!(
            !plan.changes(text),
            "已经是格式化后的样子就不该改动：{:?}",
            plan.text
        );
    }

    #[test]
    fn mapping_falls_back_to_clamping_outside_statements() {
        let old = "select 1;\n\n";
        let new = "SELECT\n  1;\n";
        // 落在语句之外（结尾空行）→ 钳到新文本长度（不断言具体值，只断言不越界）
        let mapped = map_offset(old, new, old.len());
        assert!(
            mapped <= new.len(),
            "{mapped} 越过了新文本长度 {}",
            new.len()
        );
        // 越界也不 panic：先钳到旧文本长度，再保证不越新文本
        let mapped = map_offset("select 1;", new, usize::MAX);
        assert!(
            mapped <= new.len(),
            "{mapped} 越过了新文本长度 {}",
            new.len()
        );
    }
}
