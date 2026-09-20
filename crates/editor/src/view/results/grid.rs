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
//! 单元格与表头都**截断**（结果里的长文本 / 长 JSON / 长列名比列宽宽是常态），截掉的部分
//! 靠**悬停全文**看（自己画 `render_th` / `render_td` 就是为了挂它）。右键菜单除了按值筛选
//! 与冻结，还给三种复制：此值 · 此列 · 整行（TSV，走 [`::shared::string::tsv_cell`] 同一份转义）。
//!
//! 原型 §2.4 的工具栏还包含**筛选 · 下发开关 · 分析 · 导出**——它们各自属 B15 / B14 / B7，
//! **没实现就不摆按钮**；分页/取下一段属 B5b，位置留在 ⑦ 那一行。
//!
//! 数据源是 [`crate::store::ResultStore`]（结果唯一权威）。网格**不持有真值**——
//! delegate 里的行是从权威那里拷来的投影，`set_data` 是唯一的写入点。
//!
//! ## 类型感知渲染（本切片）
//!
//! 列类型（[`crate::store::ColumnKind`]）只影响**怎么画**：数字右对齐、布尔 / 时间族用等宽字体
//! （主题的 `mono_font_family`，不引新依赖）、表头在列名右侧挂一个类型小标签（次级色）。
//! **类型缺失时一个像素都不变**：对齐、字体、表头都回到今天的样子——驱动填上列类型之前，
//! 所有真实结果走的都是这一档。
//!
//! ## 按需取值（本切片）
//!
//! 网格不预计算任何展示文本：组件的虚拟滚动只问可见窗口，delegate 就只回答那一格的取值。
//! 于是三条本来会按**整表**算的事都推到了真要的时候：
//!
//! - 视图行序：没有筛选也没有排序时**不物化**那份映射（`view_rows = None` 即恒等）；
//! - 筛选匹配：两边都是 ASCII 时零分配（回落 `to_lowercase` 只是为了保住口径）；
//! - 类型解析：入库时**每列一次**，不是每格一次。
//!
//! 需要**整表真值**的出口仍然是整表，但它们都是用户动作触发的（复制整列 / 整行 · 导出 ·
//! 筛选 · 排序），不是入库时预支的。

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// TSV 转义（剪贴板 / 导出的唯一实现，与 mock 预览表共用）：本 crate 根部有同名模块
// `crate::shared`，所以 `shared` 得从 crate 外路径（`::`）进来
use ::shared::string::{tsv_cell, tsv_row};

use crate::store::{ColumnKind, ColumnType};
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
    /// 复制整列（**数据列**下标）：当前视图行序里这一列的全部取值，一行一个
    /// （与 mock 预览「复制此列」同一口径：只给已抓到的行，不重查源库）
    CopyColumn(usize),
    /// 复制整行（**视图行**下标）：整行拼成 TSV（列之间是制表符，粘进表格就能分列）
    CopyRow(usize),
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

/// 右键落点：菜单项要回答的「这是哪一格、这一列的处境」（[`context_menu_items`] 的输入）
///
/// 打包成一个结构体而不是递一串位置参数：`column` / `row` / `rows` 挨在一起时
/// 很容易传反（三个都是数字），而传反了菜单看上去照样正常。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTarget {
    /// 目标单元格的取值（原样交给「按值筛选」与「复制此值」，预览只管显示）
    pub value: String,
    /// 目标单元格所在数据列的列名
    pub column_name: String,
    /// 数据列下标（含行号槽的列下标已折算掉）
    pub column: usize,
    /// 目标单元格所在**视图行**（复制整行用它）
    pub row: usize,
    /// 这一列在视图里的取值个数（复制此列的文案要写明会拷走多少）
    pub rows: usize,
    /// 这一列冻结了吗（文案随它反向）
    pub frozen: bool,
    /// 【M8】这份结果能不能洞察（宿主接了端口 + 这份结果可取样）
    pub insight: bool,
}

/// 右键菜单的项（**纯函数**：有什么、叫什么都在这里定，界面只负责画）
///
/// 原型 §5.5 的右键入口：按值筛选（写进筛选框、默认本地）· 复制（此值 / 此列 / 整行）·
/// 冻结 / 取消冻结（原生 `Column.fixed`）·【M8】洞察此列。
///
/// `target.insight` = 【M8】这份结果能不能洞察（宿主接了端口 + 这份结果可取样，见
/// `ResultEntry::can_insight_column`）——**能力没有就不摆入口**。
///
/// 文案与 mock 预览表的右键菜单同一套：两张表是同一类东西，同一种动作不该两个叫法。
pub fn context_menu_items(target: &ContextTarget) -> Vec<ContextMenuItem> {
    let value = target.value.as_str();
    let column_name = target.column_name.as_str();
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
            // 个数写明：这一列拷走多少得让用户心里有数（与 mock 预览同一句文案）
            label: format!("复制此列（{} 个取值）", target.rows),
            action: ContextAction::CopyColumn(target.column),
            separator_before: false,
        },
        ContextMenuItem {
            label: "复制整行（TSV）".to_string(),
            action: ContextAction::CopyRow(target.row),
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
            label: if target.frozen {
                "取消冻结此列".to_string()
            } else {
                format!("冻结「{column_name}」")
            },
            action: ContextAction::ToggleFreeze(target.column),
            separator_before: false,
        },
    ];
    if target.insight {
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
    /// **每列的类型**（与 `columns` 同序；空 = 这份结果没带类型）
    ///
    /// 只影响怎么画（右对齐 / 等宽 / 表头小标签），不影响任何取值；没带类型时与今天完全一致。
    /// 由面板在 `set_data` **之后**同一次同步里给（见 [`ResultGridDelegate::set_column_types`]）。
    column_types: Vec<Option<ColumnType>>,
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
    /// `None` = **与数据行序相同**（既没筛选也没排序）：这时不物化那份映射——
    /// 10 万行的结果切一次结果集就为它分配一个 10 万元素的表，而绝大多数时候视图
    /// 与数据是同一个顺序，那份表一次也用不上（入库 / 切换结果集时不该做这件事）。
    ///
    /// 筛选与排序都只改这个映射，**数据行一行不动**（所以清除筛选能原样恢复，
    /// 也不会把“已抓到的窗口”搞乱）。行号列显示的是**视图行号**，不是数据行号。
    view_rows: Option<Vec<usize>>,
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
        // 类型是**这一份结果自己的**：先把上一份的清掉，等面板紧跟着给新的一份
        // （漏给就是“没类型”，不会错套到新数据上）
        self.column_types.clear();
        self.rebuild_view_rows();
    }

    /// 同步每列的类型（**与 `set_data` 同一次同步里、在它之后调**）
    ///
    /// 为什么是单独一次而不是 `set_data` 加参数：`set_data` 的签名上挂着面板的现有调用，
    /// 而类型今天还只能从另一条路来（上游填上驱动类型之前它一直是空）。
    /// 顺序反了（先给类型再 `set_data`）会被清掉——**不会**把上一份的类型套到新数据上。
    pub fn set_column_types(&mut self, types: Vec<Option<ColumnType>>) {
        self.column_types = types;
    }

    /// 清空（切到无结果的模式 / 关闭文档时）
    pub fn clear(&mut self, text: impl Into<String>) {
        self.columns.clear();
        self.rows.clear();
        self.column_types.clear();
        self.view_rows = None;
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
        (0..self.view_len())
            .filter_map(|view_ix| self.data_row(view_ix))
            .filter_map(|data_ix| self.rows.get(data_ix).cloned())
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
        // 既没筛选也没排序：视图行序就是数据行序——**不物化**那份映射（`None` 即恒等）。
        // 入库 / 切结果集时这一步占了整一轮逐行工作，而绝大多数时候根本用不上。
        if filter.is_empty() && self.sort.is_none() {
            self.view_rows = None;
            return;
        }
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
        self.view_rows = Some(view);
    }

    /// 这一行有没有单元格命中筛选词（大小写不敏感的子串，与 DBeaver 的本地筛选同类）
    fn row_matches(&self, row_ix: usize, needle: &str) -> bool {
        self.rows
            .get(row_ix)
            .is_some_and(|row| row.iter().any(|cell| cell_matches(cell, needle)))
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

    /// 视图行号 → 数据行号（`None` = 没有这一行）
    ///
    /// 视图行序没物化时（既没筛选也没排序）就是**恒等**：越界要按数据行数判，
    /// 不能直接把行号当数据行号返回。
    fn data_row(&self, row_ix: usize) -> Option<usize> {
        match &self.view_rows {
            Some(view) => view.get(row_ix).copied(),
            None => (row_ix < self.rows.len()).then_some(row_ix),
        }
    }

    /// 视图行数（没筛选/排序时就是数据行数）
    fn view_len(&self) -> usize {
        self.view_rows.as_ref().map_or(self.rows.len(), Vec::len)
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

    /// 测试用：视图行序现在是**物化**的吗（`false` = 恒等，入库 / 切结果集时那份映射被省掉了）
    #[cfg(test)]
    pub fn view_order_materialized_for_test(&self) -> bool {
        self.view_rows.is_some()
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
        self.view_len()
    }

    /// 这份结果一共抓到多少行（数据行数，与筛选无关）
    pub fn data_row_count(&self) -> usize {
        self.rows.len()
    }

    /// 第 `col_ix` 列是不是行号槽（首列；没有数据时不画它）
    fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 第 `col_ix` 列（**含行号槽**）的语义类型：行号槽与没带类型的列都是 `Unknown`
    ///
    /// 渲染只读这一条（对齐 / 等宽），每格 O(1)：类型在 `set_column_types` 时就已经解析好了
    /// （每列一次，不是每格一次）。
    fn column_kind(&self, col_ix: usize) -> ColumnKind {
        if Self::is_row_number(col_ix) {
            return ColumnKind::Unknown;
        }
        self.column_types
            .get(col_ix - 1)
            .and_then(Option::as_ref)
            .map_or(ColumnKind::Unknown, |column| column.kind)
    }

    /// 表头的类型小标签：驱动报的**原始名字**（行号槽 / 没带类型 = `None`，那就一个字也不摆）
    fn type_label(&self, col_ix: usize) -> Option<&str> {
        if Self::is_row_number(col_ix) {
            return None;
        }
        self.column_types
            .get(col_ix - 1)
            .and_then(Option::as_ref)
            .map(|column| column.name.as_str())
    }

    /// 单元格文案：首列是行号，其余按 `col_ix - 1` 取数据
    ///
    /// 行号与数据都走**视图行序**（筛选/排序之后看着的那一行）；
    /// 缺列/缺行给空串而不是 panic（结果行由驱动给出，形状不可全信）。
    fn cell(&self, row_ix: usize, col_ix: usize) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        self.row_cell_text(row_ix, col_ix)
            .unwrap_or_default()
            .to_string()
    }

    /// 这一格的文本（**只借不拷**；`#` 行号槽与不存在的格都是 `None`）
    ///
    /// “这一格是什么”只此一份：`cell()` 画它、结果内查找搜它、将来类型化导出也读它。
    /// 行号槽不在这里（它不是一个数据格：显示的是视图行号，而视图行号由渲染现算）。
    fn row_cell_text(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        if Self::is_row_number(col_ix) {
            return None;
        }
        let data_ix = self.data_row(row_ix)?;
        self.rows.get(data_ix)?.get(col_ix - 1).map(String::as_str)
    }

    /// 结果内查找：从 `after` **之后**找下一处命中，找完一圈回到开头（`None` = 整份结果里没有）
    ///
    /// 口径与本地筛选**同一个实现**（[`find_in_cell`]）：大小写不敏感的子串；只看**看得见的行**
    /// （筛选 / 排序之后的视图行序——找到看不见的行等于没找到）；`#` 行号槽不是数据，不参与。
    ///
    /// `after = None` = 从第一格开始。区间只在与词**都是 ASCII** 时给出（折叠不改字节长度）；
    /// 含非 ASCII 时是 `None`，宿主退化成**整格**高亮（`İ` → 两个字符，折叠后的字节偏移在原文里
    /// 不是合法边界，格内高亮的区间映射要单独设计，不在本切片）。
    ///
    /// 为什么是纯读：查找**不改任何状态**——选中哪一格、要不要滚过去、要不要画高亮都是宿主的
    /// 决定（宿主才知道焦点与滚动句柄）。这里只回答“下一处在哪”。
    pub fn find_next(&self, needle: &str, after: Option<(usize, usize)>) -> Option<ResultMatch> {
        let needle = needle.trim().to_lowercase();
        let columns = self.columns.len();
        let rows = self.view_len();
        if needle.is_empty() || columns == 0 || rows == 0 {
            return None;
        }
        // 把（视图行, 列）当成一条带子上的格子：搜一圈就是一条不大不小的扫描
        let per_row = columns + 1;
        let cells = rows * per_row;
        let start = match after {
            Some((row, col)) if col < per_row => {
                row.saturating_mul(per_row).saturating_add(col + 1) % cells
            }
            // 没给过起点，或起点已经不在这份结果里（刷新 / 换结果之后）：从头找
            _ => 0,
        };
        for step in 0..cells {
            let flat = (start + step) % cells;
            let (row, col) = (flat / per_row, flat % per_row);
            let Some(text) = self.row_cell_text(row, col) else {
                continue;
            };
            if let Some(range) = find_in_cell(text, &needle) {
                return Some(ResultMatch { row, col, range });
            }
        }
        None
    }

    /// 这一数据列在**当前视图行序**下的全部取值（一行一个，换行分隔；逐格按 TSV 转义）
    ///
    /// 只给**已抓到的行**、不重查源库（与 mock 预览「复制此列」同一口径）：
    /// 一份十万行的结果整列就是十万个值，粘到哪里都不好用——真要全列就导出。
    fn column_text(&self, column: usize) -> String {
        let mut text = String::new();
        for (index, data_ix) in (0..self.view_len())
            .filter_map(|view_ix| self.data_row(view_ix))
            .enumerate()
        {
            if index > 0 {
                text.push('\n');
            }
            let cell = self
                .rows
                .get(data_ix)
                .and_then(|row| row.get(column))
                .map(String::as_str)
                .map(tsv_cell)
                .unwrap_or_default();
            text.push_str(&cell);
        }
        text
    }

    /// 这一行拼成 TSV（**视图行序**：复制的是用户看着的那一行，不是数据行序）
    fn row_text(&self, row_ix: usize) -> String {
        let Some(data_ix) = self.data_row(row_ix) else {
            return String::new();
        };
        self.rows
            .get(data_ix)
            .map(|row| tsv_row(row))
            .unwrap_or_default()
    }

    /// 【B14】选中内容的复制文本（`Ctrl+C` 用）：一格给**原文**，一整行给 TSV
    ///
    /// 与右键「复制此值 / 复制整行（TSV）」同一套口径与同一份实现（本方法就是它俩的入口）：
    /// 单格不转义（要的就是那一格的值），整行才拼 TSV。
    /// `None` = 没东西可复制（选择指向的行已经不在视图里了——筛选 / 刷新之后可能如此）。
    pub fn selection_text(&self, selection: GridSelection) -> Option<String> {
        let row = match selection {
            GridSelection::Cell { row, .. } | GridSelection::Row { row } => row,
        };
        // 行不在了就说清“没东西可复制”，而不是安静地拷一个空串（用户会粘出一片空白）
        if self.data_row(row).is_none() {
            return None;
        }
        Some(match selection {
            GridSelection::Cell { row, col } => self.cell(row, col),
            GridSelection::Row { row } => self.row_text(row),
        })
    }
}

/// 单元格 / 表头的悬停全文：取值与列名都可能很宽，`truncate` 之后只剩这一条看全的路。
///
/// 宽度走 `ui::RESULT_TOOLTIP_MAX_WIDTH`（结构尺寸只认常量表），并让它**折行**：
/// 悬停提示跟着鼠标，横向越宽越容易被窗口边缘截掉。
fn result_cell_tooltip(text: String, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        div()
            .max_w(rems(ui::RESULT_TOOLTIP_MAX_WIDTH))
            .whitespace_normal()
            .text_xs()
            .child(text.clone())
    })
    .build(window, cx)
}

/// 【B14】网格里**最近一次选中**（`Ctrl+C` 复制谁）
///
/// 为什么由面板记事件而不是读组件状态：组件没公开“当前是选格还是选行”，而
/// `selected_cell()` / `selected_row()` 可以**同时有值**（选过整行再点一个格子）——
/// 猜哪个更新会猜错，所以只认最近一次选择事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSelection {
    /// 选中一格（`col` 是含行号槽的列下标）
    Cell { row: usize, col: usize },
    /// 选中一整行（**视图行序**）
    Row { row: usize },
}

/// 结果内查找的一个命中（[`ResultGridDelegate::find_next`] 的产物）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultMatch {
    /// **视图行号**（筛选 / 排序之后看着的那一行）
    pub row: usize,
    /// 含行号槽的列下标
    pub col: usize,
    /// 格内区间（字节；`None` = 命中但给不出区间，见 [`ResultGridDelegate::find_next`]）
    pub range: Option<Range<usize>>,
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

/// 一格的渲染决策（**纯函数**产出就是这一份；渲染只负责把它落到元素上）
///
/// 为什么单独拿出来：`Unknown` 这一档必须是**今天的样式**，而“没带类型就一个像素不变”
/// 这句话只有被断言过才算数——渲染函数里测不到（gpui 不暴露算出来的对齐与字体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    /// 文本对齐（数字右对齐：个位对齐才看得出一列的数量级差）
    align: TextAlign,
    /// 用等宽字体吗（布尔 / 时间族；字体族从主题取，不引新依赖）
    monospace: bool,
}

/// 列类型 → 这一列所有格子的样式
fn cell_style(kind: ColumnKind) -> CellStyle {
    CellStyle {
        align: if kind.is_numeric() {
            TextAlign::Right
        } else {
            TextAlign::Left
        },
        monospace: kind.is_monospace(),
    }
}

/// 单元格里找 `needle`（**已小写**）的字节区间：两边都是 ASCII → 零分配
fn ascii_match_range(cell: &str, needle: &str) -> Option<Range<usize>> {
    let (haystack, needle) = (cell.as_bytes(), needle.as_bytes());
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|at| at..at + needle.len())
}

/// 格内命中的唯一实现：本地筛选与结果内查找都走它（一份口径，不许两套）
///
/// - `Some(Some(range))`：命中，且给得出**格内区间**（格与词都是 ASCII —— 折叠不改字节长度）；
/// - `Some(None)`：命中，但给不出区间（含非 ASCII）；
/// - `None`：没命中。
///
/// 为什么要单独一条 ASCII 快路径：筛选框每敲一下都要把**整表**过一遍，原先每格
/// `to_lowercase()` 一次分配（10 万行 × 20 列 = 200 万次），而结果里绝大多数格子是 ASCII。
/// 含非 ASCII 时回落 `to_lowercase`：`str::to_lowercase` 有上下文相关的特例（如希腊文末位
/// sigma），自写的逐字节比较会与它不一致——**口径不能有两种**，宁可慢一点。
fn find_in_cell(cell: &str, needle: &str) -> Option<Option<Range<usize>>> {
    if needle.is_empty() {
        return None;
    }
    if cell.is_ascii() && needle.is_ascii() {
        return ascii_match_range(cell, needle).map(Some);
    }
    cell.to_lowercase().contains(needle).then_some(None)
}

/// 单元格命中判据（筛选用：只看有没有）
fn cell_matches(cell: &str, needle: &str) -> bool {
    find_in_cell(cell, needle).is_some()
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
        self.view_len()
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

    /// 表头单元格：组件默认只画列名（`div().size_full().child(name)`），这里补三件事——
    /// **截断**、**悬停全文**、**列类型小标签**。列名常常比默认列宽（128px）长，截断之后
    /// 就没有第二条知道它是什么的路（单元格取值有悬停，表头同样需要）。
    ///
    /// 类型小标签摆在列名**右侧**并且 `flex_none`：先截断的总是列名——反过来的话，
    /// 窄列上被省略号吃掉的恰好是“类型”（那正是想看它的时候）。
    ///
    /// 行号槽（`#`）不挂悬停也不挂标签——它不是数据。`debug_selector` 是给用例找这个表头用的
    /// （`.id(...)` 不登记坐标）：表头的排序箭头就在它右端，直调 `perform_sort` 验不到那段几何。
    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = if Self::is_row_number(col_ix) {
            "#".to_string()
        } else {
            self.columns.get(col_ix - 1).cloned().unwrap_or_default()
        };
        // 类型缺失时 `label` 是 `None`：这一档与今天一模一样（表头就一个列名）
        let label = self.type_label(col_ix).map(str::to_string);
        let muted = cx.theme().colors.muted_foreground;

        let head = div()
            .h_flex()
            .items_center()
            .gap_1()
            .size_full()
            .min_w_0()
            .truncate()
            .debug_selector(move || format!("editor-result-th-{col_ix}"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(name.clone())),
            )
            .when_some(label.clone(), |head, label| {
                head.child(
                    div()
                        .flex_none()
                        .text_size(rems(ui::RESULT_HEADER_TYPE_FONT_SIZE))
                        .text_color(muted)
                        .child(SharedString::from(label)),
                )
            });
        if Self::is_row_number(col_ix) || name.is_empty() {
            return head.into_any_element();
        }
        // 悬停给全名 + 类型：类型在窄列上也可能被截掉（它带参数时很长，`numeric(38,10)`）
        let full = match label {
            Some(label) => format!("{name} · {label}"),
            None => name,
        };
        head.id(("editor-result-th", col_ix))
            .tooltip(move |window, cx| result_cell_tooltip(full.clone(), window, cx))
            .into_any_element()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self.cell(row_ix, col_ix);
        // 类型决定“怎么画”（数字右对齐、布尔 / 时间族等宽）；没带类型就是 `Unknown`
        // ——那一档与今天完全一致
        let style = cell_style(self.column_kind(col_ix));
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let foreground = theme.colors.foreground;
        // 字体族从主题取（不引新依赖）：只在真要等宽的那几类上取一次
        let mono_family = style.monospace.then(|| theme.mono_font_family.clone());

        // 行号槽永远是灰的（它不是数据）
        if Self::is_row_number(col_ix) {
            return div()
                .px_2()
                .h_full()
                .truncate()
                .text_xs()
                .text_color(muted)
                // 用例按它量行高（`.id()` 不登记坐标）：这是「表格密度」那条口径的回归哨兵
                .debug_selector(move || format!("editor-result-rowno-{row_ix}"))
                .child(SharedString::from(text))
                .into_any_element();
        }

        // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（原型 §2.4）
        let is_null = text == "NULL";
        // 【B14】右键菜单要用“哪个单元格”：组件库只公开了 `right_clicked_row`，
        // 没有单元格访问器，所以在单元格自己身上记一笔（事件从最内层派发，不会丢）
        let table = cx.entity().clone();
        let mut cell = div()
            // 悬停提示要求元素有 id：gpui 的**流式** `tooltip` 声明在
            // `StatefulInteractiveElement` 上（没 id 的裸 `div` 只有直调 `Interactivity` 的写法），
            // 组件库自己的表头 / 行头单元格也是这么挂的
            .id((
                "editor-result-cell",
                row_ix * (self.columns.len() + 1) + col_ix,
            ))
            .px_2()
            .truncate()
            .text_xs()
            .text_color(if is_null { muted } else { foreground })
            // 【类型感知】数字右对齐；其余左对齐
            .text_align(style.align)
            // 【类型感知】布尔 / 时间族用主题的等宽字体（两列放一起时字形对得齐）
            .when_some(mono_family, |cell, family| cell.font_family(family))
            .when(is_null, |cell| cell.italic())
            // 用例按它点/量真单元格（`Ctrl+C` 复制选中、行距口径都从这里进去）
            .debug_selector(move || format!("editor-result-cell-{row_ix}-{col_ix}"))
            .on_mouse_down(MouseButton::Right, move |_, _window, app| {
                table.update(app, |state, _cx| {
                    state.delegate_mut().set_context_cell(Some((row_ix, col_ix)));
                });
            });
        // 取值可能很宽（JSON / 长文本），`truncate` 之后就只剩悬停这一条看全的路
        if !text.is_empty() {
            let full = text.clone();
            cell = cell.tooltip(move |window, cx| result_cell_tooltip(full.clone(), window, cx));
        }
        cell.child(SharedString::from(text)).into_any_element()
    }

    /// 【B14】右键菜单：按值筛选 / 复制（此值 / 此列 / 整行）/ 冻结此列（原型 §5.5 的右键入口）
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
        if Self::is_row_number(col) || row >= self.view_len() {
            return menu;
        }
        let column = col - 1;
        let column_name = self
            .columns
            .get(column)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", column + 1));

        let mut menu = menu;
        let insight_available = self.on_insight_column.is_some() && self.insight_available;
        let target = ContextTarget {
            value: self.cell(row, col),
            column_name,
            column,
            row,
            rows: self.view_len(),
            frozen: self.is_frozen(column),
            insight: insight_available,
        };
        for item in context_menu_items(&target) {
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
                ContextAction::CopyColumn(column) => {
                    // 文案先拼好再交给点击回调：菜单项活到的比这一次 `context_menu` 调用长，
                    // 而“当时那一列是什么”只有现在知道。取值走**视图行序**（看着的才是拷走的）
                    let text = self.column_text(column);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyRow(row) => {
                    // 同上：拼好的 TSV 带上（整行 = 各列制表符分隔，粘进表格直接分列）
                    let text = self.row_text(row);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
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
    /// 【B15】来源摘要（血缘）：`原查询` / `下发筛选` / `排序下发` / `取下一段` / 标题（执行计划 / 分析）
    pub lineage: Option<String>,
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
        // 【B15】血缘（原型 §2.4：结果集要带来源摘要）——排在连接之前：
        // “这份是怎么来的”比“从哪条连接来”更常被问
        if let Some(lineage) = &self.lineage {
            parts.push(lineage.clone());
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
    /// 【B15】本地分析（下拉：计数 / 逐列分组计数；数据是桥接过来的行）
    pub analysis: Option<AnyElement>,
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
            // 组件自带的窄行头去掉（`cell_selectable` 下默认会画一条）：我们的 `#` 列
            // 就是行把手，两条并列各能点一半是纯粹的干扰。关掉后仍能选整行——
            // **再点一下已选中的格子**升级为整行（组件自己的口径，`row_selectable` 已开）
            .row_header(false)
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
                        // 顺序照原型 §2.4 的右段：⌕ 筛选 · ⚗ 分析 ▾ · ⤓ 导出 ▾ · ⟳ · 复制
                        .children(controls.filter)
                        .children(controls.analysis)
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
                    // 【B14】`Ctrl+C` 的上下文只挂这一层（键位在 `crates/app` 注册）：
                    // 焦点在网格里时它才在 dispatch path 上，编辑区里按 `Ctrl+C`
                    // 仍是内核的文本复制。焦点本身不用我们管——组件的 `DataTable`
                    // 自己 `track_focus`，点一下就在它身上了（用例从真点击一路验到剪贴板）
                    .key_context(crate::commands::RESULT_GRID_CONTEXT)
                    // 密度档走常量表（`RESULT_TABLE_SIZE` = 组件 XSmall = 26px）——
                    // 不写 size 就落到组件默认档 32px（原型稿写的是 22px，三个数都不一致，见常量注释）
                    .child(
                        DataTable::new(state)
                            .stripe(true)
                            .bordered(false)
                            .with_size(ui::RESULT_TABLE_SIZE),
                    ),
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
        ContextAction, ContextTarget, ResultGridDelegate, ResultMatch, ResultStatus, ResultToolbar,
        TextAlign, cell_matches, cell_style, compare_cells, context_menu_items, duration_text,
        find_in_cell, preview_of, status_segments, thousands, truncated_hint,
    };
    use crate::store::{ColumnKind, ColumnType};
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

    /// 右键落点（用例只改关心的那几项）
    fn target(value: &str, name: &str, column: usize) -> ContextTarget {
        ContextTarget {
            value: value.to_string(),
            column_name: name.to_string(),
            column,
            row: 0,
            rows: 2,
            frozen: false,
            insight: false,
        }
    }

    /// 【B14】右键菜单的项：按值筛选 / 复制（此值 · 此列 · 整行）/ 冻结（文案随冻结状态变）
    #[test]
    fn context_menu_offers_filter_copy_and_freeze() {
        let target = target("orders", "name", 1);
        let items = context_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "按值筛选「orders」",
                "复制此值",
                "复制此列（2 个取值）",
                "复制整行（TSV）",
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
            ContextAction::CopyColumn(1),
            "复制整列认的是**数据列**下标（1 列、行号槽已折算掉）"
        );
        assert_eq!(
            items[3].action,
            ContextAction::CopyRow(0),
            "复制整行认的是**视图行**下标，不是列下标（两个都是数字，传反了菜单照样能摆出来）"
        );
        assert_eq!(
            items[4].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: false
            }
        );
        assert_eq!(
            items[5].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: true
            }
        );
        assert!(
            items[4].separator_before,
            "“对整份结果的操作”那组前面要有分隔线"
        );
        assert!(!items[6].separator_before, "冻结与排序同组");
        assert_eq!(items[6].action, ContextAction::ToggleFreeze(1));

        let mut done = target;
        done.frozen = true;
        let items = context_menu_items(&done);
        assert_eq!(items[6].label, "取消冻结此列", "已冻结时给的是反向动作");
    }

    /// 【M8】「洞察此列」只在**能洞察**时出现（宿主接了端口 + 这份结果可取样）：
    /// 不能洞察时连项都不摆（能力没有就不给入口，与「按值筛选」同口径）。
    #[test]
    fn context_menu_only_offers_insight_when_available() {
        let without = context_menu_items(&target("orders", "name", 1));
        assert!(
            !without
                .iter()
                .any(|item| matches!(item.action, ContextAction::InsightColumn { .. })),
            "不能洞察时不该摆这一项：{without:?}"
        );

        let mut with = target("orders", "name", 1);
        with.insight = true;
        let with = context_menu_items(&with);
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
        let items = context_menu_items(&target(&long, "c", 0));
        assert!(items[0].label.contains('…'), "{}", items[0].label);
    }

    /// 复制整列 / 整行：走**视图行序**，并按 TSV 规则转义（与 `ResultEntry::to_tsv`
    /// 同一份实现）：带制表符 / 换行 / 引号的值不许把粘出去的形状搞坏。
    #[test]
    fn copying_a_column_and_a_row_keeps_the_tsv_shape() {
        let mut grid = grid_with(&[&["1", "plain"], &["2", "two\tcells"], &["3", "say \"hi\""]]);

        // 整行：列之间是制表符，只有会破坏形状的格子才加引号
        assert_eq!(grid.row_text(0), "1\tplain");
        assert_eq!(grid.row_text(1), "2\t\"two\tcells\"");
        assert_eq!(grid.row_text(2), "3\t\"say \"\"hi\"\"\"");
        assert_eq!(
            grid.row_text(99),
            "",
            "行不存在给空串（不摆这一项的是界面）"
        );

        // 整列：一行一个取值（本身就是单列 TSV）
        assert_eq!(grid.column_text(0), "1\n2\n3");
        assert_eq!(
            grid.column_text(1),
            "plain\n\"two\tcells\"\n\"say \"\"hi\"\"\""
        );

        // 取值里的换行也被包起来：不然它会拆出一行，看上去像多了一行数据
        let with_break = grid_with(&[&["1", "line\nbreak"]]);
        assert_eq!(with_break.column_text(1), "\"line\nbreak\"");
        assert_eq!(with_break.row_text(0), "1\t\"line\nbreak\"");

        // 视图行序：本地排序 / 筛选之后，复制的是**看着的那些行**
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.column_text(0), "3\n2\n1");
        assert_eq!(grid.row_text(0), "3\t\"say \"\"hi\"\"\"");

        grid.set_filter("two");
        assert_eq!(grid.column_text(0), "2", "筛选生效时只复制看得见的行");
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

    /// 类型 → 格子样式（表驱动）：数字右对齐、布尔 / 时间族等宽、**未知档就是今天的样式**
    ///
    /// 这条钉的是“类型感知渲染”的最后一跳：模型说“是数字”还不够，渲染得真按右对齐画。
    #[test]
    fn the_cell_style_follows_the_column_type_and_unknown_stays_as_today() {
        let cases: &[(ColumnKind, TextAlign, bool)] = &[
            (ColumnKind::Integer, TextAlign::Right, false),
            (ColumnKind::Float, TextAlign::Right, false),
            (ColumnKind::Decimal, TextAlign::Right, false),
            (ColumnKind::Boolean, TextAlign::Left, true),
            (ColumnKind::Timestamp, TextAlign::Left, true),
            (ColumnKind::Date, TextAlign::Left, true),
            (ColumnKind::Time, TextAlign::Left, true),
            (ColumnKind::Uuid, TextAlign::Left, true),
            (ColumnKind::Text, TextAlign::Left, false),
            (ColumnKind::Json, TextAlign::Left, false),
            (ColumnKind::Binary, TextAlign::Left, false),
            // 没类型 / 认不出：左对齐 + 默认字体 —— 就是今天的样子
            (ColumnKind::Unknown, TextAlign::Left, false),
        ];
        for (kind, align, monospace) in cases {
            let style = cell_style(*kind);
            assert_eq!(style.align, *align, "{kind:?} 的对齐");
            assert_eq!(style.monospace, *monospace, "{kind:?} 的字体");
        }
    }

    /// 表头类型小标签：驱动报了才摆（行号槽永不摆），名字**原样**（方言名照显示）
    ///
    /// 同一条用例兼验“换了一份结果/清空之后类型不跟过来”：漏给就是“没类型”，
    /// 而**没类型这一档就是今天**（表头只有一个列名）。
    #[test]
    fn the_header_shows_the_type_only_when_the_driver_reported_one() {
        let mut grid = grid();
        assert!(grid.type_label(0).is_none(), "行号槽不是数据");
        assert!(grid.type_label(1).is_none(), "没带类型 = 不摆标签");
        assert!(grid.type_label(2).is_none());
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);

        grid.set_column_types(vec![
            Some(ColumnType::parse("bigint")),
            Some(ColumnType::parse("numeric(38,10)")),
        ]);
        assert_eq!(grid.type_label(1), Some("bigint"));
        assert_eq!(
            grid.type_label(2),
            Some("numeric(38,10)"),
            "表头显示的是驱动报的**原样名字**（带参数也照显）"
        );
        assert_eq!(grid.type_label(0), None, "行号槽没有类型可谈");
        assert_eq!(grid.type_label(9), None, "越界不 panic");
        assert_eq!(grid.column_kind(1), ColumnKind::Integer);
        assert_eq!(grid.column_kind(2), ColumnKind::Decimal);
        assert_eq!(
            grid.column_kind(0),
            ColumnKind::Unknown,
            "行号槽永远是无类型档"
        );

        // 换一份结果：上一份的类型不许跟过来（面板漏给 = 没类型，与今天一致）
        grid.set_data(vec!["a".to_string()], vec![vec!["1".to_string()]]);
        assert_eq!(grid.type_label(1), None);
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);

        // 一条列报了、另一条没报：没报的那条不摆标签
        grid.set_column_types(vec![Some(ColumnType::parse("uuid")), None]);
        assert_eq!(grid.type_label(1), Some("uuid"));
        assert_eq!(grid.type_label(2), None);

        grid.clear("执行失败：boom");
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);
        assert!(grid.type_label(1).is_none());
    }

    /// 没有筛选也没有排序时**不物化**视图行序（入库 / 切结果集时省掉的正是这份整表映射）；
    /// 而“不物化”必须与“物化一份恒等映射”给出**一模一样**的行——否则就是省错了
    #[test]
    fn the_view_order_is_not_materialized_until_a_filter_or_sort_needs_it() {
        let mut grid = grid_with(&[&["2", "orders"], &["1", "users"]]);
        assert!(
            !grid.view_order_materialized_for_test(),
            "刚入库：恒等，不建映射"
        );
        let plain = grid.visible_rows();
        assert_eq!(plain.len(), 2);

        // 筛一下：这时才建（本来就要逐行看一遍）
        grid.set_filter("users");
        assert!(grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_row_count(), 1);
        assert_eq!(grid.cell(0, 2), "users");

        // 清掉筛选：又回到恒等（不是“留一份恰好是全集的映射”）
        grid.set_filter("");
        assert!(!grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_rows(), plain, "与物化版给出的行必须一模一样");

        // 排序也是（用户真点了列头才建）
        grid.apply_sort(Some((0, false)));
        assert!(grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_rows()[0][0], "1");
        grid.apply_sort(None);
        assert!(!grid.view_order_materialized_for_test());
        assert_eq!(
            grid.visible_rows(),
            plain,
            "取消排序回原顺序（不是部分退还）"
        );

        grid.clear("执行失败：boom");
        assert!(!grid.view_order_materialized_for_test());
    }

    /// 筛选匹配的口径（表驱动）：与 `to_lowercase().contains` 一致；ASCII 边上零分配
    ///
    /// `needle` 的契约是**已小写**（调用方 `to_lowercase` 一次，不给每格都分一次）；
    /// ASCII 快路径两边大小写都不敏感，非 ASCII 回落路径按小写词比。
    #[test]
    fn the_filter_matcher_keeps_the_lowercase_semantics() {
        let cases: &[(&str, &str, bool)] = &[
            ("orders", "orders", true),
            ("ORDERS", "orders", true),
            ("Orders_Archive", "orders", true),
            ("users", "orders", false),
            ("", "orders", false),
            ("ord", "orders", false),
            ("1", "orders", false),
            // 非 ASCII：回落 `to_lowercase`（大小写仍然不敏感，中文不受影响）
            ("Ärger", "ärger", true),
            ("ärger", "ärger", true),
            ("订单_2026", "订单", true),
            ("订单_2026", "2026", true),
            ("订单", "orders", false),
        ];
        for (cell, needle, expected) in cases {
            assert_eq!(
                cell_matches(cell, needle),
                *expected,
                "格 {cell:?} 找 {needle:?}"
            );
        }

        // 格内区间：两边都是 ASCII 才给得出；含非 ASCII 时是 `None`（命中但无区间）
        assert_eq!(find_in_cell("Orders_Archive", "orders"), Some(Some(0..6)));
        assert_eq!(find_in_cell("订单_2026", "2026"), Some(None));
        assert_eq!(find_in_cell("plain", ""), None, "空词不命中任何格");
        assert_eq!(find_in_cell("plain", "plainly"), None, "词比格长");
    }

    /// 结果内查找：只看**数据列**（`#` 槽不算）、按视图行序、找完一圈回到开头
    #[test]
    fn finding_in_the_result_walks_the_visible_rows_and_wraps() {
        let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
        assert_eq!(
            grid.find_next("orders", None),
            Some(ResultMatch {
                row: 0,
                col: 2,
                range: Some(0..6),
            }),
            "第一处命中的是**数据列**（行号槽不是数据）"
        );
        assert_eq!(
            grid.find_next("orders", Some((0, 2))).map(|hit| hit.row),
            Some(2),
            "从上一处继续：中间那行的第二列（users）不命中，继续往后找"
        );
        assert_eq!(
            grid.find_next("orders", Some((2, 2))).map(|hit| hit.row),
            Some(0),
            "找不到就绕回开头（表是圆的）"
        );

        // 行号槽里的数字不算命中（`#` 显示的是视图行号，不是数据）
        assert_eq!(
            grid.find_next("3", None),
            Some(ResultMatch {
                row: 2,
                col: 1,
                range: Some(0..1),
            })
        );
        assert_eq!(grid.find_next("nope", None), None);
        assert_eq!(grid.find_next("   ", None), None, "空词不找任何东西");

        // 含非 ASCII 的格：命中但不给格内区间（宿主退化成整格高亮）
        let chinese = grid_with(&[&["1", "订单_2026"]]);
        assert_eq!(
            chinese.find_next("2026", None),
            Some(ResultMatch {
                row: 0,
                col: 2,
                range: None,
            })
        );

        // 筛选之后只找**看得见的行**（找到看不见的行等于没找到）
        grid.set_filter("users");
        assert_eq!(grid.find_next("orders", None), None);
        assert_eq!(grid.find_next("users", None).map(|hit| hit.row), Some(0));
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

    /// 工具栏左段：`行数 N` │ `耗时 1.2s` │ `来源摘要` │ `连接名`，没有的东西不占位
    #[test]
    fn toolbar_segments_follow_the_prototype_order() {
        let full = ResultToolbar {
            rows: Some(1_204),
            affected_rows: None,
            failed: false,
            elapsed_ms: Some(1_200),
            // 【B15】血缘排在连接之前（“这份是怎么来的”比“从哪条连接来”更常被问）
            lineage: Some("下发筛选".to_string()),
            connection: Some("●P·orders".to_string()),
            ..Default::default()
        };
        assert_eq!(
            full.segments(),
            ["行数 1,204", "耗时 1.2s", "下发筛选", "●P·orders"]
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
