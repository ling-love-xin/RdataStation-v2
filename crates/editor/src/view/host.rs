//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use std::cell::RefCell;

use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity, Position};
use gpui_kit::base::{StyledExt as _, resizable_panel, v_resizable};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::dock::{
    BasePanel, DockArea, Panel as ComponentPanel, PanelEvent as BasePanelEvent, PanelId, TabGroup,
};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::table::{TableEvent, TableState};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::channel::{self, ExecChannel};
use crate::commands::{
    CopyGridSelection, ExecuteAll, ExecuteSql, FormatDocument, SaveDocument, ToggleComment,
};
use crate::diagnostics;
use crate::edit;
use crate::execution::{self, ExecMenuKind, ExecTarget, ResultPlacement};
use crate::export::{self, ExportFormat, ExportScope};
use crate::format;
use crate::translate;
use crate::view::completion;
use crate::mode::{self, CellGranularity};
use crate::model::{DocumentId, EditorMode, ReadOnly};
use crate::persist;
use crate::service::Document;
use crate::shared::{EditorShared, InsightColumnRequest};
use crate::sources;
use crate::store::ResultEntry;
use crate::ui;
use crate::view::dialogs;
use crate::view::highlight;
use crate::view::results::error_card::{self, ErrorCard};
use crate::view::results::grid::{self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::view::results::sets::{self as result_sets, ResultSetTab};
use crate::view::widgets::status_bar::{self, StatusInputs};

/// 【B7】「抓全量后导出」的在途状态
///
/// `set` 是发起时的结果集下标：抓取期间用户切了结果集就作废（取段是 `Append` 落位，
/// 会接到**当前选中**那份上，接着写盘就会把两份结果拼在一起）。
struct PendingExport {
    format: ExportFormat,
    path: std::path::PathBuf,
    set: usize,
}

/// 一份文档的编辑面板
pub struct EditorHostPanel {
    shared: EditorShared,
    document: DocumentId,
    focus_handle: FocusHandle,
    /// 编辑内核状态（构造需要 `window`，故在 `new` 里一次建好）
    editor: Entity<EditorState>,
    /// 语句数缓存（A7）：内容变化时算一次，**不在渲染期扫描**（render 是纯读路径）
    statements: usize,
    /// 所在的 tab 组（`Panel::on_added_to` 交给面板；用于“激活自己”）
    ///
    /// 面板不持有 `DockArea`：那是容器，容器知道面板就够了。反向只握一个弱句柄，
    /// 容器消失时自动失效（不会把整棵 Dock 拖住不放）。
    pub(crate) group: Option<WeakEntity<TabGroup>>,
    /// 状态栏提示：动作失败的真实原因（未命名需另存为 / 有未保存改动 / 只读拒绝）
    ///
    /// 有它才不会有“按了没反应”的动作：失败必须留下可见痕迹。成功即清空。
    pub(crate) message: Option<String>,
    /// 是否已被 Dock 移除（移除即关闭文档，见 `Panel::on_removed`）
    pub(crate) closed: bool,
    /// 结果网格（A14）：数据由 `ResultStore` 拷入 delegate，网格不持有第二份真值
    grid: Entity<TableState<ResultGridDelegate>>,
    /// 本文档**已提交、尚未回填**的语句数（B2：批量 = 多条；`> 0` 就是“执行中”）
    pending: usize,
    /// 本文档当前这轮执行是什么时候开始的（B3：状态栏耗时累加；`pending == 0` 时清掉）
    running_since: Option<std::time::Instant>,
    /// 【B4】自动提交：开 = 每次执行自成一体（默认）；关 = 执行时若没有事务就先开一个，
    /// 由用户显式提交或回滚
    autocommit: bool,
    /// 【B4】本文档绑定的连接上有没有活动事务（界面上的 TX 区读它）
    tx_open: bool,
    /// 【B4】这个事务是什么时候开始的（**本地观测时刻**：事务可能是在别处开的，
    /// 界面的时长从“我们第一次看到它”算起）
    tx_since: Option<std::time::Instant>,
    /// 【B4】已发出、尚未回执的事务动作数（轮询泵据此多活一会儿）
    tx_pending: usize,
    /// 【B13】已请出、尚未回执的“重新挂载加速源”数（同上）
    refresh_pending: usize,
    /// 【B7 切片二】已请出、尚未回执的 DuckDB 导出（Parquet / XLSX）数（同上）
    export_pending: usize,
    /// 【B10】在途结果集的**自定义标题**队列（按提交顺序回填；`None` = 就是“结果 N”）
    ///
    /// 为什么要队列：执行是**一条语句一个结论**地回到界面的（批量更是好几条），
    /// “这一份是执行计划”这种信息必须**按位**对上，不能拿一个布尔量糊过去。
    pending_labels: std::collections::VecDeque<Option<String>>,
    /// 【B5】结果区要画的东西（工具栏 ⑥ + 状态行 ⑦ + 错误卡片）
    ///
    /// 从选中结果集投影一次就缓在这里：render 是纯读路径，不在渲染期算文案。
    /// `None` = 还没有结论（结果区不出现）。
    result_toolbar: Option<ResultToolbar>,
    /// 【B5】结果状态行（⑦）；只有网格时才给（失败 / 写语句那一行是噪音）
    result_status: Option<ResultStatus>,
    /// 结果集标签（从 `ResultStore` 投影；同样不在渲染期重算）
    result_tabs: Vec<ResultSetTab>,
    /// 当前选中的结果集下标（标签条的选中态）
    result_active: usize,
    /// 【B5】当前选中结果集的 SQL（工具栏「刷新」重跑它；`None` = 无可重跑的东西）
    result_sql: Option<String>,
    /// 【B15】当前选中结果是**本地分析**的产物（不摆“刷新”：重跑它没有意义）
    result_is_analysis: bool,
    /// 【B5】当前选中结果集有没有可复制的东西（没有网格就不摆复制按钮）
    result_can_copy: bool,
    /// 【B6】错误卡片的内容（失败时才有；`None` = 这次成功）
    result_error_card: Option<ErrorCard>,
    /// 【B6】当前选中结果集定位到的出错处（诊断范围 + 状态栏里的位置文案读它）
    error_site: Option<diagnostics::ErrorSite>,
    /// 【B6】上一次算过的位置是哪条结论（`(这次跑的 SQL, 错误文本)`）
    ///
    /// 位置要拿整篇文档做匹配，而 `sync_result_view` 会被反复调用（回填 / 切标签），
    /// 所以只在结论变了的时候重算。
    error_site_key: Option<(String, String)>,
    /// 面板所在窗口（构造时存下：出错回填发生在没有窗口的轮询里，聚焦要用）
    window: AnyWindowHandle,
    /// 【B5】结果区高度（rem；拖拽分栏后记在这里，下一次渲染用它当初始尺寸）
    ///
    /// 只活在这个面板里（尚未随会话持久化——原型同。）
    result_height: std::rc::Rc<std::cell::Cell<f32>>,
    /// 【B7】「抓全量后导出」的在途状态（`None` = 没有导出在跑）
    ///
    /// 抓全量是**多轮取段**：每段回来推进一次，最后一段（没拿满）才落盘。中途换结果集、
    /// 取段失败、用户中断都作废——**不落一个半截的文件**（宁可没有，也不要一份看起来
    /// 完整、实则少一半的导出）。
    pending_export: Option<PendingExport>,
    /// 【B5b】正在取下一段（滚动到底自动加载的防重入真值；同步给网格 delegate）
    fetching_more: bool,
    /// 【B15】本地筛选框（原型 §5.5 的 `⌕ 筛选`；只作用于视图层，不重查）
    filter_input: Entity<InputState>,
    /// 【B15】筛选词的真值（输入框变化 → 防抖 300ms → 应用到网格）
    filter_text: String,
    /// 【B15】防抖版本号（连打时只让最后一次生效）
    filter_version: u64,
    /// 【B15】防抖任务句柄（仅持有；下一次输入会取代它）
    filter_debounce: RefCell<Option<Task<()>>>,
    /// 【B15】下发源库开关（原型 §5.5 的 `▢ 下发源库`）：开着时应用筛选会重查源库
    pushdown: bool,
    /// 【B13】结果区顶部那一行（切通道后“旧结果来自 X”；`None` = 不提示）
    ///
    /// 与标签条上那份星徽标互为补充：徽标说“它来自哪档”，这一行说“为什么它现在是旧的”。
    result_notice: Option<String>,
    /// 【B14】结果网格里**最近一次选中**（`Ctrl+C` 复制谁）
    ///
    /// 组件没给“当前是选格还是选行”的访问器，而它的 `selected_cell()` / `selected_row()`
    /// 可以同时有值（选过整行再点一个格子）——所以在这里记**事件**，不猜状态。
    result_selection: Option<result_grid::GridSelection>,
    /// 结果轮询任务句柄（空闲即退出；句柄存活到下一次提交）
    exec_pump: RefCell<Option<Task<()>>>,
    /// 内容回写订阅：句柄即生命周期（释放即取消）
    _editor_sub: Option<Subscription>,
    /// 网格事件订阅（行选中 → 状态行的“已选第 N 行”要跟着变）
    _grid_sub: Option<Subscription>,
    /// 【B15】筛选框订阅（输入 → 防抖）
    _filter_sub: Option<Subscription>,
}

impl EditorHostPanel {
    /// 为某文档建面板
    ///
    /// 文档不存在时退化为**只读空面板**而不 panic：面板可能在文档被关闭后仍被保留一帧，
    /// 让整个工作台跟着崩掉不是可接受的降级。
    pub fn new(
        shared: EditorShared,
        document: DocumentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        let (content, editor_read_only, mode) = {
            let service = shared.service();
            match service.find(&document) {
                Some(doc) => (
                    doc.content().to_string(),
                    !doc.read_only().can_edit(),
                    doc.mode(),
                ),
                None => (String::new(), true, EditorMode::Text),
            }
        };

        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx);
            state.set_value(content, window, cx);
            state.set_readonly(editor_read_only, cx);
            // A4：SQL 语义着色（文本模式是纯记事本，不解析不上色；分析模式留 1c 逐单元处理）
            if highlight::is_enabled(mode) {
                highlight::install(&mut state);
            }
            // B9：SQL 补全（候选来自宿主端口；文本模式 / 大文件档位不装）
            if shared.completion_enabled(&document) {
                completion::install(&mut state, shared.clone(), document.clone());
            }
            state
        });

        let sub = cx.subscribe_in(
            &editor,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    // 内核 → 服务：内容只落到 `EditorService`，脏状态由它比较基线得出
                    let text = this.editor_text(cx);
                    this.statements = count_statements(&text);
                    this.shared
                        .update(|service| service.set_content(&this.document, text));
                    // 重绘以刷新标签脏点与状态栏
                    cx.notify();
                }
            },
        );

        let statements = count_statements(
            &shared
                .service()
                .find(&document)
                .map(|doc| doc.content().to_string())
                .unwrap_or_default(),
        );

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

        // 结果网格：构造时一次建成，以后结果变化只 `refresh`（不重建，避免丢掉列宽 / 滚动位置）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行——按 Ctrl+Enter 执行当前语句"),
            window,
            cx,
        );
        // 行选中 → 状态行那一段（⑦ 的“已选第 N 行”）要重画（表格自己不通知宿主）
        let grid_sub = cx.subscribe(&grid, |panel, grid, event: &TableEvent, cx| {
            let selected_row = grid.read(cx).selected_row().map(|row| row + 1);
            // 【B14】`Ctrl+C` 复制谁：只认**最近一次选择事件**（组件没公开“当前是选格还是选行”，
            // 而它两个字段可以同时有值）。键位失效之后再选也就自动作废了，不用额外清理。
            panel.result_selection = match event {
                TableEvent::SelectCell(row, col) => Some(result_grid::GridSelection::Cell {
                    row: *row,
                    col: *col,
                }),
                TableEvent::SelectRow(row) => Some(result_grid::GridSelection::Row { row: *row }),
                TableEvent::ClearSelection => None,
                _ => panel.result_selection,
            };
            if let Some(status) = panel.result_status.take() {
                panel.result_status = Some(ResultStatus {
                    selected_row,
                    ..status
                });
            }
            cx.notify();
        });

        // 【B5b】滚动到底自动加载：组件库在可见范围接近末尾时调 delegate 的 `load_more`，
        // 而“该不该再取一段”只有面板知道（选中哪份结果、忙不忙）——走钩子交回给面板。
        // 面板自己的 `fetch_more` 仍是**显式按钮**与自动加载共用的同一条路。
        // 【B14】右键「按值筛选」也走同一套：值写进筛选框并立刻应用（滤镜 UI 在面板这边）。
        {
            let load_weak = cx.entity().downgrade();
            let filter_weak = cx.entity().downgrade();
            let sort_weak = cx.entity().downgrade();
            let insight_weak = cx.entity().downgrade();
            // 【M8】宿主没接「洞察此列」端口就不装钩子——菜单据此不摆那一项（能力没有就不给入口）
            let insight_port = shared.insight_column_port();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_load_more_hook(std::rc::Rc::new(
                    move |app: &mut App| {
                        _ = load_weak.update(app, |panel, cx| panel.fetch_more(cx));
                    },
                ));
                state.delegate_mut().set_filter_value_hook(std::rc::Rc::new(
                    move |value: &str, app: &mut App| {
                        let value = value.to_string();
                        _ = filter_weak
                            .update(app, |panel, cx| panel.apply_filter_value(&value, cx));
                    },
                ));
                let sort_weak = sort_weak;
                state.delegate_mut().set_sort_down_hook(std::rc::Rc::new(
                    move |column: &str, descending: bool, app: &mut App| {
                        let column = column.to_string();
                        _ = sort_weak.update(app, |panel, cx| {
                            panel.sort_down(&column, descending, cx)
                        });
                    },
                ));
                if insight_port.is_some() {
                    state.delegate_mut().set_insight_column_hook(std::rc::Rc::new(
                        move |column: &str, app: &mut App| {
                            let column = column.to_string();
                            _ = insight_weak.update(app, |panel, cx| {
                                panel.insight_column(&column, cx)
                            });
                        },
                    ));
                }
            });
        }

        Self {
            shared,
            document,
            focus_handle,
            editor,
            statements,
            group: None,
            message: None,
            closed: false,
            grid,
            pending: 0,
            running_since: None,
            autocommit: true,
            tx_open: false,
            tx_since: None,
            tx_pending: 0,
            refresh_pending: 0,
            export_pending: 0,
            pending_labels: std::collections::VecDeque::new(),
            result_toolbar: None,
            result_status: None,
            result_tabs: Vec::new(),
            result_active: 0,
            result_sql: None,
            result_is_analysis: false,
            result_can_copy: false,
            result_error_card: None,
            error_site: None,
            error_site_key: None,
            window: window.window_handle(),
            result_height: std::rc::Rc::new(std::cell::Cell::new(ui::RESULT_PANE_HEIGHT)),
            pending_export: None,
            fetching_more: false,
            filter_input,
            filter_text: String::new(),
            filter_version: 0,
            filter_debounce: RefCell::new(None),
            pushdown: false,
            result_notice: None,
            result_selection: None,
            exec_pump: RefCell::new(None),
            _editor_sub: Some(sub),
            _grid_sub: Some(grid_sub),
            _filter_sub: Some(filter_sub),
        }
    }

    pub fn document(&self) -> &DocumentId {
        &self.document
    }

    /// 当前文档是否脏（与基线不同）
    pub fn is_dirty(&self) -> bool {
        self.with_document(Document::is_dirty).unwrap_or(false)
    }

    /// 面板是否已被 Dock 移除（文档随之关闭）
    ///
    /// 宿主用它清理自己那份面板列表：`DockArea` 关掉一个 tab 时不会通知宿主，
    /// 只有面板自己知道（`Panel::on_removed`）。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 保存当前文档（**只从事件路径调用**：`Ctrl+S` / 保存命令）
    ///
    /// 先写盘、后清脏（见 `persist::save_document`）：写失败不会留下“已保存”的假状态。
    /// 未命名文档返回 `Untitled`，调用方应改走“另存为”。
    pub fn save(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<std::path::PathBuf, persist::PersistError> {
        let result = persist::save_document(&self.shared, &self.document);
        if result.is_ok() {
            cx.notify();
        }
        result
    }

    /// 另存为：写盘 → 换路径与标题（文档**身份不变**，只换路径）
    ///
    /// 路径由宿主给出（系统文件对话框在 workbench 侧，本 crate 不依赖 `rfd`）。
    pub fn save_as(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Result<std::path::PathBuf, persist::PersistError> {
        let result = persist::save_as(&self.shared, &self.document, path);
        if result.is_ok() {
            cx.notify();
        }
        result
    }

    /// 文档标题（宿主拼确认文案用；文档不存在时给“（已关闭）”）
    pub fn title_text(&self) -> String {
        self.with_document(|doc| doc.title().to_string())
            .unwrap_or_else(|| "（已关闭）".to_string())
    }

    /// 文档落盘路径（另存为对话框的默认目录 / 默认文件名用它）
    pub fn document_path(&self) -> Option<std::path::PathBuf> {
        self.with_document(|doc| doc.path().map(std::path::Path::to_path_buf))
            .flatten()
    }

    /// 文档当前模式（宿主取另存为的默认文件名后缀用它）
    pub fn document_mode(&self) -> Option<EditorMode> {
        self.with_document(|doc| doc.mode())
    }

    /// 把自己所在的标签激活（宿主打开已存在文档时调用）
    ///
    /// `TabGroup::select_tab` 是 Dock 公开的激活入口；面板从 `on_added_to` 拿到组句柄，
    /// 因此不必反持 `DockArea`（容器知道面板就够了）。
    ///
    /// “当前文档”在这里**直接**置位，不等 Dock 的帧末激活回调：面板是组里唯一标签时
    /// `select_tab` 会因“已经是当前标签”而早退（不发 `ActiveChanged`），
    /// 但服务层的当前文档可能已经被别的打开动作抢走过。
    pub fn focus_self(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .update(|service| service.activate(&self.document));
        let Some(group) = self.group.clone() else {
            return;
        };
        let id = PanelId::from(cx.entity_id());
        let found = group.update(cx, |group, cx| {
            let index = group
                .panels()
                .iter()
                .position(|panel| panel.panel_id(cx) == id);
            if let Some(index) = index {
                group.select_tab(index, window, cx);
            }
            index.is_some()
        });
        if !matches!(found, Ok(true)) {
            // 组已不在（面板被拖到另一个组 / 整棵 Dock 被重建）：动作不落地，但不静默
            self.set_message(Some("面板不在标签组里，无法激活".to_string()), cx);
        }
    }

    /// 状态栏提示（`None` = 清空）
    pub fn set_message(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        self.message = text;
        cx.notify();
    }

    /// 清掉状态栏提示（动作成功 / 用户取消时调：旧提示不该继续挂着）
    pub fn clear_message(&mut self, cx: &mut Context<Self>) {
        self.set_message(None, cx);
    }

    /// 当前会话快照（关文档 / 退出时保存用）
    ///
    /// 未命名文档没有稳定标识，返回 `None`（不存——重启后无法把它认回来）。
    pub fn session_snapshot(&self, cx: &App) -> Option<crate::session::SavedSession> {
        let (path, mode, channel, connection) = self.with_document(|doc| {
            let path = doc.path()?.to_string_lossy().into_owned();
            Some((path, doc.mode(), doc.channel(), doc.connection().map(str::to_string)))
        })??;
        let id = crate::session::session_id_for_path(std::path::Path::new(&path));

        let (cursor, selection) = {
            let state = self.editor.read(cx);
            let range = state.selected_range();
            let selection = (range.end > range.start).then_some((range.start, range.end));
            (range.start, selection)
        };

        Some(crate::session::SavedSession {
            id,
            path: Some(path),
            mode,
            channel,
            connection,
            content: self.editor_text(cx),
            cursor,
            selection,
        })
    }

    /// 保存会话（关文档 / 退出时调用；未命名或空文档自动跳过）
    pub fn save_session_now(&self, cx: &App) {
        let Some(session) = self.session_snapshot(cx) else {
            return;
        };
        if !crate::session::worth_saving(&session) {
            return;
        }
        if let Err(error) = self.shared.save_session(&session) {
            // 事件路径上不弹窗：落到 stderr 由日志承接，不静默吞掉
            eprintln!("[editor] 会话保存失败（{}）：{error}", session.id);
        }
    }

    /// 恢复会话：把光标/选区写回内核（模式与内容由宿主开文档时给出）
    pub fn restore_session(
        &mut self,
        session: &crate::session::SavedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 会话可能比现在的文档长（内容被截断 / 外部修改）：光标钳到当前文本末尾
        let len = self.editor_text(cx).len();
        let clamp = |offset: usize| offset.min(len);
        let start = clamp(session.selection.map(|(start, _)| start).unwrap_or(session.cursor));
        let end = clamp(session.selection.map(|(_, end)| end).unwrap_or(session.cursor));

        self.editor.update(cx, |state, cx| {
            // `set_cursor_position` 是内核里唯一“滚到指定偏移”的公开入口（会顺带聚焦）
            let position = {
                use gpui_kit::component::input::RopeExt as _;
                state.text().offset_to_position(start)
            };
            state.set_cursor_position(position, window, cx);
            state.set_selected_range(start..end, cx);
        });
        // 【B13】通道也随会话回来；**已失效就回退源库并说清原因**（原型 §5.7：绑定过但
        // 现已失效——比如之后再打开时那个连接没开本地加速——不能静静地接着用它）
        self.restore_channel(session.channel, cx);
        // 【B1】连接绑定同理：那条连接可能已经被删了 / 不在当前项目
        self.restore_connection(session.connection.as_deref(), cx);
        cx.notify();
    }

    /// 【B1 余项】把会话里的**连接绑定**写回文档
    ///
    /// 判据用宿主给的连接列表快照：列表里没有 = 连接被删 / 换了项目 → **回退到「跟随当前连接」
    /// 并把原因说出来**（留一个指向不存在连接的绑定，会让用户执行时才发现发错了地方）。
    /// 端口未接 = **不校验也不丢**（测试与嵌入场景；宁可不猜也不把用户的绑定抹掉）。
    /// 不主动建连：启动恢复不该拨号，执行时由执行器按绑定去建/去用。
    fn restore_connection(&mut self, connection: Option<&str>, cx: &mut Context<Self>) {
        let Some(conn_id) = connection else {
            return;
        };
        if !self.shared.has_connections() {
            self.shared.update(|service| {
                service.set_connection(&self.document, Some(conn_id.to_string()))
            });
            return;
        }
        if self
            .shared
            .connection_options()
            .iter()
            .any(|option| option.id == conn_id)
        {
            self.shared.update(|service| {
                service.set_connection(&self.document, Some(conn_id.to_string()))
            });
            return;
        }
        self.set_message(
            Some(format!(
                "连接 {conn_id} 不在当前连接列表：已回退到「跟随当前连接」"
            )),
            cx,
        );
    }

    /// 【B13】把会话里的通道写回文档（不可用 → 留源库档 + 状态栏给原因）
    fn restore_channel(&mut self, channel: ExecChannel, cx: &mut Context<Self>) {
        if channel == ExecChannel::Source {
            return;
        }
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        let gate = availability.for_channel(channel);
        if gate.available {
            self.shared
                .update(|service| service.set_channel(&self.document, channel));
            self.sync_result_view(cx);
            return;
        }
        let reason = gate.reason.unwrap_or_else(|| "不可用".to_string());
        self.set_message(
            Some(format!("{}通道已失效：{reason}（回退源库）", channel.label())),
            cx,
        );
    }

    /// 编辑器是否可输入（供窗口测试断言只读组合；生产代码读同一个判据）
    pub fn is_editable_for_test(&self) -> bool {
        !self.editor_read_only()
    }

    /// 当前内核文本（供测试断言动作效果）
    pub fn text_for_test(&self, cx: &App) -> String {
        self.editor_text(cx)
    }

    /// 服务层记着的文档内容（供测试断言“内容真的写回了服务层”，不只是画在屏幕上）
    pub fn document_content_for_test(&self) -> Option<String> {
        self.with_document(|doc| doc.content().to_string())
    }

    /// 编辑内核的焦点句柄（供测试把焦点交给内核，模拟真实打字场景）
    pub fn editor_focus_handle_for_test(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }

    /// 当前选区（供测试断言光标 / 选区恢复：无选区时是 `cursor..cursor`）
    pub fn selected_range_for_test(&self, cx: &App) -> std::ops::Range<usize> {
        self.editor.read(cx).selected_range()
    }

    /// 只读访问当前文档（渲染路径用；不克隆整份文档）
    fn with_document<R>(&self, read: impl FnOnce(&Document) -> R) -> Option<R> {
        let service = self.shared.service();
        service.find(&self.document).map(read)
    }

    fn editor_text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    /// 编辑内核快照（文本 + 选区）：执行目标解析的输入
    fn editor_snapshot(&self, cx: &App) -> (String, std::ops::Range<usize>) {
        let state = self.editor.read(cx);
        (state.value().to_string(), state.selected_range())
    }

    /// 工具栏（原型 §2.2）：按其分层只放**今天真有动作**的控件
    ///
    /// - 最左：**模式指示器**（文本 / SQL / 分析，点击切换）
    /// - 执行级：**执行 ▾**——主按钮 = 执行（选区优先 → 当前语句），下拉 = 执行族里已经能跑的
    ///   三项（当前语句 / 选区 / 全部）。**只在 SQL 模式出现**：文本模式按能力表不通信，
    ///   分析模式的执行属笔记级动作（1c 随单元落地）。
    /// - 文档级：**⇤ 格式化**——整篇 / 选区，与 `Ctrl+Shift+F` 同一条路。同样只在
    ///   SQL 模式出现（文本模式不解析 SQL；分析模式的格式化为笔记级动作）。
    /// - 还没实现的（历史 / ⋯更多）**不放按钮**——
    ///   “只宣传不实现”是原型 §2.2 的明确排除项。
    fn render_toolbar(&self, mode: EditorMode, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors.border;
        let entity = cx.entity();
        let current_channel = self.channel();

        let mut toolbar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .h(rems(ui::EDITOR_TOOLBAR_HEIGHT))
            .border_b(ui::HAIRLINE)
            .border_color(border)
            .child(
                Button::new("editor-mode-indicator")
                    .ghost()
                    .small()
                    // 测试按选择器断言“这个模式该有哪些控件”：模式与工具栏分层不是装饰
                    .debug_selector(|| "editor-mode-indicator".to_string())
                    .label(format!("{} ▾", mode.label()))
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        for candidate in EditorMode::ALL {
                            let is_current = candidate == mode;
                            let entity = entity.clone();
                            menu = menu.item(
                                PopupMenuItem::new(candidate.label())
                                    // 当前模式打勾而不是隐藏：三个模式总在同一处可切
                                    .checked(is_current)
                                    .on_click(move |_, window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.request_mode_switch(candidate, window, cx);
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            );

        if mode == EditorMode::Sql {
            // 只有 SQL 模式的执行是“对当前文档执行”：文本模式按能力表不通信，
            // 分析模式的执行是**笔记级**动作（单元级按钮 + Shift+Enter，1c 落地）
            toolbar = toolbar.child(self.render_exec_group(cx));
            // 文档级：格式化（原型 §2.2：执行级之后是文档级）
            toolbar = toolbar.child(self.render_format_button(cx));
            // 文档级：⋯ 更多 ▾（低频动作收拢：方言转译 …）
            toolbar = toolbar.child(self.render_more_group(cx));
            // 通道级 + 连接（右对齐）：原型 §2.2 两栏——“执行位置”在“连接”左边
            toolbar = toolbar.child(
                div()
                    .ml_auto()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(current_channel == ExecChannel::Federated, |row| {
                        row.child(self.render_sources_picker(cx))
                    })
                    .child(self.render_channel_picker(cx))
                    .child(self.render_connection_picker(cx)),
            );
        }
        toolbar
    }

    /// 【B13】执行位置选择器（工具栏右侧：原型 §2.2 的「执行位置 ▾」）
    ///
    /// 三档**互斥**（原型 §5.7）：源库直连 / DuckDB 本地加速 / 联邦查询。菜单项由纯函数
    /// [`channel::menu_items`] 给（哪项能点、为什么不能点都在那儿定），这里只负责画：
    /// 不可用项**保留形态但置灰 + 行尾给原因**，当前项打勾。
    ///
    /// 本地跑的那两档额外给一个**维护动作**：`重新挂载源库（刷新表清单）`（见
    /// [`channel::refresh_item_label`]）——源库新建的表要重挂才看得见。
    fn render_channel_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let current = self.channel();
        let items = self.channel_menu();
        let refresh_label = channel::refresh_item_label(current);

        Button::new("editor-channel")
            .ghost()
            .small()
            .debug_selector(|| "editor-channel".to_string())
            .label(format!("执行位置：{} ▾", current.label()))
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                // 菜单回调是 `Fn`：不能把 `items` 搬进去，逐项 clone 字段名与下标（少而短）
                for item in items.iter() {
                    let entity = entity.clone();
                    let channel = item.channel;
                    menu = menu.item(
                        PopupMenuItem::new(item.label.clone())
                            .checked(item.current)
                            .disabled(!item.available)
                            .on_click(move |_, _window, app| {
                                entity.update(app, |panel, cx| panel.set_channel(channel, cx));
                            }),
                    );
                }
                if let Some(label) = refresh_label {
                    let entity = entity.clone();
                    menu = menu.separator().item(
                        PopupMenuItem::new(label).on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.refresh_source(cx));
                        }),
                    );
                }
                menu
            })
    }

    /// 本文档绑定的连接 id（`None` = 未绑定，执行跟随当前活动连接）
    fn bound_connection(&self) -> Option<String> {
        self.with_document(|doc| doc.connection().map(str::to_string))
            .flatten()
    }

    /// 本文档当前的执行通道（B13；文档属性，读的是服务层的真值）
    pub fn channel(&self) -> ExecChannel {
        self.with_document(|doc| doc.channel())
            .unwrap_or_default()
    }

    /// 源库档能不能用：绑定的连接已建连（**未绑定算可用**——那是在跟随当前活动连接，
    /// 它到底连没连不归这里断言，真发错东西了由执行器报可读原因）
    fn source_channel_ready(&self) -> bool {
        match self.bound_connection() {
            None => true,
            Some(id) => self
                .shared
                .connection_options()
                .iter()
                .find(|option| option.id == id)
                .is_some_and(|option| option.connected),
        }
    }

    /// 【B13】「执行位置 ▾」的菜单项（纯函数算的：可用性 + 不可用原因 + 当前项打勾）
    ///
    /// 门控真值来自宿主注入的 `ChannelsPort`；**没注入端口就不给选**（如实报“尚未接入”）。
    pub fn channel_menu(&self) -> Vec<channel::ChannelMenuItem> {
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        channel::menu_items(self.channel(), self.source_channel_ready(), &availability)
    }

    /// 【B10】格式化整篇 / 选区（`Ctrl+Shift+F`、工具栏「⇤ 格式化」）
    ///
    /// 计划（格式化哪一段 / 光标去哪 / 有几条没动）由纯函数 [`crate::format::plan`] 算；
    /// 这里只做三件事：**只读拒绝**、落一次**可撤销**的替换（`replace_all` 保留撤销历史）、
    /// 把结果如实说给用户（“改了 N 条” / “M 条解析不了，原样保留”）。
    pub(crate) fn format_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_read_only() {
            self.set_message(Some("只读文档不能格式化".to_string()), cx);
            return;
        }
        if !self.execution_allowed() {
            // 文本模式是“记事本”：不解析、不上色，自然也不格式化（能力表说了算）
            self.set_message(Some("文本模式不解析 SQL".to_string()), cx);
            return;
        }
        let (text, selection) = self.editor_snapshot(cx);
        let dialect = format::dialect_of(&self.connection_db_type());
        let plan = format::plan(&text, selection, dialect);

        if !plan.changes(&text) {
            // 没改动也要说清楚是“本来就是格式化好的”还是“解析不了”——两者感受完全不同
            let message = if plan.kept_verbatim > 0 {
                format!(
                    "{} 条语句解析不了（可能是还没写完），已原样保留；其余本来就是格式化好的",
                    plan.kept_verbatim
                )
            } else {
                "已经是格式化后的样子".to_string()
            };
            self.set_message(Some(message), cx);
            return;
        }

        self.apply_rewrite(
            plan.text.clone(),
            plan.selection.clone(),
            plan.cursor,
            window,
            cx,
        );
        // 内核的 Change 事件会把内容写回服务层（脏状态跟着变），这里只补一句结果
        let mut message = format!("已格式化 {} 条语句", plan.formatted);
        if plan.kept_verbatim > 0 {
            message.push_str(&format!(
                "；{} 条解析不了，原样保留",
                plan.kept_verbatim
            ));
        }
        self.set_message(Some(message), cx);
    }

    /// 【B10】方言转译整篇 / 选区（工具栏「⋯ 更多 ▾ ▸ 转译为」）
    ///
    /// 与格式化同一条路（计划在 [`crate::translate::plan`]，落地在 [`Self::apply_rewrite`]），
    /// 但**源方言是硬前提**：转译的方向错不得，所以未绑定连接 / 驱动认不出时直接拒绝并说明，
    /// 不拿起 Ansi 乱翻（那会把 `\``a\`` 与 `# 注释` 静默改成别的意思）。
    pub(crate) fn transpile_document(
        &mut self,
        target: engine::sql::SqlDialect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("只读文档不能转译".to_string()), cx);
            return;
        }
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不解析 SQL".to_string()), cx);
            return;
        }
        let source = match self.source_dialect() {
            Ok(source) => source,
            Err(reason) => {
                self.set_message(Some(reason), cx);
                return;
            }
        };
        if source == target {
            self.set_message(Some("目标方言与源相同，没什么可转译的".to_string()), cx);
            return;
        }

        let label = translate::label_of(target);
        let (text, selection) = self.editor_snapshot(cx);
        let plan = translate::plan(&text, selection, source, target);
        if !plan.changes(&text) {
            let message = if plan.kept_verbatim > 0 {
                format!(
                    "{} 条语句转译不了（可能是还没写完），已原样保留",
                    plan.kept_verbatim
                )
            } else {
                format!("已经是 {label} 能直接用的写法")
            };
            self.set_message(Some(message), cx);
            return;
        }

        self.apply_rewrite(
            plan.text.clone(),
            plan.selection.clone(),
            plan.cursor,
            window,
            cx,
        );
        let mut message = format!("已转译为 {label}（{} 条语句）", plan.transpiled);
        if plan.kept_verbatim > 0 {
            message.push_str(&format!(
                "；{} 条解析不了，原样保留",
                plan.kept_verbatim
            ));
        }
        self.set_message(Some(message), cx);
    }

    /// 把一次“整篇 / 选区改写”的结果落到内核（格式化与转译共用）
    ///
    /// - 用 `replace_all`（进撤销栈）——`set_value` 会清空撤销历史，绝不能用于这两个动作；
    /// - 落完把选区 / 光标恢复回去：用户按的是“排一下 / 翻一下”，不是“跳回文首”。
    fn apply_rewrite(
        &mut self,
        new_text: String,
        selection: Option<std::ops::Range<usize>>,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            state.replace_all(new_text, window, cx);
            match selection.clone() {
                Some(range) => state.set_selected_range(range, cx),
                None => {
                    // 光标按“第几条语句里的第几个字节”映射过去，并滑到可见区
                    use gpui_kit::component::input::RopeExt as _;
                    let position = state.text().offset_to_position(cursor);
                    state.set_cursor_position(position, window, cx);
                    state.set_selected_range(cursor..cursor, cx);
                }
            }
        });
    }

    /// 本文档的**源方言**（转译用）
    ///
    /// 未绑定连接 / 驱动认不出 → `Err(原因)`：转译的方向错不得，宁可不做也不猜。
    fn source_dialect(&self) -> Result<engine::sql::SqlDialect, String> {
        let db_type = self.connection_db_type();
        if db_type.trim().is_empty() {
            return Err("未绑定连接：转译要知道源方言（先选一个连接）".to_string());
        }
        format::dialect_of_known(&db_type)
            .ok_or_else(|| format!("认不出驱动「{db_type}」的方言，无法确定转译方向"))
    }

    /// 本文档绑定连接的驱动类型（`None` / 未绑定 → 空串 → 方言用 Ansi）
    ///
    /// 方言只能从**连接**来（同一个连接两种引擎时不能猜）；编辑器不知道驱动，
    /// 是宿主填在 `ConnectionOption.db_type` 里的。
    fn connection_db_type(&self) -> String {
        let bound = self
            .with_document(|doc| doc.connection().map(str::to_string))
            .flatten();
        bound
            .and_then(|id| {
                self.shared
                    .connection_options()
                    .into_iter()
                    .find(|option| option.id == id)
                    .map(|option| option.db_type)
            })
            .unwrap_or_default()
    }

    /// 本文档绑定连接的驱动类型（供测试断言方言选择）
    pub fn connection_db_type_for_test(&self) -> String {
        self.connection_db_type()
    }

    /// 【T1.6】源清单选择器（工具栏右侧，**只在联邦档出现**）
    ///
    /// 内容是 [`crate::sources::menu_entries`] 算的（纯函数）：汇总 + 逐行源（失败行带原话）
    /// + 动作（重挂全部 / 设为主源 / 逐源重挂）。快照是**宿主注入的内存读**；动作走旁路
    /// 线程（`SourceAction`），回执回到状态栏。
    fn render_sources_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let snapshot = self.sources_snapshot();
        let entries = sources::menu_entries(snapshot.as_ref());
        let label = sources::button_label(snapshot.as_ref());

        Button::new("editor-sources")
            .ghost()
            .small()
            .debug_selector(|| "editor-sources".to_string())
            .label(label)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for entry in entries.iter() {
                    let entity = entity.clone();
                    menu = match entry {
                        // 信息行与源行都是“看”的：不可点（禁用态在菜单里就是灰字）
                        sources::SourceMenuEntry::Info(text) => {
                            menu.item(PopupMenuItem::new(text.clone()).disabled(true))
                        }
                        sources::SourceMenuEntry::Source(row) => {
                            menu.item(PopupMenuItem::new(row.text()).disabled(true))
                        }
                        sources::SourceMenuEntry::Action { label, action } => {
                            let action = action.clone();
                            menu.item(PopupMenuItem::new(label.clone()).on_click(
                                move |_, _window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.run_source_action(action.clone(), cx);
                                    });
                                },
                            ))
                        }
                    };
                }
                menu
            })
    }

    /// 这个文档的联邦源清单快照（**渲染路径可调**：宿主给的是内存读）
    pub fn sources_snapshot(&self) -> Option<sources::SourcesSnapshot> {
        let conn_id = self.bound_connection()?;
        self.shared.sources_snapshot(&conn_id)
    }

    /// 【T1.6】源清单里的动作（重挂 / 换主源）
    ///
    /// 与「重新挂载源库」同一条路：旁路线程 + 回执（不占执行位）。没绑连接时直说原因。
    pub(crate) fn run_source_action(
        &mut self,
        action: crate::execution::SourceAction,
        cx: &mut Context<Self>,
    ) {
        if !self.channel().runs_locally() {
            self.set_message(
                Some("现在不是本地跑的那两档，源清单动作没意义".to_string()),
                cx,
            );
            return;
        }
        let channel = self.channel();
        let action_label = action.label();
        match self.shared.request_source_action(
            self.document.clone(),
            self.bound_connection(),
            channel,
            action,
        ) {
            Ok(()) => {
                self.refresh_pending += 1;
                self.set_message(Some(format!("正在{action_label}…")), cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【B13】重新挂载源（表清单刷新）
    ///
    /// 加速档重挂那一条源；联邦档逐源全挂（源清单里的是行级动作）。动作在旁路线程上做，
    /// 回执由轮询泵取回。
    pub(crate) fn refresh_source(&mut self, cx: &mut Context<Self>) {
        if !self.channel().runs_locally() {
            self.set_message(
                Some("现在不是本地加速档，没有可重新挂载的源".to_string()),
                cx,
            );
            return;
        }
        self.run_source_action(crate::execution::SourceAction::RefreshAll, cx);
    }

    /// 【B13】还没回执的重新挂载数（供测试断言）
    pub fn refresh_pending_for_test(&self) -> usize {
        self.refresh_pending
    }

    /// 【B7 切片二】还没回执的 DuckDB 导出数（供测试断言）
    pub fn export_pending_for_test(&self) -> usize {
        self.export_pending
    }

    /// 【B13】切换执行通道（「执行位置 ▾」调用）
    ///
    /// 门控是**第二道闸**（菜单已经置灰了）：不可用的通道就算被程序叫到也不给切——
    /// 否则界面会停在一个“看着能用、一按就错”的状态。切完旧结果**保留**（原型 §5.7 规则 2：
    /// 不删用户的东西，只标灰 + 顶部一行说清它来自哪档）。
    pub(crate) fn set_channel(&mut self, channel: ExecChannel, cx: &mut Context<Self>) {
        if channel != ExecChannel::Source {
            let availability = self
                .shared
                .channel_availability(self.bound_connection().as_deref());
            let gate = availability.for_channel(channel);
            if !gate.available {
                let reason = gate.reason.unwrap_or_else(|| "不可用".to_string());
                self.set_message(Some(format!("{}不可用：{reason}", channel.label())), cx);
                return;
            }
        }
        let changed = self
            .shared
            .update(|service| service.set_channel(&self.document, channel));
        // 结果区的投影跟着换（旧结果标灰与顶部提示都在那边算）；它会重写状态栏提示，
        // 所以“已切到 X”这句必须放在它**之后**说
        self.sync_result_view(cx);
        if !changed {
            return; // 本来就在这档：不啥都留痕（切了才留）
        }
        let mut message = format!("执行位置已切到{}", channel.label());
        if channel.runs_locally() {
            message.push_str("（源库以只读方式挂载；作用源库的写语句会被拒）");
        }
        if self.tx_open && !channel.allows_transactions() {
            message.push_str("；源库上还有未提交的事务（切回源库可见）");
        }
        self.set_message(Some(message), cx);
    }

    /// 【B13】当前通道能不能跑写语句（本地加速档对源库是**只读挂载**，写语句直接拒）
    fn channel_write_check(&self, target: &ExecTarget) -> Result<(), String> {
        // 【B15】本地分析不碰源库（数据是桥接过来的行）——通道闸管的是“写源库对象”，放行
        if matches!(target, ExecTarget::Analysis(_)) {
            return Ok(());
        }
        let channel = self.channel();
        if channel.allows_source_writes() {
            return Ok(());
        }
        for sql in target.statements() {
            channel::statement_allowed(channel, &sql)?;
        }
        Ok(())
    }

    /// 项目只读闸（原型 §1.4 的“连接只读”维度）：写源库对象的语句一律拒
    ///
    /// 与通道闸**互不替代**：通道闸管“在哪儿跑”（本地两档对源库是只读挂载，永远不能写），
    /// 这里管“项目锁着的时候能不能写库”（解锁后就能写）。读语句一律不受影响。
    ///
    /// **强度**：项目锁（另一实例占用 / 归档项目）就属架构 §13 #9 里的“**强只读**”那一档 →
    /// 直接拒；连接策略里的“提醒后放行”那档随连接策略（B1 余项）一起做，现在不假装有。
    /// 判据与通道闸同一份（[`channel::writes_source_object`]）：宁可多拒一句，不能放过去真写。
    pub(crate) fn project_write_check(&self, target: &ExecTarget) -> Result<(), String> {
        // 【B15】本地分析同上：它在本地 DuckDB 上建临时表，与项目锁无关
        if matches!(target, ExecTarget::Analysis(_)) {
            return Ok(());
        }
        if !self.shared.project_read_only() {
            return Ok(());
        }
        for sql in target.statements() {
            if channel::writes_source_object(&sql) {
                return Err("项目为只读模式：写语句被拒（要改数据先解锁项目）".to_string());
            }
        }
        Ok(())
    }

    /// 状态栏用的只读两维度：文档自身的那一维 + **项目锁**（连接只读那一维的真值）
    ///
    /// 合成在这里而不写回文档：项目锁是窗口级状态，写进 `Document` 会留下“解锁了但文档还
    /// 写着只读”的陈旧值。
    pub(crate) fn read_only_flags(&self) -> ReadOnly {
        let document = self.with_document(|doc| doc.read_only()).unwrap_or_default();
        ReadOnly {
            editor: document.editor,
            connection: document.connection || self.shared.project_read_only(),
        }
    }

    /// 【B13】通道现在能不能执行（不可用就给可读原因）
    fn channel_ready(&self) -> Result<(), String> {
        let channel = self.channel();
        if channel == ExecChannel::Source {
            return Ok(());
        }
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        let gate = availability.for_channel(channel);
        if gate.available {
            Ok(())
        } else {
            Err(format!(
                "{}不可用：{}",
                channel.label(),
                gate.reason.unwrap_or_else(|| "不可用".to_string())
            ))
        }
    }

    /// 连接选择器（工具栏右侧）：本文档执行时用哪个连接（B1）
    ///
    /// - 已绑定 → 菜单里打勾；未绑定 → 菜单第一项「跟随当前连接」打勾（1a 口径，如实说明）
    /// - 选中一个连接时**先自动建连**（同 M4）：建连失败就**不绑定**，并在状态栏给出原因
    /// - 未接端口（宿主没给列表）时菜单直说“未接入连接列表”，不假装有可选项
    fn render_connection_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let options = self.shared.connection_options();
        let has_port = self.shared.has_connections();
        let bound = self
            .with_document(|doc| doc.connection().map(str::to_string))
            .flatten();
        let label = match self.shared.connection_chip(bound.as_deref()) {
            Some(chip) => format!("{} ▾", chip.text()),
            None => "未绑定连接 ▾".to_string(),
        };
        let current = bound.clone();

        Button::new("editor-connection")
            .ghost()
            .small()
            .debug_selector(|| "editor-connection".to_string())
            .label(label)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if !has_port {
                    return menu.item(PopupMenuItem::new("（未接入连接列表）").disabled(true));
                }
                // 不绑定也是合法选择：执行跟随当前活动连接（与 1a 一致）
                let follow_entity = entity.clone();
                menu = menu.item(
                    PopupMenuItem::new("跟随当前连接")
                        .checked(current.is_none())
                        .on_click(move |_, _window, app| {
                            follow_entity.update(app, |panel, cx| {
                                panel.bind_connection(None, cx);
                            });
                        }),
                );
                menu = menu.separator();
                for option in options.iter() {
                    let is_current = current.as_deref() == Some(option.id.as_str());
                    let entity = entity.clone();
                    let label = format!("{} · {}", option.short, option.name);
                    let id = option.id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(is_current)
                            .on_click(move |_, _window, app| {
                                let id = id.clone();
                                entity.update(app, |panel, cx| {
                                    panel.bind_connection(Some(id), cx);
                                });
                            }),
                    );
                }
                menu
            })
    }

    /// 绑定连接（工具栏选择器调用）：先用端口确保已建连，失败就**不绑定**并留原因
    ///
    /// `None` = 解绑（跟随当前连接）。绑定是**文档属性**，同一窗口的多份文档互不影响。
    pub(crate) fn bind_connection(
        &mut self,
        conn_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(conn_id) = conn_id else {
            self.shared
                .update(|service| service.set_connection(&self.document, None));
            self.set_message(None, cx);
            return;
        };

        match self.shared.ensure_connected(&conn_id) {
            Ok(()) => {
                self.shared.update(|service| {
                    service.set_connection(&self.document, Some(conn_id.clone()));
                });
                let chip = self.shared.connection_status_text(Some(&conn_id));
                self.set_message(Some(format!("已绑定 {chip}")), cx);
            }
            Err(reason) => {
                // 建连失败不绑定：半绑定状态比不绑定更难排查
                self.set_message(Some(format!("连接不可用：{reason}")), cx);
            }
        }
    }

    /// 执行族（执行级）：主按钮 + 下拉菜单
    ///
    /// 主按钮与 `Ctrl+Enter` 走**同一条路**（`resolve_target`：选区优先）；下拉里的五项是
    /// **显式目标**（`target_for_menu`，后两项见 [`ExecMenuKind`] 的注释）——“执行选区”
    /// 没选区时置灰、“批量执行”语句不足两条时置灰，比“点了没反应”明确。
    fn render_exec_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let run_entity = cx.entity();
        let (_, selection) = self.editor_snapshot(cx);
        let statements = self.statements;

        DropdownButton::new("editor-exec")
            .small()
            .button(
                Button::new("editor-exec-run")
                    .primary()
                    .small()
                    .debug_selector(|| "editor-exec-run".to_string())
                    .label("执行")
                    .on_click(move |_, _window, app| {
                        run_entity.update(app, |panel, cx| {
                            panel.execute_preferring_selection(cx);
                        });
                    }),
            )
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for kind in ExecMenuKind::ALL {
                    let entity = entity.clone();
                    let available = kind.is_available(&selection, statements);
                    menu = menu.item(
                        PopupMenuItem::new(kind.label()).disabled(!available).on_click(
                            move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    let (text, selection) = panel.editor_snapshot(cx);
                                    let target = execution::target_for_menu(kind, &text, selection);
                                    panel.execute(target, kind.placement(), cx);
                                });
                            },
                        ),
                    );
                }
                menu
            })
    }

    /// 【B10】文档级：格式化按钮（原型 §2.2 的「⇤ 格式化」，紧随执行级）
    fn render_format_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("editor-format")
            .ghost()
            .small()
            .debug_selector(|| "editor-format".to_string())
            .label("格式化")
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| panel.format_document(window, cx));
            })
    }

    /// 【B10】文档级：`⋯ 更多 ▾`（原型 §2.2 的“低频动作收拢”，不占常驻宽度）
    ///
    /// 今天只有 **方言转译**（切片二）；原型的“执行计划”随切片三进来，“校验语法 / 保存为
    /// 片段 / 复制为 INSERT”还没实现——**没实现就不摆**。
    ///
    /// 目标清单在**渲染时**定好（源方言已知就排除它自己）；菜单回调是 `Fn`，不能在里头读面板。
    /// 源方言未知（未绑定连接）时仍把目标列出来——点下去会收到一句可读的拒绝，
    /// 比“菜单里什么都没有”更容易看懂。
    fn render_more_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let targets: Vec<translate::Target> = match self.source_dialect() {
            Ok(source) => translate::targets_for(source),
            Err(_) => translate::TARGETS.to_vec(),
        };
        // 【B9 切片二】模板片段（内存读：菜单弹出时读一次；没模板 / 没接端口就不摆这项）
        let templates = {
            let items = self.shared.completion_templates();
            (!items.is_empty()).then_some(items)
        };

        Button::new("editor-more")
            .ghost()
            .small()
            .debug_selector(|| "editor-more".to_string())
            .label("⋯ 更多 ▾")
            .dropdown_menu(move |menu, window, cx| {
                // 【B10】执行计划在“转译”之前（原型 §2.2 的更多菜单顺序）
                let plan_entity = entity.clone();
                let menu = menu.item(
                    PopupMenuItem::new("执行计划").on_click(move |_, _window, app| {
                        plan_entity.update(app, |panel, cx| panel.explain_current(cx));
                    }),
                );
                menu.submenu("转译为", window, cx, {
                    let entity = entity.clone();
                    let targets = targets.clone();
                    move |sub, _window, _cx| {
                        let mut sub = sub;
                        for target in targets.iter() {
                            let entity = entity.clone();
                            let dialect = target.dialect;
                            sub = sub.item(PopupMenuItem::new(target.label).on_click(
                                move |_, window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.transpile_document(dialect, window, cx);
                                    });
                                },
                            ));
                        }
                        sub
                    }
                })
                // 【B9 切片二】插入模板（来自 `sql_template_store`；没模板就不摆这一项）
                .when_some(templates.clone(), |menu, templates| {
                    menu.submenu("插入模板", window, cx, {
                        let entity = entity.clone();
                        move |sub, _window, _cx| {
                            let mut sub = sub;
                            for (index, template) in templates.iter().enumerate() {
                                let entity = entity.clone();
                                sub = sub.item(PopupMenuItem::new(template.name.clone()).on_click(
                                    move |_, window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.insert_template(index, window, cx);
                                        });
                                    },
                                ));
                            }
                            sub
                        }
                    })
                })
            })
    }

    fn editor_read_only(&self) -> bool {
        self.with_document(|doc| !doc.read_only().can_edit())
            .unwrap_or(true)
    }

    /// 模式切换后同步视图侧状态（高亮是否启用、只读）
    ///
    /// 模式是**文档属性**：判定与确认在 `mode::plan_switch` 与对话框层，这里只负责
    /// “按当前模式刷新视图”，由切换流程在确认之后调用（**视图不自己改模式**）。
    pub fn sync_mode(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let read_only = self.editor_read_only();

        self.editor.update(cx, |state, cx| {
            state.set_readonly(read_only, cx);
            if highlight::is_enabled(mode) {
                highlight::install(state);
            } else {
                // 非 SQL 模式去掉着色（已缓存的 token 会随内容变化失效）
                state.lsp_mut().semantic_tokens_provider = None;
            }
            // B9：补全按能力表装 / 摘（文本模式不接，分析模式留 1c 逐单元）
            self.sync_completion_provider(state);
        });
        cx.notify();
    }

    /// 按当前文档的能力与档位装 / 摘补全 provider（**唯一开关处**：新建与切模式都走它）
    ///
    /// 真值在 `EditorShared::completion_enabled`（能力表 + 编辑器只读 + 大文件档位）——
    /// 这里只把那个判断落到内核上，不另写一套判据。
    fn sync_completion_provider(&self, state: &mut gpui_kit::component::input::EditorState) {
        if self.shared.completion_enabled(&self.document) {
            completion::install(state, self.shared.clone(), self.document.clone());
        } else {
            completion::uninstall(state);
        }
    }

    /// 把文档内容推回内核（另存为 / 外部修改后重新加载时调用）
    pub fn reload_from_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.with_document(|doc| doc.content().to_string()) else {
            return;
        };
        self.statements = count_statements(&text);
        self.editor.update(cx, |state, cx| {
            if state.value() != text {
                state.set_value(text, window, cx);
            }
        });
        cx.notify();
    }

    // ===== 动作（A10）=====
    //
    // 动作只改共享状态或向内核写文本，不直接操作 Dock（关闭走 TabGroup 的请求路径）。

    /// `Ctrl+S`：保存；未命名 / 写盘失败 → 状态栏给出原因
    pub(crate) fn on_save(&mut self, _: &SaveDocument, _window: &mut Window, cx: &mut Context<Self>) {
        match self.save(cx) {
            Ok(_) => {
                // A12：保存时顺便把会话（光标/选区/模式）落库——用户按 Ctrl+S 是最自然的时机
                self.save_session_now(cx);
                self.set_message(None, cx);
            }
            Err(error) => self.set_message(Some(error.to_string()), cx),
        }
    }

    /// `Ctrl+Shift+F`：格式化（整篇 / 选区）
    ///
    /// 动作本身极薄：格式化是**文档级**动作，真伪都由 [`Self::format_document`] 判定
    /// （只读拒绝 / 文本模式拒绝 / 解析不了的语句原样保留）。
    pub(crate) fn on_format(
        &mut self,
        _: &FormatDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.format_document(window, cx);
    }

    /// `Ctrl+Space`：手动请一次补全（B9 切片二）
    ///
    /// 打字触发那条路由内核管（`is_completion_trigger`）；这条是**用户明确要候选**——
    /// 所以候选与打字那条**同一份**（`view/completion.rs::items_at`），只是替换范围改由
    /// 我们算（光标前的标识符）。拒绝的理由要说出来（文本模式 / 只读 / 没候选）。
    pub(crate) fn on_trigger_completion(
        &mut self,
        _: &crate::commands::TriggerCompletion,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.trigger_completion(cx);
    }

    /// 手动补全的落地（动作与测试都走它）
    pub(crate) fn trigger_completion(&mut self, cx: &mut Context<Self>) {
        if !self.shared.completion_enabled(&self.document) {
            // 三档拒绝理由分开说：文本模式 / 只读 / 大文件档位——“按了没反应”是不允许的
            let reason = self
                .with_document(|doc| {
                    if doc.mode() != EditorMode::Sql {
                        Some("文本模式不解析 SQL（补全是 SQL 模式的能力）".to_string())
                    } else if !doc.read_only().can_edit() {
                        Some("文档只读，无法补全".to_string())
                    } else if doc.tier().disables_completion() {
                        Some("文件太大（超大文件档位关掉了补全）".to_string())
                    } else {
                        None
                    }
                })
                .flatten()
                .unwrap_or_else(|| "当前不可补全".to_string());
            self.set_message(Some(reason), cx);
            return;
        }
        let (text, offset) = {
            let state = self.editor.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let (start, query, items) = completion::items_at(&self.shared, &self.document, &text, offset);
        if items.is_empty() {
            self.set_message(
                Some("没有可补的候选（元数据可能还没载好，或光标处认不出上下文）".to_string()),
                cx,
            );
            return;
        }
        self.editor.update(cx, |state, cx| {
            state.present_completion_items(start, query, items, cx);
        });
        self.set_message(None, cx);
    }

    /// 【B9 切片二】插入一条模板片段（「⋯ 更多 ▾ ▸ 插入模板」调用）
    ///
    /// 插入后**选中 `{table}` 占位符**：用户接着敲表名就把它覆写了。
    /// 只读与文本模式在这里就拒（与其它写文本的动作同口径）。
    pub(crate) fn insert_template(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法插入模板".to_string()), cx);
            return;
        }
        let templates = self.shared.completion_templates();
        let Some(template) = templates.get(index) else {
            return;
        };
        let content = template.content.clone();
        let name = template.name.clone();
        let start = {
            let state = self.editor.read(cx);
            state.cursor()
        };
        self.editor
            .update(cx, |state, cx| state.insert(content.clone(), window, cx));
        if let Some((from, to)) = crate::completion::placeholder_range(&content) {
            self.editor
                .update(cx, |state, cx| state.set_selected_range(start + from..start + to, cx));
        }
        self.set_message(Some(format!("已插入模板「{name}」")), cx);
    }

    /// `Ctrl+/`：行注释开关
    ///
    /// 写回内核走「选中块 → `replace` → 重新选中」：`replace` 会进撤销栈且保留历史，
    /// 而 `set_value` 会清掉撤销栈（注释必须可撤销，见 `edit` 模块说明）。
    pub(crate) fn on_toggle_comment(
        &mut self,
        _: &ToggleComment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法注释".to_string()), cx);
            return;
        }
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let prefix = edit::comment_prefix(mode);
        let (text, selection) = {
            let state = self.editor.read(cx);
            (state.value().to_string(), state.selected_range())
        };

        let merged = edit::toggle_line_comment(&text, selection, prefix);
        if merged.text == text {
            // 全是空行：没有可注释的内容（不报错，也不留提示）
            return;
        }

        let new_text = merged.text.clone();
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(merged.replaced.clone(), cx);
            state.replace(merged.block.clone(), window, cx);
            state.set_selected_range(merged.selection.clone(), cx);
        });
        // 程序化写入不保证发 `Change` 事件，这里主动同步服务层（同一值重复写入是幂等的）
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(None, cx);
    }

    /// 把某个文件的内容插到**光标处**（拖放落点；原型 §4.5）。
    ///
    /// 只读文档直接拒绝；读盘失败 / 空文件都给一条消息，不静默。
    fn insert_file_contents(
        &mut self,
        payload: &::shared::InsertFileDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法插入内容".to_string()), cx);
            return;
        }
        let text = match crate::persist::load(&payload.path) {
            Ok(text) => text,
            Err(error) => {
                self.set_message(Some(format!("插入失败：{error}")), cx);
                return;
            }
        };
        if text.is_empty() {
            self.set_message(
                Some(format!("{} 是空文件，没有可插入的内容", payload.label)),
                cx,
            );
            return;
        }
        // 在光标处插入：`replace` 替换当前选区，并把光标留在插入内容之后。
        self.editor
            .update(cx, |state, cx| state.replace(text, window, cx));
        // 程序化写入不保证发 `Change` 事件 → 主动同步服务层（与行注释同一口径，幂等）。
        let new_text = self.editor.read(cx).value().to_string();
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(Some(format!("已插入 {}", payload.label)), cx);
    }

    // ===== 模式切换（A5 的矩阵 + A9 的确认对话框）=====
    //
    // 入口是工具栏最左的**模式指示器**（原型 §2.2）：点击选目标模式，代价由 `mode::plan_switch`
    // 判定，需确认的先弹对话框；确认后本面板落地（写内容 + 改模式 + 刷新着色/只读）。
    // **禁止静默切换**：任何需要付出代价的切换都不过状态（原型 §1.3）。

    /// 请求切换到目标模式（工具栏模式指示器调用）
    pub(crate) fn request_mode_switch(
        &mut self,
        to: EditorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        if from == to {
            return;
        }

        let plan = self.switch_plan(from, to, CellGranularity::default(), cx);
        let Some(kind) = plan.confirm else {
            // 免确认（文本 → SQL）：直接切
            self.apply_mode_switch(plan, window, cx);
            return;
        };

        let entity = cx.entity();
        dialogs::open_switch_confirm(
            window,
            cx,
            kind,
            from,
            to,
            plan.note,
            move |granularity, window, cx| {
                entity.update(cx, |panel, cx| {
                    panel.confirm_mode_switch(to, granularity, window, cx);
                });
            },
        );
    }

    /// 确认后的落地（对话框回调调用）
    ///
    /// “从哪个模式切过来”在这里**重新读**而不是用弹窗前的快照：粒度是确认时才拿到的输入，
    /// 计划必须带着它重算（拿弹窗前那份就等于丢掉用户的选择）。
    pub fn confirm_mode_switch(
        &mut self,
        to: EditorMode,
        granularity: CellGranularity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let plan = self.switch_plan(from, to, granularity, cx);
        self.apply_mode_switch(plan, window, cx);
    }

    /// 算一份切换计划（读取当前文本与“有没有结果”，交给纯函数判定）
    fn switch_plan(
        &self,
        from: EditorMode,
        to: EditorMode,
        granularity: CellGranularity,
        cx: &App,
    ) -> mode::SwitchPlan {
        mode::plan_switch(
            from,
            to,
            &self.editor_text(cx),
            self.result_toolbar.is_some(),
            granularity,
        )
    }

    /// 落地一次模式切换（**已确认**）：内容变换 → 模式变更 → 视图刷新
    ///
    /// 内容先写回 `EditorService`，再让面板从文档重载（`reload_from_document`）——
    /// 内容只有一条真值路径，不直接把变换结果塞进内核（否则服务层与内核就分家了）。
    fn apply_mode_switch(
        &mut self,
        plan: mode::SwitchPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rewritten = match plan.content {
            mode::SwitchContent::Unchanged => None,
            mode::SwitchContent::Sql(text) => Some(text),
            mode::SwitchContent::Cells(cells) => Some(mode::cells_to_text(&cells)),
        };
        if let Some(text) = rewritten {
            let statements = count_statements(&text);
            self.shared
                .update(|service| service.set_content(&self.document, text));
            self.statements = statements;
        }
        self.shared
            .update(|service| service.set_mode(&self.document, plan.to));
        self.sync_mode(window, cx);
        self.reload_from_document(window, cx);
        self.set_message(plan.note.map(str::to_string), cx);
    }

    // ===== 执行（A14）=====
    //
    // 四个入口（Ctrl+Enter / Ctrl+Shift+Enter / 工具栏主按钮 / 执行族菜单）共用 `execute`：
    // 目标解析 → 提交 → 轮询回填。界面不等 I/O。

    /// `Ctrl+Enter`：执行（选区优先 → 光标所在语句）
    pub(crate) fn on_execute_sql(
        &mut self,
        _: &ExecuteSql,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.execute_preferring_selection(cx);
    }

    /// 【B14】结果网格里的 `Ctrl+C`（动作入口；键位在 `crates/app` 注册）
    pub(crate) fn on_copy_grid_selection(
        &mut self,
        _: &CopyGridSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_grid_selection(cx);
    }

    /// 执行（选区优先 → 当前语句）：快捷键与工具栏主按钮共用同一条路
    pub(crate) fn execute_preferring_selection(&mut self, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::resolve_target(&text, selection);
        self.execute(target, ResultPlacement::Replace, cx);
    }

    /// `Ctrl+Shift+Enter`：执行全部
    pub(crate) fn on_execute_all(
        &mut self,
        _: &ExecuteAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_all(cx);
    }

    /// 执行整篇（**宿主发起的唯一入口**：导航「查看数据」打开文档后自动跑一次，B11）
    ///
    /// 与 `Ctrl+Shift+Enter` 同一条路：目标解析 → 提交 → 轮询回填。
    pub fn run_all(&mut self, cx: &mut Context<Self>) {
        let (text, _selection) = self.editor_snapshot(cx);
        let target = execution::all_target(&text);
        self.execute(target, ResultPlacement::Replace, cx);
    }

    /// 提交一次执行
    ///
    /// `placement` 决定结果落到当前结果集还是新结果集（B2 的「在新结果标签中执行」/「批量执行」）；
    /// 自动提交关闭时（B4）本次执行先开一个事务。
    /// 拒绝都要留痕迹：文本模式（能力表禁止通信）、未接入执行、忙、空目标。
    ///
    /// 返回**是否真的提交了**：导出那条路要先记在途状态、再提交取段，
    /// 提交没成（忙 / 未接入）就得把在途状态撒回去（否则状态会残留到下一次）。
    pub(crate) fn execute(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        cx: &mut Context<Self>,
    ) -> bool {
        self.execute_labeled(target, placement, None, cx)
    }

    /// 同 [`Self::execute`]，但给这一批回填的结果贴一个**自定义标题**（今天只有执行计划用）
    ///
    /// 标题是**按位**记的（`pending_labels`）：回填是一条语句一个结论，
    /// “这份是计划、那份是数据”不能靠一个布尔量猜。
    pub(crate) fn execute_labeled(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return false;
        }
        // 【B13】通道门控与写拒绝都在**提交之前**：注定被拒的语句不该跑一半（也不该让
        // “本地副本只读”这种事等驱动报一个谁也不懂的错）
        if let Err(reason) = self.channel_ready().and_then(|()| self.channel_write_check(&target))
        {
            self.set_message(Some(reason), cx);
            return false;
        }
        // 项目只读（标题栏的锁）：写语句同样在**提交之前**拒（旧 SQL 面板有这道闸，B12 遗失）
        if let Err(reason) = self.project_write_check(&target) {
            self.set_message(Some(reason), cx);
            return false;
        }
        let channel = self.channel();
        // 预期回填条数 = 本次要跑的语句数（批量 > 1）：跑完这几条才算“不执行中”
        let expected = target.statements().len();
        // B4：自动提交关掉时，本次执行进事务（引擎在没有事务时会先自动开一个）；
        // B13：加速 / 联邦没有事务语义，那两档上不走事务
        let options = execution::RunOptions {
            use_transaction: !self.autocommit && channel.allows_transactions(),
        };
        match self
            .shared
            .submit(self.document.clone(), &target, placement, options)
        {
            Ok(()) => {
                if self.pending == 0 {
                    // 批量期间一直计同一轮的时间：后一句不该把计时清零
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += expected;
                self.pending_labels
                    .extend((0..expected).map(|_| title.clone()));
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
                true
            }
            Err(error) => {
                self.set_message(Some(error.message().to_string()), cx);
                false
            }
        }
    }

    /// 【B10】执行计划（原型 §5.1：`EXPLAIN` 当前语句 / 选区，**在当前通道上执行**）
    ///
    /// - 前缀**按方言生成**（SQLite 是 `EXPLAIN QUERY PLAN`；SQL Server / Oracle 要先改会话开关
    ///   或再查计划视图，如实说不支持）；
    /// - 方言取**当前通道**的：源库档用源库的，加速 / 联邦档用 DuckDB 的——那儿才是真要跑的引擎；
    /// - 结果落**新结果集**并贴「执行计划」标题（不抢用户正在看的那份）。
    pub(crate) fn explain_current(&mut self, cx: &mut Context<Self>) {
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return;
        }
        let (text, selection) = self.editor_snapshot(cx);
        let sql = match execution::resolve_target(&text, selection) {
            ExecTarget::Selection(sql) | ExecTarget::Statement(sql) => sql,
            // `resolve_target` 只会给这三种；空文档 / 只有注释的文档没什么可解释的
            _ => {
                self.set_message(Some("没有可生成执行计划的语句".to_string()), cx);
                return;
            }
        };
        let dialect = if self.channel().runs_locally() {
            // 加速 / 联邦：语句（连带计划）都在 DuckDB 上跑，计划就该看 DuckDB 的
            engine::sql::SqlDialect::Duckdb
        } else {
            match self.source_dialect() {
                Ok(dialect) => dialect,
                Err(reason) => {
                    self.set_message(Some(reason), cx);
                    return;
                }
            }
        };
        let Some(explain) = engine::sql::explain_sql(dialect, &sql) else {
            self.set_message(
                Some(format!(
                    "{} 暂不支持生成执行计划（要先改会话开关 / 再查计划视图）",
                    translate::label_of(dialect)
                )),
                cx,
            );
            return;
        };
        if self.execute_labeled(
            ExecTarget::Statement(explain),
            ResultPlacement::NewSet,
            Some("执行计划".to_string()),
            cx,
        ) {
            self.set_message(Some("已提交执行计划（结果落新的结果集）".to_string()), cx);
        }
    }

    /// 【B4】请一个事务动作（开始 / 提交 / 回滚）：状态栏 TX 区的菜单调它
    pub(crate) fn tx_action(&mut self, action: execution::TxAction, cx: &mut Context<Self>) {
        match self
            .shared
            .request_transaction(self.document.clone(), action)
        {
            Ok(()) => {
                self.tx_pending += 1;
                self.set_message(Some(format!("{}…", action.label())), cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【B4】切换自动提交（下一次执行生效）
    pub(crate) fn toggle_autocommit(&mut self, cx: &mut Context<Self>) {
        self.autocommit = !self.autocommit;
        let text = if self.autocommit {
            "自动提交：开（每次执行自成一体）".to_string()
        } else {
            "自动提交：关（执行后停在事务里，待提交/回滚）".to_string()
        };
        self.set_message(Some(text), cx);
    }

    /// 【B4】用一次快照刷新 TX 区（时长从**首次看到**事务开始算）
    fn apply_tx_snapshot(&mut self, snapshot: execution::TxSnapshot) {
        self.tx_open = snapshot.in_transaction;
        match (snapshot.in_transaction, self.tx_since) {
            (true, None) => self.tx_since = Some(std::time::Instant::now()),
            (false, _) => self.tx_since = None,
            _ => {}
        }
    }

    /// TX 区文案（纯文本；按钮另画）：“TX 未开启” / “TX 已开启 3.4s”
    fn tx_text(&self) -> String {
        match (self.tx_open, self.tx_since) {
            (true, Some(since)) => format!(
                "TX 已开启 {}",
                status_bar::elapsed_text(since.elapsed())
            ),
            (true, None) => "TX 已开启".to_string(),
            (false, _) => "TX 未开启".to_string(),
        }
    }

    /// 中断当前执行（B3；原型 §5.1：入口在**状态栏 ■**）
    ///
    /// 同步只回绝“没在跑”；真正的中断在工作线程上做（端口实现允许阻塞）。
    /// 中断之后的剩余语句由通道标“已取消”（不是接着往下跑）。
    pub(crate) fn interrupt(&mut self, cx: &mut Context<Self>) {
        match self.shared.cancel() {
            Ok(()) => self.set_message(Some("已请求中断…".to_string()), cx),
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 本文档现在能不能中断（状态栏据此决定要不要画■按钮）
    fn can_interrupt(&self) -> bool {
        self.pending > 0
    }

    /// 状态栏那行“执行中 3.4s…”读的耗时（供测试断言“跑完就不再计时”）
    pub fn elapsed_for_test(&self) -> Option<std::time::Duration> {
        self.running_since.map(|since| since.elapsed())
    }

    /// 【B4】TX 区文案（供测试断言）
    pub fn tx_text_for_test(&self) -> String {
        self.tx_text()
    }

    /// 【B4】自动提交开关的当前值
    pub fn autocommit_for_test(&self) -> bool {
        self.autocommit
    }

    /// 【B4】还没回执的事务动作数
    pub fn tx_pending_for_test(&self) -> usize {
        self.tx_pending
    }

    /// 当前模式是否允许执行（**读能力表**，不在视图里另写一份模式判断）
    ///
    /// 文本模式恒为 `false`（“不与数据库通信”是能力表里的硬约束）。
    fn execution_allowed(&self) -> bool {
        self.with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false)
    }

    /// 本文档的文件档位提示（>50MB / ≥200MB；`None` = 不显示提示卡）
    pub fn tier_notice(&self) -> Option<&'static str> {
        self.with_document(|doc| doc.tier_notice()).flatten()
    }

    /// 【B6】编辑内核里的诊断条数（错误回填画了没有）
    pub fn diagnostics_for_test(&self, cx: &App) -> usize {
        self.editor
            .read(cx)
            .diagnostics()
            .map(|set| set.len())
            .unwrap_or(0)
    }

    /// 结果工具栏（⑥）的左段文案（供测试断言；`None` = 还没有结论）
    ///
    /// 就是 `ResultToolbar::segments()` 拼起来的那串，原型 §2.4 的 `行数 1,204 │ 耗时 1.2s │ orders`。
    pub fn result_summary_for_test(&self) -> Option<String> {
        self.result_toolbar
            .as_ref()
            .map(|toolbar| toolbar.segments().join(" · "))
    }

    /// 结果工具栏（⑥）的真实输入（供测试断言各段与显隐）
    pub fn result_toolbar_for_test(&self) -> Option<ResultToolbar> {
        self.result_toolbar.clone()
    }

    /// 结果状态行（⑦）的真实输入（供测试断言总行数 / 截断提示）
    pub fn result_status_for_test(&self) -> Option<ResultStatus> {
        self.result_status.clone()
    }

    /// 当前选中结果集的可重跑 SQL / 可复制（供测试断言按钮显隐）
    pub fn result_actions_for_test(&self) -> (Option<String>, bool) {
        (self.result_sql.clone(), self.result_can_copy)
    }

    /// 【B9 切片二】补全弹层状态（供测试断言“真的弹了”、查询词与候选）
    ///
    /// 读的是内核的 `completion_menu_state()`（`#[doc(hidden)] pub`）——**不是我们的副本**：
    /// 断言的就是内核眼里“弹层开着”这件事。
    pub fn completion_menu_for_test(&self, cx: &App) -> (bool, String, Vec<String>) {
        let state = self.editor.read(cx);
        let menu = state.completion_menu_state();
        (
            menu.open,
            menu.query.clone(),
            menu.items.iter().map(|item| item.label.clone()).collect(),
        )
    }

    /// 【B6】错误卡片的内容（供测试断言摘要与定位文案）
    pub fn result_error_card_for_test(&self) -> Option<ErrorCard> {
        self.result_error_card.clone()
    }

    /// 【B5】选中网格里的某一行（供测试断言状态行⑦的“已选第 N 行”；真机上点行就是这条路）
    pub fn select_grid_row_for_test(&mut self, row: usize, cx: &mut Context<Self>) {
        self.grid
            .update(cx, |state, cx| state.set_selected_row(row, cx));
    }

    /// 结果集标签（供测试断言“批量跑出三个结果集”与标签文案）
    pub fn result_tabs_for_test(&self) -> Vec<(String, bool)> {
        self.result_tabs
            .iter()
            .map(|tab| (tab.label.clone(), tab.failed))
            .collect()
    }

    /// 【B13】结果集标签的通道徽标（供测试断言“每份结果记住了自己是哪档跑的”）
    pub fn result_badges_for_test(&self) -> Vec<String> {
        self.result_tabs.iter().map(|tab| tab.badge()).collect()
    }

    /// 【B13】结果区顶部那一行提示（切通道后旧结果那一条）
    pub fn result_notice_for_test(&self) -> Option<String> {
        self.result_notice.clone()
    }

    /// 当前选中的结果集下标（供测试断言点标签真的切了）
    pub fn result_active_for_test(&self) -> usize {
        self.result_active
    }

    /// 还有几条语句没回填（供测试断言“批量执行至少知道自己在跑什么”）
    pub fn pending_for_test(&self) -> usize {
        self.pending
    }

    /// 结果网格当前行数（供测试断言网格真的拿到了数据）
    pub fn grid_row_count_for_test(&self, cx: &App) -> usize {
        // 【B15】看的是**视图行数**：筛选生效时网格里就是命中那些行
        self.grid.read(cx).delegate().visible_row_count()
    }

    /// 网格实体（测试用：滚动到底自动加载得真在表格上调 `load_more` 才算验到链路）
    #[cfg(test)]
    pub(crate) fn grid_for_test(&self) -> Entity<TableState<ResultGridDelegate>> {
        self.grid.clone()
    }

    /// 把光标放到指定位移（供测试断言“执行的是光标所在那句”；键位路径仍走真按键）
    pub fn set_caret_for_test(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
    }

    /// 设置选区（供测试断言“有选区只格式化选区”；键位路径仍走真按键）
    pub fn set_selection_for_test(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(range, cx));
    }

    /// 启动结果轮询（已有存活任务时不重复启动）
    ///
    /// 与 `workbench` 的导航任务同一模式：**后台等、主线程回填**，空闲即退出。
    /// 执行期间顺带 `cx.notify()`：状态栏的耗时累加跟着这个节拍走（不另起一个定时器）。
    fn ensure_exec_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.exec_pump.borrow().as_ref()
            && !task.is_ready()
        {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                // 还有执行在跑（含别的文档）就继续等；一个都没有了就退出，下次提交重新起
                let keep_going = weak
                    .update(cx, |this, cx| {
                        this.drain_exec_results(cx);
                        if this.pending > 0 || this.tx_pending > 0 || this.refresh_pending > 0 {
                            cx.notify();
                        }
                        // 事务动作 / 重新挂载 / DuckDB 导出的回执还没到也要继续轮询（它们不在“执行忙”里）
                        this.shared.is_executing()
                            || this.tx_pending > 0
                            || this.refresh_pending > 0
                            || this.export_pending > 0
                    })
                    .unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        });
        *self.exec_pump.borrow_mut() = Some(task);
    }

    /// 取回已完成的执行，填入结果存储与网格
    ///
    /// **一条语句一条结论**：批量执行会连续回来多条，每条各自落一个结果集（落位由 outcome
    /// 自带）。本文档的回填计数减到 0 才算执行完。
    pub(crate) fn drain_exec_results(&mut self, cx: &mut Context<Self>) {
        // 中断尝试的结果先收（失败 / 没在跑 都要留痕，不能默默把按钮点一下就算完）
        if let Some(note) = self.shared.drain_cancel_notes().into_iter().last() {
            self.set_message(Some(note), cx);
        }

        // B4：事务动作的结论（成功/失败都要让界面看见；成功时顺便刷新 TX 区）
        for note in self.shared.drain_tx_notes() {
            self.tx_pending = self.tx_pending.saturating_sub(1);
            if note.document != self.document {
                continue;
            }
            match note.result {
                Ok(snapshot) => {
                    self.apply_tx_snapshot(snapshot);
                    self.set_message(Some(format!("已{}", note.action.label())), cx);
                }
                Err(reason) => self.set_message(
                    Some(format!("{}失败：{reason}", note.action.label())),
                    cx,
                ),
            }
        }

        // 【B13/T1.6】源动作的回执（成败都要说出来：它是用户按的一个动作）
        for note in self.shared.drain_source_notes() {
            self.refresh_pending = self.refresh_pending.saturating_sub(1);
            if note.document != self.document {
                continue;
            }
            let label = note.action.label();
            match note.result {
                Ok(done) => self.set_message(Some(done), cx),
                Err(reason) => self.set_message(Some(format!("{label}失败：{reason}")), cx),
            }
        }

        // 【B7 切片二】DuckDB 导出（Parquet / XLSX）的回执：文案与同步那条同口径
        for note in self.shared.drain_export_notes() {
            self.export_pending = self.export_pending.saturating_sub(1);
            if note.document != self.document {
                continue;
            }
            match note.result {
                Ok(rows) => {
                    // “（已筛选）”照 B7 的口径明示（导的是筛后的行集）
                    let filtered = if note.filtered { "（已筛选）" } else { "" };
                    self.set_message(
                        Some(format!(
                            "已导出 {rows} 行{filtered}（{}）→ {}",
                            note.format.label(),
                            note.path.display()
                        )),
                        cx,
                    )
                }
                Err(reason) => self.set_message(Some(format!("导出失败：{reason}")), cx),
            }
        }

        let outcomes = self.shared.drain_exec();
        let mut mine_arrived = 0usize;
        // 【B6】这一轮新到的失败是哪条语句（回填之后要是有位置就跳过去）
        let mut fresh_failure: Option<String> = None;
        // 【B5b】取下一段失败的原因（要等结果区同步完再说，否则会被“选中那份没有错”冲掉）
        let mut segment_failure: Option<String> = None;
        // 【B14】执行自带的一句说明（如“已去掉原查询的 LIMIT”）——同样要等同步完再说
        let mut fresh_notice: Option<String> = None;
        // 【B14】落在**新结果集**上的失败：它不抢选中（B2 的语义），所以结果区不会报它——
        // 状态栏补一句，否则用户只看到标签上多个红点，不知道为什么
        let mut background_failure: Option<String> = None;
        for outcome in outcomes {
            let is_mine = outcome.document == self.document;
            if is_mine {
                mine_arrived += 1;
                // B4：事务状态跟着结论回来（用户手敲 BEGIN / COMMIT 也走同一路径）
                self.apply_tx_snapshot(outcome.transaction);
                // 【B14】提示从**结论**里取（它随这次执行回来，不是猜的）
                if let Ok(data) = outcome.result.as_ref()
                    && let Some(notice) = data.notice.clone()
                {
                    fresh_notice = Some(notice);
                }
            }
            let placement = outcome.placement;
            // 【B10】在途标题按位弹出（只有本文档的回填才算数；别的文档那份不能把队列吃了）
            let title = if is_mine {
                self.pending_labels.pop_front().flatten()
            } else {
                None
            };
            let mut entry = entry_from(outcome);
            if let Some(title) = title {
                entry = entry.with_title(title);
            }
            if is_mine && entry.failed() {
                // 【B5b】取下一段失败：**已经抓到的行是用户的成果**，不能因为再抓失败就清掉。
                // 所以这次失败不入存储（选中那份原样不动），只把原因说出来。
                if placement == ResultPlacement::Append {
                    segment_failure = Some(format!(
                        "取下一段失败：{}",
                        entry.error.clone().unwrap_or_default()
                    ));
                    continue;
                }
                fresh_failure = Some(entry.sql.clone());
                if placement == ResultPlacement::NewSet {
                    background_failure = Some(format!(
                        "新结果集执行失败：{}",
                        entry.error.clone().unwrap_or_default()
                    ));
                }
            }
            self.shared.update_results(|store| store.push(entry, placement));
        }
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
                // 【B5b】本文档这一轮执行全部结束：取段也结束了（滚动到底可以再次触发）
                self.fetching_more = false;
            }
            self.sync_result_view(cx);
            // 【B5b】取段失败的原因在这里说（同步结果区会按“选中那份”重写提示）
            let segment_reason = segment_failure.is_some();
            if let Some(reason) = segment_failure {
                self.set_message(Some(reason), cx);
            }
            // 【B14】执行自带的说明（成功且没有失败原因时才说，别把失败提示盖掉）
            if let Some(notice) = fresh_notice
                && self.message.is_none()
            {
                self.set_message(Some(notice), cx);
            }
            // 【B14】没被选中的那份失败了：至少让状态栏说一句
            if let Some(reason) = background_failure
                && self.message.is_none()
            {
                self.set_message(Some(reason), cx);
            }
            // 【B7】导出在跑就推进它（还要抓就再提交一段；抓完了/崩了就落盘或作废）
            self.advance_pending_export(segment_reason, cx);
            // 【B6】失败且能定位：把光标送到出错处并聚焦（拿不到位置就只留原因）
            if let Some(sql) = fresh_failure
                && self.result_sql.as_deref() == Some(sql.as_str())
                && self.error_site.is_some()
            {
                self.jump_to_error_site(cx);
            }
        }
    }

    /// 按「当前选中的结果集」刷新结果区（网格数据 + 状态行 + 标签条）
    ///
    /// **结果区唯一的读点**：结果集可能因一次执行回填、点标签、批量推进而变，但界面只从
    /// 这里读一次——否则会出现“网格还是上一份、摘要已经换了”的错位（多结果集之后这种错位
    /// 很难靠看界面发现）。
    fn sync_result_view(&mut self, cx: &mut Context<Self>) {
        // 先把要用的数据从权威存储里拷出来（不把 `Ref` 带进下面的 `grid.update`）
        let current_channel = self.channel();
        let (
            tabs,
            active,
            grid_data,
            empty,
            toolbar,
            status,
            failure,
            extra,
            extra_analysis,
            notice,
            insight_available,
        ) = {
            let store = self.shared.results();
            let active = store.active_index(&self.document).unwrap_or(0);
            let entry = store.active(&self.document);
            let connection = entry.and_then(|entry| {
                entry
                    .connection
                    .as_deref()
                    .map(|id| self.shared.connection_status_text(Some(id)))
            });
            (
                result_sets::tabs(store.sets(&self.document), current_channel),
                active,
                entry
                    .filter(|entry| entry.has_grid())
                    .map(|entry| (entry.columns.clone(), entry.rows.clone())),
                entry
                    .filter(|entry| !entry.has_grid())
                    .map(empty_text)
                    .unwrap_or_default(),
                // 【B5】工具栏（⑥）：行数 / 影响行数 / 失败 / 耗时 / 连接名
                entry.map(|entry| ResultToolbar {
                    rows: entry.has_grid().then(|| entry.row_count()),
                    affected_rows: entry.affected_rows,
                    failed: entry.failed(),
                    elapsed_ms: Some(entry.elapsed_ms),
                    connection: connection.clone(),
                    // 【B15】来源摘要：有自定义标题的（执行计划 / 分析）用标题，其余用血缘
                    lineage: entry
                        .title
                        .clone()
                        .or_else(|| entry.lineage.clone()),
                    // 【B15】筛选统计随后由 `refresh_filter_hint` 从网格真值填上
                    filtered: None,
                }),
                // 【B5】状态行（⑦）：只有网格才给（写语句的“共 0 行”、失败时的“共 0 行”都是噪音）
                entry.filter(|entry| entry.has_grid()).map(|entry| ResultStatus {
                    total_rows: entry.row_count(),
                    // 选中行在渲染时现读（点行不改结果集，没必要每帧回写状态）
                    selected_row: None,
                    truncated_hint: entry
                        .truncated
                        .then(|| result_grid::truncated_hint(entry.row_count())),
                    has_more: entry.can_fetch_more(),
                }),
                // 【B6】失败才谈得上定位：把「哪条 SQL + 什么错误」一起带出去
                entry.and_then(|entry| {
                    entry
                        .error
                        .as_ref()
                        .map(|error| (entry.sql.clone(), error.clone()))
                }),
                entry.map(|entry| (entry.sql.clone(), entry.has_grid())),
                // 【B15】这份是不是本地分析的产物（工具栏据此不摆“刷新”）
                entry.is_some_and(|entry| entry.analysis),
                // 【B13】选中这份来自别的通道 → 顶部一行说清“旧结果来自 X”（原型 §5.7 规则 2）；
                // 同一档就不提示（切换本身已经写在工具栏与状态栏上了）
                entry
                    .filter(|entry| entry.channel != current_channel)
                    .map(|entry| channel::stale_notice(entry.channel, current_channel)),
                // 【M8】「洞察此列」能不能给：条目本身（成功 / 有列 / 只读）+ 连接解析得出来
                entry.is_some_and(|entry| self.insight_available(entry)),
            )
        };

        self.result_tabs = tabs;
        self.result_active = active;
        self.result_notice = notice;
        self.result_toolbar = toolbar;
        // 【B5b】滚动到底自动加载的两个真值要先取出来（`status` 接着就被移动了）
        let has_more = status.as_ref().is_some_and(|status| status.has_more);
        let loading_more = self.fetching_more;
        self.result_status = status;
        // 【B15】筛选统计的真值在网格那边（视图行序是它算的）
        self.refresh_filter_hint(cx);
        let (sql, can_copy) = extra.unwrap_or_default();
        self.result_sql = Some(sql).filter(|sql| !sql.trim().is_empty());
        self.result_can_copy = can_copy;
        self.result_is_analysis = extra_analysis;
        self.grid.update(cx, |state, cx| {
            match grid_data {
                Some((columns, rows)) => state.delegate_mut().set_data(columns, rows),
                None => state.delegate_mut().clear(empty),
            }
            // 【M8】这份结果能不能洞察（菜单项据此出现 / 消失）
            state.delegate_mut().set_insight_available(insight_available);
            // 【B5b】这份结果还能不能再取一段 / 是不是正在取
            state.delegate_mut().set_has_more(has_more);
            state.delegate_mut().set_loading_more(loading_more);
            state.refresh(cx);
        });

        // 【B6】定位：结论没变就不重算（要拿整篇文档做匹配，而这里会被反复调用）
        let site = match &failure {
            Some((sql, error)) => {
                let key = (sql.clone(), error.clone());
                if self.error_site_key.as_ref() == Some(&key) {
                    self.error_site.clone()
                } else {
                    let document = self.editor_text(cx);
                    let site = diagnostics::site_in_document(&document, sql, error);
                    self.error_site_key = Some(key);
                    site
                }
            }
            None => {
                self.error_site_key = None;
                None
            }
        };
        self.error_site = site;
        let reason = failure.map(|(_, error)| error);
        // 【B6】错误卡片的两个真值：驱动原话 + “定位到第 N 行”（认不出位置就不给后者）
        self.result_error_card = reason.as_ref().map(|error| ErrorCard {
            message: error.clone(),
            location: self.error_site.as_ref().map(|site| site.location_text()),
        });
        self.apply_error_marks(reason.clone(), cx);

        // 失败原因同时进状态栏（结果区可能被滚出视野）；选中成功的那份则清掉旧提示。
        // 【B6】能定位的失败把位置一并说出来（“第 3 行 第 15 列”——用户能直接去那儿看）
        let message = reason.map(|error| match &self.error_site {
            Some(site) => format!("{error}（{}）", site.location_text()),
            None => error,
        });
        self.set_message(message, cx);
    }

    /// 【B6】把出错范围画进编辑内核（行内高亮 + 悬停弹层；没有位置就只清旧的）
    fn apply_error_marks(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        let site = self.error_site.clone();
        self.editor.update(cx, |state, cx| {
            let Some(diagnostics) = state.diagnostics_mut() else {
                return;
            };
            diagnostics.clear();
            if let (Some(site), Some(reason)) = (site, reason) {
                let (end_line, end_column) = site.end();
                let range = Position::new(site.line.saturating_sub(1) as u32, site.column.saturating_sub(1) as u32)
                    ..Position::new(end_line.saturating_sub(1) as u32, end_column.saturating_sub(1) as u32);
                diagnostics.push(
                    Diagnostic::new(range, reason)
                        .with_severity(DiagnosticSeverity::Error)
                        .with_source("执行"),
                );
            }
            cx.notify();
        });
    }

    /// 【B6】把光标送到出错处并聚焦
    ///
    /// 回填发生在没有窗口的轮询里，所以借构造时存下的窗口句柄。窗口已经没了、或者这次更新
    /// 落在另一个窗口更新里面（headless 测试就是这样）时，这一步只是没做成——不 panic：
    /// 位置已经画在诊断与状态栏里了。
    fn jump_to_error_site(&mut self, cx: &mut Context<Self>) {
        let Some(site) = self.error_site.clone() else {
            return;
        };
        let range = site.range();
        let editor = self.editor.clone();
        let _ = self.window.update(cx, move |_view, window, app| {
            editor.update(app, |state, cx| {
                state.set_selected_range(range, cx);
                state.focus(window, cx);
            });
        });
    }

    /// 跳转的**落地入口**（供测试驱动；真机走上面的窗口句柄路径）
    /// 跳转的**落地入口**：手上有窗口时直接做（错误卡片上那个按钮，以及测试）
    ///
    /// 真机的自动跳转走上面的窗口句柄路径（回填发生在没有窗口的后台轮询里）；这条入口是
    /// 按下去就有窗口的情况，也是测试能驱动的那一半（与对话框“真点击”同一口径，架构 §12 #29）。
    pub(crate) fn jump_to_error_site_with(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.error_site.clone() else {
            return;
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(site.range(), cx);
            state.focus(window, cx);
        });
    }

    /// 【B5】复制当前结果集（TSV）
    ///
    /// 复制的是**已抓到的行**（被截断的那份就只有那些行）——不假装能拿到没抓的部分。
    pub(crate) fn copy_active_result(&mut self, cx: &mut Context<Self>) {
        let text = self
            .shared
            .results()
            .active(&self.document)
            .filter(|entry| entry.has_grid())
            .map(ResultEntry::to_tsv)
            .filter(|text| !text.is_empty());
        let Some(text) = text else {
            self.set_message(Some("没有可复制的结果集".to_string()), cx);
            return;
        };
        let rows = self
            .shared
            .results()
            .active(&self.document)
            .map(ResultEntry::row_count)
            .unwrap_or(0);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.set_message(Some(format!("已复制 {rows} 行（TSV）")), cx);
    }

    /// 【B14】`Ctrl+C`（网格里）：把选中的**一格**或**一整行**拷到剪贴板
    ///
    /// 与右键菜单同一套口径：一格给**原文**（要的就是那一格的值），一整行拼 TSV。
    /// 选择自身是组件的真值，面板只记“最近一次选的是格还是行”（见 `result_selection`），
    /// 文本由 delegate 按**视图行序**拼（筛选 / 排序之后看着的那一行）。
    ///
    /// 设了键位就不能静默失败：没选中 / 选中的行已经不在视图里时给一句可读的提示。
    pub(crate) fn copy_grid_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.result_selection;
        let text =
            selection.and_then(|selection| self.grid.read(cx).delegate().selection_text(selection));
        let Some(text) = text else {
            self.set_message(Some("先在结果网格里选一格或一行再复制".to_string()), cx);
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let message = match selection {
            Some(result_grid::GridSelection::Row { .. }) => "已复制整行（TSV）".to_string(),
            _ => "已复制单元格".to_string(),
        };
        self.set_message(Some(message), cx);
    }

    /// 【B5】重跑当前结果集的 SQL（结果**换掉选中那份**，不是新开一份——原位刷新）
    pub(crate) fn refresh_active_result(&mut self, cx: &mut Context<Self>) {
        let Some(sql) = self.result_sql.clone() else {
            self.set_message(Some("这份结果没有可重跑的 SQL".to_string()), cx);
            return;
        };
        self.execute(ExecTarget::Statement(sql), ResultPlacement::Replace, cx);
    }

    /// 【B5b】取下一段（状态行⑦上的那个按钮）
    ///
    /// 提交一次 `Segment` 目标（落位 `Append`）：不动用户在看的那份的位置，新抓到的行
    /// **接在后面**。忙 / 没结果 / 没有下一段都回绝得可读（不静默）。
    /// 与滚动到底自动加载走的是**同一条路**（那个只负责把意图送到这里）。
    pub(crate) fn fetch_more(&mut self, cx: &mut Context<Self>) {
        let Some(sql) = self.result_sql.clone() else {
            self.set_message(Some("这份结果没有可重取的 SQL".to_string()), cx);
            return;
        };
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::can_fetch_more)
        else {
            self.set_message(Some("这份结果已经取完了".to_string()), cx);
            return;
        };
        let offset = entry.row_count();
        let submitted = self.execute(
            ExecTarget::Segment {
                sql,
                offset,
                limit: execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            cx,
        );
        // 提交成功才算“正在取”：回填之前滚动到底不该再触发一次（防重入的真值在这边）
        if submitted {
            self.mark_fetching_more(cx);
        } else {
            // 没提交成功：把 delegate 的乐观置位撤回去，否则滚动到底会一直不再触发
            let grid = self.grid.clone();
            grid.update(cx, |state, _cx| state.delegate_mut().set_loading_more(false));
        }
    }

    /// 【B5b】标记“正在取下一段”
    ///
    /// 除了面板自己的真值，还要**当场**同步给网格的 delegate：提交到回填之间没有
    /// `sync_result_view`（那是一次全量投影，太重），不补这一下的话“滚动到底”会在
    /// 这段窗口里反复触发、每次都撞上“执行中”的回绝。
    fn mark_fetching_more(&mut self, cx: &mut Context<Self>) {
        self.fetching_more = true;
        let grid = self.grid.clone();
        grid.update(cx, |state, _cx| state.delegate_mut().set_loading_more(true));
    }

    /// 【B15】筛选框输入（原型 §5.5：本地即时筛选、300ms 防抖、不重查）
    ///
    /// 防抖靠“版本号 + 定时任务”：连打时任务会被后来者取代，只有最后一次到点——
    /// 否则每敲一个字都要重算整份视图行序（大结果集上能看见卡）。
    pub(crate) fn on_filter_input(&mut self, text: String, cx: &mut Context<Self>) {
        if self.filter_text == text {
            return;
        }
        self.filter_text = text;
        self.filter_version = self.filter_version.wrapping_add(1);
        let version = self.filter_version;
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            executor
                .timer(std::time::Duration::from_millis(300))
                .await;
            _ = weak.update(cx, |panel, cx| {
                if panel.filter_version == version {
                    panel.apply_filter(cx);
                }
            });
        });
        *self.filter_debounce.borrow_mut() = Some(task);
    }

    /// 【B15】把当前筛选词应用到网格（事件路径：重算一次视图行序）
    ///
    /// 【B14】开关开着（且词非空）时，**同一份词也下发一次**：本地筛完再看源库那边——
    /// 两档语义并存（原型 §5.5），下发产生的是一份**新结果集**，原结果不动。
    pub(crate) fn apply_filter(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter_text.clone();
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(filter);
            state.refresh(cx);
        });
        self.refresh_filter_hint(cx);
        cx.notify();
        if self.pushdown && !self.filter_text.trim().is_empty() {
            self.pushdown_filter(cx);
        }
    }

    /// 【B14】下发源库：把当前筛选词交给执行器拼 `WHERE` 重查（结果落**新结果集**）
    pub(crate) fn pushdown_filter(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可下发的结果集".to_string()), cx);
            return;
        };
        let filter = self.filter_text.trim().to_string();
        if filter.is_empty() {
            self.set_message(Some("先填一个筛选词再下发".to_string()), cx);
            return;
        }
        self.execute(
            ExecTarget::Filtered {
                sql: entry.sql.clone(),
                filter,
                columns: entry.columns.clone(),
            },
            // 原型 §5.5：下发**产生新结果集**，原结果保留
            ResultPlacement::NewSet,
            cx,
        );
    }

    /// 【B14】按值筛选（右键菜单来的）：把值写进筛选框并立刻应用（**本地**那档）
    ///
    /// 输入框要跟着变（用户得看到自己被填了什么）；窗口句柄是构造时存的——
    /// 菜单点击发生在独立事件里，但没有 `window` 参数（headless 里甚至真的没有窗口），
    /// 拿不到就只改真值，筛选照样生效。
    pub(crate) fn apply_filter_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.filter_text = value.to_string();
        // 掐掉在途的防抖：这次是直接定稿，不该再被 300ms 后的旧词覆盖
        self.filter_version = self.filter_version.wrapping_add(1);
        let input = self.filter_input.clone();
        let text = value.to_string();
        let _ = self.window.update(cx, |_panel, window, app| {
            input.update(app, |state, cx| state.set_value(text, window, cx));
        });
        self.apply_filter(cx);
    }

    /// 【B14】排序下发（右键菜单来的）：按这一列重查源库，结果落**新结果集**
    pub(crate) fn sort_down(&mut self, column: &str, descending: bool, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可下发的结果集".to_string()), cx);
            return;
        };
        self.execute(
            ExecTarget::SortedDown {
                sql: entry.sql.clone(),
                column: column.to_string(),
                descending,
            },
            // 与筛选下发同口径：**产生新结果集**，原结果保留
            ResultPlacement::NewSet,
            cx,
        );
    }

    /// 【M8】这份结果**实际**跑在哪条连接上：绑定优先，未绑定回退**当前活动连接**
    ///
    /// 口径与执行完全一致（`QueryRunner::active_connection` ⇒ 与 `resolve_conn_id` 同一处
    /// 语义）：否则会出现「执行走了 A、洞察取样走了 B」这种最难查的偏差。
    fn effective_connection(&self, entry: &ResultEntry) -> Option<String> {
        entry
            .connection
            .clone()
            .or_else(|| self.shared.active_connection())
    }

    /// 【M8】这份结果能不能给「洞察此列」入口（菜单项据此出现 / 消失）
    fn insight_available(&self, entry: &ResultEntry) -> bool {
        entry.can_insight_column() && self.effective_connection(entry).is_some()
    }

    /// 【M8】「洞察此列」（右键菜单来的）：把**列名 + 产生它的 SQL + 连接**交给宿主端口
    ///
    /// 不物化结果集（决策在案）：洞察侧拿这段 SQL 重跑一句带 `LIMIT` 的取样。
    /// 拿不到当前结果 / 解析不出连接 / 宿主没接端口都只是**不动作**——入口本来就不该出现
    /// （`insight_available` 已经挡住了），所以不需要再给用户弹一句话。
    pub(crate) fn insight_column(&mut self, column: &str, cx: &mut Context<Self>) {
        let Some(port) = self.shared.insight_column_port() else {
            return;
        };
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            return;
        };
        let Some(connection) = self.effective_connection(&entry) else {
            return;
        };
        let label = self.result_set_label();
        port(
            InsightColumnRequest {
                column: column.to_string(),
                sql: entry.sql.clone(),
                connection,
                label,
            },
            cx,
        );
    }

    /// 当前结果集在界面上的名字（「洞察此列」的来源标签：面板副标题 / 快照来源用）
    fn result_set_label(&self) -> String {
        let index = self.result_active;
        let titled = self
            .shared
            .results()
            .active(&self.document)
            .and_then(|entry| entry.title.clone());
        match titled {
            Some(title) => title,
            None => format!("结果 {}", index + 1),
        }
    }

    /// 【B14】开关切换：打开时若已有筛选词就立刻下发一次（“打开后把条件拼为 WHERE 重查”）
    pub(crate) fn set_pushdown(&mut self, open: bool, cx: &mut Context<Self>) {
        self.pushdown = open;
        cx.notify();
        if open && !self.filter_text.trim().is_empty() {
            self.pushdown_filter(cx);
        }
    }

    /// 【B15】清除筛选（工具栏那个 ✕）：输入框清空也要走 change，所以直接一起做
    pub(crate) fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_text.clear();
        self.filter_version = self.filter_version.wrapping_add(1);
        self.filter_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.apply_filter(cx);
    }

    /// 【B15】测试用：直接应用筛选词（跳过 300ms 防抖，验“应用之后怎样”）
    #[cfg(test)]
    pub(crate) fn set_filter_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter_text = text.to_string();
        self.apply_filter(cx);
    }

    /// 【B15】把“已筛选 N / M 行”写进结果工具栏（统计跟随筛选——原型 §5.5 的要求）
    fn refresh_filter_hint(&mut self, cx: &mut Context<Self>) {
        let (visible, total, filtering) = {
            let delegate = self.grid.read(cx).delegate();
            (
                delegate.visible_row_count(),
                delegate.data_row_count(),
                !delegate.filter().trim().is_empty(),
            )
        };
        if let Some(toolbar) = self.result_toolbar.as_mut() {
            toolbar.filtered = filtering.then_some((visible, total));
        }
    }

    /// 切换结果集（结果集标签条点击）：选中项只有 `ResultStore` 能改，界面按它重画
    pub(crate) fn select_result_set(&mut self, index: usize, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .update_results(|store| store.select(&self.document, index));
        if changed {
            self.sync_result_view(cx);
        }
    }

    /// 【B15 切片二】自定义分析 SQL：先问一条 SQL，再把选中那份结果的已抓行桥接过去跑
    ///
    /// 与预置菜单同一个落点（[`Self::run_analysis`]），只是 SQL 由用户给。
    /// 说明里的行数取自 `request_for` 的桥接口径（截断与计数只有那处），不在界面侧另算。
    pub(crate) fn request_custom_analysis(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.shared.results_active(&self.document) else {
            self.set_message(Some("当前没有可分析的结果".to_string()), cx);
            return;
        };
        if !entry.has_grid() || entry.columns.is_empty() {
            // 与菜单同一判据：没有网格就没有行可桥接
            self.set_message(Some("这份结果没有行可分析".to_string()), cx);
            return;
        }
        // 用模板先算一遍桥接，只为把“多少行参与”说准（真的那次在 `run_analysis` 里重算）
        let bridged =
            crate::analysis::request_for(&entry, crate::analysis::CUSTOM_DEFAULT_SQL.to_string());
        let entity = cx.entity();
        dialogs::open_analysis_sql(
            window,
            cx,
            crate::analysis::custom_hint(bridged.bridged_rows(), bridged.dropped_rows),
            crate::analysis::CUSTOM_DEFAULT_SQL,
            move |sql, _window, cx| {
                entity.update(cx, |panel, cx| panel.run_analysis(sql, cx));
            },
        );
    }

    /// 【B15】本地分析：对**选中那份结果的已抓行**跑一条聚合 SQL（落新结果集）
    ///
    /// 三条口径：
    /// - 数据是桥接过来的行（把当前结果集已抓到的行建成本地 DuckDB 临时表），**不碰源库**——
    ///   所以不走通道闸 / 项目只读闸（那两道管的是“写源库对象”）；
    /// - 结果落**新结果集**并贴「分析」标题（原结果一行不动）；
    /// - 基于多少行、有没有被上限截掉，由执行器写进结果的说明（`analysis::notice`）。
    pub(crate) fn run_analysis(&mut self, sql: String, cx: &mut Context<Self>) {
        // 空的不提交：提交了也只会拿到一句驱动报错（“没写东西”在本地就能说清）
        if !crate::analysis::is_runnable(&sql) {
            self.set_message(Some("分析 SQL 为空（写一条 SELECT 再运行）".to_string()), cx);
            return;
        }
        let Some(entry) = self.shared.results_active(&self.document) else {
            self.set_message(Some("当前没有可分析的结果".to_string()), cx);
            return;
        };
        let request = crate::analysis::request_for(&entry, sql);
        self.execute_labeled(
            ExecTarget::Analysis(request),
            ResultPlacement::NewSet,
            Some(crate::analysis::ANALYSIS_TITLE.to_string()),
            cx,
        );
    }

    /// 【B7】导出选中结果集（**仅已抓取的行**，不发新查询）
    pub(crate) fn export_active_result(
        &mut self,
        format: ExportFormat,
        cx: &mut Context<Self>,
    ) {
        self.start_export(format, ExportScope::Fetched, cx);
    }

    /// 【B7】抓全量后导出（多轮取段；中途失败 / 换结果集就作废，不落半截文件）
    pub(crate) fn export_active_result_all(
        &mut self,
        format: ExportFormat,
        cx: &mut Context<Self>,
    ) {
        self.start_export(format, ExportScope::All, cx);
    }

    /// 导出的共同开头：选路径（用户取消 = 什么都不做）→ 直接落盘 / 先抓全量
    fn start_export(&mut self, format: ExportFormat, scope: ExportScope, cx: &mut Context<Self>) {
        if !self.shared.has_export_path_picker() {
            self.set_message(Some("导出未接入（宿主未注入路径选择器）".to_string()), cx);
            return;
        }
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可导出的结果集".to_string()), cx);
            return;
        };
        // 抓到一半再发现“执行器忙”更糟（路径都选完了），先问清楚（通道的忙是全局的：别的文档也在跑）
        if self.shared.is_executing() {
            self.set_message(Some("执行中，稍候再导出".to_string()), cx);
            return;
        }
        // 结果集序号（1 基）当默认名的兜底：SQL 里能猜出表名就用表名
        let index = self.result_active + 1;
        let default_name = export::default_file_name(Some(&entry.sql), index, format);
        let Some(path) = self.shared.pick_export_path(format, default_name) else {
            // 用户取消：不是错误，不弹提示（与另存为同口径）
            return;
        };
        // 「抓全量」只在**真还有下一段**时才需要取段；已经抓完了就是普通导出
        if scope == ExportScope::Fetched || !entry.can_fetch_more() {
            self.write_export(&entry, format, index, &path, cx);
            return;
        }
        self.pending_export = Some(PendingExport {
            format,
            path,
            set: self.result_active,
        });
        // 不另外摆一条“正在抓取”提示：状态栏已经在报「执行中 3.4s…」，
        // 而且 `execute` 成功时会清掉提示（设了也会被清）
        let submitted = self.execute(
            ExecTarget::Segment {
                sql: entry.sql.clone(),
                offset: entry.row_count(),
                limit: execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            cx,
        );
        if !submitted {
            // 没提交成功（通道拒了）：在途状态得撒回去，否则会残留到下一次取段
            self.pending_export = None;
        } else {
            self.mark_fetching_more(cx);
        }
    }

    /// 【B7】筛选中时导出的是**筛后的行集**（原型 §5.5）；没筛选就是 `None`（导全部已抓行）
    fn visible_rows_for_export(&self, cx: &mut Context<Self>) -> Option<Vec<Vec<String>>> {
        let delegate = self.grid.read(cx).delegate();
        (!delegate.filter().trim().is_empty()).then(|| delegate.visible_rows())
    }

    /// 【B7 切片二】把行交给 DuckDB 落成 Parquet / XLSX（**后台线程**，不占执行位）
    ///
    /// 为什么不在这里同步写：XLSX 首次要 `INSTALL excel`（联网，探针实测本机 3–11 秒），
    /// 在 UI 线程上等它就是界面假死；回执走 [`Self::drain_exec_results`] 的同一条轮询泵。
    fn start_duckdb_export(
        &mut self,
        entry: &ResultEntry,
        format: ExportFormat,
        index: usize,
        path: &std::path::Path,
        visible: Option<Vec<Vec<String>>>,
        cx: &mut Context<Self>,
    ) {
        let rows = match visible {
            Some(rows) => rows,
            None => entry.rows.clone(),
        };
        // 筛选中导出的是**筛后的行集**（原型 §5.5）：这个事实要跟着请求走，回执里要明示
        let filtered = !self.grid.read(cx).delegate().filter().trim().is_empty();
        let request = execution::DuckDbExportRequest {
            document: self.document.clone(),
            set: index,
            format,
            path: path.to_path_buf(),
            columns: entry.columns.clone(),
            rows,
            filtered,
        };
        match self.shared.request_duckdb_export(request) {
            // 说一句“在跑”：导出可能几秒（首次装扩展），不能点完什么都不说
            Ok(()) => {
                self.export_pending += 1;
                self.set_message(
                    Some(format!("正在导出 {}（首次装 excel 扩展可能要几秒）…", format.label())),
                    cx,
                );
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(format!("导出未提交：{reason}")), cx),
        }
    }

    /// 编码并落盘（事件路径上同步写：与另存为同一口径）
    ///
    /// 【B7 切片二】**Parquet / XLSX 不走这里**：它们要 DuckDB `COPY`（XLSX 首次还要联网装扩展），
    /// 交给 [`Self::start_duckdb_export`] 到后台线程上做。
    fn write_export(
        &mut self,
        entry: &ResultEntry,
        format: ExportFormat,
        index: usize,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        // 【B7 切片二】筛选中就导出筛后的行（原型 §5.5：导出的就是当前筛选后的行集，并且要明示）
        let visible = self.visible_rows_for_export(cx);
        if format.is_duckdb_backed() {
            self.start_duckdb_export(entry, format, index, path, visible, cx);
            return;
        }
        let table = export::default_table_name(Some(&entry.sql), index);
        let text = match &visible {
            Some(rows) => export::encode_rows(entry, format, &table, rows),
            None => export::encode(entry, format, &table),
        };
        // INSERT 对空结果给空串（没有行就没有语句）；CSV / JSON 空结果仍有表头 / `[]`
        if text.is_empty() {
            self.set_message(Some("这份结果没有可导出的行".to_string()), cx);
            return;
        }
        match std::fs::write(path, text) {
            Ok(()) => {
                let rows_text = match &visible {
                    Some(rows) => format!("{} 行（已筛选）", rows.len()),
                    None => format!("{} 行", entry.row_count()),
                };
                self.set_message(
                    Some(format!(
                        "已导出 {rows_text}（{}）→ {}",
                        format.label(),
                        path.display()
                    )),
                    cx,
                )
            }
            Err(error) => self.set_message(Some(format!("导出失败：{error}")), cx),
        }
    }

    /// 【B7】推进「抓全量后导出」：每段回填后决定“再抓一段”还是“落盘”
    ///
    /// `last_segment_failed` 来自回填循环（取段失败时那句失败**不入存储**，得在这里了断）：
    /// 失败 / 结果集被切换都作废，**不落半截文件**。
    fn advance_pending_export(&mut self, last_segment_failed: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_export.as_ref() else {
            return;
        };
        if last_segment_failed {
            self.pending_export = None;
            return;
        }
        let set = pending.set;
        if self.result_active != set {
            self.pending_export = None;
            self.set_message(Some("导出取消：结果集已切换".to_string()), cx);
            return;
        }
        let Some(entry) = self.shared.results_active(&self.document) else {
            self.pending_export = None;
            return;
        };
        if !entry.can_fetch_more() {
            let Some(pending) = self.pending_export.take() else {
                return;
            };
            self.write_export(&entry, pending.format, set + 1, &pending.path, cx);
            return;
        }
        // 还有下一段：接着抓（每次一段，等它回来再推进）
        let Some(sql) = self.result_sql.clone() else {
            self.pending_export = None;
            return;
        };
        let offset = entry.row_count();
        let submitted = self.execute(
            ExecTarget::Segment {
                sql,
                offset,
                limit: execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            cx,
        );
        if !submitted {
            self.pending_export = None;
        } else {
            self.mark_fetching_more(cx);
        }
    }

    /// 是否要显示结果区（本文档有结果或正在执行；且模式允许通信）
    fn result_visible(&self) -> bool {
        self.execution_allowed() && (self.pending > 0 || self.result_toolbar.is_some())
    }

    /// 【B6】复制错误原文（错误卡片上的「复制」）：便于拿去搜索 / 报 bug
    pub(crate) fn copy_error_message(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.result_error_card.clone() else {
            self.set_message(Some("这次没有错误可复制".to_string()), cx);
            return;
        };
        let location = card
            .location
            .as_ref()
            .map(|location| format!("（{location}）"))
            .unwrap_or_default();
        cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "{}{location}",
            card.message
        )));
        self.set_message(Some("已复制错误原文".to_string()), cx);
    }

    /// `Ctrl+W`：请求关闭当前文档
    ///
    /// **不由面板自己执行**：`DockArea` 移除面板会读面板本体（可见性 / 可关闭性），
    /// 从面板自己的 `update`（动作处理器就在其中）里发起就是重入，GPUI 直接 panic。
    /// 所以关闭由宿主调用 [`close_document_in_dock`]，面板只负责回答“能不能关”。
    /// 脏文档在这里拦下并说明原因（Dock 没有“关闭前否决”钩子，架构 §12 #18）；
    /// 三态确认（保存 / 不保存 / 取消）由宿主弹 [`dialogs::open_close_confirm`]。
    pub(crate) fn refuse_close_when_dirty(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_dirty() {
            return false;
        }
        self.set_message(Some("有未保存的改动，先保存再关闭".to_string()), cx);
        true
    }
}

impl EventEmitter<BasePanelEvent> for EditorHostPanel {}

/// 执行结论 → 结果记录（视图模型转换，不在这做任何 I/O）
fn entry_from(outcome: execution::ExecOutcome) -> ResultEntry {
    let connection = outcome.connection.clone();
    // 【B13】通道也随结论回来：标签星徽标与“切通道后标灰”靠它
    let channel = outcome.channel;
    // 【B15】血缘：失败也照样带（“这句话是下发筛选发出去的”对排查同样有用）
    let lineage = outcome.lineage;
    match outcome.result {
        Ok(data) => ResultEntry::success(
            outcome.document,
            outcome.sql,
            data.elapsed_ms,
            data.truncated,
            data.columns,
            data.rows,
        )
        .with_affected_rows(data.affected_rows)
        .with_has_more(data.has_more)
        .with_connection(connection)
        .with_channel(channel)
        .with_analysis(outcome.analysis)
        .with_lineage(lineage),
        Err(error) => ResultEntry::failure(outcome.document, outcome.sql, error, 0)
            .with_connection(connection)
            .with_channel(channel)
            .with_lineage(lineage),
    }
}

/// 网格空态文案（没有网格的结果：失败原因 / 写语句都不留空白面板）
///
/// 写语句的影响行数已经在工具栏报了（`affected 3 行 · 35 ms`），网格这再说一遍
/// 只是重复，所以这里只回答“网格为什么是空的”。
fn empty_text(entry: &ResultEntry) -> String {
    if entry.affects_rows_only() {
        "写语句没有结果集".to_string()
    } else {
        entry.summary()
    }
}

/// 把**一批**打开文档的会话全部落库（E3：窗口退出兜底）
///
/// 与单份的 [`EditorHostPanel::save_session_now`] 同一口径（未命名 / 空文档跳过；
/// 失败只打日志不弹窗）——差别只在「一次存一整批」：窗口关闭时**没有第二次机会**。
///
/// 为什么放在编辑器侧：这是编辑器语义（哪些文档值得存、空文档怎么算），
/// 宿主只负责「在窗口真的关之前叫我一声」。
pub fn save_sessions_for(panels: &[Entity<EditorHostPanel>], cx: &mut App) {
    for panel in panels {
        panel.update(cx, |panel, cx| panel.save_session_now(cx));
    }
}

/// 关闭一份文档的面板（**宿主调用**，比如 `Ctrl+W`）
///
/// 为什么是宿主的活：[`Panel::on_removed`] 会回读面板（可见性 / 可关闭性），
/// 而动作处理器本身就在面板的 `update` 里——从那里发起关闭是重入，GPUI 会直接 panic。
/// 宿主不在面板的 `update` 中，所以只有它能安全地让容器移除面板。
///
/// **脏文档在这里被拦下**（返回 `false`，并在面板状态栏留原因）：Dock 没有“关闭前否决”
/// 钩子（架构 §12 #18），这个判据就是安全网。要真正关掉脏文档，调用方必须先走三态确认
/// （[`dialogs::open_close_confirm`]）再调 [`close_document_now`]。
///
/// 文档本身由 [`EditorHostPanel::on_removed`] 从 `EditorService` 里关闭——
/// “一个面板 = 一份文档”的对应关系只有一处。
pub fn close_document_in_dock(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if panel.read(cx).is_dirty() {
        panel.update(cx, |panel, cx| panel.refuse_close_when_dirty(cx));
        return false;
    }
    close_document_now(area, panel, window, cx)
}

/// 直接关闭（**不做脏检查**）：调用方已就未保存改动拿到用户选择
///
/// 只应有两条调用路径：文档本来就干净（走 [`close_document_in_dock`]）、
/// 用户在关闭确认里选了“不保存”（走 [`request_close_document`]）。
pub fn close_document_now(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    area.update(cx, |area, cx| area.remove_panel(panel, window, cx));
    true
}

/// 关闭请求的完整流程（宿主键位 / 标签关闭都走它）
///
/// 干净 → 直接关；脏 → 三态确认（保存 / 不保存 / 取消）。流程放在这里而不是宿主，
/// 是因为它是**编辑器语义**（写盘、二次确认、失败分支）——宿主只负责“把面板从 Dock 上摘掉”
/// 与“弹系统文件对话框”（路径选择端口注入）。
pub fn request_close_document(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    if !panel.read(cx).is_dirty() {
        close_document_in_dock(area, panel, window, cx);
        return;
    }

    let title = panel.read(cx).title_text();
    let dialog_panel = panel.clone();
    let dialog_area = area.clone();
    dialogs::open_close_confirm(window, cx, title, move |choice, window, cx| {
        resolve_close_choice(&dialog_area, dialog_panel.clone(), choice, window, cx);
    });
}

/// 用户在“关闭前确认”上的选择落地（对话框回调与测试都走这里）
///
/// 与模式切换的 `confirm_mode_switch` 同一口径：弹窗只负责收集选择，
/// 落地只有一个入口——否则“对话框里点了保存”与“测试里走保存分支”会变成两段代码。
pub fn resolve_close_choice(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    choice: dialogs::CloseChoice,
    window: &mut Window,
    cx: &mut App,
) {
    match choice {
        // 取消：什么都不做（小改动常来自误触关闭，不该顺手丢掉）
        dialogs::CloseChoice::Cancel => {}
        dialogs::CloseChoice::Discard => {
            close_document_now(area, panel, window, cx);
        }
        dialogs::CloseChoice::Save => {
            save_then_close(area.clone(), panel, window, cx);
        }
    }
}

/// 保存当前文档（`Ctrl+Shift+S`，**不关文档**）：选路径 → 写盘；返回是否真写盘
///
/// 失败只落到状态栏（这里没在关闭流程上，不该用弹框拦住用户的手）。
pub fn request_save_as(panel: &Entity<EditorHostPanel>, cx: &mut App) -> bool {
    match pick_and_save_as(panel, cx) {
        Ok(()) => {
            panel.update(cx, |panel, cx| panel.clear_message(cx));
            true
        }
        Err(SaveAsFailure::Cancelled) => {
            // 用户取消：不是错误，但也不留上一次的提示
            panel.update(cx, |panel, cx| panel.clear_message(cx));
            false
        }
        Err(failure) => {
            let message = failure.message();
            panel.update(cx, |panel, cx| panel.set_message(Some(message), cx));
            false
        }
    }
}

/// 另存为的四种结局（调用方据此决定“关不关 / 弹不弹二次确认”）
///
/// “取消”单独成一类而不是 `None`：**用户取消不是失败**，不该弹二次确认、也不该报“未接入”。
enum SaveAsFailure {
    /// 宿主没接系统文件对话框（端口未注入）
    NoPicker,
    /// 用户取消
    Cancelled,
    /// 写盘失败（占用 / 权限 / 磁盘）
    Io(persist::PersistError),
    /// 文档已关闭（面板可能多活一帧）
    Gone,
}

impl SaveAsFailure {
    /// 状态栏文案（取消不产生文案，由调用方单独处理）
    fn message(&self) -> String {
        match self {
            Self::NoPicker => "未接入系统文件对话框，无法另存为".to_string(),
            Self::Cancelled => String::new(),
            Self::Io(error) => format!("另存为失败：{error}"),
            Self::Gone => "文档已关闭".to_string(),
        }
    }
}

impl From<persist::PersistError> for SaveAsFailure {
    fn from(error: persist::PersistError) -> Self {
        Self::Io(error)
    }
}

/// 另存为的“选路径 + 写盘”核心（不含失败后的交互）
///
/// 路径选择走宿主注入的端口（[`EditorShared::pick_save_path`]）：编辑器**不依赖** `rfd`。
fn pick_and_save_as(
    panel: &Entity<EditorHostPanel>,
    cx: &mut App,
) -> Result<(), SaveAsFailure> {
    let (current, mode) = {
        let read = panel.read(cx);
        (read.document_path(), read.document_mode())
    };
    let Some(mode) = mode else {
        return Err(SaveAsFailure::Gone);
    };
    if !panel.read(cx).shared.has_save_path_picker() {
        return Err(SaveAsFailure::NoPicker);
    }

    let default_name = mode.default_file_name().to_string();
    let picked = panel
        .read(cx)
        .shared
        .pick_save_path(current, default_name);
    let Some(path) = picked else {
        return Err(SaveAsFailure::Cancelled);
    };

    panel.update(cx, |panel, cx| panel.save_as(&path, cx)).map(|_| ())?;
    Ok(())
}

/// 保存后关闭（关闭三态的“保存”分支）
///
/// 三种结果分别处理：写盘成功 → 关；未命名 → 先另存为（用户取消就不关）；
/// 写盘失败（占用 / 权限 / 磁盘）→ 二次确认（重试 / 另存为 / 取消），
/// **绝不把脏文档当干净关掉**。
fn save_then_close(
    area: Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    match panel.update(cx, |panel, cx| panel.save(cx)) {
        Ok(_) => {
            close_document_now(&area, panel, window, cx);
        }
        Err(persist::PersistError::Untitled(_)) => {
            // 未命名：没有路径可写，先选一个（取消就不关）
            pick_save_as_then_close(area, panel, window, cx);
        }
        Err(error) => {
            let title = panel.read(cx).title_text();
            let reason = error.to_string();
            open_save_failure(&area, &panel, title, reason, window, cx);
        }
    }
}

/// 另存为后关闭（“保存”分支里未命名文档 / 用户选“另存为”的路径）
fn pick_save_as_then_close(
    area: Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    match pick_and_save_as(&panel, cx) {
        Ok(()) => {
            close_document_now(&area, panel, window, cx);
        }
        Err(SaveAsFailure::Cancelled) => {}
        Err(SaveAsFailure::Io(error)) => {
            let title = panel.read(cx).title_text();
            let reason = error.to_string();
            open_save_failure(&area, &panel, title, reason, window, cx);
        }
        Err(failure) => {
            let message = failure.message();
            panel.update(cx, |panel, cx| panel.set_message(Some(message), cx));
        }
    }
}

/// 写盘失败后的二次确认（重试 / 另存为 / 取消）
///
/// 三条路都保留：文件被占用 / 短暂无权限时“重试”最省事；目标路径本身不行时“另存为”能绕开；
/// “取消”后文档仍是脏的（真实状态不粉饰）。
fn open_save_failure(
    area: &Entity<DockArea>,
    panel: &Entity<EditorHostPanel>,
    title: String,
    reason: String,
    window: &mut Window,
    cx: &mut App,
) {
    let retry_area = area.clone();
    let retry_panel = panel.clone();
    let as_area = area.clone();
    let as_panel = panel.clone();
    let cancel_panel = panel.clone();
    dialogs::open_save_failure_confirm(
        window,
        cx,
        title,
        reason,
        move |choice, window, cx| match choice {
            dialogs::SaveFailureChoice::Cancel => {
                // 取消保存：文档仍是脏的（未保存状态是真实状态，不该粉饰）
                cancel_panel.update(cx, |panel, cx| panel.clear_message(cx));
            }
            dialogs::SaveFailureChoice::Retry => {
                save_then_close(retry_area.clone(), retry_panel.clone(), window, cx);
            }
            dialogs::SaveFailureChoice::SaveAs => {
                pick_save_as_then_close(as_area.clone(), as_panel.clone(), window, cx);
            }
        },
    );
}


/// 语句数：走 `engine::sql::split` 的**词法级**切分（不是 `split(';')`）
///
/// 只在内容变化时调一次（构造 / 输入回写 / 重新加载），渲染期只读缓存值。
fn count_statements(text: &str) -> usize {
    engine::sql::split_statements(text).len()
}

impl Focusable for EditorHostPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for EditorHostPanel {
    fn panel_name(&self) -> &'static str {
        "editor"
    }

    /// 可关闭：**脏文档不给 ✕**（Dock 无“关闭前否决”钩子，架构 §12 #18）
    ///
    /// 标签 ✕ 由 Dock 自己处理：`DockArea` 收到 `TabGroupEvent::ClosePanel` 就直接移除面板，
    /// 没有地方能插一句“先问用户”。因此脏文档的关闭入口只留**显式的那一个**：
    /// `Ctrl+W` → [`request_close_document`]（保存 / 不保存 / 取消）。
    /// 保存后脏点消失，✕ 自然回来。
    fn closable(&self, _cx: &App) -> bool {
        !self.is_dirty()
    }

    /// 记下所在标签组的弱句柄：
    /// - 把自己激活（宿主打开已存在文档时）
    /// - 请求关闭（`Ctrl+W`）
    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }

    /// 成为当前显示的标签 → 把焦点交给编辑内核，切过去就能直接打字
    ///
    /// 同时把“当前文档”告知服务层：宿主的 `Ctrl+W` 等文档级动作按它找面板。
    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active && !self.closed {
            self.shared
                .update(|service| service.activate(&self.document));
            self.editor.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    /// 被 Dock 移除 = 这份文档的生命周期结面：**一个面板 = 一份文档**，
    /// 面板走人，文档随之从 `EditorService` 里关掉（宿主不必再维护对应关系）。
    fn on_removed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // A12：走人之前把会话（光标 / 选区 / 模式 / 内容）落库——此刻文档与内核都还在
        self.save_session_now(cx);
        self.closed = true;
        self.group = None;
        self.shared
            .update(|service| service.close(&self.document));
        // 文档关了，结果也不留（结果不跟着已关闭的文档挂着）
        self.shared
            .update_results(|store| store.clear(&self.document));
        cx.notify();
    }
}

impl ComponentPanel for EditorHostPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        self.with_document(|doc| SharedString::from(doc.title().to_string()))
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().child(self.title_text())
    }

    /// 脏点：Dock 没有"关闭前询问"钩子，脏状态必须**在标签上可见**（否则用户会误关）
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        self.is_dirty().then(|| {
            div()
                .size(rems(ui::STATUS_DOT_SIZE))
                .rounded_full()
                .bg(cx.theme().colors.primary)
        })
    }
}

impl Render for EditorHostPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, primary, muted) = {
            let theme = cx.theme();
            (
                theme.colors.background,
                theme.colors.primary,
                theme.colors.muted_foreground,
            )
        };
        let _ = (primary, muted);
        let read_only = self.editor_read_only();

        // 光标 / 选区：从内核读真实值（行号列号按 1 基显示）
        let (line, column, selected_chars) = {
            let state = self.editor.read(cx);
            let cursor = state.cursor_position();
            (
                cursor.line as usize + 1,
                cursor.character as usize + 1,
                state.selected_text().chars().count(),
            )
        };
        // 连接段只在会通信的模式出现（文本模式没有连接概念，不显示占位）
        let communicating = self
            .with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false);
        let connection_text = if communicating {
            let bound = self
                .with_document(|doc| doc.connection().map(str::to_string))
                .flatten();
            Some(self.shared.connection_status_text(bound.as_deref()))
        } else {
            None
        };
        // 【B13】通道段：会通信的模式才有（文本模式没有“执行位置”这回事）
        let current_channel = self.channel();
        let channel_text = communicating.then(|| channel::status_text(current_channel));
        // TX 区（B4）：会通信 + 执行器真支持事务 + **当前通道支持事务**
        // （加速 / 联邦没有事务语义，那时摆一个 TX 区就是摆了个假控件）
        let tx_available =
            communicating && self.shared.has_transactions() && current_channel.allows_transactions();
        let tx_text = tx_available.then(|| self.tx_text());
        let status = StatusInputs {
            mode: self.with_document(|doc| doc.mode()).unwrap_or(EditorMode::Text),
            dirty: self.is_dirty(),
            read_only: self.read_only_flags(),
            statements: self.statements,
            line,
            column,
            selected_chars,
            message: self.message.as_deref(),
            executing: self.pending > 0,
            elapsed: self.running_since.map(|since| since.elapsed()),
            connection: connection_text.as_deref(),
            channel: channel_text.as_deref(),
            tx: tx_text.as_deref(),
        };

        // 中断（B3）：只在本文档真有语句没回填时出现（原型 §5.1：「中断」挂在状态栏■）
        let interrupt = self.can_interrupt().then(|| {
            let entity = cx.entity();
            Button::new("editor-interrupt")
                .ghost()
                .small()
                .debug_selector(|| "editor-interrupt".to_string())
                .label("■ 中断")
                .on_click(move |_, _window, app| {
                    entity.update(app, |panel, cx| panel.interrupt(cx));
                })
                .into_any_element()
        });

        // 事务区控件（B4）：自动提交常显（它是模式，模式要一直看得见），
        // 提交 / 回滚只在事务真开着时出现（原型 §2.5：事务区）
        let tx_controls = tx_available.then(|| {
            let mut row = div().h_flex().items_center().gap_1();
            let autocommit_entity = cx.entity();
            row = row.child(
                Button::new("editor-autocommit")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-autocommit".to_string())
                    .toggled(self.autocommit)
                    .label("自动提交")
                    .on_click(move |_, _window, app| {
                        autocommit_entity.update(app, |panel, cx| panel.toggle_autocommit(cx));
                    }),
            );
            if self.tx_open {
                for (action, id) in [
                    (execution::TxAction::Commit, "editor-tx-commit"),
                    (execution::TxAction::Rollback, "editor-tx-rollback"),
                ] {
                    let entity = cx.entity();
                    row = row.child(
                        Button::new(id)
                            .ghost()
                            .small()
                            .debug_selector(move || id.to_string())
                            .label(action.label())
                            .on_click(move |_, _window, app| {
                                entity.update(app, |panel, cx| panel.tx_action(action, cx));
                            }),
                    );
                }
            }
            row.into_any_element()
        });

        let mut root = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(bg)
            // A10：键位都绑在这个 context 上（`crates/app` 注册）。
            // 焦点在内核里时内核的 `Input` context 更深、先被尝试；它没处理的键再冒泡到这里。
            // `track_focus` 不能省：没有它面板不在 dispatch path 上，快捷键就落不到动作（已踩）。
            .key_context("editor")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_format))
            .on_action(cx.listener(Self::on_toggle_comment))
            .on_action(cx.listener(Self::on_trigger_completion))
            .on_action(cx.listener(Self::on_execute_sql))
            .on_action(cx.listener(Self::on_execute_all))
            // 【B14】网格里的 `Ctrl+C`：键位绑在 `view/results/grid.rs` 那一层的上下文上，
            // 所以只有焦点在网格里时才会走到这里（编辑区里 `Ctrl+C` 仍归内核的文本复制）
            .on_action(cx.listener(Self::on_copy_grid_selection));
        // 工具栏（②）：模式指示器在这里，模式不再是只能从状态栏读到的短标签
        let mode = self.with_document(|doc| doc.mode()).unwrap_or(EditorMode::Text);
        root = root.child(self.render_toolbar(mode, cx));
        // 提示卡（A13）：档位带来的限制要在界面上说清，而不是让用户自己撞上（“能编辑却改不了”）
        if let Some(notice) = self.tier_notice() {
            let theme = cx.theme();
            let warning = theme.colors.warning;
            let warning_fill = warning.opacity(0.18);
            root = root.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(warning)
                    .bg(warning_fill)
                    .border_b(ui::HAIRLINE)
                    .border_color(warning)
                    .child(SharedString::from(notice)),
            );
        }
        // 结果区（B5）：有结果或正在执行时才出现（否则不占位置——不显示空壳）
        let font_size = cx.theme().font_size;
        let result_pane = self.result_visible().then(|| {
            // ⑥ 工具栏：投影里已经有真值；还没结论时（刚提交）只报“执行中…”
            let toolbar = self.result_toolbar.clone().unwrap_or(ResultToolbar {
                elapsed_ms: None,
                ..Default::default()
            });
            // 结果集标签条（⑤）：两份以上结果才画（一份结果不需要切换器）
            let tabs = (self.result_tabs.len() >= 2).then(|| {
                let entity = cx.entity();
                result_sets::render(
                    &self.result_tabs,
                    self.result_active,
                    move |index, _window, app| {
                        let index = *index;
                        entity.update(app, |panel, cx| panel.select_result_set(index, cx));
                    },
                    cx,
                )
                .into_any_element()
            });
            // 【B15】本地筛选框（原型 §5.5 的 `⌕ 筛选`）：只作用于视图层，不重查数据库；
            // 输入走 300ms 防抖（`on_filter_input`），有词时给一个 ✕ 清除
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
                    let entity = cx.entity();
                    row.child(
                        Button::new("editor-result-filter-clear")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-filter-clear".to_string())
                            .label("✕")
                            .on_click(move |_, window, app| {
                                entity.update(app, |panel, cx| panel.clear_filter(window, cx));
                            }),
                    )
                })
                // 【B14】下发源库开关（原型 §5.5 的 `▢ 下发源库`）：开着时应用筛选会重查
                .child({
                    let entity = cx.entity();
                    Switch::new("editor-result-pushdown")
                        .checked(self.pushdown)
                        .label("下发源库")
                        .on_click(move |checked, _window, app| {
                            let open = *checked;
                            entity.update(app, |panel, cx| panel.set_pushdown(open, cx));
                        })
                })
                .into_any_element();

            // 动作：有网格才摆复制（没东西可复制就不摆）；有 SQL 就摆重跑（原位刷新）
            let copy = self.result_can_copy.then(|| {
                let entity = cx.entity();
                Button::new("editor-result-copy")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-copy".to_string())
                    .label("复制")
                    .on_click(move |_, _window, app| {
                        entity.update(app, |panel, cx| panel.copy_active_result(cx));
                    })
                    .into_any_element()
            });
            let refresh = (self.result_sql.is_some() && !self.result_is_analysis).then(|| {
                let entity = cx.entity();
                Button::new("editor-result-refresh")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-refresh".to_string())
                    .label("⟳ 刷新")
                    .on_click(move |_, _window, app| {
                        entity.update(app, |panel, cx| panel.refresh_active_result(cx));
                    })
                    .into_any_element()
            });
            // 【B5b】取下一段：只在这份结果真还有下一段时摆（状态行⑦里）
            let more = self
                .result_status
                .as_ref()
                .is_some_and(|status| status.has_more)
                .then(|| {
                    let entity = cx.entity();
                    Button::new("editor-result-more")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-more".to_string())
                        .label("取下一段")
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.fetch_more(cx));
                        })
                        .into_any_element()
                });

            // 【B7】导出：格式 × 范围（原型 §2.4 的 `⤓ 导出 ▾`）。没网格就不摆（没东西可导）。
            // 菜单项由 `export::menu_items` 给（纯函数，可逐项断言）——这里只负责画。
            let export = self.result_can_copy.then(|| {
                let entity = cx.entity();
                let rows_text = self
                    .result_status
                    .as_ref()
                    .map(|status| result_grid::thousands(status.total_rows))
                    .unwrap_or_else(|| "0".to_string());
                let has_more = self
                    .result_status
                    .as_ref()
                    .is_some_and(|status| status.has_more);
                DropdownButton::new("editor-result-export")
                    .small()
                    .button(
                        Button::new("editor-result-export-btn")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-export".to_string())
                            .label("⤓ 导出"),
                    )
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        for item in export::menu_items(has_more, &rows_text) {
                            if item.separator_before {
                                menu = menu.separator();
                            }
                            let Some((format, scope)) = item.action else {
                                // 分组标题（“已抓取 N 行” / “抓全量后导出”）
                                menu = menu.item(PopupMenuItem::label(item.label));
                                continue;
                            };
                            let entity = entity.clone();
                            menu = menu.item(
                                PopupMenuItem::new(item.label).on_click(move |_, _window, app| {
                                    entity.update(app, |panel, cx| match scope {
                                        ExportScope::Fetched => {
                                            panel.export_active_result(format, cx)
                                        }
                                        ExportScope::All => {
                                            panel.export_active_result_all(format, cx)
                                        }
                                    });
                                }),
                            );
                        }
                        menu
                    })
                    .into_any_element()
            });

            // 【B15】分析：对选中那份结果的**已抓行**跑聚合 SQL（本地 DuckDB，不碰源库）。
            // 菜单项由 `analysis::menu_items` 给（纯函数，可逐项断言）；没网格就不摆。
            let analysis_items = self
                .shared
                .results_active(&self.document)
                .map(|entry| crate::analysis::menu_items(&entry))
                .unwrap_or_default();
            let analysis = (!analysis_items.is_empty()).then(|| {
                let entity = cx.entity();
                let items = analysis_items.clone();
                DropdownButton::new("editor-result-analysis")
                    .small()
                    .button(
                        Button::new("editor-result-analysis-btn")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-analysis".to_string())
                            .label("⚗ 分析"),
                    )
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        for item in items.iter() {
                            let entity = entity.clone();
                            let sql = item.sql.clone();
                            menu = menu.item(PopupMenuItem::new(item.label.clone()).on_click(
                                move |_, _window, app| {
                                    let sql = sql.clone();
                                    entity.update(app, |panel, cx| panel.run_analysis(sql, cx));
                                },
                            ));
                        }
                        // 【B15 切片二】预置项之外，自己写一条（弹输入框，模板预填）
                        let entity = entity.clone();
                        menu.separator().item(
                            PopupMenuItem::new(crate::analysis::CUSTOM_LABEL).on_click(
                                move |_, window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.request_custom_analysis(window, cx)
                                    });
                                },
                            ),
                        )
                    })
                    .into_any_element()
            });

            // 错误卡片（B6，原型 §2.4）：失败时替掉网格；两个按钮都是真的能按的
            let card = self.result_error_card.clone().map(|card| {
                let locate = card.location.as_ref().map(|location| {
                    let entity = cx.entity();
                    let label = error_card::locate_label(location);
                    Button::new("editor-result-error-locate")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-error-locate".to_string())
                        .label(label)
                        .on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.jump_to_error_site_with(window, cx)
                            });
                        })
                        .into_any_element()
                });
                let copy = {
                    let entity = cx.entity();
                    Button::new("editor-result-error-copy")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-result-error-copy".to_string())
                        .label("复制")
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.copy_error_message(cx));
                        })
                        .into_any_element()
                };
                error_card::render(
                    &card,
                    error_card::ErrorCardControls {
                        locate,
                        copy: Some(copy),
                    },
                    cx,
                )
                .into_any_element()
            });

            result_grid::render(
                &self.grid,
                result_grid::ResultPane {
                    toolbar,
                    status: self.result_status.clone(),
                    controls: result_grid::ResultControls {
                        filter: Some(filter),
                        analysis,
                        copy,
                        export,
                        refresh,
                        more,
                    },
                    card,
                    tabs,
                    notice: self.result_notice.clone(),
                },
                cx,
            )
            .into_any_element()
        });

        // 编辑区 + 结果区：下半区可拖拽改高（原型 §2.1 ④ 分隔条；拖动条是组件库的
        // `ResizablePanel`，不手搓鼠标拖拽）。没有结果时只有一个面板，编辑区独占。
        let mut split = v_resizable("editor-result-split").child(
            resizable_panel().child(
                div()
                    .flex_1()
                    .min_h_0()
                    // 测试按选择器断言“结果区没有把编辑区挤掉”
                    .debug_selector(|| "editor-code-area".to_string())
                    // 局部内距走 Tailwind 尺度（8px）；结构尺寸才进 ui.rs 常量表
                    .px_2()
                    // 拖放落点（原型 §4.5：拖文件节点到编辑区 → 插入文件内容到光标处）
                    .on_drop({
                        let entity = cx.entity();
                        move |payload: &::shared::InsertFileDrag, window, app| {
                            entity.update(app, |this, cx| {
                                this.insert_file_contents(payload, window, cx)
                            });
                        }
                    })
                    .child(
                        Editor::new(&self.editor)
                            .appearance(false)
                            .bordered(false)
                            .readonly(read_only)
                            .size_full(),
                    ),
            ),
        );
        if let Some(pane) = result_pane {
            let height = self.result_height.clone();
            split = split
                .child(
                    resizable_panel()
                        .size(font_size * height.get())
                        .size_range(
                            font_size * ui::RESULT_MIN_HEIGHT..font_size * ui::RESULT_MAX_HEIGHT,
                        )
                        .flex_none()
                        .child(pane),
                )
                // 拖到哪就记到哪：下一次渲染用它当初始尺寸（同一面板内记得住）
                .on_resize(move |state, _window, app| {
                    if let Some(last) = state.read(app).sizes().last() {
                        height.set(last.as_f32() / app.theme().font_size.as_f32());
                    }
                });
        }
        root = root.child(split);
        root.child(status_bar::render(
            &status,
            status_bar::StatusControls {
                tx: tx_controls,
                interrupt,
            },
            cx,
        ))
    }
}
