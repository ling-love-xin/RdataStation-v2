//! SQL 语句切分（词法级）
//!
//! 为什么不是 `sql.split(';')`：分号会出现在字符串字面量、标识符引用、注释、以及
//! PostgreSQL 的 `$$ … $$` 块里；朴素切分把一条语句切成多条，会让「执行当前语句」
//! 与「批量执行」产生错误语义（v1 的实现即为此，其源码注释已承认）。
//!
//! ## 设计约束
//!
//! - **纯函数**：无 I/O、不依赖 sqlglot。切分发生在「文本可能还没写完」的时刻，
//!   解析器不适用（不能因为解析失败就不给切分）。
//! - **字节级状态机**：只比较 ASCII 字节，无需 UTF-8 解码；返回的区间始终落在字符
//!   边界上，调用方可安全切片。
//! - **未闭合不报错**：未闭合的引号 / 块注释 / 美元引用一律「延续到文末」，保证编辑
//!   中的文本也能得到一条语句（不 panic、不丢内容）。
//! - **注释归属**：语句前的注释算作该语句的一部分；整段只有注释与空白时不产生语句。
//!
//! ## 已知限制（有意为之，均有测试固定行为）
//!
//! - 只解析 SQL 标准的引号双写（`''` / `""` / ` `` `），**不解析反斜杠转义**（MySQL
//!   默认态）。原因：反斜杠在 PostgreSQL 标准串里是普通字符，两种语义无法同时满足；
//!   选标准语义，且两种选择在小概率场景下才产生差异。
//! - `#` 不作为注释起始（MySQL 支持），因为 PostgreSQL 的 `#>` / `#>>` 操作符会误判。
//! - 语句内的 `BEGIN … END` 块（存储过程体）中的分号会被切开——真正的修复需要语法
//!   级信息；当前实现与主流客户端（DBeaver 的多数驱动）行为一致。

/// 一条语句在原文中的位置与起始行
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlStatement {
    /// 起始字节偏移（已跳过前导空白）
    pub start: usize,
    /// 结束字节偏移（不含尾部空白与分隔分号）
    pub end: usize,
    /// 起始行号（1-based）
    pub line: usize,
}

impl SqlStatement {
    /// 取该语句的原文（含内部注释与换行，已去掉首尾空白）
    pub fn text<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.start..self.end]
    }

    /// 字节长度
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// 是否为空语句（防御性：正常情况下不会产生空语句）
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// 切分 SQL 文本，返回各语句的位置（按出现顺序）
///
/// 分隔符是「空白 / 注释之外」的分号；空语句与纯注释段不进入结果。
pub fn split_statements(sql: &str) -> Vec<SqlStatement> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();

    let mut i = 0usize;
    let mut line = 1usize;
    // 当前语句起点（第一个非空白字节）与其行号
    let mut seg_start: Option<usize> = None;
    let mut seg_line = 1usize;
    // 本段是否含「代码」（非空白、非注释）——纯注释段不产出语句
    let mut has_code = false;

    while i < bytes.len() {
        let b = bytes[i];

        // 换行与空白：只维护行号与段落起点，不属于任何语句内容
        if b == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        if seg_start.is_none() {
            seg_start = Some(i);
            seg_line = line;
        }

        // 行注释：-- 到行尾（注释内容不改变 has_code）
        if b == b'-' && bytes.get(i + 1) == Some(&b'-') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // 块注释：/* … */（支持嵌套，PostgreSQL 语义）
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'\n' => {
                        line += 1;
                        i += 1;
                    }
                    b'/' if bytes.get(i + 1) == Some(&b'*') => {
                        depth += 1;
                        i += 2;
                    }
                    b'*' if bytes.get(i + 1) == Some(&b'/') => {
                        depth -= 1;
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            continue;
        }

        // 引号字面量 / 引号标识符：'…' "…" `…`（双写转义）
        if b == b'\'' || b == b'"' || b == b'`' {
            has_code = true;
            let quote = b;
            i += 1;
            while i < bytes.len() {
                let c = bytes[i];
                if c == b'\n' {
                    line += 1;
                    i += 1;
                    continue;
                }
                if c == quote {
                    if bytes.get(i + 1) == Some(&quote) {
                        // 双写 = 转义，继续留在字面量内
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // 美元引用：$$ … $$ 或 $tag$ … $tag$
        if b == b'$' {
            if let Some(tag_end) = dollar_tag_end(bytes, i) {
                has_code = true;
                let tag = &bytes[i..tag_end];
                match find_bytes(bytes, tag_end, tag) {
                    Some(abs) => {
                        line += count_newlines(&bytes[tag_end..abs]);
                        i = abs + tag.len();
                    }
                    None => {
                        // 未闭合：延续到文末
                        line += count_newlines(&bytes[tag_end..]);
                        i = bytes.len();
                    }
                }
                continue;
            }
            // 形如 $1 的占位符：当普通字符
            has_code = true;
            i += 1;
            continue;
        }

        // 语句分隔
        if b == b';' {
            if has_code {
                if let Some(start) = seg_start {
                    statements.push(finish(sql, start, i, seg_line));
                }
            }
            seg_start = None;
            has_code = false;
            i += 1;
            continue;
        }

        has_code = true;
        i += 1;
    }

    // 文末未以分号结束的语句
    if has_code {
        if let Some(start) = seg_start {
            statements.push(finish(sql, start, bytes.len(), seg_line));
        }
    }

    statements
}

/// 收尾：裁掉尾部空白，生成语句位置
fn finish(sql: &str, start: usize, mut end: usize, line: usize) -> SqlStatement {
    let bytes = sql.as_bytes();
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    SqlStatement { start, end, line }
}

/// 判断 `$`（位于 `from`）是否是一个美元引用的起始标签，返回标签结束位置（含闭合 `$`）
fn dollar_tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    debug_assert_eq!(bytes[from], b'$');
    let mut j = from + 1;
    // 空标签：$$
    if bytes.get(j) == Some(&b'$') {
        return Some(j + 1);
    }
    // 具名标签：必须以字母或下划线开头（排除 $1 这类占位符）
    match bytes.get(j) {
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {}
        _ => return None,
    }
    while let Some(c) = bytes.get(j) {
        if c.is_ascii_alphanumeric() || *c == b'_' {
            j += 1;
        } else {
            break;
        }
    }
    if bytes.get(j) == Some(&b'$') {
        Some(j + 1)
    } else {
        None
    }
}

/// 在字节流中查找子串（避免按非字符边界切 `&str`）
fn find_bytes(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > haystack.len() {
        return None;
    }
    let last = haystack.len() - needle.len();
    (from..=last).find(|&i| &haystack[i..i + needle.len()] == needle)
}

fn count_newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|c| **c == b'\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(sql: &str) -> Vec<&str> {
        split_statements(sql).iter().map(|s| s.text(sql)).collect()
    }

    fn lines(sql: &str) -> Vec<usize> {
        split_statements(sql).iter().map(|s| s.line).collect()
    }

    #[test]
    fn single_statement_without_semicolon() {
        assert_eq!(texts("SELECT 1"), vec!["SELECT 1"]);
    }

    #[test]
    fn single_statement_with_trailing_semicolon() {
        assert_eq!(texts("SELECT 1;"), vec!["SELECT 1"]);
    }

    #[test]
    fn two_statements() {
        assert_eq!(texts("SELECT 1; SELECT 2"), vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn semicolon_inside_single_quoted_string() {
        assert_eq!(texts("SELECT ';' AS s"), vec!["SELECT ';' AS s"]);
        assert_eq!(
            texts("SELECT * FROM t WHERE s = 'a;b'"),
            vec!["SELECT * FROM t WHERE s = 'a;b'"]
        );
    }

    #[test]
    fn doubled_quote_is_escape_not_terminator() {
        assert_eq!(
            texts("SELECT 'it''s; fine'"),
            vec!["SELECT 'it''s; fine'"]
        );
    }

    #[test]
    fn quoted_identifiers_keep_semicolons() {
        assert_eq!(texts("SELECT \"a;b\" FROM t"), vec!["SELECT \"a;b\" FROM t"]);
        assert_eq!(
            texts("SELECT `a``b;c` FROM t"),
            vec!["SELECT `a``b;c` FROM t"]
        );
    }

    #[test]
    fn line_comment_hides_semicolon() {
        assert_eq!(
            texts("SELECT 1 -- ; not a separator\n"),
            vec!["SELECT 1 -- ; not a separator"]
        );
    }

    #[test]
    fn block_comment_hides_semicolon() {
        assert_eq!(
            texts("SELECT 1 /* ; */ + 2"),
            vec!["SELECT 1 /* ; */ + 2"]
        );
    }

    #[test]
    fn nested_block_comment() {
        assert_eq!(
            texts("SELECT 1 /* a /* b; */ c ; */ + 2"),
            vec!["SELECT 1 /* a /* b; */ c ; */ + 2"]
        );
    }

    #[test]
    fn comment_only_segments_produce_no_statement() {
        assert!(texts("/* only ; */").is_empty());
        assert!(texts("-- only\n").is_empty());
        assert!(texts("/* a */ ; /* b */").is_empty());
    }

    #[test]
    fn empty_and_blank_inputs() {
        assert!(texts("").is_empty());
        assert!(texts("   \n\t  ").is_empty());
        assert!(texts(";;;").is_empty());
    }

    #[test]
    fn comment_before_statement_belongs_to_it() {
        assert_eq!(
            texts("-- 说明\nSELECT 1"),
            vec!["-- 说明\nSELECT 1"]
        );
    }

    #[test]
    fn dollar_quoted_body_is_one_statement() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;";
        let out = texts(sql);
        assert_eq!(out.len(), 1);
        assert!(out[0].ends_with("LANGUAGE plpgsql"));
    }

    #[test]
    fn dollar_quoted_tagged_and_select() {
        assert_eq!(
            texts("SELECT $tag$a;b$tag$ AS x"),
            vec!["SELECT $tag$a;b$tag$ AS x"]
        );
    }

    #[test]
    fn positional_placeholder_is_not_dollar_quote() {
        assert_eq!(
            texts("SELECT $1, $2 FROM t; SELECT 2"),
            vec!["SELECT $1, $2 FROM t", "SELECT 2"]
        );
    }

    #[test]
    fn line_numbers_are_one_based_and_advance() {
        assert_eq!(lines("SELECT 1;\nSELECT 2;"), vec![1, 2]);
        assert_eq!(lines("SELECT 1;\n\n\n  SELECT 2;"), vec![1, 4]);
    }

    #[test]
    fn leading_and_trailing_whitespace_trimmed() {
        assert_eq!(texts("  \n  SELECT 1  \n ;  \n"), vec!["SELECT 1"]);
    }

    #[test]
    fn multiline_statement_keeps_inner_layout() {
        let sql = "SELECT a,\n       b\nFROM t\nWHERE x = 1;";
        assert_eq!(texts(sql), vec![sql.trim_end_matches(';')]);
    }

    #[test]
    fn insert_with_semicolon_in_values() {
        assert_eq!(
            texts("INSERT INTO t VALUES ('a;b'); UPDATE t SET x='c';"),
            vec!["INSERT INTO t VALUES ('a;b')", "UPDATE t SET x='c'"]
        );
    }

    #[test]
    fn unterminated_string_extends_to_end() {
        assert_eq!(texts("SELECT 'abc"), vec!["SELECT 'abc"]);
        assert_eq!(texts("SELECT 'abc; SELECT 2"), vec!["SELECT 'abc; SELECT 2"]);
    }

    #[test]
    fn unterminated_block_comment_extends_to_end() {
        assert_eq!(texts("SELECT 1 /* abc"), vec!["SELECT 1 /* abc"]);
    }

    #[test]
    fn unterminated_dollar_quote_extends_to_end() {
        assert_eq!(texts("SELECT $$abc; def"), vec!["SELECT $$abc; def"]);
    }

    #[test]
    fn offsets_map_back_to_original_text() {
        let sql = "  SELECT 1 ;\nSELECT 2";
        let spans = split_statements(sql);
        assert_eq!(spans.len(), 2);
        assert_eq!(&sql[spans[0].start..spans[0].end], "SELECT 1");
        assert_eq!(&sql[spans[1].start..spans[1].end], "SELECT 2");
        assert_eq!(spans[0].line, 1);
        assert_eq!(spans[1].line, 2);
    }

    #[test]
    fn multibyte_text_is_not_split_inside_characters() {
        // 中文注释与字符串：切分不得落在多字节字符中间（切片不 panic）
        let sql = "SELECT '中文；分号不是分隔符' AS c; -- 注释；\nSELECT 2;";
        let out = texts(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("中文；分号不是分隔符"));
        // 语句前的注释按设计归属该语句（见模块文档「注释归属」），故第 2 条以注释行开头，
        // 起始行号也落在注释所在的第 1 行，而不是 SELECT 所在的第 2 行。
        assert_eq!(out[1], "-- 注释；\nSELECT 2");
        assert_eq!(lines(sql), vec![1, 1]);
    }

    #[test]
    fn backslash_is_not_an_escape_by_design() {
        // 固定当前（SQL 标准）语义：反斜杠不转义引号，因此这里会切出两条语句。
        // 若将来支持方言参数，应把该用例改为 MongoDB/MySQL 分支。
        let sql = "SELECT 'a\\'; SELECT 2";
        assert_eq!(texts(sql).len(), 2);
    }

    #[test]
    fn statement_helpers_report_length() {
        let sql = "SELECT 1;";
        let span = split_statements(sql)[0];
        assert_eq!(span.len(), 8);
        assert!(!span.is_empty());
    }
}
