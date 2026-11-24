//! 表格命中测试：窗口坐标 → (行, 列)。
//!
//! 用途：单元格拖拽选区。DBeaver 在 `mouseMove` 里把鼠标位置换算成 hover 格
//! （`handleCellHover`，`LightGrid.java:3539-3566`），再把该格当作「带 Shift 的新终点」
//! 调 `updateCellSelection(newCell, ctrlFlag | SWT.MOD2, ...)`；本组件要做到同一件事，
//! 就必须有一条独立的坐标换算。换算只依赖布局量，故抽成纯函数，便于直接单测。
//!
//! 坐标约定：`(x, y)` 相对**表格容器**左上角（容器 = 表头 + 行区域整体）。

/// 命中测试所需的布局量（单位 px，坐标相对表格容器左上角）。
#[derive(Clone, Debug, Default)]
pub struct CellHitGeometry {
    /// 行区域顶边（= 表头总高；`y < body_top` 落在表头，不算格）
    pub body_top: f32,
    /// 行高（`TableSize::table_row_height()`）
    pub row_height: f32,
    /// 已向下滚动距离（= `-vertical_scroll_handle.offset().y`，≥ 0）
    pub scroll_y: f32,
    /// 已向右滚动距离（= `-horizontal_scroll_handle.offset().x`，≥ 0）
    pub scroll_x: f32,
    /// 容器宽度（判定右侧固定列用）
    pub width: f32,
    /// 左固定列合计宽度（不随水平滚动移动）
    pub left_fixed_width: f32,
    /// 右固定列合计宽度（贴容器右缘）
    pub right_fixed_width: f32,
    /// 左固定列数量
    pub left_fixed_count: usize,
    /// 右固定列数量
    pub right_fixed_count: usize,
    /// 全部列宽（含两侧固定列，按显示序）
    pub col_widths: Vec<f32>,
    /// 行数
    pub rows_count: usize,
}

/// 把相对坐标换算成 `(row, col)`。
///
/// 与 DBeaver 的差别只在越界处理：DBeaver 在 `mouseMove` 里发现 hover 落在可视区外时
/// 改用 `bottomIndex+1` / `topIndex-1`（再 `showItem` 滚一格），此处先按边界**收拢**
/// 到首/末行列（自动滚动另做），保证拖到边缘时选区停在该边界而不跳空。
pub fn cell_at_point(x: f32, y: f32, g: &CellHitGeometry) -> Option<(usize, usize)> {
    let cols = g.col_widths.len();
    if g.rows_count == 0 || cols == 0 || g.row_height <= 0.0 || g.left_fixed_count > cols {
        return None;
    }
    // 表头区域：DBeaver `getRow` 在这里返回 -1，拖拽逻辑改走 `topIndex-1`
    if y < g.body_top {
        return None;
    }

    let row = (((y - g.body_top + g.scroll_y) / g.row_height).floor()).max(0.0) as usize;
    let row = row.min(g.rows_count - 1);

    let col_start = g.left_fixed_count.min(cols);
    let col_end = cols.saturating_sub(g.right_fixed_count.min(cols));

    let col = if col_start > 0 && x < g.left_fixed_width {
        // 左固定区：从列 0 起累积，不受 scroll_x 影响
        index_in_range(g, 0, col_start, x)
    } else if g.right_fixed_width > 0.0
        && col_end < cols
        && x >= g.width - g.right_fixed_width
    {
        // 右固定区：贴容器右缘
        let x_right = x - (g.width - g.right_fixed_width);
        index_in_range(g, col_end, cols, x_right)
    } else if col_end > col_start {
        // 可滚动区：扣掉左固定宽，再叠加水平滚动量
        let x_scroll = (x - g.left_fixed_width + g.scroll_x).max(0.0);
        index_in_range(g, col_start, col_end, x_scroll)
    } else {
        // 没有可滚动列（全固定）：落在最后一个左固定列
        col_start.saturating_sub(1)
    };

    Some((row, col.min(cols - 1)))
}

/// 在 `[start, end)` 里按累积宽度找列；越过最后一个列宽时返回最后一列。
fn index_in_range(g: &CellHitGeometry, start: usize, end: usize, x: f32) -> usize {
    let mut acc = 0.0;
    let mut last = start;
    for col in start..end {
        last = col;
        acc += g.col_widths[col];
        if x < acc {
            return col;
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::{cell_at_point, CellHitGeometry};

    fn base() -> CellHitGeometry {
        CellHitGeometry {
            body_top: 30.0,
            row_height: 20.0,
            rows_count: 100,
            col_widths: vec![40.0, 100.0, 100.0, 100.0, 60.0],
            left_fixed_count: 1,
            right_fixed_count: 1,
            left_fixed_width: 40.0,
            right_fixed_width: 60.0,
            width: 500.0,
            ..Default::default()
        }
    }

    #[test]
    fn header_area_maps_to_no_cell() {
        let g = base();
        assert_eq!(cell_at_point(10.0, 5.0, &g), None);
    }

    #[test]
    fn body_maps_to_row_and_scrollable_column() {
        let g = base();
        // 第一行、x = 40 + 20 → 可滚动区第 0 个（列 1）
        assert_eq!(cell_at_point(60.0, 35.0, &g), Some((0, 1)));
        // 第二行
        assert_eq!(cell_at_point(60.0, 55.0, &g), Some((1, 1)));
        // 落在第 2 个可滚动列（col 2）
        assert_eq!(cell_at_point(160.0, 35.0, &g), Some((0, 2)));
    }

    #[test]
    fn scrolling_shifts_rows_and_columns() {
        let mut g = base();
        g.scroll_y = 20.0;
        g.scroll_x = 100.0;
        // 向下滚一行：y 仍是内容区第一行的高度区间，但行号 +1
        assert_eq!(cell_at_point(10.0, 35.0, &g), Some((1, 0)));
        // 向右滚一列：可滚动区首列变成原来的列 2
        assert_eq!(cell_at_point(60.0, 35.0, &g), Some((0, 2)));
    }

    #[test]
    fn fixed_columns_do_not_move_with_scroll() {
        let mut g = base();
        g.scroll_x = 100.0;
        // 左固定列
        assert_eq!(cell_at_point(10.0, 35.0, &g), Some((0, 0)));
        // 右固定列（width 500 - 60 = 440 之后）
        assert_eq!(cell_at_point(470.0, 35.0, &g), Some((0, 4)));
    }

    #[test]
    fn out_of_range_points_clamp_to_the_last_row_and_column() {
        let g = base();
        // y 远超出内容区底部
        assert_eq!(cell_at_point(60.0, 5000.0, &g), Some((99, 3)));
        // x 落在可滚动区最右之后（但没到右固定区）
        let mut g2 = base();
        g2.right_fixed_count = 0;
        g2.right_fixed_width = 0.0;
        assert_eq!(cell_at_point(1000.0, 35.0, &g2), Some((0, 4)));
    }
}
