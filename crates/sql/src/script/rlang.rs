//! R 语言词法器（轻量、纯函数）
//!
//! 覆盖：关键字、常量（TRUE/FALSE/NULL/NA/NaN/Inf）、字符串（单/双引号、转义）、
//! `#` 行注释、数字、`%…%` 运算符与赋值符（`<-` / `->` / `<<-` / `->>`）、
//! 函数调用启发式（标识符紧跟 `(`）。普通标识符不上色（正文色）。
//! 注意 `#` 只在字符串**外**算注释——扫描按字符推进、字符串态优先。

use crate::{HighlightSpan, TokenClass};

/// R 关键字
const KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "repeat", "break", "next", "function", "return", "in",
    "switch", "try", "tryCatch", "finally", "stop", "warning", "message", "require",
    "library", "source", "attach", "detach", "install", "packages",
];

/// R 内置常量（语义常量，映射主题 `constant`）
const CONSTANTS: &[&str] = &["TRUE", "FALSE", "NULL", "NA", "NaN", "Inf"];

/// 计算 R 的高亮区间（整篇扫描，按出现顺序、互不重叠）
pub fn r_spans(text: &str) -> Vec<HighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            // 行注释（字符串态已在 read_string 内跳过，到这里的 `#` 一定是注释）
            b'#' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                spans.push(span(start, i, TokenClass::Comment));
            }
            b'\'' | b'"' => {
                let end = read_string(text, i);
                spans.push(span(i, end, TokenClass::String));
                i = end;
            }
            b'0'..=b'9' => {
                let end = read_number(text, i);
                spans.push(span(i, end, TokenClass::Number));
                i = end;
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'.' => {
                // R 标识符可以 `.` 开头（`.name`），但数字后的 `.` 是小数点，不进来
                let (start, end) = read_ident(text, i);
                let word = &text[start..end];
                if CONSTANTS.contains(&word) {
                    spans.push(span(start, end, TokenClass::Constant));
                    i = end;
                } else if KEYWORDS.contains(&word) {
                    spans.push(span(start, end, TokenClass::Keyword));
                    i = end;
                } else if end < bytes.len() && bytes[end] == b'(' {
                    spans.push(span(start, end, TokenClass::Function));
                    i = end;
                } else {
                    i = end;
                }
            }
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' => {
                spans.push(span(i, i + 1, TokenClass::Punctuation));
                i += 1;
            }
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

/// 读字符串（单/双引号、`\` 转义；未闭合到行尾，不 panic）
fn read_string(text: &str, quote: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = quote + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return i, // 未闭合：到行尾
            b'\'' | b'"' if bytes[i] == bytes[quote] => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// 读数字（小数 / 指数 / 下划线可选；R 无进制字面量前缀）
fn read_number(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    let mut saw_dot = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit() || b == b'_' {
            i += 1;
        } else if b == b'.' && !saw_dot && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            saw_dot = true;
            i += 1;
        } else if (b == b'e' || b == b'E')
            && i + 1 < bytes.len()
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
    i
}

/// 读标识符：字母/`.`/`_` 开头（`.` 后必须跟字母/数字，避免 `.` 当标点）
fn read_ident(text: &str, start: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
    {
        i += 1;
    }
    (start, i)
}

/// 运算符长度（多字符优先）：赋值 / 比较 / `%…%`；不是运算符返回 0
fn operator_len(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    const TWO: &[&[u8]] = &[
        b"<<-", b"->>", b"<-", b"->", b"==", b"!=", b"<=", b">=", b"&&", b"||", b"**",
    ];
    let rest = &bytes[i..];
    for op in TWO {
        if rest.starts_with(op) {
            return Some(op.len());
        }
    }
    if bytes[i] == b'%' {
        // `%in%` / `%*%` 等：`%` 到下一个 `%`（含两端），未闭合则只取当前 `%`
        let mut end = i + 1;
        while end < bytes.len() && bytes[end] != b'%' {
            end += 1;
        }
        if end < bytes.len() {
            return Some(end + 1 - i);
        }
        return Some(1);
    }
    if matches!(
        bytes[i],
        b'+' | b'-' | b'*' | b'/' | b'^' | b'=' | b'<' | b'>' | b'&' | b'|' | b'!' | b'~' | b':'
    ) {
        return Some(1);
    }
    None
}

/// 跳过当前字符（按 UTF-8 长度）
fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, |c| c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::{TokenClass, r_spans};

    fn classes(text: &str) -> Vec<(String, TokenClass)> {
        r_spans(text)
            .into_iter()
            .map(|s| (s.text(text).to_string(), s.class))
            .collect()
    }

    #[test]
    fn keywords_constants_functions() {
        let got = classes("f <- function(x) mean(x) if (x > 1) TRUE else FALSE");
        assert!(got.contains(&("function".into(), TokenClass::Keyword)));
        assert!(got.contains(&("if".into(), TokenClass::Keyword)));
        assert!(got.contains(&("TRUE".into(), TokenClass::Constant)));
        assert!(got.contains(&("FALSE".into(), TokenClass::Constant)));
        // 函数启发式：标识符紧跟 `(`（`mean(x)`），`f <- …` 的 f 不上色
        assert!(got.contains(&("mean".into(), TokenClass::Function)), "{got:?}");
        assert!(!got.iter().any(|(t, c)| t == "f" && *c == TokenClass::Function), "{got:?}");
        assert!(got.contains(&("<-".into(), TokenClass::Operator)));
    }

    #[test]
    fn hash_is_comment_outside_strings_only() {
        let got = classes("# 注释\nx <- \"a # b\"");
        assert_eq!(got[0], ("# 注释".into(), TokenClass::Comment));
        assert!(
            got.iter().any(|(t, c)| *c == TokenClass::String && t == "\"a # b\""),
            "字符串内的 # 不应算注释：{got:?}"
        );
    }

    #[test]
    fn percent_operators_and_assignments() {
        let got = classes("x %in% y\nz <<- 1\na -> b");
        assert!(got.contains(&("%in%".into(), TokenClass::Operator)), "{got:?}");
        assert!(got.contains(&("<<-".into(), TokenClass::Operator)));
        assert!(got.contains(&("->".into(), TokenClass::Operator)));
    }

    #[test]
    fn numbers_and_punctuation() {
        let got = classes("mean(1.5, 2e3, 0.25)");
        assert!(got.contains(&("1.5".into(), TokenClass::Number)), "{got:?}");
        assert!(got.contains(&("2e3".into(), TokenClass::Number)));
        assert!(got.contains(&("0.25".into(), TokenClass::Number)));
        assert!(got.contains(&(",".into(), TokenClass::Punctuation)));
    }

    #[test]
    fn unterminated_string_is_safe() {
        let got = r_spans("x <- \"abc\ny <- 2");
        assert!(got.iter().any(|s| s.class == TokenClass::Number));
    }
}
