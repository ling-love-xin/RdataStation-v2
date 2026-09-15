//! 编辑器文本操作（纯函数，可在单测里穷举）
//!
//! 这里放"对文本本身做什么"的计算，不碰 GPUI、不碰 `EditorService`、不碰 I/O。
//! 视图层只负责把结果写回内核（`set_value` + `set_selected_range`）并 notify。
//!
//! 为什么把计算单独拿出来：注释开关（`Ctrl+/`）是编辑器里最容易出错的两件小事之一
//! （另一件是缩进），错误表现又是"肉眼看着差不多、实际少了个字符"。做成纯函数之后，
//! 加/去注释、空行、缩进、复选、行尾无换行这些情形都能逐条断言，不必开窗口。

use std::ops::Range;

use crate::model::EditorMode;

/// 行注释前缀（按模式取）
///
/// 1a 只有 SQL 一种语言：文本模式同样给 SQL 注释符（用户拿它写 `.txt` 时也多半是 SQL 片段）。
/// 1b 起按方言取（`engine::sql` 有方言表），分析模式按**单元语言**取（1c）。
pub fn comment_prefix(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Text | EditorMode::Sql | EditorMode::Analysis => "--",
    }
}

/// `toggle_line_comment` 的结果
///
/// 拆分出 `replaced` / `block` 是给内核用的：`EditorState::replace` 替换的是**当前选区**，
/// 所以调用方的顺序是「选中 `replaced` → `replace(block)` → 选中 `selection`」。
/// 走 `replace` 而不是 `set_value`，是因为后者会**清掉撤销栈**（注释必须可撤销）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentEdit {
    /// 全文（调用方同步服务层 / 测试断言）
    pub text: String,
    /// 原文中被替换的块（按字节）
    pub replaced: Range<usize>,
    /// 替换后的块文本
    pub block: String,
    /// 替换后的块在全文中的范围（落回选区的值：**整块**，连续按 `Ctrl+/` 行为稳定）
    pub selection: Range<usize>,
    /// 操作后是否处于注释态（true = 加了注释）
    pub commented: bool,
}

/// 行注释开关（`Ctrl+/`）
///
/// 规则（与 VSCode / DataGrip 一致）：
/// - 选区覆盖的行构成"块"；**选区结束恰在行首时不吞下一行**（拖选到行尾的常见结果）；
/// - 块内**空行不参与**：给空行插 `--` 只是噪点（SQL 脚本里尤其明显）；
/// - 所有非空行都已注释 → 去注释（删前缀与紧随的**一个**空格）；否则 → 加注释（保持缩进）；
/// - 前缀判定按 `trim_start` 之后的文本，故 `---- 分隔线` 会被当作已注释——这与主流编辑器
///   的判定一致，也是"再按一次能去掉一层"的来源。
pub fn toggle_line_comment(text: &str, selection: Range<usize>, prefix: &str) -> CommentEdit {
    let start = floor_char_boundary(text, selection.start);
    let end = floor_char_boundary(text, selection.end).max(start);

    // 结束位置：非空选区且结束落在行首 → 回退一格，避免把下一行算进块
    let end_offset = if end > start && end == line_start(text, end) {
        end - 1
    } else {
        end
    };

    let block_start = line_start(text, start);
    let block_end = line_end(text, end_offset);
    let block = &text[block_start..block_end];

    let lines: Vec<&str> = block.split('\n').collect();
    let state = comment_state(&lines, prefix);
    let strip = matches!(state, Some(true));
    let mut out = String::with_capacity(block.len() + lines.len() * (prefix.len() + 1));
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        write_line(&mut out, line, prefix, strip);
    }

    let mut result = String::with_capacity(text.len() + out.len());
    result.push_str(&text[..block_start]);
    result.push_str(&out);
    result.push_str(&text[block_end..]);
    let selection_end = block_start + out.len();

    CommentEdit {
        text: result,
        replaced: block_start..block_end,
        block: out,
        selection: block_start..selection_end,
        // 无内容可注释时（空文档 / 全是空行）结果仍是“未注释”
        commented: state.is_some() && !strip,
    }
}

/// 块内是否存在非空行，以及它们是否都已带前缀（`None` = 全是空行）
fn comment_state(lines: &[&str], prefix: &str) -> Option<bool> {
    let mut saw_content = false;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        saw_content = true;
        if !line.trim_start().starts_with(prefix) {
            return Some(false);
        }
    }
    saw_content.then_some(true)
}

/// 写出一行（加注释 / 去注释 / 原样）
fn write_line(out: &mut String, line: &str, prefix: &str, commented: bool) {
    if line.trim().is_empty() {
        out.push_str(line);
        return;
    }
    let indent = line.len() - line.trim_start().len();
    out.push_str(&line[..indent]);
    if commented {
        let rest = &line[indent + prefix.len()..];
        out.push_str(rest.strip_prefix(' ').unwrap_or(rest));
    } else {
        out.push_str(prefix);
        out.push(' ');
        out.push_str(&line[indent..]);
    }
}

/// 所在行的行首（`offset` 必须是字符边界）
fn line_start(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}

/// 所在行的行尾（不含换行符）
fn line_end(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index)
}

/// 向前找到最近的字符边界（`str::floor_char_boundary` 还未稳定）
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（父模块引入了 gpui 系依赖，`use super::*` 会混淆 `test` 宏）
    use super::{CommentEdit, comment_prefix, toggle_line_comment};
    use crate::model::EditorMode;
    use std::ops::Range;

    const SQL: &str = "--";

    fn at(text: &str, selection: Range<usize>) -> CommentEdit {
        toggle_line_comment(text, selection, SQL)
    }

    /// 选中整篇（模拟 `Ctrl+A`）
    fn all(text: &str) -> Range<usize> {
        0..text.len()
    }

    #[test]
    fn comments_every_selected_line_and_keeps_indent() {
        let text = "select 1\n  where x = 2";
        let edit = at(text, all(text));
        assert_eq!(edit.text, "-- select 1\n  -- where x = 2");
        assert!(edit.commented);
        // 选区覆盖整块，连续按第二次仍作用在同一批行上
        assert_eq!(edit.selection, 0..edit.text.len());
    }

    #[test]
    fn second_press_removes_the_prefix_and_the_single_space() {
        let text = "-- select 1\n  -- where x = 2";
        let edit = at(text, all(text));
        assert_eq!(edit.text, "select 1\n  where x = 2");
        assert!(!edit.commented);
    }

    #[test]
    fn removing_is_not_fooled_by_a_double_dash_inside_text() {
        // 只有行首（缩进之后）的 `--` 才算注释；行内的不动
        let text = "-- select a - -1 as v";
        let edit = at(text, all(text));
        assert_eq!(edit.text, "select a - -1 as v");
    }

    #[test]
    fn mixed_block_gets_commented_wholesale() {
        // 一行已注释、一行没有 → 整体按"加注释"处理（不半加半去）
        let text = "-- select 1\nselect 2";
        let edit = at(text, all(text));
        assert_eq!(edit.text, "-- -- select 1\n-- select 2");
        assert!(edit.commented);
    }

    #[test]
    fn blank_lines_are_left_alone() {
        let text = "select 1\n\nselect 2";
        let edit = at(text, all(text));
        assert_eq!(edit.text, "-- select 1\n\n-- select 2");
    }

    #[test]
    fn only_the_lines_touched_by_the_selection_change() {
        let text = "select 1\nselect 2\nselect 3";
        // 光标停在第二行中间 → 只有第二行
        let caret = 12;
        let edit = at(text, caret..caret);
        assert_eq!(edit.text, "select 1\n-- select 2\nselect 3");
    }

    #[test]
    fn selection_ending_at_line_start_does_not_take_the_next_line() {
        let text = "select 1\nselect 2";
        // 选区结束正好落在第二行行首 = 选到第一行末尾
        let edit = at(text, 0..9);
        assert_eq!(edit.text, "-- select 1\nselect 2");
    }

    #[test]
    fn last_line_without_trailing_newline_is_handled() {
        let text = "select 1\nselect 2";
        let caret = text.len();
        let edit = at(text, caret..caret);
        assert_eq!(edit.text, "select 1\n-- select 2");
    }

    #[test]
    fn empty_document_is_a_no_op() {
        let edit = at("", 0..0);
        assert_eq!(edit.text, "");
        assert!(!edit.commented);
    }

    #[test]
    fn document_of_blank_lines_is_a_no_op() {
        let text = "\n\n";
        let edit = at(text, all(text));
        assert_eq!(edit.text, text);
        assert!(!edit.commented, "没有非空行可注释，就不算处于注释态");
    }

    #[test]
    fn utf8_selection_is_clamped_to_char_boundaries() {
        // 选区落在多字节字符中间时不能 panic，也不能切坏字符
        let text = "-- 中文注释\nselect 1";
        let edit = at(text, 0..text.len());
        assert_eq!(edit.text, "-- -- 中文注释\n-- select 1");
        assert!(edit.commented);
        assert!(edit.text.is_char_boundary(edit.selection.end));

        // 选区起点落在“中”字中间（'中' 占 3 字节）→ 向前收敛到字符边界，不 panic、不切坏字符
        let edit = at(text, 2..5);
        assert_eq!(edit.text, "中文注释\nselect 1");
        assert!(!edit.commented);
    }

    #[test]
    fn crlf_lines_keep_their_carriage_return_out_of_the_prefix() {
        // 行尾的 `\r` 属于上一行，必须留在行尾（不能被当成内容前缀吃掉）
        let text = "select 1\r\nselect 2\r\n";
        let edit = at(text, all(text));
        assert_eq!(edit.text, "-- select 1\r\n-- select 2\r\n");
    }

    #[test]
    fn block_replaces_exactly_the_recorded_range() {
        // 不变量：`text` == 把 `replaced` 换成 `block` 的结果（视图层就是按这个顺序写回内核的）
        let cases: [(String, Range<usize>); 4] = [
            ("select 1\nselect 2".to_string(), all("select 1\nselect 2")),
            ("-- select 1\nselect 2\n".to_string(), all("-- select 1\nselect 2\n")),
            ("a\n\nb".to_string(), 0..1),
            ("-- a\n-- b".to_string(), all("-- a\n-- b")),
        ];
        for (text, selection) in cases {
            let edit = at(&text, selection);
            let mut expected = text.clone();
            expected.replace_range(edit.replaced.clone(), &edit.block);
            assert_eq!(edit.text, expected, "块与全文不一致：{text:?}");
            assert_eq!(edit.selection, edit.replaced.start..edit.replaced.start + edit.block.len());
        }
    }

    #[test]
    fn prefix_follows_the_mode() {
        assert_eq!(comment_prefix(EditorMode::Text), "--");
        assert_eq!(comment_prefix(EditorMode::Sql), "--");
    }
}
