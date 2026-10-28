//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use std::cell::{Cell, RefCell};
use result_panel::panel::ResultPanel;
use result_panel::ports::ResultActionQueue;

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _, DropdownButton};
use gpui_kit::component::resizable::{resizable_panel, v_resizable};
use gpui_kit::component::{Icon, InteractiveElementExt as _, WindowExt as _};
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::component::dock::{
    BasePanel, DockArea, Panel as ComponentPanel, PanelEvent as BasePanelEvent, PanelId, TabGroup,
};
use gpui_kit::component::input::{
    EditorState, InputEvent, InputState, RangeDecorationCollection,
};
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法
use settings::SettingsService;

/// #48 读编辑器设置快照（面板构造 / render 调用）：测试环境未注册 Settings global
/// 时回退出厂默认（生产在 app 启动经 `SettingsService::init` 注册，不走此分支）。
fn editor_settings_or_default(cx: &App) -> settings::model::Editor {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::editor_defaults(cx)
    } else {
        settings::model::Editor::default()
    }
}

/// #48 读行号开关（同上：测试环境回退默认 true）。
fn editor_show_line_numbers_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::show_line_numbers(cx)
    } else {
        true
    }
}

/// 【2026-10-27】读折叠开关（设置在设置页；测试环境回退默认 true）。
fn editor_show_folds_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::show_folds(cx)
    } else {
        true
    }
}

/// 【2026-10-28】读「基于缩进的折叠」开关（测试环境回退默认 true）。
fn editor_indent_folding_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::indent_folding(cx)
    } else {
        true
    }
}

/// 【2026-10-28】读「自定义开块关键字」（逗号分隔字符串解析为 Vec<String>；测试环境用默认）。
fn editor_fold_open_keywords_or_default(cx: &App) -> Vec<String> {
    if cx.has_global::<settings::model::Settings>() {
        let s = SettingsService::fold_open_keywords(cx);
        s.split(',').map(|kw| kw.trim().to_uppercase()).filter(|kw| !kw.is_empty()).collect()
    } else {
        vec!["CASE".to_string(), "BEGIN".to_string(), "IF".to_string(), "LOOP".to_string(), "WHILE".to_string(), "FOR".to_string(), "REPEAT".to_string()]
    }
}

/// 【2026-10-28】读「自定义闭块关键字」（逗号分隔字符串解析为 Vec<String>；测试环境用默认）。
fn editor_fold_close_keywords_or_default(cx: &App) -> Vec<String> {
    if cx.has_global::<settings::model::Settings>() {
        let s = SettingsService::fold_close_keywords(cx);
        s.split(',').map(|kw| kw.trim().to_uppercase()).filter(|kw| !kw.is_empty()).collect()
    } else {
        vec!["END".to_string()]
    }
}

/// 【B3】读「自动换行」开关（测试环境回退默认 true，与内核一致）。
fn editor_soft_wrap_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::soft_wrap(cx)
    } else {
        true
    }
}

/// 【B4】读「自动闭合括号」开关（测试环境回退默认 true，与内核一致）。
fn editor_auto_close_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::auto_close(cx)
    } else {
        true
    }
}

/// 【B4】读「智能缩进」开关（测试环境回退默认 true，与内核一致）。
fn editor_smart_indent_or_default(cx: &App) -> bool {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::smart_indent(cx)
    } else {
        true
    }
}

/// 【B4】高亮类型 → 编辑器语言键（决定自动闭合 / 智能缩进的规则来源）
///
/// - Sql / 未识别扩展名 → `"sql"`（走 `language_config::install` 注册的 SQL 规则）；
/// - 脚本类型 → gpui-component 默认键（`ComponentLanguages` 对 python 有缩进规则，
///   json 有专属配对，其余落 fallback 默认 `()[]{}`+引号）；
/// - None（不上色：CSV / 分析模式）→ 不设语言（保持空键，内核默认）。
fn editor_language_key(kind: crate::view::highlight::HighlightKind) -> Option<&'static str> {
    use crate::mode::ScriptFileType;
    use crate::view::highlight::HighlightKind;
    match kind {
        HighlightKind::Sql => Some("sql"),
        HighlightKind::Script(ScriptFileType::Python) => Some("python"),
        HighlightKind::Script(ScriptFileType::R) => Some("r"),
        HighlightKind::Script(ScriptFileType::Rust) => Some("rust"),
        HighlightKind::Script(ScriptFileType::Markdown) => Some("markdown"),
        HighlightKind::Script(ScriptFileType::Json) => Some("json"),
        // CSV 走表格展示、Log 无专属编辑规则：留空键 = 内核默认
        HighlightKind::Script(ScriptFileType::Csv | ScriptFileType::Log) => None,
        HighlightKind::None => None,
    }
}

/// 【B3】读事实档载体偏好快照（分析 / 带类型导出在事件路径上取一次，随请求带入工作线程：
/// `QueryRunner` 是 `Send + Sync + 'static` 且方法签名无 `cx`，工作线程内不能自己读 global）。
/// 测试环境未注册 Settings global 时回退 `Auto`（生产在 app 启动经 `SettingsService::init` 注册）。
fn fact_storage_pref_or_default(cx: &App) -> ::shared::StoragePref {
    if cx.has_global::<settings::model::Settings>() {
        SettingsService::get(cx).result_panel.fact_storage.to_shared()
    } else {
        ::shared::StoragePref::Auto
    }
}

/// 【B3】取当前选中结果集的事实档 `Arc` 快照（无事实档 = `None`，执行器走显示矩阵降级档）。
/// 克隆 `Arc` 即出 `Ref` 借用边界，不在 `results()` 引用存活期间回写 store。
fn active_fact_snapshot(
    shared: &EditorShared,
    document: &crate::model::DocumentId,
) -> Option<std::sync::Arc<::shared::FactFrame>> {
    shared
        .results()
        .active_fact(document)
        .and_then(|state| state.fact().map(std::sync::Arc::clone))
}

/// 【B4】当前选中结果集的槽位下标（无结果集回退 0）：分析 / 导出物化回执据此回挂原槽位。
fn active_set_index(shared: &EditorShared, document: &crate::model::DocumentId) -> usize {
    shared.results().active_index(document).unwrap_or(0)
}
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::table::{TableDelegate as _, TableEvent, TableState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::changes::CellChange;
use crate::channel::{self};
use crate::diagnostics;
use crate::execution::{self, ExecTarget};
use crate::export::{self, ExportFormat};
use crate::language_service;
use crate::model::{DocumentId, EditorMode};
use crate::persist;
use crate::service::Document;
use crate::shared::EditorShared;
use crate::completion::ColumnAnnotation;
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::store::{ResultEntry, ResultPlacement};
use crate::ui;
use crate::view::completion;
use crate::view::dialogs;
use crate::view::fold;
use crate::view::highlight;
use crate::view::results::grid::{
    self as result_grid, ResultGridDelegate,
};
use crate::view::widgets::status_bar;

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
    /// 跨会话持久化属 workbench context 层，字段已存在见遗留 #5）
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


// ==== Step 3 拆分（2026-10-29）：impl 方法块按能力域迁入子模块 ====
pub mod session;
pub mod toolbar;
pub mod env;
pub mod format;
pub mod bookmarks;
pub mod notebook;
pub mod editor_body;
pub mod exec;
pub mod results;

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
        // 【B4】语言编辑规则全局注册（幂等：重复注册同键覆盖）。面板首次创建时
        // 完成一次即可——`set_language_config` 是叠加注册，多面板重复调用无害。
        crate::lsp::language_config::install(cx);

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
            // 【B4】语言编辑规则：SQL 走注册表（`set_language_config("sql", …)`），
            // 脚本类型走 gpui-component 默认（text / json / python 已有 config）。
            // language 键决定自动闭合 / 智能缩进的规则来源，与高亮类型一致。
            if let Some(language) = editor_language_key(highlight_kind) {
                state = state.language(language);
            }
            state.set_value(content, window, cx);
            state.set_readonly(editor_read_only, cx);
            // 去掉编辑器默认左边 padding（让文字顶到最左边）
            state.set_editor_paddings(gpui::Edges {
                top: Pixels::ZERO,
                right: Pixels::from(8.),
                bottom: Pixels::ZERO,
                left: Pixels::ZERO,
            });
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
                    // 【#54-C4 二期】输入 `(` 自动弹签名帮助：光标前一字符是 `(` 且
                    // `auto_call` 识别出函数调用才弹（打字中光标紧随参数/逗号天然不触发）；
                    // 已有模态 Dialog 时不抢（保险：签名帮助打开期间编辑器已失焦）
                    if !window.has_active_dialog(cx) {
                        let cursor = state
                            .read(cx)
                            .cursor_offsets()
                            .first()
                            .copied()
                            .unwrap_or(0);
                        if let Some(view) = crate::view::signature_help::auto_call(&text, cursor) {
                            crate::view::signature_help::open_signature_help(window, cx, view);
                        }
                    }
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
                    // 【方案 A】书签偏移校正：取内核本次编辑的行增量，平移 shared 书签并回写
                    this.adjust_bookmarks_for_edit(state, cx);
                    // 【2c】显示隐藏字符开启时：正文变了，标注区间要跟着重算（区间是字节偏移，
                    // 文字一动就全失效；没开显示则集合为空 / 未建，直接跳过）
                    if this.shared.document_show_invisible(&this.document) {
                        let ranges = this.invisible_ranges_for(cx);
                        this.refresh_invisible_decorations(ranges, cx);
                    }
                    // 重绘以刷新标签脏点与状态栏
                    cx.notify();
                    // 状态栏内容变了（行 / 列 / 脏点 / 语句数）：通知宿主刷新全局状态栏段
                    cx.emit(EditorHostEvent::StatusChanged);
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

        // 【B15】本地筛选框已随 Dock 化迁入面板实体（`ResultPanel::new` 自建含防抖订阅）；
        // 编辑器只保留 右键「按值筛选」→ 面板 `apply_filter_value` 的转发（见钩子注入区）。

        // 结果网格：构造时一次建成，以后结果变化只 `refresh`（不重建，避免丢掉列宽 / 滚动位置）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行——按 Ctrl+Enter 执行当前语句"),
            window,
            cx,
        );
        // 【并排对比】对比栏网格：与主网格同处一次建成（复用一个实体，结果变化只 refresh）。
        // 只在对比模式渲染；数据经 `sync_compare_grid` 从 ResultStore 拷入。
        let compare_grid = result_grid::new_table_state(
            ResultGridDelegate::empty("未选择对比结果"),
            window,
            cx,
        );

        // Dock 化接线：面板实体 + 动作队列（渲染期 push / 事件路径 drain 分派）。
        // 网格经 `set_grids` 注入**同一实体**——编辑器动作与面板渲染读写同一份 TableState。
        let result_action_queue = ResultActionQueue::new();
        let result_panel = cx.new(|cx| {
            let mut panel = ResultPanel::new(window, cx, result_action_queue.clone());
            panel.set_grids(grid.clone(), Some(compare_grid.clone()));
            // 【B2】全局偏好 `result_panel.show_column_comments` 启动时同步到面板（面板级临时开关
            // 覆盖此默认值；用户在面板工具栏切换后不再回写全局——全局只控"默认开不开"）
            if let Some(settings) = cx.try_global::<settings::model::Settings>() {
                // 先拷贝偏好值、释放 `settings` 借用，再对 panel 可变借用 `cx`
                //（try_global 的借用与 `cx` 可变借用冲突 → E0502，2026-10-28 修复）。
                let on = settings.result_panel.show_column_comments;
                let type_style = settings.result_panel.type_style();
                if on {
                    panel.set_show_column_comments(true, cx);
                }
                // 【类型样式】全局偏好启动同步（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）；
                // 设置页后续变更走 `apply_result_style`（workbench 钩子）即时生效
                panel.set_type_style(type_style, cx);
            }
            panel
        });
        // 【#8-按钮断链修复】队列通知目标 = 结果面板实体（弱引用）。此后按钮 `request`
        // 入队即通知面板 → 下方 observe_in 回调 drain，点击不再依赖"碰巧的 notify"。
        result_action_queue.set_panel(result_panel.downgrade());
        // Dock 化 §8.5（用户已确认「editor 宿主订阅自 drain」）：面板按钮 push 队列 →
        // 面板 notify → 本宿主观察回调 drain 分派回动作方法。gpui 的通知延迟到事务
        // 边界派发（app.rs notify → pending_effects），回调执行时面板已不在 update 中，
        // 不会「cannot update while already being updated」。
        let result_panel_sub = cx.observe_in(&result_panel, window, |this, _panel, _window, cx| {
            this.drain_result_actions(cx);
        });
        // 行选中 → 状态行那一段（⑦ 的“已选第 N 行”）要重画（表格自己不通知宿主）
        let grid_sub = cx.subscribe_in(&grid, window, |panel, grid, event: &TableEvent, window, cx| {
            let selected_row = grid.read(cx).selected_row().map(|row| row + 1);
            // 状态行「当前格值」段走面板增量更新（DBeaver 对齐）：先取一次面板实体，
            // 各分支复用（`result_panel()` 返回 clone，不借用 panel，可随后写 result_selection）
            let result_panel = panel.result_panel();
            // 【B14】`Ctrl+C` 复制谁：只认**最近一次选择事件**（组件没公开“当前是选格还是选行”，
            // 而它两个字段可以同时有值）。键位失效之后再选也就自动作废了，不用额外清理。
            panel.result_selection = match event {
                TableEvent::SelectCell(row, col) => {
                    // 编辑器在事件里读网格 delegate 算好值传给面板（面板不碰网格；
                    // 长值截断防状态行被撑爆）
                    let text = grid.read(cx).delegate().cell_text(*row, *col, cx);
                    result_panel.update(cx, |panel, cx| {
                        panel.set_selected_cell(Some(Self::clip_cell_preview(&text)), cx);
                        // 【侧栏】右侧辅助栏（值详情 / 列浏览器）要位置：选中格的原生事件就带着
                        // window 同步列浏览器 List 内部选中高亮（网格反推路径）
                        panel.set_selection_pos(Some((*row, *col)), window, cx);
                    });
                    Some(result_grid::GridSelection::Cell { row: *row, col: *col })
                }
                TableEvent::SelectRow(row) => {
                    result_panel.update(cx, |panel, cx| {
                        panel.set_selected_cell(None, cx);
                        // 【侧栏】选整行不投影单格值详情
                        panel.set_selection_pos(None, window, cx);
                    });
                    Some(result_grid::GridSelection::Row { row: *row })
                }
                TableEvent::ClearSelection => {
                    result_panel.update(cx, |panel, cx| {
                        panel.set_selected_cell(None, cx);
                        panel.set_selection_pos(None, window, cx);
                    });
                    None
                }
                TableEvent::ColumnWidthsChanged(widths) => {
                    // 【P1-5】拖宽把列宽记进 delegate：排序 / 改行数重建表头时不被打回默认档。
                    // 组件发完事件已自刷新，这里不再 notify；只记宽度，不改动选中状态。
                    grid.update(cx, |state, _cx| {
                        state.delegate_mut().set_column_widths(widths)
                    });
                    panel.result_selection
                }
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
                        // 编辑被拦提示迁入面板（增量写入；sync 全量覆写前保持显示）
                        panel
                            .result_panel
                            .update(cx, |p, cx| p.set_notice(Some(format!("编辑被拦：{}", reason.reason())), cx));
                    }
                }
            }
            // 行选中 → 状态行「已选第 N 行」增量更新（迁入面板实体：`set_selected_row`
            // 只补 status.selected_row，sync 全量覆写前的中间态保持一致）
            panel
                .result_panel
                .update(cx, |p, cx| p.set_selected_row(selected_row, cx));
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
            |panel, state, event, window, cx| match event {
                InputEvent::Change => {
                    let value = state.read(cx).value().to_string();
                    panel.grid.update(cx, |table, _cx| {
                        table.delegate_mut().sync_edit_value(value);
                    });
                }
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    // 【类型感知】`commit_edit` 返回是否真的交了：类型校验失败 = 编辑态
                    // 保留 + 红框提示，此时**不能**跑整轮投影（`sync_result_view` 重建网格
                    // 会把编辑态冲掉）——只请求重绘让红框 / 错误提示上屏。
                    let committed = panel.grid.update(cx, |table, _cx| {
                        table.delegate_mut().commit_edit()
                    });
                    if committed {
                        // 【T2a-4】提交完成后投影刷新（脏格 + 待提交计数 + 网格数据一起走
                        // `sync_result_view`；grid 已退出上面的 update，这里可安全 update 面板）。
                        // Dock 化后提交按钮组 / 脏格底色读面板投影，不刷新就不会出现。
                        panel.sync_result_view(cx);
                    } else {
                        window.refresh();
                    }
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
                    move |row, column, original, edited| {
                        // 【T2a-4】提交落权威变更集：只动 Shared（独立实体，可安全 update）。
                        // 钩子是在网格 delegate 内部（`commit_edit`）被调用的——这里再
                        // update 网格就是重入 panic；脏格重投影由订阅闭包（PressEnter /
                        // Blur 分支）在 commit 完成后做，那里 grid 已退出 update。
                        shared_for_commit.update_results(|store| {
                            let active = store.active_index(&document_for_commit).unwrap_or(0);
                            let changed = edited != original;
                            store.mutate_change_set(&document_for_commit, active, |changes| {
                                if !changed {
                                    // 值没变（改回来）：清掉这条变更
                                    changes.remove(row, column);
                                } else {
                                    changes.upsert(CellChange::pending(
                                        row, column, original.clone(), edited.clone(),
                                    ));
                                }
                            });
                            // 【撤销】值真的变了才记一步历史（改回原值不是新编辑，不入栈；
                            // 新编辑入栈时 EditHistory 自动清空重做栈——标准撤销语义）
                            if changed {
                                store.mutate_history(&document_for_commit, active, |history| {
                                    history.push(CellChange::pending(
                                        row, column, original, edited,
                                    ));
                                });
                            }
                        });
                    },
                ));
            });
            // 【#28-B4】本地行格编辑提交：只写回本地行快照，不进变更集、不进格级 /
            // 行级撤销栈（本地行格编辑提交即落快照，拍板口径；重投影复用 Enter / Blur 后
            // 的 `sync_result_view`，与底层格编辑同一条刷新路径）。
            let shared_for_local = shared.clone();
            let document_for_local = document.clone();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_commit_local_edit_hook(std::rc::Rc::new(
                    move |local_id, column, edited| {
                        shared_for_local.update_results(|store| {
                            let active = store
                                .active_index(&document_for_local)
                                .unwrap_or(0);
                            store.mutate_local_rows(&document_for_local, active, |set| {
                                if let Some(row) = set.local_row_mut(local_id) {
                                    if let Some(cell) = row.cells.get_mut(column) {
                                        *cell = edited;
                                    }
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
            // 【筛选】列头筛选钩子与「按值筛选」共用宿主弱引用：两个闭包各要一份（WeakEntity 非 Copy）
            let filter_weak_col = filter_weak.clone();
            let sort_weak = cx.entity().downgrade();
            let insight_weak = cx.entity().downgrade();
            let preview_weak = cx.entity().downgrade();
            // 【#28-B3】右键行级动作（新增 / 复制 / 删除）：delegate 已打包成 ResultRequest，
            // 这里转投结果动作队列并让面板 notify 一下，由 observe 回调统一 drain 分派落库。
            let row_action_weak = cx.entity().downgrade();
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
                // 【筛选】列头菜单「按值筛选此列 / 清除」交回面板（空串 = 清除）
                let filter_weak = filter_weak_col;
                state.delegate_mut().set_column_filter_hook(std::rc::Rc::new(
                    move |column: usize, value: &str, app: &mut App| {
                        let value = value.to_string();
                        _ = filter_weak
                            .update(app, |panel, cx| panel.apply_column_filter(column, &value, cx));
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
                // 【#28-B3】右键新增 / 复制 / 删除行：动作已在 delegate 打包，转投队列后 notify，
                // 由面板 observe 回调 drain 落库（与状态栏按钮同一条分派路径）。
                state
                    .delegate_mut()
                    .set_row_action_hook(std::rc::Rc::new(move |request, app| {
                        _ = row_action_weak.update(app, |host, cx| {
                            host.result_action_queue.push(request);
                            host.result_panel.update(cx, |_, cx| cx.notify());
                        });
                    }));
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
            // #48 面板创建即取设置当前值（与内核默认一致；后续变化由 render 同步）
            show_line_numbers: editor_show_line_numbers_or_default(cx),
            // #64 自动换行缓存：创建时取设置值，render 权威同步点比对后同步内核
            soft_wrap: editor_soft_wrap_or_default(cx),
            // #65 自动闭合 / 智能缩进缓存：同 soft_wrap（创建时取设置值）
            auto_close: editor_auto_close_or_default(cx),
            smart_indent: editor_smart_indent_or_default(cx),
            // 用户只读开关默认关（可编辑）；roLock 点击后翻转
            user_ro_lock: false,
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
            result_panel,
            result_height: std::rc::Rc::new(Cell::new(ui::RESULT_PANE_HEIGHT)),
            result_action_queue,
            compare_grid: Some(compare_grid),
            pending_bind: None,
            error_site: None,
            script_card: None,
            error_site_key: None,
            window: window.window_handle(),
            value_preview: None,
            csv_active: false,
            pending_export: None,
            fetching_more: false,
            result_selection: None,
            exec_pump: RefCell::new(None),
            scanner: crate::scan::Scanner::new(),
            scan_revision: 0,
            scan_pending: false,
            scan_diagnostics: Vec::new(),
            error_reason: None,
            _editor_sub: Some(sub),
            _grid_sub: Some(grid_sub),
            _result_panel_sub: Some(result_panel_sub),
            _edit_sub: Some(_edit_sub),
            commit_in_flight: None,
            toolbar: crate::toolbar::ToolbarLayout::default(),
            invisible_decorations: None,
            bookmark_decorations: None,
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
        // #48 设置默认值落库：新建 / 未动过格式的文档应用（会话恢复在 show_document
        // 里于面板创建**之后**执行，会话值会覆盖这里的默认——文档级优先）。
        panel.apply_editor_defaults(cx);
        // 【遗留 #10】单元编辑器**不在构造期创建**：构造上下文里 `cx.new` + 订阅会
        // 触发「面板正在被更新」重入 panic。建实例走事件路径——`on_added_to`
        // （面板进 Dock）与单元增删方法。
        panel
    }

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

/// 执行结论 → 结果条目；同时回执事实帧候选（方案跳 1：无 batches 回执为 `None`）。
///
/// 事实档不挂进 [`ResultEntry`]（避免连带进它的 `Clone` / 等价比较与深拷贝），由入槽点
/// 单独传给 `ResultStore::push` 落到平行事实槽位。
fn entry_from(
    outcome: execution::ExecOutcome,
) -> (ResultEntry, Option<std::sync::Arc<::shared::FactFrame>>) {
    let connection = outcome.connection.clone();
    // 【B13】通道也随结论回来：标签星徽标与“切通道后标灰”靠它
    let channel = outcome.channel;
    // 【B15】血缘：失败也照样带（“这句话是下发筛选发出去的”对排查同样有用）
    let lineage = outcome.lineage;
    // 【A2】形成原料（下发筛选 / 排序下发）：刷新 ⟳ 按它重查，保持形态
    let pushdown = outcome.pushdown;
    match outcome.result {
        Ok(data) => {
            let fact = data.fact;
            let entry = ResultEntry::success(
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
            .with_pushdown(pushdown);
            (entry, fact)
        }
        Err(error) => {
            let entry = ResultEntry::failure(outcome.document, outcome.sql, error, 0)
                .with_connection(connection)
                .with_channel(channel)
                .with_lineage(lineage)
                .with_pushdown(pushdown);
            (entry, None)
        }
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
        let indent_folding = shared.indent_folding();
        let open_keywords = shared.fold_open_keywords();
        let close_keywords = shared.fold_close_keywords();
        fold::install(state, &text, indent_folding, &open_keywords, &close_keywords, cx);
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
        // 【书签】进 Dock 读一次 sidecar（有路径才读；事件路径，render 不碰 I/O）
        self.load_bookmarks(cx);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // #48 行号开关：设置 → 面板缓存 → 内核（render 是权威同步点，同 apply_left_mode）。
        // 只在值变化时调 `set_line_number`（内核值相同内部直接 return，不 notify → 无重绘循环）。
        let want_line_numbers = editor_show_line_numbers_or_default(cx);
        if self.show_line_numbers != want_line_numbers {
            self.show_line_numbers = want_line_numbers;
            self.editor.update(cx, |state, cx| {
                state.set_line_number(want_line_numbers, window, cx)
            });
        }
        // 【2026-10-27】折叠开关：设置 → shared.show_folds → 内核（render 权威同步点，
        // 同 #48 行号）。只开关在设置页（工具栏按钮已移除）；仅在值变化时重跑
        // `sync_fold`（install / set_folding），不 notify → 无重绘循环。
        let want_folds = editor_show_folds_or_default(cx);
        if self.shared.show_folds() != want_folds {
            self.shared.set_show_folds(want_folds);
            let shared = &self.shared;
            let document = self.document.clone();
            self.editor.update(cx, |state, cx| {
                sync_fold(shared, &document, state, window, cx);
            });
        }
        // 【2026-10-28】基于缩进的折叠开关：设置 → shared.indent_folding → 重新计算折叠候选。
        // 与 show_folds 同款同步逻辑。
        let want_indent_folding = editor_indent_folding_or_default(cx);
        if self.shared.indent_folding() != want_indent_folding {
            self.shared.set_indent_folding(want_indent_folding);
            let shared = &self.shared;
            let document = self.document.clone();
            self.editor.update(cx, |state, cx| {
                sync_fold(shared, &document, state, window, cx);
            });
        }
        // 【2026-10-28】自定义折叠关键字：设置 → shared → 重新计算折叠候选。
        let want_open_keywords = editor_fold_open_keywords_or_default(cx);
        let want_close_keywords = editor_fold_close_keywords_or_default(cx);
        if self.shared.fold_open_keywords() != want_open_keywords || self.shared.fold_close_keywords() != want_close_keywords {
            self.shared.set_fold_open_keywords(want_open_keywords);
            self.shared.set_fold_close_keywords(want_close_keywords);
            let shared = &self.shared;
            let document = self.document.clone();
            self.editor.update(cx, |state, cx| {
                sync_fold(shared, &document, state, window, cx);
            });
        }
        // #64 自动换行：设置 → 面板缓存 → 内核（render 权威同步点，同 #48 行号）。
        // 内核 `set_soft_wrap` 值相同内部直接 return，不 notify → 无重绘循环。
        let want_soft_wrap = editor_soft_wrap_or_default(cx);
        if self.soft_wrap != want_soft_wrap {
            self.soft_wrap = want_soft_wrap;
            self.editor.update(cx, |state, cx| {
                state.set_soft_wrap(want_soft_wrap, window, cx);
            });
        }
        // #65 自动闭合括号 / 智能缩进：与 soft_wrap 同款同步（render 权威同步点）。
        let want_auto_close = editor_auto_close_or_default(cx);
        if self.auto_close != want_auto_close {
            self.auto_close = want_auto_close;
            self.editor.update(cx, |state, cx| {
                state.set_auto_close(want_auto_close, window, cx);
            });
        }
        let want_smart_indent = editor_smart_indent_or_default(cx);
        if self.smart_indent != want_smart_indent {
            self.smart_indent = want_smart_indent;
            self.editor.update(cx, |state, cx| {
                state.set_smart_indent(want_smart_indent, window, cx);
            });
        }
        let (bg, primary, muted) = {
            let theme = cx.theme();
            (
                theme.colors.background,
                theme.colors.primary,
                theme.colors.muted_foreground,
            )
        };
        let _ = (primary, muted);

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
            // 【#54-C5】函数手册：`Ctrl+Shift+M`（键位在 crates/app 注册，context 同为 editor）
            .on_action(cx.listener(Self::on_function_manual))
            // 【#54-C4】签名帮助：`Ctrl+Shift+Space`
            .on_action(cx.listener(Self::on_signature_help))
            // 【B14】网格里的 `Ctrl+C`：键位绑在 `view/results/grid.rs` 那一层的上下文上，
            // 所以只有焦点在网格里时才会走到这里（编辑区里 `Ctrl+C` 仍归内核的文本复制）
            .on_action(cx.listener(Self::on_copy_grid_selection))
            // 【撤销/重做】网格里的 `Ctrl+Z` / `Ctrl+Shift+Z`：同挂在网格上下文上；
            // 编辑框内 `Ctrl+Z` 由输入内核处理，只有焦点在网格（非编辑框）时才到这两个
            .on_action(cx.listener(Self::on_undo_result_edit))
            .on_action(cx.listener(Self::on_redo_result_edit))
            // 【书签】`Ctrl+Shift+B` 切换 / `F8` 下一 / `Shift+F8` 上一（键位在 crates/app 注册）
            .on_action(cx.listener(Self::on_toggle_bookmark))
            .on_action(cx.listener(Self::on_next_bookmark))
            .on_action(cx.listener(Self::on_prev_bookmark));
        // 工具栏（②）：模式指示器在这里，模式不再是只能从状态栏读到的短标签
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        root = root.child(self.render_toolbar(mode, window, cx));
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
        // 编辑区 + 结果区：v_resizable 上下分栏（DBeaver 口径，结果集跟编辑器 tab 走，
        // 不再挂全局底部 dock）。结果区有内容时才加第二个 panel（无内容时高度为 0 不占位）。
        // 2026-10-28 修复「结果区不可拉伸」：此前被换成固定高度 div（去 Resizable 修遮挡
        // 的中间态），现恢复组件库 `v_resizable / resizable_panel`（不手搓拖动条），
        // 拖到哪记到哪（on_resize 回写 `result_height`，结果区关闭重开仍沿用）。
        let result_has_content = self.result_panel.read(cx).has_content();
        let font_size = cx.theme().font_size;
        let edit_area = div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-code-area".to_string())
            // 行号区域左侧对齐 Dock 面板左边：去掉左边 padding，只保留右边
            .pl_0()
            .pr_2()
            .on_drop({
                let entity = cx.entity();
                move |payload: &::shared::InsertFileDrag, window, app| {
                    entity.update(app, |this, cx| {
                        this.insert_file_contents(payload, window, cx)
                    });
                }
            })
            .child(self.render_editor_body(cx));

        // 编辑区 + 结果区：上下分栏（结果区有内容时才显示）
        let split = if result_has_content {
            let result_height = self.result_height.clone();
            v_resizable("editor-result-split")
                .child(
                    resizable_panel()
                        .child(edit_area),
                )
                .child(
                    resizable_panel()
                        .child(
                            div()
                                .size_full()
                                .min_h_0()
                                .child(self.result_panel.read(cx).render(cx)),
                        )
                        .size(font_size * self.result_height.get())
                        .size_range(
                            font_size * ui::RESULT_PANE_MIN_HEIGHT..font_size * ui::RESULT_MAX_HEIGHT,
                        ),
                )
                .on_resize(move |state, _window, _cx| {
                    // 拖到哪记到哪：上下分栏末位 panel 即结果区，换算回 rem 存面板字段
                    //（Pixels Div → f32；`px.0` 字段是 pub(crate) 不可外读）
                    if let Some(px) = state.read(_cx).sizes().last().copied() {
                        result_height.set(px / font_size);
                    }
                })
                .into_any_element()
        } else {
            edit_area.into_any_element()
        };
        root = root.child(
            div()
                .flex_1()
                .min_h_0()
                // 中间层必须是 flex_col：否则 edit_area 的 flex_1 在 block 布局里无效，
                // 无结果集时编辑器高度退化为内容高度（第 3 行被裁切、下面大片空白）。
                // 与 gpui-kit tab_group.rs / tab_panel.rs 注释里的 block 忽略 flex_grow 同因。
                .flex()
                .flex_col()
                .child(split),
        );
        // 编辑器自身状态栏（§2.5 定稿规格；需求③/去杂乱）：挂到编辑器 tab 底部。
        // 栏体由 `status_bar::render` 自绘（侧栏底 + 顶分隔线 + 固定高），
        // 文档级上下文（面包屑/方言/事务/微标/控件）都在其内，不再重复包一层。
        if let Some(status_bar) = render_editor_status_bar(self, &cx.entity(), cx) {
            root = root.child(status_bar);
        }
        root
    }
}



/// 编辑器自身状态栏（需求③ / 状态栏去杂乱）：把文档级上下文（连接·通道·事务 /
/// 环境族·未保存·执行中·消息 + 右段 行-列·编码·换行·缩进·模式·脏 + 编辑模式 /
/// 重设 / 事务 / 中断控件）迁到编辑器 tab 底部自己的状态栏，不再塞进工作台全局栏。
///
/// 由编辑器宿主自身的 `render` 挂载（把 `&self` 与自身实体句柄传入）：纯读构造
/// （不写 Shared / 不做 I/O），控件事件回调统一走 `panel.update`，不会重入正在
/// 渲染的宿主。返回 `None` = 没有活动文档 / 文档已关。
pub(crate) fn render_editor_status_bar(
    host: &EditorHostPanel,
    panel_entity: &Entity<EditorHostPanel>,
    cx: &App,
) -> Option<AnyElement> {
    if host.closed {
        return None;
    }
    let snapshot = host.status_snapshot(cx)?;
    let panel_entity = panel_entity.clone();

    // §2.5 右段微标补全：文档字节数 + 选区（字符 / 行数）+ 光标全文偏移从编辑器内核取（render 只读）。
    let (bytes, selected_chars, selected_rows, cursor_offset) = {
        let state = host.editor.read(cx);
        let value = state.value();
        let range = state.selected_range();
        let start = range.start.min(value.len());
        let end = range.end.min(value.len());
        let selected_chars = value[start..end].chars().count();
        let selected_rows = if start == end {
            0
        } else {
            value[start..end].chars().filter(|c| *c == '\n').count() + 1
        };
        // 光标全文偏移：内核给的是**字节**偏移，VSCode「Ch」微标是**字符**偏移——
        // 经 Rope `byte_to_char_idx` 换算（按 chunk 定位，大文件也是 O(log n)，不比 Ln/Col 贵）。
        let cursor_offset = {
            let bytes = state.cursor_offsets().first().copied().unwrap_or(0);
            state.text().byte_to_char_idx(bytes) + 1
        };
        (value.len(), selected_chars, selected_rows, cursor_offset)
    };

    // 状态栏输入（全部来自真实状态；无数据源的字段不放进结构体）
    let inputs = status_bar::StatusInputs {
        mode: snapshot.mode,
        dirty: snapshot.dirty,
        read_only: snapshot.read_only,
        statements: snapshot.statements,
        line: snapshot.line,
        column: snapshot.column,
        cursor_offset,
        cursor_count: snapshot.cursor_count,
        tab_size: snapshot.tab_size,
        hard_tabs: snapshot.hard_tabs,
        format: snapshot.format,
        bytes,
        selected_chars,
        selected_rows,
        message: snapshot.message.as_deref(),
        executing: snapshot.executing,
        elapsed: snapshot.elapsed,
        // 连接定位（短码·名称 / schema@db）已迁工具栏右区固定槽（render_conn_slots）；
        // 状态栏左段不再显示连接路径文本（labels().connection 保留给纯函数测试）
        connection: None,
        dialect: snapshot.dialect.as_deref(),
        // 执行方式由左段 channel 控件表达（可点切换）；文本段不再重复显示
        channel: None,
        env: snapshot.env.as_deref(),
        file_type: snapshot.file_type.as_deref(),
        lsp: snapshot.lsp.as_deref(),
        tx: snapshot.tx.as_deref(),
        session: snapshot.session.as_deref(),
        kernel: snapshot.kernel.as_deref(),
        units: snapshot.units.as_deref(),
    };

    // 执行方式（通道）由工具栏右区连接选择器旁的通道 pill 表达；状态栏不再占它
    let channel_ctrl = None;

    // ── 交互控件：编辑模式 / 缩进 / 换行 / 编码 / 不可见字符 / 事务 / 中断
    //（事件走 panel.update，不重入宿主）──
    // 编辑模式（行 / 列编辑；**循环按钮**——2026-10-27 去掉下拉框，点击在 行编辑/列编辑 间循环）
    let edit_mode_ctrl = {
        let mode = host.edit_mode;
        let row_entity = panel_entity.clone();
        let col_entity = panel_entity.clone();
        Button::new("editor-edit-mode")
            .ghost()
            .small()
            .debug_selector(|| "editor-edit-mode".to_string())
            .label(mode.short_label())
            .tooltip(match mode {
                EditMode::Row => "行编辑（点击切换为列编辑）",
                EditMode::Column => "列编辑（点击切换为行编辑）",
            })
            .on_click(move |_, window, app| {
                if mode == EditMode::Row {
                    col_entity.update(app, |panel, cx| {
                        panel.set_edit_mode(EditMode::Column, cx);
                    });
                } else {
                    // 切回行编辑：立即收拢多光标（走生产代码同一条 `enforce_edit_mode`）
                    row_entity.update(app, |panel, cx| {
                        panel.set_edit_mode(EditMode::Row, cx);
                        panel.enforce_edit_mode(window, cx);
                    });
                }
            })
            .into_any_element()
    };
    // 换行（LF / CRLF；2026-10-27 由下拉菜单改回**纯按钮循环切换**：点击 LF↔CRLF 翻转，
    // 无下拉箭头；宽度固定（以 CRLF 为基准）——切换时状态栏布局不乱动）
    let eol_ctrl = {
        let eol = snapshot.format.eol;
        let entity = panel_entity.clone();
        Button::new("editor-eol")
            .ghost()
            .small()
            .debug_selector(|| "editor-eol".to_string())
            .label(eol.label())
            .w(rems(ui::EOL_SLOT_WIDTH))
            .flex_none()
            .tooltip("换行符（点击切换 LF / CRLF）")
            .on_click(move |_, _window, app| {
                let next = match eol {
                    crate::encoding::LineEnding::Lf => crate::encoding::LineEnding::Crlf,
                    crate::encoding::LineEnding::Crlf => crate::encoding::LineEnding::Lf,
                };
                entity.update(app, |panel, cx| {
                    panel.set_eol(next, cx);
                });
            })
            .into_any_element()
    };
    // 编码（默认显示当前编码短名，点击弹出选择对话框；2026-10-27 拆出——见 encoding_dialog.rs）
    let encoding_ctrl = {
        let current = snapshot.format.encoding;
        let entity = panel_entity.clone();
        Button::new("editor-encoding")
            .ghost()
            .small()
            .debug_selector(|| "editor-encoding".to_string())
            .label(current.short_label())
            .tooltip("编码（点击选择）")
            .on_click(move |_, window, app| {
                let entity = entity.clone();
                crate::view::encoding_dialog::open_encoding_dialog(
                    window,
                    app,
                    current,
                    move |encoding, _window, app| {
                        entity.update(app, |panel, cx| panel.set_encoding(encoding, cx));
                    },
                );
            })
            .into_any_element()
    };
    // 缩进（VSCode 式菜单：`Tab` / `空格: N`，点击弹菜单选择 制表符 / 空格宽度；
    // 2026-10-27 合并 indent_kind + tab_size 两维度到一个入口，替换原「重设 ▾」里的缩进族）
    let indent_ctrl = {
        let current_tab = snapshot.tab_size;
        let hard_tabs = snapshot.hard_tabs;
        let entity = panel_entity.clone();
        // 对齐 VSCode：点击弹出缩进命令面板（双语项 + 分组标签 + tab_size 自定义输入框）
        Button::new("editor-indent")
            .ghost()
            .small()
            .debug_selector(|| "editor-indent".to_string())
            .label(if hard_tabs {
                "Tab".to_string()
            } else {
                format!("空格: {current_tab}")
            })
            .tooltip("缩进设置（点击打开）")
            .on_click(move |_, window, app| {
                let entity = entity.clone();
                let hard = hard_tabs;
                let tab = current_tab;
                crate::view::indent_dialog::open_indent_dialog(
                    window,
                    app,
                    hard,
                    tab,
                    move |action, _window, app| {
                        let entity = entity.clone();
                        entity.update(app, |panel, cx| match action {
                            crate::view::indent_dialog::IndentAction::UseSpaces => {
                                panel.set_hard_tabs(false, cx);
                            }
                            crate::view::indent_dialog::IndentAction::UseTabs => {
                                panel.set_hard_tabs(true, cx);
                            }
                            crate::view::indent_dialog::IndentAction::ChangeTabDisplaySize(size) => {
                                panel.set_hard_tabs(false, cx);
                                panel.set_tab_size(size.max(1), cx);
                            }
                            crate::view::indent_dialog::IndentAction::DetectFromContent => {
                                panel.detect_indent_from_content(cx);
                            }
                            crate::view::indent_dialog::IndentAction::ConvertToSpaces => {
                                panel.convert_indent(false, _window, cx);
                            }
                            crate::view::indent_dialog::IndentAction::ConvertToTabs => {
                                panel.convert_indent(true, _window, cx);
                            }
                            crate::view::indent_dialog::IndentAction::TrimTrailingWhitespace => {
                                panel.trim_trailing_whitespace(_window, cx);
                            }
                        });
                    },
                );
            })
            .into_any_element()
    };
    // 不可见字符开关（2026-10-27 升级：点击弹出**双栏配置面板**，自定义每类符号显示/隐藏；
    // ¶ 按钮 toggled 表示「当前有类别在标注」；面板见 `view/invisible_dialog.rs`）
    let invisible_ctrl = {
        let show_invisible = host.shared.document_show_invisible(&host.document);
        let mask = host.shared.document_invisible_mask(&host.document);
        let entity = panel_entity.clone();
        Button::new("editor-invisible")
            .ghost()
            .small()
            .debug_selector(|| "editor-invisible".to_string())
            .label("¶")
            .toggled(show_invisible)
            .tooltip("不可见字符标注（点击自定义：零宽 / 特殊空格 / 双向控制 / 控制字符）")
            .on_click(move |_, window, app| {
                let entity = entity.clone();
                let current_mask = mask;
                crate::view::invisible_dialog::open_invisible_dialog(
                    window,
                    app,
                    current_mask,
                    move |new_mask, _window, app| {
                        entity.update(app, |panel, cx| {
                            panel.set_invisible_categories(new_mask, cx);
                        });
                    },
                );
            })
            .into_any_element()
    };
    // 事务（提交 / 回滚；只有会通信 + 执行器支持事务 + 通道支持事务才摆）
    let tx_available = snapshot.connection.is_some()
        && host.shared.has_transactions()
        && host.channel().allows_transactions();
    let tx_ctrl = tx_available.then(|| {
        let mut row = div().h_flex().items_center().gap_1();
        if host.tx_open {
            for (action, id) in [
                (execution::TxAction::Commit, "editor-tx-commit"),
                (execution::TxAction::Rollback, "editor-tx-rollback"),
            ] {
                let entity = panel_entity.clone();
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
    // 中断（B3）：只在本文档真有语句没回填时出现
    let interrupt = host.can_interrupt().then(|| {
        let entity = panel_entity.clone();
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

    let controls = status_bar::StatusControls {
        // 2026-10-27：连接 ▸ 库 ▸ Schema 定位整体迁工具栏右区固定槽（render_conn_slots），
        // 状态栏不再摆面包屑（左段只留方言 + 执行方式 + 事务等当前上下文）
        edit_mode: Some(edit_mode_ctrl),
        indent: Some(indent_ctrl),
        encoding: Some(encoding_ctrl),
        eol: Some(eol_ctrl),
        invisible: Some(invisible_ctrl),
        tx: tx_ctrl,
        channel: channel_ctrl,
        interrupt,
    };
    Some(status_bar::render(&inputs, controls, cx).into_any_element())
}

/// 工具栏右区「执行通道」pill（源库 / 加速 / 联邦可切；原型 §2.2 通道级+连接层）。
///
/// 2026-10-27：pill 固定宽（`CHANNEL_SLOT_WIDTH`）——与连接名称 / schema@db 并成
/// 工具栏右区三固定区，不随标签长短推挤布局；超长由容器裁掉（源库/加速/联邦本就很短，
/// 兜底用）。菜单构建发生在点击时（事件路径）——`channel_menu` 的门控真值来自宿主注入的
/// `ChannelsPort`；未注入端口时如实报「尚未接入」，不假装有通道可选。
fn render_channel_control(
    host: &EditorHostPanel,
    panel_entity: &Entity<EditorHostPanel>,
) -> Option<AnyElement> {
    let entity = panel_entity.clone();
    let current = host.channel();
    let items = host.channel_menu();
    let refresh_label = channel::refresh_item_label(current);
    Some(
        div()
            .flex_none()
            .overflow_hidden()
            .child(
                Button::new("editor-channel")
                    // 执行通道 pill 带背景（2026-10-27）：与连接/schema 两个 ghost 槽区分，
                    // 一眼看出「执行发往哪一档」
                    .with_variant(ButtonVariant::Secondary)
                    .small()
                    .debug_selector(|| "editor-channel".to_string())
                    // 执行位置图标化：图标（Locate = 定位执行到哪里）+ 通道短标签，去「执行位置：」文字前缀
                    .icon(IconName::Locate)
                    .label(format!("{} ▾", current.label()))
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
                                        entity.update(app, |panel, cx| {
                                            panel.set_channel(channel, cx);
                                        });
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
                    .into_any_element(),
            )
            .into_any_element(),
    )
}

/// 工具栏右区槽位长名截断（> `max` 字符 → 前 `max-1` + …；完整名靠菜单项 / 工具提示）
fn truncate_label(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut shortened: String = text.chars().take(max.saturating_sub(1)).collect();
        shortened.push('…');
        shortened
    }
}

/// 工具栏右区「连接定位」固定槽：运行点 + 连接名称（短码·名称） ▾ schema@库
///
/// 2026-10-27 按用户要求：连接定位从状态栏移工具栏右区，且**固定大小占用区域**——
/// 执行通道 pill / 连接名称 / schema@db 三区各自固定宽（`w(rem N).flex_none`），
/// 超长截断（`truncate_label` + 容器 `overflow_hidden` 兜底），不随内容推挤布局。
/// 状态栏左段不再占连接路径（`status_bar::render` 同步改，只留方言/事务等上下文）。
///
/// - **连接名称**：菜单只列**实时在线**连接（事件路径 `online_ids`）+「跟随当前连接」
///   （默认态）；运行点是**静态绑定指示**（绑定即亮；在线判定留给菜单打开时）。
/// - **schema@库**：schema 可切（schemas + 使用连接默认）；显示 `schema@库`（库 = catalog，
///   从 schemas 列表按当前 schema 反查）；绑定但未选时给可点 `schema ▾` 占位；
///   未绑定连接（跟随当前连接态）时无槽——schema 概念还没出现，不假装。
fn render_conn_slots(
    host: &EditorHostPanel,
    panel_entity: &Entity<EditorHostPanel>,
    theme: &gpui_kit::component::Theme,
) -> Option<AnyElement> {
    let entity = panel_entity.clone();
    let shared = host.shared.clone();
    let options = host.shared.connection_options();
    let has_port = host.shared.has_connections();
    let bound = host
        .with_document(|doc| doc.connection().map(str::to_string))
        .flatten();
    let schema = host
        .with_document(|doc| doc.schema().map(str::to_string))
        .flatten();
    let has_bound = bound.is_some();
    let schemas = host.shared.schemas(bound.as_deref());

    // ── 连接名称槽（固定宽；短码·名称 截断）──
    let conn_label = bound
        .as_deref()
        .and_then(|id| {
            options
                .iter()
                .find(|o| o.id == id)
                .map(|o| format!("{}·{}", o.short, o.name))
        })
        .unwrap_or_else(|| "跟随当前连接".to_string());
    let dot_color = if bound.is_some() {
        theme.colors.success
    } else {
        theme.colors.muted_foreground
    };
    let conn_entity = entity.clone();
    // 连接名称槽（2026-10-28 重构）：使用 DropdownButton 组件，左边是名称按钮，
    // 右边是下拉箭头，合并成一个整体，避免两个按钮分开的视觉问题。
    let conn_name_button = Button::new("editor-conn-name")
        .debug_selector(|| "editor-conn-name".to_string())
        .icon(
            Icon::empty()
                .path("icons/dot.svg")
                .size(theme.font_size * ui::STATUS_DOT_SIZE)
                .text_color(dot_color),
        )
        .label(truncate_label(&conn_label, 14))
        .on_click({
            let entity = entity.clone();
            let shared = shared.clone();
            let bound = bound.clone();
            move |_, window, app| {
                let options = shared.connection_options();
                let current = bound.clone();
                let entity = entity.clone();
                crate::view::dialogs::open_connection_picker(
                    window,
                    app,
                    options,
                    current,
                    move |id, _window, app| {
                        entity.update(app, |panel, cx| panel.bind_connection(id, cx));
                    },
                );
            }
        });
    let conn_slot = DropdownButton::new("editor-conn-dropdown")
        .button(conn_name_button)
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            if !has_port {
                return menu
                    .item(PopupMenuItem::new("（未接入连接列表）").disabled(true));
            }
            let follow_entity = conn_entity.clone();
            let unbound = bound.is_none();
            menu = menu.item(
                PopupMenuItem::new("跟随当前连接")
                    .checked(unbound)
                    .on_click(move |_, _window, app| {
                        follow_entity.update(app, |panel, cx| {
                            panel.bind_connection(None, cx);
                        });
                    }),
            );
            menu = menu.separator();
            // 实时在线（菜单打开时查，事件路径）：只列真在线连接
            let online = shared.online_ids();
            let online_conns: Vec<(String, String)> = options
                .iter()
                .filter(|o| online.contains(&o.id))
                .map(|o| (format!("{}·{}", o.short, o.name), o.id.clone()))
                .collect();
            if online_conns.is_empty() {
                return menu
                    .item(PopupMenuItem::new("（暂无在线连接）").disabled(true));
            }
            for (label, id) in &online_conns {
                let entity = conn_entity.clone();
                let is_current = bound.as_deref() == Some(id.as_str());
                let label = label.clone();
                let id = id.clone();
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
        .with_variant(ButtonVariant::Secondary)
        .small()
        .into_any_element();

    // ── schema@库 槽（固定宽；有绑定连接才摆；`schema@catalog` 截断）──
    // 2026-10-27 拆两段：名称区点击弹「选择 Database/Schema」窗（左库右 Schema 选绑定），
    // ▾ 区点击弹现有 schema 下拉（含「使用连接默认」）。显示规则：有 schema 时 `schema@库`，
    // 库即 Schema（catalog 为空）时只显示库名——与弹窗同一口径。
    let schema_slot = has_bound.then(|| {
        let schemas_menu = schemas.clone();
        let current_schema = schema.clone();
        let entity = entity.clone();
        let label = match &schema {
            Some(s) => {
                let catalog = schemas
                    .iter()
                    .find(|item| item.schema == *s)
                    .and_then(|item| item.catalog.clone());
                match catalog {
                    Some(c) => format!("{s}@{c}"),
                    None => s.clone(),
                }
            }
            None => "schema".to_string(),
        };
        let schema_name_button = Button::new("editor-schema-name")
            .debug_selector(|| "editor-schema-name".to_string())
            .label(truncate_label(&label, 12))
            .on_click({
                let entity = entity.clone();
                let schemas = schemas.clone();
                let schema = schema.clone();
                move |_, window, app| {
                    let schemas = schemas.clone();
                    let schema = schema.clone();
                    let entity = entity.clone();
                    crate::view::dialogs::open_schema_picker(
                        window,
                        app,
                        schemas,
                        schema,
                        move |sch, _window, app| {
                            entity.update(app, |panel, cx| {
                                panel.set_schema(Some(sch), cx);
                            });
                        },
                    );
                }
            });
        DropdownButton::new("editor-schema-dropdown")
            .button(schema_name_button)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if schemas_menu.is_empty() {
                    return menu
                        .item(PopupMenuItem::new("（暂无可切 Schema）").disabled(true));
                }
                for item in &schemas_menu {
                    let entity = entity.clone();
                    let schema_name = item.schema.clone();
                    let is_current =
                        current_schema.as_deref() == Some(schema_name.as_str());
                    menu = menu.item(
                        PopupMenuItem::new(schema_name.clone())
                            .checked(is_current)
                            .on_click(move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.set_schema(Some(schema_name.clone()), cx);
                                });
                            }),
                    );
                }
                let entity = entity.clone();
                let using_default = current_schema.is_none();
                menu = menu.item(
                    PopupMenuItem::new("使用连接默认")
                        .checked(using_default)
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.set_schema(None, cx));
                        }),
                );
                menu
            })
            .with_variant(ButtonVariant::Secondary)
            .small()
            .into_any_element()
    });

    Some(
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .child(conn_slot)
            .children(schema_slot)
            .into_any_element(),
    )
}
