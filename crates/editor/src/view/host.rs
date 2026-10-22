//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use std::cell::RefCell;

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity, Position, RopeExt as _};
use gpui_kit::base::text::TextView;
use gpui_kit::base::{StyledExt as _, resizable_panel, v_resizable};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{Icon};
use gpui_kit::ScrollHandle;
use gpui_kit::component::dock::{
    BasePanel, DockArea, Panel as ComponentPanel, PanelEvent as BasePanelEvent, PanelId, TabGroup,
};
use gpui_kit::component::input::{
    Editor, EditorState, Input, InputEvent, InputState, RangeDecoration, RangeDecorationCollection,
    RangeDecorationStyle,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::table::{TableEvent, TableState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::changes::CellChange;
use crate::channel::{self, ExecChannel};
use crate::commands::{
    CopyGridSelection, ExecuteAll, ExecuteSql, FormatDocument, SaveDocument, ToggleComment,
};
use crate::diagnostics;
use crate::edit;
use crate::execution::{self, ExecMenuKind, ExecTarget, ResultPlacement};
use crate::export::{self, ExportFormat, ExportScope};
use crate::format;
use crate::language_service;
use crate::mode::{self, CellGranularity};
use crate::model::{DocumentId, EditorMode, ReadOnly};
use crate::persist;
use crate::service::Document;
use crate::shared::{EditorShared, InsightColumnRequest};
use crate::sources;
use crate::completion::ColumnAnnotation;
use crate::store::ResultEntry;
use crate::translate;
use crate::ui;
use crate::view::completion;
use crate::view::dialogs;
use crate::view::fold;
use crate::view::highlight;
use crate::view::results::error_card::{self, ErrorCard};
use crate::view::results::script_card;
use crate::view::results::grid::{
    self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar,
};
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

/// 【T2a-6】一次批量提交的在途状态（回执回来前暂存）
///
/// 为什么索引要**在生成时存下来**：执行器的回执只按语句顺序报影响行数
/// （`CommitReport::per_statement`），它不知道每条对应结果集的哪一行哪一列——
/// 那个映射在 `commit_updates_rows` 生成 UPDATE 时才有。回执一到，用它把
/// 「影响行数 0 = 冲突」标到具体的格子（`mark_conflict`），不猜不重算。
struct CommitInFlight {
    /// 发起提交的结果集下标（回执处理要 `mutate_change_set` 定位它）
    set_index: usize,
    /// 每条 UPDATE 对应的 `(数据行号, 变更列)`（与执行器逐条影响行数**同序**）
    per_line: Vec<(usize, Vec<usize>)>,
    /// 生成时跳过的行数（键值 NULL 定位不到）——回执文案要如实说出来
    skipped: usize,
}

/// 【遗留 #7】值查看器的目标（右键「查看完整值」弹出；`value_preview` 为 `Some` 时
/// 渲染期就弹 [`dialogs::open_value_preview`]）
#[derive(Clone)]
struct ValuePreview {
    value: String,
    column: String,
    row: usize,
}

/// 【T2c-3 收尾】编辑内容模式：行编辑（默认，单光标）/ 列编辑（多光标）。
///
/// 列编辑 = 内核多光标能力（`add_cursor_above|below`，0.7.1 经 vendor 补丁公开）；
/// 行编辑模式下宿主在文本变更时强制收拢到单光标（见 Change 事件处理器）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum EditMode {
    #[default]
    Row,
    Column,
}

impl EditMode {
    /// 状态栏入口的短标签（`行 ▾` / `列 ▾`）
    pub(crate) fn short_label(self) -> &'static str {
        match self {
            EditMode::Row => "行",
            EditMode::Column => "列",
        }
    }
}

/// 一份文档的编辑面板
/// 【#17】一次进行中的内核单元执行（同一时间最多一个）
#[derive(Clone)]
struct KernelRun {
    /// 目标内核（`ExecutionSource::Kernel` 指定，否则语言默认 id）
    kernel_id: String,
    /// 目标单元 id（事件回执按它回填输出）
    cell_id: String,
    /// 目标会话 id（切换会话后回执到了不落）
    session_id: String,
}

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
    /// 所在 `DockArea` 的弱句柄（宿主在加入 Dock 前用 [`Self::set_area`] 注入）。
    ///
    /// 用途：脏 tab 的**自绘 ✕**（`title_suffix`）要弹「保存/不保存/取消」确认——
    /// Dock 内置 ✕ 是 `group.close` 直接移除、没有任何“关闭前询问”钩子（见
    /// `closable` 注释），所以脏文档的内置 ✕ 保持隐藏，改为这里给确认入口。
    pub(crate) area: Option<WeakEntity<DockArea>>,
    /// 状态栏提示：动作失败的真实原因（未命名需另存为 / 有未保存改动 / 只读拒绝）
    ///
    /// 有它才不会有“按了没反应”的动作：失败必须留下可见痕迹。成功即清空。
    pub(crate) message: Option<String>,
    /// 【T2c-3 收尾】编辑内容模式（行编辑 / 列编辑；面板内存态，不随会话持久化）
    edit_mode: EditMode,
    /// 是否已被 Dock 移除（移除即关闭文档，见 `Panel::on_removed`）
    pub(crate) closed: bool,
    /// 欢迎页是否已被用户点掉（启动空白文档的“空编辑器”换成欢迎页；
    /// 点「开始编写」或内容非空后回到正常编辑器）
    welcome_dismissed: bool,
    /// 【2026-10-19】脏态自绘 ✕ 的悬浮开关：鼠标悬停标题才显示关闭按钮，
    /// 避免常驻 ✕ 与标题内容互相遮挡（见 `title_suffix`）。
    title_hovered: std::cell::Cell<bool>,
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
    /// 结果集标签条的滚动句柄（标签随查询累积增多，传给 TabBar `track_scroll` 保持跨帧滚动状态）
    result_tabs_scroll: ScrollHandle,
    /// 当前选中的结果集下标（标签条的选中态）
    result_active: usize,
    /// 【B5】当前选中结果集的 SQL（工具栏「刷新」重跑它；`None` = 无可重跑的东西）
    result_sql: Option<String>,
    /// 【B15】当前选中结果是**本地分析**的产物（不摆“刷新”：重跑它没有意义）
    result_is_analysis: bool,
    /// 【B5】当前选中结果集有没有可复制的东西（没有网格就不摆复制按钮）
    result_can_copy: bool,
    /// 【P2-4】转置视图是否打开（单行转置；数据行随 [`Self::transpose_row`] 竖排）
    transposed: bool,
    /// 【P2-4】转置当前展示的数据行（越界由 delegate 夹取）
    transpose_row: usize,
    /// 【B6】错误卡片的内容（失败时才有；`None` = 这次成功）
    result_error_card: Option<ErrorCard>,
    /// 【2b】脚本执行的输出卡片（通用模式跑完才有；`None` = 没跑过 / 已切走）
    script_card: Option<crate::view::results::script_card::ScriptCard>,
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
    /// 【遗留 #7】值查看器：右键「查看完整值」弹出的目标（`Some` = 弹层开着）
    value_preview: Option<ValuePreview>,
    /// 【遗留 #14】CSV 表格展示：文档是 CSV 且网格已填（渲染据此走精简结果区；
    /// 数据在 `grid` 里，不经过结果 store——CSV 没有 SQL 执行语义）
    csv_active: bool,
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
    /// 【B18】后台扫描器（语句数 / 折叠候选 / 打字期诊断）
    ///
    /// 为什么要有它：这三件事原先全在 `InputEvent::Change` 上**同步**做——性能基线
    /// （`tests/offline_baseline.rs`）实测 5 千行脚本每敲一下要按住主线程 90–130 ms，
    /// 贴 1 MB 约 0.6–1.0 s。挪到工作线程后，按键只做一次提交（带修订号）。
    scanner: crate::scan::Scanner,
    /// 【B18】当前修订号：回执只接受与它相等的那一份（陈旧丢弃）
    scan_revision: u64,
    /// 【B18】有扫描在途（结果轮询泵据此多活一会儿）
    scan_pending: bool,
    /// 【B18】最近一次扫描给的打字期诊断（与执行错误回填**一起**画，见 `apply_error_marks`）
    scan_diagnostics: Vec<language_service::Diagnostic>,
    /// 【B6】执行错误的原因（`None` = 这次成功）；与词法诊断合并重建时要连它一起重画
    error_reason: Option<String>,
    /// 结果轮询任务句柄（空闲即退出；句柄存活到下一次提交）
    exec_pump: RefCell<Option<Task<()>>>,
    /// 【#17】正在跑的内核单元（同一时间最多一个，与 `has_running` 防并发一致）
    kernel_running: Option<KernelRun>,
    /// 【#17】内核事件轮询任务句柄（空闲即退出）
    kernel_pump: RefCell<Option<Task<()>>>,
    /// 【2d-笔记级】「全部运行」队列（待跑单元 id；空 = 没排队）。
    ///
    /// 执行队列一次只跑一个（`ExecQueue` busy 检查），所以全部运行是**串行队列**：
    /// 入队全部可执行单元，跑完一个（SQL 回执 / 内核收口）自动提交下一个。
    notebook_pending: Vec<String>,
    /// 内容回写订阅：句柄即生命周期（释放即取消）
    _editor_sub: Option<Subscription>,
    /// 网格事件订阅（行选中 → 状态行的“已选第 N 行”要跟着变）
    _grid_sub: Option<Subscription>,
    /// 【B15】筛选框订阅（输入 → 防抖）
    _filter_sub: Option<Subscription>,
    /// 【T2a-4】编辑框订阅（输入变化 → 同步编辑态；Enter / 失焦 → 提交）。
    /// 句柄即生命周期：不存字段就会被 drop、订阅即刻取消（编辑器内踩过一次）。
    _edit_sub: Option<Subscription>,
    /// 【T2a-6】提交中的一次批量提交（`None` = 没有提交在跑；回执回来前按钮保持灰）
    commit_in_flight: Option<CommitInFlight>,
    /// 【T2c-5】工具栏分组自定义布局（权威状态；render 纯读，改动走面板方法）
    toolbar: crate::toolbar::ToolbarLayout,
    /// 【2c】隐藏字符标注集合（`show_invisible` 开启时懒创建；`None` = 未启用/未建）
    ///
    /// 集合句柄借的是内核实体：面板 drop 时随内核一起释放，无需手动 `dispose`。
    invisible_decorations: Option<RangeDecorationCollection>,
    /// 【LSP】本文档的语言服务会话（`None` = 宿主办无可用的服务；didOpen 已发）
    lsp: Option<crate::lsp::LspSession>,
    /// 【LSP】本文档的 LSP 文档版本（didOpen = 1，此后每次 didChange 自增）
    lsp_version: i64,
    /// 【LSP】语言服务是否健康（收到 IoError / EOF 后置 false → 摘 provider、停轮询）
    lsp_alive: bool,
    /// 【LSP】语言服务最近一次推送的诊断（与执行错误 / 词法诊断三源合并重画）
    lsp_diagnostics: Vec<lsp_types::Diagnostic>,
    /// 【遗留 #10】分析笔记本：单元源的内核编辑器实例（键 = 稳定 `cell_id`，不用下标；
    /// 渲染据此用 [`Editor`] 画单元正文——原型 §4.2「源编辑区 = 内核编辑器」）
    /// `pub(crate)`：analysis.rs（同 crate）渲染期只读取它
    pub(crate) cell_editors: std::collections::HashMap<String, Entity<EditorState>>,
    /// 【遗留 #10】单元编辑器的 Change 订阅（句柄即生命周期；移除单元时一并丢弃）
    _cell_editor_subs: std::collections::HashMap<String, Subscription>,
    /// 【遗留】通用模式 Markdown 文档的渲染态：true = 显示渲染视图（TextView::markdown），
    /// false = 源码编辑器（默认）。仅对 `ScriptFileType::Markdown` 生效；非 markdown 文档忽略。
    markdown_preview: bool,
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
                None => (String::new(), true, EditorMode::General),
            }
        };
        // 分析模式的 `.rdsnote` 解析要读正文，而 `content` 稍后被 move 进编辑内核闭包——先 clone
        let notebook_content = content.clone();
        // 语义着色类型在闭包外算好：SQL 模式与未识别扩展名走 SQL 词法，通用模式按
        // 脚本扩展名分派（CSV 走表格展示不上色）。`service.find` 拿文档路径做判定。
        let highlight_kind = {
            let service = shared.service();
            match service.find(&document) {
                Some(doc) => highlight::kind_for_document(doc.path(), doc.mode()),
                None => highlight::kind_for_document(None, mode),
            }
        };

        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx);
            state.set_value(content, window, cx);
            state.set_readonly(editor_read_only, cx);
            // A4：语义着色（SQL 词法；通用模式按脚本类型分派；CSV / 分析模式不上色）
            if highlight::is_enabled(mode) {
                highlight::install(&mut state, cx, highlight_kind);
            }
            // B9：SQL 补全（候选来自宿主端口；文本模式 / 大文件档位不装）
            if shared.completion_enabled(&document) {
                completion::install(&mut state, shared.clone(), document.clone());
            }
            // B17：折叠（内核只认 highlighter 给的候选，我们自己算，见 `fold` / `view::fold`）。
            // 装不上就真关开关，不只留一个判据。
            sync_fold(&shared, &document, &mut state, window, cx);
            state
        });

        let sub = cx.subscribe_in(
            &editor,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // 【T2c-3 收尾】行编辑模式强制单光标（列编辑 = 多光标）：文本变更时
                    // 把多光标收拢到活动光标。订阅闭包里的 `state` 是内核实体的引用，
                    // 先 `read` 判断、再 `update` 收拢（避免与后面的 `this.editor_text` 争用）。
                    if this.edit_mode == EditMode::Row && state.read(cx).cursor_count() > 1 {
                        state.update(cx, |state, cx| state.collapse_cursors(window, cx));
                    }
                    // 内核 → 服务：内容只落到 `EditorService`，脏状态由它比较基线得出
                    let text = this.editor_text(cx);
                    // 【LSP】全文同步给语言服务（版本自增；会话健康才发，死了不写）
                    if this.lsp_alive {
                        if let Some(session) = this.lsp.as_ref() {
                            this.lsp_version += 1;
                            let _ = session
                                .client
                                .did_change(&session.uri, this.lsp_version, &text);
                        }
                    }
                    // 【B18】语句数 / 折叠候选 / 打字期诊断一律**不在事件路径上算**：
                    // 提交一份带修订号的扫描（正文要两份——服务层一份、扫描器一份，所以克隆一次），
                    // 回执由结果轮询泵回填（`drain_exec_results`）——陈旧的那份会被丢掉。
                    // 动作与补全 / 格式化共用这一条事件路径，所以 `replace_all`
                    // （模板 / 注释 / 格式化）也覆盖到。
                    this.submit_scan(text.clone(), cx);
                    this.shared
                        .update(|service| service.set_content(&this.document, text));
                    // 【遗留 #14】CSV 表格展示：编辑 CSV 文本时实时重解析进网格
                    //（每次击键全量解析对大数据文件有开销，但 CSV 预览场景数据量
                    //  通常可控；卡顿再换防抖泵）
                    this.sync_csv_grid(cx);
                    // 【2c】显示隐藏字符开启时：正文变了，标注区间要跟着重算（区间是字节偏移，
                    // 文字一动就全失效；没开显示则集合为空 / 未建，直接跳过）
                    if this.shared.document_show_invisible(&this.document) {
                        let ranges = crate::hidden::hidden_ranges(&this.editor_text(cx));
                        this.refresh_invisible_decorations(ranges, cx);
                    }
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
        let grid_sub = cx.subscribe_in(&grid, window, |panel, grid, event: &TableEvent, window, cx| {
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
            // 【T2a-4】双击单元格 → 进入编辑（原生 `DoubleClickedCell` 事件；判据一次
            // 问清：通道 / 项目 / 文档 / 连接 / 行标识 / 列。不过就不进编辑，顶部说明原因。
            if let TableEvent::DoubleClickedCell(row, col) = event {
                let gate = {
                    let store = panel.shared.results();
                    let entry = store.active(&panel.document);
                    crate::editability::cell_editable(
                        panel.channel(),
                        crate::project::ProjectState {
                            read_only: panel.shared.project_read_only(),
                        },
                        panel.read_only_flags(),
                        entry.and_then(|entry| entry.row_key()).as_ref(),
                        entry.map(|entry| entry.columns.len()).unwrap_or(0),
                        col.saturating_sub(1),
                    )
                };
                match gate {
                    Ok(()) => {
                        grid.update(cx, |state, cx| {
                            if state.delegate_mut().begin_edit(*row, *col) {
                                cx.notify();
                            }
                        });
                        // 【T2a-4】进入编辑后**立即**把焦点交给编辑框（事件路径，不是 render——
                        // render 是纯读阶段，那里的 `window.focus` 不可靠）。焦点事件在下一帧
                        // draw 时派发，输入内核据此激活文本输入，用户才能直接打字。
                        if let Some(input) = grid.read(cx).delegate().edit_input() {
                            let focus = input.read(cx).focus_handle(cx);
                            window.focus(&focus, cx);
                        }
                    }
                    Err(reason) => {
                        panel.result_notice = Some(format!("编辑被拦：{}", reason.reason()));
                    }
                }
            }
            if let Some(status) = panel.result_status.take() {
                panel.result_status = Some(ResultStatus {
                    selected_row,
                    ..status
                });
            }
            cx.notify();
        });

        // 【T2a-4】结果表格的单元格编辑框：**一个复用实体**（每次编辑复用同一个，
        // 不做“每格一个实体”的泄漏）。初值同步与聚焦在网格渲染路径做（幂等一次）。
        let edit_input = cx.new(|cx| InputState::new(window, cx));
        // 编辑框事件（原生 `InputEvent`）：输入变化 → 同步到编辑态；Enter / 失焦 → 提交。
        // 提交幂等：`commit_edit` 先清编辑态，失焦带起的第二次 Blur 是空转。
        let _edit_sub = cx.subscribe_in(
            &edit_input,
            window,
            |panel, state, event, _window, cx| match event {
                InputEvent::Change => {
                    let value = state.read(cx).value().to_string();
                    panel.grid.update(cx, |table, _cx| {
                        table.delegate_mut().sync_edit_value(value);
                    });
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    panel.grid.update(cx, |table, cx| {
                        table.delegate_mut().commit_edit(cx);
                    });
                    // 【T2a-4】提交完成后再刷新脏格底色（grid 已退出上面的 update，
                    // 这里可安全再 update；投影数据读权威变更集——与真实状态同一口径）
                    let dirty = {
                        let store = panel.shared.results();
                        let active = store.active_index(&panel.document).unwrap_or(0);
                        store
                            .change_set(&panel.document, active)
                            .map(|changes| {
                                changes
                                    .pending()
                                    .iter()
                                    .map(|change| (change.row, change.column))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    };
                    panel.grid.update(cx, |table, cx| {
                        table.delegate_mut().set_dirty(dirty);
                        cx.notify();
                    });
                }
                _ => {}
            },
        );
        // 编辑框实体 + 提交钩子注入网格（提交走钩子交回面板落变更集）
        {
            let edit_weak = edit_input.clone();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_edit_input(edit_weak);
            });
            let shared_for_commit = shared.clone();
            let document_for_commit = document.clone();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_commit_edit_hook(std::rc::Rc::new(
                    move |row, column, original, edited, _cx| {
                        // 【T2a-4】提交落权威变更集：只动 Shared（独立实体，可安全 update）。
                        // 钩子是在网格 delegate 内部（`commit_edit`）被调用的——这里再
                        // update 网格就是重入 panic；脏格重投影由订阅闭包（PressEnter /
                        // Blur 分支）在 commit 完成后做，那里 grid 已退出 update。
                        shared_for_commit.update_results(|store| {
                            let active = store.active_index(&document_for_commit).unwrap_or(0);
                            store.mutate_change_set(&document_for_commit, active, |changes| {
                                if edited == original {
                                    // 值没变（改回来）：清掉这条变更
                                    changes.remove(row, column);
                                } else {
                                    changes.upsert(CellChange::pending(
                                        row, column, original, edited,
                                    ));
                                }
                            });
                        });
                    },
                ));
            });
        }

        // 【B5b】滚动到底自动加载：组件库在可见范围接近末尾时调 delegate 的 `load_more`，
        // 而“该不该再取一段”只有面板知道（选中哪份结果、忙不忙）——走钩子交回给面板。
        // 面板自己的 `fetch_more` 仍是**显式按钮**与自动加载共用的同一条路。
        // 【B14】右键「按值筛选」也走同一套：值写进筛选框并立刻应用（滤镜 UI 在面板这边）。
        {
            let load_weak = cx.entity().downgrade();
            let filter_weak = cx.entity().downgrade();
            let sort_weak = cx.entity().downgrade();
            let insight_weak = cx.entity().downgrade();
            let preview_weak = cx.entity().downgrade();
            // 【M8】宿主没接「洞察此列」端口就不装钩子——菜单据此不摆那一项（能力没有就不给入口）
            let insight_port = shared.insight_column_port();
            grid.update(cx, |state, _cx| {
                state
                    .delegate_mut()
                    .set_load_more_hook(std::rc::Rc::new(move |app: &mut App| {
                        _ = load_weak.update(app, |panel, cx| panel.fetch_more(cx));
                    }));
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
                        _ = sort_weak
                            .update(app, |panel, cx| panel.sort_down(&column, descending, cx));
                    },
                ));
                if insight_port.is_some() {
                    state
                        .delegate_mut()
                        .set_insight_column_hook(std::rc::Rc::new(
                            move |column: &str, app: &mut App| {
                                let column = column.to_string();
                                _ = insight_weak
                                    .update(app, |panel, cx| panel.insight_column(&column, cx));
                            },
                        ));
                }
                // 【遗留 #7】「查看完整值」：弹层是宿主的事（对话框生命周期归宿主），
                // 钩子只把 值 / 列名 / 视图行 带回面板
                let preview_for_value = preview_weak.clone();
                state.delegate_mut().set_preview_value_hook(std::rc::Rc::new(
                    move |value: String, column: String, _column_ix: usize, row: usize, app: &mut App| {
                        _ = preview_for_value
                            .update(app, |panel, cx| panel.open_value_preview(&value, &column, row, cx));
                    },
                ));
                // 【P2-3】列统计：统计文本已由网格算好，弹层复用「查看完整值」的对话框
                // （标题 = 列名，正文 = 统计行）
                let preview_for_stats = preview_weak.clone();
                state.delegate_mut().set_column_stats_hook(std::rc::Rc::new(
                    move |column: String, text: String, app: &mut App| {
                        _ = preview_for_stats
                            .update(app, |panel, cx| panel.open_value_preview(&text, &column, 0, cx));
                    },
                ));
            });
        }

        // 【2d/T2d-5】分析模式：正文是 `.rdsnote` JSON——解析进笔记本持有器（渲染读它）。
        // 空正文 = 新建空笔记；解析失败如实报（状态栏提示，不假装打开成功）。
        if mode == EditorMode::Analysis {
            if notebook_content.trim().is_empty() {
                shared.set_notebook_state(&document, crate::cell::NotebookState::new());
            } else {
                match crate::cell::NotebookFile::from_json(&notebook_content) {
                    Ok(file) => {
                        let (session, cells) = file.into_state();
                        shared.set_notebook_state(
                            &document,
                            crate::cell::NotebookState {
                                session,
                                cells,
                            },
                        );
                    }
                    Err(_reason) => {
                        // 提示卡由面板的消息位承接（构造期没有 cx.notify 目标，先落空态）
                        shared.set_notebook_state(&document, crate::cell::NotebookState::new());
                    }
                }
            }
        }

        let mut panel = Self {
            shared,
            document,
            focus_handle,
            editor,
            statements,
            group: None,
            area: None,
            message: None,
            edit_mode: EditMode::Row,
            closed: false,
            welcome_dismissed: false,
            title_hovered: std::cell::Cell::new(false),
            grid,
            pending: 0,
            running_since: None,
            kernel_running: None,
            kernel_pump: RefCell::new(None),
            notebook_pending: Vec::new(),
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
            result_tabs_scroll: ScrollHandle::default(),
            result_active: 0,
            result_sql: None,
            result_is_analysis: false,
            result_can_copy: false,
            transposed: false,
            transpose_row: 0,
            result_error_card: None,
            script_card: None,
            error_site: None,
            error_site_key: None,
            window: window.window_handle(),
            result_height: std::rc::Rc::new(std::cell::Cell::new(ui::RESULT_PANE_HEIGHT)),
            value_preview: None,
            csv_active: false,
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
            scanner: crate::scan::Scanner::new(),
            scan_revision: 0,
            scan_pending: false,
            scan_diagnostics: Vec::new(),
            error_reason: None,
            _editor_sub: Some(sub),
            _grid_sub: Some(grid_sub),
            _filter_sub: Some(filter_sub),
            _edit_sub: Some(_edit_sub),
            commit_in_flight: None,
            toolbar: crate::toolbar::ToolbarLayout::default(),
            invisible_decorations: None,
            lsp: None,
            lsp_version: 0,
            lsp_alive: false,
            lsp_diagnostics: Vec::new(),
            cell_editors: std::collections::HashMap::new(),
            _cell_editor_subs: std::collections::HashMap::new(),
            markdown_preview: false,
        };
        // 【LSP】建面板即向宿主要一次服务（有 → didOpen + 挂 provider；无 → 如实不接）
        panel.sync_lsp_session(cx);
        // 【LSP】新会话的 provider 装配：`new` 里的内联 install 只按 SQL 能力装，
        // 通用模式的 LSP provider 走这里（`sync_mode` 只在切模式时跑，新建不经过它）
        panel.editor.update(cx, |state, _cx| panel.sync_completion_provider(state));
        // 【遗留 #14】CSV 表格展示：打开即把内容解析进结果网格（空文本也是合法 CSV，
        // 给一个空表而非不显示）
        panel.sync_csv_grid(cx);
        // 【遗留 #10】单元编辑器**不在构造期创建**：构造上下文里 `cx.new` + 订阅会
        // 触发「面板正在被更新」重入 panic。建实例走事件路径——`on_added_to`
        // （面板进 Dock）与单元增删方法。
        panel
    }

    pub fn document(&self) -> &DocumentId {
        &self.document
    }

    /// 当前文档是否脏（与基线不同）
    pub fn is_dirty(&self) -> bool {
        self.with_document(Document::is_dirty).unwrap_or(false)
    }

    /// 注入所在 `DockArea` 的弱句柄（宿主在 `add_panel_view` 前调用）。
    ///
    /// 脏 tab 的自绘 ✕（`title_suffix`）弹「保存/不保存/取消」要用它走
    /// [`request_close_document`]；不注入则 ✕ 退化为“点了没反应”（弱句柄为空时
    /// 回调安全忽略，不 panic）。
    pub fn set_area(&mut self, area: WeakEntity<DockArea>) {
        self.area = Some(area);
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
    ///
    /// 切标签必须**离开本面板的 update**：`select_tab` 内部会经
    /// `active_panel() → visible()` 同步 `read` 回本面板（`panel.rs` 的 `Entity::read`），
    /// 而 `focus_self` 永远跑在本面板自己的 `update` 闭包里——此时直接调会触发
    /// gpui 的 double-lease panic（“cannot read … while it is already being updated”）。
    /// `spawn_in` 把动作放进异步任务：在**当前帧所有 effect 处理完之后**才运行，
    /// 面板的更新租约早已归还，`select_tab` 读面板是安全的。
    pub fn focus_self(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .update(|service| service.activate(&self.document));
        let Some(group) = self.group.clone() else {
            return;
        };
        let id = PanelId::from(cx.entity_id());
        cx.spawn_in(window, async move |this, cx| {
            _ = cx.update(|window, cx| {
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
                    // 组已不在（面板被拖到另一个组 / 整棵 Dock 被重建）：动作不落地，但不静默。
                    // 任务里面板不在更新中，这里可以安全地更新它。
                    this.update(cx, |panel, cx| {
                        panel.set_message(Some("面板不在标签组里，无法激活".to_string()), cx)
                    })
                    .ok();
                }
            });
        })
        .detach();
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
        let (path, mode, channel, connection, schema) = self.with_document(|doc| {
            let path = doc.path()?.to_string_lossy().into_owned();
            Some((
                path,
                doc.mode(),
                doc.channel(),
                doc.connection().map(str::to_string),
                doc.schema().map(str::to_string),
            ))
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
            // 【T2c-6】工具栏布局：面板权威状态的编码串（改动即存，不设“值得存”阈值）
            toolbar: Some(self.toolbar.encode()),
            // 【T2c-1 余项】schema 绑定随会话走（`None` = 未选 → 连接默认）
            schema,
            // 【T2c-4 余项】缩进随会话走（文档级权威值）
            tab_size: Some(self.shared.document_tab_size(&self.document)),
            hard_tabs: Some(self.shared.document_hard_tabs(&self.document)),
            // 【遗留 #5】结果区分栏高度随会话走（用户拖出来的比例是布局的一部分）
            result_height: Some(self.result_height.get()),
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
        let start = clamp(
            session
                .selection
                .map(|(start, _)| start)
                .unwrap_or(session.cursor),
        );
        let end = clamp(
            session
                .selection
                .map(|(_, end)| end)
                .unwrap_or(session.cursor),
        );

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
        // 【T2c-6】工具栏布局：旧会话没存（`None`）或解码失败 → 保持默认，不覆盖当前
        if let Some(code) = session.toolbar.as_deref() {
            if let Some(layout) = crate::toolbar::ToolbarLayout::decode(code) {
                self.toolbar = layout;
            }
        }
        // 【T2c-1 余项】schema 绑定：必须在连接恢复之后（schema 列表按连接查）
        self.restore_schema(session.schema.as_deref(), cx);
        // 【T2c-4 余项】缩进双写（文档状态 + 内核 TabSize）；先宽度后制表符（后者读前者）
        if let Some(size) = session.tab_size {
            self.set_tab_size(size, cx);
        }
        if let Some(hard) = session.hard_tabs {
            self.set_hard_tabs(hard, cx);
        }
        // 【遗留 #5】结果区分栏高度：旧会话没存（`None`）→ 保持默认，不覆盖当前；
        // 值钳到合法区间（与拖拽条 `size_range` 同一套上下界）
        if let Some(height) = session.result_height {
            self.result_height.set(height.clamp(ui::RESULT_MIN_HEIGHT, ui::RESULT_MAX_HEIGHT));
        }
        cx.notify();
    }

    /// 【T2c-1 余项】把会话里的 **schema 绑定**写回文档
    ///
    /// 判据同 `restore_connection`：schema 列表里没有 = schema 被删 / 换库 → **回退到
    /// 「使用连接默认」并把原因说出来**。schema 列表按**当前连接**查（连接恢复在前）。
    /// 端口未接 / 连接未绑 = **不校验也不丢**（宁可不猜也不把用户的绑定抹掉）。
    fn restore_schema(&mut self, schema: Option<&str>, cx: &mut Context<Self>) {
        let Some(schema_name) = schema else {
            return;
        };
        let conn = self.bound_connection();
        let list = self.shared.schemas(conn.as_deref());
        let apply = |service: &mut crate::service::EditorService| {
            service.set_schema(&self.document, Some(schema_name.to_string()))
        };
        if list.is_empty() {
            // 端口未接 / 连接未绑：不校验也不丢（与 restore_connection 同语义）
            self.shared.update(apply);
            return;
        }
        if list.iter().any(|info| info.schema == schema_name) {
            self.shared.update(apply);
            return;
        }
        self.shared.update(|service| {
            service.set_schema(&self.document, None)
        });
        self.set_message(
            Some(format!(
                "schema {schema_name} 不在当前连接里：已回退到连接默认"
            )),
            cx,
        );
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
            Some(format!(
                "{}通道已失效：{reason}（回退源库）",
                channel.label()
            )),
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

    /// 【A4】本文档的编辑内核上是否装了 SQL 语义着色。
    ///
    /// 断言的是**内核里那个 provider**（不是我们自己的副本）：着色只有「装上了」才会被内核
    /// 在可视区拉取并用活跃主题解析——拆掉 `highlight::install` 就会假起来。
    pub fn highlight_installed_for_test(&self, cx: &App) -> bool {
        self.editor
            .read(cx)
            .lsp()
            .semantic_tokens_provider
            .is_some()
    }

    /// 【遗留 #5】结果区分栏高度（rem；测试断言「会话恢复带高度回来」用）
    pub fn result_height_for_test(&self) -> f32 {
        self.result_height.get()
    }

    /// 【A4】装上内核的那个着色器声明的 token 名（供测试断言「装的是 SQL 着色器」）
    ///
    /// 名字必须落在活跃主题的词汇里，否则内核会静默跳过该 token——所以这份图例就是「染不染色」
    /// 的对外契约（见 `crate::view::highlight::TOKEN_NAMES`）。
    pub fn highlight_legend_for_test(&self, cx: &App) -> Vec<String> {        self.editor
            .read(cx)
            .lsp()
            .semantic_tokens_provider
            .as_ref()
            .map(|provider| {
                provider
                    .legend()
                    .token_types
                    .iter()
                    .map(|name| name.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default()
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

    /// 工具栏（原型 §2.2 + T2c-5 分组自定义）：按其分层只放**今天真有动作**的控件
    ///
    /// - 最左：**模式指示器**（文本 / SQL / 分析，点击切换）——固定项，不参与
    ///   分组 / 排序 / 可见性（原型 ⑯）。
    /// - 左区：按 [`crate::toolbar`] 的分组模型渲染（组 = 分隔符界定的连续段；折叠组
    ///   显示为「首组件 + ▾」，▾ 收纳组内其余可见成员；展开组逐可见项渲染）。
    ///   **只在 SQL 模式出现**：文本模式按能力表不通信，分析模式的执行属笔记级动作
    ///   （1c 随单元落地）。
    /// - 右对齐簇（执行位置 + 连接）：T2c-5 里固定在最右，不参与分组移动
    ///   （右对齐语义稳定；T2c-6 随会话持久化时再放开）。
    /// - 最右：**`···` 定制按钮**——打开「工具栏自定义」面板（可见性 / 折叠 / 移动 /
    ///   竖线分隔符 / 重置）。
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
                    // 原型 ⑯：图标 + 模式名 + ▾（图标由 toolbar 层的纯函数给出）
                    .icon(crate::toolbar::mode_icon(mode))
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

        // 【核对】工具栏形态以能力表为唯一分叉（不再按 mode 硬编码）：Full = SQL 组列表 +
        // 执行位置 / 连接；Minimal = 环境 + 脚本运行；Notebook = 笔记级动作 1c 留白。
        match crate::model::Capabilities::for_mode(mode).toolbar {
            crate::model::ToolbarStyle::Full => {
            // 左区：组列表（T2c-5）。只有 SQL 模式的执行是“对当前文档执行”：
            // 文本模式按能力表不通信，分析模式的执行是**笔记级**动作（1c 落地）。
            let groups = self.toolbar.groups();
            for group in &groups {
                toolbar = toolbar.child(self.render_toolbar_group(group, cx));
            }
            // 通道级 + 连接（右对齐）：原型 §2.2 两栏——“执行位置”在“连接”左边；
            // T2c-5 固定不动（右对齐语义稳定）。
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
            // 最右：`···` 定制面板（布局自定义；低频动作已全部提升常驻，见原型 §2.2 修订）
            toolbar = toolbar.child(self.render_toolbar_customize(cx));
            }
            crate::model::ToolbarStyle::Minimal => {
            // 【2b】通用模式：环境选择器 + 运行按钮（无连接档：不摆执行位置 / 连接面包屑）
            // Markdown 文档额外有「预览 / 源码」切换（渲染态是只读视图）。
            toolbar = toolbar.child(
                div()
                    .ml_auto()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(self.is_markdown_document(), |row| {
                        row.child(self.render_markdown_preview_toggle(cx))
                    })
                    .child(self.render_env_picker(cx))
                    .child(self.render_run_script_button(cx)),
            );
            }
            // Notebook：笔记级工具栏（会话 ▾ / 全部运行 / 重启）——1c 落地。
            // 与 SQL 组模型无关：会话绑定（连接）是笔记级事实，执行是顺序跑全部单元。
            crate::model::ToolbarStyle::Notebook => {
                toolbar = toolbar.child(
                    div()
                        .ml_auto()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(self.render_notebook_session_button(cx))
                        .child(self.render_notebook_run_all_button(cx))
                        .child(self.render_notebook_restart_button(cx)),
                );
            }
        }
        toolbar
    }

    /// 【2d-笔记级】会话 ▾ 按钮：绑定连接的切换入口（笔记级事实）。
    ///
    /// 菜单 = 连接列表（当前会话绑定勾选）+「跟随当前活动连接」；空列表时保持
    /// 「跟随当前活动连接」可点（与 SQL 模式 B1 同口径，不假装有连接）。
    fn render_notebook_session_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let options = self.shared.connection_options();
        let current = self.notebook_state().session.default_connection.clone();
        // 会话绑定连接展示名（`None` = 跟随当前活动连接）
        let label = match &current {
            Some(id) => options
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("连接 {id}")),
            None => "跟随当前连接".to_string(),
        };
        Button::new("editor-notebook-session")
            .ghost()
            .small()
            .debug_selector(|| "editor-notebook-session".to_string())
            .icon(IconName::Globe)
            .tooltip("会话连接")
            .label(format!("{label} ▾"))
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                // 第一项：跟随当前活动连接（默认态）
                let follow = PopupMenuItem::new("跟随当前活动连接")
                    .checked(current.is_none())
                    .on_click({
                        let entity = entity.clone();
                        move |_, _window, app| {
                            entity.update(app, |panel, cx| {
                                panel.notebook_switch_connection(None, cx);
                            });
                        }
                    });
                menu = menu.item(follow);
                for conn in &options {
                    let selected = current.as_deref() == Some(conn.id.as_str());
                    let item = PopupMenuItem::new(conn.name.clone())
                        .checked(selected)
                        .on_click({
                            let entity = entity.clone();
                            let conn_id = conn.id.clone();
                            move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.notebook_switch_connection(Some(conn_id.clone()), cx);
                                });
                            }
                        });
                    menu = menu.item(item);
                }
                menu
            })
    }

    /// 【2d-笔记级】全部运行按钮：顺序跑所有可执行单元（SQL / 内核）。
    fn render_notebook_run_all_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("editor-notebook-run-all")
            .ghost()
            .small()
            .debug_selector(|| "editor-notebook-run-all".to_string())
            .icon(IconName::Play)
            .tooltip("全部运行")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.notebook_run_all(cx));
            })
    }

    /// 【2d-笔记级】重启按钮：关闭活跃内核（下次执行自动重拉）。
    fn render_notebook_restart_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("editor-notebook-restart")
            .ghost()
            .small()
            .debug_selector(|| "editor-notebook-restart".to_string())
            .icon(IconName::RefreshCw)
            .tooltip("重启内核")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.notebook_restart_kernel(cx));
            })
    }

    /// 当前文档是否为 Markdown（通用模式下的 `.md` 脚本类型）。决定「预览」切换是否出现。
    fn is_markdown_document(&self) -> bool {
        let Some(path) = self.document_path() else {
            return false;
        };
        crate::mode::script_file_type(&path) == Some(crate::mode::ScriptFileType::Markdown)
    }

    /// 【遗留】Markdown 文档「预览 / 源码」切换按钮（通用模式工具栏右侧）。
    /// 预览态 = 只读渲染视图（TextView::markdown），源码态 = 内核编辑器。
    fn render_markdown_preview_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let previewing = self.markdown_preview;
        Button::new("editor-markdown-preview-toggle")
            .ghost()
            .small()
            // 测试按选择器断言「预览态下渲染视图出现 / 源码态下编辑器出现」
            .debug_selector(|| "editor-markdown-preview-toggle".to_string())
            .label(if previewing { "源码" } else { "预览" })
            .toggled(previewing)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.toggle_markdown_preview(cx));
            })
    }

    /// 切换 Markdown 渲染态：预览 ↔ 源码编辑器（纯状态切换，渲染由 render 分支体现）。
    pub(crate) fn toggle_markdown_preview(&mut self, cx: &mut Context<Self>) {
        self.markdown_preview = !self.markdown_preview;
        cx.notify();
    }

    /// 【2b】通用模式的环境选择器（工具栏右侧）：环境名 + 版本 ▾；未接端口 / 空列表如实置灰。
    fn render_env_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let envs = self.envs();
        let current = self.env_for();
        // 装没装是 EnvPort 的 `installed`（命令可达），在闭包外一次算好（闭包是 `move`）
        let env_installed_states: std::collections::HashMap<String, bool> = envs
            .iter()
            .map(|env| (env.id.clone(), self.shared.env_installed(&env.id)))
            .collect();
        let label = match &current {
            Some(id) => envs
                .iter()
                .find(|env| env.id == *id)
                .map(|env| env.display_name())
                .unwrap_or_else(|| format!("{id}（不在列表）")),
            None => "选择环境 ▾".to_string(),
        };
        Button::new("editor-env-picker")
            .ghost()
            .small()
            .debug_selector(|| "editor-env-picker".to_string())
            .label(label)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for env in &envs {
                    let id = env.id.clone();
                    let entity = entity.clone();
                    let checked = Some(id.as_str()) == current.as_deref();
                    let installed = env_installed_states.get(env.id.as_str()).copied().unwrap_or(false);
                    let mut item = PopupMenuItem::new(env.display_name())
                        .checked(checked)
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| {
                                panel.set_env(Some(id.clone()), cx);
                            });
                        });
                    if !installed {
                        item = item.disabled(true);
                    }
                    menu = menu.item(item);
                }
                // 分隔 + 「管理环境…」：列表可能为空（没探测到任何解释器），
                // 入口**不置灰**——这正是新增自定义环境的那条路。
                if !envs.is_empty() {
                    menu = menu.separator();
                }
                let manage_entity = entity.clone();
                let manage = PopupMenuItem::new("管理环境…")
                    .on_click(move |_, window, app| {
                        manage_entity.update(app, |panel, cx| {
                            panel.open_manage_environments(window, cx);
                        });
                    });
                menu = menu.item(manage);
                menu
            })
    }

    /// 【2b】通用模式的运行按钮：未选环境 / 环境未装 / 只读 → 置灰 + 悬停原因。
    fn render_run_script_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let enabled = self
            .env_for()
            .map(|id| self.shared.env_installed(&id) && !self.editor_read_only())
            .unwrap_or(false);
        Button::new("editor-run-script")
            .small()
            .debug_selector(|| "editor-run-script".to_string())
            .label("▶ 运行")
            .when(!enabled, |b| b.disabled(true))
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.run_script(cx));
            })
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
                    menu = menu.separator().item(PopupMenuItem::new(label).on_click(
                        move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.refresh_source(cx));
                        },
                    ));
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
        self.with_document(|doc| doc.channel()).unwrap_or_default()
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
            message.push_str(&format!("；{} 条解析不了，原样保留", plan.kept_verbatim));
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
            message.push_str(&format!("；{} 条解析不了，原样保留", plan.kept_verbatim));
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

    /// 【T2c-1】切换文档绑定的 schema（`None` = 用连接默认 schema）
    pub(crate) fn set_schema(&mut self, schema: Option<String>, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .update(|service| service.set_schema(&self.document, schema));
        if changed {
            cx.notify();
        }
    }

    /// 【T2c-4】重设文档缩进宽度：文档级状态 + 内核 `set_tab_size` 双写（Tab 键缩进真变）
    pub(crate) fn set_tab_size(&mut self, size: usize, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .set_document_tab_size(&self.document, size);
        if changed {
            // 内核 TabSize 随文档状态走（0.7.1 `set_tab_size` 公开在 EditorState 别名上）
            self.editor
                .update(cx, |state, cx| {
                    state.set_tab_size(
                        gpui_kit::component::input::TabSize {
                            tab_size: size,
                            hard_tabs: false,
                        },
                        cx,
                    );
                });
            cx.notify();
        }
    }

    /// 【T2c-4】重设缩进用制表符还是空格：文档状态 + 内核 TabSize 双写
    pub(crate) fn set_hard_tabs(&mut self, hard: bool, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .set_document_hard_tabs(&self.document, hard);
        if changed {
            let size = self.shared.document_tab_size(&self.document);
            self.editor
                .update(cx, |state, cx| {
                    state.set_tab_size(
                        gpui_kit::component::input::TabSize {
                            tab_size: size,
                            hard_tabs: hard,
                        },
                        cx,
                    );
                });
            cx.notify();
        }
    }

    /// 【2c】重设文档保存编码（UTF-8 / UTF-8 (BOM) / GBK / UTF-16 LE / UTF-16 BE）
    ///
    /// 只改**保存契约**：内核文本永远是 Unicode，切换编码不会实时重新解码（无从重解码），
    /// 只让下次保存按新编码产出字节。`is_dirty` 由 `format != saved_format` 得出，切换即标脏。
    pub(crate) fn set_encoding(
        &mut self,
        encoding: crate::encoding::TextEncoding,
        cx: &mut Context<Self>,
    ) {
        let current = self.shared.document_save_format(&self.document);
        if current.encoding == encoding {
            return;
        }
        let changed = self
            .shared
            .set_document_save_format(&self.document, current.with_encoding(encoding));
        if changed {
            cx.notify();
        }
    }

    /// 【2c】重设文档保存换行符（LF / CRLF）
    ///
    /// 同上：只改保存契约；load 时 CRLF 原样进内核（编辑语义不变），保存时才按这里
    /// 的 eol 统一生成。切换即标脏（会产出不同字节）。
    pub(crate) fn set_eol(&mut self, eol: crate::encoding::LineEnding, cx: &mut Context<Self>) {
        let current = self.shared.document_save_format(&self.document);
        if current.eol == eol {
            return;
        }
        let changed = self
            .shared
            .set_document_save_format(&self.document, current.with_eol(eol));
        if changed {
            cx.notify();
        }
    }

    /// 【2c】显示空白字符（空格 `•` / 制表符 `→`）：文档状态 + 内核 `set_show_whitespaces` 双写
    pub(crate) fn set_show_whitespaces(
        &mut self,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self
            .shared
            .set_document_show_whitespaces(&self.document, show);
        if changed {
            self.editor.update(cx, |state, cx| {
                state.set_show_whitespaces(show, window, cx);
            });
            cx.notify();
        }
    }

    /// 【2c】显示隐藏字符标注（零宽 / 全角空格 / 控制字符）：文档状态 + 内核 RangeDecoration 双写
    ///
    /// 启用时懒创建标注集合（`.set` 会把当前正文里所有隐藏字符刷一遍）；停用时清空集合。
    pub(crate) fn set_show_invisible(&mut self, show: bool, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .set_document_show_invisible(&self.document, show);
        if !changed {
            return;
        }
        if show {
            if self.invisible_decorations.is_none() {
                // 懒创建：句柄借内核实体，面板 drop 随内核一起释放
                self.invisible_decorations = Some(self.editor.update(cx, |state, cx| {
                    state.create_range_decorations_collection(Vec::new(), cx)
                }));
            }
            let ranges = crate::hidden::hidden_ranges(&self.editor_text(cx));
            self.refresh_invisible_decorations(ranges, cx);
        } else if let Some(collection) = &self.invisible_decorations {
            collection.clear(cx);
        }
        cx.notify();
    }

    /// 【2c】把当前隐藏字符区间刷进标注集合（`None` = 没开显示，什么都不做）
    fn refresh_invisible_decorations(
        &mut self,
        ranges: Vec<std::ops::Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let Some(collection) = &self.invisible_decorations else {
            return;
        };
        let color = cx.theme().colors.warning.opacity(0.3);
        let decorations = ranges
            .into_iter()
            .map(|range| {
                RangeDecoration::new(range)
                    .with_style(RangeDecorationStyle::Fill)
                    .with_color(color)
            })
            .collect::<Vec<_>>();
        collection.set(decorations, cx);
    }

    /// 【T2c-3 收尾】切换编辑内容模式（行编辑 / 列编辑）。只改面板态并重绘；
    /// 切回行编辑时的多光标收拢由调用方（菜单 on_click 持有 window）经 [`Self::enforce_edit_mode`] 触发。
    pub(crate) fn set_edit_mode(&mut self, mode: EditMode, cx: &mut Context<Self>) {
        if self.edit_mode == mode {
            return;
        }
        self.edit_mode = mode;
        cx.notify();
    }

    /// 【T2c-3 收尾】按当前编辑模式对齐内核光标：行编辑 = 强制单光标（多光标收拢到活动光标）；
    /// 列编辑 = 保持多光标。需要 `window`（内核 `collapse_cursors` 的签名要求，实参未用）。
    pub(crate) fn enforce_edit_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit_mode != EditMode::Row {
            return;
        }
        self.editor.update(cx, |state, cx| {
            if state.cursor_count() > 1 {
                state.collapse_cursors(window, cx);
            }
        });
    }

    /// 【2b】绑定 / 解绑本文档的执行环境（`None` = 未选 → 运行不亮）
    pub(crate) fn set_env(&mut self, env: Option<String>, cx: &mut Context<Self>) {
        self.shared
            .update(|service| service.set_env(&self.document, env));
        cx.notify();
    }

    /// 【2b】本文档选中的执行环境 id（`None` = 未选）
    pub(crate) fn env_for(&self) -> Option<String> {
        self.shared.service().env_for(&self.document)
    }

    /// 【2b】可用环境列表（宿主注入的 EnvPort；未接端口 = 空，如实）
    fn envs(&self) -> Vec<crate::env::Environment> {
        self.shared.envs()
    }

    /// 【2b】打开「管理环境」对话框（环境配置表：增删改路径 / 参数）
    ///
    /// 增删改经端口直呼宿主（全局库 `script_environments` 表）；删除成功后回调
    /// [`Self::delete_env`]：把文档里选中了这条环境的绑定清掉，选择器不会显示一条
    /// 已不存在的 id。
    pub(crate) fn open_manage_environments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(port) = self.shared.env_port() else {
            self.set_message(Some("环境端口未接入".to_string()), cx);
            return;
        };
        let entity = cx.entity();
        crate::view::manage_env::open_manage_environments(window, cx, port, {
            let entity = entity.clone();
            move |id, window, app| {
                entity.update(app, |panel, cx| panel.delete_env(&id, window, cx));
            }
        });
    }

    /// 【2b】删除环境后的收尾：本文档正选中这条 → 解绑（选择器回「选择环境」）
    pub(crate) fn delete_env(&mut self, id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        if self.env_for().as_deref() == Some(id) {
            self.set_env(None, cx);
        }
    }

    /// 【2b】运行脚本：把**当前文档内容**交给选中的环境执行（spawn + stdin + 超时 + 杀进程树
    /// 都在执行器侧）。缺环境 / 未选中 / 没内容 / 只读都如实拒绝，不假装能跑。
    pub(crate) fn run_script(&mut self, cx: &mut Context<Self>) {
        let Some(env_id) = self.env_for() else {
            self.set_message(Some("未选中执行环境——先在工具栏选一个".to_string()), cx);
            return;
        };
        if !self.shared.env_installed(&env_id) {
            self.set_message(Some("所选环境未安装（命令不可达）".to_string()), cx);
            return;
        }
        if self.editor_read_only() {
            self.set_message(Some("文档只读，不能执行脚本".to_string()), cx);
            return;
        }
        let code = self.editor_text(cx);
        if code.trim().is_empty() {
            self.set_message(Some("文档是空的，没有可执行的脚本".to_string()), cx);
            return;
        }
        let target = ExecTarget::Script(crate::env::ScriptRequest { env_id, code });
        match self
            .shared
            .submit(self.document.clone(), &target, ResultPlacement::NewSet, execution::RunOptions::default())
        {
            Ok(()) => {
                if self.pending == 0 {
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += 1;
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
            }
            Err(error) => self.set_message(Some(error.message().to_string()), cx),
        }
    }

    /// 【2b】收脚本执行的结论（主线程轮询取走）：变成一张输出卡片钉在结果区
    pub(crate) fn drain_script_notes(&mut self, cx: &mut Context<Self>) {
        let mut mine_arrived = 0usize;
        for note in self.shared.drain_script_notes() {
            if note.document != self.document {
                continue;
            }
            mine_arrived += 1;
            let env_name = self
                .envs()
                .into_iter()
                .find(|env| env.id == note.env_id)
                .map(|env| env.display_name())
                .unwrap_or_else(|| note.env_id.clone());
            let card = match note.result {
                Ok(outcome) => crate::view::results::script_card::ScriptCard {
                    env_name,
                    result: Ok(crate::view::results::script_card::ScriptOutcomeView {
                        stdout: outcome.stdout.clone(),
                        stderr: outcome.stderr.clone(),
                        exit_code: outcome.exit_code,
                        timed_out: outcome.timed_out,
                        elapsed_ms: self
                            .running_since
                            .map(|s| s.elapsed().as_millis() as u64)
                            .unwrap_or(0),
                    }),
                },
                Err(reason) => crate::view::results::script_card::ScriptCard {
                    env_name,
                    result: Err(reason),
                },
            };
            self.script_card = Some(card);
        }
        // 脚本回执不走 `drain_exec`（它是独立通道），所以 `pending` 在这里按本文档的份数收口
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
            }
        }
        cx.notify();
    }

    // ===== 【2d】分析笔记本：单元动作（状态在 `EditorShared` 的笔记本持有器） =====

    /// 【遗留 #10】把笔记本现有单元的编辑器实例补齐（面板进 Dock / 测试入口调用；
    /// 渲染期只读不建实体，创建一律走事件路径——构造期建会重入 panic）
    pub(crate) fn sync_cell_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document_mode() != Some(EditorMode::Analysis) {
            return;
        }
        let cells = self.notebook_state().cells;
        for cell in &cells {
            self.ensure_cell_editor(cell, window, cx);
        }
    }

    /// 【遗留 #10】为一个单元确保内核编辑器实例存在（无则创建 + 订阅 Change → 回写正文）
    ///
    /// 订阅挂在单元编辑器实体上：Change 事件从它发出，面板此刻不在更新中，
    /// 回写 shared 不会重入（gpui-kit-dev「不要在回调里重入 update 当前实体」）。
    fn ensure_cell_editor(&mut self, cell: &crate::cell::Cell, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_editors.contains_key(&cell.id) {
            return;
        }
        let body = cell.body.clone();
        // 内核编辑器语言 = 单元语言的持久化键（EditorState::language 吃 "sql"/"markdown" 等）
        let language = cell.language.as_key();
        let editor = cx.new(|cx| {
            // `language` 是消费式 builder：链在 `new` 后面
            let mut state = EditorState::new(window, cx).language(language);
            state.set_value(body, window, cx);
            state
        });
        let document = self.document.clone();
        let shared = self.shared.clone();
        let cell_id = cell.id.clone();
        // 订阅挂面板上下文会 read 面板（subscribe_in 的 ensure_window），面板正被
        // update 时直接 panic；改用无窗口版 `subscribe`（回调里不需要 window，
        // 只回写 shared）——派发时面板已不在更新中
        let sub = cx.subscribe(
            &editor,
            move |_this, state, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // 正文从单元编辑器实体读（事件路径，可直接 read）
                let text = state.read(cx).text().to_string();
                let mut cells = shared.notebook_cells(&document);
                if let Some(target) = cells.iter_mut().find(|c| c.id == cell_id) {
                    target.body = text;
                }
                shared.set_notebook_cells(&document, cells);
                cx.notify();
            },
        );
        self.cell_editors.insert(cell.id.clone(), editor);
        self._cell_editor_subs.insert(cell.id.clone(), sub);
    }

    /// 取本文档的笔记状态（渲染用快照）
    pub(crate) fn notebook_state(&self) -> crate::cell::NotebookState {
        self.shared.notebook_state(&self.document)
    }

    /// 新增一个单元（追加到序列尾部；新建即 Idle，执行源默认 = 会话）
    pub(crate) fn notebook_add_cell(
        &mut self,
        _document: &DocumentId,
        language: crate::cell::CellLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let id = format!(
            "cell-{}-{}",
            state.cells.len() + 1,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        );
        let body = match language {
            crate::cell::CellLanguage::Sql => "select 1;".to_string(),
            crate::cell::CellLanguage::Markdown => String::new(),
            // Python / R / Julia：给一段最小可执行样板，用户一进来就知道能跑
            crate::cell::CellLanguage::Python => "print('hello')".to_string(),
            crate::cell::CellLanguage::R => "print(1 + 1)".to_string(),
            crate::cell::CellLanguage::Julia => "println(1 + 1)".to_string(),
        };
        state.cells.push(crate::cell::Cell::new(id, language, body));
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】新单元同步建内核编辑器（编辑能力随单元出现就位）
        if let Some(last) = self.notebook_state().cells.last() {
            let cell = last.clone();
            self.ensure_cell_editor(&cell, window, cx);
        }
        cx.notify();
    }

    /// 【#16】重设单元执行源（连接 / 内核；`None` = 会话默认）；同值 = 无操作
    pub(crate) fn notebook_set_execution_source(
        &mut self,
        _document: &DocumentId,
        id: &str,
        source: Option<crate::cell::ExecutionSource>,
        cx: &mut Context<Self>,
    ) {
        if self
            .shared
            .set_cell_execution_source(&self.document, id, source)
        {
            cx.notify();
        }
    }

    /// 删除一个单元（按 id；不存在 = 忽略）
    pub(crate) fn notebook_delete_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        state.cells.retain(|cell| cell.id != id);
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】编辑器实例与订阅一并丢弃（句柄即生命周期）
        self.cell_editors.remove(id);
        self._cell_editor_subs.remove(id);
        cx.notify();
    }

    /// 上移 / 下移一个单元（`delta` = ±1；到边界不动）
    pub(crate) fn notebook_move_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        delta: isize,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(pos) = state.cells.iter().position(|cell| cell.id == id) else {
            return;
        };
        let target = pos as isize + delta;
        if target < 0 || target >= state.cells.len() as isize {
            return; // 到边界不动（如实）
        }
        state.cells.swap(pos, target as usize);
        self.shared.set_notebook_cells(&self.document, state.cells);
        cx.notify();
    }

    /// 复制一个单元（副本插在原单元后面；新 id，正文 / 类型原样，输出清空）
    pub(crate) fn notebook_duplicate_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(pos) = state.cells.iter().position(|cell| cell.id == id) else {
            return;
        };
        let original = state.cells[pos].clone();
        let mut copy = crate::cell::Cell::new(
            format!("{}-copy", original.id),
            original.language,
            original.body,
        );
        // 副本继承执行源（复制的就是同一语言的同一执行目标）
        copy.execution_source = original.execution_source.clone();
        state.cells.insert(pos + 1, copy);
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】副本同步建编辑器实例（新 id）
        let copy = self.notebook_state().cells[pos + 1].clone();
        self.ensure_cell_editor(&copy, window, cx);
        cx.notify();
    }

    /// SQL ↔ 文本互转（正文原样，类型翻转；有输出则清空并标 Idle）
    pub(crate) fn notebook_convert_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(cell) = state.cell_mut(id) else {
            return;
        };
        // SQL ↔ Markdown 互转（其余语言本版本不做 convert：python 单元要换执行目标，
        // 属于 #17 的职责）
        cell.language = if cell.language == crate::cell::CellLanguage::Sql {
            crate::cell::CellLanguage::Markdown
        } else {
            crate::cell::CellLanguage::Sql
        };
        cell.execution_source = None;
        cell.state = crate::cell::CellState::Idle;
        cell.outputs.clear();
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】类型翻转 → 编辑器语言跟着变：丢弃旧实例重建（正文原样由
        // `ensure_cell_editor` 从 shared 读，不需要手动搬运）
        self.cell_editors.remove(id);
        self._cell_editor_subs.remove(id);
        if let Some(cell) = self.notebook_state().cells.iter().find(|c| c.id == id) {
            let cell = cell.clone();
            self.ensure_cell_editor(&cell, window, cx);
        }
        cx.notify();
    }

    /// 运行一个单元（仅 SQL 单元；Text / 空正文 / 有执行在跑都如实拒绝）
    pub(crate) fn notebook_run_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        if state.has_running() {
            self.set_message(Some("已有单元在执行中".to_string()), cx);
            return;
        }
        // 会话 id 先克隆出来（`cell_mut` 会可变借用 state，不能同时借 `state.session`）
        let session_id = state.session.id.clone();
        // 判据先读出来、放开借用（内核分支要在 shared 上另起读，不能挂在同一个借用上）
        let (is_executable, is_sql, is_empty) = {
            let Some(cell) = state.cell_mut(id) else {
                return;
            };
            (
                cell.language.is_executable(),
                cell.language == crate::cell::CellLanguage::Sql,
                cell.body.trim().is_empty(),
            )
        };
        if !is_executable {
            self.set_message(Some("Markdown 单元不可执行（只能做备注）".to_string()), cx);
            return;
        }
        // SQL 走执行链（跑在连接上：单元指定连接优先，否则会话默认）；
        // Python / R / Julia 走内核执行（KernelSession，见 `run_kernel_cell`）
        if !is_sql {
            self.run_kernel_cell(&session_id, id, cx);
            return;
        }
        if is_empty {
            self.set_message(Some("这个 SQL 单元是空的，没有可执行的语句".to_string()), cx);
            return;
        }
        let Some(cell) = state.cell_mut(id) else {
            return;
        };
        // 执行源指定的连接（None = 文档 / 会话默认）
        let conn_id = match &cell.execution_source {
            Some(crate::cell::ExecutionSource::Connection(id)) => Some(id.clone()),
            _ => None,
        };
        let sql = cell.body.clone();
        cell.mark_running();
        let request = crate::cell::CellRequest {
            session_id,
            cell_id: id.to_string(),
            sql,
            conn_id,
        };
        self.shared.set_notebook_cells(&self.document, state.cells);
        match self.shared.submit_cell(self.document.clone(), &request) {
            Ok(()) => {
                if self.pending == 0 {
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += 1;
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => {
                // 提交被拒：把 Running 标回去（不假装在跑）
                let mut state = self.notebook_state();
                if let Some(cell) = state.cell_mut(id) {
                    cell.state = crate::cell::CellState::Idle;
                    cell.outputs.clear();
                }
                self.shared.set_notebook_cells(&self.document, state.cells);
                self.set_message(Some(reason), cx);
            }
        }
        cx.notify();
    }

    /// 【2d-笔记级】切换分析会话（绑定连接 / 环境）：换绑定 + 全部输出标 stale。
    ///
    /// `conn_id` = 新的默认连接（`None` = 跟随当前活动连接，与 SQL 模式 B1 同口径）；
    /// 环境维度暂不在此菜单改（分析环境跟随通用模式解释器，见 `env` 端口）。
    pub(crate) fn notebook_switch_connection(
        &mut self,
        conn_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        // 防并发：有单元在跑时切换会话，回执会因会话不匹配而作废（输出会被清），
        // 不如提前拒绝、让用户先中断。
        if self.notebook_state().has_running() {
            self.set_message(Some("有单元在执行中，先中断再切换会话".to_string()), cx);
            return;
        }
        let mut session = self.notebook_state().session;
        session.default_connection = conn_id;
        self.shared
            .switch_notebook_session(&self.document, session);
        self.set_message(None, cx);
        cx.notify();
    }

    /// 【2d-笔记级】全部运行：按顺序跑所有**可执行**单元（SQL / 内核）。
    ///
    /// 执行队列一次只跑一个（`ExecQueue` busy 检查），所以这里**只入队 + 提交队首**，
    /// 跑完一个（SQL 回执 / 内核收口）由 [`Self::advance_notebook_queue`] 自动提交下一个。
    /// 有单元在跑 → 拒绝；Markdown 备注 / 空单元不入队。
    pub(crate) fn notebook_run_all(&mut self, cx: &mut Context<Self>) {
        if self.notebook_state().has_running() || !self.notebook_pending.is_empty() {
            self.set_message(Some("已有单元在执行中".to_string()), cx);
            return;
        }
        let cells = self.notebook_state().cells;
        self.notebook_pending = cells
            .iter()
            .filter(|cell| cell.language.is_executable() && !cell.body.trim().is_empty())
            .map(|cell| cell.id.clone())
            .collect();
        if self.notebook_pending.is_empty() {
            self.set_message(Some("没有可执行的单元（可执行单元要有代码）".to_string()), cx);
            return;
        }
        self.advance_notebook_queue(cx);
    }

    /// 【2d-笔记级】推进「全部运行」队列：队首入执行，跑完一个自动接下一个。
    ///
    /// 守卫（防两个收口点重复推进）：队列空 / 还有单元在跑 → 不动。
    /// 队列走完 = 全部运行完成（消息位清掉，不打扰）。
    fn advance_notebook_queue(&mut self, cx: &mut Context<Self>) {
        if self.notebook_pending.is_empty() || self.notebook_state().has_running() {
            return;
        }
        let next = self.notebook_pending.remove(0);
        // 复用的是单发入口（notebook_run_cell）：它自己会 mark_running + 提交；
        // 此时无 running（上面守卫），不会自锁
        let document = self.document.clone();
        self.notebook_run_cell(&document, &next, cx);
    }

    /// 【2d-笔记级】重启：关闭当前**活跃**内核（下次执行自动重拉）。
    ///
    /// 空 = 没起过内核 → 提示无需重启；正在跑的内核单元也会被一并停掉
    /// （`kernel_running` 作废，回执不再回填）。
    pub(crate) fn notebook_restart_kernel(&mut self, cx: &mut Context<Self>) {
        let active = self.shared.active_kernel_ids();
        if active.is_empty() {
            self.set_message(Some("没有活跃内核，无需重启".to_string()), cx);
            return;
        }
        for id in &active {
            self.shared.kernel_shutdown(id);
        }
        // 正在跑的内核单元：作废运行标记（内核已关，回执不会再回来）
        self.kernel_running = None;
        // 「全部运行」队列一并作废：当前单元被中断，后续单元不该继续自动跑
        self.notebook_pending.clear();
        self.set_message(Some(format!("已重启 {} 个内核", active.len())), cx);
        cx.notify();
    }

    /// 【#17 → #20】跑一个内核单元（Python / R / Julia）：
    /// 解析实际内核（显式指定 > 会话默认 > 注册表第一个可用）→ 启动（复用）内核 →
    /// 标记 Running → 提交代码 → 内核泵轮询事件回填输出；成功后把内核回写为
    /// 会话默认（“上次用过的内核”）。
    ///
    /// 与 SQL 分支分开读 shared（`notebook_run_cell` 里不保留对 state 的借用再调这里）
    fn run_kernel_cell(
        &mut self,
        session_id: &str,
        cell_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(cell) = state.cell_mut(cell_id) else {
            return;
        };
        if cell.body.trim().is_empty() {
            self.set_message(
                Some(format!("这个 {} 单元是空的，没有可执行的代码", cell.language.label())),
                cx,
            );
            return;
        }
        // 实际内核：执行源指定优先，否则会话默认 / 注册表第一个可用（#20 注册表路由）
        let source = cell.execution_source.clone();
        let language = cell.language;
        let kernel_id = match self.shared.resolve_kernel_for(&self.document, source.as_ref(), language) {
            Ok(spec) => spec.id.clone(),
            Err(reason) => {
                self.set_message(Some(reason), cx);
                return;
            }
        };
        let Some(command) = self
            .shared
            .kernel_specs()
            .into_iter()
            .find(|spec| spec.id == kernel_id)
            .map(|spec| spec.command)
        else {
            self.set_message(Some(format!("内核「{kernel_id}」已从注册表消失，请重启面板")), cx);
            return;
        };
        if let Err(reason) = self.shared.ensure_kernel(&kernel_id, &command) {
            self.set_message(Some(format!("内核 {kernel_id} 启动失败：{reason}")), cx);
            return;
        }
        let code = cell.body.clone();
        cell.mark_running();
        self.kernel_running = Some(KernelRun {
            kernel_id: kernel_id.clone(),
            cell_id: cell_id.to_string(),
            session_id: session_id.to_string(),
        });
        self.shared.set_notebook_cells(&self.document, state.cells);
        if let Err(reason) = self.shared.kernel_execute(&kernel_id, &code) {
            self.kernel_running = None;
            let mut state = self.notebook_state();
            if let Some(cell) = state.cell_mut(cell_id) {
                cell.state = crate::cell::CellState::Idle;
                cell.outputs.clear();
            }
            self.shared.set_notebook_cells(&self.document, state.cells);
            self.set_message(Some(reason), cx);
            return;
        }
        // 执行提交成功 → 回写会话默认（“上次用过的内核”）
        self.shared.set_session_default_kernel(&self.document, Some(kernel_id));
        self.set_message(None, cx);
        self.ensure_kernel_pump(cx);
    }

    /// 【#17】内核事件泵：60ms 一拍轮询当前内核单元的事件，回填输出直到 ExecuteReply /
    /// Failed（内核空闲即退出循环，下次执行重新起）
    fn ensure_kernel_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.kernel_pump.borrow().as_ref()
            && !task.is_ready()
        {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let keep_going = weak
                    .update(cx, |this, cx| {
                        let before = this.kernel_running.is_some();
                        this.drain_kernel_events(cx);
                        if before {
                            cx.notify();
                        }
                        this.kernel_running.is_some()
                    })
                    .unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        });
        *self.kernel_pump.borrow_mut() = Some(task);
    }

    /// 【#17】取走内核事件并落到当前运行的单元：
    /// 流式输出实时上屏（Display / Stream 追加），ExecuteReply 收口（成功置 Done /
    /// 失败置 Error），Failed 给原因
    fn drain_kernel_events(&mut self, _cx: &mut Context<Self>) {
        let Some(run) = self.kernel_running.clone() else {
            return;
        };
        let events = self.shared.kernel_drain(&run.kernel_id);
        if events.is_empty() {
            return;
        }
        let mut state = self.notebook_state();
        // 会话 id 先克隆（`cell_mut` 之后不能同时借 `state.session`）
        let session_id = state.session.id.clone();
        let Some(cell) = state.cell_mut(&run.cell_id) else {
            // 单元没了：执行作废（不再等回执）
            self.kernel_running = None;
            return;
        };
        // 会话已切换：回执不落，执行作废
        if session_id != run.session_id {
            self.kernel_running = None;
            cell.state = crate::cell::CellState::Idle;
            cell.outputs.clear();
            self.shared.set_notebook_cells(&self.document, state.cells);
            return;
        }
        let mut finished = false;
        let mut failed_reason: Option<String> = None;
        for event in events {
            match event {
                rds_kernel::KernelEvent::Display(media) => {
                    // mime bundle → 原生输出（图片 / 表格 / markdown / 文本）
                    let outputs = crate::media::media_to_outputs(&media);
                    cell.outputs.extend(outputs);
                }
                rds_kernel::KernelEvent::Stream { name, text } => {
                    // stdout 无前缀；stderr 带标记（如实反映来源）
                    if name == "stderr" {
                        cell.outputs
                            .push(crate::cell::Output::Text(format!("stderr: {text}")));
                    } else {
                        cell.outputs.push(crate::cell::Output::Text(text));
                    }
                }
                rds_kernel::KernelEvent::ExecuteReply { status, .. } => {
                    finished = true;
                    if status != "ok" {
                        failed_reason = Some("内核返回执行失败".to_string());
                    }
                }
                rds_kernel::KernelEvent::Error { ename, evalue, .. } => {
                    cell.outputs.push(crate::cell::Output::Error(format!(
                        "{ename}: {evalue}"
                    )));
                }
                rds_kernel::KernelEvent::Status(_) => {}
                rds_kernel::KernelEvent::Failed(reason) => {
                    finished = true;
                    failed_reason = Some(reason);
                }
            }
        }
        if finished {
            self.kernel_running = None;
            if let Some(reason) = failed_reason {
                cell.state = crate::cell::CellState::Error;
                if !cell
                    .outputs
                    .iter()
                    .any(|o| matches!(o, crate::cell::Output::Error(_)))
                {
                    cell.outputs.push(crate::cell::Output::Error(reason));
                }
            } else {
                cell.state = crate::cell::CellState::Done;
            }
        }
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【2d-笔记级】全部运行队列推进：这个内核单元收口（ExecuteReply / Failed），
        // 接下一个（advance 内部守卫：队列空 / 还有单元在跑就不动）
        if finished {
            self.advance_notebook_queue(_cx);
        }
    }

    /// 中断单元执行（沿执行队列的取消；没在跑 = 如实回绝）
    pub(crate) fn notebook_cancel_run(&mut self, cx: &mut Context<Self>) {
        match self.shared.cancel() {
            Ok(()) => self.set_message(Some("已请求中断".to_string()), cx),
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【2d】收单元执行的结论（主线程轮询取走）：按会话 + 单元 id 回填内联输出
    pub(crate) fn drain_cell_notes(&mut self, cx: &mut Context<Self>) {
        let mut mine_arrived = 0usize;
        for note in self.shared.drain_cell_notes() {
            if note.document != self.document {
                continue;
            }
            mine_arrived += 1;
            self.shared.apply_cell_note(&self.document, &note);
        }
        // 回执按本文档的份数收口 pending（与脚本同口径）
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
            }
        }
        // 【2d-笔记级】全部运行队列推进：这个单元跑完（回执已落），接下一个
        if mine_arrived > 0 {
            self.advance_notebook_queue(cx);
        }
        cx.notify();
    }

    /// 编辑区正文：分析模式 = 笔记本（单元卡片）；其它模式 = 内核编辑器
    fn render_editor_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        if mode != EditorMode::Analysis {
            // 欢迎页：启动空白文档（未命名 + 空内容 + 未点过「开始编写」）显示欢迎页
            // 而不是光秃秃的空编辑器（2026-10-14 用户点名）。点按钮或输入内容后
            // 回到正常编辑器（`welcome_dismissed` 置位 / 内容非空即退出欢迎态）。
            if self.show_welcome() {
                return self.render_welcome_page(cx);
            }
            // 【遗留】通用模式 Markdown 文档的渲染态：渲染视图（只读）替代源码编辑器。
            // 判据 = 预览开关 + 文档类型是 Markdown；其余文档一律走编辑器。
            if self.markdown_preview && self.is_markdown_document() {
                let source = self
                    .with_document(|doc| doc.content().to_string())
                    .unwrap_or_default();
                // 外层带 id + test_support 的容器：headless 测试用 find 断言渲染视图出现
                return div()
                    .id("markdown-preview-root")
                    .test_support()
                    .size_full()
                    .child(TextView::markdown("md-preview", source).scrollable(true).size_full())
                    .into_any_element();
            }
            let read_only = self.editor_read_only();
            return Editor::new(&self.editor)
                .appearance(false)
                .bordered(false)
                .readonly(read_only)
                .size_full()
                .into_any_element();
        }
        // 分析模式：笔记本正文（单元卡片列表 / 空态引导卡）。编辑区可滚动。
        let state = self.notebook_state();
        let entity = cx.entity();
        // 连接列表是内存快照（右下角执行源选择器数据源）；render 纯读
        let connections = self.shared.connection_options();
        // 【#20】内核注册表快照（执行源「内核」档的菜单数据源）；render 纯读
        let kernels = self.shared.kernel_specs();
        // render 纯读：单元编辑器实例按引用传入（不能 read 正在被 render 的面板）
        crate::view::analysis::render(
            cx,
            &state,
            &self.document,
            entity,
            &self.cell_editors,
            &connections,
            &kernels,
        )
        .into_any_element()
    }

    /// 是否显示欢迎页：启动空白文档（未命名 + 内容为空 + 用户没点过「开始编写」）。
    ///
    /// 判据特意用**服务层的文档路径与内容**：路径为空 = 未命名，内容空 = 还没写东西。
    /// 用户一旦开始输入（内容非空）或点了欢迎页按钮（`welcome_dismissed`），立即退出欢迎态。
    pub(crate) fn show_welcome(&self) -> bool {
        if self.welcome_dismissed || self.document_path().is_some() {
            return false;
        }
        self.with_document(|doc| doc.content().trim().is_empty())
            .unwrap_or(false)
    }

    /// 点掉欢迎页（「开始编写」按钮）：置位后回到正常编辑器。
    pub(crate) fn dismiss_welcome(&mut self, cx: &mut Context<Self>) {
        self.welcome_dismissed = true;
        cx.notify();
    }

    /// 欢迎页：启动空白文档的“空编辑器”换成一页引导（2026-10-14 用户点名）。
    ///
    /// 内容：居中品牌字 + 短标语 + 「开始编写」按钮（点掉欢迎态、聚焦编辑内核）。
    /// 不接任何工作台级动作（打开草稿 / 搜索等要宿主能力，留到后续版本），
    /// 按钮只做“从欢迎页进入编辑”这一件事。
    fn render_welcome_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;
        let entity = cx.entity();

        let start = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.dismiss_welcome(cx));
                // 关掉欢迎页后把焦点交给编辑内核，用户可以直接开始打字
                entity.update(app, |this, cx| {
                    let handle = this.editor.read(cx).focus_handle(cx);
                    handle.focus(window, cx);
                });
            }
        };

        div()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_3()
            .text_color(fg)
            .child(
                Icon::new(IconName::Database)
                    .size(rems(2.5))
                    .text_color(primary),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child("欢迎使用 RdataStation"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("连接数据，分析洞察——从一段 SQL 开始"),
            )
            .child(
                Button::new("welcome-start")
                    .label("开始编写 SQL")
                    .on_click(start),
            )
            .into_any_element()
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
        let document = self
            .with_document(|doc| doc.read_only())
            .unwrap_or_default();
        ReadOnly {
            editor: document.editor,
            connection: document.connection || self.shared.project_read_only(),
        }
    }

    /// 【T2a-6】批量提交当前结果集的变更集（提交前复核四道防线，与编辑判据同一套口径）
    ///
    /// 四道防线（通道 / 项目只读 / 连接只读 / 无行标识）在**提交前**复核而不是只信编辑时的
    /// 判据：用户可能改完几格才切通道 / 锁项目，提交时防线已变——这里把变了的拦下。
    ///
    /// 返回 `Err(可读原因)` = 提交被拦（调用方负责展示）；成功则把这次提交记为在途
    /// （[`CommitInFlight`]），回执回来在轮询泵里组装冲突并落状态栏。
    pub(crate) fn submit_pending_changes(&mut self, _cx: &mut Context<Self>) -> Result<(), String> {
        // ① 提交中不再接新提交（回执回来前按钮保持灰）
        if self.commit_in_flight.is_some() {
            return Err("已有提交在进行中（等回执回来）".to_string());
        }
        // ② 通道闸：写回只走源库通道（加速 / 联邦只读挂载，改不了源库）
        let channel = self.channel();
        if !channel.allows_source_writes() {
            return Err(format!("提交只支持源库通道（当前是 {}）", channel.label()));
        }
        // ③ 项目只读（沿用 project_write_check 的判据：强只读直接拒）
        if self.shared.project_read_only() {
            return Err("项目为只读模式：写语句被拒（要改数据先解锁项目）".to_string());
        }
        // ④ 连接只读
        let connection_read_only = self
            .with_document(|doc| doc.read_only().connection)
            .unwrap_or(false);
        if connection_read_only {
            return Err("连接为只读模式：批量提交被拒".to_string());
        }
        // ⑤ 无行标识 → 整份只读（判据层已拦住编辑；这里防判据变化后仍能提交）
        let entry = self
            .shared
            .results_active(&self.document)
            .ok_or_else(|| "当前没有可提交的结果集".to_string())?;
        let set_index = self
            .shared
            .results()
            .active_index(&self.document)
            .ok_or_else(|| "当前没有可提交的结果集".to_string())?;
        let key_columns = entry
            .key_columns
            .clone()
            .ok_or_else(|| "这份结果没探测到主键：改不了（提交靠主键定位行）".to_string())?;
        let key = crate::export::RowKey::new(key_columns)
            .ok_or_else(|| "主键列为空（不该发生：探测到就不会是空）".to_string())?;

        // ⑥ 生成 UPDATE 文本（含语句→行/列索引）；表名与主键探测同一处猜测（probe_primary_key）
        let table = crate::export::default_table_name(Some(&entry.sql), 0);
        // pending 克隆出借用边界（不拖住 results() 的 Ref）
        let pending: Vec<crate::changes::CellChange> = {
            let results = self.shared.results();
            results
                .change_set(&self.document, set_index)
                .map(|changes| changes.pending().iter().map(|c| (*c).clone()).collect())
                .unwrap_or_default()
        };
        if pending.is_empty() {
            return Err("没有待提交的变更".to_string());
        }
        let mut by_row: std::collections::BTreeMap<usize, Vec<&crate::changes::CellChange>> =
            std::collections::BTreeMap::new();
        for change in &pending {
            by_row.entry(change.row).or_default().push(change);
        }
        let updates = crate::changes::commit_updates_rows(
            &table,
            &key,
            &entry.columns,
            &entry.rows,
            &by_row,
        )?;

        // ⑦ 请执行器提交；成则记为在途（回执回来前不重复提交）
        self.shared.request_commit_changes(
            self.document.clone(),
            table,
            key,
            entry.sql.clone(),
            updates.sql,
        )?;
        self.commit_in_flight = Some(CommitInFlight {
            set_index,
            per_line: updates.per_line,
            skipped: updates.skipped,
        });
        Ok(())
    }

    /// 提交中的真值（结果工具栏据此把「提交变更」置灰 / 显示进行中）
    pub fn commit_pending(&self) -> bool {
        self.commit_in_flight.is_some()
    }

    /// 【T2a-7】丢弃全部**待提交**变更（回滚：DBeaver 提交/回滚对的「回滚」侧）
    ///
    /// 清掉变更集里的 Pending 项（Committed 已落库、Conflict 是证据，都保留），
    /// 网格的脏格底色随 [`Self::sync_result_view`] 的投影一起消失。
    pub(crate) fn discard_pending_changes(&mut self, cx: &mut Context<Self>) {
        let index = self.result_active;
        let changed = self.shared.update_results(|store| {
            store.mutate_change_set(&self.document, index, |changes| changes.discard_pending())
        });
        if changed {
            self.sync_result_view(cx);
            self.set_message(Some("已撤销未提交的变更".to_string()), cx);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn commit_in_flight_for_test(
        &self,
    ) -> Option<(usize, Vec<(usize, Vec<usize>)>, usize)> {
        self.commit_in_flight.as_ref().map(|inflight| {
            (
                inflight.set_index,
                inflight.per_line.clone(),
                inflight.skipped,
            )
        })
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
                    menu = menu.item(PopupMenuItem::new(label).checked(is_current).on_click(
                        move |_, _window, app| {
                            let id = id.clone();
                            entity.update(app, |panel, cx| {
                                panel.bind_connection(Some(id), cx);
                            });
                        },
                    ));
                }
                menu
            })
    }

    /// 绑定连接（工具栏选择器调用）：先用端口确保已建连，失败就**不绑定**并留原因
    ///
    /// `None` = 解绑（跟随当前连接）。绑定是**文档属性**，同一窗口的多份文档互不影响。
    pub(crate) fn bind_connection(&mut self, conn_id: Option<String>, cx: &mut Context<Self>) {
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

    /// 【T2c-5】渲染一个组（组 = 分隔符界定的连续段，见 [`crate::toolbar`]）
    ///
    /// - 折叠（且组内可见成员 ≥ 2）：显示为「首组件 + ▾」——▾ 收纳组内其余**可见**成员；
    ///   这正是旧 `render_exec_group` 的形态，所以折叠 / 展开是同一组的两副面孔。
    /// - 展开：逐可见项渲染（执行 / 格式化 / 执行计划等独立按钮）。
    fn render_toolbar_group(
        &self,
        group: &crate::toolbar::ToolbarGroup,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible: Vec<crate::toolbar::ToolbarItemId> = group.visible_items(&self.toolbar);
        let collapsed = visible.len() >= 2 && self.toolbar.is_collapsed(group.head);
        if collapsed {
            self.render_collapsed_group(group.head, &visible, cx)
                .into_any_element()
        } else {
            let mut row = div().h_flex().items_center().gap_1();
            for id in &visible {
                row = row.child(self.render_toolbar_item(*id, cx));
            }
            row.into_any_element()
        }
    }

    /// 【T2c-5】渲染一个单项（目录 [`crate::toolbar::ToolbarItemId`]；动作统一走
    /// [`Self::run_toolbar_item_action`]——渲染只按 id 挂事件，动作语义集中在那一处）
    fn render_toolbar_item(
        &self,
        id: crate::toolbar::ToolbarItemId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        match id {
            crate::toolbar::ToolbarItemId::ExecRun => self.render_exec_run_button(cx).into_any_element(),
            crate::toolbar::ToolbarItemId::ExecAll => {
                self.render_exec_all_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::ExecCurrent => self
                .render_exec_kind_button(id, ExecMenuKind::CurrentStatement, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::ExecSelection => self
                .render_exec_kind_button(id, ExecMenuKind::Selection, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::ExecBatch => self
                .render_exec_kind_button(id, ExecMenuKind::Batch, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::Explain => {
                self.render_explain_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::Translate => {
                self.render_translate_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::Template => {
                self.render_template_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::TxAutoCommit => {
                self.render_tx_auto_commit_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::Format => {
                self.render_format_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::History => {
                self.render_history_button(cx).into_any_element()
            }
        }
    }

    /// 【T2c-5】执行主按钮：与 `Ctrl+Enter` 走同一条路（选区优先 → 当前语句）
    fn render_exec_run_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-exec-run")
            .primary()
            .small()
            .debug_selector(|| "editor-exec-run".to_string())
            .icon(crate::toolbar::ToolbarItemId::ExecRun.icon())
            .tooltip("执行（Ctrl+Enter）")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.execute_preferring_selection(cx));
            })
    }

    /// 【T2c-5】全部运行：与 `Ctrl+Shift+Enter` 同一条路（整篇脚本）
    fn render_exec_all_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-exec-all")
            .ghost()
            .small()
            .debug_selector(|| "editor-exec-all".to_string())
            .icon(crate::toolbar::ToolbarItemId::ExecAll.icon())
            .tooltip("全部运行（Ctrl+Shift+Enter）")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.run_all(cx));
            })
    }

    /// 【T2c-5】执行族的显式目标按钮（当前语句 / 选区 / 批量）：`target_for_menu` 解析，
    /// 与旧下拉菜单的**显式目标**语义一致——“执行选区”没选区时置灰、“批量执行”语句不足
    /// 两条时置灰，比“点了没反应”明确。
    fn render_exec_kind_button(
        &self,
        id: crate::toolbar::ToolbarItemId,
        kind: ExecMenuKind,
        cx: &mut Context<Self>,
    ) -> Button {
        let entity = cx.entity();
        let (_, selection) = self.editor_snapshot(cx);
        let statements = self.statements;
        let available = kind.is_available(&selection, statements);
        let selector = match id {
            crate::toolbar::ToolbarItemId::ExecCurrent => "editor-exec-current",
            crate::toolbar::ToolbarItemId::ExecSelection => "editor-exec-selection",
            crate::toolbar::ToolbarItemId::ExecBatch => "editor-exec-batch",
            _ => unreachable!("执行族显式目标按钮只有这三种"),
        };
        Button::new(selector)
            .ghost()
            .small()
            .debug_selector(move || selector.to_string())
            .icon(id.icon())
            .tooltip(id.label())
            .disabled(!available)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.execute_menu_kind(kind, cx));
            })
    }

    /// 【T2c-5】折叠组：首组件 + ▾，▾ 收纳组内其余可见成员
    ///
    /// ▾ 菜单只收执行族（`Exec*`）：格式化要 `&mut Window` 而菜单回调拿不到可靠的窗口，
    /// ⋯更多本身是嵌套菜单——两都放不进折叠菜单。执行族动作都不需要窗口，收进来没问题。
    ///
    /// 组头（`head`）必须是可 `Button` 化的项；`⋯ 更多` 由 [`Self::render_toolbar_group`]
    /// 提前退化为展开，到不了这里。
    fn render_collapsed_group(
        &self,
        head: crate::toolbar::ToolbarItemId,
        visible: &[crate::toolbar::ToolbarItemId],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::toolbar::ToolbarItemId;
        let entity = cx.entity();
        let (_, selection) = self.editor_snapshot(cx);
        let statements = self.statements;
        // ElementId 用业务键（组头）不用下标：折叠多组时也互不冲突
        let key = format!("editor-toolbar-{:?}", head);
        let head_button: Button = match head {
            ToolbarItemId::ExecRun => self.render_exec_run_button(cx),
            ToolbarItemId::ExecAll => self.render_exec_all_button(cx),
            ToolbarItemId::ExecCurrent => {
                self.render_exec_kind_button(head, ExecMenuKind::CurrentStatement, cx)
            }
            ToolbarItemId::ExecSelection => {
                self.render_exec_kind_button(head, ExecMenuKind::Selection, cx)
            }
            ToolbarItemId::ExecBatch => self.render_exec_kind_button(head, ExecMenuKind::Batch, cx),
            ToolbarItemId::Explain => self.render_explain_button(cx),
            // 折叠组头只放纯按钮：下拉菜单由 DropdownButton 的 ▾ 半区提供，
            // 完整「方言转换 ▾ / 模板 ▾」本体见 render_translate_button / render_template_button
            ToolbarItemId::Translate => Button::new("editor-translate")
                .ghost()
                .small()
                .icon(ToolbarItemId::Translate.icon())
                .tooltip(ToolbarItemId::Translate.label()),
            ToolbarItemId::Template => Button::new("editor-template")
                .ghost()
                .small()
                .icon(ToolbarItemId::Template.icon())
                .tooltip(ToolbarItemId::Template.label())
                .disabled(self.shared.completion_templates().is_empty()),
            ToolbarItemId::TxAutoCommit => self.render_tx_auto_commit_button(cx),
            ToolbarItemId::Format => self.render_format_button(cx),
            ToolbarItemId::History => self.render_history_button(cx),
        };
        let menu_items: Vec<ToolbarItemId> = visible
            .iter()
            .copied()
            .filter(|id| {
                *id != head
                    && matches!(
                        *id,
                        ToolbarItemId::ExecRun
                            | ToolbarItemId::ExecAll
                            | ToolbarItemId::ExecCurrent
                            | ToolbarItemId::ExecSelection
                            | ToolbarItemId::ExecBatch
                    )
            })
            .collect();
        DropdownButton::new(key)
            .small()
            .button(head_button)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for id in menu_items.iter() {
                    let entity = entity.clone();
                    let kind = match id {
                        ToolbarItemId::ExecRun => ExecMenuKind::All,
                        ToolbarItemId::ExecAll => ExecMenuKind::All,
                        ToolbarItemId::ExecCurrent => ExecMenuKind::CurrentStatement,
                        ToolbarItemId::ExecSelection => ExecMenuKind::Selection,
                        ToolbarItemId::ExecBatch => ExecMenuKind::Batch,
                        _ => unreachable!("折叠菜单只收执行族"),
                    };
                    let available = kind.is_available(&selection, statements);
                    let id = *id;
                    menu = menu.item(
                        PopupMenuItem::new(id.label())
                            .disabled(!available)
                            .on_click(move |_, window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.run_toolbar_item_action(id, window, cx);
                                });
                            }),
                    );
                }
                menu
            })
    }

    /// 【B10】文档级：格式化按钮（原型 §2.2 的「⇤ 格式化」，紧随执行级）
    fn render_format_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-format")
            .ghost()
            .small()
            .debug_selector(|| "editor-format".to_string())
            .icon(crate::toolbar::ToolbarItemId::Format.icon())
            .tooltip("格式化")
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| panel.format_document(window, cx));
            })
    }

    /// 【B8 余项】文档级「⟲ 历史」：打开右 Dock 历史面板（端口注入——编辑器不依赖工作台
    /// 的 Dock）。未接端口时按钮置灰（`.disabled`），点了也会在状态栏给可读原因（不静默）。
    fn render_history_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        let available = self.shared.open_history_port().is_some();
        Button::new("editor-history")
            .ghost()
            .small()
            .disabled(!available)
            .debug_selector(|| "editor-history".to_string())
            .icon(crate::toolbar::ToolbarItemId::History.icon())
            .tooltip("历史")
            .on_click(move |_, _window, app| {
                if entity.read(app).shared.open_history_port().is_none() {
                    // 未接端口：状态栏给原因，而不是“点了没反应”
                    entity.update(app, |panel, cx| {
                        panel.set_message(Some("未接入历史面板".to_string()), cx);
                    });
                    return;
                }
                let port = entity.read(app).shared.open_history_port().unwrap();
                port(app);
            })
    }

    /// 【原型 §2.2 分析组】执行计划（EXPLAIN）：常驻按钮（2026-10-11 提升常驻，不再进"更多"）
    fn render_explain_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-explain")
            .ghost()
            .small()
            .debug_selector(|| "editor-explain".to_string())
            .icon(crate::toolbar::ToolbarItemId::Explain.icon())
            .tooltip("执行计划")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.explain_current(cx));
            })
    }

    /// 【原型 §2.2 分析组】方言转换：常驻按钮（目标方言列表）
    ///
    /// 目标清单在**渲染时**定好（源方言已知就排除它自己）；菜单回调是 `Fn`，不能在里头读面板。
    /// 源方言未知（未绑定连接）时仍把目标列出来——点下去会收到一句可读的拒绝，
    /// 比“菜单里什么都没有”更容易看懂。
    fn render_translate_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let targets: Vec<translate::Target> = match self.source_dialect() {
            Ok(source) => translate::targets_for(source),
            Err(_) => translate::TARGETS.to_vec(),
        };
        Button::new("editor-translate")
            .ghost()
            .small()
            .debug_selector(|| "editor-translate".to_string())
            .icon(crate::toolbar::ToolbarItemId::Translate.icon())
            .tooltip("方言转换")
            .dropdown_menu(move |menu, window, cx| {
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
            })
    }

    /// 【原型 §2.2 事务/模板】模板 / 代码片段：常驻按钮（无模板时置灰，菜单不摆空项）
    fn render_template_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // 【B9 切片二】模板片段（内存读：渲染时读一次；没模板 / 没接端口就置灰）
        let templates = self.shared.completion_templates();
        let has_templates = !templates.is_empty();
        Button::new("editor-template")
            .ghost()
            .small()
            .debug_selector(|| "editor-template".to_string())
            .disabled(!has_templates)
            .icon(crate::toolbar::ToolbarItemId::Template.icon())
            .tooltip("模板")
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for (index, template) in templates.iter().enumerate() {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(template.name.clone()).on_click(
                        move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.insert_template(index, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
    }

    /// 【原型 §2.2 事务/模板】事务提交方式（自动 ⇄ 手动）：常驻开关（模式要一直看得见）
    ///
    /// 从状态栏 TX 区移到工具栏（原型 §2.2 的“事务开关常显”）；状态栏 TX 区只留
    /// 状态文本 + 事务开启时的提交 / 回滚。
    fn render_tx_auto_commit_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-autocommit")
            .ghost()
            .small()
            .debug_selector(|| "editor-autocommit".to_string())
            .toggled(self.autocommit)
            .icon(crate::toolbar::ToolbarItemId::TxAutoCommit.icon())
            .tooltip("自动提交")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.toggle_autocommit(cx));
            })
    }

    /// 【T2c-5】`···` 定制按钮 + 三栏设置面板（可见性 / 折叠 / 移动 / 竖线分隔符 / 重置）
    ///
    /// 为什么用 `Popover` 而不是 `DropdownMenu`：菜单实体是**缓存**的，切换可见性后内容会
    /// 陈旧；`Popover::content` 的闭包**每帧重建**，能实时反映面板状态。面板不是菜单——
    /// 它是「图标功能｜可见｜移动」三栏的设置视图，用 `Checkbox` / `Switch` / `Button` /
    /// `Separator` 现成组件拼装（组件禁手搓）。
    ///
    /// 面板分四段：标题 → 组头行（组名 + 折叠开关 + 组移动↑↓）→ 成员行（图标+标签｜
    /// 可见 ▢｜单项移动↑↓）→ 段尾分隔符行（┃ 竖线分隔符 + ✕）→ 底部（＋添加竖线分隔符｜
    /// ↺ 重置默认）。所有动作走面板方法（事件路径），render 只按 id 挂回调。
    fn render_toolbar_customize(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::toolbar::MoveDir;
        let entity = cx.entity();
        let panel_width = rems(ui::TOOLBAR_CUSTOMIZE_WIDTH);
        Popover::new("editor-toolbar-customize-popover")
            // 最右的按钮：面板向右下展开，避免被窗口右缘截断
            .anchor(Anchor::BottomRight)
            .offset(ui::TOOLBAR_CUSTOMIZE_OFFSET)
            .trigger(
                Button::new("editor-toolbar-customize")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-toolbar-customize".to_string())
                    .icon(IconName::Ellipsis)
                    .tooltip("自定义工具栏"),
            )
            .content(move |_state, _window, cx| {
                let muted = cx.theme().colors.muted_foreground;
                let panel = entity.read(cx);
                let layout = &panel.toolbar;
                let groups = layout.groups();

                let mut body = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .w(panel_width)
                    .debug_selector(|| "editor-toolbar-customize-panel".to_string());
                body = body.child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(muted)
                        .child("自定义工具栏"),
                );
                body = body.child(Separator::horizontal());
                // 竖线分隔符位置快照：分离出来，避免 `'static` 闭包借用临时值
                let separator_positions = layout.separator_positions();

                for group in &groups {
                    // 组头行：组名 + 折叠开关 + 组移动↑↓
                    let head = group.head;
                    let collapsed = layout.is_collapsed(head);
                    let can_move_left = head != group_of_first(&groups);
                    let can_move_right = head != group_of_last(&groups);
                    body = body.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .debug_selector(move || {
                                format!("editor-toolbar-customize-group-{:?}", head)
                            })
                            .child(panel.item_icon_row(head, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(head.label()),
                            )
                            .child(Switch::new(format!("toolbar-fold-{:?}", head))
                                .checked(collapsed)
                                .label("折叠")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.toggle_toolbar_collapsed(head, cx);
                                        });
                                    }
                                }))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-group-up-{:?}", head),
                                IconName::ChevronUp,
                                can_move_left,
                                move |panel, cx| panel.move_toolbar_group(head, MoveDir::Left, cx),
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-group-down-{:?}", head),
                                IconName::ChevronDown,
                                can_move_right,
                                move |panel, cx| panel.move_toolbar_group(head, MoveDir::Right, cx),
                            )),
                    );
                    // 成员行：图标+标签｜可见 ▢｜单项移动↑↓
                    for id in &group.items {
                        let id = *id;
                        let visible = layout.is_visible(id);
                        let fixed = id.fixed_visible();
                        body = body.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .pl_3()
                                .debug_selector(move || {
                                    format!("editor-toolbar-customize-item-{:?}", id)
                                })
                                .child(panel.item_icon_row(id, cx))
                                .child(div().flex_1().text_xs().child(id.label()))
                                .child(
                                    Checkbox::new(format!("toolbar-vis-{:?}", id))
                                        .checked(visible)
                                        .disabled(fixed)
                                        .on_click({
                                            let entity = entity.clone();
                                            move |&on, _window, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.set_toolbar_visible(id, on, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(panel.small_icon_button(
                                    &entity,
                                    format!("toolbar-item-up-{:?}", id),
                                    IconName::ChevronUp,
                                    true,
                                    move |panel, cx| panel.move_toolbar_item(id, MoveDir::Left, cx),
                                ))
                                .child(panel.small_icon_button(
                                    &entity,
                                    format!("toolbar-item-down-{:?}", id),
                                    IconName::ChevronDown,
                                    true,
                                    move |panel, cx| {
                                        panel.move_toolbar_item(id, MoveDir::Right, cx)
                                    },
                                )),
                        );
                    }
                }

                // 段尾分隔符行：每条竖线分隔符一个「┃ 竖线分隔符 + ✕」
                for (index, pos) in separator_positions.iter().enumerate() {
                    let key = format!("toolbar-sep-{index}");
                    let pos = *pos;
                    body = body.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .pl_3()
                            .debug_selector(move || key.clone())
                            .child(Separator::vertical().h(rems(0.75)))
                            .child(div().flex_1().text_xs().text_color(muted).child("竖线分隔符"))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-up-{index}"),
                                IconName::ChevronUp,
                                true,
                                move |panel, cx| {
                                    panel.move_toolbar_separator(pos, MoveDir::Left, cx)
                                },
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-down-{index}"),
                                IconName::ChevronDown,
                                true,
                                move |panel, cx| {
                                    panel.move_toolbar_separator(pos, MoveDir::Right, cx)
                                },
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-remove-{index}"),
                                IconName::X,
                                true,
                                move |panel, cx| panel.remove_toolbar_separator(pos, cx),
                            )),
                    );
                }

                body = body.child(Separator::horizontal());
                // 底部：＋添加竖线分隔符｜↺ 重置默认
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("editor-toolbar-customize-add-sep")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label("添加竖线分隔符")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.insert_toolbar_separator(cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("editor-toolbar-customize-reset")
                                .ghost()
                                .small()
                                .icon(IconName::RotateCcw)
                                .label("重置默认")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| panel.reset_toolbar(cx));
                                    }
                                }),
                        ),
                );
                body
            })
    }

    /// 【T2c-5】面板成员行的最左列：功能图标（挂 `editor-toolbar-customize-icon-*` 选择器）
    ///
    /// 从 Popover content 闭包里调：那儿的 `cx` 是 `Context<PopoverState>`，所以只收 `&App`
    /// 取主题色，不做任何面板状态的读写（状态由 content 闭包在外层 `entity.read` 统一读）。
    fn item_icon_row(
        &self,
        id: crate::toolbar::ToolbarItemId,
        cx: &gpui_kit::App,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let key = format!("editor-toolbar-customize-icon-{:?}", id);
        div()
            .w_4()
            .h_flex()
            .items_center()
            .justify_center()
            .debug_selector(move || key.clone())
            .child(Icon::new(id.icon()).text_color(muted))
    }

    /// 【T2c-5】面板里的小图标按钮（↑↓ / ✕ 用；统一 ghost + small + 图标）
    ///
    /// 为什么显式传 `entity` 而不是 `cx.entity()`：Popover content 闭包里的 `cx` 是
    /// `Context<PopoverState>`，拿不到面板实体——由调用方把捕获的 `entity` 传进来，
    /// 动作统一 `entity.update` 转交面板方法。
    fn small_icon_button(
        &self,
        entity: &Entity<Self>,
        id: impl Into<gpui_kit::ElementId>,
        icon: IconName,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Button {
        let entity = entity.clone();
        Button::new(id)
            .ghost()
            .small()
            .icon(icon)
            .disabled(!enabled)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| action(panel, cx));
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
    pub fn sync_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let read_only = self.editor_read_only();
        // 语义着色类型：模式切换后按新模式与文档扩展名重算（脚本分派 / CSV 不上色）
        let highlight_kind =
            self.with_document(|doc| highlight::kind_for_document(doc.path(), doc.mode()))
                .unwrap_or(highlight::HighlightKind::None);

        // 【LSP】模式切换时重新向宿主要服务（插件加载 / 宿主判定变了能接上；没变 = 无操作）
        self.sync_lsp_session(cx);

        self.editor.update(cx, |state, cx| {
            state.set_readonly(read_only, cx);
            if highlight::is_enabled(mode) {
                highlight::install(state, cx, highlight_kind);
            } else {
                // 未启用的模式（当前只剩分析）：摘掉着色器，不再取新 token。内核的
                // token 缓存没有公开的清入口，摘除后旧 token 要等下一次文本变更才失效。
                state.lsp_mut().semantic_tokens_provider = None;
            }
            // B9：补全按能力表装 / 摘（文本模式不接，分析模式留 1c 逐单元）
            self.sync_completion_provider(state);
            // B17：折叠按能力表与档位装 / 关（分析模式能力位为 false → 真关）
            sync_fold(&self.shared, &self.document, state, window, cx);
        });
        cx.notify();
    }

    /// 按当前文档的能力与档位装 / 摘补全 provider（**唯一开关处**：新建与切模式都走它）
    ///
    /// 三选一（纯逻辑在 [`crate::lsp::host::choose_completion`]）：
    /// - SQL 模式 → 本地 SQL 目录补全（`completion_enabled`：能力表 + 只读 + 大文件档位）；
    /// - 通用 / 分析模式且语言服务健康可编辑 → LSP 补全（请求转发给语言服务）；
    /// - 都没有 → 摘干净（两个 provider 都摘，不残留）。
    fn sync_completion_provider(&self, state: &mut gpui_kit::component::input::EditorState) {
        let sql_enabled = self.shared.completion_enabled(&self.document);
        let lsp_available = self.lsp_alive
            && self.lsp.is_some()
            && !self.editor_read_only()
            && !self
                .with_document(|doc| doc.tier().disables_completion())
                .unwrap_or(true);
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        match crate::lsp::host::choose_completion(mode, lsp_available, sql_enabled) {
            crate::lsp::CompletionChoice::Lsp => {
                if let Some(session) = self.lsp.as_ref() {
                    let provider = std::rc::Rc::new(crate::lsp::bridge::LspCompletionProvider::new(
                        session.client.clone(),
                        session.uri.clone(),
                    ));
                    crate::lsp::bridge::install(state, provider);
                    // 【#21】hover 与补全同源（语言服务）：一起挂、一起摘。
                    // 弹层由内核自带（150ms 防抖 + 自动渲染/收起），这里只补数据源。
                    let hover = std::rc::Rc::new(crate::lsp::bridge::LspHoverProvider::new(
                        session.client.clone(),
                        session.uri.clone(),
                    ));
                    crate::lsp::bridge::install_hover(state, hover);
                }
            }
            crate::lsp::CompletionChoice::Sql => {
                completion::install(state, self.shared.clone(), self.document.clone());
            }
            crate::lsp::CompletionChoice::None => {
                crate::lsp::bridge::uninstall(state);
                crate::lsp::bridge::uninstall_hover(state);
                completion::uninstall(state);
            }
        }
    }

    /// 【LSP】按当前文档重新向宿主要服务（建面板 / 切模式 / 泵每拍都调用）
    ///
    /// 会话有变化才动作：换会话先给旧服务发 `didClose`（尽力），清旧诊断，再 `didOpen`
    /// 新会话并置健康；没变化 = 无操作（不重复 didOpen）。服务不可得 = 如实摘干净。
    ///
    /// **两个判据都不可省**：① `uri` / `language_id` 相同也可能是**换了连接**（server 崩溃
    /// 重启后 uri 不变），所以并上 `Arc::ptr_eq`；② 末尾**无条件**按「会话健康」或「本该有
    /// 服务」决定是否保持泵——`.py` 文档初始化期间正是「没会话但想要」，early return 会错过它。
    fn sync_lsp_session(&mut self, cx: &mut Context<Self>) {
        let session = self.shared.lsp_service(&self.document);
        let same = match (&self.lsp, &session) {
            (Some(old), Some(new)) => {
                old.uri == new.uri
                    && old.language_id == new.language_id
                    && std::sync::Arc::ptr_eq(&old.client, &new.client)
            }
            (None, None) => true,
            _ => false,
        };
        if !same {
            // 旧会话收尾（尽力）：关文档 + 清诊断（服务端缓存别留垃圾）
            if let Some(old) = self.lsp.take() {
                let _ = old.client.did_close(&old.uri);
            }
            self.lsp_diagnostics.clear();
            self.lsp_version = 0;
            self.lsp_alive = false;
            if let Some(session) = session {
                let text = self.editor_text(cx);
                let _ = session.client.did_open(&session.uri, &session.language_id, &text);
                self.lsp_version = 1;
                self.lsp_alive = true;
                self.lsp = Some(session);
            }
            // 会话变了（挂或摘）都要整体重画一遍三源诊断：摘了不清内核里旧的 LSP 标记
            self.apply_error_marks(self.error_reason.clone(), cx);
            // provider 的摘 / 挂跟着会话走（泵里重连也靠这里——不只 `new` / `sync_mode` 那两次）
            self.editor.update(cx, |state, _cx| {
                self.sync_completion_provider(state);
            });
        }
        // 会话健康，或「本该有服务但还没起 / 已被停」→ 保持轮询（重启后自动重连靠这条）
        if self.lsp_alive || self.shared.lsp_wants(&self.document) {
            self.ensure_exec_pump(cx);
        }
    }

    /// 把文档内容推回内核（另存为 / 外部修改后重新加载时调用）
    pub fn reload_from_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.with_document(|doc| doc.content().to_string()) else {
            return;
        };
        // 【2d/T2d-5】分析模式：正文是 `.rdsnote` JSON（会话 + 单元）——解析进笔记本持有器，
        // 渲染读它；解析失败如实报（不假装打开成功）。其它模式照旧推内核文本。
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        if mode == EditorMode::Analysis {
            if text.trim().is_empty() {
                self.shared.set_notebook_state(&self.document, crate::cell::NotebookState::new());
            } else {
                match crate::cell::NotebookFile::from_json(&text) {
                    Ok(file) => {
                        let (session, cells) = file.into_state();
                        self.shared.set_notebook_state(
                            &self.document,
                            crate::cell::NotebookState {
                                session,
                                cells,
                            },
                        );
                    }
                    Err(reason) => {
                        self.set_message(Some(format!("打开 .rdsnote 失败：{reason}")), cx);
                    }
                }
            }
        }
        self.statements = count_statements(&text);
        let value_changed = {
            let before = self.editor.read(cx).value().to_string();
            let changed = before != text;
            self.editor.update(cx, |state, cx| {
                if changed {
                    state.set_value(text.clone(), window, cx);
                }
                // **`set_value` 不发 `InputEvent::Change`**（内核里 `emit_events = false`，`state.rs:901`），
                // 所以整篇换文本这条路不会经过事件订阅——折叠候选必须在这里显式重算，
                // 否则重新加载后候选还指着旧文本的行号。
                sync_fold(&self.shared, &self.document, state, window, cx);
            });
            changed
        };
        // 【LSP】同上：整篇换文本不走 Change 事件，didChange 也要在这里显式补发（版本自增）
        if value_changed && self.lsp_alive {
            if let Some(session) = self.lsp.as_ref() {
                self.lsp_version += 1;
                let _ = session.client.did_change(&session.uri, self.lsp_version, &text);
            }
        }
        // 【2c】同上：隐藏字符标注也走不到 Change 事件（区间是字节偏移，整篇换文本必失效），
        // 开着显示就显式重算一遍；没开则集合未建，直接跳过。
        if self.shared.document_show_invisible(&self.document) {
            let ranges = crate::hidden::hidden_ranges(&self.editor_text(cx));
            self.refresh_invisible_decorations(ranges, cx);
        }
        cx.notify();
    }

    // ===== 动作（A10）=====
    //
    // 动作只改共享状态或向内核写文本，不直接操作 Dock（关闭走 TabGroup 的请求路径）。

    /// `Ctrl+S`：保存；未命名 / 写盘失败 → 状态栏给出原因
    pub(crate) fn on_save(
        &mut self,
        _: &SaveDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let (start, query, items) =
            completion::items_at(&self.shared, &self.document, &text, offset);
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
            self.editor.update(cx, |state, cx| {
                state.set_selected_range(start + from..start + to, cx)
            });
        }
        self.set_message(Some(format!("已插入模板「{name}」")), cx);
    }

    /// 【T2c-5】工具栏单项动作的统一分发（渲染只按 id 挂事件，语义集中在这里）
    ///
    /// 折叠 ▾ 菜单点击项也走这里（执行族）。`More` 是嵌套菜单本身、不参与折叠菜单，
    /// 这里兜底空转（不会真被调到——`render_collapsed_group` 已把它过滤掉）。
    /// 格式化的 `&mut Window` 从事件回调（按钮 / 菜单项都带窗口）透传进来。
    pub(crate) fn run_toolbar_item_action(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::toolbar::ToolbarItemId;
        match id {
            ToolbarItemId::ExecRun => self.execute_preferring_selection(cx),
            ToolbarItemId::ExecAll => self.run_all(cx),
            ToolbarItemId::ExecCurrent => self.execute_menu_kind(ExecMenuKind::CurrentStatement, cx),
            ToolbarItemId::ExecSelection => self.execute_menu_kind(ExecMenuKind::Selection, cx),
            ToolbarItemId::ExecBatch => self.execute_menu_kind(ExecMenuKind::Batch, cx),
            ToolbarItemId::Format => self.format_document(window, cx),
            // Explain / Translate / Template / TxAutoCommit / History 是带下拉的文档级
            // 按钮（`dropdown_menu` 回调内直接调方法，不走这条兜底）；折叠菜单只收执行族
            // （见 `render_collapsed_group`），这里兜底空转（不会真被调到）
            ToolbarItemId::Explain => {}
            ToolbarItemId::Translate => {}
            ToolbarItemId::Template => {}
            ToolbarItemId::TxAutoCommit => {}
            ToolbarItemId::History => {}
        }
    }

    /// 【T2c-5】按显式菜单目标执行（`target_for_menu`；工具栏显式目标按钮与折叠 ▾ 菜单共用）
    pub(crate) fn execute_menu_kind(&mut self, kind: ExecMenuKind, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::target_for_menu(kind, &text, selection);
        self.execute(target, kind.placement(), cx);
    }

    /// 【T2c-5】面板：切换某项可见性（`fixed_visible` 项不可隐藏，面板上 Checkbox 置灰）
    ///
    /// 返回是否真的变了（测试断言「不可隐藏」用）。
    pub(crate) fn set_toolbar_visible(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        on: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.toolbar.set_visible(id, on);
        cx.notify();
        changed
    }

    /// 【T2c-5】面板：单项移动（可跨组；跨过分隔符自然形成新组）
    pub(crate) fn move_toolbar_item(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_item(id, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：整组移动（含段尾分隔符；`false` = 已在边缘）
    pub(crate) fn move_toolbar_group(
        &mut self,
        head: crate::toolbar::ToolbarItemId,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_group(head, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：切换某组折叠（折叠 = 只显示首组件 + ▾）
    pub(crate) fn toggle_toolbar_collapsed(
        &mut self,
        head: crate::toolbar::ToolbarItemId,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.toggle_collapsed(head);
        cx.notify();
    }

    /// 【T2c-5】面板：在左区末尾加一条竖线分隔符
    pub(crate) fn insert_toolbar_separator(&mut self, cx: &mut Context<Self>) {
        self.toolbar.insert_separator();
        cx.notify();
    }

    /// 【T2c-5】面板：移除某条竖线分隔符
    pub(crate) fn remove_toolbar_separator(&mut self, index: usize, cx: &mut Context<Self>) {
        self.toolbar.remove_separator(index);
        cx.notify();
    }

    /// 【T2c-5】面板：移动某条竖线分隔符（跨过一项即改变组边界）
    pub(crate) fn move_toolbar_separator(
        &mut self,
        index: usize,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_separator(index, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：重置默认（清折叠 / 可见性回默认 / 删新增分隔符）
    pub(crate) fn reset_toolbar(&mut self, cx: &mut Context<Self>) {
        self.toolbar.reset();
        cx.notify();
    }

    /// 【T2c-5】测试访问器：当前工具栏布局快照（断言折叠 / 可见性 / 顺序）
    pub fn toolbar_layout_for_test(&self) -> crate::toolbar::ToolbarLayout {
        self.toolbar.clone()
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
        if let Err(reason) = self
            .channel_ready()
            .and_then(|()| self.channel_write_check(&target))
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
            (true, Some(since)) => {
                format!("TX 已开启 {}", status_bar::elapsed_text(since.elapsed()))
            }
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

    /// 【2b】脚本输出卡片（供测试断言脚本执行的结果形态；`None` = 还没有结论）
    pub fn script_card_for_test(
        &self,
    ) -> Option<crate::view::results::script_card::ScriptCard> {
        self.script_card.clone()
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
    #[cfg_attr(not(test), allow(dead_code))]
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

    /// 【T2c-3 收尾】光标数（多光标 = 列编辑；经 vendor 公开的 `cursor_count` 读内核真实值）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor_count_for_test(&self, cx: &App) -> usize {
        self.editor.read(cx).cursor_count()
    }

    /// 【T2c-3 收尾】给活动光标加一个下方光标（列编辑多光标；调内核公开的 `add_cursor_below_line`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn add_cursor_below_for_test(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.add_cursor_below_line(cx));
    }

    /// 【T2c-3 收尾】直调内核公开的 `select_columnar_block` 框选一块（列编辑鼠标框选）——
    /// 同 Alt+Shift 拖拽产出的矩形选区，免鼠标捕获，供窗口用例验证框选路径真到了内核。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn select_columnar_block_for_test(
        &mut self,
        start_offset: usize,
        end_offset: usize,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            state.select_columnar_block(start_offset, end_offset, cx)
        });
    }

    /// 【T2c-3 收尾】所有光标字节偏移（活动在前；经 vendor 公开的 `cursor_offsets` 读内核真实值）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor_offsets_for_test(&self, cx: &App) -> Vec<usize> {
        self.editor.read(cx).cursor_offsets()
    }

    /// 【T2c-3 收尾】当前编辑模式（行 / 列）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn edit_mode_for_test(&self) -> EditMode {
        self.edit_mode
    }

    /// 【T2c-3 收尾】设置编辑模式（走生产代码同一条 `set_edit_mode`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_edit_mode_for_test(&mut self, mode: EditMode, cx: &mut Context<Self>) {
        self.set_edit_mode(mode, cx);
    }

    /// 【T2c-3 收尾】按当前模式对齐内核光标（走生产代码同一条 `enforce_edit_mode`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn enforce_edit_mode_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enforce_edit_mode(window, cx);
    }

    /// 启动结果轮询（已有存活任务时不重复启动）
    ///
    /// 与 `workbench` 的导航任务同一模式：**后台等、主线程回填**，空闲即退出。
    /// 执行期间顺带 `cx.notify()`：状态栏的耗时累加跟着这个节拍走（不另起一个定时器）。
    /// 【B18】提交一次后台扫描（**每次文本变更**一次；打开文档与重新加载各一次）
    ///
    /// 修订号只升不降（回绕也不怕：比较的是与当前值相等）；范围由档位的降级表回答
    /// （`EditorShared::scan_scope`），不在这里另写判据。
    fn submit_scan(&mut self, text: String, cx: &mut Context<Self>) {
        let scope = self.shared.scan_scope(&self.document);
        let channel = self
            .with_document(|doc| doc.channel())
            .unwrap_or_else(Default::default);
        self.scan_revision = self.scan_revision.wrapping_add(1);
        self.scanner
            .submit(self.scan_revision, text, channel, scope);
        self.scan_pending = true;
        // 扫描不是“执行”，但它也要人按节拍把回执取回来——共用同一条轮询泵
        self.ensure_exec_pump(cx);
    }

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
                        // 【LSP】每拍重问宿主：握手就绪 / 服务被停 / 崩溃重启都在这里收敛
                        this.sync_lsp_session(cx);
                        this.drain_exec_results(cx);
                        if this.pending > 0 || this.tx_pending > 0 || this.refresh_pending > 0 {
                            cx.notify();
                        }
                        // 事务动作 / 重新挂载 / DuckDB 导出的回执还没到也要继续轮询（它们不在“执行忙”里）
                        this.shared.is_executing()
                            || this.tx_pending > 0
                            || this.refresh_pending > 0
                            || this.export_pending > 0
                            // 【B18】后台扫描在途（回执到手就结束，不为扫描留常驻定时器）
                            || this.scan_pending
                            // 【LSP】语言服务是常驻连接：诊断随时会来，会话健康就一直轮询
                            || this.lsp_alive
                            // 【LSP】本该有服务但还没起 / 被停（重启后自动重连靠这条）
                            || this.shared.lsp_wants(&this.document)
                    })
                    .unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        });
        *self.exec_pump.borrow_mut() = Some(task);
    }

    /// 【LSP】取走语言服务事件并按本文档折叠（诊断 → 三源合并；IoError → 摘服务 / 报告）
    ///
    /// 纯逻辑在 [`crate::lsp::host::fold_events`]（按 `uri` 认领 + 后者覆盖），这里只落面板状态：
    /// 有诊断重画错误标记；服务断开则摘 provider（补全、轮询一起停）并给状态栏原因。
    fn drain_lsp_events(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.lsp.clone() else {
            return;
        };
        let events = self.shared.drain_lsp_events(&self.document);
        if events.is_empty() {
            return;
        }
        let fold = crate::lsp::host::fold_events(&session.uri, events);
        let mut changed = false;
        if let Some(diags) = fold.diagnostics {
            self.lsp_diagnostics = diags;
            changed = true;
        }
        if let Some(reason) = fold.io_error {
            self.lsp_alive = false;
            self.lsp_diagnostics.clear();
            changed = true;
            // 服务死了：清僵尸会话（否则重启时 `same` 判据会挡住重连），并给状态栏原因
            self.lsp.take();
            self.set_message(Some(format!("语言服务已断开：{reason}")), cx);
            // 服务死了：补全不再走语言服务（摘 provider，回到无 / 本地路径）
            self.editor.update(cx, |state, _cx| {
                self.sync_completion_provider(state);
            });
        }
        if changed {
            self.apply_error_marks(self.error_reason.clone(), cx);
            cx.notify();
        }
    }

    /// 取回已完成的执行，填入结果存储与网格
    ///
    /// **一条语句一条结论**：批量执行会连续回来多条，每条各自落一个结果集（落位由 outcome
    /// 自带）。本文档的回填计数减到 0 才算执行完。
    pub(crate) fn drain_exec_results(&mut self, cx: &mut Context<Self>) {
        // 【LSP】语言服务事件先收（诊断异步推送，独立于执行通道）
        self.drain_lsp_events(cx);
        // 【B18】后台扫描的回执先收：**只接受与当前修订号相等的那一份**（陈旧丢弃），
        // 然后三件产物各归各位：语句数进状态栏、候选喂内核、诊断与执行错误一起重画
        if let Some(report) = self.scanner.try_recv() {
            self.scan_pending = false;
            if crate::scan::is_current(&report, self.scan_revision) {
                self.statements = report.statements;
                self.scan_diagnostics = report.diagnostics;
                let folds = report.folds;
                self.editor
                    .update(cx, |state, cx| fold::refresh(state, folds, cx));
                self.apply_error_marks(self.error_reason.clone(), cx);
                cx.notify();
            }
        }
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
                Err(reason) => {
                    self.set_message(Some(format!("{}失败：{reason}", note.action.label())), cx)
                }
            }
        }

        // 【T2a-6】批量提交的回执（成功/冲突/失败都要说清楚；冲突格在变更集里标出来）
        //
        // 组装规则：执行器按语句顺序报影响行数（`per_statement`），面板拿**生成时存的**
        // 语句→(行,列) 索引（`CommitInFlight::per_line`）对齐——影响行数 0 = 这一行在源库
        // 已定位不到（键值被改/行被删），该行所有变更列标冲突；≥1 = 这一格真的写到了，标已提交。
        for note in self.shared.drain_commit_notes() {
            if note.document != self.document {
                continue;
            }
            match note.result {
                Ok(mut report) => {
                    let Some(inflight) = self.commit_in_flight.take() else {
                        continue; // 过期回执（没有在途提交）——不误清别家状态
                    };
                    // 组装冲突回执（执行器不编造行/列，这里补全）：影响行数 0 = 这一行在源库
                    // 已定位不到（键值被改 / 行被删），该行所有变更列标冲突；≥1 = 这一格真的写到了
                    let mut committed = 0usize;
                    for (i, (row, columns)) in inflight.per_line.iter().enumerate() {
                        let affected = report.per_statement.get(i).copied().unwrap_or(0);
                        for &column in columns {
                            if affected >= 1 {
                                committed += 1;
                                self.shared.update_results(|store| {
                                    store.mutate_change_set(
                                        &note.document,
                                        inflight.set_index,
                                        |changes| changes.mark_committed(*row, column),
                                    );
                                });
                            } else {
                                report.conflicts.push((
                                    *row,
                                    column,
                                    "影响行数 0：源库里的旧值已被别人改过".to_string(),
                                ));
                                self.shared.update_results(|store| {
                                    store.mutate_change_set(
                                        &note.document,
                                        inflight.set_index,
                                        |changes| changes.mark_conflict(*row, column),
                                    );
                                });
                            }
                        }
                    }
                    let mut message = format!("已提交 {committed} 条变更");
                    if !report.conflicts.is_empty() {
                        message.push_str(&format!(
                            "（{} 条冲突：源库里的旧值已被改过）",
                            report.conflicts.len()
                        ));
                    }
                    if inflight.skipped > 0 {
                        message.push_str(&format!("（{} 行键值为空，已跳过）", inflight.skipped));
                    }
                    // 先同步网格（冲突格标色）再落文案：`sync_result_view` 末尾会按结果成败
                    // 重写状态栏（本次无失败 → `set_message(None)`），顺序反了文案会被清掉
                    self.sync_result_view(cx);
                    self.set_message(Some(message), cx);
                }
                Err(reason) => {
                    self.commit_in_flight = None;
                    self.set_message(Some(format!("批量提交失败：{reason}")), cx);
                }
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

        // 【2b】脚本执行的结论：独立通道，先于 exec outcomes 收（脚本与 SQL 互不挤占）
        self.drain_script_notes(cx);
        // 【2d】单元执行的结论：独立通道（结果走内联输出，不是结果区）
        self.drain_cell_notes(cx);

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
            // 【T2a-2】行标识探测：结果来自哪张表 → 宿主导航缓存查主键（内存读，不阻塞）。
            // 探测不到 = `None` → 判据 `NoRowKey` 如实置只读；有主键但结果没 SELECT 键列
            // → `row_key()` 也为 `None`（写回没有可绑定的值），同样只读。
            entry.key_columns = probe_primary_key(self, &entry);
            // 【P1】字段注释：血缘 + 表列元数据（内存读；长度与 `columns` 一致）
            entry.column_annotations = annotate_columns(self, &entry);
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
            self.shared
                .update_results(|store| store.push(entry, placement));
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
    /// 【遗留 #14】CSV 表格展示：把编辑器文本解析成表格填进结果网格
    ///
    /// 只对 CSV 文档生效（扩展名判定，口径与状态栏文件类型一致）；非 CSV 文档
    /// 把 `csv_active` 复位——**不动网格**，那是 SQL 执行结果的区域。
    /// 表头取第一行（CSV 常见约定）；列类型未知，全部 `None`（不做类型推断）。
    pub fn sync_csv_grid(&mut self, cx: &mut Context<Self>) {
        let is_csv = self
            .document_path()
            .as_deref()
            .and_then(crate::mode::script_file_type)
            == Some(crate::mode::ScriptFileType::Csv);
        if !is_csv {
            self.csv_active = false;
            return;
        }
        let text = self.editor_text(cx);
        let rows = engine::csv::parse(&text);
        let (columns, body) = match rows.split_first() {
            Some((header, rest)) => (header.clone(), rest.to_vec()),
            None => (Vec::new(), Vec::new()),
        };
        let column_count = columns.len();
        self.grid.update(cx, |state, cx| {
            state.delegate_mut().set_data(columns, body);
            // 类型必须**在 set_data 之后**设（set_data 会清空类型，防串台）
            state.delegate_mut().set_column_types(vec![None; column_count]);
            state.refresh(cx);
        });
        self.csv_active = true;
    }

    /// 【遗留 #14】CSV 表格展示：复制整个 CSV 表格（TSV，与结果集复制同口径）
    pub fn copy_csv_table(&mut self, cx: &mut Context<Self>) {
        let (headers, rows) = self.grid.read(cx).dump(cx);
        if headers.is_empty() {
            self.set_message(Some("CSV 没有内容可复制".to_string()), cx);
            return;
        }
        let mut out = String::new();
        out.push_str(&headers.join("\t"));
        for row in &rows {
            out.push('\n');
            out.push_str(&row.join("\t"));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(out));
        self.set_message(Some(format!("已复制 {} 行（TSV）", rows.len())), cx);
    }

    fn sync_result_view(&mut self, cx: &mut Context<Self>) {
        // 【P2-4】换结果 = 换一批行列：转置视图自动退出（delegate 侧已由 `set_data` 清掉，
        // 这里把面板开关同步复位——`toggled` 与网格状态保持同一份真值）
        self.transposed = false;
        self.transpose_row = 0;
        // 先把要用的数据从权威存储里拷出来（不把 `Ref` 带进下面的 `grid.update`）
        let current_channel = self.channel();        let (
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
                entry.filter(|entry| entry.has_grid()).map(|entry| {
                    (
                        entry.columns.clone(),
                        entry.rows.clone(),
                        // 【Q6】列类型跟数据一起交给网格
                        entry.column_types.clone(),
                        // 【A9】来源表名（右键「为 INSERT」的目标；猜不到 = None → 不摆那一项）
                        crate::export::table_from_sql(&entry.sql),
                        // 【P1】字段注释（表头 tooltip；与列同序，空 vec = 没报）
                        entry.column_annotations.clone(),
                    )
                }),
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
                    lineage: entry.title.clone().or_else(|| entry.lineage.clone()),
                    // 【B15】筛选统计随后由 `refresh_filter_hint` 从网格真值填上
                    filtered: None,
                }),
                // 【B5】状态行（⑦）：只有网格才给（写语句的“共 0 行”、失败时的“共 0 行”都是噪音）
                entry
                    .filter(|entry| entry.has_grid())
                    .map(|entry| ResultStatus {
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
        // 【T2a-3/4/6】这份结果能不能编辑（入口级判据）+ 脏格 / 冲突格集合（投影自权威变更集）
        //
        // 冲突格与脏格分开投影：冲突格（提交没写进去）不是待提交变更，底色用 danger，
        // 网格里 `set_conflict` 与 `set_dirty` 分开标，渲染时互不覆盖。
        let (editable, dirty, conflict) = {
            let store = self.shared.results();
            let editable = store.active(&self.document).is_some_and(|entry| {
                crate::editability::result_editable(
                    self.channel(),
                    crate::project::ProjectState {
                        read_only: self.shared.project_read_only(),
                    },
                    self.read_only_flags(),
                    entry.row_key().as_ref(),
                )
                .is_ok()
            });
            let change_set = store.change_set(&self.document, active);
            let dirty = change_set
                .map(|changes| {
                    changes
                        .pending()
                        .into_iter()
                        .map(|change| (change.row, change.column))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let conflict = change_set
                .map(|changes| changes.conflicts())
                .unwrap_or_default();
            (editable, dirty, conflict)
        };
        self.grid.update(cx, |state, cx| {
            match grid_data {
                Some((columns, rows, types, table, annotations)) => {
                    state.delegate_mut().set_data(columns, rows);
                    // 【Q6】类型必须**在 set_data 之后**设：`set_data` 会清空类型（防"上一份结果的类型套到新数据上"）
                    state.delegate_mut().set_column_types(types);
                    // 【A9】来源表名同理（set_data 会清掉旧的；紧跟同步，漏给 = 不摆 INSERT 项）
                    state.delegate_mut().set_result_table(table);
                    // 【P1】字段注释同理（set_data 会清掉旧的；紧跟同步，漏给 = 表头只画列名）
                    state.delegate_mut().set_column_annotations(annotations);
                }
                None => state.delegate_mut().clear(empty),
            }
            // 【M8】这份结果能不能洞察（菜单项据此出现 / 消失）
            state
                .delegate_mut()
                .set_insight_available(insight_available);
            // 【B5b】这份结果还能不能再取一段 / 是不是正在取
            state.delegate_mut().set_has_more(has_more);
            state.delegate_mut().set_loading_more(loading_more);
            // 【T2a-4/6】单元格编辑开关 + 脏格 / 冲突格底色（双击进编辑 / 标脏 / 冲突都看这里）
            state.delegate_mut().set_editable(editable);
            state.delegate_mut().set_dirty(dirty);
            state.delegate_mut().set_conflict(conflict);
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
    ///
    /// 【B18】它同时负责**三个来源的合并**：执行错误回填 + 打字期词法诊断 + LSP 语言服务
    /// 诊断。三源必须走同一条重建流程：内核的诊断集合只有 `clear` + `push`，分开各写一份
    /// 就会互相擦掉（“执行出错了，词法诊断没了”那种抳不到的怪象）。
    fn apply_error_marks(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        self.error_reason = reason;
        let site = self.error_site.clone();
        let reason = self.error_reason.clone();
        let lexical = self.scan_diagnostics.clone();
        let lsp_diags = self.lsp_diagnostics.clone();
        self.editor.update(cx, |state, cx| {
            // 文本先取：`diagnostics_mut` 借的就是同一份状态，两个借用不能同时活着
            let text = state.value().to_string();
            let Some(diagnostics) = state.diagnostics_mut() else {
                return;
            };
            diagnostics.clear();
            // 打字期诊断（词法层能证明的那些；来源标“语法”以与执行错误区分）
            for one in &lexical {
                let ((start_line, start_column), (end_line, end_column)) = one.positions(&text);
                diagnostics.push(
                    Diagnostic::new(
                        Position::new(start_line as u32, start_column as u32)
                            ..Position::new(end_line as u32, end_column as u32),
                        one.message.clone(),
                    )
                    .with_severity(DiagnosticSeverity::Error)
                    .with_source("语法")
                    .with_code(one.code),
                );
            }
            // LSP 语言服务诊断（内核有 `From<lsp_types::Diagnostic>`，source 一并带过来）
            for one in &lsp_diags {
                diagnostics.push(Diagnostic::from(one.clone()));
            }
            if let (Some(site), Some(reason)) = (site, reason) {
                let (end_line, end_column) = site.end();
                let range = Position::new(
                    site.line.saturating_sub(1) as u32,
                    site.column.saturating_sub(1) as u32,
                )
                    ..Position::new(
                        end_line.saturating_sub(1) as u32,
                        end_column.saturating_sub(1) as u32,
                    );
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

    /// M5 命中跳行：把光标/选区移到 `line`（1 基行号）行，并聚焦面板。
    ///
    /// `span`（行内命中字节区间，来自草稿箱内容搜索）有值时**选中命中文本**；
    /// 无值时光标落在行首。行号越界 / 区间非法时静默退化为「光标放行首」，再不行就只聚焦。
    /// 与 `jump_to_error_site_with` 同口径（`set_selected_range` 会滚到可见处）。
    pub fn reveal_search_target(
        &mut self,
        line: u32,
        span: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let full = self.editor.read(cx).text().to_string();
        let Some(line_start) = diagnostics::offset_for_line_col(&full, line as usize, 1) else {
            return;
        };
        let range = match span {
            Some((s, e)) if s < e => line_start + s..line_start + e,
            _ => line_start..line_start,
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
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

    /// 【P2-4】转置视图开关（单行转置，DBeaver 式）：开 = 把当前行竖排成
    /// 「# / 字段 / 值」三列；关 = 原网格原样回来（排序 / 冻结 / 隐藏都没动过）。
    /// 切换在事件路径里改 delegate + `refresh`（render 是纯读，不在这里做状态）
    fn toggle_transpose(&mut self, cx: &mut Context<Self>) {
        let open = !self.transposed;
        self.transposed = open;
        let row = self.transpose_row;
        self.grid.update(cx, |state, cx| {
            state.delegate_mut().set_transposed(open, row);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【P2-4】转置行导航：上一行 / 下一行（越界夹取；数据行数来自 delegate）
    fn transpose_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let max = self
            .grid
            .read(cx)
            .delegate()
            .data_row_count()
            .saturating_sub(1);
        let next = (self.transpose_row as isize + delta).clamp(0, max as isize) as usize;
        self.transpose_row = next;
        self.grid.update(cx, |state, cx| {
            state.delegate_mut().set_transpose_row(next);
            state.refresh(cx);
        });
        cx.notify();
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
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("这份结果没有可重跑的 SQL".to_string()), cx);
            return;
        };
        // 【A2】保持形态：下发筛选 / 排序下发的结果集带当初的原料重查（不退回全量）；
        // 普通执行 = 原 SQL 重跑。
        let target = match entry.pushdown.clone() {
            Some(execution::Pushdown::Filtered { filter, columns }) => {
                ExecTarget::Filtered {
                    sql: entry.sql.clone(),
                    filter,
                    columns,
                }
            }
            Some(execution::Pushdown::SortedDown {
                column,
                descending,
            }) => ExecTarget::SortedDown {
                sql: entry.sql.clone(),
                column,
                descending,
            },
            None => ExecTarget::Statement(entry.sql.clone()),
        };
        self.execute(target, ResultPlacement::Replace, cx);
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
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_loading_more(false)
            });
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
            executor.timer(std::time::Duration::from_millis(300)).await;
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

    /// 【遗留 #7】右键「查看完整值」：记录目标并弹值查看器（对话框由 dialogs 层开）
    ///
    /// 钩子给的是 `&mut App`（拿不到窗口），对话框要 `&mut Window`——所以经
    /// `self.window.update` 转手调 [`Self::open_value_preview_with_window`]。
    /// headless 测试里窗口不在 `App::windows`（测试上下文持有），转手会报
    /// `window not found` 而弹不出——这是测试环境限制，真实窗口下正常。
    pub(crate) fn open_value_preview(
        &mut self,
        value: &str,
        column: &str,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        self.value_preview = Some(ValuePreview {
            value: value.to_string(),
            column: column.to_string(),
            row,
        });
        let target = self.value_preview.clone().expect("刚写入");
        let result = self.window.update(cx, move |_panel, window, app| {
            // 转手闭包里 window 是真实窗口：直接弹
            Self::open_value_preview_with_window_impl(
                window,
                app,
                target.value.clone(),
                target.column.clone(),
                target.row,
            );
        });
        // 窗口句柄不可用（headless / 窗口已关）→ 留一句可读原因，不静默
        if let Err(error) = result {
            self.set_message(Some(format!("值查看器弹不出：{error}")), cx);
        }
    }

    /// 【遗留 #7】带窗口的值查看器入口（测试走它做窗口级验收；生产钩子经
    /// [`Self::open_value_preview`] 转手调同一实现）
    pub(crate) fn open_value_preview_with_window(        &mut self,
        value: &str,
        column: &str,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.value_preview = Some(ValuePreview {
            value: value.to_string(),
            column: column.to_string(),
            row,
        });
        Self::open_value_preview_with_window_impl(
            window,
            cx,
            value.to_string(),
            column.to_string(),
            row,
        );
    }

    /// 弹层实现（与窗口来源无关）：`dialogs::open_value_preview` 是模态弹层，
    /// 复制不关弹层（用户复制完可能还要接着看）
    fn open_value_preview_with_window_impl(
        window: &mut Window,
        cx: &mut App,
        value: String,
        column: String,
        row: usize,
    ) {
        dialogs::open_value_preview(
            window,
            cx,
            value.clone(),
            column.clone(),
            row,
            move |app| app.write_to_clipboard(ClipboardItem::new_string(value.clone())),
        );
    }

    /// 【遗留 #7】只读访问器：值查看器目标是否已记录（测试断言用；`ValuePreview` 是私有类型，只暴露 bool）
    pub(crate) fn value_preview_for_test(&self) -> bool {
        self.value_preview.is_some()
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
            self.set_message(
                Some("分析 SQL 为空（写一条 SELECT 再运行）".to_string()),
                cx,
            );
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
    pub(crate) fn export_active_result(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
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
                    Some(format!(
                        "正在导出 {}（首次装 excel 扩展可能要几秒）…",
                        format.label()
                    )),
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
        // 【B7 切片三】`SQL Updates` 的键列：与菜单同一来源（表结构端口），
        // 给了键就真编码成 UPDATE（`encode_rows_with_key`）；没键走普通编码
        // （`WHERE 子句` 不需要键，其余档不受影响）
        let key = self
            .bound_connection()
            .as_deref()
            .and_then(|conn| {
                self.shared
                    .table_key_columns(Some(conn), &table)
            })
            .and_then(|columns| export::RowKey::new(columns));
        let text = match &visible {
            Some(rows) => export::encode_rows_with_key(entry, format, &table, key.as_ref(), rows),
            None => export::encode_rows_with_key(entry, format, &table, key.as_ref(), &entry.rows),
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
        // 【2b】通用模式的脚本输出卡片走**独立通道**（本地进程，没有连接概念，不占
        // `execute` 能力位）：有卡片或脚本在跑时结果区照常显示。其余模式沿用
        // 「能执行才有结果区」（SQL / 分析模式不会出现 script_card，此分支天然互斥）。
        let script_showing = self.script_card.is_some() || self.pending > 0;
        (self.execution_allowed() && (self.pending > 0 || self.result_toolbar.is_some()))
            || script_showing
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

impl Drop for EditorHostPanel {
    /// 面板关闭（文档随之关闭）：给语言服务发 `didClose`（尽力），别把文档留在服务端缓存里。
    ///
    /// Drop 期间不做任何面板 / 内核操作——只发一条不期待响应的通知；client 已停机时
    /// `notify` 返回错误，静默忽略。
    fn drop(&mut self) {
        if let Some(session) = self.lsp.take() {
            let _ = session.client.did_close(&session.uri);
        }
    }
}

/// 【T2c-5】第一个组的组头（组移动↑↓ 的左边缘判定）
fn group_of_first(groups: &[crate::toolbar::ToolbarGroup]) -> crate::toolbar::ToolbarItemId {
    groups.first().map(|g| g.head).unwrap_or(crate::toolbar::ToolbarItemId::ExecRun)
}

/// 【T2c-5】最后一个组的组头（组移动↑↓ 的右边缘判定）
fn group_of_last(groups: &[crate::toolbar::ToolbarGroup]) -> crate::toolbar::ToolbarItemId {
    groups.last().map(|g| g.head).unwrap_or(crate::toolbar::ToolbarItemId::ExecRun)
}

/// 执行结论 → 结果记录（视图模型转换，不在这做任何 I/O）
/// 【T2a-2】结果集的行标识探测：从结果 SQL 猜表名 → 宿主导航缓存查主键列（内存读）
///
/// 返回 `None` 的四种情况（都如实置只读，不编造行标识）：
/// - 失败结果 / 写语句（没有可编辑的行）——不探测，省一次查询；
/// - 未绑定连接（`entry.connection` 为空）——不知道去哪张表；
/// - 宿主没接补全端口 / 端口不支持探测——如实没有行标识；
/// - 缓存没这张表 / 表没有主键——探测结果为空，同样给 `None`。
fn probe_primary_key(panel: &EditorHostPanel, entry: &ResultEntry) -> Option<Vec<String>> {
    if entry.error.is_some() || entry.columns.is_empty() {
        return None;
    }
    let Some(connection) = entry.connection.as_deref() else {
        return None;
    };
    let table = export::default_table_name(Some(&entry.sql), 0);
    panel
        .shared
        .primary_key_for(Some(connection), entry.channel, &table)
        .filter(|columns| !columns.is_empty())
}

/// 【P1】结果集字段注释：血缘（sqlglot 投影映射）+ 表列元数据（宿主导航缓存，内存读）
///
/// 与 [`probe_primary_key`] 同口径：失败结果 / 写语句 / 未绑定连接不探测；
/// 宿主没接注释端口 = 全 `None`（渲染只画列名）。长度与 `entry.columns` 一致。
fn annotate_columns(panel: &EditorHostPanel, entry: &ResultEntry) -> Vec<Option<ColumnAnnotation>> {
    if entry.error.is_some() || entry.columns.is_empty() {
        return vec![];
    }
    let Some(connection) = entry.connection.as_deref() else {
        return vec![None; entry.columns.len()];
    };
    panel
        .shared
        .column_annotations_for(Some(connection), entry.channel, &entry.sql, entry.columns.len())
}

fn entry_from(outcome: execution::ExecOutcome) -> ResultEntry {    let connection = outcome.connection.clone();
    // 【B13】通道也随结论回来：标签星徽标与“切通道后标灰”靠它
    let channel = outcome.channel;
    // 【B15】血缘：失败也照样带（“这句话是下发筛选发出去的”对排查同样有用）
    let lineage = outcome.lineage;
    // 【A2】形成原料（下发筛选 / 排序下发）：刷新 ⟳ 按它重查，保持形态
    let pushdown = outcome.pushdown;
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
        // 【Q6】列类型随结果一起进结果集（空 = 驱动报不出 → 网格走“无类型档”）
        .with_column_types(&data.column_types)
        .with_connection(connection)
        .with_channel(channel)
        .with_analysis(outcome.analysis)
        .with_lineage(lineage)
        .with_pushdown(pushdown),
        Err(error) => ResultEntry::failure(outcome.document, outcome.sql, error, 0)
            .with_connection(connection)
            .with_channel(channel)
            .with_lineage(lineage)
            .with_pushdown(pushdown),
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
fn pick_and_save_as(panel: &Entity<EditorHostPanel>, cx: &mut App) -> Result<(), SaveAsFailure> {
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
    let picked = panel.read(cx).shared.pick_save_path(current, default_name);
    let Some(path) = picked else {
        return Err(SaveAsFailure::Cancelled);
    };

    panel
        .update(cx, |panel, cx| panel.save_as(&path, cx))
        .map(|_| ())?;
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
    dialogs::open_save_failure_confirm(window, cx, title, reason, move |choice, window, cx| {
        match choice {
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
        }
    });
}

/// 语句数：走 `engine::sql::split` 的**词法级**切分（不是 `split(';')`）
///
/// 只在内容变化时调一次（构造 / 输入回写 / 重新加载），渲染期只读缓存值。
fn count_statements(text: &str) -> usize {
    engine::sql::split_statements(text).len()
}

/// 折叠按能力与档位装 / 关（**唯一开关处**：新建、切模式、整篇换文本都走它）
///
/// 真值在 [`EditorShared::folding_enabled`]（能力表 + 大文件档位）；这里只把那个判断落到内核上，不另写一套。
/// 装不上就**真关开关**（`set_folding(false)`）：大文件档位下折叠每键要重扫全文，只留一个判据
/// 而不关开关就是把那份代价白付了。文本取自**内核当下值**（不另传参数），两处调用点因此不会
/// 把“服务层的文本”和“屏幕上的文本”搞混。
fn sync_fold(
    shared: &EditorShared,
    document: &DocumentId,
    state: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
) {
    if shared.folding_enabled(document) {
        let text = state.value().to_string();
        fold::install(state, &text, cx);
    } else {
        state.set_folding(false, window, cx);
    }
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
        // 【遗留 #10】分析模式：进 Dock 时把单元编辑器建齐（事件路径；构造期不能建）
        self.sync_cell_editors(window, cx);
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
        self.shared.update(|service| service.close(&self.document));
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
    ///
    /// 2026-10-19：自绘 ✕ 改为**悬停显示**（`title_hovered` 随标题区 hover 翻转），
    /// 平时只留脏点；鼠标移上标题才出现关闭按钮，避免常驻 ✕ 遮挡标题（#16）。
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        self.is_dirty().then(|| {
            // 脏态可见（脏点 + 自绘 ✕）：
            // Dock 内置 ✕ 在脏时被 `closable` 藏掉（防 `group.close` 直接丢修改），
            // 这里补一个走「保存/不保存/取消」确认的 ✕——与 Ctrl+W 同一入口
            // （`request_close_document`），保证“编辑区也有关闭入口”且不会误丢。
            let panel = cx.entity();
            let area = self.area.clone();
            let hovered = self.title_hovered.get();
            div()
                .id(format!("sp-dirty-hover-{}", panel.entity_id()))
                .h_flex()
                .items_center()
                .gap_1()
                .on_hover({
                    // hover 闭包用 clone：不消耗 `panel`，留给下方 on_click 用。
                    let panel_hover = panel.clone();
                    move |&hovering, _, app| {
                        panel_hover.update(app, |this, cx| {
                            if this.title_hovered.get() != hovering {
                                this.title_hovered.set(hovering);
                                cx.notify();
                            }
                        });
                    }
                })
                .child(
                    div()
                        .size(rems(ui::STATUS_DOT_SIZE))
                        .rounded_full()
                        .bg(cx.theme().colors.primary),
                )
                .child(
                    Button::new(format!("sp-dirty-close-{}", panel.entity_id()))
                        .icon(IconName::Close)
                        .xsmall()
                        .ghost()
                        .tab_stop(false)
                        .opacity(if hovered { 1.0 } else { 0.0 })
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            if let Some(area) = area.as_ref().and_then(|weak| weak.upgrade()) {
                                request_close_document(&area, panel.clone(), window, cx);
                            }
                        }),
                )
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

        // 光标 / 选区：从内核读真实值（行号列号按 1 基显示；Ch = 光标全文字符偏移；
        // 光标数 = 列编辑多光标真实值；选中行数 = 选区跨行数，供状态栏 `Row N` 微标）
        let (line, column, cursor_offset, cursor_count, selected_chars, selected_rows) = {
            let state = self.editor.read(cx);
            let cursor = state.cursor_position();
            let range = state.selected_range();
            let rows = if range.is_empty() {
                0
            } else {
                // 首尾偏移 → 行号差 + 1（0 基行号，差 +1 即跨行数）
                let start = state.text().offset_to_position(range.start).line;
                let end = state.text().offset_to_position(range.end).line;
                (end - start + 1) as usize
            };
            (
                cursor.line as usize + 1,
                cursor.character as usize + 1,
                state.selected_range().start,
                state.cursor_count(),
                state.selected_text().chars().count(),
                rows,
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
        // 【T2c-2】方言段：从绑定连接的 db_type 解析（会通信 + 绑定连接才有；
        // 未绑定连接不摆——「跟随当前连接」时方言是谁都说不准，如实不显示）
        let dialect_text = communicating.then(|| {
            let bound = self
                .with_document(|doc| doc.connection().map(str::to_string))
                .flatten();
            bound.as_deref().and_then(|conn_id| {
                self.shared
                    .connection_options()
                    .iter()
                    .find(|option| option.id == conn_id)
                    .map(|option| crate::connection::dialect_label(&option.db_type))
            })
        });
        // flatten 结果绑定成具名变量：`as_deref` 借它直到 render（临时值撑不过那条语句，E0716）
        let dialect_owned = dialect_text.flatten();
        // TX 区（B4）：会通信 + 执行器真支持事务 + **当前通道支持事务**
        // （加速 / 联邦没有事务语义，那时摆一个 TX 区就是摆了个假控件）
        let tx_available = communicating
            && self.shared.has_transactions()
            && current_channel.allows_transactions();
        let tx_text = tx_available.then(|| self.tx_text());
        // 【2b】通用模式：环境段（环境名 + 版本；非通用 / 未选 = 不显示）。
        // 环境只在通用模式才有语义（脚本执行目标），其它模式如实不摆。
        let env_text = (self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General)
            == EditorMode::General)
        .then(|| {
            self.env_for()
                .and_then(|id| self.envs().into_iter().find(|env| env.id == id))
                .map(|env| format!("环境 {}", env.display_name()))
        });
        // flatten 结果绑定成具名变量：`as_deref` 借它直到 render（临时值撑不过那条语句，E0716）
        let env_owned = env_text.flatten();
        // 【2b/遗留 #14】通用模式：脚本文件类型段（`.py` → `类型 Python`；非脚本扩展名不摆）。
        // 判定走 `mode::script_file_type` 纯函数；上色本体已接（`view::highlight` 按类型分派
        // 词法器），这里仍只展示类型名，与高亮互不干扰。
        let file_type_text = (self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General)
            == EditorMode::General)
            .then(|| {
                self.document_path()
                    .as_deref()
                    .and_then(crate::mode::script_file_type)
                    .map(|t| format!("类型 {}", t.label()))
            });
        let file_type_owned = file_type_text.flatten();
        // 【LSP】通用模式语言服务状态段（原型 §2.5 左侧环境族第三项「环境 · 文件类型 · LSP」）：
        // 只有“本文档配了语言服务”（`lsp_wants`，如 Python 脚本）才摆；会话健康显示「就绪」，
        // 服务没起 / 被停 / 崩溃时如实报「未就绪」——状态栏不假装有服务。
        let lsp_text = (self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General)
            == EditorMode::General)
            .then(|| {
                self.shared.lsp_wants(&self.document).then(|| {
                    if self.lsp_alive { "LSP 就绪" } else { "LSP 未就绪" }
                })
            });
        let lsp_owned = lsp_text.flatten();
        let status = StatusInputs {
            mode: self
                .with_document(|doc| doc.mode())
                .unwrap_or(EditorMode::General),
            dirty: self.is_dirty(),
            read_only: self.read_only_flags(),
            statements: self.statements,
            line,
            column,
            cursor_offset,
            cursor_count,
            // 缩进宽度：文档级权威值（T2c-4 重设后从这读；默认 2）
            tab_size: self.shared.document_tab_size(&self.document),
            // 缩进是否制表符：文档级权威值（T2c-4）
            hard_tabs: self.shared.document_hard_tabs(&self.document),
            // 【2c】保存格式（编码 + 换行）：状态栏右段微标真实值
            format: self.shared.document_save_format(&self.document),
            bytes: self.shared.document_len(&self.document),
            selected_chars,
            selected_rows,
            message: self.message.as_deref(),
            executing: self.pending > 0,
            elapsed: self.running_since.map(|since| since.elapsed()),
            connection: connection_text.as_deref(),
            dialect: dialect_owned.as_deref(),
            channel: channel_text.as_deref(),
            env: env_owned.as_deref(),
            file_type: file_type_owned.as_deref(),
            lsp: lsp_owned.as_deref(),
            tx: tx_text.as_deref(),
        };

        // 【T2c-1】连接 ▸ 库 ▸ Schema 面包屑（参考 DBeaver）：连接绑定且有 schema 列表才构造
        // （没有可切项就退化为文本连接段——`labels` 的 `connection` 段兜底，不造一个假控件）。
        // 点击 schema 段弹出列表：选中写回文档绑定（`None` = 用连接默认 schema）。
        let breadcrumb = communicating.then(|| {
            let conn_id = self.bound_connection();
            let schemas = self.shared.schemas(conn_id.as_deref());
            if schemas.is_empty() {
                return None;
            }
            let current = self
                .with_document(|doc| doc.schema().map(str::to_string))
                .flatten();
            let crumb = crate::connection::breadcrumb(
                conn_id.as_deref(),
                &self.shared.connection_options(),
                &schemas,
                current.as_deref(),
            );
            let mut label = crumb.connection.clone();
            if let Some(catalog) = crumb.catalog {
                label.push_str(" ▸ ");
                label.push_str(&catalog);
            }
            label.push_str(" ▸ ");
            label.push_str(crumb.schema.as_deref().unwrap_or("schema"));
            let entity = cx.entity();
            let items = schemas.clone();
            let label_clone = label.clone();
            Some(
                DropdownButton::new("editor-breadcrumb")
                    .small()
                    .button(
                        Button::new("editor-breadcrumb-btn")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-breadcrumb".to_string())
                            .label(label_clone),
                    )
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        for item in items.iter() {
                            let entity = entity.clone();
                            let schema = item.schema.clone();
                            menu = menu.item(
                                PopupMenuItem::new(schema.clone()).on_click(move |_, _window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.set_schema(Some(schema.clone()), cx);
                                    });
                                }),
                            );
                        }
                        // 分隔项：显式回到连接默认 schema（文档绑定解绑）
                        let entity = entity.clone();
                        menu.separator().item(
                            PopupMenuItem::new("使用连接默认").on_click(move |_, _window, app| {
                                entity.update(app, |panel, cx| panel.set_schema(None, cx));
                            }),
                        )
                    })
                    .into_any_element(),
            )
        });

        // 【T2c-3 收尾】行｜列编辑切换入口：列编辑 = 多光标（内核 `add_cursor_above|below`
        // 已由 vendor 补丁公开，`cursor_count` 徽标是真实值）。行编辑 = 单光标（文本变更时
        // 宿主强制收拢，见 Change 事件处理器）；切回行编辑时立即收拢一次。
        let edit_mode = {
            let entity = cx.entity();
            let mode = self.edit_mode;
            DropdownButton::new("editor-edit-mode")
                .small()
                .button(
                    Button::new("editor-edit-mode-btn")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-edit-mode".to_string())
                        .label(format!("{} ▾", mode.short_label())),
                )
                .dropdown_menu(move |menu, _window, _cx| {
                    let row_entity = entity.clone();
                    let col_entity = entity.clone();
                    let row = mode == EditMode::Row;
                    let column = mode == EditMode::Column;
                    menu.item(
                        PopupMenuItem::new("行编辑")
                            .checked(row)
                            .on_click(move |_, window, app| {
                                row_entity.update(app, |panel, cx| {
                                    panel.set_edit_mode(EditMode::Row, cx)
                                });
                                // 切回行编辑：立即收拢多光标（走生产代码同一条 `enforce_edit_mode`）
                                row_entity.update(app, |panel, cx| {
                                    panel.enforce_edit_mode(window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("列编辑")
                            .checked(column)
                            .on_click(move |_, _window, app| {
                                col_entity.update(app, |panel, cx| {
                                    panel.set_edit_mode(EditMode::Column, cx);
                                });
                            }),
                    )
                })
                .into_any_element()
        };

        // 【T2c-4】重设菜单（编码 / 换行 / 缩进 / 显示）：
        // - 缩进真改内核 TabSize（空格宽或制表符）；
        // - 编码 / 换行只改**保存契约**（内核文本统一 Unicode，IO 层按 `SaveFormat` 产出字节），
        //   切换即标脏（`format != saved_format`），见 `set_encoding` / `set_eol`；
        // - 显示组（空白字符 / 隐藏字符）是视图偏好：双写文档状态 + 内核（见下面各 `set_*`）。
        let reset_menu = {
            let entity = cx.entity();
            let current_tab = self.shared.document_tab_size(&self.document);
            let hard_tabs = self.shared.document_hard_tabs(&self.document);
            let format = self.shared.document_save_format(&self.document);
            let show_whitespaces = self.shared.document_show_whitespaces(&self.document);
            let show_invisible = self.shared.document_show_invisible(&self.document);
            DropdownButton::new("editor-reset-menu")
                .small()
                .button(
                    Button::new("editor-reset-menu-btn")
                        .ghost()
                        .small()
                        .debug_selector(|| "editor-reset-menu".to_string())
                        .label("重设 ▾"),
                )
                .dropdown_menu(move |menu, _window, _cx| {
                    // ── 编码组：5 项全亮；checked 按当前保存格式；on_click 只改保存契约 ──
                    let mut menu = menu;
                    let enc = format.encoding;
                    menu = menu.separator().item(PopupMenuItem::new("编码").disabled(true));
                    for (label, variant) in [
                        ("UTF-8", crate::encoding::TextEncoding::Utf8),
                        ("UTF-8 (BOM)", crate::encoding::TextEncoding::Utf8Bom),
                        ("GBK", crate::encoding::TextEncoding::Gbk),
                        ("UTF-16 LE", crate::encoding::TextEncoding::Utf16Le),
                        ("UTF-16 BE", crate::encoding::TextEncoding::Utf16Be),
                    ] {
                        let entity = entity.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(enc == variant)
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.set_encoding(variant, cx);
                                    });
                                }),
                        );
                    }
                    // ── 换行组：LF / CRLF ──
                    let eol = format.eol;
                    let lf_entity = entity.clone();
                    let crlf_entity = entity.clone();
                    menu = menu
                        .separator()
                        .item(PopupMenuItem::new("换行").disabled(true))
                        .item(
                            PopupMenuItem::new("LF")
                                .checked(eol == crate::encoding::LineEnding::Lf)
                                .on_click(move |_, _, app| {
                                    lf_entity.update(app, |panel, cx| {
                                        panel.set_eol(crate::encoding::LineEnding::Lf, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("CRLF")
                                .checked(eol == crate::encoding::LineEnding::Crlf)
                                .on_click(move |_, _, app| {
                                    crlf_entity.update(app, |panel, cx| {
                                        panel.set_eol(crate::encoding::LineEnding::Crlf, cx);
                                    });
                                }),
                        );
                    // ── 缩进组：真改内核 TabSize（空格宽 / 制表符）──
                    menu = menu.separator().item(PopupMenuItem::new("缩进").disabled(true));
                    menu = menu
                        .item({
                            let entity = entity.clone();
                            PopupMenuItem::new("空格: 2")
                                .checked(!hard_tabs && current_tab == 2)
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.set_hard_tabs(false, cx);
                                        panel.set_tab_size(2, cx);
                                    });
                                })
                        })
                        .item({
                            let entity = entity.clone();
                            PopupMenuItem::new("空格: 4")
                                .checked(!hard_tabs && current_tab == 4)
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.set_hard_tabs(false, cx);
                                        panel.set_tab_size(4, cx);
                                    });
                                })
                        })
                        .item({
                            let entity = entity.clone();
                            PopupMenuItem::new("空格: 8")
                                .checked(!hard_tabs && current_tab == 8)
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.set_hard_tabs(false, cx);
                                        panel.set_tab_size(8, cx);
                                    });
                                })
                        })
                        .item({
                            let entity = entity.clone();
                            PopupMenuItem::new("制表符")
                                .checked(hard_tabs)
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.set_hard_tabs(true, cx);
                                    });
                                })
                        });
                    // ── 显示组：视图偏好（双写文档状态 + 内核）──
                    let ws_entity = entity.clone();
                    let inv_entity = entity.clone();
                    menu.separator()
                        .item(PopupMenuItem::new("显示").disabled(true))
                        .item(
                            PopupMenuItem::new("显示空白字符")
                                .checked(show_whitespaces)
                                .on_click(move |_, window, app| {
                                    ws_entity.update(app, |panel, cx| {
                                        panel.set_show_whitespaces(!show_whitespaces, window, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("显示隐藏字符")
                                .checked(show_invisible)
                                .on_click(move |_, _window, app| {
                                    inv_entity.update(app, |panel, cx| {
                                        panel.set_show_invisible(!show_invisible, cx);
                                    });
                                }),
                        )
                })
                .into_any_element()
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

        // 事务区控件（B4）：自动提交开关已移到工具栏（原型 §2.2「事务开关常显」——
        // 它是模式，模式要一直看得见，放工具栏比状态栏更显眼）；状态栏 TX 区只留
        // 提交 / 回滚（事务真开着时出现，原型 §2.5：事务区）
        let tx_controls = tx_available.then(|| {
            let mut row = div().h_flex().items_center().gap_1();
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
            .id("editor-host-panel")
            .role(gpui::Role::Group)
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
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
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
        let result_pane = if self.csv_active {
            // 【遗留 #14】CSV 表格展示：精简结果区（本地筛选 + 复制 + 行数状态）。
            // 不摆 SQL 语义控件（提交 / 分析 / 导出 / 刷新 / 取下一段）——CSV 没有
            // 执行语义，那些按钮点了就是错。
            let row_count = self.grid.read(cx).delegate().data_row_count();
            let toolbar = ResultToolbar {
                rows: Some(row_count),
                affected_rows: None,
                failed: false,
                elapsed_ms: None,
                connection: None,
                lineage: Some("CSV 预览".to_string()),
                filtered: None,
            };
            let status = Some(ResultStatus {
                total_rows: row_count,
                selected_row: None,
                truncated_hint: None,
                has_more: false,
            });
            // 本地筛选复用 SQL 结果区的同一套（`on_filter_input` 走网格 delegate，
            // 对 CSV 数据同样生效：只筛选视图行，不重读文件）
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
                });
            let copy = {
                let entity = cx.entity();
                Button::new("editor-result-copy")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-result-copy".to_string())
                    .label("⧉ 复制")
                    .on_click(move |_, _window, app| {
                        entity.update(app, |panel, cx| panel.copy_csv_table(cx));
                    })
                    .into_any_element()
            };
            // 与 SQL 结果区同一渲染入口（`result_grid::render` 消费 `ResultPane`）：
            // 本分支只准备数据，渲染交给同一个函数，避免两套结果区渲染
            Some(
                result_grid::render(
                    &self.grid,
                    result_grid::ResultPane {
                        toolbar,
                        status,
                        controls: result_grid::ResultControls {
                            filter: Some(filter.into_any_element()),
                            transpose: None,
                            commit: None,
                            analysis: None,
                            copy: Some(copy),
                            export: None,
                            refresh: None,
                            more: None,
                        },
                        card: None,
                        tabs: None,
                        notice: None,
                    },
                    cx,
                )
                .into_any_element(),
            )
        } else {
            self.result_visible().then(|| {
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
                    &self.result_tabs_scroll,
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

            // 【P2-4】转置视图（单行转置，DBeaver 式）：有网格才摆。开关本身是个
            // 图标按钮；**激活时**左边跟上一行 / 下一行导航（看宽表不靠拖横向滚动条）
            let transpose = self.result_can_copy.then(|| {
                let toggle = {
                    let entity = cx.entity();
                    Button::new("editor-result-transpose")
                        .ghost()
                        .small()
                        .toggled(self.transposed)
                        .icon(IconName::Table2)
                        .tooltip("转置视图")
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.toggle_transpose(cx));
                        })
                };
                if !self.transposed {
                    return toggle.into_any_element();
                }
                let prev = {
                    let entity = cx.entity();
                    Button::new("editor-result-transpose-prev")
                        .ghost()
                        .small()
                        .icon(IconName::ChevronUp)
                        .tooltip("上一行")
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.transpose_step(-1, cx));
                        })
                };
                let next = {
                    let entity = cx.entity();
                    Button::new("editor-result-transpose-next")
                        .ghost()
                        .small()
                        .icon(IconName::ChevronDown)
                        .tooltip("下一行")
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.transpose_step(1, cx));
                        })
                };
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

            // 【T2a-7】批量提交/回滚对（参考 DBeaver）：有**待提交**变更才摆（改过的格才能
            // 提交/撤销）；提交中提交按钮置灰（回执回来前不重复提交）。
            let pending_changes = self
                .shared
                .results()
                .change_set(&self.document, self.result_active)
                .map(|changes| changes.pending_count())
                .unwrap_or(0);
            let commit = (pending_changes > 0).then(|| {
                let entity = cx.entity();
                let count = pending_changes;
                let submitting = self.commit_pending();
                let commit_entity = entity.clone();
                let discard_entity = entity.clone();
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    // 测试按选择器断言按钮组的出现/消失（有变更才亮）
                    .debug_selector(|| "editor-result-commit-group".to_string())
                    .child(
                        Button::new("editor-result-commit")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-commit".to_string())
                            .disabled(submitting)
                            .label(format!("提交变更 ({count})"))
                            .on_click(move |_, _window, app| {
                                commit_entity.update(app, |panel, cx| {
                                    if let Err(reason) = panel.submit_pending_changes(cx) {
                                        panel.set_message(Some(reason), cx);
                                    }
                                });
                            }),
                    )
                    .child(
                        Button::new("editor-result-discard")
                            .ghost()
                            .small()
                            .debug_selector(|| "editor-result-discard".to_string())
                            .label("撤销变更")
                            .on_click(move |_, _window, app| {
                                discard_entity.update(app, |panel, cx| {
                                    panel.discard_pending_changes(cx)
                                });
                            }),
                    )
                    .into_any_element()
            });

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
                // 【B7 切片三】`SQL Updates` 的键列查询只在**菜单弹出时**做（事件路径，
                // 端口读导航 L2 缓存）：render 里只把「连接 + 表名」算好（纯内存），
                // 不在这里查端口（render 不做 I/O）
                let conn = self.bound_connection();
                let entry_sql = self
                    .shared
                    .results_active(&self.document)
                    .map(|entry| (entry.sql.clone(), self.result_active + 1));
                let shared = self.shared.clone();
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
                        // 【B7 切片三】附加上下文：`SQL Updates` 要有**键列**才摆
                        // （结果集自己不知道主键，键列由表结构端口给——导航侧 L2 缓存）。
                        // 查不到 / 没接端口 = 不摆，不猜“拿第一列当键”（改错行的代价太高）
                        let key = entry_sql.as_ref().and_then(|(sql, index)| {
                            let table = export::default_table_name(Some(sql), *index);
                            conn.as_deref()
                                .and_then(|conn| shared.table_key_columns(Some(conn), &table))
                                .and_then(|columns| export::RowKey::new(columns))
                        });
                        let extras = export::ExportExtras {
                            sql_text: true,
                            key,
                        };
                        for item in export::menu_items_for(has_more, &rows_text, &extras) {
                            if item.separator_before {
                                menu = menu.separator();
                            }
                            let Some((format, scope)) = item.action else {
                                // 分组标题（“已抓取 N 行” / “抓全量后导出”）
                                menu = menu.item(PopupMenuItem::label(item.label));
                                continue;
                            };
                            let entity = entity.clone();
                            menu = menu.item(PopupMenuItem::new(item.label).on_click(
                                move |_, _window, app| {
                                    entity.update(app, |panel, cx| match scope {
                                        ExportScope::Fetched => {
                                            panel.export_active_result(format, cx)
                                        }
                                        ExportScope::All => {
                                            panel.export_active_result_all(format, cx)
                                        }
                                    });
                                },
                            ));
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
                            entity
                                .update(app, |panel, cx| panel.jump_to_error_site_with(window, cx));
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

            // 【2b】脚本输出卡片：错误卡片没有时才顶上来（脚本结果也是“替掉网格”的那类卡片）
            let card = card.or_else(|| {
                self.script_card.clone().map(|script| {
                    script_card::render(&script, cx).into_any_element()
                })
            });

            result_grid::render(
                &self.grid,
                result_grid::ResultPane {
                    toolbar,
                    status: self.result_status.clone(),
                    controls: result_grid::ResultControls {
                        filter: Some(filter),
                        transpose,
                        commit,
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
            })
        };

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
                    .child(self.render_editor_body(cx)),
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
                breadcrumb: breadcrumb.flatten(),
                edit_mode: Some(edit_mode),
                reset_menu: Some(reset_menu),
                tx: tx_controls,
                interrupt,
            },
            cx,
        ))
    }
}
