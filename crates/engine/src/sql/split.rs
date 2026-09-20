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
//!   后果要注意：MySQL 脚本里以 `#` 开头的行会**变成一条语句文本**（内容是注释），
//!   「批量执行」会把这条发出去。
//! - `DELIMITER` 这类客户端指令不认：改分隔符是客户端状态，不是 SQL 语法。
//! - 语句内的 `BEGIN … END` 块（存储过程体）中的分号会被切开——真正的修复需要语法
//!   级信息；当前实现与主流客户端（DBeaver 的多数驱动）行为一致。
//! - `--` 后不跟空白也算注释（PostgreSQL 语义）；MySQL 会当两个减号。
//! - 分号是**唯一**的边界：两条没有分号的多行语句会被当成一条（不按行猜新语句——
//!   行启发式靠关键字开头，猜错就是少执行 / 多执行一句）。
//! - 「语句外的空白」除了 ASCII 空白，还当空白跳过的只有三种**粘贴里真实会遇上**的
//!   序列：BOM（`U+FEFF`，Windows 工具另存）、不换行空格（`U+00A0`，网页复制）、
//!   全角空格（`U+3000`，中文输入法）。其余 Unicode 空白（如 `U+2003`）不跳：那要
//!   解码 UTF-8，而这里的判据是字节级的（见 [`LAYOUT_BLANKS`]）。

/// 语句外的「排版空白」：这三个序列当空白跳过（不设段起点、不算代码、也不进语句文本）
///
/// 为什么不是「所有 Unicode 空白」：那要解码 UTF-8，而本模块的判据是**字节级**的
/// （见模块头）。这三个是粘贴 / 另存里真实会带进来的：BOM（`U+FEFF`）、不换行空格
/// （`U+00A0`）、全角空格（`U+3000`）。
///
/// 为什么 BOM 一定要管：带上它，整个语句文本就不是「一条能跑的 SQL」了——实测
/// `SqlEngine::parse_and_route("\u{feff}SELECT 1")` 掉到 `Unknown`（不是 `Select`），
/// 于是只读查询的判据（如「洞察此列」）跟着错，驱动拿到的头三个字节也是 BOM。
const LAYOUT_BLANKS: [&[u8]; 3] = [b"\xef\xbb\xbf", b"\xc2\xa0", b"\xe3\x80\x80"];

/// 从 `from` 开始的排版空白序列有多长（不是就返回 `None`）
fn layout_blank_len(bytes: &[u8], from: usize) -> Option<usize> {
    LAYOUT_BLANKS
        .iter()
        .find(|blank| bytes[from..].starts_with(blank))
        .map(|blank| blank.len())
}

/// 恰好以 `end` 结尾的排版空白序列有多长（不会跨越 `start`）
fn layout_blank_ending_at(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    LAYOUT_BLANKS
        .iter()
        .find(|blank| end >= start + blank.len() && bytes[end - blank.len()..end].eq(**blank))
        .map(|blank| blank.len())
}

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
        // 排版空白（BOM / 不换行空格 / 全角空格）：与 ASCII 空白同待遇
        if let Some(len) = layout_blank_len(bytes, i) {
            i += len;
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
        //
        // `$` 紧贴在标识符字节后面时**不算引用开头**（PostgreSQL 自己的规矩：紧跟
        // 关键字 / 标识符的 `$` 归前一个词）——`SELECT price$usd$ FROM t` 里的 `$usd$`
        // 是那个标识符的一部分；不认这条就会把后面的 `;` 一并吞掉（实测：
        // `SELECT price$usd$ FROM t; SELECT 2` 会被切出一条语句而不是两条）。
        if b == b'$' && !glued_to_ident(bytes, i) {
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

/// 收尾：裁掉尾部空白（含 [`LAYOUT_BLANKS`] 那三种排版空白），生成语句位置
fn finish(sql: &str, start: usize, mut end: usize, line: usize) -> SqlStatement {
    let bytes = sql.as_bytes();
    loop {
        while end > start && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        // 尾部也可能是排版空白（`SELECT 1<BOM>;`）：它不是语句内容的一部分
        match layout_blank_ending_at(bytes, start, end) {
            Some(len) => end -= len,
            None => break,
        }
    }
    SqlStatement { start, end, line }
}

/// 这个字节是不是「标识符里会出现」的字节（高字节也算：多字节序列的每个字节都 ≥ 0x80）
fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// `$`（位于 `at`）是不是紧贴在一个标识符字节后面（那时它是那个词的一部分）
fn glued_to_ident(bytes: &[u8], at: usize) -> bool {
    at > 0 && is_ident_byte(bytes[at - 1])
}

/// 判断 `$`（位于 `from`）是否是一个美元引用的起始标签，返回标签结束位置（含闭合 `$`）
fn dollar_tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    debug_assert_eq!(bytes[from], b'$');
    let mut j = from + 1;
    // 空标签：$$
    if bytes.get(j) == Some(&b'$') {
        return Some(j + 1);
    }
    // 具名标签：必须以字母、下划线或非 ASCII 字符开头（排除 $1 这类占位符；
    // PostgreSQL 的标签就是标识符，中文标签在那里合法）
    match bytes.get(j) {
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' || *c >= 0x80 => {}
        _ => return None,
    }
    while let Some(c) = bytes.get(j) {
        if c.is_ascii_alphanumeric() || *c == b'_' || *c >= 0x80 {
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
        assert_eq!(texts("SELECT 'it''s; fine'"), vec!["SELECT 'it''s; fine'"]);
    }

    #[test]
    fn quoted_identifiers_keep_semicolons() {
        assert_eq!(
            texts("SELECT \"a;b\" FROM t"),
            vec!["SELECT \"a;b\" FROM t"]
        );
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
        assert_eq!(texts("SELECT 1 /* ; */ + 2"), vec!["SELECT 1 /* ; */ + 2"]);
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
        assert_eq!(texts("-- 说明\nSELECT 1"), vec!["-- 说明\nSELECT 1"]);
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
        assert_eq!(
            texts("SELECT 'abc; SELECT 2"),
            vec!["SELECT 'abc; SELECT 2"]
        );
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

    // ══════════════════════════════════════════════════════════════════
    // 边界情形表（P0.4 补测）
    //
    // 为什么用表：这里要固定的是**判据**（分号在什么位置不算分隔符），不是某一个函数的
    // 行为形状；一条边界一行，加边界就是加一行。表里的期望值都是**先写下来再对实现**的
    // （对不上就查是谁错：判据错就改期望并写进「已知限制」，实现错就修实现）。
    // 参考来源：同类产品 sqlab（MIT）的 `query_detector` 测试集（那里靠 tree-sitter，
    // 本模块是词法层），取舍与模块头的约束一致。
    // ══════════════════════════════════════════════════════════════════

    /// 必须切对的边界：输入 → 期望切出的语句文本
    const BOUNDARIES: &[(&str, &[&str])] = &[
        // ── 字符串 / 引用标识符里的分号 ──────────────────────────
        ("SELECT ';'", &["SELECT ';'"]),
        (
            "SELECT 'a;b' AS s; SELECT 2",
            &["SELECT 'a;b' AS s", "SELECT 2"],
        ),
        // 双写转义：`''` 是内容，不是字面量的结束
        ("SELECT 'it''s; fine';", &["SELECT 'it''s; fine'"]),
        (
            "SELECT \"a;b\" FROM t; SELECT 2",
            &["SELECT \"a;b\" FROM t", "SELECT 2"],
        ),
        // 反引号（MySQL 标识符引用）+ 内部双写
        (
            "SELECT `a``b;c`; SELECT 2",
            &["SELECT `a``b;c`", "SELECT 2"],
        ),
        // 跨行字符串：行号要跟着字符串里的换行走（见 lines_advance_... 用例）
        (
            "SELECT 'a;\nb;';\nSELECT 2",
            &["SELECT 'a;\nb;'", "SELECT 2"],
        ),
        // 跨行函数调用：分号在字符串参数里
        (
            "SELECT concat(\n  'x;y',\n  'z;w'\n); SELECT 2",
            &["SELECT concat(\n  'x;y',\n  'z;w'\n)", "SELECT 2"],
        ),
        // ── 注释里的分号 ──────────────────────────────────
        ("SELECT 1 /* ; */ + 2", &["SELECT 1 /* ; */ + 2"]),
        // 块注释可嵌套（PostgreSQL 语义；MySQL 不嵌套，见已知限制）
        (
            "SELECT 1 /* a /* b; */ c ; */ + 2",
            &["SELECT 1 /* a /* b; */ c ; */ + 2"],
        ),
        // 行注释末尾没有换行：仍然是「延续到文末」
        (
            "SELECT 1 -- 到文末没有换行",
            &["SELECT 1 -- 到文末没有换行"],
        ),
        // 分号在行注释里：后面的 `SELECT 2` 被注释掉了，不能算第二条语句
        ("SELECT 1; -- 注释里的分号 ; SELECT 2", &["SELECT 1"]),
        ("/* 只有注释 ; */", &[]),
        ("-- 只有注释\n-- 还是注释\n", &[]),
        ("/* a */ ; /* b */", &[]),
        // ── 美元引用 `$$ … $$` / `$tag$ … $tag$` ────────────────
        (
            "DO $$ BEGIN RAISE NOTICE 'x'; END $$; SELECT 1;",
            &["DO $$ BEGIN RAISE NOTICE 'x'; END $$", "SELECT 1"],
        ),
        (
            "CREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; $body$ LANGUAGE sql;",
            &["CREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; $body$ LANGUAGE sql"],
        ),
        // 体里的引号不参与配对（美元引用里没有转义，`$` 之间原样）
        ("SELECT $$'$$; SELECT 2", &["SELECT $$'$$", "SELECT 2"]),
        // 非 ASCII 标签：PostgreSQL 允许（标签就是标识符）
        (
            "SELECT $标签$ 一;二 $标签$; SELECT 2",
            &["SELECT $标签$ 一;二 $标签$", "SELECT 2"],
        ),
        // 紧跟标识符字节的 `$` 是标识符的一部分，不是引用开头（PG 的 lexer 规矩）
        (
            "SELECT price$usd$ FROM t; SELECT 2",
            &["SELECT price$usd$ FROM t", "SELECT 2"],
        ),
        ("SELECT a$b$c FROM t", &["SELECT a$b$c FROM t"]),
        // `$1` 占位符：不算标签
        (
            "SELECT $1, $2 FROM t; SELECT 2",
            &["SELECT $1, $2 FROM t", "SELECT 2"],
        ),
        // 未闭合：延续到文末（不 panic、不丢内容）
        ("SELECT $$abc; def", &["SELECT $$abc; def"]),
        // ── 空语句 / 前后空白 ──────────────────────────────
        ("", &[]),
        ("   \n\t  ", &[]),
        (";;;", &[]),
        ("  \n  SELECT 1  \n ;  \n", &["SELECT 1"]),
        ("SELECT 1;;SELECT 2", &["SELECT 1", "SELECT 2"]),
        ("SELECT 1;\n\n;SELECT 2", &["SELECT 1", "SELECT 2"]),
        // ── BOM / 排版空白 / CRLF ─────────────────────────
        // BOM（Windows 工具另存常见）在语句外，不是 SQL 的一部分
        ("\u{feff}SELECT 1;\nSELECT 2;", &["SELECT 1", "SELECT 2"]),
        ("\u{feff}", &[]),
        ("\u{feff}   \n", &[]),
        ("SELECT 1;\u{feff}SELECT 2", &["SELECT 1", "SELECT 2"]),
        // 字符串里的 BOM 是内容（不跳）
        (
            "SELECT '\u{feff}' AS x; SELECT 2",
            &["SELECT '\u{feff}' AS x", "SELECT 2"],
        ),
        // 不换行空格（网页复制）/ 全角空格（中文输入法）：同空白待遇
        ("SELECT 1;\u{a0}SELECT 2", &["SELECT 1", "SELECT 2"]),
        ("SELECT 1;\u{3000}SELECT 2", &["SELECT 1", "SELECT 2"]),
        ("SELECT 1\u{3000}; SELECT 2", &["SELECT 1", "SELECT 2"]),
        // CRLF：`\r` 不算换行，行号只跟 `\n`
        ("SELECT 1;\r\nSELECT 2;\r\n", &["SELECT 1", "SELECT 2"]),
        ("\r\n", &[]),
        // ── Unicode ──────────────────────────────────────
        // 全角分号（U+FF1B）不是分隔符
        (
            "SELECT '；' AS x; SELECT 2",
            &["SELECT '；' AS x", "SELECT 2"],
        ),
        (
            "SELECT 名 FROM 表 WHERE 名 = '值'",
            &["SELECT 名 FROM 表 WHERE 名 = '值'"],
        ),
        (
            "SELECT \"列;名\" FROM 表; SELECT 2",
            &["SELECT \"列;名\" FROM 表", "SELECT 2"],
        ),
        // ── CASE … END / 事务块 ────────────────────────────
        // CASE 的 `END` 与切分无关（我们不跟踪关键字配对，也不该在这条上出错）
        (
            "SELECT CASE a WHEN 1 THEN 'x;' ELSE 'y' END AS r; SELECT 2",
            &[
                "SELECT CASE a WHEN 1 THEN 'x;' ELSE 'y' END AS r",
                "SELECT 2",
            ],
        ),
        ("BEGIN; SELECT 1; COMMIT;", &["BEGIN", "SELECT 1", "COMMIT"]),
    ];

    #[test]
    fn boundary_table_holds() {
        for &(input, expected) in BOUNDARIES {
            assert_eq!(&texts(input)[..], expected, "输入：{input:?}");
            // 区间必须能原样切片（落在字符边界上，且不含首尾空白）
            for span in split_statements(input) {
                let _ = &input[span.start..span.end];
            }
        }
    }

    /// 已知限制：这些**今天不认**，行为被钉住
    ///
    /// 钉住不等于认可：每一条都是明写的取舍（模块头的「已知限制」一节），改它们要先改那份
    /// 取舍（多半要先有语法级信息或方言参数，两者都不是本模块能自己定的）。
    const KNOWN_LIMITATIONS: &[(&str, &[&str])] = &[
        // 没有美元引用的 `BEGIN … END` 块：块内分号照切（真正的修复要语法级信息）。
        // 后果：批量执行会把 `END` 当一条独立语句发出去。
        (
            "CREATE PROCEDURE p() BEGIN SELECT 1; END;",
            &["CREATE PROCEDURE p() BEGIN SELECT 1", "END"],
        ),
        (
            "CREATE TRIGGER t AFTER INSERT ON x BEGIN UPDATE a SET n = 1; END;",
            &[
                "CREATE TRIGGER t AFTER INSERT ON x BEGIN UPDATE a SET n = 1",
                "END",
            ],
        ),
        // MySQL 的 `#` 注释：不认（认了会把 PG 的 `#>` / `#>>` 操作符切开）。
        // 后果：`#` 行会被当成一条语句文本（内容是注释）。
        ("# 注释 ;\nSELECT 1;", &["# 注释", "SELECT 1"]),
        (
            "SELECT 1 # 注释 ; SELECT 2",
            &["SELECT 1 # 注释", "SELECT 2"],
        ),
        // 客户端指令 `DELIMITER`：不认（它不是 SQL，改分隔符是客户端状态）
        (
            "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END//\nDELIMITER ;\nSELECT 1;",
            &[
                "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1",
                "END//\nDELIMITER",
                "SELECT 1",
            ],
        ),
        // `--` 后不跟空白也算注释（PostgreSQL 语义）；MySQL 会当成两个减号
        ("SELECT 1 --注释; SELECT 2", &["SELECT 1 --注释; SELECT 2"]),
        // 反斜杠不是转义（标准串语义；MySQL 默认态会差一）
        ("SELECT 'a\\'; SELECT 2", &["SELECT 'a\\'", "SELECT 2"]),
        // 没有分号就没有边界：两行 SQL 合成一条（**不**按行猜新语句）
        (
            "select current_user\nselect current_date",
            &["select current_user\nselect current_date"],
        ),
        // 只跳三种真实会遇到的排版空白（BOM / NBSP / 全角空格）；其余 Unicode 空白
        // （如 U+2003 EM SPACE）不跳：那要解码 UTF-8，而这里的判据是字节级的
        (
            "SELECT 1;\u{2003}SELECT 2",
            &["SELECT 1", "\u{2003}SELECT 2"],
        ),
    ];

    #[test]
    fn known_limitations_are_pinned() {
        for &(input, expected) in KNOWN_LIMITATIONS {
            assert_eq!(
                &texts(input)[..],
                expected,
                "已知限制被改动了？输入：{input:?}"
            );
        }
    }

    /// BOM 不只是“多三个字节”：带着它去分类会掉到 `Unknown`（实测），
    /// 于是只读查询的判据（如「洞察此列」）与驱动拿到的 SQL 都会跟着错。
    #[test]
    fn bom_is_not_part_of_the_statement() {
        let sql = "\u{feff}SELECT 1;\nSELECT 2;";
        let spans = split_statements(sql);
        assert_eq!(spans[0].text(sql), "SELECT 1");
        assert_eq!(&sql[spans[0].start..spans[0].end], "SELECT 1");
        assert_eq!(
            crate::SqlEngine::parse_and_route(spans[0].text(sql), crate::SqlDialect::Ansi).0,
            crate::SqlStatementType::Select,
            "去掉 BOM 之后这条语句才分得出「是 SELECT」"
        );
        // 只有 BOM 的“文件”没有语句（否则「执行全部」会把 BOM 发给驱动）
        assert!(split_statements("\u{feff}").is_empty());
    }

    /// 行号要跨过「被跳过的区间」：多行字符串 / 美元引用体 / 多行注释
    #[test]
    fn lines_advance_across_skipped_regions() {
        assert_eq!(lines("SELECT 'a\nb';\nSELECT 2"), vec![1, 3]);
        assert_eq!(lines("SELECT $$\n\n$$;\nSELECT 2"), vec![1, 4]);
        assert_eq!(lines("SELECT 1 /* a\nb */;\nSELECT 2"), vec![1, 3]);
        assert_eq!(lines("\u{feff}SELECT 1;\nSELECT 2"), vec![1, 2]);
        assert_eq!(lines("SELECT 1;\r\nSELECT 2"), vec![1, 2]);
    }

    /// 区间契约：`start` 跳过前导空白与 BOM，`end` 不含尾部空白与分隔分号
    #[test]
    fn spans_exclude_leading_and_trailing_blanks() {
        // 行布局：BOM+空白（1） / `SELECT 1`（2） / `; SELECT 2`（3）
        let sql = "\u{feff}  \n SELECT 1 \n ; SELECT 2";
        let spans = split_statements(sql);
        assert_eq!(spans.len(), 2);
        assert_eq!(&sql[spans[0].start..spans[0].end], "SELECT 1");
        assert_eq!(&sql[spans[1].start..spans[1].end], "SELECT 2");
        assert_eq!(spans[0].line, 2);
        assert_eq!(spans[1].line, 3, "行号跟着 `\\n` 走，BOM 不算换行");
    }
}
