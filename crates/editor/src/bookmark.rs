//! 书签（编辑器接入 `rds-file-meta` 的核心能力）
//!
//! 纯逻辑：切换 / 跳转（下一、上一，循环）/ 排序去重。书签数据本身复用
//! [`rds_file_meta::Bookmark`]（与 sidecar 同一数据源，单一真值），本模块只做
//! **编辑动作**与**跳转定位**的纯函数，**不碰 I/O**（I/O 在 `view/host` 的事件路径）。
//!
//! 行号语义与 `rds-file-meta` 一致：**从 1 开始**。编辑器内核的行号是 0 起，
//! 换算在 `view/host` 的接入处做（本模块的入参/返回值都是 1 起）。

use file_meta::Bookmark;
use gpui_kit::base::input::LineDelta;

/// 切换某行书签：有则删除、无则追加（默认说明 = 行号文本）
///
/// 返回是否**新增**（`true` = 新增，`false` = 删除）。
pub fn toggle(bookmarks: &mut Vec<Bookmark>, line: usize) -> bool {
    if let Some(pos) = bookmarks.iter().position(|b| b.line == line) {
        bookmarks.remove(pos);
        false
    } else {
        bookmarks.push(Bookmark {
            line,
            label: line.to_string(),
        });
        true
    }
}

/// 按行号排序 + 去重（写回 sidecar 前的规整；跳转假设已排序）
pub fn normalize(bookmarks: &mut Vec<Bookmark>) {
    bookmarks.sort_by_key(|b| b.line);
    bookmarks.dedup_by_key(|b| b.line);
}

/// 下一个书签行（**循环**）：第一个严格大于 `from_line` 的书签；没有则回绕到第一个。
/// 无书签时返回 `None`。要求 `bookmarks` 已按行号升序（调用方先 [`normalize`]）。
pub fn next_after(bookmarks: &[Bookmark], from_line: usize) -> Option<usize> {
    if bookmarks.is_empty() {
        return None;
    }
    bookmarks
        .iter()
        .find(|b| b.line > from_line)
        .map(|b| b.line)
        .or_else(|| bookmarks.first().map(|b| b.line))
}

/// 上一个书签行（**循环**）：最后一个严格小于 `from_line` 的书签；没有则回绕到最后一个。
/// 无书签时返回 `None`。要求 `bookmarks` 已按行号升序。
pub fn prev_before(bookmarks: &[Bookmark], from_line: usize) -> Option<usize> {
    if bookmarks.is_empty() {
        return None;
    }
    bookmarks
        .iter()
        .rev()
        .find(|b| b.line < from_line)
        .map(|b| b.line)
        .or_else(|| bookmarks.last().map(|b| b.line))
}

/// 所有书签行号（升序；跳转与渲染共用）。无书签 = 空向量。
pub fn lines(bookmarks: &[Bookmark]) -> Vec<usize> {
    let mut out: Vec<usize> = bookmarks.iter().map(|b| b.line).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// 按一次文本编辑的行增量校正书签行号（方案 A：内核暴露的编辑增量）。
///
/// [`LineDelta`] 是 0 基（与 `DisplayMap::adjust_folds_for_edit` 同口径），本函数把它
/// 落到 1 基书签上，规则与 VSCode 书签插件一致：
/// - 编辑区之前的书签：不动；
/// - 落在被替换 / 删除行内的书签：锚定到编辑区首行（不随文本一起消失）；
/// - 编辑区之后的书签：整体平移 `inserted - deleted` 行（删多则上移，下限钳到编辑区首行）。
///
/// 返回是否发生了平移（调用方据此决定是否重绘 / 写回 sidecar）。
pub fn adjust_for_edit(bookmarks: &mut Vec<Bookmark>, delta: LineDelta) -> bool {
    if delta.deleted_lines == 0 && delta.inserted_lines == 0 {
        return false;
    }
    // 1 基的编辑区行区间：编辑区首行 = start_line+1，末行 = start_line+deleted_lines
    let edit_first = delta.start_line + 1;
    let edit_last = delta.start_line + delta.deleted_lines;
    let shift = delta.inserted_lines as isize - delta.deleted_lines as isize;
    let mut changed = false;
    for b in bookmarks.iter_mut() {
        if b.line < edit_first {
            continue;
        }
        if b.line <= edit_last {
            // 书签落在被删 / 被替换的行上：锚定到编辑区首行
            b.line = edit_first;
            changed = true;
        } else {
            // 编辑区之后：整体平移；删行过多时钳到编辑区首行（不进编辑区）
            let new_line = (b.line as isize + shift).max(edit_first as isize) as usize;
            if new_line != b.line {
                b.line = new_line;
                changed = true;
            }
        }
    }
    if changed {
        normalize(bookmarks);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(line: usize) -> Bookmark {
        Bookmark {
            line,
            label: line.to_string(),
        }
    }

    fn d(start_line: usize, deleted_lines: usize, inserted_lines: usize) -> LineDelta {
        LineDelta {
            start_line,
            deleted_lines,
            inserted_lines,
        }
    }

    #[test]
    fn adjust_shifts_bookmarks_after_inserted_block() {
        // 0 基第 1 行（=1 基第 2 行）前插入 2 行：之后的 1 基 3/4 → 5/6
        let mut b = vec![bm(1), bm(3), bm(4)];
        assert!(adjust_for_edit(&mut b, d(1, 0, 2)));
        let got: Vec<usize> = b.iter().map(|x| x.line).collect();
        assert_eq!(got, vec![1, 5, 6], "编辑区之后整体 +2");
    }

    #[test]
    fn adjust_removes_lines_and_shifts_up() {
        // 0 基第 0 行起删 2 行（=1 基 1..=2 行被删）：3 → 锚到编辑区首行 1，4 → 2
        let mut b = vec![bm(1), bm(3), bm(4)];
        assert!(adjust_for_edit(&mut b, d(0, 2, 0)));
        // adjust 内部会 normalize（同一行去重）：原本在 1 的书签 + 锚到 1 的 3 → 都落在行 1
        assert_eq!(lines(&b), vec![1, 2], "被删行锚到首行（与已在首行的合并），之后的 4 上移到 2");
    }

    #[test]
    fn adjust_anchors_bookmark_in_deleted_range() {
        // 0 基第 1 行（1 基第 2 行）删 1 行：位于该行的书签锚到编辑区首行(1基2)
        let mut b = vec![bm(2), bm(5)];
        assert!(adjust_for_edit(&mut b, d(1, 1, 0)));
        let got: Vec<usize> = b.iter().map(|x| x.line).collect();
        assert_eq!(got, vec![2, 4], "被删行锚到首行 2，之后的 5 → 4");
    }

    #[test]
    fn adjust_noop_when_no_edit() {
        let mut b = vec![bm(1), bm(3)];
        assert!(!adjust_for_edit(&mut b, d(0, 0, 0)), "零增量 = 无变化");
        assert!(!adjust_for_edit(&mut b, d(5, 0, 0)), "空删除 = 无变化");
    }

    #[test]
    fn adjust_shift_does_not_cross_into_edit_block() {
        // 删 5 行但书签在编辑区之后很远：钳到编辑区首行
        let mut b = vec![bm(3)];
        assert!(adjust_for_edit(&mut b, d(0, 5, 0)));
        assert_eq!(b[0].line, 1, "下移钳到编辑区首行（1 基）");
    }

    #[test]
    fn toggle_adds_then_removes() {
        let mut b = Vec::new();
        assert!(toggle(&mut b, 3), "新增返回 true");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].line, 3);
        assert_eq!(b[0].label, "3", "默认说明 = 行号文本");

        assert!(!toggle(&mut b, 3), "再次切换 = 删除，返回 false");
        assert!(b.is_empty());
    }

    #[test]
    fn normalize_sorts_and_dedups() {
        let mut b = vec![bm(5), bm(1), bm(3), bm(3), bm(2)];
        normalize(&mut b);
        let got: Vec<usize> = b.iter().map(|x| x.line).collect();
        assert_eq!(got, vec![1, 2, 3, 5], "升序且去重");
    }

    #[test]
    fn next_after_wraps_to_first() {
        let b = vec![bm(1), bm(3), bm(7)];
        assert_eq!(next_after(&b, 1), Some(3), "严格大于当前行");
        assert_eq!(next_after(&b, 4), Some(7));
        assert_eq!(next_after(&b, 7), Some(1), "到末尾回绕到第一个");
        assert_eq!(next_after(&b, 0), Some(1));
        assert_eq!(next_after(&[], 1), None, "无书签 = None");
    }

    #[test]
    fn prev_before_wraps_to_last() {
        let b = vec![bm(1), bm(3), bm(7)];
        assert_eq!(prev_before(&b, 7), Some(3), "严格小于当前行");
        assert_eq!(prev_before(&b, 4), Some(3));
        assert_eq!(prev_before(&b, 1), Some(7), "到开头回绕到最后一个");
        assert_eq!(prev_before(&b, 0), Some(7));
        assert_eq!(prev_before(&[], 1), None);
    }

    #[test]
    fn lines_returns_sorted_unique() {
        let b = vec![bm(3), bm(1), bm(3), bm(2)];
        assert_eq!(lines(&b), vec![1, 2, 3]);
        assert!(lines(&[]).is_empty());
    }
}
