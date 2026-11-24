//! 【D10】网格的矩形单元格选区（对齐 DBeaver `LightGrid` 的 `selectedCells`）
//!
//! DBeaver 的选区真值是**一组** `GridPos{col,row}` 加一个锚点格
//! （`focusColumn/focusItem`，`LightGrid.java:1590-1662`）；Shift 点击与正文拖拽都先
//! 把锚点和终点各自归一化成 `from ≤ to`，再按矩形逐格铺开
//! （`LightGrid.java:2674-2790`、`:4448-4486`）。本仓只做「单个矩形」这一档：
//! 矩形由两个对角格唯一确定，所以不必存集合——`anchor`/`focus` 就是全部真值，
//! 需要格集合的地方按矩形现算（`contains` / `rows_count` / `data_columns`）。
//!
//! 与组件的分工：组件自己只会记住**一个**选中格（`TableSelection::Cell`，
//! `vendor/gpui-component/src/table/state.rs:64-74`），它画的那层高亮也只覆盖焦点格；
//! 矩形里其余的格由 delegate 在 `render_td` 里铺同槽底色（见 `grid/cell.rs`）。

/// 一次矩形选区：`anchor` = 按下（或上一次无修饰键点击）的格，`focus` = 当前点到的格。
///
/// 坐标都是**视图坐标**：行是视图行序（筛选/排序后的行号），列是组件列下标
/// （含首列的行号槽，列 0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellRange {
    /// 锚点格（DBeaver 的 `focusColumn/focusItem` 在按下时的那份快照）
    pub anchor: (usize, usize),
    /// 当前点到的格（选区矩形的另一个对角）
    pub focus: (usize, usize),
}

impl CellRange {
    /// 单格选区（普通点击 / 键盘导航）
    pub fn single(cell: (usize, usize)) -> Self {
        Self {
            anchor: cell,
            focus: cell,
        }
    }

    /// 归一化后的行区间（含两端）
    pub fn rows(&self) -> (usize, usize) {
        let (a, b) = (self.anchor.0, self.focus.0);
        (a.min(b), a.max(b))
    }

    /// 归一化后的列区间（含两端）
    pub fn cols(&self) -> (usize, usize) {
        let (a, b) = (self.anchor.1, self.focus.1);
        (a.min(b), a.max(b))
    }

    /// 该格是否落在矩形内
    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (top, bottom) = self.rows();
        let (left, right) = self.cols();
        row >= top && row <= bottom && col >= left && col <= right
    }

    /// 矩形覆盖的行数（不含裁剪；越界由调用方另行裁剪）。
    ///
    /// 对齐 DBeaver 的 `SelectionExt.getSelectedRowCount()`
    /// （行区间 = `rows().1 - rows().0 + 1`）。
    pub fn rows_count(&self) -> usize {
        let (top, bottom) = self.rows();
        bottom - top + 1
    }

    /// 选区覆盖的**数据列**（视图列下标，升序）：去掉首列行号槽。
    ///
    /// 计算页「按列分组」要的是「选区里有哪些列」
    /// （`AggregateColumnsPanel.java:231-249`），不是格坐标，所以单独给一份；
    /// 它的长度即 DBeaver 的 `getSelectedColumnCount()`（行号槽不是数据列）。
    pub fn data_columns(&self) -> Vec<usize> {
        let (left, right) = self.cols();
        // 列 0 是行号槽（`crate::grid::table_impl` 的首列），不是数据列
        (left.max(1)..=right).collect()
    }

    /// 选区覆盖的**数据格数**（行 × 数据列）。
    ///
    /// 对齐 DBeaver 的 `getCellSelectionSize()`（`LightGrid.java:4349-4351` =
    /// `selectedCells.size()`：矩形选区内每格都在集合里、行号槽那列不在）。
    /// 状态行「单元格数」用它；行号槽自成一列，与 [`Self::data_columns`] 同口径。
    pub fn data_cells_count(&self) -> usize {
        self.rows_count() * self.data_columns().len()
    }
}

/// 状态行三项（行数 / 列数 / 单元格数）的取值口径。
///
/// DBeaver 的三项全部来自同一次 `updateSelectionCache()`
/// （`LightGrid.java:2863-2882`）：`selectedRows` / `selectedColumns` 由
/// `selectedCells` **反推**（凡有一格在该行/该列即计入），所以矩形 `rows × cols`
/// 的数就是 `(rows, cols, rows*cols)`；单格即 `(1, 1, 1)`。
///
/// `None` 表示还没建立矩形（键盘刚落到第一格，或面板自建的初始态）：按单格算，
/// 与 DBeaver 单格选区一个集合只装一格等价。
pub fn selection_counts(range: Option<&CellRange>) -> (usize, usize, usize) {
    match range {
        Some(range) => (
            range.rows_count(),
            range.data_columns().len(),
            range.data_cells_count(),
        ),
        None => (1, 1, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::{selection_counts, CellRange};

    #[test]
    fn a_range_normalizes_rows_and_columns_from_either_corner() {
        let range = CellRange {
            anchor: (5, 4),
            focus: (1, 2),
        };
        assert_eq!(range.rows(), (1, 5));
        assert_eq!(range.cols(), (2, 4));
        assert_eq!(range.rows_count(), 5);
        assert_eq!(range.data_columns(), vec![2, 3, 4]);
        assert!(range.contains(3, 3));
        assert!(!range.contains(0, 3));
        assert!(!range.contains(3, 5));
    }

    #[test]
    fn a_single_cell_range_holds_just_that_cell_and_no_data_column() {
        let range = CellRange::single((0, 0));
        assert_eq!(range.rows_count(), 1);
        assert!(range.contains(0, 0));
        // 行号槽不算数据列
        assert!(range.data_columns().is_empty());
    }

    #[test]
    fn data_columns_skip_the_row_number_slot() {
        let range = CellRange {
            anchor: (2, 0),
            focus: (4, 3),
        };
        assert_eq!(range.data_columns(), vec![1, 2, 3]);
    }

    #[test]
    fn data_cells_count_multiplies_rows_by_data_columns() {
        // 3 行 × 3 数据列（含行号槽的 2..=4 三列里，行号槽那列不算数据格）
        let range = CellRange {
            anchor: (2, 1),
            focus: (4, 3),
        };
        assert_eq!(range.rows_count(), 3);
        assert_eq!(range.data_columns().len(), 3);
        assert_eq!(range.data_cells_count(), 9);
        // 只点行号槽：没有数据格
        assert_eq!(CellRange::single((7, 0)).data_cells_count(), 0);
    }

    #[test]
    fn selection_counts_mirror_the_rectangles_rows_and_data_columns() {
        // 2 行 × 2 数据列 —— DBeaver `updateSelectionCache` 反推出来的三项
        let range = CellRange {
            anchor: (0, 1),
            focus: (1, 2),
        };
        assert_eq!(selection_counts(Some(&range)), (2, 2, 4));
        // 单格
        assert_eq!(
            selection_counts(Some(&CellRange::single((3, 4)))),
            (1, 1, 1)
        );
        // 没建立矩形 → 按单格
        assert_eq!(selection_counts(None), (1, 1, 1));
    }
}
