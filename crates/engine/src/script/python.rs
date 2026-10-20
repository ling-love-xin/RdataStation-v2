//! Python 词法器（轻量、纯函数）
//!
//! 覆盖：关键字、常量（True/False/None）、字符串（含 r/b/u/f 前缀与三引号跨行）、
//! `#` 行注释、数字（进制/小数/指数/下划线/虚数）、函数调用启发式（标识符紧跟 `(`）、
//! 运算符与标点。标识符（变量/函数定义名/模块名）**不上色**（正文色，与 SQL 一致）。
//! 词法失败不 panic：未闭合字符串按「到行尾」收尾，其余照常上色。

use crate::sql::{HighlightSpan, TokenClass};

/// Python 关键字（Python 3，不含 True/False/None——它们是常量）
const KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
    "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
    "is", "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "try",
    "while", "with", "yield",
];

/// Python 内置常量
const CONSTANTS: &[&str] = &["True", "False", "None"];

/// 字符串前缀字母（r/b/u/f 的 1–3 个组合，如 `r` / `rb` / `fr`）
const STRING_PREFIX_CHARS: &[u8] = b"rRbBuUfF";

/// 计算 Python 的高亮区间（整篇扫描，按出现顺序、互不重叠）
pub fn python_spans(text: &str) -> Vec<HighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            // 行注释：`#` 到行尾（不含换行）
            b'#' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                spans.push(span(start, i, TokenClass::Comment));
            }
            // 引号：可能是字符串（先看是否有前缀字母并入前缀）
            b'\'' | b'"' => {
                let start = string_prefix_start(text, i);
                let end = read_string(text, i);
                spans.push(span(start, end, TokenClass::String));
                i = end;
            }
            // 数字
            b'0'..=b'9' => {
                let (start, end) = read_number(text, i);
                spans.push(span(start, end, TokenClass::Number));
                i = end;
            }
            // 标识符：关键字 / 常量 / 字符串前缀+引号 / 函数调用启发式，其余不上色
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let (start, end) = read_ident(text, i);
                let word = &text[start..end];
                if KEYWORDS.contains(&word) {
                    spans.push(span(start, end, TokenClass::Keyword));
                    i = end;
                } else if CONSTANTS.contains(&word) {
                    spans.push(span(start, end, TokenClass::Constant));
                    i = end;
                } else if is_string_prefix(word) && end < bytes.len()
                    && (bytes[end] == b'\'' || bytes[end] == b'"')
                {
                    // 字符串前缀 + 引号：前缀并入字符串整体上色
                    let str_end = read_string(text, end);
                    spans.push(span(start, str_end, TokenClass::String));
                    i = str_end;
                } else if end < bytes.len() && bytes[end] == b'(' {
                    // 函数调用启发式：标识符紧跟 `(`（无空白）
                    spans.push(span(start, end, TokenClass::Function));
                    i = end;
                } else {
                    i = end;
                }
            }
            // 标点
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b':' | b';' | b'.' => {
                spans.push(span(i, i + 1, TokenClass::Punctuation));
                i += 1;
            }
            // 运算符（多字符优先）或未知字符（非 ASCII 等）跳过
            _ => {
                if let Some(len) = operator_len(text, i) {
                    spans.push(span(i, i + len, TokenClass::Operator));
                    i += len;
                } else {
                    i += char_len(text, i);
                }
            }
        }
    }
    spans
}

fn span(start: usize, end: usize, class: TokenClass) -> HighlightSpan {
    HighlightSpan { start, end, class }
}

/// 从引号位置向前找字符串前缀起点（`f'..'` 的前缀 `f` 所在字节；无前缀则返回引号自身）
fn string_prefix_start(text: &str, quote: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = quote;
    while start > 0 && STRING_PREFIX_CHARS.contains(&bytes[start - 1]) {
        // 最多 3 个前缀字母；前一个字符是标识符字符时（如 `arr'`）不算前缀
        if quote.saturating_sub(start) >= 3 {
            break;
        }
        start -= 1;
    }
    // 确认 `start..quote` 全是前缀字母（组合合法）；否则退回引号自身
    let prefix = &text[start..quote];
    if prefix.is_empty() || is_string_prefix(prefix) {
        start
    } else {
        quote
    }
}

/// 是否是合法字符串前缀（r/b/u/f 的 1–3 个字母组合）
fn is_string_prefix(word: &str) -> bool {
    if word.is_empty() || word.len() > 3 {
        return false;
    }
    word.bytes().all(|b| STRING_PREFIX_CHARS.contains(&b))
}

/// 从引号处读字符串，返回**闭引号后一位**的偏移；未闭合按「到行尾」收尾
fn read_string(text: &str, quote: usize) -> usize {
    let bytes = text.as_bytes();
    // 三引号：连续三个同种引号（可跨行）
    let triple = bytes.len() >= quote + 3 && bytes[quote + 1] == bytes[quote]
        && bytes[quote + 2] == bytes[quote];
    let mut i = quote;
    if triple {
        i += 3;
        // 匹配三个同种引号即闭合；转义不处理（三引号里转义引号少见，如实）
        while i + 2 < bytes.len() {
            if bytes[i] == bytes[quote]
                && bytes[i + 1] == bytes[quote]
                && bytes[i + 2] == bytes[quote]
            {
                return i + 3;
            }
            i += 1;
        }
        return bytes.len();
    }
    // 单行串：遇闭引号或行尾结束；`\` 转义跳过下一字符（含行尾反斜杠续行）
    i += 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2; // 跳过转义（若 `\` 在行尾，i 可能越过换行继续——续行语义）
            }
            b'\n' => return i, // 未闭合：到行尾
            b'\'' | b'"' if bytes[i] == bytes[quote] => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// 读数字字面量，返回 (起点, 终点)。覆盖：0x/0o/0b、小数、指数、下划线、虚数 j/J
fn read_number(text: &str, start: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut i = start;
    // 进制前缀（0x / 0o / 0b）
    if i + 1 < bytes.len() && bytes[i] == b'0' {
        match bytes[i + 1] {
            b'x' | b'X' | b'o' | b'O' | b'b' | b'B' => {
                i += 2;
                while i < bytes.len() && (is_hex_digit(bytes[i]) || bytes[i] == b'_') {
                    i += 1;
                }
                return (start, i);
            }
            _ => {}
        }
    }
    // 十进制：数字与下划线；一个 `.`（后跟数字才算小数）；e/E 指数（后跟 +/- 数字）
    let mut saw_dot = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit() || b == b'_' {
            i += 1;
        } else if b == b'.' && !saw_dot && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            saw_dot = true;
            i += 1;
        } else if (b == b'e' || b == b'E') && i + 1 < bytes.len()
            && (bytes[i + 1].is_ascii_digit()
                || ((bytes[i + 1] == b'+' || bytes[i + 1] == b'-')
                    && i + 2 < bytes.len()
                    && bytes[i + 2].is_ascii_digit()))
        {
            i += 1;
            if bytes[i] == b'+' || bytes[i] == b'-' {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
        } else {
            break;
        }
    }
    // 虚数后缀 j/J
    if i < bytes.len() && (bytes[i] == b'j' || bytes[i] == b'J') {
        i += 1;
    }
    (start, i)
}

fn is_hex_digit(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)
}

/// 读标识符（字母/_ 开头，后随字母/数字/_），返回 (起点, 终点)
fn read_ident(text: &str, start: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (start, i)
}

/// 单/双字符运算符长度（最长匹配）；不是运算符返回 0
fn operator_len(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    const TWO: &[&[u8]] = &[
        b"**=", b"//=", b"<<=", b">>=", b"**", b"//", b"<<", b">>", b"<=", b">=", b"==",
        b"!=", b"+=", b"-=", b"*=", b"/=", b"%=", b"@=", b"&=", b"|=", b"^=", b"->", b":=",
    ];
    let rest = &bytes[i..];
    for op in TWO {
        if rest.starts_with(op) {
            return Some(op.len());
        }
    }
    if matches!(
        bytes[i],
        b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'<' | b'>' | b'&' | b'|' | b'^' | b'~' | b'@'
    ) {
        return Some(1);
    }
    None
}

/// 跳过当前字符（按 UTF-8 长度；仅用于非 ASCII / 未识别字节）
fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, |c| c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::{python_spans, TokenClass};

    fn classes(text: &str) -> Vec<(String, TokenClass)> {
        python_spans(text)
            .into_iter()
            .map(|s| (s.text(text).to_string(), s.class))
            .collect()
    }

    #[test]
    fn keywords_constants_and_functions() {
        let got = classes("def f():\n    return True");
        assert!(got.contains(&("def".into(), TokenClass::Keyword)));
        assert!(got.contains(&("return".into(), TokenClass::Keyword)));
        assert!(got.contains(&("True".into(), TokenClass::Constant)));
        assert!(got.contains(&("f".into(), TokenClass::Function)), "{got:?}");
    }

    #[test]
    fn strings_with_prefix_and_triple_quote() {
        let got = classes("x = f'v={v}'\n\ny = '''多行\n字符串'''");
        assert!(got.contains(&("f'v={v}'".into(), TokenClass::String)), "{got:?}");
        assert!(
            got.iter().any(|(t, c)| *c == TokenClass::String && t.contains('\n')),
            "三引号跨行应整体上色：{got:?}"
        );
    }

    #[test]
    fn line_comment_stops_at_eol() {
        let got = classes("# 注释\nx = 1");
        assert_eq!(got[0], ("# 注释".into(), TokenClass::Comment));
        assert_eq!(got[0].0, "# 注释");
    }

    #[test]
    fn numbers_cover_bases_float_and_underscores() {
        let got = classes("0x1F 1_000 3.14 1e-3 2j");
        let numbers: Vec<&str> = got
            .iter()
            .filter(|(_, c)| *c == TokenClass::Number)
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(numbers, vec!["0x1F", "1_000", "3.14", "1e-3", "2j"]);
    }

    #[test]
    fn operators_and_punctuation() {
        let got = classes("a <= b // 2\n(x, y)")
            .into_iter()
            .filter(|(_, c)| *c != TokenClass::Function)
            .collect::<Vec<_>>();
        assert!(got.contains(&("<=".into(), TokenClass::Operator)), "{got:?}");
        assert!(got.contains(&("//".into(), TokenClass::Operator)), "{got:?}");
        assert!(got.contains(&("(".into(), TokenClass::Punctuation)));
        assert!(got.contains(&(",".into(), TokenClass::Punctuation)));
    }

    #[test]
    fn identifiers_and_cjk_stay_uncoloured() {
        let got = python_spans("变量 = name");
        // 标识符（含 CJK）不上色；只有 `=` 运算符有 token
        assert!(
            got.iter().all(|s| s.class == TokenClass::Operator),
            "{got:?}"
        );
    }

    #[test]
    fn unterminated_string_does_not_panic() {
        let got = python_spans("x = 'abc\ny = 1");
        // 未闭合串按「到行尾」收尾，不 panic；后面的数字仍上色
        assert!(got.iter().any(|s| s.class == TokenClass::Number));
    }

    #[test]
    fn raw_and_bytes_prefix_join_the_string() {
        let got = classes(r"rb'\x41' br'\x41'");
        for (t, c) in &got {
            assert_eq!(*c, TokenClass::String, "前缀应并入字符串：{got:?}");
            assert!(t.starts_with("rb") || t.starts_with("br"));
        }
    }
}
