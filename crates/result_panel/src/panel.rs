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

use gpui_kit::*;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::table::TableState;
use gpui_kit::prelude::FluentBuilder as _;

use crate::error_card::{self, ErrorCard};
use crate::export;
use crate::grid::{self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{ResultActionQueue, ResultRequest};
use crate::script_card::{self, ScriptCard};
use crate::sets::{self as result_sets, ResultSetTab};
use crate::ui;
use workbench_shell::model::ColumnAnnotation;

/// 一次投影里交到网格的数据（列 / 行 / 类型 / 来源表 / 字段注释；`set_data` 之后
/// 的类型与注释要紧接着设——`set_data` 会清空它们，见 [`crate::grid`] 实现）
#[derive(Debug, Clone)]
pub struct GridPayload {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_types: Vec<Option<crate::store::ColumnType>>,
    pub table: Option<String>,
    pub annotations: Vec<Option<ColumnAnnotation>>,
}

/// 分析菜单项（宿主算好传入：只含渲染要的文案与动作载荷）
#[derive(Debug, Clone)]
pub struct AnalysisItem {
    pub label: String,
    pub sql: String,
}

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
    /// 【并排对比】对比栏的投影（编辑器 `sync_compare_grid` 时经
    /// [`Self::set_compare_projection`] 写入；render 期只读）
    compare_active: usize,
    compare_toolbar: Option<ResultToolbar>,
    compare_status: Option<ResultStatus>,
    result_sql: Option<String>,
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

    // 【B15】本地筛选框（视图层自洽：防抖 + 清除 + 统计回填都在面板内）
    filter_input: Entity<InputState>,
    filter_text: String,
    /// 防抖版本号（连打时任务被取代，只有最后一次到点）
    filter_version: u64,
    /// 在途防抖任务（持有以保活；换词即被取代）
    filter_debounce: RefCell<Option<Task<()>>>,
    /// 【B14】下发源库开关（开着时应用筛选会重查源库——重查经队列交回编辑器）
    pushdown: bool,
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

        // 占位网格：编辑器构造真网格（含钩子）后经 `set_grids` 替换；占位仅保证
        // 构造后到接线前这段窗口里实体非空（渲染不会走到——还没 sync 过）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行"),
            window,
            cx,
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
            compare_active: 0,
            compare_toolbar: None,
            compare_status: None,
            result_sql: None,
            result_is_analysis: false,
            result_can_copy: false,
            fetching_more: false,
            export_ctx: None,
            analysis_items: Vec::new(),
            pending_changes: 0,
            committing: false,
            filter_input,
            filter_text: String::new(),
            filter_version: 0,
            filter_debounce: RefCell::new(None),
            pushdown: false,
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
        self.fetching_more = input.loading_more;

        // 【B15】筛选统计的真值在网格那边（视图行序是它算的）——写入 toolbar 前先回填
        self.refresh_filter_hint(cx);

        self.grid.update(cx, |state, cx| {
            match input.grid {
                Some(payload) => {
                    state.delegate_mut().set_data(payload.columns, payload.rows);
                    // 【Q6】类型必须**在 set_data 之后**设：`set_data` 会清空类型
                    state.delegate_mut().set_column_types(payload.column_types);
                    // 【A9】来源表名同理（set_data 会清掉旧的）
                    state.delegate_mut().set_result_table(payload.table);
                    // 【P1】字段注释同理（set_data 会清掉旧的）
                    state.delegate_mut().set_column_annotations(payload.annotations);
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
                        .on_click(move |_, _window, _app| {
                            queue.push(ResultRequest::LocateError);
                        })
                        .into_any_element()
                });
                let queue = self.queue.clone();
                let copy = Button::new("editor-result-error-copy")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-error-copy".to_string())
                    .label("复制")
                    .on_click(move |_, _window, _app| {
                        queue.push(ResultRequest::CopyError);
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

        let toolbar = self.toolbar.clone().unwrap_or(ResultToolbar {
            elapsed_ms: None,
            ..Default::default()
        });
        let tabs = (self.tabs.len() >= 2).then(|| {
            let queue = self.queue.clone();
            result_sets::render(
                &self.tabs,
                self.active,
                move |index, _window, _app| {
                    let index = *index;
                    queue.push(ResultRequest::SelectSet(index));
                },
                cx,
                &self.tabs_scroll,
            )
            .into_any_element()
        });

        // 【B15】本地筛选框：只作用于视图层，不重查数据库；输入走 300ms 防抖
        // （`on_filter_input`），有词时给一个 ✕ 清除；「下发源库」开关开着时应用
        // 筛选会重查（入队，结果落新结果集）
        let filter = div()
            .h_flex()
            .items_center()
            .gap_1()
            .debug_selector(|| "editor-result-filter".to_string())
            .child(
                div()
                    .w(rems(ui::RESULT_FILTER_WIDTH))
                    .child(Input::new(&self.filter_input)),
            )
            .when(!self.filter_text.trim().is_empty(), |row| {
                let queue = self.queue.clone();
                row.child(
                    Button::new("editor-result-filter-clear")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-filter-clear".to_string())
                        .label("✕")
                        .on_click(move |_, _window, _app| {
                            queue.push(ResultRequest::ClearFilter);
                        }),
                )
            })
            .child({
                let queue = self.queue.clone();
                Switch::new("editor-result-pushdown")
                    .checked(self.pushdown)
                    .label("下发源库")
                    .on_click(move |checked, _window, _app| {
                        let open = *checked;
                        queue.push(ResultRequest::SetPushdown { open });
                    })
            })
            .into_any_element();

        // 【P2-4】转置视图（单行转置，DBeaver 式）：有网格才摆。开关激活时左
        // 边跟上一行 / 下一行导航（看宽表不靠拖横向滚动条）
        let transpose = self.result_can_copy.then(|| {
            let queue = self.queue.clone();
            let toggle = Button::new("editor-result-transpose")
                .ghost()
                .small()
                .toggled(self.transposed)
                .icon(IconName::Table2)
                .tooltip("转置视图")
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::ToggleTranspose);
                });
            if !self.transposed {
                return toggle.into_any_element();
            }
            let queue = self.queue.clone();
            let prev = Button::new("editor-result-transpose-prev")
                .ghost()
                .small()
                .icon(IconName::ChevronUp)
                .tooltip("上一行")
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::TransposePrev);
                });
            let queue = self.queue.clone();
            let next = Button::new("editor-result-transpose-next")
                .ghost()
                .small()
                .icon(IconName::ChevronDown)
                .tooltip("下一行")
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::TransposeNext);
                });
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .debug_selector(|| "editor-result-transpose-group".to_string())
                .child(prev)
                .child(toggle)
                .child(next)
                .into_any_element()
        });

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
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::ToggleCompareMode);
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
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::CommitPending);
                });
            let queue = self.queue.clone();
            let discard = Button::new("editor-result-discard")
                .ghost()
                .small()
                .debug_selector(|| "editor-result-discard".to_string())
                .label("撤销变更")
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::RollbackPending);
                });
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .debug_selector(|| "editor-result-commit-group".to_string())
                .child(commit_btn)
                .child(discard)
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
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::CopyTsv);
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
                .on_click(move |_, _window, _app| {
                    queue.push(ResultRequest::Refresh);
                })
                .into_any_element()
        });
        // 【B5b】取下一段：只在这份结果真还有下一段时摆（状态行⑦里）
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
                    .label("取下一段")
                    .on_click(move |_, _window, _app| {
                        queue.push(ResultRequest::FetchMore);
                    })
                    .into_any_element()
            });

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
                            move |_, _window, _app| {
                                queue.push(ResultRequest::Export {
                                    format,
                                    all: matches!(scope, export::ExportScope::All),
                                });
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
                            move |_, _window, _app| {
                                let sql = sql.clone();
                                queue.push(ResultRequest::Analysis { sql });
                            },
                        ));
                    }
                    let queue = queue.clone();
                    menu.separator().item(
                        PopupMenuItem::new(crate::analysis::CUSTOM_LABEL).on_click(
                            move |_, _window, _app| {
                                queue.push(ResultRequest::AnalysisCustom);
                            },
                        ),
                    )
                })
                .into_any_element()
        });

        let result_pane = result_grid::render(
            &self.grid,
            result_grid::ResultPane {
                toolbar,
                status: self.status.clone(),
                controls: result_grid::ResultControls {
                    filter: Some(filter),
                    transpose,
                    compare,
                    commit,
                    analysis,
                    copy,
                    export,
                    refresh,
                    more,
                },
                card,
                tabs,
                notice: self.notice.clone(),
            },
            cx,
        )
        .into_any_element();

        // 【并排对比】对比模式双栏：左栏 = 主选中（上面组装好的主 pane），
        // 右栏 = 对比选中（自己的标签条 + 只读网格）。右栏数据由编辑器
        // `sync_compare_grid` 提前拷进 `compare_grid`（render 只读网格状态）。
        // 右栏不接 filter/transpose/commit/analysis/export/refresh/more——那些
        // 动作的语义都归主栏，并排对照不需要第二套动作。
        if !self.compare_mode {
            return result_pane;
        }
        let compare_tabs = (self.tabs.len() >= 2).then(|| {
            let queue = self.queue.clone();
            let compare_active = self.compare_active;
            result_sets::render(
                &self.tabs,
                compare_active,
                move |index, _window, _app| {
                    let index = *index;
                    queue.push(ResultRequest::SelectCompare(index));
                },
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
                },
                cx,
            )
            .into_any_element()
        });
        let Some(compare_pane) = compare_pane else {
            return result_pane;
        };
        // 双栏：中间一条竖分隔线（结构元素用 `ui::HAIRLINE`），两栏各占一半
        div()
            .h_flex()
            .items_stretch()
            .w_full()
            .h_full()
            .min_h_0()
            .debug_selector(|| "editor-result-compare-layout".to_string())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .border_r(ui::HAIRLINE)
                    .child(result_pane),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .debug_selector(|| "editor-result-compare-pane".to_string())
                    .child(compare_pane),
            )
            .into_any_element()
    }

    // ── 视图层动作（面板内自洽：只碰网格 delegate 与本实体状态） ──────────────

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
        let filter = self.filter_text.clone();
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(filter);
            state.refresh(cx);
        });
        self.refresh_filter_hint(cx);
        cx.notify();
        if self.pushdown && !self.filter_text.trim().is_empty() {
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
        let row = self.transpose_row;
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_transposed(open, row);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【P2-4】转置行导航：上一行 / 下一行（越界夹取；数据行数来自 delegate）
    pub fn transpose_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let max = self
            .grid
            .read(cx)
            .delegate()
            .data_row_count()
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

    /// 【B15】把「已筛选 N / M 行」写进结果工具栏（统计跟随筛选——原型 §5.5）
    fn refresh_filter_hint(&mut self, cx: &mut Context<Self>) {
        let (visible, total, filtering) = {
            let delegate = self.grid.read(cx).delegate();
            (
                delegate.visible_row_count(),
                delegate.data_row_count(),
                !delegate.filter().trim().is_empty(),
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

    /// 当前选中结果集的 SQL（「刷新」重跑它；`None` = 无可重跑）
    pub fn sql(&self) -> Option<&str> {
        self.result_sql.as_deref()
    }

    /// 有没有可复制的东西（没网格不摆复制 / 转置 / 导出）
    pub fn can_copy(&self) -> bool {
        self.result_can_copy
    }

    /// 顶部提示行（切通道后「旧结果来自 X」）
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
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
}
