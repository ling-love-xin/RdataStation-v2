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

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::*;

use crate::ui;
use crate::view::widgets::status_bar;

/// 滚动到底要更多数据时的回调（面板注入：它才知道当前选中哪份结果、忙不忙）
///
/// 组件库在**可见范围接近末尾**时调 delegate 的 [`TableDelegate::load_more`]，
/// 而 delegate 在 `TableState` 里拿不到面板实体，所以走这个钩子（与执行通道同一个口径：
/// 视图不自己发执行，只把意图交回给知道全局的那一层）。
pub type LoadMoreHook = Rc<dyn Fn(&mut App)>;

/// 【B14】「按值筛选」的钩子（面板注入：把值写进筛选框并立刻生效）
pub type FilterValueHook = Rc<dyn Fn(&str, &mut App)>;

/// 【B14】「排序下发」的钩子（面板注入：按列名重查源库；入参 = （列名，是否降序））
pub type SortDownHook = Rc<dyn Fn(&str, bool, &mut App)>;

/// 【M8】「洞察此列」的钩子（面板注入：把列名交给面板，面板再连同 SQL / 连接交给宿主）
///
/// 网格不知道结果集的 SQL 与连接（那是面板/存储的事），所以这里只传列名——
/// 与「按值筛选」同一种分工：**网格只负责说“用户点了谁”**。
pub type InsightColumnHook = Rc<dyn Fn(&str, &mut App)>;

/// 右键菜单的动作（界面按它决定点击后干什么）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextAction {
    /// 按值筛选（值原样交给面板）
    FilterByValue(String),
    /// 复制此值
    CopyValue(String),
    /// 冻结 / 取消冻结这一**数据列**
    ToggleFreeze(usize),
    /// 按这一列（列名）排序并**下发重查**
    SortDown { column: String, descending: bool },
    /// 【M8】洞察这一列（列名；宿主据此取样 → 列画像）
    InsightColumn { column: String },
}

/// 右键菜单里的一项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuItem {
    pub label: String,
    pub action: ContextAction,
    /// 这项前面要不要加一条分隔线
    pub separator_before: bool,
}

/// 右键菜单的项（**纯函数**：有什么、叫什么都在这里定，界面只负责画）
///
/// 原型 §5.5 的右键入口：按值筛选（写进筛选框、默认本地）· 复制此值 ·
/// 冻结 / 取消冻结（原生 `Column.fixed`）。
///
/// `insight` = 【M8】这份结果能不能洞察（宿主接了端口 + 这份结果可取样，见
/// `ResultEntry::can_insight_column`）——**能力没有就不摆入口**。
pub fn context_menu_items(
    value: &str,
    column_name: &str,
    column: usize,
    frozen: bool,
    insight: bool,
) -> Vec<ContextMenuItem> {
    let preview = preview_of(value);
    let mut items = vec![
        ContextMenuItem {
            label: format!("按值筛选「{preview}」"),
            action: ContextAction::FilterByValue(value.to_string()),
            separator_before: false,
        },
        ContextMenuItem {
            label: "复制此值".to_string(),
            action: ContextAction::CopyValue(value.to_string()),
            separator_before: false,
        },
        ContextMenuItem {
            label: format!("按「{column_name}」升序（下发源库）"),
            action: ContextAction::SortDown {
                column: column_name.to_string(),
                descending: false,
            },
            separator_before: true,
        },
        ContextMenuItem {
            label: format!("按「{column_name}」降序（下发源库）"),
            action: ContextAction::SortDown {
                column: column_name.to_string(),
                descending: true,
            },
            separator_before: false,
        },
        ContextMenuItem {
            label: if frozen {
                "取消冻结此列".to_string()
            } else {
                format!("冻结「{column_name}」")
            },
            action: ContextAction::ToggleFreeze(column),
            separator_before: false,
        },
    ];
    if insight {
        items.push(ContextMenuItem {
            label: format!("洞察「{column_name}」这一列"),
            action: ContextAction::InsightColumn {
                column: column_name.to_string(),
            },
            separator_before: true,
        });
    }
    items
}

/// 网格数据（表头 + 行）
#[derive(Default)]
pub struct ResultGridDelegate {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// 空态文案（没有结果 / 执行失败都在这里说清楚）
    empty_text: String,
    /// 这份结果**还能不能取下一段**（面板从权威结果同步过来；`load_more` 据此决定要不要真的取）
    has_more: bool,
    /// 正在取下一段（防止“滚动到底”在回填之前反复触发）
    loading_more: bool,
    /// 取下一段的回调（面板构造时注入）
    on_load_more: Option<LoadMoreHook>,
    /// 【B15】本地筛选词（视图层：只影响看见与导出的行，**不重查数据库**）
    filter: String,
    /// 【B15】本地排序（列下标按**数据列**算，`0` = 第一数据列；`None` = 原顺序）
    sort: Option<(usize, bool /* 降序 */)>,
    /// 【B14】最近右键的单元格（视图行号，列下标含行号槽）——组件库没给访问器，
    /// 所以在 `render_td` 上自己记（事件从最内层开始派发，不会丢）
    context_cell: Option<(usize, usize)>,
    /// 【B15】冻结的数据列（原生 `Column.fixed`：横向滚动时钉在左侧）
    frozen: Vec<usize>,
    /// 【B14】按值筛选的钩子（面板注入）
    on_filter_value: Option<FilterValueHook>,
    /// 【B14】排序下发的钩子（面板注入）
    on_sort_down: Option<SortDownHook>,
    /// 【M8】「洞察此列」的钩子（宿主接端口时面板才注入）
    on_insight_column: Option<InsightColumnHook>,
    /// 【M8】这份结果能不能洞察（面板同步数据时算：见 `ResultEntry::can_insight_column`）
    insight_available: bool,
    /// 【B15】视图行序：当前看着的这一串行，元素是 `rows` 里的下标
    ///
    /// 筛选与排序都只改这个映射，**数据行一行不动**（所以清除筛选能原样恢复，
    /// 也不会把“已抓到的窗口”搞乱）。行号列显示的是**视图行号**，不是数据行号。
    view_rows: Vec<usize>,
}

impl ResultGridDelegate {
    /// 空网格：只带一句空态说明
    pub fn empty(text: impl Into<String>) -> Self {
        Self {
            empty_text: text.into(),
            ..Default::default()
        }
    }

    /// 换一批数据（结果集变化时从权威拷贝一次；网格不持有第二份真值）
    pub fn set_data(&mut self, columns: Vec<String>, rows: Vec<Vec<String>>) {
        self.columns = columns;
        self.rows = rows;
        self.empty_text = "查询返回 0 行".to_string();
        // 换了一份结果，旧的排序与筛选不该跟过来（列都可能不是同一批）
        self.sort = None;
        // 冻结也一样：列都换了一批，钉子留在旧列号上没有意义
        self.frozen.clear();
        self.context_cell = None;
        self.rebuild_view_rows();
    }

    /// 清空（切到无结果的模式 / 关闭文档时）
    pub fn clear(&mut self, text: impl Into<String>) {
        self.columns.clear();
        self.rows.clear();
        self.view_rows.clear();
        self.empty_text = text.into();
        // 没有结果就没有“下一段”：留着会让滚动到底去取一份已经不存在的结果
        self.has_more = false;
        self.loading_more = false;
    }

    /// 【B14】记下最近右键的单元格（由 `render_td` 上的右键处理调用）
    pub fn set_context_cell(&mut self, cell: Option<(usize, usize)>) {
        self.context_cell = cell;
    }

    /// 【B14】注入「按值筛选」钩子（面板构造时一次）
    pub fn set_filter_value_hook(&mut self, hook: FilterValueHook) {
        self.on_filter_value = Some(hook);
    }

    /// 【M8】注入「洞察此列」钩子（宿主接了端口时面板构造一次）
    pub fn set_insight_column_hook(&mut self, hook: InsightColumnHook) {
        self.on_insight_column = Some(hook);
    }

    /// 【M8】这份结果能不能洞察（面板每次同步结果时设；钩子没装也等于不能）
    pub fn set_insight_available(&mut self, available: bool) {
        self.insight_available = available;
    }

    /// 【M8】测试用：钩子装上了吗
    pub fn has_insight_column_hook(&self) -> bool {
        self.on_insight_column.is_some()
    }

    /// 【M8】测试用：入口现在该不该出现（钩子在 + 这份结果可洞察）
    pub fn insight_available(&self) -> bool {
        self.on_insight_column.is_some() && self.insight_available
    }

    /// 【B14】注入「排序下发」钩子（面板构造时一次）
    pub fn set_sort_down_hook(&mut self, hook: SortDownHook) {
        self.on_sort_down = Some(hook);
    }

    /// 【B15】这一数据列冻结了吗
    pub fn is_frozen(&self, column: usize) -> bool {
        self.frozen.contains(&column)
    }

    /// 【B15】冻结 / 取消冻结（切换；返回切换后的状态）
    pub fn toggle_freeze(&mut self, column: usize) -> bool {
        if let Some(at) = self.frozen.iter().position(|frozen| *frozen == column) {
            self.frozen.remove(at);
            false
        } else {
            self.frozen.push(column);
            true
        }
    }

    /// 【B15】设置本地筛选词（空 = 不筛）；只改视图行序，不重查
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.rebuild_view_rows();
    }

    /// 【B15】当前筛选词（面板与导出要用同一份真值）
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// 【B15】筛选后的行（导出跟随筛选：原型 §5.5 的口径）
    pub fn visible_rows(&self) -> Vec<Vec<String>> {
        self.view_rows
            .iter()
            .filter_map(|ix| self.rows.get(*ix).cloned())
            .collect()
    }

    /// 【B15】当前排序（数据列下标，是否降序）
    pub fn sort_state(&self) -> Option<(usize, bool)> {
        self.sort
    }

    /// 【B15】应用本地排序（`None` = 回原顺序）；组件库的列头点击最终走到这里
    pub fn apply_sort(&mut self, sort: Option<(usize, bool)>) {
        self.sort = sort;
        self.rebuild_view_rows();
    }

    /// 【B15】重算视图行序（筛选 → 排序；数据不动）
    fn rebuild_view_rows(&mut self) {
        let filter = self.filter.trim().to_lowercase();
        let mut view: Vec<usize> = (0..self.rows.len())
            .filter(|ix| filter.is_empty() || self.row_matches(*ix, &filter))
            .collect();
        if let Some((column, descending)) = self.sort {
            // 稳定排序：同值时按原顺序（用数据行号做 tie-break，与快排的不稳定性无关）
            view.sort_by(|left, right| {
                let ordering = compare_cells(
                    self.rows.get(*left).and_then(|row| row.get(column)),
                    self.rows.get(*right).and_then(|row| row.get(column)),
                );
                let ordering = if descending { ordering.reverse() } else { ordering };
                ordering.then(left.cmp(right))
            });
        }
        self.view_rows = view;
    }

    /// 这一行有没有单元格命中筛选词（大小写不敏感的子串，与 DBeaver 的本地筛选同类）
    fn row_matches(&self, row_ix: usize, needle: &str) -> bool {
        self.rows.get(row_ix).is_some_and(|row| {
            row.iter()
                .any(|cell| cell.to_lowercase().contains(needle))
        })
    }

    /// 【B15】该数据列在表头上的排序标记
    ///
    /// `Default` = 可排但未排（组件库画的是“上下箭头”图标）；当前排序列才给升/降。
    fn sort_mark(&self, index: usize) -> ColumnSort {
        match self.sort {
            Some((column, true)) if column == index => ColumnSort::Descending,
            Some((column, false)) if column == index => ColumnSort::Ascending,
            _ => ColumnSort::Default,
        }
    }

    /// 视图行号 → 数据行号
    fn data_row(&self, row_ix: usize) -> Option<usize> {
        self.view_rows.get(row_ix).copied()
    }

    /// 这份结果还能不能取下一段（面板在结果变化时同步；`true` 才启用滚动到底加载）
    pub fn set_has_more(&mut self, has_more: bool) {
        self.has_more = has_more;
    }

    /// 正在取下一段（回填之前不该重复触发）
    pub fn set_loading_more(&mut self, loading_more: bool) {
        self.loading_more = loading_more;
    }

    /// 注入取下一段的回调（面板构造时一次）
    pub fn set_load_more_hook(&mut self, hook: LoadMoreHook) {
        self.on_load_more = Some(hook);
    }

    /// 能不能现在就取下一段（纯函数：面板与测试都是同一套判据）
    pub fn wants_more(&self) -> bool {
        self.has_more && !self.loading_more && self.on_load_more.is_some()
    }

    /// 测试用：右键菜单的两个钩子接上了吗（接线正确性；真点击在窗口里驱动）
    #[cfg(test)]
    pub fn has_filter_value_hook(&self) -> bool {
        self.on_filter_value.is_some()
    }

    /// 测试用：见上
    #[cfg(test)]
    pub fn has_sort_down_hook(&self) -> bool {
        self.on_sort_down.is_some()
    }

    /// 网格里的列名（供测试断言；不复制行数据）
    pub fn columns_for_test(&self) -> &[String] {
        &self.columns
    }

    /// 网格里的行数（供测试断言）
    pub fn row_count_for_test(&self) -> usize {
        self.rows.len()
    }

    /// 筛选后能看见多少行（视图行数）
    pub fn visible_row_count(&self) -> usize {
        self.view_rows.len()
    }

    /// 这份结果一共抓到多少行（数据行数，与筛选无关）
    pub fn data_row_count(&self) -> usize {
        self.rows.len()
    }

    /// 第 `col_ix` 列是不是行号槽（首列；没有数据时不画它）
    fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 单元格文案：首列是行号，其余按 `col_ix - 1` 取数据
    ///
    /// 行号与数据都走**视图行序**（筛选/排序之后看着的那一行）；
    /// 缺列/缺行给空串而不是 panic（结果行由驱动给出，形状不可全信）。
    fn cell(&self, row_ix: usize, col_ix: usize) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        let Some(data_ix) = self.data_row(row_ix) else {
            return String::new();
        };
        self.rows
            .get(data_ix)
            .and_then(|row| row.get(col_ix - 1))
            .cloned()
            .unwrap_or_default()
    }
}

/// 菜单里的值预览（短、单行；太长的值不该把菜单撑开）
fn preview_of(value: &str) -> String {
    let folded: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() > 24 {
        let mut text: String = folded.chars().take(24).collect();
        text.push('…');
        text
    } else {
        folded
    }
}

/// 两个单元格比大小（本地排序用）
///
/// 两边都像数字就按**数值**比（`1, 2, 10` 不该排成 `1, 10, 2`——展示文本是字符串，
/// 但用户看的是数）；否则按字符串比。`None`（缺列）排在最后。
fn compare_cells(left: Option<&String>, right: Option<&String>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (Some(left), Some(right)) = (left, right) else {
        return match (left.is_some(), right.is_some()) {
            (false, false) => Ordering::Equal,
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (true, true) => Ordering::Equal,
        };
    };
    if let (Ok(left), Ok(right)) = (left.parse::<f64>(), right.parse::<f64>()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    left.cmp(right)
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
        self.view_rows.len()
    }

    /// 【B15】列头点击排序：组件库已在它那边循环 `Default → Descending → Ascending`，
    /// 交到这里的已经是**新值**；本 delegate 只负责记住它并重算视图行序（本地排序，不重查）。
    /// `col_ix` 是含行号槽的列下标，折算成数据列。
    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if Self::is_row_number(col_ix) {
            return;
        }
        let column = col_ix - 1;
        self.apply_sort(match sort {
            ColumnSort::Default => None,
            ColumnSort::Ascending => Some((column, false)),
            ColumnSort::Descending => Some((column, true)),
        });
        // 视图行序变了，重画（表格自己也会 notify，但这里是真值变了）
        cx.notify();
    }

    /// 滚动到底自动加载（B5b）：组件库在可见范围距末尾不足 `load_more_threshold` 行时
    /// 反复调 [`Self::load_more`]，所以真实条件与防重入都在这里判——
    /// “还有没有下一段”是结果的真值（面板同步），不是表格猜的。
    fn has_more(&self, _cx: &App) -> bool {
        self.has_more
    }

    fn load_more(&mut self, _window: &mut Window, cx: &mut Context<TableState<Self>>) {
        if !self.wants_more() {
            return;
        }
        let Some(hook) = self.on_load_more.clone() else {
            return;
        };
        // 乐观置位：组件库会在“接近末尾”的每一帧都调进来，而回调是异步的（下面那行
        // `spawn_in`）——不先拦住的话同一帧里的后续调用会再发一次。真实值由面板回填时同步
        // （面板提交通道上失败会把这一下撤回去）。
        self.loading_more = true;
        //
        // 面板会在提交成功时置 `loading_more`（下一次同步回这里），
        // 所以这里不必自己改状态——避免“面板以为没在取、delegate 以为在取”的两份真值。
        //
        // 回调要**挪出当前更新栈**：面板会回头把 `loading_more` 写回本 delegate，
        // 同步调用就是“更新一个正在更新的实体”→ panic（组件库自己的
        // `load_more_if_need` 也是用 `spawn_in` 把它挪出去的）。
        cx.spawn_in(_window, async move |_view, window| {
            _ = window.update(|_window, cx| hook(cx));
        })
        .detach();
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
            // 【B15】可本地排序：组件库点列头时会在 Default → Descending → Ascending 里循环，
            // 并回调 `perform_sort`（排序状态存在它那边，这里只把当前态回去，refresh 后指示器不丢）
            .sort(self.sort_mark(index))
            // 【B15】冻结列用原生 `fixed_left`（横向滚动钉在左侧；行号槽一直是钉住的）
            .when(self.is_frozen(index), |column| column.fixed_left())
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
        // 【B14】右键菜单要用“哪个单元格”：组件库只公开了 `right_clicked_row`，
        // 没有单元格访问器，所以在单元格自己身上记一笔（事件从最内层派发，不会丢）
        let table = cx.entity().clone();
        div()
            .px_2()
            .truncate()
            .text_xs()
            .text_color(if is_null { muted } else { foreground })
            .when(is_null, |cell| cell.italic())
            .on_mouse_down(MouseButton::Right, move |_, _window, app| {
                table.update(app, |state, _cx| {
                    state.delegate_mut().set_context_cell(Some((row_ix, col_ix)));
                });
            })
            .child(SharedString::from(text))
    }

    /// 【B14】右键菜单：按值筛选 / 复制此值 / 冻结此列（原型 §5.5 的右键入口）
    ///
    /// 目标单元格是**最近右键的那一个**（[`Self::context_cell`]）：没有就什么都不摆
    /// （不猜一个“大概是想筛这个”）。菜单项由 [`context_menu_items`] 给（纯函数，可断言）。
    fn context_menu(
        &mut self,
        _row_ix: usize,
        menu: PopupMenu,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some((row, col)) = self.context_cell else {
            return menu;
        };
        if Self::is_row_number(col) || row >= self.view_rows.len() {
            return menu;
        }
        let column = col - 1;
        let value = self.cell(row, col);
        let column_name = self
            .columns
            .get(column)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", column + 1));

        let mut menu = menu;
        let insight_available = self.on_insight_column.is_some() && self.insight_available;
        for item in context_menu_items(
            &value,
            &column_name,
            column,
            self.is_frozen(column),
            insight_available,
        ) {
            if item.separator_before {
                menu = menu.separator();
            }
            match item.action {
                ContextAction::FilterByValue(needle) => {
                    // 没接钩子就不摆这一项（能力没有就不给入口）
                    if let Some(hook) = self.on_filter_value.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&needle, app),
                        ));
                    }
                }
                ContextAction::SortDown { column, descending } => {
                    // 与「按值筛选」同口径：没接钩子就不摆这一项
                    if let Some(hook) = self.on_sort_down.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&column, descending, app),
                        ));
                    }
                }
                ContextAction::CopyValue(copied) => {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                        },
                    ));
                }
                ContextAction::ToggleFreeze(column) => {
                    // 冻结是**表格自己的事**（改的是 `Column.fixed`），不需要面板：
                    // 点击发生在独立事件里（不在表格的更新栈内），可以安全地更新它并重建列组
                    let table = cx.entity().clone();
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            table.update(app, |state, cx| {
                                state.delegate_mut().toggle_freeze(column);
                                state.refresh(cx);
                            });
                        },
                    ));
                }
                ContextAction::InsightColumn { column } => {
                    // 同「按值筛选」：没接钩子（宿主没接洞察端口）就不摆这一项
                    if let Some(hook) = self.on_insight_column.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&column, app),
                        ));
                    }
                }
            }
        }
        menu
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
    /// 【B15】本地筛选生效时的（视图行数，已抓总行数）——原型 §5.5 要求“统计跟随并明示已筛选”
    pub filtered: Option<(usize, usize)>,
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
        if let Some((visible, total)) = self.filtered {
            parts.push(format!(
                "已筛选 {} / {} 行",
                thousands(visible),
                thousands(total)
            ));
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
    /// 【B15】本地筛选框（原型 §5.5：只作用于视图层，不重查）
    pub filter: Option<AnyElement>,
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
    /// 【B13】结果区顶部那一行提示（切通道后“旧结果来自 X”）；`None` = 不摆
    pub notice: Option<String>,
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
        notice,
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
        // 【B13】切通道后的那一行提示（原型 §5.7 规则 2）：旧结果不删，但要能看出它不是
        // 现在这档跑的——颜色（标签标灰）只是一个通道，白话在这一行
        .children(notice.map(|text| {
            let theme = cx.theme();
            let warning = theme.colors.warning;
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(warning)
                .bg(warning.opacity(0.18))
                .border_b(ui::HAIRLINE)
                .border_color(warning)
                // 测试按选择器断言“切了通道要说一句”
                .debug_selector(|| "editor-result-notice".to_string())
                .child(SharedString::from(text))
        }))
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
                        // 顺序照原型 §2.4 的右段：⌕ 筛选 · … 分析 ▾ · 导出 ▾ · ⟳ · 复制
                        .children(controls.filter)
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
        ContextAction, ResultGridDelegate, ResultStatus, ResultToolbar, compare_cells,
        context_menu_items, duration_text, preview_of, status_segments, thousands, truncated_hint,
    };
    use gpui_kit::App;
    use gpui_kit::component::table::{ColumnSort, TableDelegate as _};

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

    /// 造一份指定行的网格（B15 的筛选/排序测试用：列固定为 id / name）
    fn grid_with(rows: &[&[&str]]) -> ResultGridDelegate {
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            rows.iter()
                .map(|row| row.iter().map(|cell| cell.to_string()).collect())
                .collect(),
        );
        grid
    }

    /// 【B14】右键菜单的项：按值筛选 / 复制此值 / 冻结（文案随冻结状态变）
    #[test]
    fn context_menu_offers_filter_copy_and_freeze() {
        let items = context_menu_items("orders", "name", 1, false, false);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "按值筛选「orders」",
                "复制此值",
                "按「name」升序（下发源库）",
                "按「name」降序（下发源库）",
                "冻结「name」",
            ]
        );
        assert_eq!(
            items[0].action,
            ContextAction::FilterByValue("orders".to_string()),
            "值原样给面板（预览只管显示）"
        );
        assert_eq!(items[1].action, ContextAction::CopyValue("orders".to_string()));
        assert_eq!(
            items[2].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: false
            }
        );
        assert_eq!(
            items[3].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: true
            }
        );
        assert!(
            items[2].separator_before,
            "“对整份结果的操作”那组前面要有分隔线"
        );
        assert!(!items[4].separator_before, "冻结与排序同组");
        assert_eq!(items[4].action, ContextAction::ToggleFreeze(1));

        let frozen = context_menu_items("orders", "name", 1, true, false);
        assert_eq!(frozen[4].label, "取消冻结此列", "已冻结时给的是反向动作");
    }

    /// 【M8】「洞察此列」只在**能洞察**时出现（宿主接了端口 + 这份结果可取样）：
    /// 不能洞察时连项都不摆（能力没有就不给入口，与「按值筛选」同口径）。
    #[test]
    fn context_menu_only_offers_insight_when_available() {
        let without = context_menu_items("orders", "name", 1, false, false);
        assert!(
            !without
                .iter()
                .any(|item| matches!(item.action, ContextAction::InsightColumn { .. })),
            "不能洞察时不该摆这一项：{without:?}"
        );

        let with = context_menu_items("orders", "name", 1, false, true);
        let last = with.last().expect("至少有一项");
        assert_eq!(last.label, "洞察「name」这一列");
        assert_eq!(
            last.action,
            ContextAction::InsightColumn {
                column: "name".to_string()
            }
        );
        assert!(last.separator_before, "它自成一段，前面要有分隔线");
    }

    /// 值预览：空白折叠 + 截断（长值不该把菜单撑开）
    #[test]
    fn the_value_preview_folds_and_truncates() {
        assert_eq!(preview_of("a\nb  c"), "a b c");
        let long = "x".repeat(40);
        let preview = preview_of(&long);
        assert_eq!(preview.chars().count(), 25, "24 个字符 + 省略号");
        assert!(preview.ends_with('…'), "{preview}");
        let items = context_menu_items(&long, "c", 0, false, false);
        assert!(items[0].label.contains('…'), "{}", items[0].label);
    }

    /// 【B15】冻结列：切换、换数据时清空（列都换了一批，旧列号没意义）
    #[test]
    fn freezing_columns_toggles_and_resets_with_data() {
        let mut grid = grid();
        assert!(!grid.is_frozen(0));
        assert!(grid.toggle_freeze(0), "第一次是冻结");
        assert!(grid.is_frozen(0));
        assert!(grid.toggle_freeze(1));
        assert!(!grid.toggle_freeze(0), "再切一次是取消");
        assert!(!grid.is_frozen(0) && grid.is_frozen(1));

        grid.set_data(
            vec!["a".to_string()],
            vec![vec!["1".to_string()]],
        );
        assert!(!grid.is_frozen(1), "换了一份结果：冻结不跟过来");
    }

    /// 【B15】筛选只改视图行序：命中那些行还在原位，行号跟的是视图
    #[test]
    fn filtering_keeps_matching_rows_in_place() {
        let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
        grid.set_filter("orders");
        assert_eq!(grid.visible_row_count(), 2, "命中两行");
        assert_eq!(grid.data_row_count(), 3, "数据一行没动（清掉筛选就能原样回来）");
        assert_eq!(grid.cell(0, 1), "1", "首列显示的是视图行号");
        assert_eq!(grid.cell(0, 2), "orders");
        assert_eq!(grid.cell(1, 2), "orders_archive");
        assert_eq!(grid.cell(2, 2), "", "越界的视图行给空串（不 panic）");

        // 大小写不敏感（与 DBeaver 的本地筛选同类）
        grid.set_filter("ORDERS");
        assert_eq!(grid.visible_row_count(), 2);

        // 清除筛选：全部回来，顺序不变
        grid.set_filter("");
        assert_eq!(grid.visible_row_count(), 3);
        assert_eq!(grid.visible_rows()[0][1], "orders");
        assert_eq!(grid.visible_rows()[2][1], "orders_archive");
    }

    /// 【B15】排序：数字按数值比（`1, 2, 10` 不该排成 `1, 10, 2`），字符串按字典序
    #[test]
    fn sorting_is_numeric_when_the_values_look_numeric() {
        assert_eq!(
            compare_cells(Some(&"2".to_string()), Some(&"10".to_string())),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_cells(Some(&"b".to_string()), Some(&"a".to_string())),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_cells(None, Some(&"1".to_string())),
            std::cmp::Ordering::Greater,
            "缺列排在最后"
        );

        let mut grid = grid_with(&[&["10", "b"], &["2", "c"], &["1", "a"]]);
        grid.apply_sort(Some((0, false)));
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["1", "2", "10"],
            "升序按数值"
        );
        grid.apply_sort(Some((1, true)));
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[1].clone())
                .collect::<Vec<_>>(),
            ["c", "b", "a"],
            "降序按字符串"
        );
        grid.apply_sort(None);
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["10", "2", "1"],
            "取消排序回原顺序（不是部分退还）"
        );
    }

    /// 【B15】筛选与排序叠在一起：先筛后排，两者都只动视图
    #[test]
    fn filter_and_sort_stack_on_the_same_view() {
        let mut grid = grid_with(&[&["10", "orders"], &["2", "orders"], &["7", "users"]]);
        grid.set_filter("orders");
        grid.apply_sort(Some((0, false)));
        assert_eq!(grid.visible_row_count(), 2);
        assert_eq!(grid.visible_rows()[0][0], "2");
        assert_eq!(grid.visible_rows()[1][0], "10");
    }

    /// 【B15】换一份结果：排序跟不过来（列都可能不是同一批），筛选词留着（用户还在找同样的东西）
    #[test]
    fn switching_data_clears_sort_but_keeps_the_filter() {
        let mut grid = grid_with(&[&["2", "orders"], &["1", "users"]]);
        grid.set_filter("orders");
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.sort_state(), Some((0, true)));

        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![vec!["5".to_string(), "orders".to_string()]],
        );
        assert_eq!(grid.sort_state(), None, "换结果不该继承旧的排序");
        assert_eq!(grid.filter(), "orders", "筛选词留着");
        assert_eq!(grid.visible_row_count(), 1);
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

    /// 【B15】列头可排序：数据列给 `Some(Default)`（可排但未排），行号槽不给
    ///
    /// 组件库对 `sort: None` 的列**不响应点击**（`perform_sort` 里直接 return），
    /// 所以这条钉的是“列头真的能点下去”。
    #[gpui_kit::test]
    fn data_columns_are_sortable_from_the_header(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(0, cx).sort, None, "行号槽不是数据列");
            assert_eq!(grid.column(1, cx).sort, Some(ColumnSort::Default));
            assert_eq!(grid.column(2, cx).sort, Some(ColumnSort::Default));
        });
        // 排上以后指示器要回得去（组件库 refresh 后会重新问 column）
        grid.apply_sort(Some((0, false)));
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).sort, Some(ColumnSort::Ascending));
            assert_eq!(grid.column(2, cx).sort, Some(ColumnSort::Default));
        });
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
            ..Default::default()
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
            ..Default::default()
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
