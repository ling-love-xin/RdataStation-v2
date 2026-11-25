//! 数组单元格的显示拆分（刀2：收起态三件套「三角 · 首值 · `[+N-1]`」）
//!
//! 真值坐标（DBeaver）：
//!
//! | 行为 | 出处 |
//! | --- | --- |
//! | 画三角的条件：值非 null 的非空复杂值 + 展开偏好开 | `SpreadsheetPresentation.java:2517-2525` |
//! | 三角方向：展开 = `TREE_COLLAPSE`（`▼`）、收起 = `TREE_EXPAND`（`▶`） | 同上 `:2524` |
//! | 收起态文本：取**最深**首个元素再格式化 | 同文件 `formatValue:2620-2632` |
//! | 收起态 hint：未展开且 `size > 1` 才给 `[+size-1]` | `ArrayCellHintProvider.java:48-68` |
//! | 展开态文本：`[size]` | `SpreadsheetPresentation.java:2620-2627` |
//! | 图标在单元格内、文本前，间距 `INSIDE_MARGIN = 3` | `GridCellRenderer.java:158-172` |
//! | hint 在文本右侧，间距 `LEFT_MARGIN = 6` | 同文件 `renderHints:339` |
//!
//! 两处**有意差异**（都因为本仓没有对应真值能力，不是猜的）：
//! - 空数组（`{}`）：真值里空集合不画三角、文本落空串；本仓显示原文 `{}`（原文可读、
//!   不把一个真实值画成空白）；
//! - 非法字面量：真值侧值本身是 `DBDCollection` 对象（不存在"不是数组"的可能），本仓值
//!   是驱动给的文本，解析失败就按普通文本画（`Plain`）。

use crate::store::{parse_pg_array_literal, ArrayItem, ColumnKind};

/// 收起态三角（真值 `UIIcon.TREE_EXPAND`）
pub(crate) const ARRAY_COLLAPSED_GLYPH: &str = "▶";
/// 展开态三角（真值 `UIIcon.TREE_COLLAPSE`）
pub(crate) const ARRAY_EXPANDED_GLYPH: &str = "▼";
/// 三角与文本的间距（`GridCellRenderer.INSIDE_MARGIN = 3`）
pub(crate) const ARRAY_GLYPH_SPACING: f32 = 3.0;
/// hint 与文本的间距（`GridCellRenderer.LEFT_MARGIN = 6`）
pub(crate) const ARRAY_HINT_SPACING: f32 = 6.0;

/// 数组单元格该怎么画（纯判定，`render_td` 直接消费）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArrayCellDisplay {
    /// 不按数组画：不是数组列 / `NULL` / 不是合法字面量 / 空数组
    Plain,
    /// 收起态：`▶` + 首元素（元素多于 1 个时追加 `[+N-1]`）
    Collapsed {
        /// 最深首个元素的展示文本
        lead: String,
        /// 元素数 > 1 时的 `[+N-1]`
        hint: Option<String>,
    },
    /// 展开态：`▼` + `[N]`（子行是刀3 的事，这里只给父格文本）
    Expanded {
        /// `[N]`
        label: String,
    },
}

/// 判定这一格按哪种形态画
///
/// `expanded` 由调用方传入展开状态（刀2 恒 `false`；刀3 接 `RowRef` 的展开映射）。
pub(crate) fn array_cell_display(kind: ColumnKind, text: &str, expanded: bool) -> ArrayCellDisplay {
    if kind != ColumnKind::Array || text == "NULL" {
        return ArrayCellDisplay::Plain;
    }
    let Some(items) = parse_pg_array_literal(text) else {
        return ArrayCellDisplay::Plain;
    };
    if items.is_empty() {
        return ArrayCellDisplay::Plain;
    }
    if expanded {
        return ArrayCellDisplay::Expanded {
            label: format!("[{}]", items.len()),
        };
    }
    ArrayCellDisplay::Collapsed {
        lead: deepest_first(&items),
        hint: (items.len() > 1).then(|| format!("[+{}]", items.len() - 1)),
    }
}

/// 收起态显示的首值：往嵌套里走到底再取第一个（真值 `retrieveDeepestCollectionElement`）
fn deepest_first(items: &[ArrayItem]) -> String {
    let mut item = &items[0];
    loop {
        match item {
            ArrayItem::Nested(inner) if !inner.is_empty() => item = &inner[0],
            other => return other.display(),
        }
    }
}

/// 行号槽里的行级三角该画哪个字形（`None` = 这一行没有可展开的数组，不画）
///
/// 真值 `GridRowRenderer.java:73-86` + `LightGrid.getRowState:3244-3255`：
/// - `ElementState.NONE` → 不画；`EXPANDED` → `TREE_COLLAPSE`（`▼`）；`COLLAPSED` → `TREE_EXPAND`（`▶`）；
/// - 而 `isRowExpanded` 的判据是「该行**所有**可展开列都展开」（`isAllColumnsExpanded`，`:3240-3242`），
///   部分展开仍算 `COLLAPSED`（画 `▶`）。
///
/// 本仓「可展开列」= 该行至少一格的 [`array_cell_display`] 不是 `Plain`（解析成功且非空），
/// 与 [`crate::grid::delegate::ResultGridDelegate::toggle_array_expand`] 的门控同一口径。
pub(crate) fn row_expander_glyph(expandable: usize, expanded: usize) -> Option<&'static str> {
    if expandable == 0 {
        return None;
    }
    if expanded >= expandable {
        Some(ARRAY_EXPANDED_GLYPH)
    } else {
        Some(ARRAY_COLLAPSED_GLYPH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_array_columns_are_split() {
        // 类型不是 Array：哪怕值长得像数组，也按普通文本画
        assert_eq!(
            array_cell_display(ColumnKind::Text, "{755,847,78}", false),
            ArrayCellDisplay::Plain
        );
        assert_eq!(
            array_cell_display(ColumnKind::Json, "[1,2]", false),
            ArrayCellDisplay::Plain
        );
    }

    #[test]
    fn null_and_empty_arrays_fall_back_to_plain_text() {
        assert_eq!(
            array_cell_display(ColumnKind::Array, "NULL", false),
            ArrayCellDisplay::Plain
        );
        // 空数组：不画三角，原文照旧（真值是空串，本仓有意保留 `{}`）
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{}", false),
            ArrayCellDisplay::Plain
        );
    }

    #[test]
    fn invalid_literals_fall_back_to_plain_text() {
        for text in ["{a,b", "[1,2]", "{,}", "not an array"] {
            assert_eq!(
                array_cell_display(ColumnKind::Array, text, false),
                ArrayCellDisplay::Plain,
                "值 {text:?}"
            );
        }
    }

    #[test]
    fn collapsed_state_shows_first_item_and_remainder_hint() {
        let display = array_cell_display(ColumnKind::Array, "{755,847,78}", false);
        assert_eq!(
            display,
            ArrayCellDisplay::Collapsed {
                lead: "755".to_string(),
                hint: Some("[+2]".to_string()),
            }
        );
        // 单元素：没有 hint（真值 `size > 1` 才给）
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{vip}", false),
            ArrayCellDisplay::Collapsed {
                lead: "vip".to_string(),
                hint: None,
            }
        );
    }

    #[test]
    fn quoted_first_item_keeps_its_text_and_null_renders_as_null() {
        assert_eq!(
            array_cell_display(ColumnKind::Array, r#"{"a,b","c"}"#, false),
            ArrayCellDisplay::Collapsed {
                lead: "a,b".to_string(),
                hint: Some("[+1]".to_string()),
            }
        );
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{NULL,x}", false),
            ArrayCellDisplay::Collapsed {
                lead: "NULL".to_string(),
                hint: Some("[+1]".to_string()),
            }
        );
    }

    #[test]
    fn nested_arrays_lead_with_the_deepest_first_scalar() {
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{{1,2},{3,4}}", false),
            ArrayCellDisplay::Collapsed {
                lead: "1".to_string(),
                hint: Some("[+1]".to_string()),
            }
        );
        // 嵌套里第一个是空子数组：退回空字面量文本，不 panic
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{{},{1}}", false),
            ArrayCellDisplay::Collapsed {
                lead: "{}".to_string(),
                hint: Some("[+1]".to_string()),
            }
        );
    }

    #[test]
    fn expanded_state_shows_the_element_count() {
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{755,847,78}", true),
            ArrayCellDisplay::Expanded {
                label: "[3]".to_string(),
            }
        );
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{vip}", true),
            ArrayCellDisplay::Expanded {
                label: "[1]".to_string(),
            }
        );
        // 展开态的空数组仍然是 `Plain`（没有子行可铺）
        assert_eq!(
            array_cell_display(ColumnKind::Array, "{}", true),
            ArrayCellDisplay::Plain
        );
    }

    #[test]
    fn row_expander_is_absent_without_any_array_cell() {
        // 这一行没有任何可展开的格子：行号槽不画三角（也不留空位）
        assert_eq!(row_expander_glyph(0, 0), None);
    }

    #[test]
    fn row_expander_points_right_until_every_column_is_expanded() {
        // 一个数组列、未展开 → ▶
        assert_eq!(row_expander_glyph(1, 0), Some(ARRAY_COLLAPSED_GLYPH));
        // 部分展开也算收起态（真值 `isAllColumnsExpanded` 才给 EXPANDED）
        assert_eq!(row_expander_glyph(2, 1), Some(ARRAY_COLLAPSED_GLYPH));
        // 全展开 → ▼
        assert_eq!(row_expander_glyph(2, 2), Some(ARRAY_EXPANDED_GLYPH));
        assert_eq!(row_expander_glyph(1, 1), Some(ARRAY_EXPANDED_GLYPH));
    }
}
