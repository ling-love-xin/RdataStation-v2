//! JSON 词法器（轻量、纯函数）
//!
//! 覆盖：字符串（含 `\` 转义）、**对象键**（字符串后紧跟 `:` → property）、数字、
//! `true`/`false`（→ boolean）、`null`（→ constant）、结构标点（`{}` `[]` `,` `:`）。
//! 不上色：裸词（编辑中的非法 JSON）、普通文本。词法失败不 panic：未闭合字符串
//! 按「到行尾」收尾；字符串后的空白与 `:` 前瞻判定键，不消费前瞻字符。

use crate::{HighlightSpan, TokenClass};

/// 计算 JSON 的高亮区间（整篇扫描，按出现顺序、互不重叠）
pub fn json_spans(text: &str) -> Vec<HighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'"' => {
                let end = read_string(text, i);
                // 键判定：字符串后跳过空白紧跟 `:`（不消费，循环继续从闭引号后扫描）
                let class = if next_non_space(bytes, end) == Some(b':') {
                    TokenClass::Property
                } else {
                    TokenClass::String
                };
                spans.push(span(i, end, class));
                i = end;
            }
            b'-' | b'0'..=b'9' => {
                let end = read_number(text, i);
                if end > i {
                    spans.push(span(i, end, TokenClass::Number));
                }
                i = end;
            }
            b't' | b'f' | b'n' => {
                // true / false / null（词边界：后跟非标识符字符）
                let (start, end, word) = read_word(text, i);
                if end > start {
                    match word {
                        "true" | "false" => spans.push(span(start, end, TokenClass::Boolean)),
                        "null" => spans.push(span(start, end, TokenClass::Constant)),
                        _ => {}
                    }
                }
                i = end;
            }
            b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                spans.push(span(i, i + 1, TokenClass::Punctuation));
                i += 1;
            }
            _ => i += char_len(text, i),
        }
    }
    spans
}

fn span(start: usize, end: usize, class: TokenClass) -> HighlightSpan {
    HighlightSpan { start, end, class }
}

/// 读 JSON 字符串：`"` 到闭 `"`（`\` 转义跳过下一字符）；未闭合到行尾
fn read_string(text: &str, quote: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = quote + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return i,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// 从 `end` 起跳过空白后的第一个非空白字节
fn next_non_space(bytes: &[u8], mut i: usize) -> Option<u8> {
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    bytes.get(i).copied()
}

/// 读数字：`-?` 整数（0 或 [1-9]\d*）可选小数与指数；非法数字返回原地
fn read_number(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    if i < bytes.len() && bytes[i] == b'-' {
        i += 1;
    }
    // 整数部分
    if i < bytes.len() && bytes[i] == b'0' {
        i += 1;
    } else if i < bytes.len() && bytes[i].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return start; // `-` 后无数字：非法，原样返回（外层不产出）
    }
    // 小数
    if i < bytes.len() && bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    // 指数
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        if j < bytes.len() && bytes[j].is_ascii_digit() {
            i = j;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    i
}

/// 读一个单词（true/false/null 探测），返回 (起点, 终点, 词)
fn read_word(text: &str, start: usize) -> (usize, usize, &str) {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    (start, i, &text[start..i])
}

fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, |c| c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::{TokenClass, json_spans};

    fn classes(text: &str) -> Vec<(String, TokenClass)> {
        json_spans(text)
            .into_iter()
            .map(|s| (s.text(text).to_string(), s.class))
            .collect()
    }

    #[test]
    fn keys_vs_values() {
        let got = classes(r#"{"name": "RDS", "count": 3}"#);
        assert!(got.contains(&("\"name\"".into(), TokenClass::Property)), "{got:?}");
        assert!(got.contains(&("\"RDS\"".into(), TokenClass::String)), "{got:?}");
        assert!(got.contains(&("\"count\"".into(), TokenClass::Property)));
        assert!(got.contains(&("3".into(), TokenClass::Number)));
        assert!(got.contains(&(":".into(), TokenClass::Punctuation)));
    }

    #[test]
    fn bool_null_and_numbers() {
        let got = classes(r#"{"a": true, "b": null, "c": -1.5e2}"#);
        assert!(got.contains(&("true".into(), TokenClass::Boolean)), "{got:?}");
        assert!(got.contains(&("null".into(), TokenClass::Constant)));
        assert!(got.contains(&("-1.5e2".into(), TokenClass::Number)));
    }

    #[test]
    fn escaped_strings_are_single_token() {
        let got = classes(r#""a\"b""#);
        assert!(got.iter().any(|(t, c)| *c == TokenClass::String && t == r#""a\"b""#), "{got:?}");
    }

    #[test]
    fn unterminated_string_and_bare_words_are_safe() {
        let got = classes("{\"x\": \"abc\n}");
        // 不 panic；已闭合部分照常上色
        assert!(got.iter().any(|(_, c)| *c == TokenClass::Property));
    }
}
