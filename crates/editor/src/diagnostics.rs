//! 错误定位（B6）：把驱动回的错误文本落到文档里的一个具体位置
//!
//! 「错误回填」要三件事：**诊断**（在编辑器里画出出错范围）、**定位**（把光标送过去）、
//! **聚焦**（后续输入接着落在出错处）。后两件都要位置，位置从哪来是本模块的活。
//!
//! ## 位置从哪来（按可信度排序）
//!
//! | 来源 | 位置藏在哪 | 例子（真机原文） |
//! | --- | --- | --- |
//! | PG（两个驱动） | **结构化** → 引擎渲染成 `at position N` | `Query failed at position 31: …` |
//! | DuckDB | 文本：`LINE n: …` + 下一行的 `^` | `Parser Error: syntax error at or near "WHERE"` |
//! | SQLite | 文本：`near "x"` / `at offset N` | `near "WHERE": syntax error in … at offset 14` |
//! | MySQL（两个驱动） | 文本：`near 'x' at line N` / `Table 'x' doesn't exist` | `1106 (42S02): Table 'mysql.t' doesn't exist` |
//!
//! 只有 PG 的位置能走结构化字段（sqlx 的 `Display` 还会把 PG 的**源码行号**写成
//! “at line N” 附在末尾——那不是 SQL 里的位置，**绝不能拿它当行号**）。其余驱动只能
//! 从文本里认，所以这里两种都认。
//!
//! ## 认不出就不给位置
//!
//! 定位错比不定位更糟：用户照着找，找不到，还以为自己的 SQL 没问题。所以任何一步认不出
//! （或者位置越界）都返回 `None`，界面只留一句错误原因，不动光标。
//!
//! ## 为什么是纯函数
//!
//! [`site_in_document`] 不碰 GPUI：输入（整篇文档 + 这次发的 SQL + 错误文本）→ 输出
//! （字节区间 + 人读的行列 + 出错的词），可以拿真实错误文本逐条断言（测试里有 10 种）。

use std::ops::Range;

/// 出错位置（**文档坐标系**：行号列号是 1 基，偏移是 0 基字节）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorSite {
    /// 文档里的字节起点（诊断高亮与光标都从这开始）
    pub offset: usize,
    /// 高亮长度（字节）；`0` = 只知道位置，不知道有多长
    pub len: usize,
    /// 1 基行号（说给人听）
    pub line: usize,
    /// 1 基列号（按**字符**数，不是字节——与主流编辑器一致）
    pub column: usize,
    /// 出错的那个词（`near "wheree"` 里的 `wheree`；认得出就选中它）
    pub token: Option<String>,
}

impl ErrorSite {
    /// 诊断高亮的字节区间（长度 0 → 退化成 1 个字符，免得画不出东西）
    pub fn range(&self) -> Range<usize> {
        self.offset..self.offset + self.len.max(1)
    }

    /// 状态栏里的位置文案（`第 3 行 第 12 列`）
    pub fn location_text(&self) -> String {
        format!("第 {} 行 第 {} 列", self.line, self.column)
    }

    /// 诊断区间的**结束**位置（1 基行列；列按字符数）
    ///
    /// 出错词占几个字符就用几个（不知道词就退化成 1 个字符）。
    pub fn end(&self) -> (usize, usize) {
        let width = self
            .token
            .as_deref()
            .map(|token| token.chars().count())
            .filter(|count| *count > 0)
            .unwrap_or(1);
        (self.line, self.column + width)
    }
}

/// `sql` 自己坐标系里的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalSite {
    offset: usize,
    len: usize,
}

/// 从「整篇文档 + 这次发的 SQL + 错误文本」定位到文档里的出错处（**纯函数**）
///
/// `sql` 是实际发给驱动的那一段（可能是整篇，也可能只是光标所在的那条语句）：
/// 位置先在 `sql` 里算出来，再平移回文档。文档里找不到这段 SQL（用户已经改过内容）
/// 就不给位置——不猜。
pub fn site_in_document(document: &str, sql: &str, error: &str) -> Option<ErrorSite> {
    let local = site_in_sql(error, sql)?;
    let base = document.find(sql)?;
    let offset = base + local.offset;

    // 长度没给的（位置类线索）就地取一个词：既给高亮宽度、又给“出错的是哪个词”
    let len = if local.len == 0 {
        word_at(sql, local.offset).map_or(0, |(_, len)| len)
    } else {
        local.len
    };
    let token = (len > 0)
        .then(|| sql.get(local.offset..local.offset + len).map(str::to_string))
        .flatten();

    let (line, column) = line_col(document, offset);
    Some(ErrorSite {
        offset,
        len,
        line,
        column,
        token,
    })
}

/// 在 `sql` 坐标系里定位（顺序就是模块文档里的可信度顺序）
fn site_in_sql(error: &str, sql: &str) -> Option<LocalSite> {
    // ① 引擎渲染出来的结构化位置（0 基字节；越界当没有）
    if let Some(offset) = position_marker(error).filter(|offset| *offset < sql.len()) {
        return Some(LocalSite { offset, len: 0 });
    }
    // ② `LINE n: …` + 下一行的 `^`（DuckDB）
    if let Some(site) = caret_site(error, sql) {
        return Some(site);
    }
    // ③ `at offset N`（SQLite 的 `SqlInputError`；0 基字节）
    if let Some(offset) = offset_marker(error).filter(|offset| *offset < sql.len()) {
        return Some(LocalSite { offset, len: 0 });
    }
    // ④ 消息里引号包着的那个词（表名 / 列名 / `near '…'` 片段）
    quoted_token_site(error, sql)
}

/// 引擎的结构化位置：`at position N`
fn position_marker(error: &str) -> Option<usize> {
    let tail = error.split("at position ").nth(1)?;
    number_prefix(tail)
}

/// `at offset N`（SQLite）
fn offset_marker(error: &str) -> Option<usize> {
    let tail = error.split("at offset ").nth(1)?;
    number_prefix(tail)
}

/// 取字符串开头的数字（`31: syntax error` / `14 (SQL: …)` 都得能取到）
fn number_prefix(text: &str) -> Option<usize> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

/// `LINE n: …` + 下一行的 `^`（DuckDB 的报错格式）
///
/// 两处细节由真机原文钉住：
///
/// * 插入符对齐的是**整行错误文本**（含 `LINE 1: ` 前缀），所以列号要减去前缀长度；
/// * 插入符那一行后面还可能跟着引擎拼的 ` (SQL: …)`（消息没有以换行结尾时），
///   所以只看行**开头**的空白与 `^`。
fn caret_site(error: &str, sql: &str) -> Option<LocalSite> {
    let message = without_sql_suffix(error);
    let mut lines = message.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("LINE ") else {
            continue;
        };
        let Some((number, _)) = rest.split_once(':') else {
            continue;
        };
        let line_number: usize = number.trim().parse().ok()?;
        let caret = lines.next()?.trim_end();
        // 插入符对齐的是整行错误文本：`LINE 1: ` 这段前缀要从列号里减掉
        let prefix_len =
            (line.len() - trimmed.len()) + "LINE ".len() + number.len() + ": ".len();
        let caret_column = caret.chars().take_while(|c| *c == ' ').count();
        let column = caret_column.checked_sub(prefix_len)? + 1;
        let offset = offset_for_line_col(sql, line_number, column)?;
        return Some(LocalSite { offset, len: 0 });
    }
    None
}

/// 消息里引号（`'` / `"` / `` ` ``）包着的词，在 SQL 里找到它就当出错处
///
/// 真机里的三种形态：
///
/// * `near 'WHERE' at line 1`（MySQL 语法错误）
/// * `Table 'mysql.rds_affected_probe_11092' doesn't exist`（MySQL 找不到表：带库名前缀）
/// * `关系 "t" 不存在`（PG 本地化消息）
/// * `no such table: rds_affected_probe_11092`（SQLite：没有引号，冒号后面跟词）
fn quoted_token_site(error: &str, sql: &str) -> Option<LocalSite> {
    for candidate in quoted_candidates(error) {
        // 带库名/模式名前缀的先原样试，再试最后一段（`mysql.t` → `t`）
        let mut tries = vec![candidate.clone()];
        if let Some((_, last)) = candidate.rsplit_once('.') {
            tries.push(last.to_string());
        }
        for token in tries {
            if token.is_empty() {
                continue;
            }
            if let Some(offset) = sql.find(&token) {
                return Some(LocalSite {
                    offset,
                    len: token.len(),
                });
            }
        }
    }
    None
}

/// 消息里可能指向出错处的那些词（按出现顺序）
fn quoted_candidates(error: &str) -> Vec<String> {
    let message = without_sql_suffix(error);
    let mut candidates = Vec::new();

    // 引号包着的词
    for quote in ['\'', '"', '`'] {
        let mut rest = message;
        while let Some((_, tail)) = rest.split_once(quote) {
            let Some((inner, after)) = tail.split_once(quote) else {
                break;
            };
            if !inner.is_empty() {
                candidates.push(inner.to_string());
            }
            rest = after;
        }
    }

    // SQLite 的 `no such table: x` / `no such column: x`（没有引号）
    for marker in ["no such table: ", "no such column: ", "no such function: "] {
        if let Some(tail) = message.split(marker).nth(1) {
            let word: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !word.is_empty() {
                candidates.push(word);
            }
        }
    }

    candidates
}

/// 去掉引擎拼在末尾的 ` (SQL: …)`（不然后面的解析会把 SQL 里的词当成消息里的词）
fn without_sql_suffix(error: &str) -> &str {
    error.split(" (SQL: ").next().unwrap_or(error)
}

/// `text` 里第 `line` 行（1 基）第 `column` 列（1 基**字符**）的字节偏移
///
/// 列号超出该行长度时贴到行尾（驱动给的插入符可能指在行尾之后）。
fn offset_for_line_col(text: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut start = 0;
    for _ in 1..line {
        let newline = text[start..].find('\n')?;
        start += newline + 1;
    }
    let line_text = text[start..].split('\n').next().unwrap_or("");
    let within = line_text
        .char_indices()
        .nth(column - 1)
        .map(|(offset, _)| offset)
        .unwrap_or(line_text.len());
    Some(start + within)
}

/// `offset` 处的标识符（往回找到词首，再吃到词尾）；给不出就 `None`
fn word_at(text: &str, offset: usize) -> Option<(usize, usize)> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map(|(index, _)| index)
        .unwrap_or(offset);
    let end = text[offset..]
        .char_indices()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map(|(index, c)| offset + index + c.len_utf8())
        .unwrap_or(offset);
    (end > start).then_some((start, end - start))
}

/// 1 基的行列（列按**字符**数）
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let mut line = 1;
    let mut line_start = 0;
    for (index, c) in text.char_indices() {
        if index >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    let column = text[line_start..offset].chars().count() + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ErrorSite, site_in_document, site_in_sql};

    /// 定位到的文档片段（断言“眼睛看到的”就是那个词）
    fn located(document: &str, sql: &str, error: &str) -> String {
        let site = site_in_document(document, sql, error).expect("应当定位到");
        let range = site.range();
        document[range].to_string()
    }

    /// MySQL 语法错误：`near 'WHERE' at line 1`（真机原文，sqlx 与原生驱动两种外壳）
    #[test]
    fn mysql_syntax_error_lands_on_the_offending_word() {
        let sql = "SELECT * FROM WHERE";
        let sqlx = "[DB_QUERY] Query failed: error returned from database: 1064 (42000): You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near 'WHERE' at line 1 (SQL: SELECT * FROM WHERE)";
        assert_eq!(located(sql, sql, sqlx), "WHERE");

        let native = "[DB_QUERY] Query failed: Server error: `ERROR 1064 (42000): You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near 'WHERE' at line 1' (SQL: SELECT * FROM WHERE)";
        assert_eq!(located(sql, sql, native), "WHERE");
    }

    /// MySQL 找不到表：`Table 'mysql.t' doesn't exist`——带库名前缀，要退到最后一段
    #[test]
    fn mysql_missing_table_strips_the_database_prefix() {
        let sql = "SELECT no_such_column_xyz FROM rds_affected_probe_11092";
        let error = "[DB_QUERY] Query failed: error returned from database: 1146 (42S02): Table 'mysql.rds_affected_probe_11092' doesn't exist (SQL: SELECT no_such_column_xyz FROM rds_affected_probe_11092)";
        assert_eq!(located(sql, sql, error), "rds_affected_probe_11092");
    }

    /// SQLite 语法错误：`near "WHERE": syntax error in … at offset 14`（真机原文）
    #[test]
    fn sqlite_syntax_error_uses_the_near_token_or_the_offset() {
        let sql = "SELECT * FROM WHERE";
        let error = "[DB_QUERY] Query failed: near \"WHERE\": syntax error in SELECT * FROM WHERE at offset 14 (SQL: SELECT * FROM WHERE)";
        // `near "…"` 与 `at offset` 都指向 WHERE（偏移 14 正是它的词首）
        assert_eq!(located(sql, sql, error), "WHERE");

        // 没有 `near` 时也认 `at offset`（rusqlite 的 `SqlInputError` 字段就渲染成它）
        let only_offset = "[DB_QUERY] Query failed: syntax error in SELECT * FROM WHERE at offset 14 (SQL: SELECT * FROM WHERE)";
        assert_eq!(located(sql, sql, only_offset), "WHERE");
    }

    /// SQLite 找不到表/列：`no such table: x`（真机原文，没有引号）
    #[test]
    fn sqlite_missing_names_have_no_quotes() {
        let sql = "SELECT no_such_column_xyz FROM rds_affected_probe_11092";
        let error = "[DB_QUERY] Query failed: no such table: rds_affected_probe_11092 (SQL: SELECT no_such_column_xyz FROM rds_affected_probe_11092)";
        assert_eq!(located(sql, sql, error), "rds_affected_probe_11092");

        let column = "[DB_QUERY] Query failed: no such column: no_such_column_xyz (SQL: SELECT no_such_column_xyz FROM t)";
        assert_eq!(located(sql, sql, column), "no_such_column_xyz");
    }

    /// DuckDB：`LINE n: …` + 插入符（对齐的是**整行错误文本**，含 `LINE 1: ` 前缀）
    #[test]
    fn duckdb_caret_points_at_the_token() {
        let sql = "SELECT * FROM WHERE";
        let error = "[DB_QUERY] Query failed: Parser Error: syntax error at or near \"WHERE\"\n\nLINE 1: SELECT * FROM WHERE\n                      ^ (SQL: SELECT * FROM WHERE)";
        assert_eq!(located(sql, sql, error), "WHERE");

        let table = "SELECT no_such_column_xyz FROM rds_affected_probe_11092";
        let error = "[DB_QUERY] Query failed: Catalog Error: Table with name rds_affected_probe_11092 does not exist!\nDid you mean \"duckdb_types\"?\n\nLINE 1: SELECT no_such_column_xyz FROM rds_affected_probe_11092\n                                       ^ (SQL: SELECT no_such_column_xyz FROM rds_affected_probe_11092)";
        assert_eq!(located(table, table, error), "rds_affected_probe_11092");
    }

    /// PG（结构化位置 → 引擎渲染成 `at position N`）：位置最准，优先于文本线索
    #[test]
    fn postgres_structured_position_wins() {
        let sql = "SELECT * FROM WHERE";
        let error = "[DB_QUERY] Query failed at position 14: syntax error at or near \"WHERE\" at line 1240 (SQL: SELECT * FROM WHERE)";
        let site = site_in_document(sql, sql, error).expect("应当定位到");
        assert_eq!(site.offset, 14);
        assert_eq!((site.line, site.column), (1, 15));
        assert_eq!(site.token.as_deref(), Some("WHERE"));

        // sqlx 的 “at line 1240” 是 PG **源码行号**，不是 SQL 里的行号：不能当成位置用
        assert_eq!(site.line, 1, "不能被 at line 1240 带偏");
    }

    /// PG 本地化消息（中文）：词在引号里，照样能落位
    #[test]
    fn postgres_localised_message_still_locates() {
        let sql = "SELECT no_such_column_xyz FROM rds_affected_probe_11092";
        let error = "[DB_QUERY] Query failed: error returned from database: 关系 \"rds_affected_probe_11092\" 不存在 at line 1501 (SQL: SELECT no_such_column_xyz FROM rds_affected_probe_11092)";
        assert_eq!(located(sql, sql, error), "rds_affected_probe_11092");
    }

    /// 语句只是文档的一段：位置要平移回整篇文档
    #[test]
    fn a_statement_inside_a_document_maps_back_to_the_document() {
        let document = "-- 头部注释\nSELECT 1;\nSELECT * FROM WHERE;\nSELECT 2;\n";
        let sql = "SELECT * FROM WHERE";
        let error = "[DB_QUERY] Query failed: near \"WHERE\": syntax error in SELECT * FROM WHERE at offset 14 (SQL: SELECT * FROM WHERE)";
        let site = site_in_document(document, sql, error).expect("应当定位到");
        assert_eq!((site.line, site.column), (3, 15), "第 3 行（不是第 1 行）");
        assert_eq!(&document[site.range()], "WHERE");
        assert_eq!(site.token.as_deref(), Some("WHERE"));
    }

    /// 内容已经改过（文档里找不到那段 SQL）：不给位置，只提示
    #[test]
    fn a_changed_document_gets_no_position() {
        let document = "SELECT 1";
        let sql = "SELECT * FROM WHERE";
        let error = "Query failed: syntax error at or near \"WHERE\" (SQL: SELECT * FROM WHERE)";
        assert!(site_in_document(document, sql, error).is_none());
    }

    /// MySQL 找不到列：`Unknown column 'x' in 'field list'`——位置与词名都要拿到
    #[test]
    fn mysql_unknown_column_gives_both_position_and_token() {
        let sql = "SELECT no_such_column_xyz FROM t";
        let error = "[DB_QUERY] Query failed: error returned from database: 1054 (42S22): Unknown column 'no_such_column_xyz' in 'field list' (SQL: SELECT no_such_column_xyz FROM t)";
        let site = site_in_document(sql, sql, error).expect("应当定位到");
        assert_eq!(site.token.as_deref(), Some("no_such_column_xyz"));
        assert_eq!(&sql[site.range()], "no_such_column_xyz");
        assert_eq!((site.line, site.column), (1, 8));
    }

    /// 认不出位置的错误：没有位置，但也不编一个出来
    #[test]
    fn unrecognised_errors_report_no_position() {
        let sql = "SELECT 1";
        for error in [
            "Query failed: connection reset by peer (SQL: SELECT 1)",
            "Query failed: Query cancelled (SQL: SELECT 1)",
            "Query failed: Query timed out after 1000ms (SQL: SELECT 1)",
        ] {
            assert!(site_in_document(sql, sql, error).is_none(), "{error}");
        }
    }

    /// 位置越界（驱动给的位置超出语句长度）：当作没有，不钳到末尾
    #[test]
    fn out_of_range_positions_are_dropped() {
        let sql = "SELECT 1";
        let error = "Query failed at position 99: boom (SQL: SELECT 1)";
        assert!(site_in_sql(error, sql).is_none());
    }

    /// 位置文案是给人看的 1 基行列
    #[test]
    fn location_text_is_one_based() {
        let site = ErrorSite {
            offset: 0,
            len: 0,
            line: 3,
            column: 12,
            token: None,
        };
        assert_eq!(site.location_text(), "第 3 行 第 12 列");
        assert_eq!(site.range(), 0..1, "长度 0 也要画出 1 个字符");
        assert_eq!(site.end(), (3, 13), "不知道词就占 1 个字符");
    }

    /// 诊断区间的结束位置按出错词的**字符**数算（多字节也要对）
    #[test]
    fn end_position_follows_the_token_width() {
        let site = ErrorSite {
            offset: 7,
            len: 9,
            line: 1,
            column: 8,
            token: Some("no_such_列".to_string()),
        };
        // `no_such_列` 是 9 个字符（字节数 12）：结束列按字符算 = 8 + 9
        assert_eq!(site.end(), (1, 17));
    }
}
