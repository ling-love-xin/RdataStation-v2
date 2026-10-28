//! 结果网格数据面（`grid/` 子目录第一块）：`ResultGridDelegate` 结构体 + 数据同步
//! （`set_data` 家族）与钩子注入。视图行序 / 编辑 / 取值等 impl 片段按域拆在
//! [`view_rows`] / [`edit`] / [`text`]——同一结构体的多个 impl 块（Rust 合法），
//! 字段 `pub(crate)` 供同 crate 各 impl 共享（2026-10-29 拆分，零逻辑改动）。

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;
use gpui_kit::component::input::InputState;

use ::shared::FactFrame;
use crate::rowset::{LocalRowSet, RowRef};
use crate::store::ColumnType;
use super::menu::column_distinct_values;
use super::view_rows::TextFingerprint;
use workbench_shell::model::ColumnAnnotation;
use workbench_shell::result_style::TypeStylePrefs;
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

/// 【遗留 #7】「查看完整值」的钩子（面板注入）：值 / 列名 / 数据列下标 / 视图行。
/// 弹层是宿主的事（对话框生命周期归宿主），网格只负责发意图。
pub type PreviewValueHook = Rc<dyn Fn(String, String, usize, usize, &mut App)>;

/// 【P2-3】列统计弹层的钩子（宿主开弹层；参数 = 列名、统计文本）
pub type ColumnStatsHook = Rc<dyn Fn(String, String, &mut App)>;

/// 【#28-B3】行级动作钩子（面板注入：把入队请求交回宿主分派；新增 / 复制 / 删除一行）。
/// 网格不持本地行真值，动作意图打包成 [`crate::ports::ResultRequest`] 经钩子入队，
/// 宿主在权威 store 槽位落地叠加层变更。
pub type RowActionHook = Rc<dyn Fn(crate::ports::ResultRequest, &mut App)>;
/// 网格数据（表头 + 行）
#[derive(Default)]
pub struct ResultGridDelegate {
    pub(crate) columns: Vec<String>,
    /// 【结果集 #296】列头拖拽后的**显示序**（列名数组；与 `columns` 无关——只记用户
    /// 拖出来的顺序，供面板持久化 / 恢复时按列名重排）。`move_column` 是组件拖拽
    /// 时调的 hook（`TableDelegate::move_column` 默认空），这里重排本数组即可；
    /// 组件自身的 `col_groups` 已让渲染立即按新序走，delegate 不需要改渲染。
    pub(crate) display_order: Vec<String>,
    pub(crate) rows: Vec<Vec<String>>,
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
    /// 【B1】列头是否直接显示注释（列名下方第二行小字；默认 false——注释只在 hover tooltip 里）。
    /// 由面板在切换「注释」开关时同步过来；开启后列头变高（两行：列名 + 注释小字）。
    pub(crate) show_column_comments: bool,
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
    /// 【B15】本地排序（列下标按**数据列**算，`0` = 第一数据列；`None` = 原顺序）
    pub(crate) sort: Option<(usize, bool /* 降序 */)>,
    /// 【文本视图】TSV 缓存：指纹命中直接复用，避免大结果集每次渲染全量重拼
    /// （bench：50k×8 一次拼 ~120ms，动画期间每帧重拼会卡住 UI——"点击文本
    /// 半天才出来"的根因）。指纹覆盖影响文本的所有输入（视图行数 / 列数 /
    /// 隐藏列 / 全局筛选 / 列筛选 / 排序），内容型变更（set_data、本地快照
    /// 替换）在入口显式失效，双保险不依赖逐个方法标脏。
    pub(crate) text_cache: std::cell::RefCell<Option<(TextFingerprint, String)>>,    /// 【B14】最近右键的单元格（视图行号，列下标含行号槽）——组件库没给访问器，
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
    /// 【T2a-4】提交回调（面板注入：落变更集）
    pub(crate) on_commit_edit: Option<CommitEditHook>,
    /// 【#28-B4】本地行格编辑提交回调（面板注入：写回本地行快照；不进变更集 / 撤销栈）
    pub(crate) on_commit_local_edit: Option<CommitLocalEditHook>,
    /// 【#28-B3】行级动作回调（面板注入：新增 / 复制 / 删除一行打包成请求交回宿主分派）
    pub(crate) on_row_action: Option<RowActionHook>,
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
}
impl ResultGridDelegate {
    /// 空网格：只带一句空态说明
    pub fn empty(text: impl Into<String>) -> Self {
        Self {
            empty_text: text.into(),
            ..Default::default()
        }
    }

    /// 【结果集 #296】当前显示序（列名数组；数据序 = 没拖过）。面板在
    /// `TableEvent::MoveColumn` 订阅里读这份快照做持久化。
    pub fn display_order(&self) -> Vec<String> {
        self.display_order.clone()
    }

    /// 换一批数据（结果集变化时从权威拷贝一次；网格不持有第二份真值）
    pub fn set_data(&mut self, columns: Vec<String>, rows: Vec<Vec<String>>) {
        // 【空态】0 行时塞一行全空数据：DataTable 组件在 rows=0 时切到 render_empty 不画表头，
        // 用户要求空结果集也要看到字段名（DBeaver 口径）。塞一行空行让组件画表头 + 空白数据区，
        // 空态文案在状态栏/工具栏显示，不在网格里画。
        let rows = if rows.is_empty() && !columns.is_empty() {
            vec![vec![String::new(); columns.len()]]
        } else {
            rows
        };
        self.columns = columns;
        self.rows = rows;
        // 【结果集 #296】新结果 = 新一批列：显示序重置为数据序（恢复重排在面板层做——
        // set_data 前按持久化列名序重排 columns，这里以重排后的为准）
        self.display_order = self.columns.clone();
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

    /// 【B1】面板切换「注释」开关时同步：列头是否直接显示注释（第二行小字）。
    /// 不随 `set_data` 清掉——这是面板视图偏好，不是这份结果自己的元数据。
    pub fn set_show_column_comments(&mut self, on: bool) {
        self.show_column_comments = on;
    }

    /// 【类型样式】同步全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    ///
    /// 与 [`Self::set_show_column_comments`] 同口径：**面板视图偏好**，不随 `set_data` 清掉；
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
