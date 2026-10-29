//! Rust 词法器（轻量、纯函数）
//!
//! 覆盖：关键字（含 true/false→布尔）、字符串（普通 / raw `r#"…"#` / 字节串 b / 字节 raw br）、
//! char 字面量与**生命周期**的区分（`'a` / `'static` / `'_` → type）、`//` 行注释与
//! **可嵌套** `/* */` 块注释、数字（进制 / 下划线 / 指数 / 类型后缀）、宏（`println!` →
//! function）、函数调用启发式、`#[…]` 属性（整体 tag）、运算符与标点。
//! 不做上下文语义：泛型类型实参 `<T>` 里的 T、普通标识符不上色。

use crate::{HighlightSpan, TokenClass};

/// Rust 关键字（2018 版起常用集；true/false 是布尔字面量，走 `Boolean`）
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "type",
    "unsafe", "use", "where", "while", "try",
];

/// 数字类型后缀（并入数字整体上色）
const NUMBER_SUFFIXES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
    "f32", "f64",
];

/// 计算 Rust 的高亮区间（整篇扫描，按出现顺序、互不重叠）
pub fn rust_spans(text: &str) -> Vec<HighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            // 行注释
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                spans.push(span(start, i, TokenClass::Comment));
            }
            // 块注释（可嵌套）
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                let start = i;
                let mut depth = 0usize;
                while i < bytes.len() {
                    if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                spans.push(span(start, i, TokenClass::Comment));
            }
            // 字符串 / char / 生命周期
            b'"' => {
                let end = read_string(text, i);
                spans.push(span(i, end, TokenClass::String));
                i = end;
            }
            b'\'' => {
                let (end, class) = read_char_or_lifetime(text, i);
                spans.push(span(i, end, class));
                i = end;
            }
            b'0'..=b'9' => {
                let end = read_number(text, i);
                spans.push(span(i, end, TokenClass::Number));
                i = end;
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let (start, end) = read_ident(text, i);
                let word = &text[start..end];
                if word == "true" || word == "false" {
                    spans.push(span(start, end, TokenClass::Boolean));
                    i = end;
                } else if KEYWORDS.contains(&word) {
                    spans.push(span(start, end, TokenClass::Keyword));
                    i = end;
                } else if is_byte_or_raw_prefix(word) && end < bytes.len()
                    && (bytes[end] == b'"' || bytes[end] == b'#' || bytes[end] == b'\'')
                {
                    // 字节串 b"…" / raw r#"…"# / br#"…"# / 字节 char b'…'
                    let (str_end, class) = if bytes[end] == b'\'' {
                        read_char_or_lifetime(text, end)
                    } else {
                        (read_string(text, end), TokenClass::String)
                    };
                    spans.push(span(start, str_end, class));
                    i = str_end;
                } else if end < bytes.len() && bytes[end] == b'!' {
                    // 宏调用：`println!` / `vec!` → function；`!` 本身 operator
                    spans.push(span(start, end, TokenClass::Function));
                    spans.push(span(end, end + 1, TokenClass::Operator));
                    i = end + 1;
                } else if end < bytes.len() && bytes[end] == b'(' {
                    spans.push(span(start, end, TokenClass::Function));
                    i = end;
                } else {
                    i = end;
                }
            }
            // 属性整体：`#[…]` / `#![…]` → tag（内部不再细分）
            b'#'
                if i + 1 < bytes.len()
                    && (bytes[i + 1] == b'['
                        || (bytes[i + 1] == b'!' && i + 2 < bytes.len() && bytes[i + 2] == b'[')) =>
            {
                let start = i;
                let close = find_attribute_end(text, i);
                spans.push(span(start, close, TokenClass::Tag));
                i = close;
            }
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b':' => {
                spans.push(span(i, i + 1, TokenClass::Punctuation));
                i += 1;
            }
            // `.`：`..` / `..=` 是运算符（范围/排他范围），单个 `.` 是路径标点
            b'.' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'.' {
                    let len = if i + 2 < bytes.len() && bytes[i + 2] == b'=' { 3 } else { 2 };
                    spans.push(span(i, i + len, TokenClass::Operator));
                    i += len;
                } else {
                    spans.push(span(i, i + 1, TokenClass::Punctuation));
                    i += 1;
                }
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

/// 普通 / raw / 字节字符串；`quote` 指向 `"` 或 raw 前缀 `#`，返回闭引号后偏移
///
/// 兼容两种入口：外层 `b'"'` 分支（`"` 前无 `#`）与标识符前缀分支（`r#"…"#` 的 `end`
/// 指向第一个 `#`）。先数 `#`（raw 的 hash 数），再定位 `"`，闭合需要 `"` + 相同数量 `#`。
fn read_string(text: &str, quote: usize) -> usize {
    let bytes = text.as_bytes();
    // 数 raw 前缀 `#`：`#` 序列之后必须跟 `"`
    let mut hashes = 0usize;
    let mut i = quote;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'"' {
        return bytes.len(); // 前缀后无引号：异常输入，保守到末尾
    }
    i += 1;
    while i < bytes.len() {
        if hashes > 0 {
            // raw 闭合：`"` 后跟同样数量的 `#`
            if bytes[i] == b'"'
                && bytes[i + 1..].iter().take(hashes).all(|&b| b == b'#')
            {
                return i + 1 + hashes;
            }
            i += 1;
        } else {
            match bytes[i] {
                b'\\' => i += 2,
                b'\n' => return i, // 普通串未闭合：到行尾
                _ => i += 1,
            }
        }
    }
    bytes.len()
}

/// char 字面量 vs 生命周期：`'` 指向撇号，返回 (闭符号后偏移, 类别)
///
/// 规则：`'` 后是 `\` → 转义 char；是 `'` → 空 char；是字母/`_` → 读标识符，
/// 标识符后紧跟 `'` → char（`'a'`），否则 → 生命周期（`'a` / `'static` / `'_`）。
fn read_char_or_lifetime(text: &str, quote: usize) -> (usize, TokenClass) {
    let bytes = text.as_bytes();
    if quote + 1 >= bytes.len() {
        return (bytes.len(), TokenClass::Type); // 孤撇号，保守当生命周期
    }
    match bytes[quote + 1] {
        b'\\' => {
            // 转义 char：`'\n'` / `'\u{1F600}'`——读到闭 `'`
            let mut i = quote + 2;
            while i < bytes.len() && bytes[i] != b'\'' && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'\'' {
                (i + 1, TokenClass::String)
            } else {
                (i, TokenClass::String)
            }
        }
        b'\'' => (quote + 2, TokenClass::String), // 空 char `''`（非法但词法上按 char）
        b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
            let ident_end = {
                let mut i = quote + 2;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                i
            };
            if ident_end < bytes.len() && bytes[ident_end] == b'\'' {
                // `'a'`：char 字面量（含闭合引号）
                (ident_end + 1, TokenClass::String)
            } else {
                // 生命周期：`'a` / `'static` / `'_`
                (ident_end, TokenClass::Type)
            }
        }
        _ => {
            // 单字符 char：`'x'`（含闭合引号）；无闭合 → 到行尾
            let mut i = quote + 2;
            if i < bytes.len() && bytes[i] == b'\'' {
                (i + 1, TokenClass::String)
            } else {
                while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\'' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'\'' {
                    (i + 1, TokenClass::String)
                } else {
                    (i, TokenClass::String)
                }
            }
        }
    }
}

/// 读数字：进制前缀 / 小数 / 指数 / 下划线 / 类型后缀（并入整体）
fn read_number(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    if i + 1 < bytes.len() && bytes[i] == b'0' {
        match bytes[i + 1] {
            b'x' | b'X' => {
                i += 2;
                while i < bytes.len() && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
                    i += 1;
                }
                return number_with_suffix(text, i);
            }
            b'o' | b'O' => {
                i += 2;
                while i < bytes.len() && ((b'0'..=b'7').contains(&bytes[i]) || bytes[i] == b'_') {
                    i += 1;
                }
                return number_with_suffix(text, i);
            }
            b'b' | b'B' => {
                i += 2;
                while i < bytes.len() && (bytes[i] == b'0' || bytes[i] == b'1' || bytes[i] == b'_') {
                    i += 1;
                }
                return number_with_suffix(text, i);
            }
            _ => {}
        }
    }
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
    number_with_suffix(text, i)
}

/// 数字后紧跟的类型后缀（`1u32` / `1.5f64`）并入数字；避免误吃后续标识符
fn number_with_suffix(text: &str, i: usize) -> usize {
    let bytes = text.as_bytes();
    if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        let end = {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            j
        };
        let word = &text[i..end];
        if NUMBER_SUFFIXES.contains(&word) {
            return end;
        }
    }
    i
}

/// 是否是字节串 / raw 串前缀（r / b / br / rb 之一）
fn is_byte_or_raw_prefix(word: &str) -> bool {
    matches!(word, "r" | "b" | "br" | "rb")
}

/// 找到属性 `#[…]` / `#![…]` 的闭 `]` 后偏移（属性内不做词法，整段 tag）
fn find_attribute_end(text: &str, hash: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = hash + 1;
    if i < bytes.len() && bytes[i] == b'!' {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'[' {
        i += 1;
    }
    // 属性内一般无嵌套 `[`；保守只找第一个 `]`（`derive(..)` / `cfg(..)` 均不嵌套方括号）
    while i < bytes.len() && bytes[i] != b']' {
        i += 1;
    }
    if i < bytes.len() {
        i + 1
    } else {
        bytes.len()
    }
}

/// 读标识符（字母/`_` 开头，后随字母/数字/`_`）
fn read_ident(text: &str, start: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (start, i)
}

/// 运算符长度（多字符优先）
fn operator_len(text: &str, i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    const TWO: &[&[u8]] = &[
        b"<<=", b">>=", b"..=", b"->", b"=>", b"==", b"!=", b"<=", b">=", b"&&", b"||",
        b"<<", b">>", b"+=", b"-=", b"*=", b"/=", b"%=", b"&=", b"|=", b"^=", b"..",
    ];
    let rest = &bytes[i..];
    for op in TWO {
        if rest.starts_with(op) {
            return Some(op.len());
        }
    }
    if matches!(
        bytes[i],
        b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'<' | b'>' | b'&' | b'|' | b'^' | b'!' | b'?'
    ) {
        return Some(1);
    }
    None
}

fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, |c| c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::{TokenClass, rust_spans};

    fn classes(text: &str) -> Vec<(String, TokenClass)> {
        rust_spans(text)
            .into_iter()
            .map(|s| (s.text(text).to_string(), s.class))
            .collect()
    }

    #[test]
    fn keywords_and_literals() {
        let got = classes("let x = true; fn main() { }");
        assert!(got.contains(&("let".into(), TokenClass::Keyword)));
        assert!(got.contains(&("fn".into(), TokenClass::Keyword)));
        assert!(got.contains(&("true".into(), TokenClass::Boolean)));
        assert!(got.contains(&("main".into(), TokenClass::Function)), "{got:?}");
    }

    #[test]
    fn nested_block_comment() {
        let got = classes("/* 外 /* 内 */ 外 */\nlet x = 1;");
        assert_eq!(got[0], ("/* 外 /* 内 */ 外 */".into(), TokenClass::Comment));
        assert!(got[0].0.contains("内"));
    }

    #[test]
    fn raw_string_and_byte_string() {
        // 外层用 r## 避免与内层 r#"…"# 的闭合符冲突
        let got = classes(r##"let a = r#"raw "x"#;"##);
        assert!(
            got.iter().any(|(t, c)| *c == TokenClass::String && t.contains("raw \"x\"")),
            "raw 串应整体上色：{got:?}"
        );
        let got = classes(r##"let b = br#"bytes"#;"##);
        assert!(got.iter().any(|(t, c)| *c == TokenClass::String && t == "br#\"bytes\"#"));
    }

    #[test]
    fn char_vs_lifetime() {
        let got = classes("let c = 'a'; let r: &'static str = &'a x;");
        assert!(got.contains(&("'a'".into(), TokenClass::String)), "{got:?}");
        assert!(got.contains(&("'static".into(), TokenClass::Type)), "{got:?}");
        assert!(got.contains(&("'a".into(), TokenClass::Type)), "{got:?}");
    }

    #[test]
    fn numbers_with_prefix_and_suffix() {
        let got = classes("0x1F 0b101 1_000 1.5f64 2e10usize");
        let nums: Vec<&str> = got
            .iter()
            .filter(|(_, c)| *c == TokenClass::Number)
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(nums, vec!["0x1F", "0b101", "1_000", "1.5f64", "2e10usize"]);
    }

    #[test]
    fn macros_and_attributes() {
        let got = classes("#[derive(Clone)]\nprintln!(\"hi\");");
        assert!(got.contains(&("#[derive(Clone)]".into(), TokenClass::Tag)), "{got:?}");
        assert!(got.contains(&("println".into(), TokenClass::Function)), "{got:?}");
        assert!(got.contains(&("!".into(), TokenClass::Operator)));
    }

    #[test]
    fn operators_and_arrows() {
        let got = classes("let x = a => b; c..d");
        assert!(got.contains(&("=>".into(), TokenClass::Operator)));
        assert!(got.contains(&("..".into(), TokenClass::Operator)), "{got:?}");
    }

    #[test]
    fn unterminated_string_is_safe() {
        let got = rust_spans("let s = \"abc\nlet n = 42;");
        assert!(got.iter().any(|s| s.class == TokenClass::Number));
    }
}
