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

use gpui_kit::base::TreeState;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::list::ListState;
use gpui_kit::component::table::TableState;
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）
use ::shared::FactFrame;

use crate::column_browser::ColumnBrowserDelegate;
use crate::error_card::ErrorCard;
use crate::export;
use crate::grid::{ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{
    FilterSettingsTab, GroupSort, Presentation, ResultActionQueue, RightTab, StatsItem, StatsMode,
};
use crate::script_card::ScriptCard;
use crate::sets::ResultSetTab;
use workbench_shell::model::ColumnAnnotation;
/// 【类型样式】全局偏好聚合（shell 层：settings 持久化与渲染消费共用）。
use workbench_shell::result_style::TypeStylePrefs;

/// 【筛选历史】上限：DBeaver 那边是一张可滚动的历史表（`MAX_HISTORY_PANEL_HEIGHT` 封高），
/// 本仓只保最近 8 条——筛选词通常很短，8 条已覆盖「回到上一个筛选口径」这类真实用法。
pub(crate) const FILTER_HISTORY_LIMIT: usize = 8;

/// 【筛选设置 · 存档】用户显式「保存过滤器设置」记下的那一份筛选。
///
/// 对齐 DBeaver `DataFilterRegistry.SavedDataFilter`（`DataFilterRegistry.java:182-251`）：
/// 那边一整份存的是 `constraints`（逐列的可见 / 顺序 / 冻结 / 筛选值）+ `where` + `order`。
/// 本仓的逐列部分已有独立通道（`column_order` / `column_widths` / `column_hidden` /
/// `column_frozen`，都是**随会话自动存**），所以这里只补 DBeaver 那份里本仓缺的两样
/// ——**筛选词（where）与自定义排序（order）**——外加「这份存档属于哪份结果」的键。
///
/// 语义差别要记住：列布局是自动存，这一份是**用户点了「保存过滤器设置」才存**
/// （DBeaver 同款：`isDirty()` 才写，否则把旧键删掉，`DataFilterRegistry.java:273-288`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SavedFilterSetting {
    /// 存档时那份结果的 SQL：只有重新载入**同一条 SQL** 的结果时才套用。
    ///
    /// 口径比 DBeaver 窄：它按表对象路径存（`makeObjectId` 拼
    /// `catalog@@/@@schema@@/@@table`，`DataFilterRegistry.java:310-318`），
    /// 任何查这张表的语句都会套上；本仓按 SQL 收窄，避免把 A 查询的筛选串到 B 查询。
    pub sql: String,
    /// 筛选词（对应 DBeaver 的 `SavedDataFilter.getWhere()`；空 = 存档里没有筛选）
    pub filter: String,
    /// 自定义排序（对应 `SavedDataFilter.getOrder()`；空 = 沿用原查询的 ORDER BY）
    pub order: String,
    /// 存档时的来源表（`None` = 结果没有唯一来源表，如多表 join / 计算列）。
    ///
    /// DBeaver 只在 `viewer.getDataContainer() instanceof DBSEntity` 时才摆「保存/重置」
    /// （`ResultSetFilterPanel.java:1565-1570`）；本仓没有 `DBSEntity` 这一层，
    /// 用「有没有唯一来源表」等价表达——空着也允许存（用户按本仓口径仍能记住筛选）。
    pub table: Option<String>,
}

impl SavedFilterSetting {
    /// 【筛选存档落盘】编码成会话存档串（JSON）。
    ///
    /// 为什么不用列布局那套逗号串：这一份里带 `sql`（`select a, b from t` 本身就有逗号）
    /// 与筛选词（用户可能写 `in (1,2)`），逗号串必然拆错。JSON 顺带解决了转义问题；
    /// `#[serde(default)]` 让旧串缺字段也能解出来（宁可用空默认，也不整份丢弃）。
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 解码会话存档串。空串 / 非法 JSON → `None`（宁可不恢复，也不恢复半份错存档）。
    pub fn decode(raw: &str) -> Option<Self> {
        if raw.is_empty() {
            return None;
        }
        serde_json::from_str(raw).ok()
    }
}

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
    /// 【第 60 批】面板根焦点句柄：挂在根元素的 `track_focus` 上，作为文本态（网格不渲染、
    /// 焦点元素消失）时的焦点落点，让面板级键位（`Ctrl+反引号`）仍有 dispatch path。
    pub(crate) focus_handle: FocusHandle,
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
    /// 【Metadata 生成 DDL】SQL 预览浮层的标题：`None` = 原始 SQL（默认）,
    /// 有的值为「生成 DDL」这类复用同一浮层的场景（对齐 DBeaver 用 `ViewSQLDialog`
    /// 展示生成结果的做法，`MetaDataPanel.java:203-215` → `SQLGeneratorHandlerResultSetDDL`）。
    pub(crate) sql_dialog_title: Option<gpui_kit::SharedString>,
    /// 【第 72 批】「获取全部数据」确认浮层开关（DBeaver
    /// `CONFIRM_RS_FETCH_ALL`，`ResultSetViewer.readAllData:4433-4440`）。
    ///
    /// 与 `sql_dialog_open` / `filter_settings_open` 同一范式：面板内 absolute 遮罩 +
    /// 居中卡片，不是窗口级 dialog（理由见 `dialog.rs::render_filter_settings` 的注释）。
    /// 打开由宿主决定（判完「还有没有下一段」才来调 `open_fetch_all_confirm`），
    /// 面板只负责呈现与收口。
    pub(crate) fetch_all_confirm_open: bool,
    /// 【第 75 批】「跳转到列」浮层（DBeaver `CMD_GO_TO_COLUMN` → `askForColumnToGo`，
    /// `ResultSetHandlerMain:526-534 / 762-849`）。
    ///
    /// 与取全部确认同一范式（面板内 absolute 遮罩 + 居中卡片），但**带输入框**：
    /// `goto_column_input` 在构造期就建好（`InputState::new` 需要 `&mut Window`，
    /// 事件路径拿得到、render 拿不到），所以打开时只需回填 value + 交焦点。
    pub(crate) goto_column_open: bool,
    /// 列名 / 序号输入框（常驻；`PressEnter` 提交，`Change` 清掉上一轮的错误行）
    pub(crate) goto_column_input: Entity<InputState>,
    /// 上一次提交的失败原因（「未知列名」/「列已隐藏」），有值就在输入框下面显示
    pub(crate) goto_column_error: Option<gpui_kit::SharedString>,
    /// 输入框订阅（只持有；`PressEnter` 提交、`Change` 清错误）
    pub(crate) _goto_column_sub: Option<Subscription>,
    /// 【1.2】筛选框展开态：点击筛选组展开按钮后，工具栏下方出现顶宽筛选条
    /// （输入框全宽 + ▶ 运行 + ✕ 清除 + wrapped SQL 预览）。默认收起。
    pub(crate) filter_expanded: bool,
    /// 【DBeaver 对齐 `resultset.show.filterPanel`】工具栏左段整条筛选区是否渲染
    /// （SQL 预览 / 展开 / 输入 / 运行 / 清除 / 筛选设置 / 历史 / 后退前进 / 查找）。
    ///
    /// 等价于 DBeaver 对 `ResultSetFilterPanel` 整块的 `setVisible`
    /// （`ResultSetViewer.updateFilterPanelVisibility:2002-2012`）。出厂 `true`
    /// （`DataEditorsPreferencesInitializer.java:67`）；宿主构造期注入偏好，
    /// 之后数据格右键 layout 段的「显示过滤器面板」可切换并落盘。
    pub(crate) show_filter_panel: bool,
    /// 【侧栏】左侧栏（网格 / 文本 / 记录竖条）开关；默认开（对齐 DBeaver 左缘
    /// 竖条常驻），状态栏左端开关可收起（完全收起 = 不占宽度）。
    pub(crate) show_left_sidebar: bool,
    /// 【侧栏】右侧辅助栏（列浏览器 / 值详情 / 计算 / 分组）开关；默认关
    /// （不挤网格，工具栏「列浏览器」可开）。列浏览器与值详情等共用这一栏、页签切换。
    pub(crate) show_value_sidebar: bool,
    /// 【工具栏配置】状态栏里被隐藏的命令项（对齐 DBeaver 工具条配置的
    /// `defaultVisibility`：撤销 / 重做 / 取下一段默认隐藏，其余默认显示）。
    /// 纯视图状态（不落盘）；状态行末端的配置菜单可逐项勾回。
    pub(crate) hidden_status_commands: std::collections::BTreeSet<crate::ports::StatusCommand>,
    /// 【选中统计】状态行三项开关（对齐 DBeaver `resultset.show.selected.*`，出厂全关）。
    /// 由宿主构造期注入偏好，之后数据格右键的 layout 段可逐项切换；状态行
    /// `selection_statistics` 纯函数按它决定拼哪几项。
    pub(crate) selection_stat_prefs: crate::grid::SelectionStatPrefs,
    /// 【2026-10-28 微调】右缘竖条可见性——右侧开关**三态**（比左侧多一个展开态）：
    /// `(edge_bar_visible, show_value_sidebar)` = (false,false)=收起 / (true,false)=窄展开
    /// （只显右缘竖条图标条）/ (true,true)=宽展开（竖条 + 右栏内容）。不变量：宽展开 ⟹
    /// 竖条可见。开关在状态栏右端（render_status_row 的 right_switch，三态循环点击）。
    pub(crate) edge_bar_visible: bool,
    /// 【侧栏丝滑】左右栏展开 / 收起动画代次（每次开合 +1；0 = 首帧免动画）。
    /// 动画 id 拼 epoch → with_animation 换 id 即重播（与 vendor sidebar 同机制）。
    pub(crate) left_sidebar_epoch: u64,
    pub(crate) right_sidebar_epoch: u64,
    /// 【R5 可拖分栏】右栏当前宽度（默认 `RESULT_SIDEBAR_RIGHT_WIDTH` = 320px）。
    /// 拖右栏左边缘的 resize handle 改这个值（clamp 在 ui 的上下限内，见
    /// `crate::panel::sidebars::clamp_right_sidebar_width`）；开合动画仍以它为「满宽」。
    pub(crate) right_sidebar_width: Pixels,
    /// 【R5 可拖分栏】本轮宽度是否由拖拽得来：为真时右栏宽度**直写**（不走 160ms
    /// 开合补间，否则每帧拖拽都会触发一次补间、手柄黏手）。下一次点开关时清回 false，
    /// 让开合重新有动画。
    pub(crate) right_sidebar_dragging: bool,
    /// 【R5 布局方向切换】右栏在**右侧**还是**下方**（对齐 DBeaver `verticalLayout`，
    /// 默认 `false` = 网格左 / 右栏右）。为真时主体改竖排：网格在上、右栏满宽在下，
    /// 拖拽手柄改贴右栏顶边、位移按纵向算（`right_sidebar_height` 生效）。
    pub(crate) vertical_layout: bool,
    /// 【R5 面板最大化】为真时收起结果网格、右栏独占整块结果集区域（对齐 DBeaver
    /// `CMD_TOGGLE_MAXIMIZE` → `togglePanelsMaximize` = `viewerSash.hideUp()`）。
    /// 与 `show_value_sidebar` 正交：开合态原样保留，还原后回到用户原本的开合。
    pub(crate) panel_maximized: bool,
    /// 【计算行数】宿主算回的总行数文案（对齐 DBeaver `rowCountLabel`：数字**就地**
    /// 显示在状态行那颗钮上，而不是弹一个新结果集）。`None` = 还没算过，钮显示图标 +
    /// 「计算行数」；换新结果 / 换选中结果时清空（旧数字对新结果无意义）。
    pub(crate) row_count_note: Option<String>,
    /// 【R6 段大小】「取下一段」一次取多少行（状态行输入框可改；默认
    /// `ui::RESULT_SEGMENT_ROWS_DEFAULT`）。只影响后续取段，不改已加载行；
    /// 对应 DBeaver `resultSetSize` → 数据源偏好 `RESULT_SET_MAX_ROWS`。
    pub(crate) segment_rows: usize,
    /// 【R5 布局方向切换】竖排时右栏高度（默认 `RESULT_SIDEBAR_RIGHT_HEIGHT` = 240px）。
    /// **不进全局偏好项**：偏好项 `result_panel.sidebar_width` 是横向 px 宽度口径，
    /// 纵向高度共用一个值会互相串（偏离口径见 result-panel.md §8.46.2）。
    pub(crate) right_sidebar_height: Pixels,
    /// 【值查看器】JSON 展开树状态（`tree::Tree` 组件承载；选中格变化时事件路径
    /// `rebuild_value_tree` 解析 + `set_items` 重建，render 只摆——JSON 解析不进 render）
    pub(crate) json_tree: Entity<TreeState>,
    /// 【侧栏】最近选中格位置（**视图行**, 含行号槽列；宿主 `TableEvent::SelectCell`
    /// 事件写入，选行 / 清选置 `None`）。右栏值详情据此读网格取完整值 / 列名 / 类型。
    pub(crate) selection_pos: Option<(usize, usize)>,
    /// 【值查看器 P2-1】hex 转储当前页（0 基，每页 16 行 = 256 字节）。面板自持的纯
    /// 视图态：换选中格 / 清选时复位到 0（`set_selection_pos` 里做），翻页按钮经
    /// `ResultRequest::HexPage` 回来改它。值本身很大时不物化全部行，只取本页区间。
    pub(crate) hex_page: usize,
    /// 【P2-2】值查看器图片：选中 BLOB 且魔数认得出图片格式时，事件路径把 hex 解成
    /// `Arc<Image>`（`Image::from_bytes` 只是内存构造，真正解码在 `img` 的 layout/paint
    /// 期按内容 id 走资产缓存），render 只摆 `img`。非图片 / 认不出格式 → `None`。
    pub(crate) value_image: Option<Arc<Image>>,
    /// 【结果集 #296】持久化的列序（列名数组；空 = 默认序）。宿主在构造期注入
    /// （重启恢复），`TableEvent::MoveColumn` 订阅更新，并经
    /// `ResultRequest::ColumnOrderChanged` 回写宿主落库。换新结果时按列名匹配重排。
    pub(crate) column_order: Vec<String>,
    /// 【列浏览器】清单当前选中列（**数据列序**，与隐藏 / 冻结同一套下标空间）。
    /// 清单单击写入；网格选中格经 `selection_pos` 反推同步；新结果集 / 切标签 /
    /// 对比模式重置为 `None`（渲染兜底第一可见列）。清单选中 ≠ 网格焦点。
    pub(crate) selected_col: Option<usize>,
    /// 【P2-3】列浏览器多选集合（数据列序，升序）= delegate 集合的镜像：
    /// 选集权威在 [`crate::column_browser::ColumnBrowserDelegate`]（`List` 只给
    /// `confirm(secondary)`），面板存一份用于标题行「已选 N 列」与批量按钮。
    pub(crate) multi_selected: Vec<usize>,
    /// 【列浏览器】清单 List 实体（kit `list::{ListState, ListDelegate}` 承载；
    /// 选中高亮由 List 内部 `selected_index` 管理）。
    ///
    /// 【搜索框外置】DBeaver 的过滤框不在表里，而是页顶 `filterPanel` 的一格
    /// （`MetaDataPanel.java:81-89`：`GridLayout(2 列)` = Label「筛选：」+ `Text`），
    /// 所以本仓 `List` 不再开内置搜索（`searchable(false)`），改用面板自持的
    /// [`Self::browser_filter_input`] 驱动 `ListState::set_query`。原口径「搜索框在内置里」
    /// 是 §25 取证时的形态，D2 对齐后作废。
    pub(crate) browser_list: Entity<ListState<ColumnBrowserDelegate>>,
    /// 【列浏览器·筛选框】页顶 `filterPanel` 的输入框（常驻实体：`InputState::new` 需要
    /// `&mut Window`，构造期建好）。`InputEvent::Change` → `browser_list.set_query`，
    /// `Esc` → 清空（对齐 `MetaDataPanel.java:87-98` 的 KeyListener）。
    pub(crate) browser_filter_input: Entity<InputState>,
    /// 上述筛选框的订阅（只持有；`Change` → `set_query`）
    pub(crate) _browser_filter_sub: Option<Subscription>,
    /// 【呈现】显示方式（DBeaver 左栏「显示方式」）：网格 / 文本。面板级持续——
    /// 切结果集不重置（用户选择的是「怎么看结果」，不是某份结果的属性）。
    pub(crate) presentation: Presentation,
    /// 【右栏】当前标签页（对齐 DBeaver 右侧面板组：列浏览器 / 值详情 / 计算 / 分组）
    pub(crate) right_tab: RightTab,
    /// 【右栏】已启用的页集合（规范序，恒非空；对齐 DBeaver `presentationSettings
    /// .enabledPanelIds`，`ResultSetViewer.addPanel:1388`）。页签条只列出集合内的页，
    /// 「关闭一页」= 从集合移除（其余页仍在），视图菜单的勾选态即成员关系。
    pub(crate) open_tabs: Vec<RightTab>,
    /// 【引用页】外键清单状态（对齐 DBeaver `ReferencesResultsContainer` 的键列表）：
    /// 页签切到「引用」时由宿主经 `MetadataService::list_constraints` 取回并回填；
    /// 初值 = 还没读过（`NotLoaded`）。**render 只读本字段**，取数不在 render 里发生。
    pub(crate) references: crate::references::ReferencesState,
    /// 【引用页】键下拉当前选中的下标（`None` = 没有可选键）。
    /// 清单回填时默认选第一条（DBeaver `:341-343` 的兜底），选中态由
    /// [`crate::ports::ResultRequest::SetActiveReference`] 改。
    pub(crate) active_reference: Option<usize>,
    /// 【R2 计算页】启用的统计项集合（顺序 = 渲染顺序）。对齐 DBeaver Calc 的
    /// `AggregateColumnsPanel.enabledFunctions`：`+` 添加 / 行尾 × 删除 / Reset 恢复。
    /// 顺序按下标保存，用户增删不会打乱已启用项的相对次序。
    pub(crate) stats_items: Vec<StatsItem>,
    /// 【R2 计算页】聚合口径（数值 / 字符串）。对齐 DBeaver Calc 的
    /// `aggregateAsStrings` CHECK：字符串口径下 MIN/MAX 走字典序，AVG/SUM 不出值。
    pub(crate) stats_mode: StatsMode,
    /// 【D11】「按列分组聚合」开关（对齐 `AggregateColumnsPanel.groupByColumns`，
    /// `:131` 从 `PARAM_GROUP_BY_COLUMNS` 读、`:167` 写回；出厂 false）。
    /// 开启后统计页按**选区覆盖的列**分组展示（`:231-249`），每列一组；
    /// 选区为空时退回当前作用列（DBeaver 此时整表为空，本仓保留单列视图，见 §31 明账）。
    pub(crate) stats_group_by: bool,
    /// 【D12】当前选中的统计行（工具条「−」的作用对象）。
    ///
    /// DBeaver 的「−」删 `aggregateTable` 的选中项（`RemoveFunctionAction`
    /// `:424-433`）；本仓统计页是平铺行、没有 Tree 选中模型，所以由行点击维护这**一个**
    /// 选中项（`:424` 的 `isEnabled` 只判「有选中」，不要求多选）。
    pub(crate) stats_selected: Option<StatsItem>,
    /// 【R3 分组页】额外分组列集合（数据列序；空 = 只用当前选中列）。
    /// 点选加入 / 再点移除（顺序即分组键的拼接顺序，用户可感知地决定「按 A 再按 B」）。
    /// **只活在本视图态、跨会话不记**：DBeaver 侧 `GroupingPanel.loadSettings/saveSettings`
    /// （`:114-120`）是空实现（`saveSettings` 全仓无调用点），分组键不进面板设置——
    /// 那边真正落地的是「生成一条 GROUP BY 查询交给数据源执行」（`GroupingResultsContainer
    /// .rebuildGrouping:211-247`），所以键跟着查询/编辑器走而不是偏好。见 §8.93。
    pub(crate) group_columns: Vec<usize>,
    /// 【R3 分组页】分组结果排序口径
    pub(crate) group_sort: GroupSort,
    /// 【R3 分组页】仅重复：只保留行数 > 1 的分组
    pub(crate) group_duplicates_only: bool,
    /// 【D14 分组页】页内嵌的结果集（对齐 DBeaver `GroupingResultsContainer.java:61-73`：
    /// 那一页里嵌着一整个 `ResultSetViewer`）。宿主在构造期建好交进来（与对比栏网格
    /// 同一条 [`Self::set_grids`] 通道），回执泵按 [`crate::store::ResultPlacement::Grouping`]
    /// 把 `GROUP BY` 重查的结果写进它——**不进结果标签条**，主结果集不受影响。
    pub(crate) group_grid: Option<Entity<TableState<ResultGridDelegate>>>,
    /// 【D14 分组页】最近一次下发成功的分组 SQL（`rewrite_with_grouping` 的产物）。
    /// 页内工具条把它挂成 tooltip / 供「复制语句」用；空 = 这一页还没查过。
    pub(crate) group_query: Option<String>,
    /// 【D14 分组页】页内嵌结果集的原始投影（列名 / 行 / 类型 / 注释）。
    /// 排序口径切换时据此重排重灌，不必再发一次查询（对齐 DBeaver
    /// `rebuildGrouping:249-265` 的 viewer 侧排序）。
    pub(crate) group_snapshot: Option<(
        Vec<String>,
        Vec<Vec<String>>,
        Vec<Option<crate::store::ColumnType>>,
        Vec<Option<workbench_shell::model::ColumnAnnotation>>,
    )>,
    /// 【并排对比】对比栏的投影（编辑器 `sync_compare_grid` 时经
    /// [`Self::set_compare_projection`] 写入；render 期只读）
    pub(crate) compare_active: usize,
    pub(crate) compare_toolbar: Option<ResultToolbar>,
    pub(crate) compare_status: Option<ResultStatus>,
    pub(crate) result_sql: Option<String>,
    /// 【状态历史】本面板见过的「结果状态」快照：每份被同步进来的结果 SQL 一条
    /// （对齐 DBeaver `HistoryStateItem` 里的 `dataContainer` + `filter`，本仓口径 ①：
    /// 一份状态 = 一份结果 SQL，因为重查通道就是 `ExecTarget::Statement(sql)`）。
    /// 由 [`Self::note_state_sql`]（`sync` 拿到新结果时调用）维护。
    pub(crate) state_history: Vec<String>,
    /// 【状态历史】当前位置（`state_history` 下标）；空历史时为 0（无意义但省掉 `Option`）
    pub(crate) history_pos: usize,
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
    /// 【P3】多行控件：DBeaver 的筛选输入是 `TextViewer(SWT.MULTI)`，不是单行框
    pub(crate) filter_input: Entity<TextareaState>,
    pub(crate) filter_text: String,
    /// 防抖版本号（连打时任务被取代，只有最后一次到点）
    pub(crate) filter_version: u64,
    /// 在途防抖任务（持有以保活；换词即被取代）
    pub(crate) filter_debounce: RefCell<Option<Task<()>>>,
    /// 【筛选历史】最近**提交**的筛选词（新的在前，去重、上限见
    /// [`FILTER_HISTORY_LIMIT`]）。
    ///
    /// 对齐 DBeaver 筛选条右侧的 `HistoryPanel`（`ResultSetFilterPanel.java:214`，
    /// 点击弹历史表 `createFilterHistoryPanel`；快捷键 ↓ 同样打开，`:278-282`）。
    /// 只有**显式提交**才进历史（Enter 或 ▶ 执行），打字过程中的防抖状态不入——
    /// 否则历史会被 `a` / `ab` / `abc` 这类前缀灌满。
    ///
    /// 口径差异：DBeaver 按 SQL 文本持久化到 filter manager（`loadFiltersHistory(query)`），
    /// 本仓暂无那条持久化通道，故这里是**会话级内存**（面板存活期内有效），
    /// 关闭编辑器即丢。
    pub(crate) filter_history: Vec<String>,
    /// 【结果内查找】查找栏是否打开（筛选条上的查找钮 + 查找栏本身都按它渲染）
    ///
    /// 对齐 DBeaver：筛选条工具条上的 `openFindReplaceButton` 是 **SWT.CHECK**
    /// （`ResultSetFilterPanel.java:356`，图标 `UIIcon.FIND_REPLACE`，命令
    /// `EDIT_FIND_AND_REPLACE`）：选中打开查找浮层、取消关闭（`:361-385`）。
    pub(crate) find_open: bool,
    /// 【结果内查找】查找词输入框（懒于构造期创建，与筛选框同口径）
    pub(crate) find_input: Entity<InputState>,
    /// 【结果内查找】输入框订阅句柄（**必须存字段**：`Subscription` 是「掉了即解订」
    /// 的守卫，写成 `let _ = …` 等于当场丢弃——筛选框就踩过这条，见 §8.55）
    pub(crate) _find_sub: Option<Subscription>,
    /// 【结果内查找】当前查找词（trim + 小写前的原样，供输入框回填与命中数）
    pub(crate) find_query: String,
    /// 【结果内查找】命中总数（输入词变 / 数据变时重算；画在查找栏右端）
    pub(crate) find_hits: usize,
    /// 【结果内查找】当前停在哪个命中（**视图行, 视图列**；`None` = 还没定位过）
    ///
    /// 存视图列（含 `#` 槽）而不是数据列：`ResultGridDelegate::find_next` 吐的就是视图列，
    /// 直接喂 `set_selected_cell` 不用再折算（算错一格就会跳到邻列）。
    pub(crate) find_at: Option<(usize, usize)>,
    /// 【结果内查找】「只显示匹配行」开关（查找栏上那颗勾选框，DBeaver 的
    /// `selectAllButton` / 「Show matching」）。
    ///
    /// 语义：开着时把 `find_query` 当临时行过滤灌进 delegate（`set_find_filter`），
    /// 网格只留命中行；关掉查找栏 / 取消勾选 / 词清空时撤销。**不动筛选框的 `filter`**
    /// ——那是用户的词，撤查找过滤时必须原样还在。
    pub(crate) find_filter_only: bool,
    /// 【结果内查找】匹配口径（大小写 / 正则 / 全词三开关；查找栏上三颗 CHECK）
    ///
    /// 定位与「只显示匹配行」共用这一份（DBeaver 的 `SearchQuickFilterInfo` 同款：
    /// 同一个对象既喂 `setQuickFilter` 又喂 findAll）。
    pub(crate) find_criteria: crate::ports::FindCriteria,
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
    /// 【P1-3】列宽快照（列名 → 像素宽），面板侧权威值：拖拽结束时由订阅写入、
    /// 宿主存档时读走、会话恢复时回灌。跨结果集保留（按列名匹配，对不上的列忽略）。
    pub(crate) column_widths: Vec<(String, f32)>,
    /// 【列布局落盘】隐藏列快照（列名）。与列宽同一条通道：交互路径改完即取快照、
    /// 宿主存档时读走、会话恢复时回灌，由下一次 `sync` 在 `set_data` 之后落到 delegate。
    pub(crate) column_hidden: Vec<String>,
    /// 【列布局落盘】冻结列快照（列名，口径同 `column_hidden`）
    pub(crate) column_frozen: Vec<String>,
    /// 【DBeaver 对齐】状态栏中段行数输入框（可编辑：输入行号跳到该行）
    pub(crate) row_count_input: Entity<InputState>,
    /// 【P1-2】行号输入框的订阅句柄（仅持有——句柄丢了解订就跟着走；`set_value`
    /// 程序化回填不发事件，所以只有用户真敲键才会入队 `GoRow`）
    pub(crate) _row_count_sub: Option<Subscription>,
    /// 【R6 段大小】状态栏「一段」行数输入框（对齐 DBeaver `resultSetSize` Text）
    pub(crate) segment_input: Entity<InputState>,
    /// 【R6 段大小】输入框订阅句柄（仅持有；有效输入入队 `SetSegmentRows`）
    pub(crate) _segment_sub: Option<Subscription>,
    /// 【网格事件】表格事件订阅句柄（选格 / 双击编辑 / 列序 / 列宽 / 清除选择 / 整行选中）。
    /// **必须持有**：句柄一丢就解订，`set_grids` 换网格时整体重挂（见 `set_grids` 注释）。
    pub(crate) _grid_sub: Option<Subscription>,
    /// 【B15】筛选框订阅句柄（`InputEvent::Change` → 防抖 → 应用筛选）。
    /// **必须持有**：早先写成 `let _ = filter_sub;` 当场丢弃，边打边筛从未生效。
    pub(crate) _filter_sub: Option<Subscription>,
    /// 【第 81 批】`ESC` 关结果集浮层的 app 级 keystroke interceptor 句柄。
    /// **必须持有**：句柄一丢就解订（与 `_filter_sub` 是同一个坑）。
    pub(crate) _esc_sub: Subscription,

    // ── 【筛选设置】自定义条件 / 排序浮层（DBeaver `filtersCustomButton` → FilterSettingsDialog）
    /// 浮层是否打开（`false` = 不渲染这层遮罩）
    pub(crate) filter_settings_open: bool,
    /// 自定义 `ORDER BY` 文本输入框（浮层内唯一可编辑项；WHERE 一栏走筛选框本体，
    /// 不在这里再放第二个输入源——「同一个 `InputState` 只渲染一处」）
    pub(crate) filter_order_input: Entity<InputState>,
    /// 已生效的自定义排序文本（空 = 沿用原查询的 ORDER BY）；打开浮层时用它回填
    pub(crate) filter_order: String,
    /// 浮层当前页签（默认「列约束」——DBeaver 对话框先建列页并选中它）
    pub(crate) filter_settings_tab: FilterSettingsTab,
    /// 【筛选设置 · 存档】用户显式「保存过滤器设置」留下的那一份（`None` = 没存过 /
    /// 刚重置）。面板级内存：本批不落盘，跨会话通道见 `docs/architecture/editor/result-panel.md`
    /// 的下一批登记项。
    pub(crate) saved_filter: Option<SavedFilterSetting>,
    /// 【筛选设置 · 存档】待回填筛选输入框的词：`sync` 套用存档时写在这里，由宿主在
    /// 窗口可用时转手调 [`Self::sync_filter_input`]（回填需要 `&mut Window`，`sync` 没有）。
    /// 真值不等它——`sync` 里已经把词写进 `filter_text` 并应用过了。
    pub(crate) filter_backfill: Option<String>,
}
