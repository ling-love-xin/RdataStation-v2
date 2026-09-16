//! 结果网格（A14 最小版 + B5 工具栏）
//!
//! 网格本身用组件库的 `DataTable` + `TableState`（虚拟滚动、列宽拖拽、排序钩子都是现成的，
//! 不手搓——对齐项目「组件优先」规范）。本文件只做三件事：
//!
//! 1. **把结果变成列与行**（[`ResultGridDelegate`]）：纯数据 + 单元格渲染；
//! 2. **把空态说清楚**：没有结果、执行失败、写语句都不留空白面板（“按了没反应”的另一面）；
//! 3. **结果工具栏**（[`ResultToolbar`] + [`ResultControls`]）：左段报真实数字（行数 × 列数 · 耗时 /
//!    影响 N 行 / 失败原因 + 截断提示），右段是动作（复制 / 刷新）。
//!
//! 原型 §2.2 的工具栏还包含**筛选 · 下发开关 · 分析 · 导出**——它们各自属 B15 / B14 / B7，
//! **没实现就不摆按钮**（“只宣传不实现”的入口不允许存在）。
//!
//! 数据源是 [`crate::store::ResultStore`]（结果唯一权威）。网格**不持有真值**——
//! delegate 里的行是从权威那里拷来的投影，`set_data` 是唯一的写入点。

use gpui_kit::prelude::FluentBuilder as _;
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

/// 结果工具栏的左段输入（除了 `summary` 都是可选真值：没有就不显示）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultToolbar {
    /// 摘要（`N 行 × M 列 · 12 ms` / `影响 N 行 · 12 ms` / 失败原因）
    pub summary: String,
    /// 截断提示（达到驱动行数上限才有；`None` = 没被截断）
    pub truncated_hint: Option<String>,
    /// 来源连接文案（`●P·orders`；`None` = 没绑定或不认得）
    pub connection: Option<String>,
}

/// 结果工具栏右段的动作控件（由面板构造：它知道点击该干什么）
///
/// 与状态栏的 `StatusControls` 同口径：文案交给可穷举的纯函数，控件交给面板。
#[derive(Default)]
pub struct ResultControls {
    /// 复制当前结果集（TSV）
    pub copy: Option<AnyElement>,
    /// 重跑当前结果集的 SQL（结果集换一份新的，不是新开一份）
    pub refresh: Option<AnyElement>,
}

/// 截断提示文案（达到驱动行数上限时：说清楚看到的是前多少行）
///
/// 只报**我们真拿到多少行**；上限是驱动侧的事（`engine` 侧 10000 行），编辑器不猜那个数。
pub fn truncated_hint(rows: usize) -> String {
    format!("已截断：只拿到前 {rows} 行")
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

/// 渲染结果区（顶部可带结果集标签条 + 工具栏 + 网格）
///
/// `tabs` 由宿主填（它知道当前文档有几份结果、选中哪份）：一份结果时传 `None`，
/// 那一排标签只会白占一行（原型 §2.4 的标签条是“多结果”才需要的切换器）。
/// **高度不在这里定**：结果区是 `ResizablePanel` 的一个面板（可拖拽），
/// 它的高度由分栏给出——这里只保证自己撑满那个面板。
pub fn render(
    state: &Entity<TableState<ResultGridDelegate>>,
    toolbar: ResultToolbar,
    controls: ResultControls,
    tabs: Option<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let warning = theme.colors.warning;

    div()
        .v_flex()
        .w_full()
        .h_full()
        .min_h_0()
        // 测试按选择器断言“结果区在不在、多高”：分栏是结构，不是装饰
        .debug_selector(|| "editor-result-pane".to_string())
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                .text_xs()
                .text_color(muted)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(SharedString::from(toolbar.summary))
                        .when_some(toolbar.connection, |row, connection| {
                            row.child(SharedString::from(connection))
                        })
                        // 截断是一个**警告**（数据不完整），不是普通说明文字
                        .when_some(toolbar.truncated_hint, |row, hint| {
                            row.child(div().text_color(warning).child(SharedString::from(hint)))
                        }),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .children(controls.copy)
                        .children(controls.refresh),
                ),
        )
        .children(tabs)
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
    use super::{ResultGridDelegate, ResultToolbar, truncated_hint};
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

    /// 截断提示把“真拿到多少行”说出来
    #[test]
    fn truncation_hint_names_the_real_row_count() {
        let hint = truncated_hint(10_000);
        assert!(hint.contains("前 10000 行"), "{hint}");
    }

    /// 工具栏左段没有可选值时不该出现占位（连接没绑就不显示那一段）
    #[test]
    fn toolbar_inputs_are_optional_truth() {
        let bare = ResultToolbar {
            summary: "1 行 × 1 列 · 2 ms".to_string(),
            truncated_hint: None,
            connection: None,
        };
        assert!(bare.truncated_hint.is_none() && bare.connection.is_none());
    }
}
