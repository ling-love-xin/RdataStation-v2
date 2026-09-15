//! 结果网格（A14 最小版）
//!
//! 网格本身用组件库的 `DataTable` + `TableState`（虚拟滚动、列宽拖拽、排序钩子都是现成的，
//! 不手搓——对齐项目「组件优先」规范）。本文件只做两件事：
//!
//! 1. **把结果变成列与行**（[`ResultGridDelegate`]）：纯数据 + 单元格渲染；
//! 2. **把空态说清楚**：没有结果、执行失败时都不留空白面板（"按了没反应"的另一面）。
//!
//! 数据源是 [`crate::store::ResultStore`]（结果唯一权威）。网格**不持有真值**——
//! delegate 里的行是从权威那里拷来的投影，`set_data` 是唯一的写入点。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::*;

use crate::ui;

/// 网格数据（表头 + 行）
#[derive(Default)]
pub struct ResultGridDelegate {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// 空态文案（没有结果 / 执行失败都在这里说清楚）
    empty_text: String,
}

impl ResultGridDelegate {
    /// 空网格：只带一句空态说明
    pub fn empty(text: impl Into<String>) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            empty_text: text.into(),
        }
    }

    /// 换一批数据（结果集变化时从权威拷贝一次；网格不持有第二份真值）
    pub fn set_data(&mut self, columns: Vec<String>, rows: Vec<Vec<String>>) {
        self.columns = columns;
        self.rows = rows;
        self.empty_text = "查询返回 0 行".to_string();
    }

    /// 清空（切到无结果的模式 / 关闭文档时）
    pub fn clear(&mut self, text: impl Into<String>) {
        self.columns.clear();
        self.rows.clear();
        self.empty_text = text.into();
    }

    /// 网格里的列名（供测试断言；不复制行数据）
    pub fn columns_for_test(&self) -> &[String] {
        &self.columns
    }

    /// 网格里的行数（供测试断言）
    pub fn row_count_for_test(&self) -> usize {
        self.rows.len()
    }
}

impl TableDelegate for ResultGridDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        let name = self
            .columns
            .get(col_ix)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", col_ix + 1));
        Column::new(format!("col-{col_ix}"), name)
            .width(ui::RESULT_COLUMN_WIDTH)
            .min_width(ui::RESULT_COLUMN_MIN_WIDTH)
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // 单元格文案：缺列/缺行显示空串而不是 panic（结果行由驱动给出，形状不可全信）
        let text = self
            .rows
            .get(row_ix)
            .and_then(|row| row.get(col_ix))
            .cloned()
            .unwrap_or_default();
        let muted = cx.theme().colors.muted_foreground;
        let foreground = cx.theme().colors.foreground;

        // NULL 是 SQL 里的一个真实值，灰显以区别于字符串 "NULL"
        let is_null = text == "NULL";
        div()
            .px_2()
            .truncate()
            .text_xs()
            .text_color(if is_null { muted } else { foreground })
            .child(SharedString::from(text))
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(muted)
            .child(self.empty_text.clone())
    }

    /// 导出用（1b 的 CSV 导出直接取这里；现在就让复制/导出有真值可用）
    fn cell_text(&self, row_ix: usize, col_ix: usize, _cx: &App) -> String {
        self.rows
            .get(row_ix)
            .and_then(|row| row.get(col_ix))
            .cloned()
            .unwrap_or_default()
    }
}

/// 建一个结果表状态（面板构造时一次；结果变化用 `refresh` 而不是重建）
pub fn new_table_state(
    delegate: ResultGridDelegate,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TableState<ResultGridDelegate>> {
    cx.new(|cx| {
        TableState::new(delegate, window, cx)
            .col_resizable(true)
            .row_selectable(true)
            .cell_selectable(true)
    })
}

/// 渲染结果网格（含顶部状态行）
pub fn render(
    state: &Entity<TableState<ResultGridDelegate>>,
    summary: &str,
    cx: &App,
) -> impl IntoElement {
    let muted = cx.theme().colors.muted_foreground;
    let border = cx.theme().colors.border;

    div()
        .v_flex()
        .w_full()
        .h(rems(ui::RESULT_PANE_HEIGHT))
        .min_h(rems(ui::RESULT_MIN_HEIGHT))
        .border_t(ui::HAIRLINE)
        .border_color(border)
        .child(
            div()
                .h_flex()
                .items_center()
                .px_2()
                .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                .text_xs()
                .text_color(muted)
                .child(SharedString::from(summary.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .child(DataTable::new(state).stripe(true).bordered(false)),
        )
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::ResultGridDelegate;
    use gpui_kit::App;
    use gpui_kit::component::table::TableDelegate as _;

    fn grid() -> ResultGridDelegate {
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![
                vec!["1".to_string(), "a".to_string()],
                vec!["2".to_string(), "NULL".to_string()],
            ],
        );
        grid
    }

    #[test]
    fn columns_and_rows_follow_the_data() {
        let grid = grid();
        let columns = grid.columns_for_test();
        assert_eq!(columns, ["id".to_string(), "name".to_string()]);
        assert_eq!(grid.row_count_for_test(), 2);
    }

    #[test]
    fn empty_grid_says_why_instead_of_showing_a_blank_panel() {
        let grid = ResultGridDelegate::empty("尚未执行");
        assert_eq!(grid.row_count_for_test(), 0);
        assert!(grid.columns_for_test().is_empty());
    }

    #[test]
    fn clearing_drops_previous_rows() {
        let mut grid = grid();
        grid.clear("执行失败：boom");
        assert_eq!(grid.row_count_for_test(), 0);
        assert!(grid.columns_for_test().is_empty());
    }

    /// 列数与行数要和真实数据一致（delegate 是网格唯一的取数口）
    #[gpui_kit::test]
    fn delegate_reports_counts(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.columns_count(cx), 2);
            assert_eq!(grid.rows_count(cx), 2);
        });
    }
}
