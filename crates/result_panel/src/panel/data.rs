//! 结果面板实体（`ResultPanel`）——结果区视图层从编辑器宿主整体迁入的落点
//!
//! 形态：DBeaver Data Editor 方向，停靠为 Workbench DockArea 的**底部 dock**（全局单
//! 实例，联动活动编辑器文档；有结果自动开）。本文件是实体层：**投影状态 + 渲染组装 +
//! 视图层动作**，不含执行 / 提交 / 导出写盘 / 分析跑数——那些仍归编辑器端口。
//!
//! ## 动作接线（Shared 事件队列，彻底解耦）
//!
//! - 依赖编辑器的动作（切结果集 / 并排对比 / 提交 / 撤销 / 复制 / 刷新 / 取段 / 导出 /
//!   分析 / 错误卡定位复制 / 下发源库重查）→ [`crate::ports::ResultActionQueue::push`]，
//!   编辑器在事件路径 `drain` 后分派到既有动作方法。
//! - 纯视图层动作（本地筛选输入 / 清除 / 转置开关 / 转置行导航 / 筛选统计回填）→ 面板
//!   内直接完成（只碰网格 delegate 与本实体状态，无 editor 依赖）。
//! - 网格内部交互（右键菜单 / 行编辑 / 列统计 / 值预览 / 加载更多）→ 保持
//!   [`crate::grid::ResultGridDelegate`] 既有注入钩子（跨 crate 安全），不经队列。
//!
//! ## 投影输入
//!
//! [`ResultPanel::sync`] 的判据（当前结果条目快照 / 可编辑性 / 连接文案 / 洞察可用 /
//! 错误定位 / 导出上下文 / 分析菜单项 / 提交态）由编辑器在事件路径算好以
//! [`ResultSyncInput`] 传入——面板只写状态，不反向依赖 editor。

use std::cell::RefCell;
use std::sync::Arc;

use gpui_kit::*;
use gpui_kit::base::TreeState;
use gpui_kit::component::list::ListState;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::table::TableState;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）
use ::shared::FactFrame;

use crate::column_browser::ColumnBrowserDelegate;
use crate::error_card::ErrorCard;
use crate::export;
use crate::grid::{ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{Presentation, ResultActionQueue, RightTab};
use crate::script_card::ScriptCard;
use crate::sets::ResultSetTab;
use workbench_shell::model::ColumnAnnotation;
/// 【类型样式】全局偏好聚合（shell 层：settings 持久化与渲染消费共用）。
use workbench_shell::result_style::TypeStylePrefs;

/// 一次投影里交到网格的数据（列 / 行 / 类型 / 来源表 / 字段注释；`set_data` 之后
/// 的类型与注释要紧接着设——`set_data` 会清空它们，见 [`crate::grid`] 实现）
#[derive(Debug, Clone)]
pub struct GridPayload {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_types: Vec<Option<crate::store::ColumnType>>,
    pub table: Option<String>,
    pub annotations: Vec<Option<ColumnAnnotation>>,
    /// 【B5】事实帧只读句柄（`None` = 无事实档 / 表档已释放 batches 回退矩阵口径）。
    /// 与类型 / 注释同口径：在 `set_data` 之后由 `set_fact` 注入。
    pub fact: Option<Arc<FactFrame>>,
    /// 【#28】本地叠加层快照（本地行 + 删除标记；`None` = 纯底层结果集）。
    /// 与事实帧同口径时序：在 `set_data` 之后由 `set_local_snapshot` 注入；对比栏不传。
    pub local: Option<crate::rowset::LocalRowSet>,
}

/// 分析菜单项（宿主算好传入：只含渲染要的文案与动作载荷；类型统一用
/// [`crate::analysis::AnalysisItem`]——编辑器侧的 `analysis::menu_items` 返回同一类型）
pub use crate::analysis::AnalysisItem;

/// 导出菜单的静态上下文（**render 期纯读素材**；键列查询由宿主在 sync（事件路径）算好）
#[derive(Debug, Clone)]
pub struct ExportCtx {
    /// 当前绑定连接文案（`SQL Updates` / `WhereClause` 菜单要不要摆的判据之一）
    pub connection: Option<String>,
    /// （SQL，结果集序号）——默认表名猜测的入参
    pub entry_sql: Option<(String, usize)>,
    /// 已抓行数文案（菜单分组标题「已抓取 N 行」）
    pub rows_text: String,
    /// 这份结果还能取下一段（「抓全量后导出」分组出现条件）
    pub has_more: bool,
    /// 键列（`SQL Updates` / `WhereClause` 菜单项出现条件；查不到 = `None`，
    /// 不摆那两项——宿主在 sync 时查表结构端口算好，render 期只读）
    pub key: Option<export::RowKey>,
}

/// 一次投影的全部判据（编辑器事件路径算好传入，面板只写状态）
#[derive(Debug, Clone, Default)]
pub struct ResultSyncInput {
    /// 结果集标签（标签条；两份以上才画）
    pub tabs: Vec<ResultSetTab>,
    /// 当前选中的结果集下标
    pub active: usize,
    /// 顶部提示行（切通道后「旧结果来自 X」）
    pub notice: Option<String>,
    /// 工具栏（⑥）：行数 / 影响行数 / 失败 / 耗时 / 连接 / 血缘
    pub toolbar: Option<ResultToolbar>,
    /// 状态行（⑦）：只有网格时才给
    pub status: Option<ResultStatus>,
    /// 主网格数据（`Some` = 有网格；`None` = 空结果 / 失败，用 `empty` 文案）
    pub grid: Option<GridPayload>,
    /// 无网格时的占位文案（空结果 / 失败时 delegate 清空显示它）
    pub empty: String,
    /// 错误卡片（失败时才有；有卡不画网格）
    pub error_card: Option<ErrorCard>,
    /// 脚本输出卡片（通用模式脚本结果）
    pub script_card: Option<ScriptCard>,
    /// 当前选中结果集的 SQL（「刷新」重跑它；`None` = 无可重跑）
    pub sql: Option<String>,
    /// 有没有可复制的东西（没网格不摆复制 / 转置 / 导出）
    pub can_copy: bool,
    /// 当前选中结果是本地分析产物（不摆「刷新」）
    pub is_analysis: bool,
    /// 这份结果能不能编辑（入口级判据；双击进编辑 / 标脏都看它）
    pub editable: bool,
    /// 脏格（待提交变更的格子）
    pub dirty: Vec<(usize, usize)>,
    /// 冲突格（提交没写进去的格子）
    pub conflict: Vec<(usize, usize)>,
    /// 还能取下一段
    pub has_more: bool,
    /// 正在取下一段（滚动到底自动加载的进行态）
    pub loading_more: bool,
    /// 「洞察此列」能不能给
    pub insight_available: bool,
    /// 待提交变更数（> 0 才摆提交 / 撤销对）
    pub pending_changes: usize,
    /// 提交中（提交按钮置灰，回执回来前不重复提交）
    pub committing: bool,
    /// 【#28-B3】组合撤销栈是否非空（格级 `EditHistory` 优先，空走行级 `RowHistory`；
    /// 宿主在 sync 时算好传入，面板只读以决定状态栏撤销钮禁用态）
    pub can_undo: bool,
    /// 【#28-B3】组合重做栈是否非空（与 [`Self::can_undo`] 对称）
    pub can_redo: bool,
    /// 分析菜单项（空 = 不摆分析按钮）
    pub analysis_items: Vec<AnalysisItem>,
    /// 导出菜单上下文（`None` = 不摆导出按钮）
    pub export_ctx: Option<ExportCtx>,
}

/// 结果面板实体
pub struct ResultPanel {
    /// 主网格（编辑器构造期创建并注入钩子，面板只持有并渲染 / 投影）
    pub(crate) grid: Entity<TableState<ResultGridDelegate>>,
    /// 对比栏网格（对比模式才渲染；数据由编辑器 `sync_compare_grid` 拷入）
    pub(crate) compare_grid: Option<Entity<TableState<ResultGridDelegate>>>,
    /// 动作队列（渲染期 `push`；编辑器事件路径 `drain` 分派）
    pub(crate) queue: ResultActionQueue,

    /// 投影状态（来自 [`ResultSyncInput`]，render 是纯读路径不再重算）
    pub(crate) tabs: Vec<ResultSetTab>,
    /// 标签条滚动句柄（标签随查询累积增多，传给 TabBar `track_scroll` 保持跨帧滚动）
    pub(crate) tabs_scroll: ScrollHandle,
    pub(crate) active: usize,
    pub(crate) notice: Option<String>,
    pub(crate) toolbar: Option<ResultToolbar>,
    pub(crate) status: Option<ResultStatus>,
    pub(crate) error_card: Option<ErrorCard>,
    pub(crate) script_card: Option<ScriptCard>,
    pub(crate) transposed: bool,
    pub(crate) transpose_row: usize,
    pub(crate) compare_mode: bool,
    /// 【2026-10-28 微调】SQL 预览**弹出窗口**开关：点击工具栏 SQL 文本框置 true，
    /// render 根挂浮层（遮罩 + 居中卡片 + 只读 Textarea）。**不用 kit Dialog**：
    /// `Dialog::new` 需 `&mut App`，而本面板 render 是 `&App`（host 手动调
    /// `result_panel.read(cx).render(cx)`）——等效浮层 + kit Textarea 满足"弹出窗口 +
    /// 文本组件"，Esc / 遮罩点击 / 关闭按钮三路关闭。
    pub(crate) sql_dialog_open: bool,
    /// 【2026-10-28 微调】SQL 预览弹窗里的只读 Textarea 实体（懒创建：`TextareaState::new`
    /// 需要 window + &mut App，也在事件路径建；SQL 变化时 `set_text` 刷新）。
    /// render 期 `Textarea::new(&entity)` 只需 &Entity，可直接挂浮层。
    pub(crate) sql_textarea: Option<Entity<TextareaState>>,
    /// 【1.2】筛选框展开态：点击筛选组展开按钮后，工具栏下方出现顶宽筛选条
    /// （输入框全宽 + ▶ 运行 + ✕ 清除 + wrapped SQL 预览）。默认收起。
    pub(crate) filter_expanded: bool,
    /// 【侧栏】左侧栏（网格 / 文本 / 记录竖条）开关；默认开（对齐 DBeaver 左缘
    /// 竖条常驻），状态栏左端开关可收起（完全收起 = 不占宽度）。
    pub(crate) show_left_sidebar: bool,
    /// 【侧栏】右侧辅助栏（列浏览器 / 值详情 / 计算 / 分组）开关；默认关
    /// （不挤网格，工具栏「列浏览器」可开）。列浏览器与值详情等共用这一栏、页签切换。
    pub(crate) show_value_sidebar: bool,
    /// 【2026-10-28 微调】右缘竖条可见性——右侧开关**三态**（比左侧多一个展开态）：
    /// `(edge_bar_visible, show_value_sidebar)` = (false,false)=收起 / (true,false)=窄展开
    /// （只显右缘竖条图标条）/ (true,true)=宽展开（竖条 + 右栏内容）。不变量：宽展开 ⟹
    /// 竖条可见。开关在状态栏右端（render_status_row 的 right_switch，三态循环点击）。
    pub(crate) edge_bar_visible: bool,
    /// 【侧栏丝滑】左右栏展开 / 收起动画代次（每次开合 +1；0 = 首帧免动画）。
    /// 动画 id 拼 epoch → with_animation 换 id 即重播（与 vendor sidebar 同机制）。
    pub(crate) left_sidebar_epoch: u64,
    pub(crate) right_sidebar_epoch: u64,
    /// 【值查看器】JSON 展开树状态（`tree::Tree` 组件承载；选中格变化时事件路径
    /// `rebuild_value_tree` 解析 + `set_items` 重建，render 只摆——JSON 解析不进 render）
    pub(crate) json_tree: Entity<TreeState>,
    /// 【侧栏】最近选中格位置（**视图行**, 含行号槽列；宿主 `TableEvent::SelectCell`
    /// 事件写入，选行 / 清选置 `None`）。右栏值详情据此读网格取完整值 / 列名 / 类型。
    pub(crate) selection_pos: Option<(usize, usize)>,
    /// 【结果集 #296】持久化的列序（列名数组；空 = 默认序）。宿主在构造期注入
    /// （重启恢复），`TableEvent::MoveColumn` 订阅更新，并经
    /// `ResultRequest::ColumnOrderChanged` 回写宿主落库。换新结果时按列名匹配重排。
    pub(crate) column_order: Vec<String>,
    /// 【列浏览器】清单当前选中列（**数据列序**，与隐藏 / 冻结同一套下标空间）。
    /// 清单单击写入；网格选中格经 `selection_pos` 反推同步；新结果集 / 切标签 /
    /// 对比模式重置为 `None`（渲染兜底第一可见列）。清单选中 ≠ 网格焦点。
    pub(crate) selected_col: Option<usize>,
    /// 【列浏览器】清单 List 实体（kit `list::{ListState, ListDelegate}` 承载；
    /// 选中高亮由 List 内部 `selected_index` 管理，搜索框也在 List 内置里——
    /// 面板不再自持搜索输入框 / 搜索词，见 [`crate::column_browser`]）
    pub(crate) browser_list: Entity<ListState<ColumnBrowserDelegate>>,
    /// 【呈现】显示方式（DBeaver 左栏「显示方式」）：网格 / 文本。面板级持续——
    /// 切结果集不重置（用户选择的是「怎么看结果」，不是某份结果的属性）。
    pub(crate) presentation: Presentation,
    /// 【右栏】当前标签页（对齐 DBeaver 右侧面板组：列浏览器 / 值详情 / 计算 / 分组）
    pub(crate) right_tab: RightTab,
    /// 【并排对比】对比栏的投影（编辑器 `sync_compare_grid` 时经
    /// [`Self::set_compare_projection`] 写入；render 期只读）
    pub(crate) compare_active: usize,
    pub(crate) compare_toolbar: Option<ResultToolbar>,
    pub(crate) compare_status: Option<ResultStatus>,
    pub(crate) result_sql: Option<String>,
    /// 【1.2】筛选框运行生成的 wrapped SQL（`select * from (原SQL) where …`）；
    /// 执行完成后写入，右栏「执行SQL」页签展示（只读 + 复制）。空 = 页签空态。
    pub(crate) exec_sql: Option<String>,
    pub(crate) result_is_analysis: bool,
    pub(crate) result_can_copy: bool,
    pub(crate) fetching_more: bool,
    /// 导出菜单静态上下文（sync 写入；render 期只读）
    pub(crate) export_ctx: Option<ExportCtx>,
    /// 分析菜单项（sync 写入；render 期只读）
    pub(crate) analysis_items: Vec<AnalysisItem>,
    /// 待提交变更数（sync 写入；> 0 才摆提交 / 撤销对）
    pub(crate) pending_changes: usize,
    /// 提交中（sync 写入；提交按钮置灰）
    pub(crate) committing: bool,
    /// 【#28-B3】组合撤销 / 重做栈是否非空（sync 写入；状态栏撤销 / 重做钮禁用态据此）
    pub(crate) can_undo: bool,
    pub(crate) can_redo: bool,
    /// 【#28-B3】这份结果能不能编辑（sync 写入；行级新增 / 复制 / 删除钮据此决定是否注入）
    pub(crate) editable: bool,

    // 【B15】本地筛选框（视图层自洽：防抖 + 清除 + 统计回填都在面板内）
    pub(crate) filter_input: Entity<InputState>,
    pub(crate) filter_text: String,
    /// 防抖版本号（连打时任务被取代，只有最后一次到点）
    pub(crate) filter_version: u64,
    /// 在途防抖任务（持有以保活；换词即被取代）
    pub(crate) filter_debounce: RefCell<Option<Task<()>>>,
    /// 【B14】下发源库开关（开着时应用筛选会重查源库——重查经队列交回编辑器）
    pub(crate) pushdown: bool,
    /// 【C1】正在重命名哪个结果集 tab（`None` = 不在重命名态）；会话级
    pub(crate) renaming: Option<usize>,
    /// 【C1】重命名输入框实体（懒创建：第一次进重命名态时建；重命名态关闭后保留实体复用）
    pub(crate) renaming_input: Option<Entity<InputState>>,
    /// 【B1】列头直接显示注释的面板级临时开关（覆盖全局偏好 `result_panel.show_column_comments`；
    /// 默认 false——全局偏好开启时由 host 在 sync 时同步过来）。
    pub(crate) show_column_comments: bool,
    /// 【类型样式】全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    /// host 构造时同步（settings.result_panel.type_style()），设置页变更由 workbench
    /// 钩子推 `set_type_style` 即时生效；只影响渲染显示。
    pub(crate) type_style: TypeStylePrefs,
    /// 【DBeaver 对齐】状态栏中段行数输入框（可编辑，输入行号跳转；暂不接跳转逻辑）
    pub(crate) row_count_input: Entity<InputState>,
}