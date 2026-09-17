//! 结果区（原型 §2.4 的 ⑤⑥⑦ + 网格）
//!
//! 自上而下：**结果集标签条**（由宿主给）→ **工具栏**（[`ResultToolbar`]：`行数 N` │
//! `耗时 1.2s` │ `连接名` + 动作）→ **网格 或 错误卡片** → **结果状态行**（[`ResultStatus`]：
//! `共 N 行` │ `已选第 M 行` + 截断提示）。
//!
//! 网格本身用组件库的 `DataTable` + `TableState`（虚拟滚动、列宽拖拽、行选择都是现成的，
//! 不手搓）。首列是固定的 `#` 行号槽（原型 §2.4），`NULL` 用 `muted_foreground` 斜体呈现
//! ——它是 SQL 里的一个真实值，与字符串 `"NULL"` 区分开。
//!
//! 原型 §2.4 的工具栏还包含**筛选 · 下发开关 · 分析 · 导出**——它们各自属 B15 / B14 / B7，
//! **没实现就不摆按钮**；分页/取下一段属 B5b，位置留在 ⑦ 那一行。
//!
//! 数据源是 [`crate::store::ResultStore`]（结果唯一权威）。网格**不持有真值**——
//! delegate 里的行是从权威那里拷来的投影，`set_data` 是唯一的写入点。

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::{Column, DataTable, TableDelegate, TableState};
use gpui_kit::*;

use crate::ui;
use crate::view::widgets::status_bar;

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

    /// 第 `col_ix` 列是不是行号槽（首列；没有数据时不画它）
    fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 单元格文案：首列是行号，其余按 `col_ix - 1` 取数据
    ///
    /// 缺列/缺行给空串而不是 panic（结果行由驱动给出，形状不可全信）。
    fn cell(&self, row_ix: usize, col_ix: usize) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        self.rows
            .get(row_ix)
            .and_then(|row| row.get(col_ix - 1))
            .cloned()
            .unwrap_or_default()
    }
}

impl TableDelegate for ResultGridDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        // 首列是行号槽（原型 §2.4）：没有结果时不画它，空态就交回给 `render_empty`
        if self.columns.is_empty() {
            0
        } else {
            self.columns.len() + 1
        }
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if Self::is_row_number(col_ix) {
            // 行号槽：窄、钉在左边、不可拖宽/移动——它是视图辅助，不是数据列
            return Column::new("row-no", "#")
                .width(ui::RESULT_ROW_NUMBER_WIDTH)
                .min_width(ui::RESULT_ROW_NUMBER_WIDTH)
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false);
        }
        let index = col_ix - 1;
        let name = self
            .columns
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", index + 1));
        Column::new(format!("col-{index}"), name)
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
        let text = self.cell(row_ix, col_ix);
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let foreground = theme.colors.foreground;

        // 行号槽永远是灰的（它不是数据）
        if Self::is_row_number(col_ix) {
            return div()
                .px_2()
                .truncate()
                .text_xs()
                .text_color(muted)
                .child(SharedString::from(text));
        }

        // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（原型 §2.4）
        let is_null = text == "NULL";
        div()
            .px_2()
            .truncate()
            .text_xs()
            .text_color(if is_null { muted } else { foreground })
            .when(is_null, |cell| cell.italic())
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

    /// 导出用（B7 的 CSV/JSON 导出取这里；复制走 `store::ResultEntry` 的 TSV）
    fn cell_text(&self, row_ix: usize, col_ix: usize, _cx: &App) -> String {
        self.cell(row_ix, col_ix)
    }
}

/// 结果工具栏（⑥，原型 §2.4）：左段 `行数 N` │ `耗时 1.2s` │ `连接名`
///
/// 全是可选真值：没有的东西不显示，也不用占位文案撑场面。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultToolbar {
    /// 行数（`None` = 这次没有网格：写语句 / 失败）
    pub rows: Option<usize>,
    /// 写语句的影响行数（`Some` = 这次只有影响行数，没有结果集）
    pub affected_rows: Option<u32>,
    /// 这次执行失败了吗（原因在错误卡片里，这里只标一个「失败」）
    pub failed: bool,
    /// 耗时（毫秒；`None` = 还没有结论）
    pub elapsed_ms: Option<u64>,
    /// 来源连接文案（`●P·orders`；`None` = 当时未绑定 / 认不出）
    pub connection: Option<String>,
}

impl ResultToolbar {
    /// 左段文案（**纯函数**：哪几段出现、数字怎么写都在这儿定，可逐条断言）
    pub fn segments(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(affected) = self.affected_rows {
            parts.push(format!("影响 {} 行", thousands(affected as usize)));
        } else if let Some(rows) = self.rows {
            parts.push(format!("行数 {}", thousands(rows)));
        }
        if self.failed {
            parts.push("失败".to_string());
        }
        if let Some(elapsed_ms) = self.elapsed_ms {
            parts.push(format!("耗时 {}", duration_text(elapsed_ms)));
        }
        if let Some(connection) = &self.connection {
            parts.push(connection.clone());
        }
        parts
    }
}

/// 结果状态行（⑦）的输入：那一行是分页 / 总行数 / 已选 / 截断提示的家
///
/// 分页与「取下一段」属 B5b：接上之后它们也放这一行（原型 §2.4 的 `‹ 1 2 3 … ›`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultStatus {
    /// 已抓到的行数
    pub total_rows: usize,
    /// 当前选中的行（1 基；`None` = 没选中）
    pub selected_row: Option<usize>,
    /// 截断提示（`Some` = 数据不完整）
    pub truncated_hint: Option<String>,
    /// 【B5b】还能取下一段吗（`true` 时总行数显示成 `1,000+`，并摆「取下一段」）
    pub has_more: bool,
}

/// 状态行左右两段（纯函数；截断提示单独渲染——它是警告，要另一种颜色）
pub fn status_segments(status: &ResultStatus) -> (String, String) {
    // 【B5b】还有下一段时总数不是“共”而是“已抓”：`1,000+` 说的就是“至少这么多”
    let left = if status.has_more {
        format!("共 {}+ 行", thousands(status.total_rows))
    } else {
        format!("共 {} 行", thousands(status.total_rows))
    };
    let right = match status.selected_row {
        Some(row) => format!("已选第 {} 行", thousands(row)),
        None => "未选中行".to_string(),
    };
    (left, right)
}

/// 结果工具栏右段的动作控件（由面板构造：它知道点击该干什么）
///
/// 与状态栏的 `StatusControls` 同口径：文案交给可穷举的纯函数，控件交给面板。
#[derive(Default)]
pub struct ResultControls {
    /// 复制当前结果集（TSV）
    pub copy: Option<AnyElement>,
    /// 【B7】导出当前结果集（下拉：格式 × 仅已抓取 / 抓全量）
    pub export: Option<AnyElement>,
    /// 重跑当前结果集的 SQL（结果集换一份新的，不是新开一份）
    pub refresh: Option<AnyElement>,
    /// 【B5b】取下一段（只在这份结果还有下一段时给）
    pub more: Option<AnyElement>,
}

/// 结果区一次要画的东西（面板一次读齐；`render` 只负责画）
#[derive(Default)]
pub struct ResultPane {
    pub toolbar: ResultToolbar,
    /// 结果状态行（只有网格时才给；失败 / 写语句不给）
    pub status: Option<ResultStatus>,
    pub controls: ResultControls,
    /// 失败时的错误卡片（有卡片就不画网格——网格里本来也没东西）
    pub card: Option<AnyElement>,
    /// 结果集标签条（两份以上结果才给）
    pub tabs: Option<AnyElement>,
}

/// 截断提示文案（达到驱动行数上限时；原型 §2.4 的口径是 `已截断至 N 行`）
///
/// 只报**我们真拿到多少行**；上限是驱动侧的事（`engine` 侧 10000 行），编辑器不猜那个数。
pub fn truncated_hint(rows: usize) -> String {
    format!("已截断至 {} 行", thousands(rows))
}

/// 千分位（原型里的 `1,204`）
pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(ch);
    }
    text
}

/// 耗时文案（原型 §2.4 的 `1.2s`）
///
/// 不到一秒直接报毫秒：`0.0s` 什么也没说（而且首屏结果常常就在这个量级）。
pub fn duration_text(elapsed_ms: u64) -> String {
    if elapsed_ms < 1000 {
        format!("{elapsed_ms} ms")
    } else {
        status_bar::elapsed_text(std::time::Duration::from_millis(elapsed_ms))
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

/// 画结果区（⑤ 标签条 → ⑥ 工具栏 → 网格 / 错误卡片 → ⑦ 状态行）
///
/// **高度不在这里定**：结果区是 `ResizablePanel` 的一个面板（可拖拽），
/// 它的高度由分栏给出——这里只保证自己撑满那个面板。
pub fn render(
    state: &Entity<TableState<ResultGridDelegate>>,
    pane: ResultPane,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;

    // 先把各段拆开：工具栏先拿走 refresh/copy，状态行⑦再拿 more（部分移动会让字段不可用）
    let ResultPane {
        toolbar,
        status,
        controls,
        card,
        tabs,
    } = pane;
    // 有卡片就不画网格（网格里本来也没东西，画出来只是一块空白）
    let has_card = card.is_some();

    div()
        .v_flex()
        .w_full()
        .h_full()
        .min_h_0()
        // 测试按选择器断言“结果区在不在、多高”：分栏是结构，不是装饰
        .debug_selector(|| "editor-result-pane".to_string())
        // ⑤ 结果集标签条（原型里在工具栏上面）
        .children(tabs)
        // ⑥ 结果工具栏
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
                // 测试按选择器断言原型 §2.4 的竖向顺序（⑤ 标签条 → ⑥ 工具栏 → 网格 → ⑦ 状态行）
                .debug_selector(|| "editor-result-toolbar".to_string())
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .children(
                            toolbar
                                .segments()
                                .into_iter()
                                .map(|segment| SharedString::from(segment)),
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        // 顺序照原型 §2.4 的右段：… 分析 ▾ · 导出 ▾ · ⟳ · 复制
                        .children(controls.export)
                        .children(controls.refresh)
                        .children(controls.copy),
                ),
        )
        // 有卡片：卡片替掉网格；否则画网格
        .children(card)
        .when(!has_card, |pane| {
            pane.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .debug_selector(|| "editor-result-grid".to_string())
                    .child(DataTable::new(state).stripe(true).bordered(false)),
            )
        })
        // ⑦ 结果状态行
        .when_some(status, |pane, status| {
            let (left, right) = status_segments(&status);
            let warning = theme.colors.warning;
            pane.child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .debug_selector(|| "editor-result-status-row".to_string())
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(SharedString::from(left))
                            // 截断是**警告**（数据不完整），不是普通说明文字
                            .when_some(status.truncated_hint, |row, hint| {
                                row.child(
                                    div()
                                        .text_color(warning)
                                        .child(SharedString::from(hint)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(SharedString::from(right))
                            // 【B5b】取下一段就在状态行里（原型 §2.4 的 ⑦ 是分页与“取下一段”的家）
                            .children(controls.more),
                    ),
            )
        })
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        ResultGridDelegate, ResultStatus, ResultToolbar, duration_text, status_segments, thousands,
        truncated_hint,
    };
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
    ///
    /// **列数比数据列多 1**：首列是行号槽（原型 §2.4）。
    #[gpui_kit::test]
    fn delegate_reports_counts(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.columns_count(cx), 3, "行号槽 + 两列数据");
            assert_eq!(grid.rows_count(cx), 2);
        });
    }

    /// 首列是行号槽：1 基、与数据错开一位、不进导出文本的数据
    #[gpui_kit::test]
    fn the_first_column_is_a_row_number_gutter(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.cell_text(0, 0, cx), "1", "行号是 1 基");
            assert_eq!(grid.cell_text(1, 0, cx), "2");
            assert_eq!(grid.cell_text(0, 1, cx), "1", "数据从第 2 列开始");
            assert_eq!(grid.cell_text(1, 2, cx), "NULL");
        });
    }

    /// 空网格不画行号槽（没数据时那一列是空的）
    #[gpui_kit::test]
    fn an_empty_grid_has_no_gutter(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = ResultGridDelegate::empty("尚未执行");
        cx.update(|cx: &mut App| assert_eq!(grid.columns_count(cx), 0));
    }

    /// 截断提示按原型 §2.4 的口径（`已截断至 N 行`），数字带千分位
    #[test]
    fn truncation_hint_names_the_real_row_count() {
        assert_eq!(truncated_hint(10_000), "已截断至 10,000 行");
    }

    /// 工具栏左段：`行数 N` │ `耗时 1.2s` │ `连接名`，没有的东西不占位
    #[test]
    fn toolbar_segments_follow_the_prototype_order() {
        let full = ResultToolbar {
            rows: Some(1_204),
            affected_rows: None,
            failed: false,
            elapsed_ms: Some(1_200),
            connection: Some("●P·orders".to_string()),
        };
        assert_eq!(
            full.segments(),
            ["行数 1,204", "耗时 1.2s", "●P·orders"]
        );

        // 写语句：报影响行数，没有行数段
        let write = ResultToolbar {
            rows: None,
            affected_rows: Some(3),
            failed: false,
            elapsed_ms: Some(35),
            connection: None,
        };
        assert_eq!(write.segments(), ["影响 3 行", "耗时 35 ms"]);

        // 失败：只说“失败”（原因在错误卡片里）
        let failed = ResultToolbar {
            failed: true,
            ..Default::default()
        };
        assert_eq!(failed.segments(), ["失败"]);

        // 什么都没执行过：一段都不摆
        assert!(ResultToolbar::default().segments().is_empty());
    }

    /// 状态行：`共 N 行` │ `已选第 M 行`，没选中就直说
    #[test]
    fn status_segments_report_total_and_selection() {
        let idle = ResultStatus {
            total_rows: 1_204,
            selected_row: None,
            truncated_hint: None,
            has_more: false,
        };
        assert_eq!(status_segments(&idle), ("共 1,204 行".to_string(), "未选中行".to_string()));

        let picked = ResultStatus {
            selected_row: Some(12),
            ..idle.clone()
        };
        assert_eq!(status_segments(&picked).1, "已选第 12 行");
    }

    /// 【B5b】还有下一段时总数是 `1,000+`（“至少这么多”，不是“共”）
    #[test]
    fn a_partial_result_shows_a_plus_after_the_count() {
        let partial = ResultStatus {
            total_rows: 1_000,
            selected_row: None,
            truncated_hint: None,
            has_more: true,
        };
        assert_eq!(status_segments(&partial).0, "共 1,000+ 行");
    }

    /// 耗时：不到一秒报毫秒（`0.0s` 什么都没说），过一秒按人读的写法
    #[test]
    fn duration_text_is_readable() {
        assert_eq!(duration_text(0), "0 ms");
        assert_eq!(duration_text(999), "999 ms");
        assert_eq!(duration_text(1_200), "1.2s");
        assert_eq!(duration_text(102_000), "1m42s");
    }

    /// 千分位（原型里的 `1,204`）
    #[test]
    fn thousands_separates_digit_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_204), "1,204");
        assert_eq!(thousands(12_345_678), "12,345,678");
    }
}
