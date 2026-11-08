//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use result_panel::panel::ResultPanel;
use result_panel::ports::ResultActionQueue;
use std::cell::{Cell, RefCell};

use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _, DropdownButton};
use gpui_kit::component::dock::{
    BasePanel, DockArea, Panel as ComponentPanel, PanelEvent as BasePanelEvent, PanelId, TabGroup,
};
use gpui_kit::component::date_picker::{DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{EditorState, InputEvent, InputState, RangeDecorationCollection};
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::{Icon, InteractiveElementExt as _, WindowExt as _};
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法
use settings::SettingsService;

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::table::{TableDelegate as _, TableEvent, TableState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::changes::CellChange;
use crate::channel::{self};
use crate::completion::ColumnAnnotation;
use crate::diagnostics;
use crate::execution::{self, ExecTarget};
use crate::export::{self, ExportFormat};
use crate::language_service;
use crate::model::{DocumentId, EditorMode};
use crate::persist;
use crate::service::Document;
use crate::shared::EditorShared;
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::store::{ResultEntry, ResultPlacement};
use crate::ui;
use crate::view::completion;
use crate::view::dialogs;
use crate::view::fold;
use crate::view::highlight;
use crate::view::results::grid::{self as result_grid, ResultGridDelegate};
use crate::view::widgets::status_bar;

// ==== 拆分（2026-11-08）：构造与设置辅助迁入子模块 ====
mod construct;
mod settings_defaults;
pub(crate) use settings_defaults::*;

/// 【B7】「抓全量后导出」的在途状态
///
/// `set` 是发起时的结果集下标：抓取期间用户切了结果集就作废（取段是 `Append` 落位，
/// 会接到**当前选中**那份上，接着写盘就会把两份结果拼在一起）。
pub(crate) struct PendingExport {
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
pub(crate) struct CommitInFlight {
    /// 发起提交的结果集下标（回执处理要 `mutate_change_set` 定位它）
    set_index: usize,
    /// 每条 UPDATE 对应的 `(数据行号, 变更列)`（与执行器逐条影响行数**同序**）
    per_line: Vec<(usize, Vec<usize>)>,
    /// 生成时跳过的行数（键值 NULL 定位不到）——回执文案要如实说出来
    skipped: usize,
    /// 提交时是否存在本地叠加层动作（新增 / 复制 / 删除的本地行）：本期这些行级动作不下发，
    /// 回执文案据此提示「本地行级变更未随本次提交落库」（#28-B1 排除矩阵）。
    local_pending: bool,
}

/// 【遗留 #7】值查看器的目标（右键「查看完整值」弹出；`value_preview` 为 `Some` 时
/// 渲染期就弹 [`dialogs::open_value_preview`]）
#[derive(Clone)]
pub(crate) struct ValuePreview {
    value: String,
    column: String,
    row: usize,
}

/// 【B16】参数绑定挂起（DBeaver 式 `:name`）：执行目标 + 落点 + 标题原样存着，
/// 等参数表单填完 → [`PendingBind::substitute`] 字面替换 → 继续原提交链。
#[derive(Clone)]
pub(crate) struct PendingBind {
    target: ExecTarget,
    placement: ResultPlacement,
    title: Option<String>,
}

impl PendingBind {
    /// 用填好的值替换目标里的 `:name`。只对**用户写的 SQL** 变体
    /// （Selection / Statement / All / Batch）生效；内部生成的目标本就没有参数，原样。
    fn substitute(&self, values: &std::collections::HashMap<String, String>) -> ExecTarget {
        let rebind = |sql: &String| engine::sql::substitute_params(sql, values);
        match &self.target {
            ExecTarget::Selection(sql) => ExecTarget::Selection(rebind(sql)),
            ExecTarget::Statement(sql) => ExecTarget::Statement(rebind(sql)),
            ExecTarget::All(sql) => ExecTarget::All(rebind(sql)),
            ExecTarget::Batch(list) => ExecTarget::Batch(list.iter().map(rebind).collect()),
            other => other.clone(),
        }
    }
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
    /// 状态栏编辑模式入口的标签（`行编辑` / `列编辑`；循环按钮点击即切换，2026-10-27）
    pub(crate) fn short_label(self) -> &'static str {
        match self {
            EditMode::Row => "行编辑",
            EditMode::Column => "列编辑",
        }
    }
}

/// 一份文档的编辑面板
/// 【#17】一次进行中的内核单元执行（同一时间最多一个）
#[derive(Clone)]
pub(crate) struct KernelRun {
    /// 目标内核（`ExecutionSource::Kernel` 指定，否则语言默认 id）
    kernel_id: String,
    /// 目标单元 id（事件回执按它回填输出）
    cell_id: String,
    /// 目标会话 id（切换会话后回执到了不落）
    session_id: String,
}

/// 编辑器文档状态栏快照：全局状态栏「活动文档上下文段」的只读数据源。
///
/// 与编辑器内部旧状态栏同一套数据口径（模式 / 光标 / 连接 / 通道 / 事务…），
/// 由 [`EditorHostPanel::status_snapshot`] 构造；渲染路径零副作用
/// （不触发 I/O、不写 Shared）。
#[derive(Clone, Debug)]
pub struct EditorStatusSnapshot {
    pub mode: EditorMode,
    pub dirty: bool,
    pub read_only: crate::model::ReadOnly,
    /// 语句数（仅 SQL 模式有意义；内容变化时增量计算）
    pub statements: usize,
    /// 光标位置（1 基）
    pub line: usize,
    pub column: usize,
    /// 光标数（列编辑 = 多光标）
    pub cursor_count: usize,
    /// 缩进宽度（内核 TabSize）
    pub tab_size: usize,
    /// 缩进用制表符
    pub hard_tabs: bool,
    /// 保存格式（编码 + 换行）
    pub format: crate::encoding::SaveFormat,
    /// 是否有执行在跑
    pub executing: bool,
    /// 本次执行已跑时长（`None` = 没在跑）
    pub elapsed: Option<std::time::Duration>,
    /// 连接段文案（`●P·orders`）；`None` = 无连接概念（文本模式）
    pub connection: Option<String>,
    /// 方言（`PostgreSQL`）；`None` = 未绑定连接
    pub dialect: Option<String>,
    /// 通道段文案；`None` = 不显示
    pub channel: Option<String>,
    /// 当前 Schema（`None` = 用连接默认）
    pub schema: Option<String>,
    /// 通用模式环境段；`None` = 非通用 / 未选
    pub env: Option<String>,
    /// 通用模式脚本文件类型段；`None` = 非脚本
    pub file_type: Option<String>,
    /// 通用模式语言服务状态段；`None` = 不需要语言服务
    pub lsp: Option<String>,
    /// 事务区文案；`None` = 无事务语义
    pub tx: Option<String>,
    /// 【§2.5 分析】会话段（`会话 orders-主库` / `会话 默认会话`）；`None` = 非分析模式
    pub session: Option<String>,
    /// 【§2.5 分析】内核段（`内核 python`）；`None` = 未启动内核
    pub kernel: Option<String>,
    /// 【§2.5 分析】单元计数段（`3 单元` / `3 单元（1 stale）`）；`None` = 非分析模式
    pub units: Option<String>,
    /// 动作失败提示；`None` = 无提示
    pub message: Option<String>,
}

/// 编辑器面板状态变化事件：宿主订阅后刷新全局状态栏的活动文档上下文段。
///
/// Dock 的 `PanelEvent` 只有 Zoom / Layout 三类，没有「激活」事件；
/// 宿主渲染时从 Dock 布局树读中央活动面板（唯一真相源），本事件只负责
/// 「面板状态变了 → 宿主重绘 → 全局段刷新」。
#[derive(Clone, Debug)]
pub enum EditorHostEvent {
    /// 状态栏可见的文档状态变了（内容 / 连接 / 通道 / Schema / 事务 / 执行 / 消息）
    StatusChanged,
    /// 执行失败（原因文本）——宿主据此弹全局错误通知。
    ExecutionFailed(String),
}

impl EventEmitter<EditorHostEvent> for EditorHostPanel {}

pub struct EditorHostPanel {
    pub(crate) shared: EditorShared,
    pub(crate) document: DocumentId,
    pub(crate) focus_handle: FocusHandle,
    /// 编辑内核状态（构造需要 `window`，故在 `new` 里一次建好）
    pub(crate) editor: Entity<EditorState>,
    /// 语句数缓存（A7）：内容变化时算一次，**不在渲染期扫描**（render 是纯读路径）
    pub(crate) statements: usize,
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
    pub(crate) edit_mode: EditMode,
    /// 是否已被 Dock 移除（移除即关闭文档，见 `Panel::on_removed`）
    pub(crate) closed: bool,
    /// #48 行号开关缓存（设置 → render 权威同步点 → 内核 `set_line_number`）
    pub(crate) show_line_numbers: bool,
    /// 书签悬停说明开关缓存（设置 → render 权威同步点 → 重新投影 gutter markers）
    pub(crate) show_bookmark_hover_note: bool,
    /// 常显书签列开关缓存（设置 → render 权威同步点 → 内核 `set_gutter_marker_column`）
    pub(crate) show_bookmark_column: bool,
    /// #64 自动换行开关缓存（设置 → render 权威同步点 → 内核 `set_soft_wrap`）
    pub(crate) soft_wrap: bool,
    /// #65 自动闭合括号开关缓存（设置 → render 权威同步点 → 内核 `set_auto_close`）
    pub(crate) auto_close: bool,
    /// #65 智能缩进开关缓存（设置 → render 权威同步点 → 内核 `set_smart_indent`）
    pub(crate) smart_indent: bool,
    /// 【2026-10-27】用户只读开关（工具栏 roLock；点击在 只读/编辑 间翻转）。
    ///
    /// 只读是**窗口级合成**（[`Self::read_only_flags`]），不写回 Document——与项目锁同一
    /// 口径（解锁了但文档还写着只读会留下陈旧值）；内核 readonly 在切换时一并同步。
    pub(crate) user_ro_lock: bool,
    /// 【2026-10-19】脏态自绘 ✕ 的悬浮开关：鼠标悬停标题才显示关闭按钮，
    /// 避免常驻 ✕ 与标题内容互相遮挡（见 `title_suffix`）。
    pub(crate) title_hovered: std::cell::Cell<bool>,
    /// 结果网格（A14）：数据由 `ResultStore` 拷入 delegate，网格不持有第二份真值
    ///
    /// Dock 化后网格实体由面板 [`ResultPanel::set_grids`] 注入同一实体——编辑器动作
    /// （取段 / 转置 / 筛选 / 复制）与面板渲染读写同一份 `TableState`，靠 `Entity<T>`
    /// 共享句柄解耦，不复制第二份状态。
    pub(crate) grid: Entity<TableState<ResultGridDelegate>>,
    /// 本文档**已提交、尚未回填**的语句数（B2：批量 = 多条；`> 0` 就是“执行中”）
    pub(crate) pending: usize,
    /// 本文档当前这轮执行是什么时候开始的（B3：状态栏耗时累加；`pending == 0` 时清掉）
    pub(crate) running_since: Option<std::time::Instant>,
    /// 【B4】自动提交：开 = 每次执行自成一体（默认）；关 = 执行时若没有事务就先开一个，
    /// 由用户显式提交或回滚
    pub(crate) autocommit: bool,
    /// 【B4】本文档绑定的连接上有没有活动事务（界面上的 TX 区读它）
    pub(crate) tx_open: bool,
    /// 【B4】这个事务是什么时候开始的（**本地观测时刻**：事务可能是在别处开的，
    /// 界面的时长从“我们第一次看到它”算起）
    pub(crate) tx_since: Option<std::time::Instant>,
    /// 【B4】已发出、尚未回执的事务动作数（轮询泵据此多活一会儿）
    pub(crate) tx_pending: usize,
    /// 【B13】已请出、尚未回执的“重新挂载加速源”数（同上）
    pub(crate) refresh_pending: usize,
    /// 【B7 切片二】已请出、尚未回执的 DuckDB 导出（Parquet / XLSX）数（同上）
    pub(crate) export_pending: usize,
    /// 【B10】在途结果集的**自定义标题**队列（按提交顺序回填；`None` = 就是“结果 N”）
    ///
    /// 为什么要队列：执行是**一条语句一个结论**地回到界面的（批量更是好几条），
    /// “这一份是执行计划”这种信息必须**按位**对上，不能拿一个布尔量糊过去。
    pub(crate) pending_labels: std::collections::VecDeque<Option<String>>,
    /// 结果面板实体（Dock 化：面板归 workbench dock 渲染；编辑器只做两件事——
    /// 事件路径算 [`ResultSyncInput`] 投影 + `drain_result_actions` 分派面板动作）
    pub(crate) result_panel: Entity<ResultPanel>,
    /// 【结果区分栏】拖拽高度（rem；面板存活期间记住，结果区关闭重开仍沿用；
    /// 跨会话持久化经 `session_snapshot` → `editor_contexts.result_height` 列，
    /// B5 余项已收口——快照与恢复两侧见 `view/host/session.rs`）
    pub(crate) result_height: std::rc::Rc<Cell<f32>>,
    /// 面板动作队列（面板渲染期 `push`；编辑器事件路径这里 `drain` 分派回动作方法）
    pub(crate) result_action_queue: ResultActionQueue,
    /// 【并排对比】对比栏的网格（第二份结果集的 delegate；惰性创建，面板生命周期内复用）
    pub(crate) compare_grid: Option<Entity<TableState<ResultGridDelegate>>>,
    /// 【B16】参数绑定挂起：执行目标里含 `:name` 时记在这里，等参数表单填完
    /// 再替换执行（`None` = 没有在等表单）
    pub(crate) pending_bind: Option<PendingBind>,
    /// 【B6】当前选中结果集定位到的出错处（诊断范围 + 状态栏里的位置文案读它）
    pub(crate) error_site: Option<diagnostics::ErrorSite>,
    /// 【2b】脚本执行输出卡片（通用模式回执写入；sync 时经 `ResultSyncInput` 透传面板——
    /// 面板的 sync 全量覆写该字段，真值必须留在编辑器这一侧）
    pub(crate) script_card: Option<crate::view::results::script_card::ScriptCard>,
    /// 【B6】上一次算过的位置是哪条结论（`(这次跑的 SQL, 错误文本)`）
    ///
    /// 位置要拿整篇文档做匹配，而 `sync_result_view` 会被反复调用（回填 / 切标签），
    /// 所以只在结论变了的时候重算。
    pub(crate) error_site_key: Option<(String, String)>,
    /// 面板所在窗口（构造时存下：出错回填发生在没有窗口的轮询里，聚焦要用）
    pub(crate) window: AnyWindowHandle,
    /// 【遗留 #7】值查看器：右键「查看完整值」弹出的目标（`Some` = 弹层开着）
    pub(crate) value_preview: Option<ValuePreview>,
    /// 【遗留 #14】CSV 表格展示：文档是 CSV 且网格已填（渲染据此走精简结果区；
    /// 数据在 `grid` 里，不经过结果 store——CSV 没有 SQL 执行语义）
    pub(crate) csv_active: bool,
    /// 【B7】「抓全量后导出」的在途状态（`None` = 没有导出在跑）
    ///
    /// 抓全量是**多轮取段**：每段回来推进一次，最后一段（没拿满）才落盘。中途换结果集、
    /// 取段失败、用户中断都作废——**不落一个半截的文件**（宁可没有，也不要一份看起来
    /// 完整、实则少一半的导出）。
    pub(crate) pending_export: Option<PendingExport>,
    /// 【B5b】正在取下一段（滚动到底自动加载的防重入真值；同步给网格 delegate）
    pub(crate) fetching_more: bool,
    /// 【B14】结果网格里**最近一次选中**（`Ctrl+C` 复制谁）
    ///
    /// 组件没给“当前是选格还是选行”的访问器，而它的 `selected_cell()` / `selected_row()`
    /// 可以同时有值（选过整行再点一个格子）——所以在这里记**事件**，不猜状态。
    pub(crate) result_selection: Option<result_grid::GridSelection>,
    /// 【B18】后台扫描器（语句数 / 折叠候选 / 打字期诊断）
    ///
    /// 为什么要有它：这三件事原先全在 `InputEvent::Change` 上**同步**做——性能基线
    /// （`tests/offline_baseline.rs`）实测 5 千行脚本每敲一下要按住主线程 90–130 ms，
    /// 贴 1 MB 约 0.6–1.0 s。挪到工作线程后，按键只做一次提交（带修订号）。
    pub(crate) scanner: crate::scan::Scanner,
    /// 【B18】当前修订号：回执只接受与它相等的那一份（陈旧丢弃）
    pub(crate) scan_revision: u64,
    /// 【B18】有扫描在途（结果轮询泵据此多活一会儿）
    pub(crate) scan_pending: bool,
    /// 【B18】最近一次扫描给的打字期诊断（与执行错误回填**一起**画，见 `apply_error_marks`）
    pub(crate) scan_diagnostics: Vec<language_service::Diagnostic>,
    /// 【B6】执行错误的原因（`None` = 这次成功）；与词法诊断合并重建时要连它一起重画
    pub(crate) error_reason: Option<String>,
    /// 结果轮询任务句柄（空闲即退出；句柄存活到下一次提交）
    pub(crate) exec_pump: RefCell<Option<Task<()>>>,
    /// 【#17】正在跑的内核单元（同一时间最多一个，与 `has_running` 防并发一致）
    pub(crate) kernel_running: Option<KernelRun>,
    /// 【#17】内核事件轮询任务句柄（空闲即退出）
    pub(crate) kernel_pump: RefCell<Option<Task<()>>>,
    /// 【2d-笔记级】「全部运行」队列（待跑单元 id；空 = 没排队）。
    ///
    /// 执行队列一次只跑一个（`ExecQueue` busy 检查），所以全部运行是**串行队列**：
    /// 入队全部可执行单元，跑完一个（SQL 回执 / 内核收口）自动提交下一个。
    pub(crate) notebook_pending: Vec<String>,
    /// 内容回写订阅：句柄即生命周期（释放即取消）
    pub(crate) _editor_sub: Option<Subscription>,
    /// 网格事件订阅（行选中 → 状态行的“已选第 N 行”要跟着变）
    pub(crate) _grid_sub: Option<Subscription>,
    /// 结果面板观察订阅（Dock 化 §8.5：面板按钮 push 队列 → 面板 notify → 本宿主
    /// drain 分派回动作方法；句柄即生命周期）
    pub(crate) _result_panel_sub: Option<Subscription>,
    /// 【T2a-4】编辑框订阅（输入变化 → 同步编辑态；Enter / 失焦 → 提交）。
    /// 句柄即生命周期：不存字段就会被 drop、订阅即刻取消（编辑器内踩过一次）。
    pub(crate) _edit_sub: Option<Subscription>,
    /// 【T2a-6】提交中的一次批量提交（`None` = 没有提交在跑；回执回来前按钮保持灰）
    pub(crate) commit_in_flight: Option<CommitInFlight>,
    /// 【T2c-5】工具栏分组自定义布局（权威状态；render 纯读，改动走面板方法）
    pub(crate) toolbar: crate::toolbar::ToolbarLayout,
    /// 【2c】隐藏字符标注集合（`show_invisible` 开启时懒创建；`None` = 未启用/未建）
    ///
    /// 集合句柄借的是内核实体：面板 drop 时随内核一起释放，无需手动 `dispose`。
    pub(crate) invisible_decorations: Option<RangeDecorationCollection>,
    /// 【书签】书签行标记集合（懒创建；`None` = 无书签 / 未建）。借内核实体，随面板释放。
    pub(crate) bookmark_decorations: Option<RangeDecorationCollection>,
    /// 【LSP】本文档的语言服务会话（`None` = 宿主办无可用的服务；didOpen 已发）
    pub(crate) lsp: Option<crate::lsp::LspSession>,
    /// 【LSP】本文档的 LSP 文档版本（didOpen = 1，此后每次 didChange 自增）
    pub(crate) lsp_version: i64,
    /// 【LSP】语言服务是否健康（收到 IoError / EOF 后置 false → 摘 provider、停轮询）
    pub(crate) lsp_alive: bool,
    /// 【LSP】语言服务最近一次推送的诊断（与执行错误 / 词法诊断三源合并重画）
    pub(crate) lsp_diagnostics: Vec<lsp_types::Diagnostic>,
    /// 【遗留 #10】分析笔记本：单元源的内核编辑器实例（键 = 稳定 `cell_id`，不用下标；
    /// 渲染据此用 [`Editor`] 画单元正文——原型 §4.2「源编辑区 = 内核编辑器」）
    /// `pub(crate)`：analysis.rs（同 crate）渲染期只读取它
    pub(crate) cell_editors: std::collections::HashMap<String, Entity<EditorState>>,
    /// 【遗留 #10】单元编辑器的 Change 订阅（句柄即生命周期；移除单元时一并丢弃）
    pub(crate) _cell_editor_subs: std::collections::HashMap<String, Subscription>,
    /// 【遗留】通用模式 Markdown 文档的渲染态：true = 显示渲染视图（TextView::markdown），
    /// false = 源码编辑器（默认）。仅对 `ScriptFileType::Markdown` 生效；非 markdown 文档忽略。
    pub(crate) markdown_preview: bool,
}

pub mod bookmarks;
pub mod editor_body;
pub mod env;
pub mod exec;
pub mod format;
pub mod notebook;
pub mod results;
pub mod session;
pub mod toolbar;

impl EditorHostPanel {
    /// #48 设置默认值落库：**只有**仍保持出厂默认格式（缩进 2 空格 / 空格缩进 /
    /// UTF-8+LF）的文档应用设置默认——用户改过或会话恢复过的文档不动。
    ///
    /// 三类 setter 都只在值变化时落库且**不标脏**：新建文档不会因此出现未保存提示。
    /// 缩进走 `set_tab_size` / `set_hard_tabs`（文档 + 内核 TabSize 双写，同「重设 ▾」）；
    /// 编码 / 换行走 `set_document_save_format`（设置文本解析失败就跳过该项，不猜）。
    fn apply_editor_defaults(&mut self, cx: &mut Context<Self>) {
        let defaults = editor_settings_or_default(cx);
        let pristine = self
            .with_document(|doc| {
                doc.tab_size() == 2
                    && !doc.hard_tabs()
                    && doc.save_format() == crate::encoding::SaveFormat::default()
            })
            .unwrap_or(true);
        if !pristine {
            return;
        }
        // 宽度下限 1（设置页校验过 1–16；手改坏的设置文件不猜）
        self.set_tab_size(defaults.default_tab_size.max(1) as usize, cx);
        self.set_hard_tabs(defaults.default_hard_tabs, cx);
        if let (Some(encoding), Some(eol)) = (
            crate::encoding::TextEncoding::from_setting(&defaults.default_encoding),
            crate::encoding::LineEnding::from_setting(&defaults.default_line_ending),
        ) {
            self.shared.set_document_save_format(
                &self.document,
                crate::encoding::SaveFormat { encoding, eol },
            );
        }
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
        // 消息是状态栏上下文段的一部分（连接 / 通道 / 事务 / 执行开始都经它留痕）
        cx.emit(EditorHostEvent::StatusChanged);
    }

    /// 清掉状态栏提示（动作成功 / 用户取消时调：旧提示不该继续挂着）
    pub fn clear_message(&mut self, cx: &mut Context<Self>) {
        self.set_message(None, cx);
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

    /// 【A4】装上内核的那个着色器声明的 token 名（供测试断言「装的是 SQL 着色器」）
    ///
    /// 名字必须落在活跃主题的词汇里，否则内核会静默跳过该 token——所以这份图例就是「染不染色」
    /// 的对外契约（见 `crate::view::highlight::TOKEN_NAMES`）。
    pub fn highlight_legend_for_test(&self, cx: &App) -> Vec<String> {
        self.editor
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
}

// ==== Step 5 拆分（2026-10-29）：Render / 面板操作 / trait 实现迁入子模块 ====
pub mod ops;
pub mod render;
// 跨 crate 公共 API（workbench 经 editor::view::host::* 调用）：显式 re-export
pub(crate) use ops::*;
pub use ops::{
    close_document_in_dock, close_document_now, request_close_document, request_save_as,
    resolve_close_choice, save_sessions_for,
};
pub(crate) use render::*;
pub(crate) mod panel_traits;
