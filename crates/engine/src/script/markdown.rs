//! Markdown 词法器（轻量、纯函数，行结构感知）
//!
//! 覆盖：ATX 标题（`#` 到行尾 → title）、围栏代码块标记（```` ``` ```` / `~~~`，块内**不上色**）、
//! 行内代码（反引号 → string）、链接 URL（`[text](url)` → string）、引用 / 列表行首标记（tag）。
//! 明确不上色：**强调**（`*…*` / `**…**` 是字体样式，语义 token 只表达颜色，如实跳过）、
//! 围栏内代码（留待后续按语言递归）、正文。
//! 词法失败不 panic：未闭合行内代码按「到行尾」收尾。

use crate::sql::{HighlightSpan, TokenClass};

/// 计算 Markdown 的高亮区间（按行扫描，围栏代码块内整段跳过）
pub fn markdown_spans(text: &str) -> Vec<HighlightSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut line_start = 0usize;
    // 围栏状态：Some((起行首字节, 围栏字符, 围栏长度))；None = 不在块内
    let mut fence: Option<(usize, u8, usize)> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line_start = i + 1;
            i += 1;
            continue;
        }
        // 行首处理
        if i == line_start {
            if let Some((_, fc, len)) = fence {
                // 块内：检查是否围栏闭合行
                if let Some((start, fchar, flen)) = fence_line(text, i) {
                    if fchar == fc && flen >= len {
                        spans.push(span(start, start + flen, TokenClass::Tag));
                        i = start + flen;
                        fence = None;
                        continue;
                    }
                }
                // 块内行不上色：跳到行尾
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            // 围栏起始
            if let Some((start, fchar, flen)) = fence_line(text, i) {
                spans.push(span(start, start + flen, TokenClass::Tag));
                i = start + flen;
                fence = Some((start, fchar, flen));
                continue;
            }
            // ATX 标题：`#{1,6}` + 空白 → 整行 title
            if let Some(end) = atx_title_end(text, i) {
                spans.push(span(i, end, TokenClass::Title));
                i = end;
                continue;
            }
            // 引用标记：行首连续 `>` → tag
            if bytes[i] == b'>' {
                let start = i;
                while i < bytes.len() && bytes[i] == b'>' {
                    i += 1;
                }
                spans.push(span(start, i, TokenClass::Tag));
                continue;
            }
            // 列表标记：`- ` / `* ` / `+ ` / `1. ` / `1)` → tag（标记本身）
            if let Some(end) = list_marker_end(text, i) {
                spans.push(span(i, end, TokenClass::Tag));
                i = end;
                continue;
            }
        }
        // 行内（非行首 / 行首已处理）：行内代码与链接
        match bytes[i] {
            b'`' => {
                let end = inline_code_end(text, i);
                spans.push(span(i, end, TokenClass::String));
                i = end;
            }
            b'[' => {
                // 链接 `[text](url)`：url 部分上色
                if let Some(url_end) = link_url_end(text, i) {
                    let (open, close) = (i, url_end);
                    // 找 `]` 后紧跟 `(`，url 在括号内
                    if let Some(url_start) = link_url_range(text, open, close) {
                        spans.push(span(url_start, close - 1, TokenClass::String));
                        i = close;
                        continue;
                    }
                }
                i += char_len(text, i);
            }
            _ => i += char_len(text, i),
        }
    }
    spans
}

fn span(start: usize, end: usize, class: TokenClass) -> HighlightSpan {
    HighlightSpan { start, end, class }
}

/// 行首围栏：3+ 个反引号或波浪线，返回 (起始, 字符, 长度)；不是围栏返回 None
fn fence_line(text: &str, start: usize) -> Option<(usize, u8, usize)> {
    let bytes = text.as_bytes();
    let ch = bytes[start];
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut len = 0usize;
    while start + len < bytes.len() && bytes[start + len] == ch {
        len += 1;
    }
    if len >= 3 {
        Some((start, ch, len))
    } else {
        None
    }
}

/// ATX 标题：行首 1–6 个 `#` 后跟空白或行尾 → 返回标题行尾（不含换行）
fn atx_title_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i] == b'#' {
        i += 1;
    }
    let hashes = i - start;
    if !(1..=6).contains(&hashes) {
        return None;
    }
    // `#` 后必须是空白或行尾，否则（如 `#tag`）不是标题
    if i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'\n' {
        return None;
    }
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    Some(i)
}

/// 列表标记：`- ` / `* ` / `+ ` / `1. ` / `1)`（含任务列表 `- [ ] `），返回标记后偏移
fn list_marker_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let b = bytes[start];
    let ordered = b.is_ascii_digit();
    let marker_end = if ordered {
        let mut i = start + 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
            i + 1
        } else {
            return None;
        }
    } else if b == b'-' || b == b'*' || b == b'+' {
        start + 1
    } else {
        return None;
    };
    // 标记后必须是空白（`-abc` 不是列表）
    if marker_end < bytes.len() && bytes[marker_end] != b' ' && bytes[marker_end] != b'\t' {
        return None;
    }
    Some(marker_end)
}

/// 行内代码：反引号到下一个反引号；未闭合到行尾
fn inline_code_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && bytes[i] != b'`' && bytes[i] != b'\n' {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'`' {
        i + 1
    } else {
        i
    }
}

/// 链接探测：`[` 后找 `]`，`]` 后紧跟 `(` → 返回 `)` 后偏移；否则返回 None
fn link_url_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    // 找 `]`（忽略 `[` 内的嵌套 `]`？链接文本内一般无 `]`，保守找第一个）
    let mut i = open + 1;
    while i < bytes.len() && bytes[i] != b']' && bytes[i] != b'\n' {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b']' {
        return None;
    }
    let close = i;
    if close + 1 < bytes.len() && bytes[close + 1] == b'(' {
        let mut j = close + 2;
        while j < bytes.len() && bytes[j] != b')' && bytes[j] != b'\n' {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b')' {
            return Some(j + 1);
        }
    }
    None
}

/// 从 `[` 到 `)` 的范围内找到 url 起始（`](` 后一位）
fn link_url_range(text: &str, open: usize, close: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = open + 1;
    while i < close && bytes[i] != b']' {
        i += 1;
    }
    if i < close && bytes[i] == b']' && i + 1 < close && bytes[i + 1] == b'(' {
        Some(i + 2)
    } else {
        None
    }
}

fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, |c| c.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::{TokenClass, markdown_spans};

    fn classes(text: &str) -> Vec<(String, TokenClass)> {
        markdown_spans(text)
            .into_iter()
            .map(|s| (s.text(text).to_string(), s.class))
            .collect()
    }

    #[test]
    fn atx_heading_is_title_to_eol() {
        let got = classes("# 标题\n正文");
        assert_eq!(got[0], ("# 标题".into(), TokenClass::Title));
    }

    #[test]
    fn fence_marks_tag_and_body_uncoloured() {
        let got = classes("```rust\nlet x = 1;\n```\n正文");
        assert_eq!(
            got,
            vec![
                ("```".into(), TokenClass::Tag),
                ("```".into(), TokenClass::Tag),
            ],
            "围栏块内不应产出 token：{got:?}"
        );
    }

    #[test]
    fn inline_code_and_link_url() {
        let got = classes("用 `code` 与 [说明](https://example.com) 测试");
        assert!(got.contains(&("`code`".into(), TokenClass::String)), "{got:?}");
        assert!(
            got.contains(&("https://example.com".into(), TokenClass::String)),
            "{got:?}"
        );
    }

    #[test]
    fn quote_and_list_markers_are_tags() {
        let got = classes("> 引用\n- 项目\n1. 步骤");
        assert!(got.contains(&(">".into(), TokenClass::Tag)), "{got:?}");
        assert!(got.contains(&("-".into(), TokenClass::Tag)), "{got:?}");
        assert!(got.contains(&("1.".into(), TokenClass::Tag)), "{got:?}");
    }

    #[test]
    fn emphasis_stays_uncoloured() {
        let got = classes("*强调* 与 **加粗** 不上色");
        assert!(got.is_empty(), "强调应如实不上色：{got:?}");
    }
}
