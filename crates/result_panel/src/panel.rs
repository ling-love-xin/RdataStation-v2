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
use std::time::Duration;

use gpui_kit::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
/// 【侧栏丝滑】宽度过渡用 `EffectTransition::width`（元素级动画，动画状态按元素 id
/// 挂在 gpui 元素树上——render 无需 `&mut App`；id 变化即重启，故用 epoch 区分每次
/// 开合，与 vendor sidebar 组件同款机制）。
use gpui_kit::base::animation::{ease_in_out_cubic, EffectTransition};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::IndexPath;
use gpui_kit::component::{ActiveTheme as _, Size, Sizable as _};
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::toolbar::ToolbarGroup;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::table::{TableDelegate, TableEvent, TableState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）
use ::shared::{CurrentTier, FactFrame};

use crate::column_browser::ColumnBrowserDelegate;
use crate::error_card::{self, ErrorCard};
use crate::export;
use crate::grid::{self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{Presentation, ResultActionQueue, ResultRequest, RightTab};
use crate::script_card::{self, ScriptCard};
use crate::sets::{self as result_sets, ResultSetTab};
use crate::ui;
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
    grid: Entity<TableState<ResultGridDelegate>>,
    /// 对比栏网格（对比模式才渲染；数据由编辑器 `sync_compare_grid` 拷入）
    compare_grid: Option<Entity<TableState<ResultGridDelegate>>>,
    /// 动作队列（渲染期 `push`；编辑器事件路径 `drain` 分派）
    queue: ResultActionQueue,

    /// 投影状态（来自 [`ResultSyncInput`]，render 是纯读路径不再重算）
    tabs: Vec<ResultSetTab>,
    /// 标签条滚动句柄（标签随查询累积增多，传给 TabBar `track_scroll` 保持跨帧滚动）
    tabs_scroll: ScrollHandle,
    active: usize,
    notice: Option<String>,
    toolbar: Option<ResultToolbar>,
    status: Option<ResultStatus>,
    error_card: Option<ErrorCard>,
    script_card: Option<ScriptCard>,
    transposed: bool,
    transpose_row: usize,
    compare_mode: bool,
    /// 【2026-10-28 微调】SQL 预览**弹出窗口**开关：点击工具栏 SQL 文本框置 true，
    /// render 根挂浮层（遮罩 + 居中卡片 + 只读 Textarea）。**不用 kit Dialog**：
    /// `Dialog::new` 需 `&mut App`，而本面板 render 是 `&App`（host 手动调
    /// `result_panel.read(cx).render(cx)`）——等效浮层 + kit Textarea 满足"弹出窗口 +
    /// 文本组件"，Esc / 遮罩点击 / 关闭按钮三路关闭。
    sql_dialog_open: bool,
    /// 【2026-10-28 微调】SQL 预览弹窗里的只读 Textarea 实体（懒创建：`TextareaState::new`
    /// 需要 window + &mut App，也在事件路径建；SQL 变化时 `set_text` 刷新）。
    /// render 期 `Textarea::new(&entity)` 只需 &Entity，可直接挂浮层。
    sql_textarea: Option<Entity<TextareaState>>,
    /// 【1.2】筛选框展开态：点击筛选组展开按钮后，工具栏下方出现顶宽筛选条
    /// （输入框全宽 + ▶ 运行 + ✕ 清除 + wrapped SQL 预览）。默认收起。
    filter_expanded: bool,
    /// 【侧栏】左侧栏（网格 / 文本 / 记录竖条）开关；默认开（对齐 DBeaver 左缘
    /// 竖条常驻），状态栏左端开关可收起（完全收起 = 不占宽度）。
    show_left_sidebar: bool,
    /// 【侧栏】右侧辅助栏（列浏览器 / 值详情 / 计算 / 分组）开关；默认关
    /// （不挤网格，工具栏「列浏览器」可开）。列浏览器与值详情等共用这一栏、页签切换。
    show_value_sidebar: bool,
    /// 【2026-10-28 微调】右缘竖条可见性——右侧开关**三态**（比左侧多一个展开态）：
    /// `(edge_bar_visible, show_value_sidebar)` = (false,false)=收起 / (true,false)=窄展开
    /// （只显右缘竖条图标条）/ (true,true)=宽展开（竖条 + 右栏内容）。不变量：宽展开 ⟹
    /// 竖条可见。开关在状态栏右端（render_status_row 的 right_switch，三态循环点击）。
    edge_bar_visible: bool,
    /// 【侧栏丝滑】左右栏展开 / 收起动画代次（每次开合 +1；0 = 首帧免动画）。
    /// 动画 id 拼 epoch → with_animation 换 id 即重播（与 vendor sidebar 同机制）。
    left_sidebar_epoch: u64,
    right_sidebar_epoch: u64,
    /// 【侧栏】最近选中格位置（**视图行**, 含行号槽列；宿主 `TableEvent::SelectCell`
    /// 事件写入，选行 / 清选置 `None`）。右栏值详情据此读网格取完整值 / 列名 / 类型。
    selection_pos: Option<(usize, usize)>,
    /// 【结果集 #296】持久化的列序（列名数组；空 = 默认序）。宿主在构造期注入
    /// （重启恢复），`TableEvent::MoveColumn` 订阅更新，并经
    /// `ResultRequest::ColumnOrderChanged` 回写宿主落库。换新结果时按列名匹配重排。
    column_order: Vec<String>,
    /// 【列浏览器】清单当前选中列（**数据列序**，与隐藏 / 冻结同一套下标空间）。
    /// 清单单击写入；网格选中格经 `selection_pos` 反推同步；新结果集 / 切标签 /
    /// 对比模式重置为 `None`（渲染兜底第一可见列）。清单选中 ≠ 网格焦点。
    selected_col: Option<usize>,
    /// 【列浏览器】清单 List 实体（kit `list::{ListState, ListDelegate}` 承载；
    /// 选中高亮由 List 内部 `selected_index` 管理，搜索框也在 List 内置里——
    /// 面板不再自持搜索输入框 / 搜索词，见 [`crate::column_browser`]）
    browser_list: Entity<ListState<ColumnBrowserDelegate>>,
    /// 【呈现】显示方式（DBeaver 左栏「显示方式」）：网格 / 文本。面板级持续——
    /// 切结果集不重置（用户选择的是「怎么看结果」，不是某份结果的属性）。
    presentation: Presentation,
    /// 【右栏】当前标签页（对齐 DBeaver 右侧面板组：列浏览器 / 值详情 / 计算 / 分组）
    right_tab: RightTab,
    /// 【并排对比】对比栏的投影（编辑器 `sync_compare_grid` 时经
    /// [`Self::set_compare_projection`] 写入；render 期只读）
    compare_active: usize,
    compare_toolbar: Option<ResultToolbar>,
    compare_status: Option<ResultStatus>,
    result_sql: Option<String>,
    /// 【1.2】筛选框运行生成的 wrapped SQL（`select * from (原SQL) where …`）；
    /// 执行完成后写入，右栏「执行SQL」页签展示（只读 + 复制）。空 = 页签空态。
    exec_sql: Option<String>,
    result_is_analysis: bool,
    result_can_copy: bool,
    fetching_more: bool,
    /// 导出菜单静态上下文（sync 写入；render 期只读）
    export_ctx: Option<ExportCtx>,
    /// 分析菜单项（sync 写入；render 期只读）
    analysis_items: Vec<AnalysisItem>,
    /// 待提交变更数（sync 写入；> 0 才摆提交 / 撤销对）
    pending_changes: usize,
    /// 提交中（sync 写入；提交按钮置灰）
    committing: bool,
    /// 【#28-B3】组合撤销 / 重做栈是否非空（sync 写入；状态栏撤销 / 重做钮禁用态据此）
    can_undo: bool,
    can_redo: bool,
    /// 【#28-B3】这份结果能不能编辑（sync 写入；行级新增 / 复制 / 删除钮据此决定是否注入）
    editable: bool,

    // 【B15】本地筛选框（视图层自洽：防抖 + 清除 + 统计回填都在面板内）
    filter_input: Entity<InputState>,
    filter_text: String,
    /// 防抖版本号（连打时任务被取代，只有最后一次到点）
    filter_version: u64,
    /// 在途防抖任务（持有以保活；换词即被取代）
    filter_debounce: RefCell<Option<Task<()>>>,
    /// 【B14】下发源库开关（开着时应用筛选会重查源库——重查经队列交回编辑器）
    pushdown: bool,
    /// 【C1】正在重命名哪个结果集 tab（`None` = 不在重命名态）；会话级
    renaming: Option<usize>,
    /// 【C1】重命名输入框实体（懒创建：第一次进重命名态时建；重命名态关闭后保留实体复用）
    renaming_input: Option<Entity<InputState>>,
    /// 【B1】列头直接显示注释的面板级临时开关（覆盖全局偏好 `result_panel.show_column_comments`；
    /// 默认 false——全局偏好开启时由 host 在 sync 时同步过来）。
    show_column_comments: bool,
    /// 【类型样式】全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    /// host 构造时同步（settings.result_panel.type_style()），设置页变更由 workbench
    /// 钩子推 `set_type_style` 即时生效；只影响渲染显示。
    type_style: TypeStylePrefs,
    /// 【DBeaver 对齐】状态栏中段行数输入框（可编辑，输入行号跳转；暂不接跳转逻辑）
    row_count_input: Entity<InputState>,
}

impl ResultPanel {
    /// 构造空面板：建筛选输入框（含 300ms 防抖订阅）。
    ///
    /// 网格实体由编辑器构造期创建（钩子注入依赖 editor 动作），经 [`Self::set_grids`]
    /// 交进来；构造后、首次 sync 前面板不渲染任何结果。
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        queue: ResultActionQueue,
    ) -> Self {
        // 【B15】本地筛选框：输入变化走防抖（原型 §5.5 的 300ms），到点才重算视图行序
        let filter_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("筛选当前结果…", window, cx);
            state
        });
        let filter_sub = cx.subscribe_in(
            &filter_input,
            window,
            |this, state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    this.on_filter_input(text, cx);
                }
            },
        );
        // 订阅句柄仅持有（防重复建立）；面板实体自身不需要额外使用
        let _ = filter_sub;

        // 【DBeaver 对齐】状态栏中段行数输入框：可编辑，输入行号跳转（暂不接跳转逻辑）
        let row_count_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("200", window, cx);
            state
        });

        // 占位网格：编辑器构造真网格（含钩子）后经 `set_grids` 替换；占位仅保证
        // 构造后到接线前这段窗口里实体非空（渲染不会走到——还没 sync 过）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行"),
            window,
            cx,
        );

        // 【列浏览器】清单 List（kit 组件承载：搜索框 / 键盘导航 / 滚动条内置）：
        // delegate 只持投影，数据由 `sync` 灌入；占位网格句柄先借用，`set_grids`
        // 替换网格后 `sync` 会重灌（构造后到接线前不会渲染，见 grid 占位注释）
        let browser_list = cx.new(|cx| {
            ListState::new(ColumnBrowserDelegate::new(queue.clone(), grid.clone()), window, cx)
                .searchable(true)
        });

        // 【DBeaver 对齐】订阅 table 事件：双击单元格编辑 / 列序拖拽 / 清除选择
        let _grid_sub = cx.subscribe_in(
            &grid,
            window,
            |this, _table, event: &TableEvent, _window, cx| {
                match event {
                    // 双击单元格：进入编辑态（当前走行级叠加层）
                    TableEvent::DoubleClickedCell(row_ix, col_ix) => {
                        // 选中该单元格（delegate 坐标），让行级模型知道要编辑哪格
                        this.selection_pos = Some((*row_ix, *col_ix));
                        cx.notify();
                    }
                    // 列序拖拽：记下新显示序（delegate 的 `display_order` 快照），
                    // 并推 `ColumnOrderChanged` 让宿主落库（跨会话持久化）
                    TableEvent::MoveColumn(from, to) => {
                        let _ = (from, to);
                        let order = this.grid.read(cx).delegate().display_order();
                        if order != this.column_order {
                            this.column_order = order;
                            this.queue.push(ResultRequest::ColumnOrderChanged {
                                order: this.column_order.clone(),
                            });
                        }
                        cx.notify();
                    }
                    // 清除选择：清空选中格位置
                    TableEvent::ClearSelection => {
                        this.selection_pos = None;
                        this.selected_col = None;
                        cx.notify();
                    }
                    // 其他事件忽略（SelectRow / SelectColumn / SelectCell 已由
                    // delegate 内联处理或后续接线）
                    _ => {}
                }
            },
        );

        Self {
            grid,
            compare_grid: None,
            queue,
            tabs: Vec::new(),
            tabs_scroll: ScrollHandle::default(),
            active: 0,
            notice: None,
            toolbar: None,
            status: None,
            error_card: None,
            script_card: None,
            transposed: false,
            transpose_row: 0,
            compare_mode: false,
            show_left_sidebar: true,
            show_value_sidebar: true,
            edge_bar_visible: true,
            left_sidebar_epoch: 0,
            right_sidebar_epoch: 0,
            sql_dialog_open: false,
            sql_textarea: None,
            filter_expanded: false,
            selection_pos: None,
            // 【结果集 #296】列序默认空 = 数据序；宿主构造期注入持久化列序
            column_order: Vec::new(),
            selected_col: None,
            browser_list,
            row_count_input,
            presentation: Presentation::Grid,
            right_tab: RightTab::Browser,
            compare_active: 0,
            compare_toolbar: None,
            compare_status: None,
            result_sql: None,
            exec_sql: None,
            result_is_analysis: false,
            result_can_copy: false,
            fetching_more: false,
            export_ctx: None,
            analysis_items: Vec::new(),
            pending_changes: 0,
            committing: false,
            can_undo: false,
            can_redo: false,
            editable: false,
            filter_input,
            filter_text: String::new(),
            filter_version: 0,
            filter_debounce: RefCell::new(None),
            pushdown: false,
            renaming: None,
            renaming_input: None,
            show_column_comments: false,
            // 类型样式默认全继承 / 原始值（与渲染现状一致；host 构造时按全局偏好覆盖）
            type_style: TypeStylePrefs::default(),
        }
    }

    /// 编辑器把构造好的网格（含钩子）交进来
    pub fn set_grids(
        &mut self,
        grid: Entity<TableState<ResultGridDelegate>>,
        compare_grid: Option<Entity<TableState<ResultGridDelegate>>>,
    ) {
        self.grid = grid;
        self.compare_grid = compare_grid;
    }

    /// 对比栏投影（编辑器 `sync_compare_grid` 时调用；与主栏投影同口径，只读）
    pub fn set_compare_projection(
        &mut self,
        active: usize,
        toolbar: Option<ResultToolbar>,
        status: Option<ResultStatus>,
    ) {
        self.compare_active = active;
        self.compare_toolbar = toolbar;
        self.compare_status = status;
    }

    /// 动作队列（编辑器订阅处理时读；渲染期不读）
    pub fn queue(&self) -> &ResultActionQueue {
        &self.queue
    }

    /// 投影：把编辑器算好的判据写进面板状态 + 主网格（事件路径调用，render 是纯读）
    pub fn sync(&mut self, cx: &mut Context<Self>, input: ResultSyncInput) {
        // 【P2-4】换结果 = 换一批行列：转置视图自动退出（delegate 侧已由 `set_data`
        // 清掉，这里把面板开关同步复位——`toggled` 与网格状态保持同一份真值）
        self.transposed = false;
        self.transpose_row = 0;
        // 【列浏览器】换结果集：选中列与清单高亮复位（渲染兜底第一可见列）；
        // 搜索词沿用全局筛选框同口径跨结果保留，由搜索框自身 ✕ / ESC 清空
        self.selected_col = None;

        self.tabs = input.tabs;
        self.active = input.active;
        self.notice = input.notice;
        self.toolbar = input.toolbar;
        self.status = input.status;
        self.error_card = input.error_card;
        self.script_card = input.script_card;
        self.result_sql = input.sql;
        self.result_can_copy = input.can_copy;
        self.result_is_analysis = input.is_analysis;
        self.export_ctx = input.export_ctx;
        self.analysis_items = input.analysis_items;
        self.pending_changes = input.pending_changes;
        self.committing = input.committing;
        self.can_undo = input.can_undo;
        self.can_redo = input.can_redo;
        self.editable = input.editable;
        self.fetching_more = input.loading_more;

        // 【B15】筛选统计的真值在网格那边（视图行序是它算的）——写入 toolbar 前先回填
        self.refresh_filter_hint(cx);

        self.grid.update(cx, |state, cx| {
            match input.grid {
                Some(payload) => {
                    // 【结果集 #296】先按持久化列名序重排（空 = 默认序，原样直通）
                    let (columns, rows) = self.apply_column_order(payload.columns, payload.rows);
                    state.delegate_mut().set_data(columns, rows);
                    // 【Q6】类型必须**在 set_data 之后**设：`set_data` 会清空类型
                    state.delegate_mut().set_column_types(payload.column_types);
                    // 【A9】来源表名同理（set_data 会清掉旧的）
                    state.delegate_mut().set_result_table(payload.table);
                    // 【P1】字段注释同理（set_data 会清掉旧的）
                    state.delegate_mut().set_column_annotations(payload.annotations);
                    // 【B5】事实帧同理（set_data 会清掉旧的）；交互派生时才读取，render 不解析
                    state.delegate_mut().set_fact(payload.fact);
                    // 【#28】本地叠加层同理（set_data 会清掉旧的）；未接行级动作时为 None
                    state.delegate_mut().set_local_snapshot(payload.local);
                }
                None => state.delegate_mut().clear(input.empty),
            }
            // 【M8】这份结果能不能洞察（菜单项据此出现 / 消失）
            state.delegate_mut().set_insight_available(input.insight_available);
            // 【B5b】这份结果还能不能再取一段 / 是不是正在取
            state.delegate_mut().set_has_more(input.has_more);
            state.delegate_mut().set_loading_more(input.loading_more);
            // 【T2a-4/6】单元格编辑开关 + 脏格 / 冲突格底色
            state.delegate_mut().set_editable(input.editable);
            state.delegate_mut().set_dirty(input.dirty);
            state.delegate_mut().set_conflict(input.conflict);
            state.refresh(cx);
        });

        // 【列浏览器】清单数据跟随网格投影重灌（换结果集 / 刷新 / 取段都走到这里；
        // 搜索词**跨结果保留**的既有口径不变——`set_data` 只换投影，不清搜索框）。
        // List 内部选中高亮（selected_index）没有窗口句柄可在这里复位，换结果后
        // 可能残留旧行高亮，用户点击 / 搜索 / 网格反推后即纠正（记录在案）。
        {
            let grid = self.grid.clone();
            let browser = self.browser_list.clone();
            browser.update(cx, |list, cx| {
                // 先换真实网格再灌投影：构造期 delegate 持有的是占位网格（面板建好
                // 表状态前 ListState 就要先建），右键动作（冻结/隐藏/显示全部/清排序）
                // 直接 update 网格，不换会落在空数据上（功能回归）
                list.delegate_mut().set_grid(grid.clone());
                let delegate = grid.read(&*cx).delegate();
                let columns = delegate.columns().to_vec();
                let type_names: Vec<String> = delegate
                    .column_types()
                    .iter()
                    .map(|t| t.as_ref().map(|t| t.name.clone()).unwrap_or_default())
                    .collect();
                let hidden: Vec<bool> = (0..columns.len())
                    .map(|c| delegate.is_hidden_column(c))
                    .collect();
                let comments: Vec<Option<String>> = delegate
                    .column_annotations()
                    .iter()
                    .map(|a| a.as_ref().and_then(|a| a.comment.clone()))
                    .collect();
                list.delegate_mut()
                    .set_data(columns, type_names, hidden, comments);
            });
        }
    }

    /// 结果区整体渲染（主栏组装 + 对比模式双栏；按钮动作分两类：
    /// 纯视图层面板内处理，依赖编辑器的入队——见模块头注释）
    pub fn render(&self, cx: &App) -> AnyElement {
        // 脚本卡 / 错误卡 / 无结论分支与 SQL 分支的判定：有卡片就不画网格。
        // 【2b】脚本输出卡片（通用模式）与错误卡片同属「替掉网格」的那类卡；
        // 两者都不会与正常网格并存（互斥由编辑器投影保证）。
        let card = self
            .error_card
            .clone()
            .map(|card| {
                let queue = self.queue.clone();
                let locate = card.location.as_ref().map(|location| {
                    let label = error_card::locate_label(location);
                    let queue = queue.clone();
                    Button::new("editor-result-error-locate")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-error-locate".to_string())
                        .label(label)
                        .on_click(move |_, _window, app| {
                            queue.request(ResultRequest::LocateError, app);
                        })
                        .into_any_element()
                });
                let queue = self.queue.clone();
                let copy = Button::new("editor-result-error-copy")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-error-copy".to_string())
                    .label("复制")
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::CopyError, app);
                    })
                    .into_any_element();
                error_card::render(
                    &card,
                    error_card::ErrorCardControls {
                        locate,
                        copy: Some(copy),
                    },
                    cx,
                )
                .into_any_element()
            })
            .or_else(|| {
                self.script_card.clone().map(|script| {
                    script_card::render(&script, cx).into_any_element()
                })
            });

        let mut toolbar = self.toolbar.clone().unwrap_or(ResultToolbar {
            elapsed_ms: None,
            ..Default::default()
        });
        // 【DBeaver 对齐】工具栏最左侧显示 SQL 预览（tooltip 看全文）
        toolbar.sql = self.result_sql.clone();
        // 【原型对齐】结果集标签条始终显示（即使只有 1 个结果集）：
        // 单个结果集时也显示名称（用户可重命名），对齐 DBeaver 口径。
        let tabs = (!self.tabs.is_empty()).then(|| {
            let queue_pick = self.queue.clone();
            let queue_rename = self.queue.clone();
            let queue_pin = self.queue.clone();
            let actions = result_sets::TabActions {
                // 「⋯」按钮：进入重命名态（事件路径；request 立即通知面板 → 宿主 drain）
                on_rename: Some(std::rc::Rc::new(move |index, _w, app| {
                    queue_rename.request(ResultRequest::BeginRename(index), app);
                })),
                on_toggle_pin: Some(std::rc::Rc::new(move |index, app| {
                    queue_pin.request(ResultRequest::TogglePin(index), app);
                })),
            };
            result_sets::render(
                &self.tabs,
                self.active,
                move |index, _window, app| {
                    let index = *index;
                    queue_pick.request(ResultRequest::SelectSet(index), app);
                },
                actions,
                cx,
                &self.tabs_scroll,
            )
            .into_any_element()
        });

        // 【C1】重命名态：tab 条下方插入一个输入框条（输入框 + ✓ 提交 + ✕ 取消）
        // 【2026-10-28 微调】输入框聚焦时 Enter 提交 / Esc 取消（§8.14 已知遗留补齐）。
        // kit Input 不消费 Enter/Esc（input.rs 无 key_down 挂载），事件沿 Bubble 冒泡到
        // 本容器（div.on_key_down 在 Bubble 阶段触发）；走队列端口（CommitRename /
        // CancelRename 变体已存在），与 ✓/✕ 按钮同口径。
        let tabs = match (tabs, self.renaming) {
            (Some(tabs_el), Some(_index)) => {
                let input = self.renaming_input.clone();
                let queue_confirm = self.queue.clone();
                let queue_cancel = self.queue.clone();
                let queue_enter = self.queue.clone();
                let queue_esc = self.queue.clone();
                Some(
                    div()
                        .v_flex()
                        .child(tabs_el)
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_1()
                                .debug_selector(|| "editor-result-rename-bar".to_string())
                                .on_key_down(move |event, _window, app| {
                                    if event.keystroke.key == "enter" {
                                        queue_enter.request(ResultRequest::CommitRename, app);
                                    } else if event.keystroke.key == "escape" {
                                        queue_esc.request(ResultRequest::CancelRename, app);
                                    }
                                })
                                .child(
                                    div()
                                        .w(rems(ui::RESULT_FILTER_WIDTH))
                                        .children(input.as_ref().map(|i| Input::new(i))),
                                )
                                .child(
                                    Button::new("editor-result-rename-confirm")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Check)
                                        .on_click(move |_e, _w, app| {
                                            queue_confirm.request(ResultRequest::CommitRename, app);
                                        }),
                                )
                                .child(
                                    Button::new("editor-result-rename-cancel")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Close)
                                        .on_click(move |_e, _w, app| {
                                            queue_cancel.request(ResultRequest::CancelRename, app);
                                        }),
                                ),
                        )
                        .into_any_element(),
                )
            }
            (tabs, _) => tabs,
        };

        // 【呈现】文本视图（DBeaver「显示方式 → 文本」）：主区换成 TSV 纯文本
        // （可选中复制）；标签条保留（可切结果集）；侧栏让位（无选中格语义——
        // 文本没有「格」）。不提前 return：文本视图只是**第三行主体**的一种内容，
        // 四行骨架（tabs / 工具栏 / 主体 / 状态栏）统一在下方组装。

        // 【B15】本地筛选框：只作用于视图层，不重查数据库；输入走 300ms 防抖
        // （`on_filter_input`），有词时给一个 ✕ 清除；「下发源库」开关开着时应用
        // 筛选会重查（入队，结果落新结果集）。
        // 【1.3 对齐】下发源库 / 注释两个 Switch 从筛选组拆出，交给工具栏右段
        // Toolbar 统一容器（与对比 / 分析 / 复制同基线），筛选组只留输入与清除。
        // 【1.2】筛选框升级：常态 180px 输入框内嵌 ▶ 运行（RunFilterAsQuery，
        // 用 `select * from (原SQL) where 词` 替换当前结果集重查）；展开按钮
        // （Maximize2）打开顶宽筛选条（filter_expanded，输入框移到工具栏下方全宽）。
        // 展开态下工具栏筛选组不再渲染输入框（同一 InputState 实体一帧只能渲染
        // 一处，双渲染会元素 id 冲突），只留展开按钮（toggled 高亮指示展开态）。
        let filter = div()
            .h_flex()
            .items_center()
            .gap_1()
            .debug_selector(|| "editor-result-filter".to_string())
            .when(!self.filter_expanded, |row| {
                let run_queue = self.queue.clone();
                row.child(
                    div()
                        .w(rems(ui::RESULT_FILTER_WIDTH))
                        .child(
                            Input::new(&self.filter_input).suffix(
                                Button::new("editor-result-filter-run")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Play)
                                    .debug_selector(|| "editor-result-filter-run".to_string())
                                    .tooltip("用 select * from (原SQL) where 词 重查（替换当前结果集）")
                                    .on_click(move |_, _window, app| {
                                        run_queue.request(ResultRequest::RunFilterAsQuery, app);
                                    }),
                            ),
                        ),
                )
            })
            .child({
                let expand_queue = self.queue.clone();
                Button::new("editor-result-filter-expand")
                    .ghost()
                    .small()
                    .icon(IconName::Maximize2)
                    .toggled(self.filter_expanded)
                    .debug_selector(|| "editor-result-filter-expand".to_string())
                    .tooltip(if self.filter_expanded {
                        "收起筛选（回到工具栏输入框）"
                    } else {
                        "展开筛选（顶宽输入框）"
                    })
                    .on_click(move |_, _window, app| {
                        expand_queue.request(ResultRequest::ToggleFilterExpand, app);
                    })
            })
            .when(!self.filter_text.trim().is_empty() && !self.filter_expanded, |row| {
                let queue = self.queue.clone();
                row.child(
                    Button::new("editor-result-filter-clear")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-filter-clear".to_string())
                        .label("✕")
                        .on_click(move |_, _window, app| {
                            queue.request(ResultRequest::ClearFilter, app);
                        }),
                )
            })
            .into_any_element();

        // 【B15】「下发源库」开关：开着时应用筛选会重查（拼 WHERE 到原查询）
        let pushdown = {
            let queue = self.queue.clone();
            Switch::new("editor-result-pushdown")
                .checked(self.pushdown)
                .label("下发源库")
                .on_click(move |checked, _window, app| {
                    let open = *checked;
                    queue.request(ResultRequest::SetPushdown { open }, app);
                })
                .into_any_element()
        };
        // 列注释开关（对齐 DBeaver：列浏览器里显示列注释）
        let comments = {
            let queue = self.queue.clone();
            Switch::new("editor-result-show-comments")
                .checked(self.show_column_comments)
                .label("注释")
                .on_click(move |checked, _window, app| {
                    let open = *checked;
                    queue.request(ResultRequest::SetShowColumnComments { open }, app);
                })
                .into_any_element()
        };

        // 【右缘竖条】右侧辅助栏开合开关已移到最右缘竖条顶部（`render_right_edge_bar`，
        // 单入口；2026-10-28 落地），状态栏右端不再挂右栏开关，`ResultControls.value_sidebar`
        // 与 `render_status_row.right_switch` 均传 `None`。

        // 【并排对比】工具栏开关：两份以上结果才摆（一份结果没有可比的；对比栏
        // 数据只读，编辑 / 取段 / 洞察等动作仍只作用于主栏）
        let compare = (self.tabs.len() >= 2).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-compare")
                .ghost()
                .small()
                .toggled(self.compare_mode)
                .icon(IconName::Columns2)
                .tooltip("并排对比")
                .debug_selector(|| "editor-result-compare".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ToggleCompareMode, app);
                })
                .into_any_element()
        });

        // 【T2a-7】批量提交/回滚对（参考 DBeaver）：有**待提交**变更才摆；提交中置灰
        let commit = (self.pending_changes > 0).then(|| {
            let queue = self.queue.clone();
            let submitting = self.committing;
            let commit_btn = Button::new("editor-result-commit")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-commit".to_string())
                .disabled(submitting)
                .label(format!("提交变更 ({})", self.pending_changes))
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CommitPending, app);
                });
            let queue = self.queue.clone();
            let discard = Button::new("editor-result-discard")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-discard".to_string())
                .label("撤销变更")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::RollbackPending, app);
                });
            // 提交/撤销成对：2026-10-28 从手搓 `div().h_flex().gap_1()` 迁
            // `toolbar::ToolbarGroup`（组件统一化 §3 容器层反例修复）。外层无样式观测壳
            // div（仅 debug_selector 锚点）：ToolbarGroup 是 RenderOnce 无 debug_selector，
            // 测试靠壳锚点断言“按钮组在不在”（观测专用，不承载任何布局/交互语义）
            div()
                .debug_selector(|| "editor-result-commit-group".to_string())
                .child(
                    ToolbarGroup::new("editor-result-commit-group")
                        .child(commit_btn)
                        .child(discard),
                )
                .into_any_element()
        });

        // 动作：有网格才摆复制（没东西可复制就不摆）；有 SQL 就摆重跑（原位刷新）
        let copy = self.result_can_copy.then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-copy")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-copy".to_string())
                .label("复制")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::CopyTsv, app);
                })
                .into_any_element()
        });
        let refresh = (self.result_sql.is_some() && !self.result_is_analysis).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-refresh")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-refresh".to_string())
                .label("⟳ 刷新")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::Refresh, app);
                })
                .into_any_element()
        });
        // 【B5b】获取数据：只在这份结果真还有下一段时摆（状态行⑦里）
        let more = self
            .status
            .as_ref()
            .is_some_and(|status| status.has_more)
            .then(|| {
                let queue = self.queue.clone();
                Button::new("editor-result-more")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-more".to_string())
                    .label("获取数据")
                    .tooltip("获取下一段数据")
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::FetchMore, app);
                    })
                    .into_any_element()
            });
        // 【DBeaver 对齐】获取全部数据：只在这份结果真还有下一段时摆（一次性抓全量）
        let fetch_all = self
            .status
            .as_ref()
            .is_some_and(|status| status.has_more)
            .then(|| {
                let queue = self.queue.clone();
                Button::new("editor-result-fetch-all")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-fetch-all".to_string())
                    .label("获取全部数据")
                    .tooltip("一次性抓取全部行")
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::FetchAll, app);
                    })
                    .into_any_element()
            });

        // 【#28-B3】状态栏行级动作钮：新增 / 复制 / 删除一行。只读结果集（`!editable`）
        // 不注入，回退 grid.rs 的禁用占位钮；复制 / 删除必须有选中行才可用。
        // 选中行号取状态行 1 基记录（格选中与行选中都记行；0 基交给宿主翻译身份）。
        let selected_view_row = self
            .status
            .as_ref()
            .and_then(|status| status.selected_row)
            .map(|row| row.saturating_sub(1));
        let insert_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let anchor = selected_view_row;
            Button::new("editor-result-insert-row")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip("在下方插入新行")
                .debug_selector(|| "editor-result-insert-row".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::InsertRowBelow(anchor), app);
                })
                .into_any_element()
        });
        let duplicate_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let row = selected_view_row;
            Button::new("editor-result-duplicate-row")
                .ghost()
                .small()
                .icon(IconName::CopyPlus)
                .tooltip("复制此行")
                .disabled(row.is_none())
                .debug_selector(|| "editor-result-duplicate-row".to_string())
                .on_click(move |_, _window, app| {
                    if let Some(row) = row {
                        queue.request(ResultRequest::DuplicateRow(row), app);
                    }
                })
                .into_any_element()
        });
        let delete_row = self.editable.then(|| {
            let queue = self.queue.clone();
            let row = selected_view_row;
            Button::new("editor-result-delete-row")
                .ghost()
                .small()
                .icon(IconName::Trash)
                .tooltip("删除此行")
                .disabled(row.is_none())
                .debug_selector(|| "editor-result-delete-row".to_string())
                .on_click(move |_, _window, app| {
                    if let Some(row) = row {
                        queue.request(ResultRequest::DeleteRow(row), app);
                    }
                })
                .into_any_element()
        });
        // 撤销 / 重做组合钮：格级栈优先、空走行级栈（宿主分派时判定）；常驻注入，
        // 栈空由 `can_undo` / `can_redo` 置灰，槽位不消失。
        let undo = {
            let queue = self.queue.clone();
            Button::new("editor-result-undo")
                .ghost()
                .small()
                .icon(IconName::Undo2)
                .tooltip("撤销")
                .disabled(!self.can_undo)
                .debug_selector(|| "editor-result-undo".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::UndoAction, app);
                })
                .into_any_element()
        };
        let redo = {
            let queue = self.queue.clone();
            Button::new("editor-result-redo")
                .ghost()
                .small()
                .icon(IconName::Redo2)
                .tooltip("重做")
                .disabled(!self.can_redo)
                .debug_selector(|| "editor-result-redo".to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::RedoAction, app);
                })
                .into_any_element()
        };

        // 【B7】导出：格式 × 范围。菜单项由 `export::menu_items_for` 给（纯函数），
        // 静态上下文（连接 / 键列 / 行数）在 sync 时由编辑器算好——render 期只读
        let export = self.export_ctx.as_ref().map(|ctx| {
            let queue = self.queue.clone();
            let ctx = ctx.clone();
            DropdownButton::new("editor-result-export")
                .small()
                .button(
                    Button::new("editor-result-export-btn")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-export".to_string())
                        .label("⤓ 导出"),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    // 【B7 切片三】附加上下文：`SQL Updates` 要有**键列**才摆
                    // （键列由宿主在 sync 时查表结构端口算好；查不到 = 不摆，不猜
                    // 「拿第一列当键」——改错行的代价太高）
                    let key = ctx.key.clone();
                    let extras = export::ExportExtras {
                        sql_text: true,
                        key,
                    };
                    for item in export::menu_items_for(ctx.has_more, &ctx.rows_text, &extras) {
                        if item.separator_before {
                            menu = menu.separator();
                        }
                        let Some((format, scope)) = item.action else {
                            menu = menu.item(PopupMenuItem::label(item.label));
                            continue;
                        };
                        let queue = queue.clone();
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| {
                                queue.request(ResultRequest::Export {
                                    format,
                                    all: matches!(scope, export::ExportScope::All),
                                }, app);
                            },
                        ));
                    }
                    menu
                })
                .into_any_element()
        });

        // 【B15】分析：对选中那份结果的**已抓行**跑聚合 SQL（本地 DuckDB）。
        // 菜单项由编辑器投影（`AnalysisItem`）给出；没网格就不摆。
        let analysis = (!self.analysis_items.is_empty()).then(|| {
            let queue = self.queue.clone();
            let items = self.analysis_items.clone();
            DropdownButton::new("editor-result-analysis")
                .small()
                .button(
                    Button::new("editor-result-analysis-btn")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-analysis".to_string())
                        .label("⚗ 分析"),
                )
                .dropdown_menu(move |menu, _window, _app| {
                    let mut menu = menu;
                    for item in items.iter() {
                        let queue = queue.clone();
                        let sql = item.sql.clone();
                        menu = menu.item(PopupMenuItem::new(item.label.clone()).on_click(
                            move |_, _window, app| {
                                let sql = sql.clone();
                                queue.request(ResultRequest::Analysis { sql }, app);
                            },
                        ));
                    }
                    let queue = queue.clone();
                    menu.separator().item(
                        PopupMenuItem::new(crate::analysis::CUSTOM_LABEL).on_click(
                            move |_, _window, app| {
                                queue.request(ResultRequest::AnalysisCustom, app);
                            },
                        ),
                    )
                })
                .into_any_element()
        });

        let controls = result_grid::ResultControls {
            filter: Some(filter),
            value_sidebar: None,
            compare,
            commit,
            analysis,
            copy,
            export,
            refresh,
            more,
            fetch_all,
            insert_row,
            duplicate_row,
            delete_row,
            undo: Some(undo),
            redo: Some(redo),
        };
        // 【DBeaver 对齐】工具栏横跨整个顶部（从主 pane 里抽出来）
        // 解构 controls，取出工具栏需要的字段；状态栏（面板级第四行）与主 pane
        // 各拿一份：AnyElement 不实现 Clone，状态动作按钮只渲染一处——pane 内已不再
        // 读它们（grid.rs render 只渲染网格），故移动进状态栏，pane.controls 传 default。
        let result_grid::ResultControls {
            filter,
            value_sidebar: _,
            compare,
            commit,
            analysis,
            copy,
            export,
            refresh,
            more,
            fetch_all,
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
        } = controls;
        // 【四行布局】状态栏动作控件 = 解构出的移动字段重组（more / fetch_all / 行级 /
        // 撤销重做 / 导出 / 刷新；`..Default` 兜底其余槽位）。导出 / 刷新 2026-10-28
        // 从工具栏移入状态栏（用户口径：执行信息与导出刷新都归结果集状态栏）
        let status_controls = result_grid::ResultControls {
            more,
            fetch_all,
            insert_row,
            duplicate_row,
            delete_row,
            undo,
            redo,
            export,
            refresh,
            ..Default::default()
        };
        // 【2026-10-28 微调】SQL 预览：工具栏最左侧可点击元素（Button 自带 hover 高亮 +
        // 手型，提示可点击）。点击**弹出窗口**显示完整原始 SQL（kit Dialog + 只读
        // Textarea）；hover 仍给全文 tooltip。
        let sql_preview = toolbar.sql.clone().map(|sql| {
            let preview: SharedString = if sql.len() > 60 {
                format!("{}…", &sql[..60]).into()
            } else {
                sql.as_str().into()
            };
            let queue = self.queue.clone();
            Button::new("editor-result-sql-preview")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-sql-preview".to_string())
                .label(SharedString::from(format!("SQL: {}", preview)))
                .tooltip("点击查看完整 SQL（弹出窗口）")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::OpenSqlDialog, app);
                })
                .into_any_element()
        });
        // 工具栏：右栏开关已移状态栏右端，这里不再摆（None）；导出/刷新已移状态栏
        let toolbar_el = result_grid::render_toolbar(
            sql_preview,
            filter,
            Some(pushdown),
            Some(comments),
            compare,
            commit,
            analysis,
            copy,
            cx,
        );
        // 状态栏右段耗时取自工具栏投影；先取出再移动 `toolbar` 进 pane
        let elapsed_ms = toolbar.elapsed_ms;
        // 【状态栏三项】右段只显示 内存 / 查询时间 / 查询条数（用户口径 2026-10-28）。
        // 内存文案在这里从 toolbar 投影（取完就随 toolbar 移进 pane，不能再读）：
        // 内存档报列式占用；表档只标载体；无事实档不摆段。
        let status_memory = match toolbar.fact_tier {
            Some(CurrentTier::InMemory) if toolbar.fact_bytes > 0 => {
                Some(format!("内存 {}", result_grid::bytes_text(toolbar.fact_bytes)))
            }
            Some(CurrentTier::InMemory) => Some("内存档".to_string()),
            Some(CurrentTier::OnDuckDb) | Some(CurrentTier::None) | None => None,
        };
        // 【状态栏三项】写语句影响行数（网格行数由 status 提供；这里补写语句口径）
        let status_affected = toolbar.affected_rows;
        // 【四行布局】状态行渲染条件放宽：网格有状态行；写语句 / 失败只有执行信息
        // （affected_rows / failed / elapsed_ms）也给状态栏——用户口径：导出 / 刷新 /
        // 执行信息都在状态栏，写语句也要能刷新（须在 `toolbar` 移动进 pane 之前算）
        let show_status = self.status.is_some()
            || toolbar.affected_rows.is_some()
            || toolbar.failed
            || toolbar.elapsed_ms.is_some();

        let result_pane = result_grid::render(
            &self.grid,
            result_grid::ResultPane {
                toolbar,
                // 状态行已抽到面板级第四行（render_status_row），pane 内不再渲染
                status: None,
                // 状态动作按钮只由 render_status_row 消费，pane 不需要第二份
                controls: result_grid::ResultControls::default(),
                card,
                // 标签条已提到面板级第一行（跨全宽），主 pane 不再渲染
                tabs: None,
                notice: self.notice.clone(),
                row_count_input: None,
            },
            cx,
        )
        .into_any_element();

        // 【四行布局】（2026-10-28 拍板）：
        //   第一行 tabs（跨全宽）→ 第二行工具栏（跨全宽）→ 第三行主体（左栏|内容|右栏）
        //   → 第四行状态栏（跨全宽，左端左栏开关 / 右端右栏开关）。
        // 左右栏 DBeaver 式：默认展开（对齐 DBeaver 常驻），状态栏两端开关可完全收起
        // （收起 = 不占宽度）。对比模式双栏，左右辅助栏让位（三栏会把网格挤没，
        // 辅助栏信息在双栏里都有）。

        // 【侧栏】左栏开关（状态栏最左）：左侧栏 = 网格 / 文本 / 记录竖条。
        // 收起态图标用 PanelLeftOpen（点击动作 = 展开），展开态 PanelLeft（当前状态）。
        let left_switch = {
            let queue = self.queue.clone();
            Button::new("editor-result-left-sidebar-toggle")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-left-sidebar-toggle".to_string())
                .toggled(self.show_left_sidebar)
                .icon(if self.show_left_sidebar {
                    IconName::PanelLeft
                } else {
                    IconName::PanelLeftOpen
                })
                .tooltip(if self.show_left_sidebar {
                    "收起左栏（网格 / 文本 / 记录）"
                } else {
                    "展开左栏（网格 / 文本 / 记录）"
                })
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ToggleLeftSidebar, app);
                })
                .into_any_element()
        };

        // 【2026-10-28 微调】右侧开关（状态栏右端）：三态循环（收起 → 窄展开 → 宽展开 →
        // 收起；比左侧开关多一个展开态）。图标：PanelRight（收起）/ PanelRightDashed（窄展开，
        // 只显右缘竖条图标条）/ PanelRightOpen（宽展开，竖条 + 右栏内容）。
        let right_switch = {
            let queue = self.queue.clone();
            let (icon, toggled, tooltip) = match (self.edge_bar_visible, self.show_value_sidebar) {
                (false, _) => (IconName::PanelRight, false, "展开右栏（图标条）"),
                (true, false) => (IconName::PanelRightDashed, true, "展开右栏（内容）"),
                (true, true) => (IconName::PanelRightOpen, true, "收起右栏"),
            };
            Button::new("editor-result-right-sidebar-toggle")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-right-sidebar-toggle".to_string())
                .toggled(toggled)
                .icon(icon)
                .tooltip(tooltip)
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::ToggleValueSidebar, app);
                })
                .into_any_element()
        };

        // 第三行主体：文本视图 / 普通视图 / 对比模式 三态
        let body = if self.presentation == Presentation::Text && !self.compare_mode {
            // 文本视图：左栏（可收起）+ 文本主区；右栏让位（无选中格语义）
            let mut row = div()
                .h_flex()
                .items_stretch()
                .w_full()
                .h_full()
                .min_h_0()
                .debug_selector(|| "editor-result-text-view".to_string());
            // 【侧栏丝滑】左右栏容器**始终挂载**（内部宽度补间 + 按 show 挂内容），
            // 不再条件渲染——收起 = 宽度平滑收缩，展开 = 宽度露出内容
            row = row.child(self.render_vertical_bar(cx));
            row.child(div().flex_1().min_w_0().h_full().child(self.render_text_view(cx)))
                .into_any_element()
        } else if !self.compare_mode {
            // 普通视图：左栏（可收起）+ 主 pane + 右栏（可收起）+ 右缘竖条（常驻）
            let mut row = div()
                .h_flex()
                .items_stretch()
                .w_full()
                .h_full()
                .min_h_0()
                .debug_selector(|| "editor-result-sidebars".to_string());
            row = row.child(self.render_vertical_bar(cx));
            row = row.child(div().flex_1().min_w_0().h_full().child(result_pane));
            row = row.child(self.render_right_sidebar(cx));
            // 【右缘竖条】三态开关控制可见性（`edge_bar_visible`）；有可复制语义
            // （有网格结果）时才挂（与右栏页签同条件）
            row = row.children(
                (self.edge_bar_visible && self.result_can_copy)
                    .then(|| self.render_right_edge_bar(cx)),
            );
            row.into_any_element()
        } else {
            // 【并排对比】对比模式双栏：左栏 = 主选中（上面组装好的主 pane），
            // 右栏 = 对比选中（自己的标签条 + 只读网格）。右栏数据由编辑器
            // `sync_compare_grid` 提前拷进 `compare_grid`（render 只读网格状态）。
            // 右栏不接 filter/transpose/commit/analysis/export/refresh/more——那些
            // 动作的语义都归主栏，并排对照不需要第二套动作。
            let compare_tabs = (self.tabs.len() >= 2).then(|| {
                let queue = self.queue.clone();
                let compare_active = self.compare_active;
                result_sets::render(
                    &self.tabs,
                    compare_active,
                    move |index, _window, app| {
                        let index = *index;
                        queue.request(ResultRequest::SelectCompare(index), app);
                    },
                    result_sets::TabActions::default(),
                    cx,
                    &self.tabs_scroll,
                )
                .into_any_element()
            });
            // 对比栏的工具栏 / 状态行：编辑器在 `sync_compare_grid` 时投影好存进
            // `compare_toolbar` / `compare_status`（与主栏同口径；只读，互动真值不需要）
            let compare_pane = self.compare_grid.as_ref().map(|compare_grid| {
                result_grid::render(
                    compare_grid,
                    result_grid::ResultPane {
                        toolbar: self.compare_toolbar.clone().unwrap_or_default(),
                        status: self.compare_status.clone(),
                        controls: result_grid::ResultControls::default(),
                        card: None,
                        tabs: compare_tabs,
                        notice: None,
                        row_count_input: None,
                    },
                    cx,
                )
                .into_any_element()
            });
            match compare_pane {
                // 双栏：中间一条竖分隔线（结构元素用 `ui::HAIRLINE`），两栏各占一半
                Some(compare_pane) => div()
                    .h_flex()
                    .items_stretch()
                    .w_full()
                    .h_full()
                    .min_h_0()
                    .debug_selector(|| "editor-result-compare-layout".to_string())
                    .child(
                        div()
                            .h_flex()
                            .items_stretch()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .border_r(ui::HAIRLINE)
                            .debug_selector(|| "editor-result-compare-main".to_string())
                            // 对比模式竖条只挂主栏左侧：视图方式切换只作用于主栏，对比栏
                            // 只读；竖条 = 左栏，跟随 `show_left_sidebar` 可收起（宽度动画在
                            // render_vertical_bar 内部，此处始终挂载）
                            .child(self.render_vertical_bar(cx))
                            .child(div().flex_1().min_w_0().h_full().child(result_pane)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .debug_selector(|| "editor-result-compare-pane".to_string())
                            .child(compare_pane),
                    )
                    .into_any_element(),
                // 无对比选中（对比开关开着但没选中可比对象）：退化为单主栏
                None => div()
                    .h_flex()
                    .items_stretch()
                    .w_full()
                    .h_full()
                    .min_h_0()
                    .debug_selector(|| "editor-result-sidebars".to_string())
                    .child(self.render_vertical_bar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(result_pane))
                    .into_any_element(),
            }
        };

        // 第四行：状态栏（跨全宽，横跨左右栏之下；左端左栏开关、右端右栏开关）。
        // 文本视图也有 status（行数等），故状态栏在三种主体形态下都渲染。
        // 【四行布局】状态行渲染条件放宽：网格有状态行；写语句 / 失败只有执行信息
        // （affected_rows / failed / elapsed_ms）也给状态栏——用户口径：导出 / 刷新 /
        // 执行信息都在状态栏，写语句也要能刷新（cell_edit_esc 回归教训同源）
        let status_row = show_status.then(|| {
            result_grid::render_status_row(
                self.status.as_ref(),
                status_controls,
                self.status.as_ref().and(Some(&self.row_count_input)),
                elapsed_ms,
                Some(left_switch),
                // 【2026-10-28 微调】右侧开关回状态栏右端（三态：收起 / 窄展开 / 宽展开）
                Some(right_switch),
                status_memory,
                status_affected,
                cx,
            )
            .into_any_element()
        });

        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .debug_selector(|| "editor-result-root".to_string())
            // 第一行：结果集标签条（跨全宽；含 C1 重命名条）
            .children(tabs)
            // 第二行：工具栏（跨全宽）
            .child(toolbar_el)
            // 【1.2】筛选展开条：点击筛选组展开按钮后，工具栏下方顶宽输入框 + ▶ + ✕
            .children(self.filter_expanded.then(|| self.render_filter_expand_bar(cx)))
            // 第三行：主体
            .child(body)
            // 第四行：状态栏
            .children(status_row)
            // 【2026-10-28 微调】SQL 预览弹窗**最后挂**（绘制顺序在全部内容之上，
            // 浮层才能盖住工具栏 / 主体；遮罩 absolute inset_0 铺满面板根）
            .children(self.sql_dialog_open.then(|| self.render_sql_dialog(cx)))
            .into_any_element()
    }


    /// 【B15】筛选输入变化（300ms 防抖，到点才重算视图行序）
    pub fn on_filter_input(&mut self, text: String, cx: &mut Context<Self>) {
        if self.filter_text == text {
            return;
        }
        self.filter_text = text;
        self.filter_version = self.filter_version.wrapping_add(1);
        let version = self.filter_version;
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            executor.timer(std::time::Duration::from_millis(300)).await;
            _ = weak.update(cx, |panel, cx| {
                if panel.filter_version == version {
                    panel.apply_filter(cx);
                }
            });
        });
        *self.filter_debounce.borrow_mut() = Some(task);
    }

    /// 【B15】把当前筛选词应用到网格（事件路径：重算一次视图行序）。
    /// 下发源库开着且词非空时，同一份词也**入队**一次（编辑器重查，落新结果集）
    pub fn apply_filter(&mut self, cx: &mut Context<Self>) {
        self.apply_filter_inner(cx, true);
    }

    /// 【B15】应用筛选的公共实现；`requeue=false` 时不触发「下发源库」重查入队
    /// （drain 分派路径用，防队列死循环）
    fn apply_filter_inner(&mut self, cx: &mut Context<Self>, requeue: bool) {
        let filter = self.filter_text.clone();
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(filter);
            state.refresh(cx);
        });
        self.refresh_filter_hint(cx);
        cx.notify();
        if requeue && self.pushdown && !self.filter_text.trim().is_empty() {
            self.queue
                .push(ResultRequest::PushdownFilter { value: self.filter_text.trim().to_string() });
        }
    }

    /// 清除筛选（工具栏那个 ✕）：输入框清空 + 立即应用空词。
    /// 需要 `window`（`InputState::set_value` 的签名）——编辑器分派
    /// [`ResultRequest::ClearFilter`] 时转手调用
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_text.clear();
        self.filter_version = self.filter_version.wrapping_add(1);
        let input = self.filter_input.clone();
        input.update(cx, |state, cx| state.set_value("", window, cx));
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(String::new());
            state.refresh(cx);
        });
        self.refresh_filter_hint(cx);
        cx.notify();
    }

    /// 【B14】下发源库开关翻转（状态面板内改；开着且有词时重查入队）
    pub fn set_pushdown(&mut self, open: bool) {
        self.pushdown = open;
        if open && !self.filter_text.trim().is_empty() {
            self.queue.push(ResultRequest::PushdownFilter {
                value: self.filter_text.trim().to_string(),
            });
        }
    }

    /// 【P2-4】转置开关：面板状态翻转 + 网格同步（事件路径，`drain` 分派时调用）
    pub fn toggle_transpose(&mut self, cx: &mut Context<Self>) {
        let open = !self.transposed;
        self.transposed = open;
        // 【DBeaver 对齐】打开记录视图时，从当前选中行开始（而不是固定第 0 行）；
        // 没有选中行时保持 transpose_row 不变（默认 0）。
        if open {
            if let Some(status) = &self.status {
                if let Some(selected) = status.selected_row {
                    self.transpose_row = selected;
                }
            }
        }
        let row = self.transpose_row;
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_transposed(open, row);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【P2-4】转置行导航：上一行 / 下一行（越界夹取；基准 = 当前视图序列长度，
    /// 本地行区段也纳入导航范围——#28-B1 拍板 D2 转置纳入本地行）
    pub fn transpose_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let max = self
            .grid
            .read(cx)
            .delegate()
            .view_len()
            .saturating_sub(1);
        let next = (self.transpose_row as isize + delta).clamp(0, max as isize) as usize;
        self.transpose_row = next;
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_transpose_row(next);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【并排对比】对比模式开关（编辑器 `ToggleCompareMode` 分派回来调；翻转后
    /// 对比栏投影由编辑器 `sync_compare_grid` 续写——数据源在 ResultStore）
    pub fn toggle_compare_mode(&mut self, cx: &mut Context<Self>) {
        self.compare_mode = !self.compare_mode;
        cx.notify();
    }

    /// 【呈现】显示方式切换（网格 ↔ 文本）。面板级持续——切结果集不重置
    /// （用户选的是「怎么看结果」，不是某份结果的属性）。
    pub fn set_presentation(&mut self, presentation: Presentation, cx: &mut Context<Self>) {
        self.presentation = presentation;
        cx.notify();
    }

    /// 【右栏】标签页切换（列浏览器 / 值详情 / 计算 / 分组 / 引用）。
    /// 右栏收起时切页签会自动展开（DBeaver 口径：右缘竖条点图标即展开对应面板）。
    pub fn set_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.right_tab = tab;
        if !self.show_value_sidebar {
            // 窄展开 / 收起态点页签图标 → 宽展开（竖条可见性保持；收起态在窄态已置 true）
            self.edge_bar_visible = true;
            self.show_value_sidebar = true;
            self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        }
        cx.notify();
    }

    /// 【1.2】筛选框展开 / 收起（点击筛选组展开按钮 / 展开条收起按钮触发）
    pub fn toggle_filter_expand(&mut self, cx: &mut Context<Self>) {
        self.filter_expanded = !self.filter_expanded;
        cx.notify();
    }

    /// 【2026-10-28 微调】SQL 预览**弹出窗口**：置 open 标志 + 懒建只读 Textarea 实体
    /// （`TextareaState::new` 需要 window，走端口事件路径调用——面板 render 只有
    /// &App 建不了）。SQL 变化时 `set_value` 刷新；render 期 `Textarea::new(&entity)`
    /// 直接挂浮层（只需 &Entity）。
    pub fn open_sql_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sql = self
            .exec_sql
            .clone()
            .or_else(|| self.result_sql.clone())
            .unwrap_or_default();
        if self.sql_textarea.is_none() {
            let textarea = cx.new(|cx| TextareaState::new(window, cx).default_value(sql));
            self.sql_textarea = Some(textarea);
        } else if let Some(ta) = self.sql_textarea.clone() {
            ta.update(cx, |state, cx| state.set_value(sql.clone(), window, cx));
        }
        self.sql_dialog_open = true;
        cx.notify();
    }

    /// 【2026-10-28 微调】关闭 SQL 预览弹窗（✕ 按钮 / Esc 触发）
    pub fn close_sql_dialog(&mut self, cx: &mut Context<Self>) {
        self.sql_dialog_open = false;
        cx.notify();
    }

    /// 【1.2】筛选框运行后宿主写入 wrapped SQL（执行完成后），并切到「执行SQL」页签    /// （右栏收起时自动展开——用户要在右侧栏看到执行 SQL）
    pub fn set_exec_sql(&mut self, sql: Option<String>, cx: &mut Context<Self>) {
        self.exec_sql = sql;
        self.right_tab = RightTab::ExecSql;
        if !self.show_value_sidebar {
            self.edge_bar_visible = true;
            self.show_value_sidebar = true;
            self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        }
        cx.notify();
    }

    /// 【侧栏】右侧开关：**三态循环**（比左侧多一个展开态；开关在状态栏右端）——
    /// 收起（竖条 + 右栏都不显示）→ 窄展开（只显右缘竖条图标条）→ 宽展开（竖条 + 右栏
    /// 内容）→ 收起。epoch 递增让宽度动画重播（首帧 epoch=0 免动画，见 `sidebar_width_anim`）
    pub fn toggle_value_sidebar(&mut self, cx: &mut Context<Self>) {
        let (edge, show) = next_right_edge_state(self.edge_bar_visible, self.show_value_sidebar);
        self.edge_bar_visible = edge;
        self.show_value_sidebar = show;
        self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        cx.notify();
    }

    /// 【侧栏】左侧栏（网格 / 文本 / 记录竖条）开关：只翻面板状态（网格数据不动）；
    /// epoch 递增同上
    pub fn toggle_left_sidebar(&mut self, cx: &mut Context<Self>) {
        self.show_left_sidebar = !self.show_left_sidebar;
        self.left_sidebar_epoch = self.left_sidebar_epoch.wrapping_add(1);
        cx.notify();
    }

    /// 【C1】进入第 `index` 份结果集 tab 的重命名态（预填当前 label；会话级）。
    ///
    /// 输入框实体懒创建：第一次进时建，后续复用。提交走 [`Self::commit_rename`]，
    /// 取消走 [`Self::cancel_rename`]（都由面板渲染的 ✓/✕ 按钮触发）。
    pub fn begin_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        // 预填当前 label（自定义标题或自动序号都显示出来供改）
        let prefill = self.tabs[index].label.clone();
        if self.renaming_input.is_none() {
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_placeholder("结果集标题（留空恢复序号）…", window, cx);
                state
            });
            self.renaming_input = Some(input);
        }
        if let Some(input) = self.renaming_input.clone() {
            input.update(cx, |state, cx| state.set_value(prefill, window, cx));
        }
        self.renaming = Some(index);
        cx.notify();
    }

    /// 【C1】提交重命名：把输入框当前值发回宿主（空串 = 恢复自动标签）。
    pub fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.renaming else {
            return;
        };
        let value = self
            .renaming_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        // 空串 / 纯空白 → None（恢复自动标签）
        let normalized = if value.trim().is_empty() {
            None
        } else {
            Some(value.trim().to_string())
        };
        self.queue.push(ResultRequest::RenameSet(index, normalized));
        self.renaming = None;
        cx.notify();
    }

    /// 【C1】取消重命名（不发请求，输入框内容保留下次再进）。
    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
    }

    /// 【B1】切换列头直接显示注释（面板级临时开关；host 也可从全局偏好同步过来）。
    pub fn set_show_column_comments(&mut self, on: bool, cx: &mut Context<Self>) {
        self.show_column_comments = on;
        cx.notify();
    }

    /// 【B1】当前是否直接显示注释（delegate 读这个决定列头画不画第二行注释）。
    pub fn show_column_comments(&self) -> bool {
        self.show_column_comments
    }

    /// 【类型样式】同步全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    ///
    /// host 构造时（`settings.result_panel.type_style()`）与 workbench 设置变更钩子
    /// （设置页改动 → 遍历 editor_hosts → 本方法）都会调用；面板再把偏好推给 delegate，
    /// 由 delegate 的 `render_td` / `render_th` 消费——不经过 `set_data`，不会丢。
    /// 【结果集 #296】宿主注入持久化的列序（重启恢复；空 = 默认序）。
    /// 只在文档重开时调用一次，换结果集不清空（列名匹配由 `apply_column_order` 做）。
    pub fn set_column_order(&mut self, order: Vec<String>) {
        self.column_order = order;
    }

    /// 【结果集 #296】当前持久化目标列序（宿主保存会话时读取）
    pub fn column_order(&self) -> Vec<String> {
        self.column_order.clone()
    }

    /// 【结果集 #296】按持久化列名序重排一份新结果的列与行（列名匹配，未匹配列
    /// 保持数据序附加在末尾——用户拖过的列还在就保持，新列不猜）。
    fn apply_column_order(&self, columns: Vec<String>, rows: Vec<Vec<String>>) -> (Vec<String>, Vec<Vec<String>>) {
        reorder_by(&self.column_order, columns, rows)
    }

    pub fn set_type_style(&mut self, prefs: TypeStylePrefs, cx: &mut Context<Self>) {        self.type_style = prefs.clone();
        self.grid.update(cx, |state, cx| {
            state.delegate_mut().set_type_style(prefs);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【侧栏】最近选中格位置（`TableEvent::SelectCell` 事件路径写入；选行 / 清选 =
    /// `None`）。值详情只跟随**格**选中——选整行时不投影单格。
    ///
    /// 同时把选中格反推成**数据列序**写入 [`Self::selected_col`]，驱动右栏列浏览器
    /// 清单高亮与底部明细；选行 / 清选 / 转置视图 / 行号槽列时不反推（保持清单当前选中）。
    pub fn set_selection_pos(
        &mut self,
        pos: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection_pos = pos;
        if let Some((_row, col)) = pos {
            if col > 0 {
                let delegate = self.grid.read(cx).delegate();
                if !delegate.is_transposed() {
                    let data_col = delegate.data_column(col - 1);
                    self.selected_col = Some(data_col);
                    // 同步清单高亮：把面板权威的选中列换算成 List 过滤后行号，
                    // 驱动 kit List 内部 selected_index（过滤掉该列时无对应行 → 不清高亮）
                    let browser = self.browser_list.clone();
                    browser.update(cx, |list, cx| {
                        let ix = list
                            .delegate()
                            .matched()
                            .iter()
                            .position(|&c| c == data_col)
                            .map(IndexPath::new);
                        list.set_selected_index(ix, window, cx);
                    });
                }
            }
        }
        cx.notify();
    }

    /// 【列浏览器】清单单击选中一列（数据列序）：写清单选中态 + 网格列选中高亮联动。
    /// 清单选中 ≠ 网格焦点——这里只把网格列选中用于高亮，不回写 `selection_pos`
    /// （避免清单选中把值详情也拽走；值详情仍只跟网格格焦点）。
    pub fn select_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                state.set_selected_col(view_col, cx);
            }
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【列浏览器】清单双击定位：网格滚到该列首行并选中（列不可见 = 隐藏列，不动作）。
    pub fn locate_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                state.set_selected_cell(0, view_col, cx);
            }
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【列浏览器】清空搜索词（搜索框 ✕ / ESC 入口；`window` 转手给 List 内置
    /// 搜索框，与 `clear_filter` 同口径）
    pub fn clear_column_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let browser = self.browser_list.clone();
        browser.update(cx, |list, cx| list.set_query("", window, cx));
        cx.notify();
    }

    /// 计算 / 分组页当前作用列（**数据列序**）：网格选中格优先（`selection_pos`），
    /// 列浏览器清单选中列（`selected_col`）兜底；转置视图 / 行号槽列不给语义列，
    /// 兜底列被隐藏时不返回。
    fn inspect_column(&self, delegate: &ResultGridDelegate) -> Option<usize> {
        if self.transposed {
            return None;
        }
        if let Some((_, col)) = self.selection_pos {
            if col > 0 {
                let view_col = col - 1;
                if view_col < delegate.columns().len() {
                    return Some(delegate.data_column(view_col));
                }
            }
        }
        self.selected_col
            .filter(|c| *c < delegate.columns().len() && !delegate.is_hidden_column(*c))
    }

    /// 【呈现】文本视图（DBeaver「显示方式 → 文本」）：当前结果拼 TSV 纯文本。
    ///
    /// 头部工具行只放「网格」切换（工具栏按钮在网格模式才有，文本模式必须自带
    /// 出口，否则进了文本出不来）；文本区等宽字体、可滚动、可选中复制——原样
    /// 文本不经表格，长结果也能全文选中。
    fn render_text_view(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let mono = theme.mono_font_family.clone();
        let queue = self.queue.clone();
        let grid = self.grid.read(cx);
        let text = grid.delegate().text_presentation();
        div()
            .v_flex()
            .h_full()
            .min_h_0()
            .debug_selector(|| "editor-result-text-view-body".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_6()
                    .border_b(ui::HAIRLINE)
                    .child(
                        Button::new("editor-result-text-head-grid")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-text-head-grid".to_string())
                            .label("网格")
                            .on_click({
                                let queue = queue.clone();
                                move |_, _window, app| {
                                    queue.request(ResultRequest::SetPresentation(Presentation::Grid), app);
                                }
                            }),
                    )
                    .child(
                        Button::new("editor-result-text-head-text")
                            .ghost()
                            .small()
                            .toggled(true)
                            .label("文本"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .font_family(mono)
                            .text_xs()
                            .whitespace_normal()
                            .child(SharedString::from(text)),
                    ),
            )
            .into_any_element()
    }

    /// 【侧栏】左栏：列结构列表（列名 + 类型 + 隐藏切换）。
    ///
    /// 交互 = **单击整行切换隐藏**（隐藏列整行标灰，点击恢复）。不摆图标按钮：
    /// 眼睛类图标不在 kit 图标表，整行点击 + 标灰是同一件事，少一层控件。
    /// 隐藏 / 显示走队列交回宿主（宿主 update 网格 + 面板），与其它工具栏动作一致。
    /// 【左缘竖条】DBeaver 左缘独立窄条：网格 / 文本 / 记录三图标竖排。
    ///
    /// 状态口径（拍板「保留双轨」）：`Presentation{Grid,Text}` 与 `transposed` 仍是
    /// 两条独立状态——网格 / 文本由 `SetPresentation` 显式目标值切换；记录（转置视图）
    /// 由 `ToggleTranspose` 翻转，高亮跟随 `transposed`。转置激活时按钮下方跟上一行 /
    /// 下一行导航（记录视图里翻记录用）。文本态下记录按钮禁用（文本没有记录语义）。
    fn render_vertical_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let is_text = self.presentation == Presentation::Text;
        let show = self.show_left_sidebar;
        let queue = self.queue.clone();
        let grid_btn = Button::new("editor-result-bar-grid")
            .ghost()
            .small()
            .icon(IconName::Table2)
            .debug_selector(|| "editor-result-bar-grid".to_string())
            .toggled(!self.transposed && !is_text)
            .tooltip("网格视图")
            .on_click(move |_, _window, app| {
                queue.request(ResultRequest::SetPresentation(Presentation::Grid), app);
            });
        let queue = self.queue.clone();
        let text_btn = Button::new("editor-result-bar-text")
            .ghost()
            .small()
            .icon(IconName::FileText)
            .debug_selector(|| "editor-result-bar-text".to_string())
            .toggled(is_text)
            .tooltip("文本视图")
            .on_click(move |_, _window, app| {
                queue.request(ResultRequest::SetPresentation(Presentation::Text), app);
            });
        let queue = self.queue.clone();
        let record_btn = Button::new("editor-result-bar-record")
            .ghost()
            .small()
            // 【记录视图图标】单行转置（# / 字段 / 值 竖排）是「一条记录的字段列表」，
            // 用 Lucide `form`（三条横档的表单条）；原 `UserRound`（人形）语义是"用户"，
            // 与记录无关，且与列浏览器里的其他图标气质不符。
            .icon(IconName::Form)
            .debug_selector(|| "editor-result-bar-record".to_string())
            .disabled(is_text)
            .toggled(self.transposed)
            .tooltip("记录视图（单行转置）")
            .on_click(move |_, _window, app| {
                queue.request(ResultRequest::ToggleTranspose, app);
            });
        // 记录视图激活时：竖条按钮下方跟上一行 / 下一行导航（与原工具栏转置组同功能，
        // 只是位置从顶部迁到竖条——动作仍是队列里的 TransposePrev / Next）。
        let record_nav = self.transposed.then(|| {
            let queue = self.queue.clone();
            let prev = Button::new("editor-result-bar-record-prev")
                .ghost()
                .small()
                .icon(IconName::ChevronUp)
                .debug_selector(|| "editor-result-bar-record-prev".to_string())
                .tooltip("上一行")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::TransposePrev, app);
                });
            let queue = self.queue.clone();
            let next = Button::new("editor-result-bar-record-next")
                .ghost()
                .small()
                .icon(IconName::ChevronDown)
                .debug_selector(|| "editor-result-bar-record-next".to_string())
                .tooltip("下一行")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::TransposeNext, app);
                });
            div()
                .v_flex()
                .gap_1()
                .items_center()
                .child(prev)
                .child(next)
                .into_any_element()
        });
        // 【侧栏丝滑】左栏收起 / 展开 = 宽度过渡（EffectTransition 元素级动画，与
        // vendor `sidebar` 组件同款机制：动画状态按 id 挂元素树，render 无需 &mut App）。
        // 容器**始终挂载**（动画驱动宽度，收起后宽 0 不占空间）、内容按 show 挂载——
        // 展开时内容随宽度露出，收起时宽度平滑收缩，消除「条件渲染瞬爆」。每次开合
        // `left_sidebar_epoch` 递增 → 动画 id 变化 → 重播；首帧（epoch=0）from==to 免动画。
        // 注：动画宽度按 gpui 默认 rem 基准（16px）换算，zoom 改 rem_size 属已知限制。
        let full = px(ui::RESULT_VERTICAL_BAR_WIDTH * 16.0);
        let (from, to) = sidebar_width_anim(self.left_sidebar_epoch, show, full);
        let wrapper = div()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .border_r(ui::HAIRLINE)
            .bg(theme.colors.sidebar)
            .debug_selector(|| "editor-result-vertical-bar".to_string())
            .when(show, |el| {
                el.v_flex()
                    .items_center()
                    .gap_1()
                    .pt_2()
                    .child(grid_btn)
                    .child(text_btn)
                    .child(record_btn)
                    .children(record_nav)
            });
        EffectTransition::new(Duration::from_millis(160))
            .ease(ease_in_out_cubic)
            .width(from, to)
            .apply(
                wrapper,
                SharedString::from(format!(
                    "editor-result-left-bar-w-{}",
                    self.left_sidebar_epoch
                )),
            )
            .into_any_element()
    }

    /// 【右缘竖条】最右侧边缘面板图标条（对齐 DBeaver 最右缘）：
    /// - 顶部：右侧边栏开合开关（点击切换 show_value_sidebar）
    /// - 下方：预留面板切换图标位置（图表/统计等，backlog）
    ///
    /// 宽度与左缘竖条一致；背景 sidebar；按钮 ghost 小尺寸。
    /// 【右栏·列浏览器页签】（对齐 DBeaver metadata 面板）：顶部搜索框（kit List 内置）+
    /// 中部列清单（kit List）+ 底部元数据明细，整体作为右栏「列浏览器」页签内容。
    ///
    /// - 搜索：列名 / 类型名 / 注释忽略大小写包含匹配（List 内置搜索框 → `perform_search`）；
    ///   隐藏列保留在清单（置灰斜体）。
    /// - 清单：单击选中列（List `confirm` → 网格列高亮联动），双击定位到该列首行；
    ///   行尾眼睛 = 切换隐藏（单击眼睛不改变选中列）；冻结 / 排序 / 统计 / 复制 / 显示全部
    ///   走右键菜单（清单行上不摆图钉 / 排序箭头 / 拖拽手柄，见拍板口径）。
    /// - 明细：只读 `selected_col`（越界 / 被隐藏时兜底第一可见列）。
    fn render_browser_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let columns = delegate.columns().to_vec();
        let types = delegate.column_types().to_vec();
        let annotations = delegate.column_annotations().to_vec();
        let type_names: Vec<String> = types
            .iter()
            .map(|t| t.as_ref().map(|t| t.name.clone()).unwrap_or_default())
            .collect();

        // 明细兜底：当前选中列越界 / 被隐藏时退回第一可见列；一份结果无可见列 = None
        let detail_col = self
            .selected_col
            .filter(|c| *c < columns.len() && !delegate.is_hidden_column(*c))
            .or_else(|| (0..columns.len()).find(|c| !delegate.is_hidden_column(*c)));

        // 明细素材在 read 期间一次取好（标题 + 键值行）；渲染闭包不持有 delegate 借用
        let detail: Option<(String, Vec<(String, String)>)> = detail_col.and_then(|data_col| {
            // 【修复】中间态：columns 非空但 type_names 为空时越界——加边界检查
            if data_col >= columns.len() || data_col >= type_names.len() {
                return None;
            }
            let name = columns[data_col].clone();
            let type_name = type_names[data_col].clone();
            let title = if type_name.is_empty() {
                name.clone()
            } else {
                format!("{name} · {type_name}")
            };
            let mut items: Vec<(String, String)> = vec![
                (
                    "可见".into(),
                    if delegate.is_hidden_column(data_col) {
                        "隐藏".to_string()
                    } else {
                        "显示".to_string()
                    },
                ),
                (
                    "冻结".into(),
                    if delegate.is_frozen(data_col) {
                        "是".to_string()
                    } else {
                        "否".to_string()
                    },
                ),
            ];
            if let Some(annotation) = annotations.get(data_col).cloned().flatten() {
                if let Some(table) = annotation.source_table {
                    items.push(("来源表".into(), table));
                }
                if let Some(source) = annotation.source_column {
                    items.push(("来源列".into(), source));
                }
                if let Some(comment) = annotation.comment {
                    items.push(("注释".into(), comment));
                }
                if let Some(data_type) = annotation.data_type {
                    items.push(("驱动类型".into(), data_type));
                }
                if let Some(nullable) = annotation.nullable {
                    items.push(("可空".into(), if nullable { "是".into() } else { "否".into() }));
                }
                if let Some(default_value) = annotation.default_value {
                    items.push(("默认值".into(), default_value));
                }
            }
            Some((title, items))
        });

        // 清单：kit List（搜索框内置 + 键盘导航 + 滚动条）；行内容 / 眼睛 / 右键菜单 /
        // 双击定位逻辑全在 [`crate::column_browser::ColumnBrowserDelegate`]（render 期
        // 只读实体挂元素，List 内部 prepare 时由 delegate 现算过滤结果）
        let browser = self.browser_list.clone();
        // 【2026-10-28 微调】表头行：与 List 行（列名 flex_1 / 类型 flex_none / 眼睛操作列）
        // 同 padding（px_3/py_1/gap_x_1 = kit ListItem 行内边距）对齐，列浏览器呈现
        // "表格 + 内嵌操作行"形态（行渲染仍在 delegate，眼睛即行内操作）。
        let table_head = div()
            .h_flex()
            .items_center()
            .gap_x_1()
            .px_3()
            .py_1()
            .border_b(ui::HAIRLINE)
            .text_xs()
            .text_color(muted)
            .child(div().flex_1().min_w_0().child("列名"))
            .child(div().flex_none().child("类型"))
            .child(div().flex_none().child(""))
            .into_any_element();
        let list_body: AnyElement = List::new(&browser)
            .with_size(Size::Small)
            .search_placeholder("搜索列名 / 类型 / 注释…")
            .scrollbar_visible(true)
            .flex_1()
            .min_h_0()
            .into_any_element();


        let detail_body: AnyElement = match detail {
            Some((title, items)) => div()
                .v_flex()
                .h_full()
                .debug_selector(|| "editor-result-col-browser-detail".to_string())
                .child(
                    div()
                        .px_2()
                        .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                        .h_flex()
                        .items_center()
                        .text_xs()
                        .text_color(muted)
                        .text_ellipsis()
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .children(items.into_iter().map(|(k, v)| kv_row(&k, &v, cx))),
                )
                .into_any_element(),
            None => self.sidebar_empty("暂无列", cx),
        };

        // 页签内容区：宽度 / 边框 / 背景由右栏外壳统一控制，这里只排上下三段
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .v_flex()
            .min_h_0()
            .debug_selector(|| "editor-result-browser-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("列浏览器 · {} 列", columns.len()))),
            )
            .child(table_head)
            .child(list_body)
            .child(
                div()
                    .border_t(ui::HAIRLINE)
                    .h(rems(ui::RESULT_BROWSER_DETAIL_HEIGHT))
                    .flex_none()
                    .v_flex()
                    .min_h_0()
                    .child(detail_body),
            )
            .into_any_element()
    }

    /// 【侧栏】右栏：对齐 DBeaver Data Editor 右侧辅助区，四页签——
    /// 列浏览器（metadata）/ 值详情（数值查看器）/ 计算（列统计）/ 分组（顺序对齐 DBeaver）。
    ///
    /// 页签用 kit `TabBar`（不手搓）；切页走队列交宿主 `set_right_tab`。
    /// 「引用」页留 backlog（见 result-panel.md §8.12）。计算 / 分组是纯函数
    /// （`column_stats` / `group_by_count`，不引 polars——见 result-panel.md §8.8）；
    /// 每次渲染按当前可见行现算（中小结果集可接受，大表本地聚合的缓存 / 下发
    /// GROUP BY 是后续项，记在文档）。
    fn render_right_sidebar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let show = self.show_value_sidebar;
        // 【2026-10-28 微调】横 tab 栏**只显示当前点击的页签**（其他页签不显示）：
        // 页签切换靠右缘竖条图标（render_right_edge_bar），横条只当「当前页签标题 +
        // 关闭钮」使用（DBeaver 侧栏收起后只留边缘图标，展开时标题区极简）。
        let active_tab = match self.right_tab {
            RightTab::Browser => (IconName::Table2, "列浏览器"),
            RightTab::Value => (IconName::Eye, "值详情"),
            RightTab::Stats => (IconName::Sigma, "计算"),
            RightTab::Group => (IconName::Funnel, "分组"),
            RightTab::Reference => (IconName::Link, "引用"),
            RightTab::ExecSql => (IconName::Braces, "执行SQL"),
        };
        let children: Vec<Tab> = std::iter::once(active_tab)
            .map(|(icon, label)| {
                Tab::new()
                    .icon(icon)
                    .label(SharedString::from(label))
            })
            .collect();
        let tab_bar = TabBar::new("editor-result-right-tabs")
            .segmented()
            .with_size(Size::Small)
            .selected_index(0)
            .children(children)
            // 单 tab 只作标题展示；页签切换在右缘竖条（render_right_edge_bar）
            .on_click(move |_index, _window, _app| {});
        let content = match self.right_tab {
            RightTab::Browser => self.render_browser_tab(cx),
            RightTab::Value => self.render_value_tab(cx),
            RightTab::Stats => self.render_stats_tab(cx),
            RightTab::Group => self.render_group_tab(cx),
            RightTab::Reference => self.render_reference_tab(cx),
            RightTab::ExecSql => self.render_exec_sql_tab(cx),
        };
        // 【页签关闭】非默认「列浏览器」页时，行尾给一个 ×：退回默认页（不收起右栏——
        // 拍板口径：× 只退页，右栏开合仍由工具栏「列浏览器」开关控制）。
        let close = (self.right_tab != RightTab::Browser).then(|| {
            let queue = self.queue.clone();
            Button::new("editor-result-right-tab-close")
                .ghost()
                .small()
                .icon(IconName::X)
                .debug_selector(|| "editor-result-right-tab-close".to_string())
                .tooltip("关闭页签，返回列浏览器")
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::SetRightTab(RightTab::Browser), app);
                })
                .into_any_element()
        });
        // 【侧栏丝滑】右栏收起 / 展开与左栏同法：宽度过渡 + 容器始终挂载（宽 0 不占
        // 空间），页签条与内容按 show 挂载（收起时无焦点残留）。开合由
        // `right_sidebar_epoch` 递增重播动画；首帧（epoch=0）免动画。
        let full = px(ui::RESULT_SIDEBAR_RIGHT_WIDTH * 16.0);
        let (from, to) = sidebar_width_anim(self.right_sidebar_epoch, show, full);
        let wrapper = div()
            .h_full()
            .flex_none()
            .overflow_hidden()
            .border_l(ui::HAIRLINE)
            .bg(theme.colors.sidebar)
            .debug_selector(|| "editor-result-value-sidebar".to_string())
            .when(show, |el| {
                el.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                        .flex_1()
                        .min_w_0()
                        .child(div().flex_1().min_w_0().child(tab_bar))
                        .children(close),
                )
                .child(content)
            });
        EffectTransition::new(Duration::from_millis(160))
            .ease(ease_in_out_cubic)
            .width(from, to)
            .apply(
                wrapper,
                SharedString::from(format!(
                    "editor-result-right-bar-w-{}",
                    self.right_sidebar_epoch
                )),
            )
            .into_any_element()
    }

    /// 【2026-10-28 微调】右缘竖条：页签快捷图标区（列浏览器 / 值详情 / 计算 / 分组 /
    /// 引用 / 执行SQL），点击切页签（右栏收起时自动展开，见 [`Self::set_right_tab`]），
    /// 当前页签高亮。开合开关**已移到状态栏右端**（`right_switch` 三态：收起 / 窄展开 /
    /// 宽展开），竖条顶部不再挂开关。
    /// - 常驻固定宽度（与左缘竖条一致 `RESULT_VERTICAL_BAR_WIDTH`）；可见性由
    ///   `edge_bar_visible` 决定（窄展开 = 只显本竖条；宽展开 = 竖条 + 右栏内容）。
    fn render_right_edge_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        // 页签快捷图标：与右栏页签同一组图标（见 render_right_sidebar）
        let tabs = [
            (RightTab::Browser, IconName::Table2, "列浏览器"),
            (RightTab::Value, IconName::Eye, "值详情"),
            (RightTab::Stats, IconName::Sigma, "计算"),
            (RightTab::Group, IconName::Funnel, "分组"),
            (RightTab::Reference, IconName::Link, "引用"),
            (RightTab::ExecSql, IconName::Braces, "执行SQL"),
        ];
        let queue = self.queue.clone();
        let active = self.right_tab;
        div()
            .h_full()
            .flex_none()
            .w(px(ui::RESULT_VERTICAL_BAR_WIDTH * 16.0))
            .overflow_hidden()
            .border_l(ui::HAIRLINE)
            .bg(theme.colors.sidebar)
            .debug_selector(|| "editor-result-right-edge-bar".to_string())
            .v_flex()
            .items_center()
            .gap_1()
            .pt_2()
            .children(tabs.into_iter().map(|(tab, icon, label)| {
                let queue = queue.clone();
                Button::new(format!("editor-result-edge-{:?}", tab))
                    .ghost()
                    .small()
                    .debug_selector(|| format!("editor-result-edge-{:?}", tab))
                    .toggled(active == tab)
                    .icon(icon)
                    .tooltip(label)
                    .on_click(move |_, _window, app| {
                        queue.request(ResultRequest::SetRightTab(tab), app);
                    })
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 【1.2】顶宽筛选条：点击筛选组展开按钮后，工具栏下方全宽输入框
    /// （suffix ▶ 运行）+ ✕ 清除 + 收起按钮 + 生成的 wrapped SQL 预览（词非空时）。
    fn render_filter_expand_bar(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let run_queue = self.queue.clone();
        let clear_queue = self.queue.clone();
        let collapse_queue = self.queue.clone();
        let collapse_queue2 = self.queue.clone();
        // wrapped SQL 预览：词非空且有原 SQL 时拼 `select * from (原SQL) where 词`
        let preview = (!self.filter_text.trim().is_empty() && self.result_sql.is_some()).then(|| {
            let sql = self.result_sql.clone().unwrap_or_default();
            let word = self.filter_text.trim();
            let wrapped = format!("select * from ({}) as _q where {}", sql, word);
            let preview_text: SharedString = if wrapped.len() > 80 {
                format!("{}…", &wrapped[..80]).into()
            } else {
                wrapped.into()
            };
            div()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(preview_text)
        });
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .flex_none()
            .min_h_0()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .bg(theme.colors.background)
            .px_2()
            .py_1()
            .text_xs()
            .debug_selector(|| "editor-result-filter-expand-bar".to_string())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        // 【2026-10-28 微调（纠正）】输入框内两个内嵌组件：
                        // **最左侧 = 展开**（prefix：Maximize2，点击收起筛选条，标识本框为
                        // 展开态）；**最右侧 = 运行**（suffix：▶，wrapped 重查）；
                        // 输入框无边框（bordered(false)，与工具栏融为搜索条风格）。
                        Input::new(&self.filter_input)
                            .bordered(false)
                            .prefix(
                                Button::new("editor-result-filter-expand-toggle")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Maximize2)
                                    .debug_selector(|| "editor-result-filter-expand-toggle".to_string())
                                    .tooltip("收起筛选条（回到工具栏输入框）")
                                    .on_click(move |_, _window, app| {
                                        collapse_queue.request(ResultRequest::ToggleFilterExpand, app);
                                    }),
                            )
                            .suffix(
                                Button::new("editor-result-filter-expand-run")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Play)
                                    .debug_selector(|| "editor-result-filter-expand-run".to_string())
                                    .tooltip("用 select * from (原SQL) where 词 重查（替换当前结果集）")
                                    .on_click(move |_, _window, app| {
                                        run_queue.request(ResultRequest::RunFilterAsQuery, app);
                                    }),
                            ),
                    ),
            )
            .when(!self.filter_text.trim().is_empty(), |row| {
                row.child(
                    Button::new("editor-result-filter-expand-clear")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-filter-expand-clear".to_string())
                        .label("✕ 清除")
                        .on_click(move |_, _window, app| {
                            clear_queue.request(ResultRequest::ClearFilter, app);
                        }),
                )
            })
            .children(preview)
            .child(
                Button::new("editor-result-filter-expand-collapse")
                    .ghost()
                    .small()
                    .icon(IconName::Minimize)
                    .debug_selector(|| "editor-result-filter-expand-collapse".to_string())
                    .label("收起")
                    .tooltip("回到工具栏输入框".to_string())
                    .on_click(move |_, _window, app| {
                        collapse_queue2.request(ResultRequest::ToggleFilterExpand, app);
                    }),
            )
            .into_any_element()
    }

    /// 【2026-10-28 微调】SQL 预览**弹出窗口**（等效浮层：半透明遮罩 + 居中卡片 +
    /// 只读 Textarea 展示原始 SQL）。不用 kit Dialog——`Dialog::new` 需 `&mut App`，
    /// 面板 render 只有 `&App`（host 手动调 `result_panel.read(cx).render(cx)`）；
    /// Textarea 实体已在事件路径预建（`TextareaState::new` 需要 window），render 期
    /// 只需 `Textarea::new(&entity)`。两路关闭：关闭按钮 / Esc。
    fn render_sql_dialog(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            // 遮罩：半透明黑（深浅模式通用；theme.colors 无 scrim 字段）。无点击关闭
            // （kit Div 无 on_click，交互走 ✕ 按钮 / Esc）
            .bg(gpui::hsla(0., 0., 0., 0.45))
            .debug_selector(|| "editor-result-sql-dialog".to_string())
            .on_key_down(move |event, _window, app| {
                if event.keystroke.key == "escape" {
                    close_queue.request(ResultRequest::CloseSqlDialog, app);
                }
            })
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(680.))
                    .max_h(px(480.))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-sql-dialog-card".to_string())
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .border_b(ui::HAIRLINE)
                            .border_color(theme.colors.border)
                            .child(div().text_sm().child("原始 SQL"))
                            .child(
                                Button::new("editor-result-sql-dialog-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| "editor-result-sql-dialog-close".to_string())
                                    .tooltip("关闭".to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue2.request(ResultRequest::CloseSqlDialog, app);
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .p_3()
                            .child(
                                self.sql_textarea
                                    .as_ref()
                                    .map(|ta| {
                                        Textarea::new(ta)
                                            .readonly(true)
                                            .h(px(360.))
                                            .into_any_element()
                                    })
                                    .unwrap_or_else(|| {
                                        div()
                                            .p_4()
                                            .text_color(theme.colors.muted_foreground)
                                            .child("（无 SQL）")
                                            .into_any_element()
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 【右栏】值详情页（数值查看器）：选中格的完整值 + 元数据（列名 / 类型 / 位置）。
    ///
    /// 与状态行「当前格值」的区别：状态行截断（防撑爆），这里**原样看全**
    /// （长 JSON / 长文本可滚动读完）。选整行 / 没选中 = 空态。
    /// 转置视图下 `col` 语义是转置格（0=行号 / 1=字段 / 2=值），列名段让位。
    fn render_value_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let Some((row, col)) = self.selection_pos else {
            return self.sidebar_empty("未选中单元格", cx);
        };
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let value = delegate.cell_text(row, col, cx);
        // 列名 / 类型：普通视图 = columns[col - 1]（col 含行号槽）；行号槽 / 转置
        // 给不出语义列名，标题只用位置
        let (name, type_name) = if col == 0 || self.transposed {
            (None, None)
        } else {
            let data_col = col - 1;
            (
                delegate.columns().get(data_col).cloned(),
                delegate
                    .column_types()
                    .get(data_col)
                    .and_then(|t| t.as_ref())
                    .map(|t| t.name.clone()),
            )
        };
        let title = match (&name, &type_name) {
            (Some(name), Some(type_name)) => format!("{name} · {type_name}"),
            (Some(name), None) => name.clone(),
            _ => "值".to_string(),
        };
        // 【#28-B4】位置标注：本地新增 / 复制行标「本地」，被删底层行标「已删除」；
        // 行号仍是视图行号（D6：不另造本地序号体系，身份靠标注与网格底色 / 删除线区分）。
        // 【2026-10-28 微调】第一行 = 值位置信息：行 + 列号（col 含行号槽，col==0 是行号
        // 槽本身，无列号可标；普通列标「第 N 列」）。
        let row_marker = {
            let base = if col == 0 {
                format!("第 {} 行", row + 1)
            } else {
                format!("第 {} 行 · 第 {} 列", row + 1, col)
            };
            if delegate.row_is_local(row) {
                format!("{base} · 本地")
            } else if delegate.is_deleted_row(row) {
                format!("{base} · 已删除")
            } else {
                base
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .debug_selector(|| "editor-result-value-tab".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_2()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(title))
                    .child(SharedString::from(row_marker)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .whitespace_normal()
                    .child(SharedString::from(value)),
            )
            .into_any_element()
    }

    /// 【右栏】计算页（DBeaver「计算」）：选中列的 count / NULL / min / max / avg / sum。
    ///
    /// 作用列 = 网格选中格优先（`selection_pos`），列浏览器清单选中列（`selected_col`）兜底；
    /// 转置视图不给语义列。数据 = 列级访问器 `column_stats_for`（只取目标列取值，
    /// 不物化整行矩阵；纯函数口径与列头菜单弹层一致）。
    /// 数值统计只在列里有可解析数值样本时才填（文本 / BLOB 只计 count / NULL）。
    fn render_stats_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let Some(data_col) = self.inspect_column(delegate) else {
            return self.sidebar_empty("选中一个数据列查看统计", cx);
        };
        // 【P2-2】列级访问器：只取目标列取值，不再 `visible_rows()` 物化整行矩阵
        let stats = delegate.column_stats_for(data_col);
        let name = delegate
            .columns()
            .get(data_col)
            .cloned()
            .unwrap_or_else(|| "列".to_string());
        let items: Vec<(String, String)> = vec![
            ("非空".into(), stats.count.to_string()),
            ("NULL".into(), stats.nulls.to_string()),
            ("最小值".into(), fmt_stat(stats.min)),
            ("最大值".into(), fmt_stat(stats.max)),
            ("均值".into(), fmt_stat(stats.avg)),
            ("合计".into(), fmt_stat(stats.sum)),
        ];
        div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-result-stats-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("「{name}」统计"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(items.into_iter().map(|(k, v)| kv_row(&k, &v, cx))),
            )
            .into_any_element()
    }

    /// 【右栏】分组页（DBeaver「分组」）：按选中列本地 groupby 计数。
    ///
    /// 作用列口径同计算页：网格选中格优先，清单选中列兜底。
    /// 纯函数 `group_by_count`（保留首现顺序，NULL 独立成组）；大表口径是
    /// 下发 GROUP BY 重查（见 §8.8），本地页只服务中小结果集的即时分组。
    fn render_group_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let Some(data_col) = self.inspect_column(delegate) else {
            return self.sidebar_empty("选中一个数据列查看分组", cx);
        };
        // 【P2-2】列级访问器：只取目标列取值，不再 `visible_rows()` 物化整行矩阵
        let groups = delegate.group_by_for(data_col);
        let name = delegate
            .columns()
            .get(data_col)
            .cloned()
            .unwrap_or_else(|| "列".to_string());
        let rows_el: Vec<AnyElement> = groups
            .iter()
            .map(|(value, count)| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .h_6()
                    .text_xs()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(SharedString::from(value.clone())),
                    )
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .into_any_element()
            })
            .collect();
        div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-result-group-tab".to_string())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("按「{name}」分组 · {} 组", groups.len()))),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(rows_el),
            )
            .into_any_element()
    }

    /// 【引用】外键引用查看页签（对齐 DBeaver 引用页签；backlog 空态）
    fn render_reference_tab(&self, cx: &App) -> AnyElement {
        self.sidebar_empty("选中一个列查看外键引用", cx)
    }

    /// 【1.2】「执行SQL」页签：筛选框运行后生成的 wrapped SQL 原文（只读 + 复制）。
    fn render_exec_sql_tab(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let Some(sql) = self.exec_sql.clone() else {
            return self.sidebar_empty("在工具栏筛选框输入词并点 ▶ 运行，生成 SQL 会显示在这里", cx);
        };
        let copy_queue = self.queue.clone();
        div()
            .v_flex()
            .w_full()
            .h_full()
            .gap_2()
            .p_2()
            .debug_selector(|| "editor-result-exec-sql-tab".to_string())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(SharedString::from("生成的 SQL")),
                    )
                    .child(
                        Button::new("editor-result-exec-sql-copy")
                            .ghost()
                            .small()
                            .icon(IconName::Copy)
                            .label("复制")
                            .tooltip("复制生成的 SQL".to_string())
                            .on_click(move |_, _window, app| {
                                copy_queue.request(ResultRequest::CopyExecSql, app);
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .bg(theme.colors.background)
                    .p_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(SharedString::from(sql)),
            )
            .into_any_element()
    }

    /// 【侧栏】右栏各页共用的空态（居中灰字；页内容区 `flex_1`）
    fn sidebar_empty(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_1()
            .px_2()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(SharedString::from(text))
            .into_any_element()
    }

    /// 【筛选】列头菜单「按值筛选此列 / 清除」落点：值非空 = 设置列级筛选，
    /// 空串 = 清除（grid 端约定，见 `set_column_filter_hook` 注释）。
    /// 只改视图行序，不重查源库；顺带刷新工具栏「已筛选 N/M 行」统计。
    pub fn apply_column_filter(&mut self, column: usize, value: &str, cx: &mut Context<Self>) {
        self.grid.update(cx, |grid, _| {
            grid.delegate_mut().set_column_filter(if value.is_empty() {
                None
            } else {
                Some((column, value.to_string()))
            });
        });
        self.refresh_filter_hint(cx);
        cx.notify();
    }

    /// 右键「按值筛选」核心：定稿筛选词并立即应用（不依赖窗口——headless 下窗口
    /// 不可用时真值照样生效；输入框回填由编辑器在窗口可用时转手调 [`Self::sync_filter_input`]）
    pub fn set_filter_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.filter_text = value.to_string();
        self.filter_version = self.filter_version.wrapping_add(1);
        self.apply_filter(cx);
    }

    /// 【B15】定稿筛选词但**不**触发「下发源库」重查入队——`drain_result_actions`
    /// 分派 [`ResultRequest::PushdownFilter`] 时用它：这条请求本身就是下发，再经
    /// `apply_filter` 入队一次会跟 drain 的 while 循环互相喂，队列无限增长撑爆内存。
    pub fn set_filter_value_dispatch(&mut self, value: &str, cx: &mut Context<Self>) {
        self.filter_text = value.to_string();
        self.filter_version = self.filter_version.wrapping_add(1);
        self.apply_filter_inner(cx, false);
    }

    /// 筛选输入框回填（所见即所得；窗口不可用跳过——真值已在 [`Self::set_filter_value`] 定稿）
    pub fn sync_filter_input(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.filter_input.clone();
        let text = value.to_string();
        input.update(cx, |state, cx| state.set_value(text, window, cx));
    }

    /// 【B15】把「已筛选 N / M 行」写进结果工具栏（统计跟随筛选——原型 §5.5；
    /// 全局筛选词与列级筛选任一生效都算“已筛选”）
    fn refresh_filter_hint(&mut self, cx: &mut Context<Self>) {
        let (visible, total, filtering) = {
            let delegate = self.grid.read(cx).delegate();
            (
                delegate.visible_row_count(),
                delegate.data_row_count(),
                !delegate.filter().trim().is_empty() || delegate.column_filter().is_some(),
            )
        };
        if let Some(toolbar) = self.toolbar.as_mut() {
            toolbar.filtered = filtering.then_some((visible, total));
        }
    }

    // ── 只读访问器（测试断言；编辑器接线也会用到） ────────────────────────────

    /// 当前是否有任何结果区内容（编辑器据此决定结果 dock 是否该开）
    pub fn has_content(&self) -> bool {
        self.toolbar.is_some()
            || self.script_card.is_some()
            || self.error_card.is_some()
    }

    /// 对比模式是否开启
    pub fn compare_mode(&self) -> bool {
        self.compare_mode
    }

    /// 当前选中结果集下标
    pub fn active(&self) -> usize {
        self.active
    }

    /// 工具栏投影（`None` = 还没有结论）
    pub fn toolbar(&self) -> Option<&ResultToolbar> {
        self.toolbar.as_ref()
    }

    /// 状态行投影（只有网格时才给）
    pub fn status(&self) -> Option<&ResultStatus> {
        self.status.as_ref()
    }

    /// 当前呈现方式（网格 / 文本）——供测试断言左竖条按钮点击链路
    pub fn presentation(&self) -> Presentation {
        self.presentation
    }

    /// 当前选中结果集的 SQL（「刷新」重跑它；`None` = 无可重跑）
    pub fn sql(&self) -> Option<&str> {
        self.result_sql.as_deref()
    }

    /// 【1.2】筛选框运行生成的 wrapped SQL（右栏「执行SQL」页签；`None` = 未运行过）
    pub fn exec_sql(&self) -> Option<&str> {
        self.exec_sql.as_deref()
    }

    /// 有没有可复制的东西（没网格不摆复制 / 转置 / 导出）
    pub fn can_copy(&self) -> bool {
        self.result_can_copy
    }

    /// 顶部提示行（切通道后「旧结果来自 X」）
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// 行选中 → 状态行「已选第 N 行」增量更新（表格 `TableEvent::SelectRow` 事件路径调用；
    /// sync 会全量覆写 status，这里只补事件之间的增量）
    pub fn set_selected_row(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        tracing::info!(row = ?row, "【探针】set_selected_row：结果网格行选中变化");
        if let Some(status) = self.status.as_mut() {
            status.selected_row = row;
        }
        cx.notify();
    }

    /// 格选中 → 状态行「当前格值」段增量更新（`TableEvent::SelectCell` 事件路径调用；
    /// 值由编辑器在事件里读网格 delegate 算好传入，面板不碰网格）
    pub fn set_selected_cell(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        if let Some(status) = self.status.as_mut() {
            status.selected_cell = value;
        }
        cx.notify();
    }

    /// 顶部提示行增量写入（编辑被拦等一次性提示；sync 会全量覆写 notice）
    pub fn set_notice(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        self.notice = text;
        cx.notify();
    }

    /// 结果集标签（测试断言标签条内容）
    pub fn tabs(&self) -> &[ResultSetTab] {
        &self.tabs
    }

    /// 错误卡片（`None` = 这次成功）
    pub fn error_card(&self) -> Option<&ErrorCard> {
        self.error_card.as_ref()
    }

    /// 脚本输出卡片
    pub fn script_card(&self) -> Option<&ScriptCard> {
        self.script_card.as_ref()
    }

    /// 主网格实体（编辑器 / 测试读选中行、数据行数等）
    pub fn grid(&self) -> &Entity<TableState<ResultGridDelegate>> {
        &self.grid
    }

    /// 对比栏网格实体（`None` = 没建）
    pub fn compare_grid(&self) -> Option<&Entity<TableState<ResultGridDelegate>>> {
        self.compare_grid.as_ref()
    }

    /// 筛选词（测试断言）
    pub fn filter_text(&self) -> &str {
        &self.filter_text
    }

    /// 下发源库开关（测试断言）
    pub fn pushdown(&self) -> bool {
        self.pushdown
    }

    /// 列浏览器清单当前选中列（数据列序；`None` = 未选中，渲染兜底第一可见列）
    pub fn selected_col(&self) -> Option<usize> {
        self.selected_col
    }
}

/// 【结果集 #296】按持久化列名序重排一份新结果的列与行（纯函数，测试直接覆盖）：
/// 列名匹配（保留持久化序），未匹配列按数据序附加末尾——用户拖过的列还在就保持，
/// 新列不猜。空 `order` = 默认序，原样直通。
fn reorder_by(
    order: &[String],
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
) -> (Vec<String>, Vec<Vec<String>>) {
    if order.is_empty() {
        return (columns, rows);
    }
    // 先按持久化序收集匹配列（重复列名只取第一个出现；去重防双写）
    let mut ordered: Vec<String> = Vec::new();
    for column in order {
        if columns.contains(column) && !ordered.contains(column) {
            ordered.push(column.clone());
        }
    }
    // 未匹配列按数据序附加末尾（新列不猜位置）
    let leftover: Vec<String> = columns
        .iter()
        .filter(|c| !order.contains(*c))
        .cloned()
        .collect();
    ordered.extend(leftover);
    // 行按同序重排（行宽与列数不符时原样保留——不猜形状）
    let row_len = columns.len();
    let mut perm: Vec<usize> = Vec::with_capacity(row_len);
    for column in &ordered {
        if let Some(at) = columns.iter().position(|c| c == column) {
            perm.push(at);
        }
    }
    let rows = rows
        .into_iter()
        .map(|row| {
            if row.len() != row_len {
                return row;
            }
            perm.iter().map(|at| row[*at].clone()).collect()
        })
        .collect();
    (ordered, rows)
}

/// 【右栏】页签下标 → 枚举（TabBar 回调用；与 `RightTab` 定义顺序一致——
/// Browser=0 / Value=1 / Stats=2 / Group=3 / Reference=4 / ExecSql=5，
/// 对齐 DBeaver 右侧页签顺序）
/// 【2026-10-28 微调】生产横 tab 栏只显激活页签后，索引映射仅供测试用
#[cfg(test)]
fn right_tab_from_index(index: usize) -> RightTab {
    match index {
        1 => RightTab::Value,
        2 => RightTab::Stats,
        3 => RightTab::Group,
        4 => RightTab::Reference,
        5 => RightTab::ExecSql,
        _ => RightTab::Browser,
    }
}

/// 【右栏】键值行（计算页 / 列浏览器明细共用）：左标签右值，值超长截断防挤
fn kv_row(label: &str, value: &str, cx: &App) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .px_2()
        .h_5()
        .text_xs()
        .child(
            div()
                .w(rems(ui::RESULT_SIDEBAR_KV_LABEL_WIDTH))
                .flex_none()
                .text_color(muted)
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(SharedString::from(value)),
        )
        .into_any_element()
}

/// 【右栏】统计值格式化：整数样去尾零（1.0 → 1），否则保留两位；无数值样本 = —
fn fmt_stat(value: Option<f64>) -> String {
    value
        .map(|v| {
            if v.fract() == 0.0 {
                format!("{}", v as i64)
            } else {
                format!("{:.2}", v)
            }
        })
        .unwrap_or_else(|| "—".to_string())
}

/// 【侧栏丝滑】展开 / 收起宽度动画的 `(from, to)` 计算（纯函数，render 调用）：
/// - `epoch == 0`（首帧）：`from == to`，无动画直接落位（面板初始展开，不能开场就来一遍滑入）
/// - `epoch > 0` 且展开：`0 → 全宽`（内容随宽度露出）
/// - `epoch > 0` 且收起：`全宽 → 0`（宽度平滑收缩）
/// 每次开合 `toggle_*_sidebar` 递增 epoch，动画 id 拼 epoch → `with_animation` 换 id 重播。
fn sidebar_width_anim(epoch: u64, show: bool, full: Pixels) -> (Pixels, Pixels) {
    let to = if show { full } else { px(0.) };
    let from = if epoch == 0 {
        to
    } else if show {
        px(0.)
    } else {
        full
    };
    (from, to)
}

/// 【2026-10-28 微调】右侧开关三态转移（纯函数，`toggle_value_sidebar` 调用）：
/// 收起 `(false,false)` → 窄展开 `(true,false)` → 宽展开 `(true,true)` → 收起。
/// 返回值 = `(edge_bar_visible, show_value_sidebar)`；不变量：宽展开 ⟹ 竖条可见。
fn next_right_edge_state(edge_bar_visible: bool, show_value_sidebar: bool) -> (bool, bool) {
    match (edge_bar_visible, show_value_sidebar) {
        (false, _) => (true, false),    // 收起 → 窄展开（只显右缘竖条图标条）
        (true, false) => (true, true),  // 窄展开 → 宽展开（右栏内容出现）
        (true, true) => (false, false), // 宽展开 → 收起（竖条与右栏一起隐去）
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入（`use gpui_kit::*` 会把 test 宏带进作用域导致 recursion limit）
    use crate::column_browser::filter_browser_columns;
    use super::next_right_edge_state;
    use super::reorder_by;
    use super::right_tab_from_index;
    use super::sidebar_width_anim;
    use crate::ports::RightTab;
    use gpui_kit::px;
    use gpui_kit::Pixels;

    /// 三份等长样本：列名 / 类型名 / 注释（注释可空）
    fn sample() -> (Vec<String>, Vec<String>, Vec<Option<String>>) {
        let names = vec![
            "id".to_string(),
            "user_name".to_string(),
            "created_at".to_string(),
        ];
        let types = vec![
            "INTEGER".to_string(),
            "VARCHAR".to_string(),
            "TIMESTAMP".to_string(),
        ];
        let comments = vec![
            Some("主键".to_string()),
            Some("用户登录名".to_string()),
            None,
        ];
        (names, types, comments)
    }

    /// 侧栏宽度动画参数：首帧免动画（from==to）/ 展开 0→全宽 / 收起 全宽→0
    #[test]
    fn sidebar_width_anim_first_frame_and_directions() {
        let full = px(36.);
        // 首帧（epoch=0）：无论开合都直接落位，不开场动画
        let (from, to) = sidebar_width_anim(0, true, full);
        assert_eq!(from, to);
        assert_eq!(to, full);
        let (from, to) = sidebar_width_anim(0, false, full);
        assert_eq!(from, to);
        assert_eq!(to, px(0.));
        // 展开（epoch>0）：0 → 全宽
        let (from, to) = sidebar_width_anim(1, true, full);
        assert_eq!(from, px(0.));
        assert_eq!(to, full);
        // 收起（epoch>0）：全宽 → 0
        let (from, to) = sidebar_width_anim(1, false, full);
        assert_eq!(from, full);
        assert_eq!(to, px(0.));
        // 类型约束（Pixels 参与计算，编译期校验）
        let _: (Pixels, Pixels) = sidebar_width_anim(2, true, full);
    }

    /// 右侧开关三态循环：收起 → 窄展开 → 宽展开 → 收起；每次点击恰好在三态间轮转
    #[test]
    fn right_edge_state_cycles_through_three_states() {
        // 从宽展开（初始默认）出发，点击三次回原态
        assert_eq!(next_right_edge_state(true, true), (false, false));
        assert_eq!(next_right_edge_state(false, false), (true, false));
        assert_eq!(next_right_edge_state(true, false), (true, true));
        // 宽展开恒有竖条可见（不变量）
        let (edge, show) = next_right_edge_state(true, false);
        assert!(show);
        assert!(edge);
        // 收起态点击直接进窄展开，不跨到宽展开（比左侧多一个展开态：窄 = 只显图标条）
        let (edge, show) = next_right_edge_state(false, false);
        assert!(edge);
        assert!(!show);
    }

    /// 空词 / 纯空白 = 全量原序（隐藏列由渲染层置灰，过滤本身不剔列）
    #[test]
    fn browser_filter_empty_returns_all_in_order() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, ""), vec![0, 1, 2]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "   "), vec![0, 1, 2]);
    }

    /// 列名匹配忽略大小写；子串包含即命中
    #[test]
    fn browser_filter_matches_name_case_insensitive() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, "USER"), vec![1]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "name"), vec![1]);
    }

    /// 类型名同样参与匹配（DBeaver 元数据面板只匹配列名；本轮拍板加宽到类型名 / 注释）
    #[test]
    fn browser_filter_matches_type_name() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, "varchar"), vec![1]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "timestamp"), vec![2]);
    }

    /// 注释参与匹配；注释为空的列不参与注释分支
    #[test]
    fn browser_filter_matches_comment() {
        let (n, t, c) = sample();
        assert_eq!(filter_browser_columns(&n, &t, &c, "登录名"), vec![1]);
        assert_eq!(filter_browser_columns(&n, &t, &c, "主键"), vec![0]);
    }

    /// 列名 / 类型 / 注释都不命中 = 空清单（渲染层摆「无匹配列」空态）
    #[test]
    fn browser_filter_no_match_returns_empty() {
        let (n, t, c) = sample();
        assert!(filter_browser_columns(&n, &t, &c, "zzz").is_empty());
    }

    /// 页签下标 → 枚举：0=列浏览器（默认）/ 1=值详情 / 2=计算 / 3=分组，越界回落默认页
    #[test]
    fn right_tab_index_maps_to_four_tabs() {
        assert_eq!(right_tab_from_index(0), RightTab::Browser);
        assert_eq!(right_tab_from_index(1), RightTab::Value);
        assert_eq!(right_tab_from_index(2), RightTab::Stats);
        assert_eq!(right_tab_from_index(3), RightTab::Group);
        assert_eq!(right_tab_from_index(99), RightTab::Browser);
    }

    /// 【结果集 #296】空列序 = 默认序原样直通（列 / 行一字不差）
    #[test]
    fn empty_order_passes_columns_through_unchanged() {
        let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let rows = vec![
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            vec!["4".to_string(), "5".to_string(), "6".to_string()],
        ];
        let (out_columns, out_rows) = reorder_by(&[], columns.clone(), rows.clone());
        assert_eq!(out_columns, columns);
        assert_eq!(out_rows, rows);
    }

    /// 【结果集 #296】按持久化列名序重排：列序与行内容同步换位
    #[test]
    fn reorder_moves_columns_and_rows_together() {
        let order = vec!["b".to_string(), "a".to_string()];
        let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let rows = vec![
            vec!["1".to_string(), "2".to_string(), "3".to_string()],
            vec!["4".to_string(), "5".to_string(), "6".to_string()],
        ];
        let (out_columns, out_rows) = reorder_by(&order, columns, rows);
        assert_eq!(
            out_columns,
            vec!["b".to_string(), "a".to_string(), "c".to_string()],
            "持久化序在前，未匹配列（c）按数据序附加末尾"
        );
        assert_eq!(
            out_rows,
            vec![
                vec!["2".to_string(), "1".to_string(), "3".to_string()],
                vec!["5".to_string(), "4".to_string(), "6".to_string()],
            ],
            "行按列同序换位"
        );
    }

    /// 【结果集 #296】持久化序里的列已不在新结果里 → 只匹配存在的；全不匹配 = 数据序
    #[test]
    fn reorder_keeps_only_matching_columns() {
        let order = vec!["gone".to_string(), "x".to_string()];
        let columns = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec!["1".to_string(), "2".to_string()]];
        let (out_columns, out_rows) = reorder_by(&order, columns, rows);
        assert_eq!(out_columns, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(out_rows, vec![vec!["1".to_string(), "2".to_string()]]);
    }
}
