//! Log 词法器（轻量、纯函数）
//!
//! 覆盖（与 VS Code 的 Log 扩展同形态，但只做词法层）：
//! - **级别词分级着色**：ERROR/FATAL/CRITICAL/ERR → constant（主题警示色）、
//!   WARN/WARNING → keyword、INFO → function、DEBUG/TRACE → comment；大小写不敏感、词边界判定；
//! - ISO / 常见时间戳 → number；
//! - `[标签]` / `(…)` 括号 → punctuation（括号本身）；
//! - `key=value` / `key: value` → key → variable、分隔符 → operator、数字值 → number、
//!   引号值 → string；
//! - `{…}` 结构括号 → punctuation。
//!
//! 正文消息不上色。词法失败不 panic：未闭合引号按「到行尾」收尾。

use crate::{HighlightSpan, TokenClass};

/// 级别 → 类别（级别词匹配用，词边界：前后都不是字母/数字/下划线）
const LEVEL_CLASSES: &[(&str, TokenClass)] = &[
    ("error", TokenClass::Constant),
    ("fatal", TokenClass::Constant),
    ("critical", TokenClass::Constant),
    ("err", TokenClass::Constant),
    ("warn", TokenClass::Keyword),
    ("warning", TokenClass::Keyword),
    ("info", TokenClass::Function),
    ("debug", TokenClass::Comment),
    ("trace", TokenClass::Comment),
];

/// 计算 Log 的高亮区间（整篇扫描，按出现顺序、互不重叠）
pub fn log_spans(text: &str) -> Vec<HighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            // 时间戳（数字开头：`2026-10-15T18:00:00Z` / `18:00:00.123`）
            b'0'..=b'9' => {
                // 冒号 / 点 / 横线 / T / Z / 空格可能出现在时间戳内
                let start = i;
                while i < bytes.len() && is_timestamp_char(bytes[i]) {
                    i += 1;
                }
                if i > start && looks_like_timestamp(&text[start..i]) {
                    spans.push(span(start, i, TokenClass::Number));
                } else {
                    i = start; // 不是时间戳：退回按数字短读
                    let end = read_digits(text, i);
                    if end > i {
                        spans.push(span(i, end, TokenClass::Number));
                    }
                    i = end;
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let (start, end) = read_word(text, i);
                let word = &text[start..end];
                let lower = word.to_ascii_lowercase();
                if let Some((_, class)) = LEVEL_CLASSES.iter().find(|(k, _)| *k == lower) {
                    spans.push(span(start, end, *class));
                    i = end;
                } else if end < bytes.len() && bytes[end] == b'=' {
                    // key=value：key → variable
                    spans.push(span(start, end, TokenClass::Variable));
                    i = end;
                } else if end < bytes.len() && (bytes[end] == b':' || bytes[end] == b'=') {
                    // key: value → key variable；分隔符下一步处理
                    spans.push(span(start, end, TokenClass::Variable));
                    i = end;
                } else {
                    i = end;
                }
            }
            b'"' | b'\'' => {
                let end = read_quoted(text, i);
                spans.push(span(i, end, TokenClass::String));
                i = end;
            }
            b'[' | b']' | b'(' | b')' | b'{' | b'}' => {
                spans.push(span(i, i + 1, TokenClass::Punctuation));
                i += 1;
            }
            b'=' | b':' => {
                spans.push(span(i, i + 1, TokenClass::Operator));
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

/// 时间戳可含字符：数字 / `:` `.` `-` `T` `Z` `+` 与空格
fn is_timestamp_char(b: u8) -> bool {
    b.is_ascii_digit()
        || matches!(b, b':' | b'.' | b'-' | b'T' | b'Z' | b'+' | b' ' | b'/')
}

/// 粗判「像时间戳」：含至少一个数字与一个冒号/横线（避免把纯数字误吞）
fn looks_like_timestamp(s: &str) -> bool {
    let has_digit = s.bytes().any(|b| b.is_ascii_digit());
    let has_sep = s.bytes().any(|b| b == b':' || b == b'-' || b == b'/');
    has_digit && has_sep
}

/// 读纯数字段
fn read_digits(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i
}

/// 读单词（字母/数字/下划线；日志字段名与级别词常用）
fn read_word(text: &str, start: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (start, i)
}

/// 读引号串：到闭引号或行尾
fn read_quoted(text: &str, quote: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = quote + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' => return i,
            b'"' | b'\'' if bytes[i] == bytes[quote] => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, |c| c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::{TokenClass, log_spans};

    fn classes(text: &str) -> Vec<(String, TokenClass)> {
        log_spans(text)
            .into_iter()
            .map(|s| (s.text(text).to_string(), s.class))
            .collect()
    }

    #[test]
    fn levels_are_coloured_by_severity() {
        let got = classes("2026-10-15 18:00:00 ERROR boom\n18:00:01 WARN warn!\n18:00:02 INFO ok\ndebug trace");
        assert!(got.contains(&("ERROR".into(), TokenClass::Constant)), "{got:?}");
        assert!(got.contains(&("WARN".into(), TokenClass::Keyword)));
        assert!(got.contains(&("INFO".into(), TokenClass::Function)));
        assert!(got.contains(&("debug".into(), TokenClass::Comment)));
        assert!(got.contains(&("trace".into(), TokenClass::Comment)));
    }

    #[test]
    fn timestamps_are_numbers() {
        let got = classes("2026-10-15T18:00:00.123Z [svc] start");
        assert!(
            got.iter().any(|(t, c)| *c == TokenClass::Number && t.starts_with("2026")),
            "{got:?}"
        );
        assert!(got.contains(&("[".into(), TokenClass::Punctuation)));
        assert!(got.contains(&("]".into(), TokenClass::Punctuation)));
    }

    #[test]
    fn key_value_pairs() {
        let got = classes("level=INFO time=\"12:00\" retries=3");
        assert!(got.contains(&("level".into(), TokenClass::Variable)), "{got:?}");
        assert!(got.contains(&("time".into(), TokenClass::Variable)));
        assert!(got.contains(&("retries".into(), TokenClass::Variable)));
        assert!(got.contains(&("\"12:00\"".into(), TokenClass::String)));
        assert!(got.contains(&("3".into(), TokenClass::Number)));
    }

    #[test]
    fn level_is_not_a_substring() {
        let got = classes("information processor");
        assert!(!got.iter().any(|(t, _)| t == "processor"), "不应把 processor 当级别");
        assert!(!got.iter().any(|(t, c)| *c == TokenClass::Constant && t == "information"));
    }

    #[test]
    fn unterminated_quoted_is_safe() {
        let got = log_spans("msg=\"abc\nnext line");
        assert!(got.iter().any(|s| s.class == TokenClass::String));
    }
}
