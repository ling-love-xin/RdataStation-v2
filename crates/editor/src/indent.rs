//! 【2026-10-27】缩进操作纯函数（VSCode 缩进面板的落地能力）
//!
//! 供状态栏「缩进」对话框调用的三类操作：**从内容检测缩进方式**（推断 制表符/空格宽度）、
//! **将缩进转换为空格/制表符**（重写每行前导缩进）、**裁剪尾随空格**。全是纯函数、可穷举
//! 测试；文本写回由宿主走 `service.set_content` + 内核重载（转换/裁剪会标脏，符合 VSCode
//! 行为；检测只改 `tab_size`/`hard_tabs`，不置脏）。

/// 从内容检测缩进方式 → `(hard_tabs, tab_size)`
///
/// 规则（VSCode 同款启发式）：找第一个有前导空白的非空行——以制表符开头 → 制表符缩进；
/// 否则返回该行的前导空格宽度（取首次出现的非零宽度）。无缩进 → 默认空格 2。
pub fn detect_indent(text: &str) -> (bool, usize) {
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let lead_len = line.len() - line.trim_start_matches([' ', '\t']).len();
        if lead_len == 0 {
            continue;
        }
        if line.as_bytes()[0] == b'\t' {
            return (true, 2);
        }
        // 空格缩进：取该行前导空格数作为宽度
        return (false, lead_len);
    }
    (false, 2)
}

/// 把每行前导缩进重写为目标形式
///
/// - `to_hard_tabs = true`：宽度按 `tab_size` 整除成制表符，余数补空格；
/// - `to_hard_tabs = false`：全部转成空格（宽度个）。
/// 非缩进正文保持不变；行尾换行符保留。
pub fn convert_indent(text: &str, to_hard_tabs: bool, tab_size: usize) -> String {
    let tab_size = tab_size.max(1);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let has_nl = line.ends_with('\n');
        let body = if has_nl { &line[..line.len() - 1] } else { line };
        // 前导空白（空格 + 制表符；\r 属于行尾，不参与缩进）
        let indent_end = body
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(body.len());
        let (indent, rest) = body.split_at(indent_end);
        let width = indent
            .chars()
            .map(|c| if c == '\t' { tab_size } else { 1 })
            .sum::<usize>();
        let new_indent = if to_hard_tabs {
            let tabs = width / tab_size;
            let spaces = width % tab_size;
            format!("{}{}", "\t".repeat(tabs), " ".repeat(spaces))
        } else {
            " ".repeat(width)
        };
        out.push_str(&new_indent);
        out.push_str(rest);
        if has_nl {
            out.push('\n');
        }
    }
    out
}

/// 去掉每行行尾的空白（空格 / 制表符），保留换行符（含 `\r\n` 与末尾空行的换行）
pub fn trim_trailing_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let has_nl = line.ends_with('\n');
        // 先剥掉行尾 `\n`，再把 CRLF 的 `\r` 单独拎出（`\r` 不是空白，会挡住 trim）
        let content = if has_nl { &line[..line.len() - 1] } else { line };
        let (trim_part, tail) = match content.strip_suffix('\r') {
            Some(stripped) => (stripped, "\r"),
            None => (content, ""),
        };
        out.push_str(trim_part.trim_end_matches(|c: char| c == ' ' || c == '\t'));
        out.push_str(tail);
        if has_nl {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_tab_indent() {
        assert_eq!(detect_indent("a\n\tselect 1\n"), (true, 2));
        assert_eq!(detect_indent("\t\tfoo\n"), (true, 2));
    }

    #[test]
    fn detect_space_indent_uses_first_indented_line_width() {
        assert_eq!(detect_indent("no indent\n    select\n"), (false, 4));
        assert_eq!(detect_indent("  x\n        y\n"), (false, 2));
    }

    #[test]
    fn detect_defaults_to_spaces_two_when_no_indent() {
        assert_eq!(detect_indent("select 1;\n"), (false, 2));
        assert_eq!(detect_indent(""), (false, 2));
    }

    #[test]
    fn convert_tabs_to_spaces_expands_each_tab_to_tab_size() {
        let out = convert_indent("\tselect 1;\n\t\tfrom t;\n", false, 4);
        assert_eq!(out, "    select 1;\n        from t;\n");
    }

    #[test]
    fn convert_spaces_to_tabs_uses_div_and_remainder() {
        // 5 空格，tab_size=2 → 2 tab + 1 空格
        let out = convert_indent("     x\n", true, 2);
        assert_eq!(out, "\t\t x\n");
        // 4 空格 → 2 tab（整除无余数）
        let out = convert_indent("    y\n", true, 2);
        assert_eq!(out, "\t\ty\n");
    }

    #[test]
    fn convert_preserves_non_indent_text_and_newlines() {
        let out = convert_indent("select 1;\n", true, 4);
        assert_eq!(out, "select 1;\n");
        // 最后一行无换行也保留
        assert_eq!(convert_indent("  z", false, 4), "  z");
    }

    #[test]
    fn trim_removes_trailing_spaces_and_tabs_keeps_newlines() {
        assert_eq!(trim_trailing_whitespace("a  \t\nb\t\nc\n"), "a\nb\nc\n");
        assert_eq!(trim_trailing_whitespace("x   "), "x");
        // 行内、非行尾的空格保留
        assert_eq!(trim_trailing_whitespace("a  b  \n"), "a  b\n");
        // \r\n 保留 \r
        assert_eq!(trim_trailing_whitespace("a  \r\n"), "a\r\n");
    }
}
