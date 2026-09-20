//! 折叠候选（B17）：哪些行块可以折起来（**纯函数**）
//!
//! ## 为什么候选要我们自己算
//!
//! 内核（`gpui-base` 0.6.1）的折叠**机制是齐全的**：`fold_map`、显示投影、侧边 chevron、
//! 点击折叠、`folding` 开关（默认 true）都在，我们的编辑器本来就落在 code-editor 模式里
//! （`EditorMode::CODE_EDITOR = true`）。但**候选只有一个来源**——highlighter
//! （`highlighter.fold_ranges(text)`，`state.rs:3604`；编辑后走 `fold_ranges_for_edit`，`:3627`），
//! 没有 highlighter 就早退。我们走语义 token、不注册 grammar（架构 D12），内核自己拿不到候选，
//! 所以由本模块算好、视图侧经公开入口 `EditorState::apply_highlighter_fold_candidates`
//! 喂进去（见 `view/fold`）。
//!
//! 顺带一个好处：折叠与高亮**共用同一份词法**（`engine::sql::highlight_spans`，sqlglot
//! tokenizer）——不引第二个词法器，也不锁方言（与 D12 的选型理由同源）。
//!
//! ## 折什么
//!
//! | 结构 | 规则 | 例 |
//! | --- | --- | --- |
//! | 括号组 | `(` 所在行 → 配对 `)` 所在行 | `SELECT (` … `) AS x` |
//! | 块关键字 | `CASE` / `BEGIN` / `IF` → 配对 `END`（`END IF` 这类复合词按一个闭合词算） | `CASE … END`、`BEGIN … END` |
//! | 多行字面量与注释 | 跨行的 `String` / `Comment` 区间 | `/*` … `*/` |
//!
//! **同一行内闭合的不产候选**：折起来没有意义（只藏 0 行），还会在侧边摆一个按不动的假 chevron。
//!
//! ## 边界（认不出就不猜）
//!
//! - **词法失败即无候选**（未闭合字符串等 → `highlight_spans` 返回空）：退化为不可折，
//!   与着色同口径（着色也是"扫不出来就不上色"）；
//! - 开括号没闭合 / 闭括号没有配对 → 不产候选：给错折叠点比不给更糟（用户按了才发现折错地方）；
//! - 字符串与注释里的括号**不参与配对**：词法层已把它们标成 `String` / `Comment`，
//!   本模块只认 `Punctuation` / `Keyword`，所以 `'('`、`-- (` 不会产生折叠点；
//! - **`LOOP` / `WHILE` 不是我们认的开词**：它们不在 sqlglot 的关键字表里（词法层给的是
//!   标识符），所以 `LOOP … END LOOP` 不折——不猜"这个词大概是个块开始"；
//! - 超过 [`MAX_BYTES`] 直接返回空（与 `view/highlight.rs` 的着色门槛同量级）：大文件不每键全扫。

use engine::sql::{TokenClass, highlight_spans};

/// 超过此字节数不算折叠候选（与着色门槛 1 MB 同量级）
pub const MAX_BYTES: usize = 1_000_000;

/// 一个可折叠的行区间（**0 基、闭区间**，保证 `start_line < end_line`）
///
/// 语义与内核的 `FoldRange` 一致：折叠后 `start_line` 与 `end_line` **都仍然可见**，
/// 藏起来的是中间那几行（`state.rs` 的 `unfold_at` 文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FoldSpan {
    pub start_line: usize,
    pub end_line: usize,
}

/// 开括号的两类：括号组 vs 块关键字（`CASE` / `BEGIN` / `IF` / `LOOP` … `END`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opener {
    Paren,
    Block,
}

/// 计算折叠候选（**纯函数**；返回按起始行升序、同起始行按结束行升序，已去重）
pub fn spans(text: &str) -> Vec<FoldSpan> {
    if text.len() > MAX_BYTES {
        return Vec::new();
    }

    let tokens = highlight_spans(text);
    if tokens.is_empty() {
        return Vec::new();
    }

    let starts = line_starts(text);
    let mut found: Vec<FoldSpan> = Vec::new();
    // 未闭合的开括号。SQL 里括号与块关键字可以互相嵌套（`(CASE … END)`），所以按"最近匹配"
    // 收：闭括号只找**同类**里最近的那个，中间夹着别的类不影响。
    let mut open: Vec<(Opener, usize)> = Vec::new();

    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let line = line_of(&starts, token.start);
        match token.class {
            TokenClass::Punctuation => match token.text(text) {
                "(" => open.push((Opener::Paren, line)),
                ")" => close(&mut open, &mut found, Opener::Paren, line),
                _ => {}
            },
            TokenClass::Keyword => {
                let upper = token.text(text).to_ascii_uppercase();
                match head_word(&upper) {
                    "CASE" | "BEGIN" | "IF" => open.push((Opener::Block, line)),
                    "END" => {
                        close(&mut open, &mut found, Opener::Block, line);
                        // `END IF` / `END CASE` / `END LOOP` 在词法上是**两个 token**：
                        // 后面那个词属于同一个闭合词，不能当开括号用（跳过它）
                        if continues_closing_word(&tokens, index, text) {
                            index += 1;
                        }
                    }
                    _ => {}
                }
            }
            // 跨行的字面量与注释本身就可以折（多行 `/* … */`、含换行的字符串）
            TokenClass::String | TokenClass::Comment => {
                let end_line = line_of(&starts, token.end.saturating_sub(1));
                push(&mut found, line, end_line);
            }
            _ => {}
        }
        index += 1;
    }

    found.sort_unstable();
    found.dedup();
    found
}

/// `END` 后面那一个词是不是同一个闭合词的一部分（`END IF` / `END CASE` / `END LOOP` …）
fn continues_closing_word(tokens: &[engine::sql::HighlightSpan], index: usize, text: &str) -> bool {
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    if next.class != TokenClass::Keyword {
        return false;
    }
    let upper = next.text(text).to_ascii_uppercase();
    matches!(head_word(&upper), "IF" | "CASE" | "LOOP" | "WHILE" | "FOR")
}

/// 收一个开括号：找**最近**的同类开括号，命中才产候选（找不到就什么都不做）
fn close(
    open: &mut Vec<(Opener, usize)>,
    found: &mut Vec<FoldSpan>,
    kind: Opener,
    end_line: usize,
) {
    let Some(index) = open.iter().rposition(|(candidate, _)| *candidate == kind) else {
        return;
    };
    let (_, start_line) = open.remove(index);
    push(found, start_line, end_line);
}

/// 只收跨行的（同行的折了等于没折）
fn push(found: &mut Vec<FoldSpan>, start_line: usize, end_line: usize) {
    if start_line < end_line {
        found.push(FoldSpan {
            start_line,
            end_line,
        });
    }
}

/// 词的第一个空格前的部分（`end if` → `end`）
fn head_word(word: &str) -> &str {
    word.split_ascii_whitespace().next().unwrap_or("")
}

/// 每行首字节偏移（下标即行号；`line_of` 用它做二分）
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::with_capacity(text.len() / 32 + 1);
    starts.push(0);
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

/// 字节偏移 → 行号（0 基）
fn line_of(starts: &[usize], offset: usize) -> usize {
    starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（见 `view/tests.rs` 顶部说明）
    use super::{FoldSpan, MAX_BYTES, spans};

    /// 断言用形状：(起始行, 结束行)
    fn lines(text: &str) -> Vec<(usize, usize)> {
        spans(text)
            .into_iter()
            .map(|span| (span.start_line, span.end_line))
            .collect()
    }

    #[test]
    fn a_parenthesis_group_over_lines_folds_from_open_to_close() {
        assert_eq!(lines("SELECT (\n  1\n) AS x"), [(0, 2)]);
    }

    #[test]
    fn groups_closed_on_one_line_do_not_fold() {
        assert_eq!(lines("SELECT (1) AS x"), []);
        assert_eq!(lines("SELECT count(*) FROM t WHERE a IN (1, 2)"), []);
    }

    #[test]
    fn nested_groups_are_sorted_and_end_where_they_close() {
        // 三处结构：外层括号 0→5、CASE 1→4、内层括号 1→3
        let sql = "SELECT (\n  CASE WHEN a THEN (\n    1\n  )\n  END\n) FROM t";
        assert_eq!(lines(sql), [(0, 5), (1, 3), (1, 4)]);
    }

    #[test]
    fn case_block_folds_without_brackets() {
        let sql = "SELECT CASE\n  WHEN a THEN 1\n  ELSE 2\nEND AS x";
        assert_eq!(lines(sql), [(0, 3)]);
    }

    #[test]
    fn plsql_blocks_fold_and_end_if_closes_the_if() {
        // BEGIN 0→4，IF 1→3：`END IF` 收 IF，最后的 `END` 收 BEGIN
        let sql = "BEGIN\n  IF a THEN\n    b := 1;\n  END IF;\nEND;";
        assert_eq!(lines(sql), [(0, 4), (1, 3)]);
    }

    #[test]
    fn loop_blocks_do_not_fold_because_our_lexer_reads_loop_as_an_identifier() {
        // `LOOP` 不在 sqlglot 的关键字表里（词法层给的是标识符）→ 我们不猜"这个词是个块开始"，
        // 于是没有开词、`END LOOP` 也就无从配对。这条**钉住的是诚实**，不是能力。
        let sql = "LOOP\n  x := x + 1;\nEND LOOP;";
        assert_eq!(lines(sql), []);
    }

    #[test]
    fn brackets_inside_strings_and_comments_do_not_pair() {
        // 字符串里的括号不是 Punctuation：既不产折叠点，也不去配对外面的括号
        let sql = "SELECT '('\n  , ')' AS x\nFROM t";
        assert_eq!(lines(sql), []);
        // 行注释里的括号同理
        let sql = "SELECT 1 -- (\nFROM t";
        assert_eq!(lines(sql), []);
    }

    #[test]
    fn a_multiline_comment_folds_over_its_own_lines() {
        let sql = "/* 说明： (\n   还是 (  */\nSELECT 1";
        assert_eq!(lines(sql), [(0, 1)]);
    }

    #[test]
    fn closing_on_the_same_line_as_the_keyword_does_not_fold() {
        assert_eq!(lines("SELECT CASE WHEN a THEN 1 END AS x"), []);
    }

    #[test]
    fn unbalanced_structures_produce_nothing() {
        // 只有开：不猜到哪里结束
        assert_eq!(lines("SELECT (\n  1"), []);
        // 只有闭：没有配对的开
        assert_eq!(lines("SELECT 1\n) AS x"), []);
        // `BEGIN` 没有 `END`
        assert_eq!(lines("BEGIN\n  x := 1;"), []);
    }

    #[test]
    fn a_failed_lex_produces_no_candidates() {
        // 未闭合字符串 → `highlight_spans` 返回空（与着色同一降级口径）
        assert_eq!(lines("SELECT 'a\n  b\nFROM t"), []);
        // 未闭合块注释同理
        assert_eq!(lines("SELECT 1 /* (\n 还是注释"), []);
    }

    #[test]
    fn line_numbers_do_not_depend_on_the_line_ending_style() {
        // CRLF（Windows 上真实存在）：行号与 LF 一致
        assert_eq!(lines("SELECT (\r\n  1\r\n) AS x\r\n"), [(0, 2)]);
    }

    #[test]
    fn oversized_text_is_not_scanned() {
        let big = "SELECT (\n1\n);\n".repeat(MAX_BYTES / 14 + 4);
        assert!(big.len() > MAX_BYTES, "测试数据本身要超过门槛");
        assert_eq!(lines(&big), []);
    }

    #[test]
    fn spans_are_unique_and_ordered() {
        // 同一行上 `(` 与 `CASE` 同时开、又同时收 → 会算出两个相同的候选，去重后只留一个
        let sql = "SELECT (CASE a\n  WHEN 1 THEN 2\nEND)";
        assert_eq!(lines(sql), [(0, 2)]);
        let sql = "SELECT (\n  1\n), (\n  2\n) FROM t";
        // 第二组括号从第 2 行的 `(`（`), (`）开到第 4 行的 `)`
        assert_eq!(lines(sql), [(0, 2), (2, 4)]);

        let found = spans(sql);
        assert!(found.iter().all(
            |FoldSpan {
                 start_line,
                 end_line,
             }| start_line < end_line
        ));
        assert!(
            found.windows(2).all(|pair| pair[0] <= pair[1]),
            "顺序必须确定"
        );
    }
}
