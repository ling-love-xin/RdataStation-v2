//! 结果网格数据面（`grid/` 子目录第一块）：`ResultGridDelegate` 结构体 + 数据同步
//! （`set_data` 家族）与钩子注入。视图行序 / 编辑 / 取值等 impl 片段按域拆在
//! [`view_rows`] / [`edit`] / [`text`]——同一结构体的多个 impl 块（Rust 合法），
//! 字段 `pub(crate)` 供同 crate 各 impl 共享（2026-10-29 拆分，零逻辑改动）。

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::date_picker::DatePickerState;
use gpui_kit::component::input::InputState;
use gpui_kit::*;

use super::menu::column_distinct_values;
use super::selection::{CellRange, GridKeyStep};
use super::view_rows::TextFingerprint;
use crate::ports::Presentation;
use crate::rowset::{LocalRowSet, RowRef};
use crate::store::ColumnType;
use ::shared::FactFrame;
use workbench_shell::model::ColumnAnnotation;
use workbench_shell::result_style::{ColumnHeaderExtra, TypeStylePrefs};
/// 结果网格的 key context（`crates/app` 把 `Ctrl+C` 绑在它上面，两边成对）
///
/// 与编辑器面板根的 `editor` 刻意分开：这个上下文只挂在结果网格那层元素上，所以
/// **编辑区里 `Ctrl+C` 仍是复制 SQL 文本、网格里才是复制选中的一格 / 一整行**。
/// 组件库的表格自己用的是 `DataTable`（方向键 / 翻页 / `Esc` 清选）——两个上下文
/// 会同时出现在焦点路径上，各管各的键。
///
/// 2026-10-23 下移：key context 字符串跨 crate 匹配（editor 的 `commands.rs` 从这里重导出）。
pub const RESULT_GRID_CONTEXT: &str = "editor-result-grid";

/// 【筛选】列头菜单「按值筛选此列」值列表的上限（DBeaver 值列表筛选取前若干去重值，
/// 不把整列几百个值全塞进菜单——要全量用全局筛选框）
pub const COLUMN_FILTER_VALUE_LIMIT: usize = 8;

/// 单元格**展示**文本的字符上限（对齐 dbdelve `result_grid.rs` `CELL_DISPLAY_LIMIT=300`）。
///
/// 只裁「画出来的这一格」：复制 / 导出 / 编辑初值 / 悬停全文 / 按值筛选 / 统计仍走完整值
/// （[`crate::ResultGridDelegate::cell`] 与 `row_cell_text` 均不经过这个裁剪）。超长
/// JSONB / PostGIS 几何 / 大文本因此不参与 GPUI 文本测量，可见格每帧布局成本有硬上界。
pub const CELL_DISPLAY_LIMIT: usize = 300;

/// 滚动到底要更多数据时的回调（面板注入：它才知道当前选中哪份结果、忙不忙）
///
/// 组件库在**可见范围接近末尾**时调 delegate 的 [`TableDelegate::load_more`]，
/// 而 delegate 在 `TableState` 里拿不到面板实体，所以走这个钩子（与执行通道同一个口径：
/// 视图不自己发执行，只把意图交回给知道全局的那一层）。
pub type LoadMoreHook = Rc<dyn Fn(&mut App)>;

/// 【T2a-4】单元格编辑**提交**的回调（面板注入：把（数据行序、数据列序、原值、新值）
/// 交给面板落变更集；面板自己在变更集里决定 upsert 还是移除）
pub type CommitEditHook = Rc<dyn Fn(usize, usize, String, String)>;

/// 【#28-B4】本地行格编辑**提交**的回调（面板注入：把（本地行 id、数据列序、新值）
/// 写回本地行快照；不进变更集、不进格级 / 行级撤销栈——本地行格编辑提交即落快照，
/// 与底层行格编辑走 `CommitEditHook` 平行、互不混栈）
pub type CommitLocalEditHook = Rc<dyn Fn(u64, usize, String)>;

/// 【T2a-4】正在编辑的单元格（编辑态由网格持有；提交 / 取消走钩子交回面板）
///
/// - 行号存两序：**视图行序**管渲染匹配（组件只懂视图行），**行身份**管写回路径
///   （`Data` 走变更集钩子，`Local` 走本地行快照钩子；筛选 / 排序之后视图行与身份可以不同）。
/// - 列下标是**数据列序**（不含 `#` 行号槽；进入编辑时从组件列序折算掉）。
/// - `settled`：编辑框的初值同步与聚焦是幂等一次性动作（在 render_td 里做），
///   做过了就不再碰——免得每次重渲染都重设值 / 抢焦点。
pub struct CellEdit {
    /// 视图行序（渲染匹配用）
    pub(crate) view_row: usize,
    /// 行身份（底层行号 / 本地行 id；决定提交走哪条钩子）
    pub(crate) reference: RowRef,
    /// 数据列序
    pub(crate) column: usize,
    /// 进入编辑时的原值（提交时若没变，面板据此不落变更）
    pub(crate) original: String,
    /// 当前输入值（`InputEvent::Change` 时从编辑框同步；提交时读它）
    pub value: String,
    /// 编辑框是否已完成首次聚焦与初值同步
    pub(crate) settled: bool,
    /// 【类型感知】提交校验失败时的错误文案（`Some` = 上次提交被拒，编辑框红框提示；
    /// 输入一变化就清除）。只影响“这次提交能不能发出去”，不落变更集。
    pub error: Option<String>,
}

/// 【B14】「按值筛选」的钩子（面板注入：把值写进筛选框并立刻生效）
pub type FilterValueHook = Rc<dyn Fn(&str, &mut App)>;

/// 【筛选】列级筛选钩子（列头菜单「按值筛选此列 / 清除」交回面板；参数 = 数据列、值）
pub type ColumnFilterHook = Rc<dyn Fn(usize, &str, &mut App)>;

/// 【B14】「排序下发」的钩子（面板注入：按列名重查源库；入参 = （列名，是否降序））
pub type SortDownHook = Rc<dyn Fn(&str, bool, &mut App)>;

/// 【M8】「洞察此列」的钩子（面板注入：把列名交给面板，面板再连同 SQL / 连接交给宿主）
///
/// 网格不知道结果集的 SQL 与连接（那是面板/存储的事），所以这里只传列名——
/// 与「按值筛选」同一种分工：**网格只负责说“用户点了谁”**。
pub type InsightColumnHook = Rc<dyn Fn(&str, &mut App)>;

/// 【结果集二次分析】「分析此结果集」的钩子（宿主接出口时面板才注入）。
///
/// 不带参数：整份结果是**面板与宿主的共同真值**（行数据在结果条目里），网格不该
/// 把 10 万行再拷一遍去传参。宿主自己按当前活动结果物化 → 送洞察。
pub type AnalyzeResultSetHook = Rc<dyn Fn(&mut App)>;

/// 【结果集持久化】「持久化为项目表」的钩子（与 [`AnalyzeResultSetHook`] 同形）。
///
/// 同一条理由不带参数：整份结果的真值在结果条目里，网格不复制行数据去传参。
pub type PersistResultSetHook = Rc<dyn Fn(&mut App)>;

/// 【遗留 #7】「查看完整值」的钩子（面板注入）：值 / 列名 / 数据列下标 / 视图行。
/// 弹层是宿主的事（对话框生命周期归宿主），网格只负责发意图。
pub type PreviewValueHook = Rc<dyn Fn(String, String, usize, usize, &mut App)>;

/// 【P2-3】列统计弹层的钩子（宿主开弹层；参数 = 列名、统计文本）
pub type ColumnStatsHook = Rc<dyn Fn(String, String, &mut App)>;

/// 【#28-B3】行级动作钩子（面板注入：把入队请求交回宿主分派；新增 / 复制 / 删除一行）。
/// 网格不持本地行真值，动作意图打包成 [`crate::ports::ResultRequest`] 经钩子入队，
/// 宿主在权威 store 槽位落地叠加层变更。
pub type RowActionHook = Rc<dyn Fn(crate::ports::ResultRequest, &mut App)>;

/// 【结果内查找】查找过滤的完整口径：词 + 三开关（对齐 DBeaver 的
/// `SearchQuickFilterInfo(text, caseSensitive, useRegex, wholeWord)`）。
///
/// 词存 `trim` 后的**原样**，不小写化——正则模式下小写化会破坏 `[A-Z]` / `\D`
/// 这类写法；小写副本在建 [`super::text::FindMatcher`] 时按需生成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FindFilter {
    pub(crate) needle: String,
    pub(crate) criteria: crate::ports::FindCriteria,
}

/// 网格数据（表头 + 行）
#[derive(Default)]
pub struct ResultGridDelegate {
    pub(crate) columns: Vec<String>,
    /// 【结果集 #296】列头拖拽后的**显示序**（列名数组；与 `columns` 无关——只记用户
    /// 拖出来的顺序，供面板持久化 / 恢复时按列名重排）。`move_column` 是组件拖拽
    /// 时调的 hook（`TableDelegate::move_column` 默认空），这里重排本数组即可；
    /// 组件自身的 `col_groups` 已让渲染立即按新序走，delegate 不需要改渲染。
    pub(crate) display_order: Vec<String>,
    /// 【列序】`display_order` 折算成**数据列下标**的缓存（视图列序的权威）。
    ///
    /// `data_column` / `view_column` 都读它，所以它必须与实际列序同源：任何改动
    /// `display_order`（`set_data` / 拖拽 `move_column` / 程序化移动 / 按列名排序 /
    /// 重置）之后都要 [`ResultGridDelegate::rebuild_column_order`]。
    /// 缓存是为了让 `data_column` 在逐格渲染里保持 O(n)（每格现算 `display_order`
    /// 的列名匹配会是 O(n²)）。
    pub(crate) order_cache: Vec<usize>,
    pub(crate) rows: Vec<Vec<String>>,
    /// 【D10-3】`rows` 里唯一那行是「空态占位行」而不是真数据。
    ///
    /// `set_data` 在「0 行 + 有列」时塞一行全空行撑起表头（见那里的注释），
    /// 于是本仓空结果集在网格里**看起来**有 1 行。DBeaver 那边 `getItemCount() == 0`，
    /// 交角全选（`LightGrid.java:4276-4277`）会直接 `return null` 什么都不选；
    /// 所以全选必须先认出这是占位行，否则状态行会把空结果报成「1 行 × N 列」。
    ///
    /// 只在 [`ResultGridDelegate::set_data`] 里维护：那是本仓唯一「整批换行」的入口。
    pub(crate) placeholder_row: bool,
    /// **每列的类型**（与 `columns` 同序；空 = 这份结果没带类型）
    ///
    /// 只影响怎么画（右对齐 / 等宽 / 表头小标签），不影响任何取值；没带类型时与今天完全一致。
    /// 由面板在 `set_data` **之后**同一次同步里给（见 [`ResultGridDelegate::set_column_types`]）。
    pub(crate) column_types: Vec<Option<ColumnType>>,
    /// 【P1】**每列的元数据注释**（与 `columns` 同序；空 = 这份结果没报注释）
    ///
    /// 只影响表头 tooltip（来源表 / 来源列 / 注释 / 类型 / 可空 / 默认值）；没报时与今天完全一致。
    /// 由面板在 `set_data` **之后**同一次同步里给（见 [`ResultGridDelegate::set_column_annotations`]）。
    pub(crate) column_annotations: Vec<Option<ColumnAnnotation>>,
    /// 【DBeaver 对齐】列头第二行画什么（三档，出厂 `Nothing`；列名下方小字）。
    /// 由面板在构造 / 设置变更时同步过来；非 `Nothing` 时表头抬成两行
    /// （列名 + 附加行）。
    pub(crate) column_header_extra: ColumnHeaderExtra,
    /// 【选中统计】三项开关的镜像（对齐 DBeaver `resultset.show.selected.*`）。
    ///
    /// 菜单要标「✓ 已开启」，而偏好真值在面板：网格只读这份镜像。面板在构造 / 切换 /
    /// 设置页变更时同步过来（[`Self::set_selection_stat_prefs`]），不随 `set_data` 清空。
    pub(crate) selection_stat_prefs: crate::grid::SelectionStatPrefs,
    /// 【布局段】呈现方式镜像（面板态；右键菜单给「网格 / 文本」标勾选态用）。
    /// 与 `selection_stat_prefs` 同口径：视图偏好，不随 `set_data` 清。
    pub(crate) presentation_view: Presentation,
    /// 【布局段】筛选条是否被**隐藏**（面板态镜像；右键菜单的「✓ 显示过滤器面板」读它）。
    ///
    /// 存反向布尔是因为本结构体 `derive(Default)`：出厂语义是「显示」
    /// （`DataEditorsPreferencesInitializer.java:67` 默认 true），而 `bool` 的
    /// `Default` 只能给 `false`；存「隐藏」正好让默认值落在正确的一侧。
    pub(crate) filter_panel_hidden: bool,
    /// 【类型样式】全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    /// 面板在 `set_data` 同一次同步里（或偏好变更时）同步过来；只影响渲染显示，
    /// 复制 / 导出 / 编辑仍用原始值。
    pub(crate) type_style: TypeStylePrefs,
    /// 空态文案（没有结果 / 执行失败都在这里说清楚）
    pub(crate) empty_text: String,
    /// 这份结果**还能不能取下一段**（面板从权威结果同步过来；`load_more` 据此决定要不要真的取）
    pub(crate) has_more: bool,
    /// 正在取下一段（防止“滚动到底”在回填之前反复触发）
    pub(crate) loading_more: bool,
    /// 取下一段的回调（面板构造时注入）
    pub(crate) on_load_more: Option<LoadMoreHook>,
    /// 【B15】本地筛选词（视图层：只影响看见与导出的行，**不重查数据库**）
    pub(crate) filter: String,
    /// 【筛选】列级筛选（`(数据列, 值)`；本地包含匹配，与全局 `filter` 叠加取交集）。
    /// 独立于全局筛选词：列头菜单的「按值筛选此列」走它，清空也不动筛选框。
    pub(crate) column_filter: Option<(usize, String)>,
    /// 【结果内查找】「只显示匹配行」的临时过滤（词 + 三开关，词已 `trim`）。
    ///
    /// 与 `filter` **不是同一个东西**：`filter` 是筛选框的权威值（用户显式设的，会跟着
    /// 结果集走），这里是查找栏「Show matching」开关的临时态——关掉查找栏就撤掉，
    /// 绝不能回头去动 `filter`（否则用户筛的词会被查找栏清掉）。
    /// 对齐 DBeaver 的 `SearchQuickFilterInfo`：`ResultsetFindReplaceOverlay` 把它灌进
    /// `model.setQuickFilter`，`onClose`/取消勾选时 `applyFilterAction(null)` 撤回。
    /// 口径与查找定位同一份实现（[`super::text::FindMatcher`]），三层筛选
    /// （全局 / 列级 / 查找）叠加取交集。
    pub(crate) find_filter: Option<FindFilter>,
    /// 【B15】本地排序（列下标按**数据列**算，`0` = 第一数据列；`None` = 原顺序）
    pub(crate) sort: Option<(usize, bool /* 降序 */)>,
    /// 【文本视图】TSV 缓存：指纹命中直接复用，避免大结果集每次渲染全量重拼
    /// （bench：50k×8 一次拼 ~120ms，动画期间每帧重拼会卡住 UI——"点击文本
    /// 半天才出来"的根因）。指纹覆盖影响文本的所有输入（视图行数 / 列数 /
    /// 隐藏列 / 全局筛选 / 列筛选 / 排序），内容型变更（set_data、本地快照
    /// 替换）在入口显式失效，双保险不依赖逐个方法标脏。
    pub(crate) text_cache: std::cell::RefCell<Option<(TextFingerprint, String)>>,
    /// 【B14】最近右键的单元格（视图行号，列下标含行号槽）——组件库没给访问器，
    /// 所以在 `render_td` 上自己记（事件从最内层开始派发，不会丢）
    pub(crate) context_cell: Option<(usize, usize)>,
    /// 【B15】冻结的数据列（原生 `Column.fixed`：横向滚动时钉在左侧）
    pub(crate) frozen: Vec<usize>,
    /// 【P2】隐藏的数据列（**数据列序**；显示时跳过它们。只影响视图列序，数据一行不动）
    pub(crate) hidden: Vec<usize>,
    /// 【P1-5】会话内列宽记忆：按**列名**记（隐藏列 / 列序变化后回填仍对得上），
    /// `None` 不算——空表 = 用户没拖过，走默认档。切结果集由 `set_data` 清空。
    /// 行号槽不进表（宽度钉死）；本机制不做落盘持久化（见 backlog）。
    pub(crate) column_widths: HashMap<String, Pixels>,
    /// 【P2-4】单行转置：把选中一行竖排成「# / 字段 / 值」三列（DBeaver 式；只读视图）
    pub(crate) transposed: bool,
    /// 【P2-4】转置当前展示的行身份（底层行 / 本地行；越界自动夹取到视图序列末位）
    pub(crate) transpose_row: RowRef,
    /// 【B14】按值筛选的钩子（面板注入）
    pub(crate) on_filter_value: Option<FilterValueHook>,
    /// 【筛选】列级筛选的钩子（面板注入；列头菜单 FilterColumnValue / ClearColumnFilter）
    pub(crate) on_column_filter: Option<ColumnFilterHook>,
    /// 【B14】排序下发的钩子（面板注入）
    pub(crate) on_sort_down: Option<SortDownHook>,
    /// 【M8】「洞察此列」的钩子（宿主接端口时面板才注入）
    pub(crate) on_insight_column: Option<InsightColumnHook>,
    /// 【结果集二次分析】「分析此结果集」的钩子（宿主接端口时面板才注入）
    pub(crate) on_analyze_result_set: Option<AnalyzeResultSetHook>,
    /// 【结果集持久化】「持久化为项目表」的钩子（宿主接了持久化出口时才注入）
    pub(crate) on_persist_result_set: Option<PersistResultSetHook>,
    /// 【遗留 #7】「查看完整值」的钩子（面板注入）
    /// 【遗留 #7】「查看完整值」的钩子（面板注入）：值 / 列名 / 数据列下标 / 视图行
    pub(crate) on_preview_value: Option<PreviewValueHook>,
    /// 【P2-3】列统计弹层的钩子（宿主开弹层；参数 = 列名、统计文本）
    pub(crate) on_column_stats: Option<ColumnStatsHook>,
    /// 【A9】这份结果的来源表名（面板从结果 SQL 猜好传来；`None` = 猜不到
    /// → 右键不摆「复制整行为 INSERT」）
    pub(crate) result_table: Option<String>,
    /// 【M8】这份结果能不能洞察（面板同步数据时算：见 `ResultEntry::can_insight_column`）
    pub(crate) insight_available: bool,
    /// 【B15】视图行序：当前看着的这一串行身份（底层数据行 `RowRef::Data` / 本地行
    /// `RowRef::Local`）。
    ///
    /// `None` = **与基准序列相同**（既没筛选也没排序）：这时不物化那份映射——
    /// 10 万行的结果切一次结果集就为它分配一个 10 万元素的表，而绝大多数时候视图
    /// 与基准是同一个顺序，那份表一次也用不上（入库 / 切换结果集时不该做这件事）。
    ///
    /// 筛选与排序都只改这个映射，**底层数据行一行不动**（所以清除筛选能原样恢复，
    /// 也不会把“已抓到的窗口”搞乱）。行号列显示的是**视图行号**，不是数据行号。
    /// 排序只作用底层行；本地行参与筛选命中、始终登记序追加末尾（#28-B1 拍板 D1）。
    pub(crate) view_rows: Option<Vec<RowRef>>,
    /// 【T2a-4】这份结果能不能编辑（面板同步数据时算：`ResultEntry` 编辑判据 +
    /// 面板当前连接 / 项目只读防线；`false` 时双击不进入编辑）
    pub(crate) editable: bool,
    /// 【T2a-4】正在编辑的单元格（有值 = 编辑中；编辑框只在这一格渲染）
    pub(crate) editing: Option<CellEdit>,
    /// 【T2a-4】复用的编辑框实体（宿主构造时创建，同步数据时注入；每次编辑
    /// 复用同一个，不做“每格一个实体”的泄漏）
    pub(crate) edit_input: Option<Entity<InputState>>,
    /// 【类型感知】复用的日期选择器实体（宿主注入）：`Date` 列编辑时用它替代 `Input`
    /// （日历弹层 + 可手敲），值仍以 `YYYY-MM-DD` 字符串提交
    pub(crate) edit_calendar: Option<Entity<DatePickerState>>,
    /// 【T2a-4】提交回调（面板注入：落变更集）
    pub(crate) on_commit_edit: Option<CommitEditHook>,
    /// 【#28-B4】本地行格编辑提交回调（面板注入：写回本地行快照；不进变更集 / 撤销栈）
    pub(crate) on_commit_local_edit: Option<CommitLocalEditHook>,
    /// 【#28-B3】行级动作回调（面板注入：新增 / 复制 / 删除一行打包成请求交回宿主分派）
    pub(crate) on_row_action: Option<RowActionHook>,
    /// 【列布局落盘】动作队列（面板注入）：列头菜单 / 列头右键里的「冻结」不直接改
    /// delegate，而是把请求交回宿主——宿主改完才能借同一条通道存会话。
    /// `None` = 独立网格（单元测试等），菜单退回就地切换。
    pub(crate) action_queue: Option<crate::ports::ResultActionQueue>,
    /// 【T2a-3】脏格集合（**数据行序**，含列序；面板从权威变更集投影，标底色）
    pub(crate) dirty: Vec<(usize, usize)>,
    /// 【T2a-6】冲突格集合（提交回执标过 `Conflict` 的格子；面板从权威变更集投影）
    ///
    /// 与 `dirty` 分开：冲突格**不是**待提交的变更（提交已结束），它是「这一格没写进去、
    /// 源库里的旧值已被别人改过」的**提示**——底色用 danger 而不是 warning，一眼能分清。
    pub(crate) conflict: Vec<(usize, usize)>,
    /// 【B5】这份结果的事实帧只读句柄（面板在 `set_data` **之后**同步进来，与类型 / 注释
    /// 同口径时序）。仅用于交互派生时的列式取值（数值统计 / 原生数值排序）；render 纯读路径
    /// 不解析它。表档帧 batches 已释放，此时本地统计 / 排序回退显示矩阵口径。
    pub(crate) fact: Option<Arc<FactFrame>>,
    /// 【#28】本地叠加层投影快照（面板在 `set_data` **之后**同步进来；delegate 只读、
    /// 不持真值——真值在 store 结果集槽位，切换结果集 / 刷新由宿主重新投影）。
    /// `None` = 纯底层结果集（现状行为不变）。本地行参与网格视图与筛选命中，不参与列排序。
    pub(crate) local: Option<LocalRowSet>,
    /// 【分页】当前视口首行（**视图行序**，0 基）。由组件库 `visible_rows_changed`
    /// 回写（滚动 / 筛选 / 排序后都会变），供状态栏「上页 / 下页」算目标行。
    /// 面板不自己猜滚动位置——表格才是滚动位置的唯一真值。
    pub(crate) visible_top: usize,
    /// 【D10-7 翻页键】当前视口能完整容纳的行数（`visible_range.len()`）。
    /// 与 [`Self::visible_top`] 同一回调回写；`PAGE_UP` / `PAGE_DOWN` 的「一页」
    /// 就取这个值（DBeaver `LightGrid.java:3842-3874` 的 `getVisibleGridHeight()`）。
    /// 0 = 还没有可见范围回写（首帧测量前），此时翻页键不位移。
    pub(crate) visible_len: usize,
    /// 【选中行】组件报来的整行选中（`TableEvent::SelectRow`；**视图行序**）。
    ///
    /// 组件自己**不给**选中行铺色：`table/state.rs:2249` 拿到 `is_selected` 后只用来决定
    /// 行底线要不要画、hover 要不要抑制，整行没有任何底色（选中行与未选中行长得一样）。
    /// 于是「再点一下已选中的格子」升级成整行选中（`grid/status.rs` 关掉了组件窄行头，
    /// 这是选整行的唯一入口）时，网格上看不出任何变化。
    ///
    /// DBeaver 的选中行是**整行色**（`LightGrid` 逐格按 `getCellBackground(selected)` 取色），
    /// 所以这里镜像一个行号，由 `table_impl.rs` 的 `render_tr` 在行容器上铺一档与单元格
    /// 选中同槽的底色（`tokens.table_active`）。
    pub(crate) selected_row: Option<usize>,
    /// 【D10】矩形单元格选区（视图坐标：行 = 视图行序，列 = 组件列下标，含行号槽）。
    ///
    /// 组件只记得**一个**选中格（`TableSelection::Cell`），它铺的高亮也只覆盖那一格
    /// （vendor `table/state.rs` 的 `is_cell_selected` overlay）。矩形里其余的格由
    /// `grid/cell.rs::render_td` 在这里铺同槽底色；焦点格跳过（组件 overlay 已经在画，
    /// 两层半透明叠加会比别的格深一档）。
    ///
    /// DBeaver 的选区是 `Set<GridPos>` + 锚点（`LightGrid.java:1590-1662`、`:2674-2790`）；
    /// 本仓只做「单个矩形」这一档，`anchor`/`focus` 就是全部真值（见 [`CellRange`]）。
    pub(crate) cell_range: Option<CellRange>,
}
impl ResultGridDelegate {
    /// 空网格：只带一句空态说明
    pub fn empty(text: impl Into<String>) -> Self {
        Self {
            empty_text: text.into(),
            ..Default::default()
        }
    }

    /// 【选中行】镜像组件报来的整行选中（`None` = 没有整行选中）。
    ///
    /// 面板在 `TableEvent::SelectRow` / `TableEvent::ClearSelection` 里调；`render_tr`
    /// 读它铺整行底色（组件自己不给选中行画色，见字段注释）。
    pub fn set_selected_row(&mut self, row: Option<usize>) {
        self.selected_row = row;
    }

    /// 【选中行】当前镜像的整行选中（视图行序）；`render_tr` 与用例读它。
    pub fn selected_view_row(&self) -> Option<usize> {
        self.selected_row
    }

    /// 【D10】一次单元格点击 → 更新矩形选区（DBeaver `updateCellSelection` 的点击档，
    /// `LightGrid.java:2705-2748`）。
    ///
    /// - `extend == false`（无修饰键）：`selectedCells` 清空重填、锚点移到该格 → 单格选区；
    /// - `extend == true`（Shift）：锚点不动，只换终点 → 两格为对角的矩形；
    /// - 还没点过任何格时按普通点击处理（DBeaver 在锚点缺失时直接返回，本仓把
    ///   「Shift 起手」当成普通点击，省得用户第一次按住 Shift 点什么都没有）。
    ///
    /// Ctrl（并集）本仓没做：DBeaver 是往集合里加/减（`:2718-2735`），单矩形模型表达不了，
    /// 所以调用方在 Ctrl 时也传 `extend = false`（差异记在基线文档 §29 台账）。
    pub fn click_cell(&mut self, row: usize, col: usize, extend: bool) {
        // 【D10-2】行号槽按下 = 选中整行。
        //
        // DBeaver 的 row header 不在 `columns` 里，所以 `getCells(row, cells)`
        // （`LightGrid.java:3125-3132`）铺出来的是**全部数据列**；Shift 点行头同理走
        // `getCells(row, focusItem, cells)`（`:3128-3129`）——行区间两端、列全宽。
        // 本仓行号槽是组件第 0 列（`columns_count` 首列），因此命中行号槽时把矩形两侧
        // 一起归一：锚点列钉在 0、焦点列钉在末列 → `cols()` = `0..=末列`，与 DBeaver
        // 「整行 = 全部数据列」等价；Shift 那次只换行的一端，列仍全宽（`:3128-3129`）。
        // 计数不受影响——`CellRange::data_columns()` 本来就把列 0 当行号槽排除
        // （`grid/selection.rs:68-72`），所以整行仍是「1 行 × N 数据列 × N 格」。
        //
        // 【D10-9】当前格：DBeaver 点行头后 `focusColumn` 落到「行头右侧第一列」
        // （`:3140-3145` 的 `focusColumn = getColumn(new Point(rowHeaderWidth + 1, e.y))`），
        // `focusItem = row`。矩形本身仍铺满整行（行号槽要高亮），两份状态分开存
        // （见 `CellRange::cursor`）。Shift 那一档 DBeaver 不重设焦点
        // （`:3140` 的 `if (!shift && !altPressed)`），所以扩展时沿用原当前格。
        if Self::is_row_number(col) {
            let last = self.last_view_column();
            let anchor_row = if extend {
                self.cell_range.map_or(row, |range| range.anchor.0)
            } else {
                row
            };
            let cursor = if extend {
                self.cell_range.and_then(|range| range.cursor)
            } else if last >= 1 {
                Some((row, 1))
            } else {
                // 只有行号槽没有数据列（空结果）：没有「首数据列」可落，退回矩形焦点
                None
            };
            self.cell_range = Some(CellRange {
                anchor: (anchor_row, 0),
                focus: (row, last),
                cursor,
            });
            return;
        }
        if extend {
            if let Some(range) = self.cell_range {
                self.cell_range = Some(CellRange {
                    anchor: range.anchor,
                    focus: (row, col),
                    // Shift 扩展不挪当前格（DBeaver `:3154-3156` 只在非 Shift 时更新
                    // `focusColumn/focusItem`），沿用原 cursor。
                    cursor: range.cursor,
                });
                return;
            }
        }
        self.cell_range = Some(CellRange::single((row, col)));
    }

    /// 【D10-5】列头按下 = 选中整列（DBeaver `LightGrid.java:3185-3204`）。
    ///
    /// DBeaver 的列头分支（`columnHeadersVisible && e.y <= headerHeight`）里
    /// `getCells(col, cells)`（`:4418-4422` → `:4424` 起的整列铺法）把**该列所有行**
    /// 塞进 `updateCellSelection(cells, e.stateMask, false, true, MOUSE)`（`:3194-3196`），
    /// 所以列头点击的语义是「整列选区」，不是「只选中列头那一格」。
    /// 排序在 DBeaver 走列头右端的排序箭头（本仓对应 vendor `render_sort_icon`
    /// → `perform_sort`），与列头点击是两条独立入口。
    ///
    /// - `extend == false`：锚点列移到该列 → 单列全高；
    /// - `extend == true`（Shift）：锚点列不动，只换另一端 → **列区间**（全部行）。
    ///
    /// 行号槽的列头就是左上交角（D10-3 已在 `grid/header.rs` 拦下并全选）；
    /// 万一走到这里，按行头按下口径给整行，不另开一套。
    ///
    /// 已知差异（§39.3）：DBeaver 列头点击后 `focusColumn = col`、`focusItem` 不动（`:3201`），
    /// 当前格就是该列的原焦点行；本仓 `focus` 是矩形对角 `(末行, col)`，两者行位置不同。
    /// 同属「focus 只能是矩形角」的限制，随 §40.3 一起解。
    pub fn click_col(&mut self, col: usize, extend: bool) {
        let rows = self.view_len();
        if rows == 0 || self.columns.is_empty() || self.placeholder_row {
            return;
        }
        if Self::is_row_number(col) {
            self.click_cell(0, 0, extend);
            return;
        }
        let anchor_col = if extend {
            self.cell_range.map_or(col, |range| range.anchor.1)
        } else {
            col
        };
        // 【D10-9 待办】DBeaver 列头点击只把 `focusColumn` 改到这一列、`focusItem` 不动
        // （`:3201`），当前格因此落在「原当前行 × 这一列」。本仓暂不跟：当前格由组件的
        // `selected_cell` 一起决定（overlay 画焦点框、`grid/cell.rs:347` 据此不铺色），
        // 而组件唯一的 setter `set_selected_cell` 会 `scroll_to_item(.., Center)` 纵向居中——
        // DBeaver 这里只 `showColumn`（横向滚），照抄会把视口纵向拽动。等组件有不滚动
        // 的选中格入口再补，先把矩形形状对齐（本刀不变）。
        self.cell_range = Some(CellRange {
            anchor: (0, anchor_col),
            focus: (rows - 1, col),
            cursor: None,
        });
    }

    /// 【D10-3】全选（左上交角点击 / 等价的整表选中）。
    ///
    /// DBeaver `LightGrid.java:3174-3184`：命中「行头带 ∩ 列头带」的左上交角
    /// （`rowHeaderVisible && x <= rowHeaderWidth && y < headerHeight`）→
    /// `selectAllCellsInternal`（`:4272-4296`）。那个函数先挡空表
    /// （`columns.isEmpty()` / `getItemCount() == 0` 直接返回 null，不选也不通知），
    /// 再把 `getAllCells()` 整个塞进 `updateCellSelection` —— 语义是「全部行 × 全部列」。
    ///
    /// 本仓矩形选区用同一个表达式：锚点钉 `(0, 0)`、焦点钉 `(末行, 末列)`。
    /// 末列走 `last_view_column()`（数据列按可见列算、转置恒 2），行号槽那列因此被
    /// 一起框住，但计数走 `CellRange::data_columns()`（排除列 0），与 DBeaver
    /// 「行头不在 columns 里」同口径。
    ///
    /// 返回 `false` = 空表，调用方不必刷新（对齐 DBeaver 那条 `return null`）。
    ///
    /// 【D10-10】当前格 = 「视口首行 × 首个数据列」：`selectAllCellsInternal`
    /// （`:4272-4296`）内部先临时借 `(首列, 0)` 当锚、选完还回去（`:4288-4289`），
    /// 但紧接着**调用方**又把它钉住——`focusColumn = getColumn(new Point(rowHeaderWidth + 1, 1))`、
    /// `focusItem = getTopIndex()`（`:3183-3184`）。行头点整行（`:3140-3145`）、
    /// 列头点整列（`:3201`）是同族口径。
    ///
    /// `CellRange::cursor` 承载「矩形铺满 + 当前格另置」：矩形照旧铺满整表（行号槽整列
    /// 靠它铺色），当前格另存。组件侧的 `selected_cell` 由调用方用
    /// `set_selected_cell_without_scroll` 同步（DBeaver 这两处都不滚视口）。
    ///
    /// `top_row` = 视口首行（组件 `visible_range().rows.start`，等价 DBeaver 的
    /// `getTopIndex()`）；越界在这里夹住（空表已被上面的 guard 挡掉，`rows >= 1`）。
    pub fn select_all_cells(&mut self, top_row: usize) -> bool {
        let rows = self.view_len();
        // 空态占位行不算数据（`set_data` 的注释）：DBeaver 那边 `getItemCount()` 是 0
        if rows == 0 || self.columns.is_empty() || self.placeholder_row {
            return false;
        }
        let last = self.last_view_column();
        // 【D10-10】矩形铺满整表，当前格另置到「视口首行 × 首个数据列」——DBeaver
        // `:3183-3184`：`focusColumn = getColumn(new Point(rowHeaderWidth + 1, 1))`、
        // `focusItem = getTopIndex()`。行号槽整列仍靠矩形铺色（`cols()` 含列 0），
        // 所以这里不需要把矩形收窄（`CellRange::cursor` 就是为此而设）。
        let cursor = if last >= 1 {
            Some((top_row.min(rows - 1), 1))
        } else {
            None
        };
        self.cell_range = Some(CellRange {
            anchor: (0, 0),
            focus: (rows - 1, last),
            cursor,
        });
        true
    }

    /// 【D10-6】方向键移动焦点格（DBeaver `LightGrid.java:3738-3841` 的 `onKeyDown`）。
    ///
    /// DBeaver 的键导航真值是 `focusColumn/focusItem`（`:3739-3744`：焦点列缺失时拿
    /// `columns[0]` 兜底）。四向的具体走法：
    ///
    /// - → / ←（`:3773-3801`）：行不动，列 ±1；`index >= size` / `index == 0` 时
    ///   `newColumnFocus = focusColumn` —— 即**到头停在原列**，不绕回另一端；
    /// - ↑ / ↓（`:3803-3821`）：列不动，行 ±1；↓ 在「还没有焦点行」时落到第 0 行；
    /// - Home / End（`:3822-3841`）：列钉首列 / 末列，行端只在
    ///   `ctrlPressed || columns.size() == 1` 时才甩到两端。
    ///
    /// 本仓的列序是 `[行号槽(0)] + 可见数据列`，而 DBeaver 的 `columns` **不含行头**，
    /// 所以「首列」= 视图列 1、「末列」= `last_view_column()`（转置视图恒 2，天然可用）。
    /// 起点取矩形 `focus`（没有矩形时按 `(0, 首列)`，等价于 DBeaver 的焦点列兜底）。
    ///
    /// `extend`（Shift）保留锚点、只挪焦点（与 `click_cell` 的 Shift 档同义）；否则新格成单格选区。
    ///
    /// 返回新焦点格（调用方据此投影状态行 / 复制目标）；`None` = 这一下没有位移（空表 /
    /// 只有占位行 / 撞到边界 / Home·End 落回原格），调用方不必刷新。
    ///
    /// 「还没有选区」时任何方向键都先把焦点落到 `(0, 首列)` 并建立选区（DBeaver 的
    /// `focusColumn` 兜底 `:3739-3744` + ↓ 的 `newSelection = 0` 特例 `:3814-3817`）；
    /// 有选区且撞到边界时同格不动，返回 `None`（DBeaver 那边 `newColumnFocus` 保持原列，
    /// 仍会发一次同格的选择事件——本仓省掉这次空刷新）。
    pub fn step_focus(&mut self, step: GridKeyStep, extend: bool) -> Option<(usize, usize)> {
        let rows = self.view_len();
        let last_col = self.last_view_column();
        if rows == 0 || self.columns.is_empty() || self.placeholder_row || last_col == 0 {
            return None;
        }
        let had_range = self.cell_range.is_some();
        // 起点读**当前格**（DBeaver 的 `focusColumn/focusItem`），而不是矩形的 `focus`：
        // 行号槽点整行之后两者列位置不同（`cursor` = 首数据列、`focus` = 末列）。
        let (cur_row, cur_col) = match self.cell_range {
            Some(range) => {
                let (row, col) = range.cursor();
                (row.min(rows - 1), col.clamp(1, last_col))
            }
            None => (0, 1),
        };
        let (row, col) = match step {
            GridKeyStep::Left => (cur_row, cur_col.saturating_sub(1).max(1)),
            GridKeyStep::Right => (cur_row, (cur_col + 1).min(last_col)),
            GridKeyStep::Up => (cur_row.saturating_sub(1), cur_col),
            GridKeyStep::Down => ((cur_row + 1).min(rows - 1), cur_col),
            GridKeyStep::Home { row_to_start } => (if row_to_start { 0 } else { cur_row }, 1),
            GridKeyStep::End { row_to_end } => {
                (if row_to_end { rows - 1 } else { cur_row }, last_col)
            }
            // 【D10-7】PageUp / PageDown：列不动，行落到调用方算好的视口目标行；
            // 越过数据末行由这里统一夹住（与 Home/End 同口径）。
            GridKeyStep::PageUp { target_row } | GridKeyStep::PageDown { target_row } => {
                (target_row.min(rows - 1), cur_col)
            }
        };
        if had_range && (row, col) == (cur_row, cur_col) {
            return None;
        }
        let anchor = if extend {
            self.cell_range.map_or((row, col), |range| range.anchor)
        } else {
            (row, col)
        };
        self.cell_range = Some(CellRange {
            anchor,
            focus: (row, col),
            // 键盘移动后当前格就是新格（DBeaver 的 `focusColumn/focusItem` 同步更新），
            // 不需要独立的 cursor。
            cursor: None,
        });
        Some((row, col))
    }

    /// 最后一个可见组件列的下标（含行号槽那列）。
    ///
    /// 组件列序 = `[行号槽(0)] + 可见数据列`，所以末列下标 = 可见数据列数
    /// （`table_impl::columns_count` 的 `columns.len() + 1 - hidden.len() - 1` 同值）；
    /// 转置视图固定三列（`#` / 字段 / 值），末列恒为 2。没有结果时只有行号槽位，
    /// 返回 0（此时选区本来也只有一格）。
    fn last_view_column(&self) -> usize {
        if self.columns.is_empty() {
            0
        } else if self.transposed {
            2
        } else {
            self.visible_column_count()
        }
    }

    /// 【D10】当前选区（`None` = 还没选过任何格 / 已被清空）
    pub fn cell_range(&self) -> Option<CellRange> {
        self.cell_range
    }

    /// 【D10】该格是否在矩形里（`render_td` 铺色用）
    pub fn in_cell_range(&self, row: usize, col: usize) -> bool {
        self.cell_range
            .is_some_and(|range| range.contains(row, col))
    }

    /// 【D10】当前格 = 最近一次点到的格。组件 overlay 自己画它，delegate 不再重复铺色。
    /// 【D10-9】读 `CellRange::cursor()`：行号槽点整行等「矩形铺满但当前格另置」的档由它表达。
    pub fn range_focus(&self) -> Option<(usize, usize)> {
        self.cell_range.map(|range| range.cursor())
    }

    /// 【D10】选区覆盖的数据列（视图列下标升序，已去掉行号槽）——计算页「按列分组」读它
    /// （`AggregateColumnsPanel.java:231-249`）。
    pub fn selected_data_columns(&self) -> Vec<usize> {
        self.cell_range
            .map(|range| range.data_columns())
            .unwrap_or_default()
    }

    /// 【D10】清空选区（`TableEvent::ClearSelection` / 换结果集时调）
    pub fn clear_cell_range(&mut self) {
        self.cell_range = None;
    }

    /// 【结果集 #296】当前显示序（列名数组；数据序 = 没拖过）。面板在
    /// `TableEvent::MoveColumn` 订阅里读这份快照做持久化。
    pub fn display_order(&self) -> Vec<String> {
        self.display_order.clone()
    }

    /// 【分页】当前视口首行（视图行序，0 基）；翻页按钮据此算上 / 下一页目标。
    pub fn visible_top(&self) -> usize {
        self.visible_top
    }

    /// 【D10-7 翻页键】当前视口完整可见的行数（0 = 首帧测量前未知）。
    ///
    /// 键盘 `PAGE_UP` / `PAGE_DOWN` 的一页步长取自它，与状态栏翻页钮的
    /// 目标行算法共用同一份视口事实（不另造页大小常量）。
    pub fn visible_len(&self) -> usize {
        self.visible_len
    }

    /// 【分页】当前视图行数（筛选 / 排序后的可见行数；转置视图为可见字段数）。
    /// 与 `TableDelegate::rows_count` 同口径，供宿主翻页算「末页」——不引 trait 到宿主。
    pub fn view_total(&self) -> usize {
        if self.transposed {
            self.transpose_columns().len()
        } else {
            self.view_len()
        }
    }

    /// 换一批数据（结果集变化时从权威拷贝一次；网格不持有第二份真值）
    pub fn set_data(&mut self, columns: Vec<String>, rows: Vec<Vec<String>>) {
        // 【空态】0 行时塞一行全空数据：DataTable 组件在 rows=0 时切到 render_empty 不画表头，
        // 用户要求空结果集也要看到字段名（DBeaver 口径）。塞一行空行让组件画表头 + 空白数据区，
        // 空态文案在状态栏/工具栏显示，不在网格里画。
        //
        // 【D10-3】这一行是**占位**不是数据：记到 `placeholder_row`，全选据此按 DBeaver
        // `getItemCount() == 0` 的口径什么都不选（状态行也就不会把空结果报成 1 行）。
        let placeholder_row = rows.is_empty() && !columns.is_empty();
        let rows = if placeholder_row {
            vec![vec![String::new(); columns.len()]]
        } else {
            rows
        };
        self.columns = columns;
        self.rows = rows;
        self.placeholder_row = placeholder_row;
        // 【D10】换了一批数据，矩形选区的旧坐标不再指向同一批格（DBeaver 重新取数
        // 也会清 `selectedCells`），连同整行镜像一起复位
        self.cell_range = None;
        self.selected_row = None;
        // 【结果集 #296】新结果 = 新一批列：显示序重置为数据序（恢复重排在面板层做——
        // set_data 前按持久化列名序重排 columns，这里以重排后的为准）
        self.display_order = self.columns.clone();
        // 【列序】显示序换了名字集合 → 折算缓存必须重建（`data_column` 读它）
        self.rebuild_column_order();
        self.empty_text = "查询返回 0 行".to_string();
        // 换了一份结果，旧的排序与筛选不该跟过来（列都可能不是同一批）
        self.sort = None;
        // 冻结也一样：列都换了一批，钉子留在旧列号上没有意义
        self.frozen.clear();
        // 【P2】隐藏列同理：换结果 = 换一批列，旧的隐藏清单不再成立
        self.hidden.clear();
        // 【P1-5】列宽按列名记：新结果列集合变了，旧宽度不成立（与隐藏 / 冻结同口径）
        self.column_widths.clear();
        // 【P2-4】转置同理：行与列都换了，旧转置行没有意义（面板侧开关状态不随数据自动关）
        self.transposed = false;
        self.transpose_row = RowRef::Data(0);
        self.context_cell = None;
        // 【选中行】换了一份结果，旧的整行选中跟着作废（行号指向的是上一份的行）
        self.selected_row = None;
        // 【T2a-3/4】数据被新结果顶掉：正在编辑的格子、标脏的集合都随旧数据作废
        self.editing = None;
        self.dirty.clear();
        self.conflict.clear();
        // 类型是**这一份结果自己的**：先把上一份的清掉，等面板紧跟着给新的一份
        // （漏给就是“没类型”，不会错套到新数据上）
        self.column_types.clear();
        // 【P1】注释同理（面板随后紧跟 set_column_annotations；漏给 = 表头只画列名）
        self.column_annotations.clear();
        // 【A9】来源表名同理（面板随后紧跟 set_result_table；漏给 = 不摆 INSERT 项）
        self.result_table = None;
        // 【B5】事实帧同理：换结果不套上一份的列式数据（面板随后紧跟 set_fact；
        // 漏给 = 本地统计 / 排序回退显示矩阵口径）
        self.fact = None;
        // 【#28】本地叠加层同理：换结果不套上一份的本地行（面板随后紧跟 set_local_snapshot）
        self.local = None;
        self.rebuild_view_rows();
        // 【文本视图】换数据 = 文本输出失效（行数 / 列数可能没变但内容变了）
        *self.text_cache.borrow_mut() = None;
    }

    /// 同步每列的类型（**与 `set_data` 同一次同步里、在它之后调**）
    ///
    /// 为什么是单独一次而不是 `set_data` 加参数：`set_data` 的签名上挂着面板的现有调用，
    /// 而类型今天还只能从另一条路来（上游填上驱动类型之前它一直是空）。
    /// 顺序反了（先给类型再 `set_data`）会被清掉——**不会**把上一份的类型套到新数据上。
    pub fn set_column_types(&mut self, types: Vec<Option<ColumnType>>) {
        self.column_types = types;
    }

    /// 【P1】同步每列的元数据注释（与 `set_data` 同一次同步里、在它之后调）
    ///
    /// 与 [`Self::set_column_types`] 同款时序：`set_data` 清掉上一份的注释，
    /// 面板紧跟给新的一份；漏给 = 表头只画列名，不套旧注释。
    pub fn set_column_annotations(&mut self, annotations: Vec<Option<ColumnAnnotation>>) {
        self.column_annotations = annotations;
    }

    /// 【DBeaver 对齐】设置页改「列头附加内容」时同步：三档决定列头第二行画
    /// 注释 / 类型名 / 什么都不画。
    /// 不随 `set_data` 清掉——这是面板视图偏好，不是这份结果自己的元数据。
    pub fn set_column_header_extra(&mut self, extra: ColumnHeaderExtra) {
        self.column_header_extra = extra;
    }

    /// 当前列头附加内容档位（`header.rs` 据此选第二行文本）。
    pub fn column_header_extra(&self) -> ColumnHeaderExtra {
        self.column_header_extra
    }

    /// 【选中统计】面板同步三项开关镜像（右键菜单据此标勾选态）。
    /// 与 [`Self::set_column_header_extra`] 同口径：面板视图偏好，不随 `set_data` 清掉。
    pub fn set_selection_stat_prefs(&mut self, prefs: crate::grid::SelectionStatPrefs) {
        self.selection_stat_prefs = prefs;
    }

    /// 【布局段】面板同步呈现方式镜像（右键菜单给「网格 / 文本」标勾选态）。
    pub fn set_presentation_view(&mut self, presentation: Presentation) {
        self.presentation_view = presentation;
    }

    /// 【布局段】当前镜像的呈现方式（右键菜单构造 `ContextTarget` 时读）。
    pub fn presentation_view(&self) -> Presentation {
        self.presentation_view
    }

    /// 【布局段】同步「筛选条显隐」镜像（面板态；右键菜单标勾选态用）。
    pub fn set_show_filter_panel(&mut self, show: bool) {
        self.filter_panel_hidden = !show;
    }

    /// 【布局段】当前镜像的筛选条显隐（`true` = 显示）。
    pub fn show_filter_panel(&self) -> bool {
        !self.filter_panel_hidden
    }

    /// 表头带该多高（`None` = 用组件默认，即 [`crate::ui::RESULT_TABLE_SIZE`] 档的行高 26px）。
    ///
    /// 只有「列头附加内容」非 `Nothing`（注释 / 类型名）时才抬：那时表头是**两行**
    /// （列名主行 + 附加小字），组件默认的 26px 减掉 `table_cell_padding` 上下各 2px
    /// 只剩 22px 内容高，两行各被裁一半——主行会被压到读不出字
    /// （2026-11-08「字段被遮挡」就是这一族问题的极端形态：列头菜单当时还是第二行，
    /// 直接把主行挤成 0）。单行表头一个像素都不动，默认视觉不变。
    ///
    /// DBeaver 侧表头行高由渲染器按需算（`GridColumnRenderer` 在两行内容下自己
    /// 报更大 preferred size），本仓是固定抬到两行够用的常量——口径相同
    /// （「有第二行才变高」），数值来源见 [`crate::ui::RESULT_HEADER_COMMENT_ROW_HEIGHT`]。
    ///
    /// 由宿主在渲染表格时读给 `DataTable::header_row_height`（`grid/status.rs`）。
    pub(crate) fn header_row_height(&self) -> Option<Pixels> {
        self.column_header_extra
            .shows_extra_line()
            .then_some(crate::ui::RESULT_HEADER_COMMENT_ROW_HEIGHT)
    }

    /// 【类型样式】同步全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    ///
    /// 与 [`Self::set_column_header_extra`] 同口径：**面板视图偏好**，不随 `set_data` 清掉；
    /// 由面板在构造、`set_data` 同步与偏好变更（workbench 设置钩子）时调用。
    pub fn set_type_style(&mut self, prefs: TypeStylePrefs) {
        self.type_style = prefs;
    }

    /// 【P1-5】记下组件拖宽后报来的全部列宽（会话内记忆；不做落盘持久化，见 backlog）。
    ///
    /// `all` 下标是组件视图列序（`0` = 行号槽，钉死不记）；其余先经
    /// [`Self::data_column`] 落到数据列、再按**列名**存——隐藏列调整后回填仍对得上。
    /// 越界 / 列名取不到就跳过那一列，不打断别的列。
    pub fn set_column_widths(&mut self, all: &[Pixels]) {
        for (ix, width) in all.iter().enumerate() {
            let Some(view_col) = ix.checked_sub(1) else {
                continue;
            };
            let data = self.data_column(view_col);
            let Some(name) = self.columns.get(data).cloned() else {
                continue;
            };
            self.column_widths.insert(name, *width);
        }
    }

    /// 【P1-3】当前会话内列宽快照（列名 → 像素宽），供宿主落进会话。
    ///
    /// 只含**用户拖过**的列：没拖过的列不进表（恢复时自然回默认档，也避免把
    /// 组件库测量出的列宽当成用户意图）。
    pub fn column_width_snapshot(&self) -> Vec<(String, f32)> {
        self.column_widths
            .iter()
            .map(|(name, width)| (name.clone(), f32::from(*width)))
            .collect()
    }

    /// 【P1-3】按列名回填持久化列宽（`set_data` 之后调：它会清空列宽表）。
    ///
    /// 列名对不上就跳过那一列（结果换表 / 列被重命名时按名字对不上，不按位置硬套）。
    pub fn apply_column_width_snapshot(&mut self, widths: &[(String, f32)]) {
        for (name, width) in widths {
            if self.columns.iter().any(|column| column == name) {
                self.column_widths.insert(name.clone(), gpui_kit::px(*width));
            }
        }
    }

    /// 【列布局落盘】当前隐藏列快照（**列名**，不是下标），供宿主落进会话。
    ///
    /// 用列名而不是下标：列序会被持久化列序重排（`set_data` 时物理重排一次），
    /// 下标跨会话不稳定；列名对不上就跳过（`apply_hidden_columns`）。
    pub fn hidden_column_snapshot(&self) -> Vec<String> {
        self.hidden
            .iter()
            .filter_map(|data_col| self.columns.get(*data_col).cloned())
            .collect()
    }

    /// 【列布局落盘】按列名回填隐藏列（`set_data` 之后调：它会清空隐藏清单）。
    ///
    /// 逐列走 [`Self::hide_column`]——「至少留 1 列可见」的底线与交互路径同一份实现；
    /// 找不到的列名静默跳过（结果换了表 / 列被重命名）。
    pub fn apply_hidden_columns(&mut self, names: &[String]) {
        for name in names {
            if let Some(data_col) = self.columns.iter().position(|column| column == name) {
                self.hide_column(data_col);
            }
        }
    }

    /// 【列布局落盘】当前冻结列快照（**列名**，口径同 [`Self::hidden_column_snapshot`]）
    pub fn frozen_column_snapshot(&self) -> Vec<String> {
        self.frozen
            .iter()
            .filter_map(|data_col| self.columns.get(*data_col).cloned())
            .collect()
    }

    /// 【列布局落盘】按列名回填冻结列（`set_data` 之后调：它会清空冻结清单）。
    ///
    /// 冻结是集合语义（不是顺序），重复列名只钉一次；找不到的列名静默跳过。
    pub fn apply_frozen_columns(&mut self, names: &[String]) {
        for name in names {
            if let Some(data_col) = self.columns.iter().position(|column| column == name) {
                if !self.frozen.contains(&data_col) {
                    self.frozen.push(data_col);
                }
            }
        }
        self.frozen.sort_unstable();
    }

    /// 【列布局落盘】注入动作队列（面板接线时调用一次）：列头菜单 / 列头右键的「冻结」
    /// 经它交回宿主分派——UI 回调里直接改 delegate 就没有落库触发点。
    pub fn set_action_queue(&mut self, queue: crate::ports::ResultActionQueue) {
        self.action_queue = Some(queue);
    }

    /// 【A9】同步结果来源表名（与 `set_data` 同一次同步里、在它之后调）
    ///
    /// `None` = 从来源 SQL 猜不到 → 右键「复制整行为 INSERT」不摆（宁可没有，
    /// 也不给一条插到错误表的语句）。
    pub fn set_result_table(&mut self, table: Option<String>) {
        self.result_table = table;
    }

    /// 【B5】同步事实帧只读句柄（与 `set_data` 同一次同步里、在它之后调，与类型 / 注释同口径）。
    ///
    /// 面板只传共享句柄，不克隆 batches；本地数值统计 / 原生数值排序在交互派生时读取，
    /// render 纯读路径不解析它。表档帧 batches 已释放，调用侧按空批回退矩阵口径。
    pub fn set_fact(&mut self, fact: Option<Arc<FactFrame>>) {
        self.fact = fact;
    }

    /// 【#28】同步本地叠加层投影快照（与 `set_data` 同一次同步里、在它之后调，与
    /// 类型 / 事实帧同口径时序）。
    ///
    /// delegate 只读这份投影、不持真值：新增 / 复制 / 删除动作由面板经事件回写 store 槽位，
    /// 再由宿主重新投影进来。`None` = 纯底层结果集（未接行级动作时行为与现状一致）。
    /// 注入后立即重算视图序列：本地行追加在底层行之后（筛选命中才出现，排序不参与）。
    pub fn set_local_snapshot(&mut self, local: Option<LocalRowSet>) {
        self.local = local;
        // 【文本视图】本地行快照替换（含格内容编辑回写）不改变指纹里的行数，须显式失效
        *self.text_cache.borrow_mut() = None;
        self.rebuild_view_rows();
    }

    /// 当前投影里的本地行个数（不参与底层行数 / 统计口径，仅供视图序列长度与导航夹取）
    pub(crate) fn local_count(&self) -> usize {
        self.local.as_ref().map_or(0, |set| set.local_count())
    }

    /// 清空（切到无结果的模式 / 关闭文档时）
    pub fn clear(&mut self, text: impl Into<String>) {
        self.columns.clear();
        self.rows.clear();
        // 【D10-3】没行也没列时不存在「空态占位行」
        self.placeholder_row = false;
        self.column_types.clear();
        // 【A9】来源表名跟着清（没有结果就没有 INSERT 目标）
        self.result_table = None;
        // 【B5】事实帧跟着清：空网格不套上一份的列式数据
        self.fact = None;
        // 【#28】本地叠加层跟着清：空网格不套上一份的本地行
        self.local = None;
        self.view_rows = None;
        self.empty_text = text.into();
        // 没有结果就没有“下一段”：留着会让滚动到底去取一份已经不存在的结果
        self.has_more = false;
        self.loading_more = false;
        // 【T2a-3/4】结果没了，编辑态与脏格一起作废
        self.editing = None;
        self.dirty.clear();
        self.conflict.clear();
        // 【P2-4】转置行身份复位（空结果里没有可竖排的行）
        self.transposed = false;
        self.transpose_row = RowRef::Data(0);
        // 【D10】选区跟着作废：换结果集后旧坐标指向的是别的行（DBeaver 的
        // `clearGrouping` / 重新取数都会把选区的 `selectedCells` 清掉）
        self.cell_range = None;
        self.selected_row = None;
    }

    /// 【B14】记下最近右键的单元格（由 `render_td` 上的右键处理调用）
    pub fn set_context_cell(&mut self, cell: Option<(usize, usize)>) {
        self.context_cell = cell;
    }

    /// 【B14】注入「按值筛选」钩子（面板构造时一次）
    pub fn set_filter_value_hook(&mut self, hook: FilterValueHook) {
        self.on_filter_value = Some(hook);
    }

    /// 【筛选】注入「列级筛选」钩子（面板构造时一次；列头菜单据此把动作交回面板）
    pub fn set_column_filter_hook(&mut self, hook: ColumnFilterHook) {
        self.on_column_filter = Some(hook);
    }

    /// 【M8】注入「洞察此列」钩子（宿主接了端口时面板构造一次）
    pub fn set_insight_column_hook(&mut self, hook: InsightColumnHook) {
        self.on_insight_column = Some(hook);
    }

    /// 【遗留 #7】注入「查看完整值」钩子（面板构造时一次）
    pub fn set_preview_value_hook(&mut self, hook: PreviewValueHook) {
        self.on_preview_value = Some(hook);
    }

    /// 【P2-3】宿主注入列统计弹层的打开钩子（没接 = 菜单不摆这一项）
    pub fn set_column_stats_hook(&mut self, hook: ColumnStatsHook) {
        self.on_column_stats = Some(hook);
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

    /// 【结果集二次分析】注入「分析此结果集」钩子（宿主接了出口时面板构造一次）
    pub fn set_analyze_result_set_hook(&mut self, hook: AnalyzeResultSetHook) {
        self.on_analyze_result_set = Some(hook);
    }

    /// 【结果集二次分析】测试用：钩子装上了吗
    pub fn has_analyze_result_set_hook(&self) -> bool {
        self.on_analyze_result_set.is_some()
    }

    /// 【结果集二次分析】入口现在该不该出现：钩子在 + 视图里有行。
    ///
    /// 不看 `insight_available`（那要求结果绑定连接）：整份结果物化成 `tmp_q_` 是
    /// **本地动作**，与结果来自哪条连接无关——本地拼出来的结果也能再分析。
    pub fn analyze_result_set_available(&self) -> bool {
        self.on_analyze_result_set.is_some() && self.view_len() > 0
    }

    /// 【结果集持久化】注入「持久化为项目表」钩子（宿主接了持久化出口时面板构造一次）
    pub fn set_persist_result_set_hook(&mut self, hook: PersistResultSetHook) {
        self.on_persist_result_set = Some(hook);
    }

    /// 【结果集持久化】入口现在该不该出现：钩子在 + 视图里有行。
    ///
    /// 与「分析此结果集」同一判据；「有没有打开项目」由宿主决定接不接钩子
    /// （未打开项目时宿主播不了，就不该给一个点了只报错的入口）。
    pub fn persist_result_set_available(&self) -> bool {
        self.on_persist_result_set.is_some() && self.view_len() > 0
    }

    /// 【结果集持久化】测试用：钩子装上了吗
    pub fn has_persist_result_set_hook(&self) -> bool {
        self.on_persist_result_set.is_some()
    }

    /// 【B14】注入「排序下发」钩子（面板构造时一次）
    pub fn set_sort_down_hook(&mut self, hook: SortDownHook) {
        self.on_sort_down = Some(hook);
    }

    /// 【B15】这一数据列冻结了吗
    pub fn is_frozen(&self, column: usize) -> bool {
        self.frozen.contains(&column)
    }

    /// 【筛选】这一数据列当前有没有列级筛选生效（列头菜单据此摆「清除此列筛选」）
    pub fn has_column_filter(&self, column: usize) -> bool {
        self.column_filter
            .as_ref()
            .is_some_and(|(filtered, _)| *filtered == column)
    }

    /// 【筛选】生效中的列级筛选（`(数据列, 值)`；`None` = 没有列筛选）
    pub fn column_filter(&self) -> Option<(usize, &str)> {
        self.column_filter
            .as_ref()
            .map(|(column, value)| (*column, value.as_str()))
    }

    /// 【筛选】设置列级筛选（本地包含匹配，只改视图行序；`None` = 清除）
    pub fn set_column_filter(&mut self, column: Option<(usize, String)>) {
        self.column_filter = column;
        self.rebuild_view_rows();
    }

    /// 【筛选】这一列的去重取值（列头菜单值列表）：已抓行里按出现顺序去重，最多
    /// `COLUMN_FILTER_VALUE_LIMIT` 个；列越界 = 空切片。只读行数据，不碰网格状态。
    pub fn column_distinct_values(&self, column: usize) -> Vec<String> {
        column_distinct_values(&self.rows, column, COLUMN_FILTER_VALUE_LIMIT)
    }
}
