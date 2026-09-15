//! 工作台 Dock 面板（Round 20）
//!
//! DockArea 布局系统接入后，原侧边栏/内容区拆为独立面板实体：
//! - `SidebarPanel`：按活动工具渲染连接列表 / 导航树 / 资源 / 设置；
//! - `EditorPanel`：中央内容区（连接详情 + 新建连接）；
//! - `Shared`：面板与工作台之间共享的状态（工具 / 选中连接 / 连接数据 / 通知文案）。
//!
//! 面板只实现 GPUI 面板协议（base `Panel` + component `Panel`），不承载业务逻辑；
//! 数据仍来自 `ConnectionItem::sample()`（占位），下一轮接 ConnectionService。

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::EventEmitter;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::{StyledExt, VirtualListScrollHandle, h_resizable, resizable_panel, v_virtual_list};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::commands::{
    NavCollapse, NavDown, NavExpand, NavOpenProperties, NavUp, ScratchpadCancelEdit,
    ScratchpadDelete, ScratchpadDown, ScratchpadNewFile, ScratchpadOpen, ScratchpadRename,
    ScratchpadSelectAll, ScratchpadUp,
};
use crate::components::connection_dialog;

use scratchpad::{
    ExternalReferenceStatus, ScratchpadEntry, ScratchpadEntryKind, ScratchpadStore, TrashEntry,
};

use database::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, PropertyKind, PropertyRef,
};
use database::sql_gen::DmlKind;

use crate::services::nav_jobs;
use crate::services::scratchpad_jobs;

use crate::services::db_navigator::NavTable;
use crate::services::query_runner::QueryOutput;
use crate::ui;
use crate::view::{ConnectionItem, LeftPanel, RightPanel, SidebarMode};
use mock::mock_view::{MockDetailView, MockPanel};
use mock::mock_view::SchemaRequest;

/// 连接对话框「项目栏」动作项 → 宿主消费分支的动作请求（#9）。
///
/// 生产方是 `connection_dialog::handle_project_confirm`（置位两个标记之一），
/// 消费方是 `WorkbenchView::render`——`Shared::take_project_action_request` 把
/// “标记 → 动作”的决策与“只消费一次”的语义收在一处，可直接单测（无需窗口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectActionRequest {
    /// 「＋ 新增项目」：走未保存拦截后直接打开新建项目对话框。
    CreateProject,
    /// 「打开现有目录…」：打开目录选择对话框。
    OpenFolder,
}

/// 面板与工作台共享的状态。
#[derive(Clone)]
pub struct Shared {
    pub active_left: Rc<Cell<LeftPanel>>,
    pub active_right: Rc<Cell<RightPanel>>,
    /// 左右边栏三模式（展开 / 收起 / 完全隐藏），互不影响。
    pub left_mode: Rc<Cell<SidebarMode>>,
    pub right_mode: Rc<Cell<SidebarMode>>,
    /// 完全隐藏前的模式快照：状态栏独立开关恢复时还原，避免丢失隐藏前状态。
    pub left_mode_before_hidden: Rc<Cell<SidebarMode>>,
    pub right_mode_before_hidden: Rc<Cell<SidebarMode>>,
    /// Quick Open 弹层开关（Ctrl+P / 标题栏搜索条）。
    pub quick_open: Rc<Cell<bool>>,
    /// 设置面板开关（活动栏底部 ⚙ / OpenSettings 命令）。
    pub settings_open: Rc<Cell<bool>>,
    pub selected: Rc<Cell<Option<usize>>>,
    pub connections: Rc<RefCell<Vec<ConnectionItem>>>,
    pub notice: Rc<RefCell<Option<String>>>,
    /// Round 25：数据库导航缓存（哪个连接加载的 + 表→列树）。
    pub nav_for: Rc<RefCell<Option<String>>>,
    pub nav_tables: Rc<RefCell<Vec<NavTable>>>,
    /// Round 30：SQL 结果归属（哪个连接执行的，切换连接即失效）。
    pub sql_for: Rc<RefCell<Option<String>>>,
    /// 编辑请求（侧边栏「编辑」→ EditorPanel 渲染时消费并打开对话框）。
    pub open_edit: Rc<RefCell<Option<String>>>,
    /// 新建数据源请求（导航面板头「＋」/ 空态按钮 → EditorPanel 渲染时消费并打开对话框）。
    pub new_connection_request: Rc<Cell<bool>>,
    /// 连接对话框的项目下拉选中「＋ 新增项目」→ 宿主打开项目新建入口（由 `WorkbenchView` 消费）。
    pub project_new_request: Rc<Cell<bool>>,
    /// 连接对话框的项目下拉选中「打开现有目录…」→ 宿主打开目录选择（由 `WorkbenchView` 消费）。
    pub project_open_request: Rc<Cell<bool>>,
    /// P0：当前项目会话（草稿箱根 / 项目作用域连接 / 标题栏项目名共用）。
    pub project: Rc<RefCell<Option<project::ui::OpenProject>>>,
    /// M5：草稿箱内容搜索结果（侧栏发起，结果落中央编辑区）。
    pub scratchpad_search: Rc<RefCell<Option<ScratchpadSearchView>>>,
    /// M1 项目管理 UI 状态（选择器 / 菜单 / 对话框 / 设置 / 项目锁）。
    pub project_ui: Rc<RefCell<project::ui::ProjectUiState>>,
    /// 编辑区是否存在未保存草稿（切换 / 关闭项目拦截信号）。
    pub editor_dirty: Rc<Cell<bool>>,
    /// 编辑区当前 SQL 文本（未保存草稿保存时使用）。
    pub editor_sql: Rc<RefCell<String>>,
    /// 清空编辑区的宿主命令（保存 / 放弃未保存草稿后调用；事件上下文执行，非 render）。
    pub editor_clear: Rc<RefCell<Option<Rc<dyn Fn(&mut Window, &mut App)>>>>,
    /// 待注入编辑区的 SQL（导航右键「新建查询 / 查看数据」→ EditorPanel 渲染时消费）。
    pub editor_set: Rc<RefCell<Option<String>>>,
    /// Phase B：属性面板请求（数据源导航双击对象 → 编辑区右侧面板）。
    pub property_target: Rc<RefCell<Option<PropertyRequest>>>,
    /// M7：Mock 面板实体句柄（弱引用；用于导航右键定向导入源库结构）。
    ///
    /// 面板自带状态与对话框（`mock::mock_view::MockPanel`），工作台只持句柄。
    pub mock_panel: Rc<RefCell<Option<WeakEntity<MockPanel>>>>,
    /// M7：中央「Mock 数据」详情 tab 实体句柄（弱引用；已关闭则重新创建）。
    pub mock_detail: Rc<RefCell<Option<WeakEntity<MockDetailView>>>>,
    /// M7：打开 Mock 详情 tab 的宿主命令（面板「查看详情」调用；需要窗口，照 `editor_clear` 口径）。
    pub open_mock_detail: Rc<RefCell<Option<Rc<dyn Fn(&mut Window, &mut App)>>>>,
    /// Phase C：Ctrl+F 请求聚焦导航搜索框（宿主置位，导航面板渲染时消费）。
    pub focus_nav_search: Rc<Cell<bool>>,
    /// 驱动 id → 类型 / 显示名（徽标、hover 卡与属性面板共用；随组织数据一次性加载）。
    pub driver_catalog: Rc<RefCell<HashMap<String, crate::services::nav_runtime::DriverMeta>>>,
    /// 宿主重绘桥：连接对话框层挂在 `WorkbenchView::render` 上，而 `Root` 的
    /// notify 不会让子视图重建元素树；打开 / 关闭对话框后必须显式通知宿主重渲染。
    pub host_redraw: Rc<RefCell<Option<Rc<dyn Fn(&mut App)>>>>,
}

impl Shared {
    /// 用真实连接数据初始化共享状态（连接为空时 selected=None，进入空态 UI）。
    pub fn with_connections(connections: Vec<ConnectionItem>, notice: Option<String>) -> Self {
        Self {
            active_left: Rc::new(Cell::new(LeftPanel::Database)),
            active_right: Rc::new(Cell::new(RightPanel::Insight)),
            left_mode: Rc::new(Cell::new(SidebarMode::Expanded)),
            right_mode: Rc::new(Cell::new(SidebarMode::Collapsed)),
            left_mode_before_hidden: Rc::new(Cell::new(SidebarMode::Expanded)),
            right_mode_before_hidden: Rc::new(Cell::new(SidebarMode::Expanded)),
            quick_open: Rc::new(Cell::new(false)),
            settings_open: Rc::new(Cell::new(false)),
            selected: Rc::new(Cell::new(if connections.is_empty() {
                None
            } else {
                Some(0)
            })),
            connections: Rc::new(RefCell::new(connections)),
            notice: Rc::new(RefCell::new(notice)),
            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
            sql_for: Rc::new(RefCell::new(None)),
            open_edit: Rc::new(RefCell::new(None)),
            new_connection_request: Rc::new(Cell::new(false)),
            project_new_request: Rc::new(Cell::new(false)),
            project_open_request: Rc::new(Cell::new(false)),
            project: Rc::new(RefCell::new(None)),
            scratchpad_search: Rc::new(RefCell::new(None)),
            project_ui: Rc::new(RefCell::new(Default::default())),
            editor_dirty: Rc::new(Cell::new(false)),
            editor_sql: Rc::new(RefCell::new(String::new())),
            editor_clear: Rc::new(RefCell::new(None)),
            editor_set: Rc::new(RefCell::new(None)),
            property_target: Rc::new(RefCell::new(None)),
            mock_panel: Rc::new(RefCell::new(None)),
            mock_detail: Rc::new(RefCell::new(None)),
            open_mock_detail: Rc::new(RefCell::new(None)),
            focus_nav_search: Rc::new(Cell::new(false)),
            driver_catalog: Rc::new(RefCell::new(HashMap::new())),
            host_redraw: Rc::new(RefCell::new(None)),
        }
    }

    pub fn new() -> Self {
        Self::with_connections(Vec::new(), None)
    }

    /// 构建草稿箱存储 + 运行时（未打开项目时报错）。
    ///
    /// 侧栏（草稿箱面板）与中央编辑区（内容搜索结果 / 替换）共用。
    pub fn scratchpad_store(&self) -> Result<(ScratchpadStore, tokio::runtime::Runtime), String> {
        let root = self
            .project
            .borrow()
            .as_ref()
            .map(|s| s.root.clone())
            .ok_or_else(|| "未打开项目".to_string())?;
        let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
        Ok((ScratchpadStore::new(root), rt))
    }

    /// 取出（并清空）项目栏的动作请求：宿主 render 每帧调用一次。
    ///
    /// - 两个标记同时置位 → 以「＋ 新增项目」优先（同一帧内的竞态，无语义歧义）；
    /// - 都未置位 → `None`（绝大多数帧，零开销）；
    /// - 取出即清空 → 同一次请求不会在后续帧重复开窗。
    pub fn take_project_action_request(&self) -> Option<ProjectActionRequest> {
        let new = self.project_new_request.replace(false);
        let open = self.project_open_request.replace(false);
        match (new, open) {
            (true, _) => Some(ProjectActionRequest::CreateProject),
            (false, true) => Some(ProjectActionRequest::OpenFolder),
            (false, false) => None,
        }
    }

    /// 当前选中连接（克隆，避免长时间持有 RefCell 借用）。
    pub fn selected_connection(&self) -> Option<ConnectionItem> {
        let conns = self.connections.borrow();
        self.selected.get().and_then(|i| conns.get(i)).cloned()
    }

    /// 通知宿主（`WorkbenchView`）重绘：模态层的挂载点在宿主 render 中，
    /// 仅靠 `Root` 的 notify 不会更新子视图元素树（`cx.notify` 只重渲染该视图子树）。
    pub fn notify_host(&self, cx: &mut App) {
        let bridge = self.host_redraw.borrow().clone();
        if let Some(redraw) = bridge {
            redraw(cx);
        }
    }

    /// 展开右 Dock 并切到指定面板。
    ///
    /// 与 `SidebarEvent::OpenRightPanel` 同一口径：只改状态，布局同步由宿主 render
    /// （`apply_right_mode`）完成，因此还需要 `notify_host` 让宿主重渲染。
    pub fn open_right_panel(&self, panel: RightPanel, cx: &mut App) {
        self.active_right.set(panel);
        self.right_mode.set(SidebarMode::Expanded);
        self.notify_host(cx);
    }

    /// 打开 Mock 面板；`source` 给定时按**源库表**定向（导航右键「生成 Mock 数据」）。
    ///
    /// 定向动作（读源库结构 + 预填目标表名）在事件路径执行：面板实体随右栏面板
    /// **构造期创建**（`RightSidebarPanel::new`），因此这里总能拿到句柄。
    pub fn open_mock_panel(&self, source: Option<SchemaRequest>, cx: &mut App) {
        self.open_right_panel(RightPanel::Mock, cx);
        let Some(source) = source else {
            return;
        };
        let panel = self.mock_panel.borrow().clone();
        if let Some(panel) = panel.and_then(|weak| weak.upgrade()) {
            panel.update(cx, |panel, cx| panel.preset_from_source(source, cx));
        }
    }
}

/// 侧边栏面板发出的事件（由工作台订阅）。
#[derive(Clone, Debug)]
pub enum SidebarEvent {
    /// 用户点击了连接条目。
    SelectConnection(usize),
    /// 用户点击了连接「编辑」。
    EditConnection(String),
    /// 用户点击了导航面板头「＋」/ 空态「新建连接」（已置位 `shared.new_connection_request`）。
    NewConnectionRequest,
    /// 导航右键「查看数据」已写入 `shared.editor_set`，请编辑区渲染时消费。
    EditorSqlRequest,
    /// 通用入口：连接 / 对象右键「在 SQL 编辑器中打开」——选中该连接并聚焦中央编辑区。
    OpenSqlEditor(String),
    /// 通用入口：右键「生成 Mock 数据」（仅表 / 视图）/「查看洞察」——展开右 Dock 并切面板。
    OpenRightPanel(RightPanel),
}

/// 侧边栏面板：按活动工具渲染内容。
pub struct SidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// M5 草稿箱面板状态（首次渲染触发加载）。
    scratchpad: Rc<RefCell<ScratchpadView>>,
    /// 数据源导航面板状态（懒加载对象树）。
    database_nav: Rc<RefCell<DatabaseNavView>>,
    /// 数据源导航搜索框（懒创建）。
    nav_search: Option<Entity<InputState>>,
    _nav_search_sub: Option<Subscription>,
    /// 连接行内联标签输入框（组织编辑器打开时创建，关闭时销毁）。
    nav_tag_input: Option<Entity<InputState>>,
    /// 当前标签输入框对应的连接 ID（切换连接时重建并重新预填）。
    nav_tag_input_for: Option<String>,
    _nav_tag_sub: Option<Subscription>,
    /// 行内「复制为模板」输入框（打开时创建，关闭时销毁）。
    nav_copy_input: Option<Entity<InputState>>,
    /// 当前复制输入框对应的连接 ID（切换连接时重建）。
    nav_copy_input_for: Option<String>,
    _nav_copy_sub: Option<Subscription>,
    /// 分组名内联重命名输入框（重命名时创建，关闭时销毁）。
    nav_group_input: Option<Entity<InputState>>,
    _nav_group_sub: Option<Subscription>,
    /// 渲染顺序重建的可见项（键盘 ↑↓ 移动 / 展开折叠 / 打开属性）。
    nav_order: Rc<RefCell<Vec<NavOrderItem>>>,
    /// 正在轮询预热进度的后台任务（避免重复启动）。
    warm_poll: Option<Task<()>>,
    /// 正在轮询导航加载结果的后台任务（避免重复启动；`&self` 路径也要访问）。
    nav_pump: RefCell<Option<Task<()>>>,
    /// 正在轮询草稿箱加载结果的后台任务（同上）。
    scratchpad_pump: RefCell<Option<Task<()>>>,
}

/// 内联编辑（新建 / 重命名 / 新建引用 / 引用改名）。
#[derive(Clone)]
enum ScratchpadEdit {
    /// 新建文件（使用 `ScratchpadView::new_template` 选中的模板）。
    NewFile,
    NewFolder,
    /// 已选定外部路径，待输入别名（引用不复制文件，只记路径）。
    NewReference { path: std::path::PathBuf },
    Rename { path: String },
    RenameReference { alias: String },
}

/// 新建文件的起步模板（原型 §4.1：自动补后缀 + 填充占位内容）。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ScratchpadTemplate {
    #[default]
    Blank,
    Sql,
    Python,
    Markdown,
    Json,
}

impl ScratchpadTemplate {
    /// 全部模板（渲染 chip 行的顺序）。
    const ALL: [ScratchpadTemplate; 5] = [
        ScratchpadTemplate::Blank,
        ScratchpadTemplate::Sql,
        ScratchpadTemplate::Python,
        ScratchpadTemplate::Markdown,
        ScratchpadTemplate::Json,
    ];

    fn label(self) -> &'static str {
        match self {
            ScratchpadTemplate::Blank => "空白",
            ScratchpadTemplate::Sql => "SQL",
            ScratchpadTemplate::Python => "Python",
            ScratchpadTemplate::Markdown => "Markdown",
            ScratchpadTemplate::Json => "JSON",
        }
    }

    /// 默认后缀（空白模板不加后缀）。
    fn extension(self) -> Option<&'static str> {
        match self {
            ScratchpadTemplate::Blank => None,
            ScratchpadTemplate::Sql => Some(".sql"),
            ScratchpadTemplate::Python => Some(".py"),
            ScratchpadTemplate::Markdown => Some(".md"),
            ScratchpadTemplate::Json => Some(".json"),
        }
    }

    /// 占位内容（创建后写入，可直接编辑）。
    fn content(self, name: &str) -> String {
        match self {
            ScratchpadTemplate::Blank => String::new(),
            ScratchpadTemplate::Sql => {
                format!("-- {name}\n-- 草稿：随手 SQL，Ctrl+S 保存回草稿箱\nSELECT 1;\n")
            }
            ScratchpadTemplate::Python => {
                format!("# {name}\n\n\ndef main() -> None:\n    pass\n\n\nif __name__ == \"__main__\":\n    main()\n")
            }
            ScratchpadTemplate::Markdown => format!("# {name}\n\n- \n"),
            ScratchpadTemplate::Json => "{\n  \n}\n".to_string(),
        }
    }
}

/// 按模板补后缀：用户自写的后缀保留；模板补的后缀则随模板切换替换。
fn scratchpad_apply_template_ext(name: &str, template: ScratchpadTemplate) -> String {
    // 已知模板后缀视为“模板补的”，可替换。
    const TEMPLATE_EXTS: [&str; 4] = [".sql", ".py", ".md", ".json"];
    let (_, current) = scratchpad_split_name(name);
    let base = if current.is_empty() {
        name.to_string()
    } else if TEMPLATE_EXTS.contains(&current.as_str()) {
        name[..name.len() - current.len()].to_string()
    } else {
        // 用户自己的后缀（如 `.txt`）→ 尊重不动。
        return name.to_string();
    };
    match template.extension() {
        Some(ext) => format!("{base}{ext}"),
        None => base,
    }
}

/// 删除撤销（底部撤销栏；批量删除时含多条回收站条目）。
#[derive(Clone)]
struct ScratchpadUndo {
    label: String,
    trash_ids: Vec<String>,
}

/// 剪贴板模式。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScratchpadClipboardMode {
    Cut,
    Copy,
}

/// 文件剪贴板（路径相对模块根）。
#[derive(Clone)]
struct ScratchpadClipboard {
    mode: ScratchpadClipboardMode,
    paths: Vec<String>,
}

/// 搜索模式（文件名过滤 / 内容搜索）。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ScratchpadSearchMode {
    #[default]
    Name,
    Content,
}

/// 内容搜索命中项（视图模型）。
#[derive(Clone)]
struct ScratchpadSearchHit {
    file: String,
    line: usize,
    content: String,
    /// 行内命中区间（字节偏移，来自后端 `SearchMatch::match_spans`）。
    spans: Vec<(usize, usize)>,
    before: Vec<String>,
    after: Vec<String>,
}

/// 内容搜索结果（落中央编辑区；侧栏发起写入，编辑区读取渲染）。
#[derive(Clone)]
pub struct ScratchpadSearchView {
    query: String,
    is_regex: bool,
    case_sensitive: bool,
    scanned: usize,
    truncated: bool,
    hits: Vec<ScratchpadSearchHit>,
}

/// 文件树排序键。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ScratchpadSort {
    #[default]
    Name,
    Size,
    Modified,
}

/// 排序标签（底部状态展示）。
fn scratchpad_sort_label(sort: ScratchpadSort, desc: bool) -> &'static str {
    match (sort, desc) {
        (ScratchpadSort::Name, false) => "名称 ↑",
        (ScratchpadSort::Name, true) => "名称 ↓",
        (ScratchpadSort::Size, false) => "大小 ↑",
        (ScratchpadSort::Size, true) => "大小 ↓",
        (ScratchpadSort::Modified, false) => "时间 ↑",
        (ScratchpadSort::Modified, true) => "时间 ↓",
    }
}

/// 循环切换排序：名称↑ → 名称↓ → 大小↑ → 大小↓ → 时间↑ → 时间↓ → 名称↑。
fn scratchpad_cycle_sort(sort: &mut ScratchpadSort, desc: &mut bool) {
    match (*sort, *desc) {
        (ScratchpadSort::Name, false) => *desc = true,
        (ScratchpadSort::Name, true) => {
            *sort = ScratchpadSort::Size;
            *desc = false;
        }
        (ScratchpadSort::Size, false) => *desc = true,
        (ScratchpadSort::Size, true) => {
            *sort = ScratchpadSort::Modified;
            *desc = false;
        }
        (ScratchpadSort::Modified, false) => *desc = true,
        (ScratchpadSort::Modified, true) => {
            *sort = ScratchpadSort::Name;
            *desc = false;
        }
    }
}

/// 目录内排序：文件夹恒在前；组内按所选键与方向。
fn scratchpad_sort_entries(entries: &mut [ScratchpadEntry], sort: ScratchpadSort, desc: bool) {
    entries.sort_by(|a, b| {
        let a_folder = a.kind == ScratchpadEntryKind::Folder;
        let b_folder = b.kind == ScratchpadEntryKind::Folder;
        if a_folder != b_folder {
            return if a_folder {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        let ord = match sort {
            ScratchpadSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            ScratchpadSort::Size => a.size.cmp(&b.size),
            ScratchpadSort::Modified => a.modified_at.cmp(&b.modified_at),
        };
        if desc { ord.reverse() } else { ord }
    });
}

/// 草稿箱面板视图状态。
///
/// 数据来自 `rds-scratchpad` 存储；根 = 当前项目会话下的模块目录 `{project}/scratchpad/`。
/// 闭环：新建/重命名/删除→回收站+撤销/回收站恢复与清空/文件名过滤/引用移除/懒加载/排序。
#[derive(Default)]
struct ScratchpadView {
    /// 是否已尝试加载（首次渲染触发一次）。
    loaded: bool,
    /// 是否正在后台加载（状态行显示「加载中…」，并抑制「草稿箱是空的」闪现）。
    loading: bool,
    /// 最近一次模块根加载的请求序号（丢弃过期结果）。
    load_seq: u64,
    /// 加载错误（未打开项目 / 运行时或存储错误）。
    error: Option<String>,
    /// 模块根的直接子条目（懒加载：展开时按需拉取子目录）。
    entries: Vec<ScratchpadEntry>,
    /// 已懒加载的子目录（父路径 → 直接子条目）。
    children: HashMap<String, Vec<ScratchpadEntry>>,
    /// 排序键与方向（`sort_desc = false` 为升序）。
    sort: ScratchpadSort,
    sort_desc: bool,
    /// 外部引用（来自草稿箱配置，含路径可用性）。
    external_refs: Vec<ExternalReferenceStatus>,
    /// 项目级回收站条目。
    trash: Vec<TrashEntry>,
    /// 回收站分组是否展开。
    trash_expanded: bool,
    /// 已展开的文件夹路径集合。
    expanded: HashSet<String>,
    /// 当前选中条目路径集合（多选）。
    selected: HashSet<String>,
    /// Shift 范围选择的锚点。
    anchor: Option<String>,
    /// 剪切/复制剪贴板。
    clipboard: Option<ScratchpadClipboard>,
    /// 内联编辑状态（新建/重命名）。
    edit: Option<ScratchpadEdit>,
    /// 内联新建的目标目录（相对模块根；空串 = 模块根）。
    new_target: String,
    /// 新建文件使用的模板（跨次新建记忆）。
    new_template: ScratchpadTemplate,
    /// 删除撤销栏。
    undo: Option<ScratchpadUndo>,
    /// 内联名称输入（懒创建）。
    name_input: Option<Entity<InputState>>,
    _name_sub: Option<Subscription>,
    /// 搜索模式与开关（内容模式才有正则 / 大小写）。
    search_mode: ScratchpadSearchMode,
    search_regex: bool,
    search_case: bool,
    /// 搜索输入（懒创建）。
    search_input: Option<Entity<InputState>>,
    _search_sub: Option<Subscription>,
    /// 草稿树的滚动句柄（虚拟列表内部滚动；键盘导航可滚到选中项）。
    list_scroll: ScratchpadListScroll,
}

/// 草稿树滚动句柄包装（`VirtualListScrollHandle` 无 `Default`，此处补一个）。
#[derive(Clone)]
struct ScratchpadListScroll(VirtualListScrollHandle);

impl Default for ScratchpadListScroll {
    fn default() -> Self {
        Self(VirtualListScrollHandle::new())
    }
}

impl ScratchpadListScroll {
    fn handle(&self) -> &VirtualListScrollHandle {
        &self.0
    }
}

/// 草稿树行取色（一次取好，虚拟列表闭包内不再访问 theme）。
#[derive(Clone, Copy)]
struct ScratchpadRowColors {
    hover_bg: Hsla,
    selected_bg: Hsla,
    fg: Hsla,
    muted: Hsla,
    folder_color: Hsla,
    primary: Hsla,
    info: Hsla,
    success: Hsla,
    active_border: Hsla,
}

/// 草稿树行渲染所需的快照（虚拟列表闭包内使用，避免每行重读 RefCell）。
#[derive(Clone)]
struct ScratchpadRowCtx {
    /// 压平后的可见行（缩进层级 + 条目）。
    rows: Rc<Vec<(usize, ScratchpadEntry)>>,
    /// 可见行的条目路径（Shift 范围选择按此顺序）。
    keys: Rc<Vec<String>>,
    /// 进行中的行内编辑（重命名行改为渲染输入框）。
    edit: Option<ScratchpadEdit>,
    /// 内联新建行的插入位置（显示序号, 缩进层级）——`None` 表示无内联新建。
    edit_insert: Option<(usize, usize)>,
    selected: HashSet<String>,
    expanded: HashSet<String>,
    loaded: HashMap<String, Vec<ScratchpadEntry>>,
    colors: ScratchpadRowColors,
}

impl ScratchpadRowCtx {
    /// 该显示行是否为内联新建行。
    fn is_edit_row(&self, display: usize) -> bool {
        matches!(self.edit_insert, Some((i, _)) if i == display)
    }

    /// 内联新建行的缩进层级（仅当该行是新建行时有意义）。
    fn edit_row_depth(&self, display: usize) -> usize {
        match self.edit_insert {
            Some((i, depth)) if i == display => depth,
            _ => 0,
        }
    }

    /// 显示序号 → 真实行序号（内联新建行不占真实行）。
    fn real_index(&self, display: usize) -> Option<usize> {
        if self.is_edit_row(display) {
            return None;
        }
        match self.edit_insert {
            Some((i, _)) if display > i => Some(display - 1),
            _ => Some(display),
        }
    }
}

/// 条目是否命中过滤（自身命中，或已加载子树命中）。
fn scratchpad_entry_matches(
    entry: &ScratchpadEntry,
    filter: &str,
    loaded: &HashMap<String, Vec<ScratchpadEntry>>,
) -> bool {
    if filter.is_empty() {
        return true;
    }
    if entry.name.to_lowercase().contains(filter) {
        return true;
    }
    let key = entry.path.to_string_lossy().to_string();
    let kids = loaded.get(&key).or(entry.children.as_ref());
    kids.map(|kids| {
        kids.iter()
            .any(|c| scratchpad_entry_matches(c, filter, loaded))
    })
    .unwrap_or(false)
}

/// 将条目树按展开状态压平成 `(缩进层级, 条目)` 行序列。
///
/// 子目录优先取懒加载缓存 `loaded`，无缓存时用 `entry.children`；每层按排序设置排列。
/// 有过滤时自动展开命中子树。
#[allow(clippy::too_many_arguments)]
fn flatten_scratchpad(
    entries: &[ScratchpadEntry],
    depth: usize,
    expanded: &HashSet<String>,
    loaded: &HashMap<String, Vec<ScratchpadEntry>>,
    sort: ScratchpadSort,
    sort_desc: bool,
    filter: &str,
    out: &mut Vec<(usize, ScratchpadEntry)>,
) {
    let mut sorted: Vec<ScratchpadEntry> = entries.to_vec();
    scratchpad_sort_entries(&mut sorted, sort, sort_desc);
    for entry in &sorted {
        if !scratchpad_entry_matches(entry, filter, loaded) {
            continue;
        }
        let key = entry.path.to_string_lossy().to_string();
        let is_folder = matches!(entry.kind, ScratchpadEntryKind::Folder);
        out.push((depth, entry.clone()));
        let open = !filter.is_empty() || expanded.contains(&key);
        if is_folder && open {
            let kids = loaded.get(&key).cloned().or_else(|| entry.children.clone());
            if let Some(children) = kids {
                flatten_scratchpad(
                    &children,
                    depth + 1,
                    expanded,
                    loaded,
                    sort,
                    sort_desc,
                    filter,
                    out,
                );
            }
        }
    }
}

/// 取路径末段文件名。
fn scratchpad_basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// 行尾元信息：相对时间（< 7 天）或日期；文件附加可读大小。
fn scratchpad_meta_label(entry: &ScratchpadEntry) -> String {
    let time = entry
        .modified_at
        .as_deref()
        .and_then(scratchpad_relative_time)
        .unwrap_or_default();
    if entry.kind == ScratchpadEntryKind::File {
        let size = scratchpad_size_label(entry.size);
        if time.is_empty() {
            size
        } else {
            format!("{size} · {time}")
        }
    } else {
        time
    }
}

/// RFC3339 → 相对时间（刚刚 / N 分钟 / N 小时 / N 天 / 日期）。
fn scratchpad_relative_time(rfc3339: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let seconds = chrono::Utc::now()
        .signed_duration_since(parsed.with_timezone(&chrono::Utc))
        .num_seconds();
    Some(if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3600)
    } else if seconds < 86_400 * 7 {
        format!("{} 天前", seconds / 86_400)
    } else {
        parsed.format("%Y-%m-%d").to_string()
    })
}

/// 字节数 → 可读大小（B / KB / MB / GB）。
fn scratchpad_size_label(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let s = size as f64;
    if s < KB {
        format!("{size} B")
    } else if s < MB {
        format!("{:.1} KB", s / KB)
    } else if s < GB {
        format!("{:.1} MB", s / MB)
    } else {
        format!("{:.1} GB", s / GB)
    }
}

/// 运行草稿箱内容搜索并构建结果视图。
///
/// 侧栏（发起搜索）与中央编辑区（替换后刷新结果）共用，保证两侧开关语义一致。
fn run_scratchpad_search(
    store: &ScratchpadStore,
    rt: &tokio::runtime::Runtime,
    query: &str,
    case_sensitive: bool,
    is_regex: bool,
) -> Result<ScratchpadSearchView, String> {
    let res = rt
        .block_on(store.search_file_content(query, case_sensitive, 2, is_regex))
        .map_err(|e| e.to_string())?;
    Ok(ScratchpadSearchView {
        query: query.to_string(),
        is_regex,
        case_sensitive,
        scanned: res.total_files_scanned,
        truncated: res.truncated,
        hits: res
            .matches
            .into_iter()
            .map(|m| ScratchpadSearchHit {
                file: m.file,
                line: m.line_number,
                content: m.line_content,
                spans: m.match_spans,
                before: m.before_context,
                after: m.after_context,
            })
            .collect(),
    })
}

/// 内容搜索结果面板（中央编辑区）：头部（查询/命中数/开关标记）+ 命中列表（含上下文）。
fn render_scratchpad_search_pane(
    search: &ScratchpadSearchView,
    theme: &gpui_kit::component::Theme,
    match_bg: Hsla,
    replace_input: Option<&Entity<InputState>>,
    replace_filled: bool,
    on_clear: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
    on_replace_all: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
) -> Div {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let border = theme.colors.border;
    let bg = theme.colors.background;
    let primary = theme.colors.primary;

    let flags = {
        let mut f = Vec::new();
        if search.is_regex {
            f.push("正则");
        }
        if search.case_sensitive {
            f.push("区分大小写");
        }
        if search.truncated {
            f.push("已截断");
        }
        if f.is_empty() {
            String::new()
        } else {
            format!(" · {}", f.join(" · "))
        }
    };

    let header = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(format!("内容搜索：{}", search.query)),
        )
        .child(div().text_xs().text_color(muted).child(format!(
            "{} 处匹配 · 扫描 {} 个文件{}",
            search.hits.len(),
            search.scanned,
            flags
        )))
        .child(div().flex_1())
        .child(
            div()
                .id("sp-search-close")
                .cursor_pointer()
                .text_xs()
                .text_color(primary)
                .child("关闭")
                .on_click(on_clear),
        );

    let mut list = div().v_flex().w_full().gap_1();
    if search.hits.is_empty() {
        list = list.child(div().text_xs().text_color(muted).child("无匹配"));
    }
    for hit in &search.hits {
        let mut group = div().v_flex().w_full().gap_0p5().child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(format!("{} · 行 {}", hit.file, hit.line)),
        );
        for line in &hit.before {
            group = group.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("    {line}")),
            );
        }
        group = group.child(
            div()
                .h_flex()
                .items_center()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .gap_0p5()
                .child(div().flex_none().text_xs().text_color(muted).child(">"))
                .child(scratchpad_hit_line(&hit.content, &hit.spans, match_bg, fg)),
        );
        for line in &hit.after {
            group = group.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("    {line}")),
            );
        }
        list = list.child(group);
    }

    // 替换栏（原型 §4.5）：预览计数 + 全部替换；无替换内容时按钮禁用。
    let unique_files = {
        let mut files: Vec<&str> = search.hits.iter().map(|h| h.file.as_str()).collect();
        files.sort_unstable();
        files.dedup();
        files.len()
    };
    let mut replace_row = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(div().flex_none().text_xs().text_color(muted).child("替换为"));
    if let Some(input) = replace_input {
        replace_row = replace_row.child(div().flex_1().min_w_0().child(Input::new(input)));
    } else {
        replace_row = replace_row.child(div().flex_1());
    }
    replace_row = replace_row
        .child(
            div().flex_none().text_xs().text_color(muted).child(format!(
                "将替换 {} 处 · {} 个文件",
                search.hits.len(),
                unique_files
            )),
        )
        .child(
            Button::new("sp-search-replace")
                .small()
                .label("全部替换")
                .disabled(!replace_filled)
                .on_click(on_replace_all),
        );

    div()
        .v_flex()
        .w_full()
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(bg)
        .child(header)
        .child(replace_row)
        .child(div().max_h(rems(16.)).overflow_hidden().child(list))
}

/// 命中高亮行：按字节区间把 `content` 切成「普通段 + 命中段」，命中段用命中底色。
///
/// 区间非法（越界 / 非 char 边界 / 重叠）时退化为整体纯文本。
fn scratchpad_hit_line(
    content: &str,
    spans: &[(usize, usize)],
    match_bg: Hsla,
    fg: Hsla,
) -> Div {
    let plain = || {
        div()
            .min_w_0()
            .text_xs()
            .text_color(fg)
            .child(content.to_string())
    };
    if spans.is_empty() {
        return plain();
    }
    let mut row = div().h_flex().items_center().min_w_0().text_xs().text_color(fg);
    let mut cursor = 0usize;
    for &(start, end) in spans {
        if start < cursor
            || end < start
            || end > content.len()
            || !content.is_char_boundary(start)
            || !content.is_char_boundary(end)
        {
            return plain();
        }
        if start > cursor {
            row = row.child(div().flex_none().child(content[cursor..start].to_string()));
        }
        row = row.child(
            div()
                .flex_none()
                .rounded_sm()
                .bg(match_bg)
                .child(content[start..end].to_string()),
        );
        cursor = end;
    }
    if cursor < content.len() {
        row = row.child(div().flex_none().child(content[cursor..].to_string()));
    }
    row
}

/// 拆分文件名与扩展名（`foo.sql` → (`foo`, `.sql`)）。
fn scratchpad_split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// 拼接模块内相对路径（父目录为空串时退化为子项名）。
fn join_scratchpad_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent.trim_end_matches(['/', '\\']), name)
    }
}

/// 复制条目（文件或文件夹，递归）到目标目录。
///
/// 实现在 `rds-scratchpad::ScratchpadStore::copy_entry`（重名避让 / 二进制安全 / 拒绝复制到自身内部）。
async fn copy_scratchpad_entry(
    store: &ScratchpadStore,
    rel_path: &str,
    target_parent: &str,
) -> Result<(), String> {
    store
        .copy_entry(rel_path, target_parent)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

impl ScratchpadView {
    /// 在已加载数据（根 + 懒加载子目录）中查找条目类型。
    fn kind_of(&self, key: &str) -> Option<ScratchpadEntryKind> {
        fn walk(entries: &[ScratchpadEntry], key: &str) -> Option<ScratchpadEntryKind> {
            for e in entries {
                if e.path.to_string_lossy() == key {
                    return Some(e.kind.clone());
                }
                if let Some(kids) = &e.children {
                    if let Some(k) = walk(kids, key) {
                        return Some(k);
                    }
                }
            }
            None
        }
        if let Some(k) = walk(&self.entries, key) {
            return Some(k);
        }
        for kids in self.children.values() {
            if let Some(k) = walk(kids, key) {
                return Some(k);
            }
        }
        None
    }
}

/// 扩展名 → 图标点色（复用主题标准色，代码零裸色）。
///
/// 颜色以参数传入（局部拷贝），避免在渲染函数里长期持有 `theme` 借用，
/// 便于后续调用 `&mut cx` 方法。
fn scratchpad_icon_color(
    name: &str,
    info: Hsla,
    success: Hsla,
    primary: Hsla,
    muted: Hsla,
) -> Hsla {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "sql" => info,
        "py" => success,
        "csv" | "tsv" | "parquet" | "xlsx" | "xls" | "json" | "ndjson" | "db" | "duckdb" => primary,
        _ => muted,
    }
}

/// 数据库导航面板状态（M4）。
///
/// 数据来自 `Shared::connections`（连接列表）+ `NavigatorService`（对象树懒加载）。
/// 来源标签页、展开态、加载结果与错误就地保存在此，重启持久化在后续切片接入。
#[derive(Default)]
struct DatabaseNavView {
    /// 来源筛选（None = 全部）。
    source_filter: Option<NavSource>,
    /// 搜索过滤词（本地筛选）。
    filter: String,
    /// 已展开节点 key。
    expanded: HashSet<String>,
    /// 父节点 key → 已加载子节点。
    children: HashMap<String, Vec<NavNode>>,
    /// 已尝试加载的节点 key（避免重复请求）。
    attempted: HashSet<String>,
    /// 正在后台加载的节点 key（渲染「加载中…」占位）。
    loading: HashSet<String>,
    /// 节点 key → 加载失败原因。
    errors: HashMap<String, String>,
    /// 本会话运行时已连接的连接 ID。
    connected: HashSet<String>,
    /// 已从库中恢复过导航状态的连接 ID。
    state_loaded: HashSet<String>,
    /// 自定义分组（项目级）。
    groups: Vec<engine::persistence::ConnectionGroup>,
    /// 连接 ID → 所属分组 ID 列表（多对多）。
    membership: HashMap<String, Vec<String>>,
    /// 分组 ID → 组内连接 ID（按手动排序优先，未排按连接 ID）。
    group_order: HashMap<String, Vec<String>>,
    /// 连接 ID → **显式主组** ID（仅用户显式指定过的连接；缺省回退到分组排序推导）。
    primary_group: HashMap<String, String>,
    /// 类别文件夹节点 key → 已渲染条数上限（大 schema 客户端分页）。
    page_limit: HashMap<String, usize>,
    /// 连接 ID → 标签列表（一次读库缓存，避免渲染期逐条查询）。
    tags: HashMap<String, Vec<String>>,
    /// 分组是否已加载。
    groups_loaded: bool,
    /// 折叠的自定义分组 ID（缺省展开）。
    collapsed_groups: HashSet<String>,
    /// 正在内联编辑分组（归组）的连接 ID（None = 未打开）。
    group_picker_for: Option<String>,
    /// 正在内联编辑**标签**的连接 ID（None = 未打开；`+` 专用）。
    tag_editor_for: Option<String>,
    /// 正在内联「复制为模板」的连接 ID（None = 未打开）。
    copy_for: Option<String>,
    /// 正在内联重命名的分组 ID（None = 未打开）。
    group_rename_for: Option<String>,
    /// 已发起列预取（C2）的类别文件夹 key（避免重复排队）。
    prefetched: HashSet<String>,
    /// 当前选中的节点 key（键盘导航与选中高亮）。
    selected_key: Option<String>,
    /// 附加 facet 筛选：类型（`drivers.type_id`）。
    type_filter: Option<String>,
    /// 附加 facet 筛选：驱动 id。
    driver_filter: Option<String>,
    /// 附加 facet 筛选：标签。
    tag_filter: Option<String>,
    /// 搜索框 facet 语法解析结果（每帧重算，不持久化）。
    search_facets: NavSearchFacets,
}

/// 附加 facet 种类（facet 弹层与持久化共用）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum NavFacet {
    /// 数据库类型（`drivers.type_id`）。
    Type,
    /// 驱动 id。
    Driver,
    /// 标签。
    Tag,
}

/// 搜索框 facet 语法解析结果（`scope:` / `type:` / `driver:` / `tag:`）。
///
/// 搜索中的 facet token 作为**额外约束**与面板 chips **叠加（AND）**，不写回 chips；
/// 这样避免“输入框回写 → 重新解析”的反馈环与光标跳动。
#[derive(Default, Clone)]
struct NavSearchFacets {
    /// 自由文本（去除 facet token 后的剩余）。
    free: String,
    /// 搜索是否包含 facet token（用于计数）。
    active: usize,
    /// `scope:` / `source:`（`source` 为历史别名）。
    source: Option<NavSource>,
    /// `type:`。
    db_type: Option<String>,
    /// `driver:`。
    driver: Option<String>,
    /// `tag:`。
    tag: Option<String>,
}

/// 解析搜索框文本：拆出 `scope:` / `type:` / `driver:` / `tag:` token，其余为自由文本。
///
/// 值不引号包裹（含空格需自行避免）；`scope` / `source` 支持全名与短码。
/// 无法识别的 token（如空值）原样保留在自由文本里，避免“输入中丢字”。
fn parse_nav_search(raw: &str) -> NavSearchFacets {
    let mut out = NavSearchFacets::default();
    let mut free: Vec<&str> = Vec::new();
    for tok in raw.split_whitespace() {
        let Some((key, value)) = tok.split_once(':') else {
            free.push(tok);
            continue;
        };
        if value.is_empty() {
            free.push(tok);
            continue;
        }
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "scope" | "source" => match NavSource::from_key(value) {
                Some(s) => {
                    out.source = Some(s);
                    out.active += 1;
                }
                None => free.push(tok),
            },
            "type" => {
                out.db_type = Some(value.to_string());
                out.active += 1;
            }
            "driver" => {
                out.driver = Some(value.to_string());
                out.active += 1;
            }
            "tag" => {
                out.tag = Some(value.to_string());
                out.active += 1;
            }
            _ => free.push(tok),
        }
    }
    out.free = free.join(" ");
    out
}

/// 渲染顺序中的可见项（键盘导航用；每帧重建）。
#[derive(Clone)]
struct NavOrderItem {
    key: String,
    conn_id: String,
    path: Option<NavPath>,
    property: Option<PropertyRef>,
    has_children: bool,
    expanded: bool,
}

/// 「未分组」固定分组的伪 ID（收纳不属于任何自定义分组的连接）。
const GROUP_UNGROUPED: &str = "__ungrouped__";

/// 连接行徽标状态（颜色通道；见原型设计 §2.3）。
#[derive(Clone, Copy)]
enum NavBadgeStatus {
    /// 已连接（`success`）。
    Connected,
    /// 建连 / 预热中（`info`）。
    Connecting,
    /// 最近一次连接失败（`danger`）。
    Failed,
    /// 未连接（`muted`，灰）。
    Idle,
}

impl NavBadgeStatus {
    fn color(self, theme: &gpui_kit::component::Theme) -> Hsla {
        match self {
            Self::Connected => theme.colors.success,
            Self::Connecting => theme.colors.info,
            Self::Failed => theme.colors.danger,
            Self::Idle => theme.colors.muted_foreground,
        }
    }

    /// 状态文案（徽标 hover 卡用）。
    fn label(self) -> &'static str {
        match self {
            Self::Connected => "已连接",
            Self::Connecting => "连接中",
            Self::Failed => "连接失败",
            Self::Idle => "未连接",
        }
    }
}

/// 徽标 hover 卡：类型 / 状态 / 驱动（gpui-kit 0.6.1 无通用 `.tooltip` 扩展，故用 `HoverCard`）。
fn nav_badge_hover_card(
    id: SharedString,
    trigger: impl IntoElement + 'static,
    type_label: String,
    status_label: &'static str,
    driver_label: String,
) -> impl IntoElement {
    use gpui_kit::component::hover_card::HoverCard;
    HoverCard::new(id)
        .open_delay(std::time::Duration::from_millis(300))
        .trigger(trigger)
        .content(move |_, _window, cx| {
            let fg = cx.theme().colors.foreground;
            let muted = cx.theme().colors.muted_foreground;
            let type_label = type_label.clone();
            let driver_label = driver_label.clone();
            div()
                .v_flex()
                .gap(rems(0.125))
                .text_xs()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(type_label),
                )
                .child(
                    div()
                        .text_color(muted)
                        .child(format!("状态：{status_label}")),
                )
                .child(
                    div()
                        .text_color(muted)
                        .child(format!("驱动：{driver_label}")),
                )
        })
}

/// 类型文案（徽标 hover 卡用）：已知类型给出「名称（分类）」，否则回退类型 id。
fn nav_type_label(type_id: &str) -> String {
    let known = match type_id {
        "postgresql" => "PostgreSQL（关系型）",
        "mysql" => "MySQL（关系型）",
        "mariadb" => "MariaDB（关系型）",
        "mssql" => "SQL Server（关系型）",
        "oracle" => "Oracle（关系型）",
        "sqlite" => "SQLite（文件型）",
        "duckdb" => "DuckDB（分析型）",
        "clickhouse" => "ClickHouse（分析型）",
        "mongodb" => "MongoDB（文档型）",
        "redis" => "Redis（键值型）",
        _ => "",
    };
    if known.is_empty() {
        type_id.to_string()
    } else {
        known.to_string()
    }
}

/// 类型短名（去掉「（关系型）」等分类后缀），facet 菜单用。
fn nav_type_short_label(type_id: &str) -> String {
    let full = nav_type_label(type_id);
    full.split('（').next().unwrap_or(&full).to_string()
}

/// 类型徽标映射：数据库类型 id →（形状资产路径，2 字母缩写）。
///
/// 形状取自 `gpui-kit-assets` 全量 Lucide（`AllAssets` 已注册，无需新增资产）；
/// **字母是权威识别，形状是冗余强化 + 扫视加速**（原型设计 §2.3）。
/// 目录外类型回退通用形状 + 类型名首 2 字母。
fn nav_type_badge(type_id: &str) -> (&'static str, String) {
    let (path, letters): (&'static str, &'static str) = match type_id {
        "postgresql" => ("icons/database.svg", "PG"),
        "mysql" => ("icons/cylinder.svg", "MY"),
        "mariadb" => ("icons/coins.svg", "MA"),
        "sqlite" => ("icons/file.svg", "SQ"),
        "duckdb" => ("icons/layers.svg", "DK"),
        "mssql" => ("icons/server.svg", "MS"),
        "oracle" => ("icons/hexagon.svg", "OR"),
        "clickhouse" => ("icons/chart-column.svg", "CH"),
        "mongodb" => ("icons/leaf.svg", "MG"),
        "redis" => ("icons/braces.svg", "RD"),
        _ => {
            let upper = type_id.to_uppercase();
            let short: String = upper.chars().take(2).collect();
            return (
                "icons/database.svg",
                if short.is_empty() { "DB".into() } else { short },
            );
        }
    };
    (path, letters.to_string())
}

/// 节点图标色（复用主题标准色，代码零裸色）。
fn nav_kind_color(kind: &NavNodeKind, theme: &gpui_kit::component::Theme) -> Hsla {
    match kind {
        NavNodeKind::Connection { .. } => theme.colors.primary,
        NavNodeKind::Catalog | NavNodeKind::Schema | NavNodeKind::Folder(_) => theme.colors.warning,
        NavNodeKind::Table { .. } => theme.colors.info,
        NavNodeKind::View => theme.colors.success,
        NavNodeKind::Routine { .. } => theme.colors.primary,
        _ => theme.colors.muted_foreground,
    }
}

/// 限定名（`catalog.schema.name`，跳过空段）：用于复制与生成 SELECT。
///
/// 无独立 Schema 层的驱动（MySQL / SQLite / DuckDB）导航把 schema 传成 catalog，
/// 相等时只留一份，避免出现 `db.db.name`。
fn nav_qualified_name(prop: &PropertyRef) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(c) = prop.catalog.as_deref() {
        if !c.is_empty() {
            parts.push(c);
        }
    }
    if let Some(s) = prop.schema.as_deref() {
        if !s.is_empty() && Some(s) != prop.catalog.as_deref() {
            parts.push(s);
        }
    }
    parts.push(prop.name.as_str());
    parts.join(".")
}

/// 搜索过滤：节点名命中，或已加载子节点中任一命中。
fn nav_node_matches(
    children: &HashMap<String, Vec<NavNode>>,
    node: &NavNode,
    filter: &str,
) -> bool {
    if node.name.to_lowercase().contains(filter) {
        return true;
    }
    match children.get(&node.key) {
        Some(kids) => kids.iter().any(|c| nav_node_matches(children, c, filter)),
        None => false,
    }
}

/// 搜索命中高亮：把 `name` 中与 `filter`（已小写）匹配的一段用命中底色标出。
///
/// 非 ASCII（如 CJK）大小写转换不改变字节长度，故按字节切片安全；
/// 仅当两端都是 char 边界时才切，否则退化为整体文本。
fn nav_name_highlight(name: &str, filter: &str, match_bg: Hsla, fg: Hsla) -> Div {
    if !filter.is_empty() {
        let lower = name.to_lowercase();
        if lower.len() == name.len() {
            if let Some(pos) = lower.find(filter) {
                let end = pos + filter.len();
                if name.is_char_boundary(pos) && name.is_char_boundary(end) {
                    let before = name[..pos].to_string();
                    let hit = name[pos..end].to_string();
                    let after = name[end..].to_string();
                    return div()
                        .h_flex()
                        .items_center()
                        .min_w_0()
                        .text_color(fg)
                        .child(before)
                        .child(div().rounded_sm().bg(match_bg).child(hit))
                        .child(after);
                }
            }
        }
    }
    div().min_w_0().text_color(fg).child(name.to_string())
}

/// 属性面板请求（导航双击对象时发出，由编辑区右侧面板渲染）。
#[derive(Clone)]
pub struct PropertyRequest {
    pub property: PropertyRef,
    pub conn_label: String,
    pub driver: String,
}

/// 属性面板状态（懒加载一次，按请求 key 失效重载）。
#[derive(Default)]
struct PropertyState {
    loaded_for: Option<String>,
    props: Option<database::property_panel::ObjectProperties>,
    error: Option<String>,
    /// 是否正在后台加载（渲染「加载中…」）。
    loading: bool,
}

impl SidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        // 从 settings.json 恢复 facet 筛选（UI 偏好，跨项目）。
        let saved = settings::SettingsService::nav_filters(cx);
        let mut nav_view = DatabaseNavView::default();
        nav_view.source_filter = saved.source.as_deref().and_then(NavSource::from_key);
        nav_view.type_filter = saved.db_type.clone();
        nav_view.driver_filter = saved.driver.clone();
        nav_view.tag_filter = saved.tag.clone();
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            scratchpad: Rc::new(RefCell::new(ScratchpadView::default())),
            database_nav: Rc::new(RefCell::new(nav_view)),
            nav_search: None,
            _nav_search_sub: None,
            nav_tag_input: None,
            nav_tag_input_for: None,
            _nav_tag_sub: None,
            nav_copy_input: None,
            nav_copy_input_for: None,
            _nav_copy_sub: None,
            nav_group_input: None,
            _nav_group_sub: None,
            nav_order: Rc::new(RefCell::new(Vec::new())),
            warm_poll: None,
            nav_pump: RefCell::new(None),
            scratchpad_pump: RefCell::new(None),
        }
    }

    /// 聚焦数据源导航搜索框（Ctrl+F，由宿主 action 调用）。
    ///
    /// 搜索框懒创建：已存在则立即聚焦，否则置位由 `render_database_nav` 消费。
    pub fn focus_nav_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.nav_search.clone() {
            let handle = input.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        } else {
            self.shared.focus_nav_search.set(true);
        }
        cx.notify();
    }

    /// 键盘导航：按渲染顺序移动选中项（`delta` 为 ±1）。
    fn nav_move(&self, delta: isize, cx: &mut Context<Self>) {
        let order = self.nav_order.borrow();
        if order.is_empty() {
            return;
        }
        let current = self.database_nav.borrow().selected_key.clone();
        let idx = current.and_then(|k| order.iter().position(|i| i.key == k));
        let next = match idx {
            None => 0,
            Some(i) => (i as isize + delta).clamp(0, order.len() as isize - 1) as usize,
        };
        let key = order[next].key.clone();
        drop(order);
        self.database_nav.borrow_mut().selected_key = Some(key);
        cx.notify();
    }

    /// 当前选中项（克隆，避免跨借用）。
    fn nav_selected(&self) -> Option<NavOrderItem> {
        let key = self.database_nav.borrow().selected_key.clone()?;
        self.nav_order
            .borrow()
            .iter()
            .find(|i| i.key == key)
            .cloned()
    }

    /// 当前选中的连接 id（仅当选中节点是连接根时）。
    ///
    /// 面板头「⟳ 刷新元数据」「断开当前连接」的作用目标（设计 §2.1 / §4.2）。
    fn nav_current_connection(&self) -> Option<String> {
        self.nav_selected()
            .filter(|item| matches!(item.path, Some(NavPath::Connection)))
            .map(|item| item.conn_id)
    }

    /// →：展开选中节点（有子节点且尚未展开）。
    fn nav_expand(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.nav_selected() else {
            return;
        };
        if item.has_children && !item.expanded {
            if let Some(path) = item.path {
                self.toggle_nav_node(&item.conn_id, &item.key, path, cx);
                cx.notify();
            }
        }
    }

    /// ←：折叠选中节点（已展开）。
    fn nav_collapse(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.nav_selected() else {
            return;
        };
        if item.has_children && item.expanded {
            if let Some(path) = item.path {
                self.toggle_nav_node(&item.conn_id, &item.key, path, cx);
                cx.notify();
            }
        }
    }

    /// Enter / F4：打开选中节点的属性面板（无属性定位时为无操作）。
    fn nav_open_properties(&self, cx: &mut Context<Self>) {
        let Some(item) = self.nav_selected() else {
            return;
        };
        let Some(property) = item.property else {
            return;
        };
        let (conn_label, driver) = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|c| c.id == item.conn_id)
            .map(|c| (c.name.clone(), c.driver.clone()))
            .unwrap_or_else(|| (item.conn_id.clone(), String::new()));
        *self.shared.property_target.borrow_mut() = Some(PropertyRequest {
            property,
            conn_label,
            driver,
        });
        cx.notify();
    }

    /// 请求重载草稿箱（渲染与事件路径共用）：**只入队 + 起轮询，不做 I/O**。
    ///
    /// 实际读盘在 `scratchpad_jobs` 的工作线程；结果由 [`Self::apply_scratchpad_loads`] 回填。
    /// 重载时保留已展开子目录（在后台重新拉取），避免「操作后展开态看起来空了」。
    fn request_scratchpad_load(&self, cx: &mut Context<Self>) {
        let Some(root) = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.root.clone())
        else {
            let mut view = self.scratchpad.borrow_mut();
            view.loaded = true;
            view.loading = false;
            // 失效在途结果：项目已关闭，旧项目的加载结果不得回填。
            view.load_seq = scratchpad_jobs::invalidate_loads();
            view.error = Some("未打开项目：草稿箱根即项目目录，请先打开项目。".to_string());
            view.entries.clear();
            view.children.clear();
            view.external_refs.clear();
            view.trash.clear();
            return;
        };
        let parents: Vec<String> = self.scratchpad.borrow().children.keys().cloned().collect();
        let seq = scratchpad_jobs::enqueue_root_load(&root, parents);
        {
            let mut view = self.scratchpad.borrow_mut();
            // `loaded` = 已受理本次请求（防渲染帧重复入队）；加载中状态另行标记。
            view.loaded = true;
            view.loading = true;
            view.load_seq = seq;
            view.error = None;
        }
        self.ensure_scratchpad_pump(cx);
    }

    /// 启动草稿箱加载结果轮询（已有存活任务时不重复启动）。
    fn ensure_scratchpad_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.scratchpad_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let loads = scratchpad_jobs::drain_loads();
                if !loads.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_scratchpad_loads(loads, cx))
                        .is_err()
                {
                    return;
                }
                let dirs = scratchpad_jobs::drain_dirs();
                if !dirs.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_scratchpad_dirs(dirs, cx))
                        .is_err()
                {
                    return;
                }
                if !scratchpad_jobs::has_pending() {
                    // 多等一拍确认没有新任务（render 可能刚入队）。
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !scratchpad_jobs::has_pending() {
                        break;
                    }
                }
            }
        });
        *self.scratchpad_pump.borrow_mut() = Some(task);
    }

    /// 回填模块根加载结果（主线程）：丢弃过期序号，写入条目/引用/回收站/子目录缓存。
    fn apply_scratchpad_loads(
        &mut self,
        results: Vec<scratchpad_jobs::LoadResult>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        {
            let mut view = self.scratchpad.borrow_mut();
            // 同一帧可能收到多份（连续重载）：只应用最新序号。
            let newest = results.iter().map(|r| r.seq).max().unwrap_or(0);
            if newest < view.load_seq {
                return;
            }
            for r in results.into_iter().filter(|r| r.seq == newest) {
                match r.entries {
                    Ok(entries) => {
                        view.entries = entries;
                        view.external_refs = r.refs;
                        view.trash = r.trash;
                        view.children = r.children.into_iter().collect();
                        view.error = None;
                    }
                    Err(e) => {
                        view.error = Some(format!("加载草稿箱失败: {e}"));
                    }
                }
                changed = true;
            }
            if changed {
                // 已应用最新序号（更晚的请求会走上面的 early return），加载态结束。
                view.loading = false;
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// 回填子目录懒加载结果（主线程）。
    fn apply_scratchpad_dirs(
        &mut self,
        results: Vec<scratchpad_jobs::DirResult>,
        cx: &mut Context<Self>,
    ) {
        {
            let mut view = self.scratchpad.borrow_mut();
            for r in results {
                match r.result {
                    Ok(kids) => {
                        view.children.insert(r.parent, kids);
                    }
                    Err(e) => view.error = Some(format!("展开失败: {e}")),
                }
            }
        }
        cx.notify();
    }

    /// 构建草稿箱存储 + 运行时（未打开项目时报错）。
    fn scratchpad_store(&self) -> Result<(ScratchpadStore, tokio::runtime::Runtime), String> {
        self.shared.scratchpad_store()
    }

    /// 懒创建草稿箱输入框（名称内联编辑 + 文件名过滤）并订阅事件。
    fn ensure_scratchpad_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scratchpad.borrow().name_input.is_some() {
            return;
        }
        let name_input = cx.new(|cx| InputState::new(window, cx));
        let name_sub = cx.subscribe_in(
            &name_input,
            window,
            |this, _e, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.commit_scratchpad_edit(window, cx);
                }
            },
        );
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索…"));
        let search_sub = cx.subscribe_in(
            &search_input,
            window,
            |this, _e, ev: &InputEvent, _w, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    let content_mode =
                        this.scratchpad.borrow().search_mode == ScratchpadSearchMode::Content;
                    if content_mode {
                        this.run_scratchpad_content_search(cx);
                    } else {
                        cx.notify();
                    }
                }
                _ => {}
            },
        );
        let mut view = self.scratchpad.borrow_mut();
        view.name_input = Some(name_input);
        view._name_sub = Some(name_sub);
        view.search_input = Some(search_input);
        view._search_sub = Some(search_sub);
    }

    /// 开始内联编辑（新建 / 重命名）。
    fn start_scratchpad_edit(
        &mut self,
        edit: ScratchpadEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // M1：只读打开时禁止新建/重命名草稿。
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许修改草稿".to_string());
            cx.notify();
            return;
        }
        self.ensure_scratchpad_inputs(window, cx);
        // 新建落点：唯一选中且为文件夹 → 该目录（并展开，使内联行可见）；否则模块根。
        if matches!(edit, ScratchpadEdit::NewFile | ScratchpadEdit::NewFolder) {
            let target = self.scratchpad_paste_target();
            let mut view = self.scratchpad.borrow_mut();
            view.new_target = target.clone();
            if !target.is_empty() {
                view.expanded.insert(target);
            }
        }
        let initial = match &edit {
            ScratchpadEdit::Rename { path } => std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            ScratchpadEdit::NewReference { path } => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            ScratchpadEdit::RenameReference { alias } => alias.clone(),
            _ => String::new(),
        };
        self.scratchpad.borrow_mut().edit = Some(edit);
        if let Some(input) = self.scratchpad.borrow().name_input.clone() {
            input.update(cx, |s, cx| s.set_value(initial, window, cx));
            let handle = input.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        cx.notify();
    }

    /// 取消内联编辑。
    fn cancel_scratchpad_edit(&mut self, cx: &mut Context<Self>) {
        self.scratchpad.borrow_mut().edit = None;
        cx.notify();
    }

    /// 重命名当前唯一选中的条目（F2）。
    fn rename_scratchpad_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = {
            let view = self.scratchpad.borrow();
            if view.selected.len() == 1 {
                view.selected.iter().next().cloned()
            } else {
                None
            }
        };
        if let Some(path) = path {
            self.start_scratchpad_edit(ScratchpadEdit::Rename { path }, window, cx);
        }
    }

    /// 提交内联编辑（新建 / 重命名）。
    fn commit_scratchpad_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.scratchpad.borrow_mut().edit.take() else {
            return;
        };
        // M1：只读打开时禁止提交草稿修改。
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许修改草稿".to_string());
            cx.notify();
            return;
        }
        let name = self
            .scratchpad
            .borrow()
            .name_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            cx.notify();
            return;
        }

        let outcome = match self.scratchpad_store() {
            Ok((store, rt)) => {
                let result = match &edit {
                    ScratchpadEdit::NewFile => {
                        // 模板：自动补后缀 + 创建后写入占位内容（空白模板不写）。
                        let (template, target) = {
                            let view = self.scratchpad.borrow();
                            (view.new_template, view.new_target.clone())
                        };
                        let parent = if target.is_empty() { None } else { Some(target.as_str()) };
                        let final_name = scratchpad_apply_template_ext(&name, template);
                        let body = template.content(&final_name);
                        rt.block_on(store.create_entry(&final_name, parent, false))
                            .and_then(|_| {
                                if body.is_empty() {
                                    Ok(())
                                } else {
                                    rt.block_on(store.save_file(
                                        &join_scratchpad_rel(&target, &final_name),
                                        &body,
                                    ))
                                }
                            })
                    }
                    ScratchpadEdit::NewFolder => {
                        let target = self.scratchpad.borrow().new_target.clone();
                        let parent = if target.is_empty() { None } else { Some(target.as_str()) };
                        rt.block_on(store.create_entry(&name, parent, true))
                            .map(|_| ())
                    }
                    ScratchpadEdit::Rename { path } => rt
                        .block_on(store.rename_entry(path, &name))
                        .map(|_| ()),
                    ScratchpadEdit::NewReference { path } => rt
                        .block_on(store.add_external_reference(name.clone(), path.clone()))
                        .map(|_| ()),
                    ScratchpadEdit::RenameReference { alias } => rt
                        .block_on(store.rename_external_reference(alias, &name)),
                };
                result.map(|_| ()).map_err(|e| e.to_string())
            }
            Err(e) => Err(e),
        };

        let mut view = self.scratchpad.borrow_mut();
        if let Some(input) = view.name_input.clone() {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        match outcome {
            Ok(()) => {
                view.loaded = false;
                view.error = None;
            }
            Err(e) => view.error = Some(format!("操作失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 删除所选条目 → 项目级回收站，并记录撤销（批量）。
    fn delete_scratchpad_selection(&mut self, cx: &mut Context<Self>) {
        // M1：只读打开时禁止删除草稿。
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许删除草稿".to_string());
            cx.notify();
            return;
        }
        let mut paths: Vec<String> = self.scratchpad.borrow().selected.iter().cloned().collect();
        if paths.is_empty() {
            return;
        }
        paths.sort();
        let label = if paths.len() == 1 {
            scratchpad_basename(&paths[0])
        } else {
            format!("{} 项", paths.len())
        };

        let result = (|| -> Result<Vec<String>, String> {
            let (store, rt) = self.scratchpad_store()?;
            let before: HashSet<String> = rt
                .block_on(store.list_trash())
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|e| e.manifest.id)
                .collect();
            for path in &paths {
                rt.block_on(store.delete_entry(path))
                    .map_err(|e| e.to_string())?;
            }
            let after = rt.block_on(store.list_trash()).map_err(|e| e.to_string())?;
            Ok(after
                .into_iter()
                .map(|e| e.manifest.id)
                .filter(|id| !before.contains(id))
                .collect())
        })();

        let mut undo_ids: Option<Vec<String>> = None;
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(trash_ids) => {
                undo_ids = Some(trash_ids.clone());
                view.undo = Some(ScratchpadUndo { label, trash_ids });
                view.selected.clear();
                view.anchor = None;
                view.loaded = false;
                view.error = None;
            }
            Err(e) => view.error = Some(format!("删除失败: {e}")),
        }
        drop(view);
        if let Some(ids) = undo_ids {
            self.schedule_undo_expiry(ids, cx);
        }
        cx.notify();
    }

    /// 删除单条（行内 ✕）：先设为唯一选中，再走批量删除。
    fn delete_scratchpad_entry(&mut self, relative_path: String, cx: &mut Context<Self>) {
        {
            let mut view = self.scratchpad.borrow_mut();
            view.selected.clear();
            view.selected.insert(relative_path.clone());
            view.anchor = Some(relative_path);
        }
        self.delete_scratchpad_selection(cx);
    }

    /// 撤销上一次删除（从回收站还原全部条目）。
    fn undo_scratchpad_delete(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.scratchpad.borrow().undo.clone() else {
            return;
        };
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => {
                let mut failure: Option<String> = None;
                for id in &undo.trash_ids {
                    if let Err(e) = rt.block_on(store.restore_from_trash(id)) {
                        failure = Some(e.to_string());
                    }
                }
                match failure {
                    None => Ok(()),
                    Some(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        view.undo = None;
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("撤销失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 把当前选中项放入剪贴板（剪切 / 复制）。
    fn set_scratchpad_clipboard(&mut self, mode: ScratchpadClipboardMode, cx: &mut Context<Self>) {
        let mut paths: Vec<String> = self.scratchpad.borrow().selected.iter().cloned().collect();
        if paths.is_empty() {
            return;
        }
        paths.sort();
        self.scratchpad.borrow_mut().clipboard = Some(ScratchpadClipboard { mode, paths });
        cx.notify();
    }

    /// 粘贴剪贴板到当前选中的文件夹（未选中文件夹则粘到模块根）。
    fn paste_scratchpad_clipboard(&mut self, cx: &mut Context<Self>) {
        // M1：只读打开时禁止写入草稿。
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许粘贴草稿".to_string());
            cx.notify();
            return;
        }
        let Some(clipboard) = self.scratchpad.borrow().clipboard.clone() else {
            return;
        };
        let target = self.scratchpad_paste_target();

        let result = (|| -> Result<(), String> {
            let (store, rt) = self.scratchpad_store()?;
            for path in &clipboard.paths {
                match clipboard.mode {
                    ScratchpadClipboardMode::Cut => {
                        rt.block_on(store.move_entry(path, &target))
                            .map_err(|e| e.to_string())?;
                    }
                    ScratchpadClipboardMode::Copy => {
                        rt.block_on(copy_scratchpad_entry(&store, path, &target))?;
                    }
                }
            }
            Ok(())
        })();

        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => {
                if clipboard.mode == ScratchpadClipboardMode::Cut {
                    view.clipboard = None;
                }
                view.selected.clear();
                view.anchor = None;
                view.loaded = false;
                view.error = None;
            }
            Err(e) => view.error = Some(format!("粘贴失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 粘贴目标：唯一选中且为文件夹 → 该目录；否则模块根（空串）。
    fn scratchpad_paste_target(&self) -> String {
        let view = self.scratchpad.borrow();
        if view.selected.len() == 1 {
            if let Some(path) = view.selected.iter().next() {
                if view
                    .kind_of(path)
                    .map(|k| k == ScratchpadEntryKind::Folder)
                    .unwrap_or(false)
                {
                    return path.clone();
                }
            }
        }
        String::new()
    }

    /// 从回收站还原指定条目。
    fn restore_scratchpad_trash(&mut self, trash_id: String, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.restore_from_trash(&trash_id))
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("还原失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 清空回收站。
    fn empty_scratchpad_trash(&mut self, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt.block_on(store.empty_trash()).map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => {
                view.loaded = false;
                view.trash_expanded = false;
            }
            Err(e) => view.error = Some(format!("清空回收站失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 移除外部引用。
    fn remove_scratchpad_reference(&mut self, alias: String, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.remove_external_reference(&alias))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("移除引用失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 重新引用（失效引用专用）：选新路径 → 只改路径，别名不变。
    fn relink_scratchpad_reference(
        &mut self,
        alias: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许修改引用".to_string());
            cx.notify();
            return;
        }
        let entity = cx.entity();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("选择引用目标（文件或目录）".into()),
        });
        window
            .spawn(cx, async move |cx| {
                if let Ok(Ok(Some(paths))) = receiver.await {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = cx.update(|_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.apply_scratchpad_relink(alias, path, cx)
                            });
                        });
                    }
                }
            })
            .detach();
    }

    /// 写入新的引用路径（`relink_scratchpad_reference` 选定路径后的落盘步骤）。
    fn apply_scratchpad_relink(
        &mut self,
        alias: String,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.update_external_reference_path(&alias, path))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => view.loaded = false,
            Err(e) => view.error = Some(format!("重新引用失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 懒加载子目录（展开文件夹时调用）：只入队 + 起轮询，结果由 `apply_scratchpad_dirs` 回填。
    fn request_scratchpad_dir(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(root) = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.root.clone())
        else {
            self.scratchpad.borrow_mut().error =
                Some("未打开项目：草稿箱根即项目目录，请先打开项目。".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_dir_load(&root, &path);
        self.ensure_scratchpad_pump(cx);
    }

    /// 导入外部文件到草稿箱（复制进来）。
    fn import_scratchpad_files(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => {
                let mut failure: Option<String> = None;
                for path in &paths {
                    if let Err(e) = rt.block_on(store.import_external_file(path)) {
                        failure = Some(e.to_string());
                    }
                }
                match failure {
                    None => Ok(()),
                    Some(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        let mut view = self.scratchpad.borrow_mut();
        match result {
            Ok(()) => {
                view.loaded = false;
                view.error = None;
            }
            Err(e) => view.error = Some(format!("导入失败: {e}")),
        }
        drop(view);
        cx.notify();
    }

    /// 在系统文件管理器中打开条目（需绝对路径）。
    fn open_scratchpad_location(&mut self, path: String, cx: &mut Context<Self>) {
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.open_in_system_explorer(std::path::Path::new(&path)))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            self.scratchpad.borrow_mut().error = Some(format!("打开位置失败: {e}"));
            cx.notify();
        }
    }

    /// 当前可见行（渲染顺序）的条目路径，供键盘导航与滚动定位。
    fn scratchpad_visible_keys(&self, cx: &App) -> Vec<String> {
        let view = self.scratchpad.borrow();
        let filter = view
            .search_input
            .as_ref()
            .map(|i| i.read(cx).value().to_lowercase().trim().to_string())
            .unwrap_or_default();
        let mut flat = Vec::new();
        flatten_scratchpad(
            &view.entries,
            0,
            &view.expanded,
            &view.children,
            view.sort,
            view.sort_desc,
            &filter,
            &mut flat,
        );
        flat.into_iter()
            .map(|(_, e)| e.path.to_string_lossy().to_string())
            .collect()
    }

    /// 树内键盘导航（↑↓）：按可见行顺序移动单选，并把选中项滚到视口内。
    fn scratchpad_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let keys = self.scratchpad_visible_keys(cx);
        if keys.is_empty() {
            return;
        }
        let current = {
            let view = self.scratchpad.borrow();
            view.anchor
                .clone()
                .or_else(|| view.selected.iter().next().cloned())
        };
        let position = current.and_then(|k| keys.iter().position(|key| key == &k));
        let next = match position {
            Some(i) => (i as isize + delta).clamp(0, keys.len() as isize - 1) as usize,
            // 无选中：↓ 取首项，↑ 取末项。
            None if delta >= 0 => 0,
            None => keys.len() - 1,
        };
        let key = keys[next].clone();
        {
            let mut view = self.scratchpad.borrow_mut();
            view.selected.clear();
            view.selected.insert(key.clone());
            view.anchor = Some(key);
        }
        self.scratchpad
            .borrow()
            .list_scroll
            .handle()
            .scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
    }

    /// Enter / →：文件夹展开折叠（展开时顺带懒加载）；文件在编辑器接入前先定位到文件。
    fn scratchpad_open_selection(&mut self, cx: &mut Context<Self>) {
        let path = {
            let view = self.scratchpad.borrow();
            if view.selected.len() == 1 {
                view.selected.iter().next().cloned()
            } else {
                None
            }
        };
        let Some(path) = path else {
            return;
        };
        let is_folder = self
            .scratchpad
            .borrow()
            .kind_of(&path)
            .map(|k| k == ScratchpadEntryKind::Folder)
            .unwrap_or(false);
        if !is_folder {
            // Phase C 前：草稿双击打开需要中央编辑器草稿模式，暂回落到「打开所在位置」。
            self.open_scratchpad_location(path.clone(), cx);
            *self.shared.notice.borrow_mut() = Some(format!(
                "{}：编辑器接入后可直接打开（已定位到文件）",
                scratchpad_basename(&path)
            ));
            cx.notify();
            return;
        }
        let needs_load = {
            let mut view = self.scratchpad.borrow_mut();
            if view.expanded.contains(&path) {
                view.expanded.remove(&path);
                false
            } else {
                view.expanded.insert(path.clone());
                !view.children.contains_key(&path)
            }
        };
        if needs_load {
            self.request_scratchpad_dir(path, cx);
        } else {
            cx.notify();
        }
    }

    /// 全选已加载条目（Ctrl+A）。
    fn select_all_scratchpad(&mut self, cx: &mut Context<Self>) {
        fn walk(entries: &[ScratchpadEntry], out: &mut Vec<String>) {
            for e in entries {
                out.push(e.path.to_string_lossy().to_string());
                if let Some(kids) = &e.children {
                    walk(kids, out);
                }
            }
        }
        let mut keys = Vec::new();
        {
            let view = self.scratchpad.borrow();
            walk(&view.entries, &mut keys);
            for kids in view.children.values() {
                walk(kids, &mut keys);
            }
        }
        let mut view = self.scratchpad.borrow_mut();
        view.selected = keys.into_iter().collect();
        view.anchor = None;
        drop(view);
        cx.notify();
    }

    /// 运行内容搜索（结果写入 `Shared::scratchpad_search`，由中央编辑区渲染）。
    fn run_scratchpad_content_search(&mut self, cx: &mut Context<Self>) {
        let query = self
            .scratchpad
            .borrow()
            .search_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            *self.shared.scratchpad_search.borrow_mut() = None;
            self.shared.notify_host(cx);
            cx.notify();
            return;
        }
        let (is_regex, case_sensitive) = {
            let v = self.scratchpad.borrow();
            (v.search_regex, v.search_case)
        };
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => run_scratchpad_search(&store, &rt, &query, case_sensitive, is_regex),
            Err(e) => Err(e),
        };
        match result {
            Ok(view) => {
                *self.shared.scratchpad_search.borrow_mut() = Some(view);
                self.scratchpad.borrow_mut().error = None;
            }
            Err(e) => {
                *self.shared.scratchpad_search.borrow_mut() = None;
                self.scratchpad.borrow_mut().error = Some(format!("搜索失败: {e}"));
            }
        }
        self.shared.notify_host(cx);
        cx.notify();
    }

    /// 懒加载子目录（展开文件夹时调用）。
    /// 安排撤销栏 5 秒后自动消失（仅当仍指向同一次删除）。
    fn schedule_undo_expiry(&self, trash_ids: Vec<String>, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            executor.timer(std::time::Duration::from_secs(5)).await;
            let _ = weak.update(cx, |this, cx| {
                let matches = this
                    .scratchpad
                    .borrow()
                    .undo
                    .as_ref()
                    .map(|u| u.trash_ids == trash_ids)
                    .unwrap_or(false);
                if matches {
                    this.scratchpad.borrow_mut().undo = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 数据源导航面板（M4）：面板头 + 来源 chips + 搜索 + 分组树。
    fn render_database_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.nav_search.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| {
                s.set_placeholder("筛选数据源 / 表 / 列 / 标签…", window, cx)
            });
            // 输入变化时通知面板重渲染，否则过滤词不会即时生效。
            let sub = cx.subscribe_in(&state, window, |_this, _e, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.nav_search = Some(state);
            self._nav_search_sub = Some(sub);
        }
        // Ctrl+F 请求（搜索框可能上一帧才创建，故在此统一消费）。
        if self.shared.focus_nav_search.get() {
            self.shared.focus_nav_search.set(false);
            if let Some(input) = self.nav_search.clone() {
                let handle = input.read(cx).focus_handle(cx);
                handle.focus(window, cx);
            }
        }

        // 本地 SQLite 一次性读取（分组/标签、各连接展开态）不在 render 做，
        // 推到本帧效果周期之后执行，完成后重绘。
        let need_org = !self.database_nav.borrow().groups_loaded;
        let need_state: Vec<String> = {
            let view = self.database_nav.borrow();
            self.shared
                .connections
                .borrow()
                .iter()
                .filter(|c| !view.state_loaded.contains(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        if need_org || !need_state.is_empty() {
            let conn_ids = need_state.clone();
            cx.defer_in(window, move |this, _window, cx| {
                let org_pending = need_org && !this.database_nav.borrow().groups_loaded;
                if org_pending {
                    this.reload_nav_org();
                }
                for cid in &conn_ids {
                    this.ensure_nav_state_loaded(cid);
                }
                cx.notify();
            });
        }
        let raw_search = self
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default();
        // 搜索框 facet 语法（`scope:` / `type:` / `driver:` / `tag:`）拆为额外约束，
        // 其余为自由文本；与面板 chips 叠加（AND）而非写回，避免输入框反馈环。
        {
            let parsed = parse_nav_search(&raw_search);
            let mut view = self.database_nav.borrow_mut();
            view.filter = parsed.free.clone();
            view.search_facets = parsed;
        }

        // 连接行内联**标签**编辑器（`+` 打开）打开时，按需创建标签输入框
        // 并用当前标签预填；关闭时销毁，保证下次打开重新回填。
        let tag_editor_for = self.database_nav.borrow().tag_editor_for.clone();
        match &tag_editor_for {
            Some(conn_id) => {
                // 已为**本**连接建过则复用；否则重建并重新预填（曾在 A 开过再切 B 时
                // 会沿用 A 的输入值 → 回车把 A 的标签写到 B）。
                let stale = self.nav_tag_input.is_none()
                    || self.nav_tag_input_for.as_deref() != Some(conn_id.as_str());
                if stale {
                    let current = crate::services::nav_runtime::list_tags(
                        conn_id,
                        self.project_root().as_deref(),
                    );
                    let text = current.join(", ");
                    let input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("标签，逗号分隔"));
                    input.update(cx, |s, cx| s.set_value(text, window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_nav_tags(cx),
                            _ => {}
                        },
                    );
                    self.nav_tag_input = Some(input);
                    self.nav_tag_input_for = Some(conn_id.clone());
                    self._nav_tag_sub = Some(sub);
                }
            }
            None => {
                self.nav_tag_input = None;
                self.nav_tag_input_for = None;
                self._nav_tag_sub = None;
            }
        }

        // 连接行内「复制为模板」输入框：打开时创建并预填「原名 副本」，关闭时销毁。
        let copy_for = self.database_nav.borrow().copy_for.clone();
        match &copy_for {
            Some(conn_id) => {
                let stale = self.nav_copy_input.is_none()
                    || self.nav_copy_input_for.as_deref() != Some(conn_id.as_str());
                if stale {
                    let base = self
                        .shared
                        .connections
                        .borrow()
                        .iter()
                        .find(|c| c.id == *conn_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| conn_id.clone());
                    let input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("新连接名称"));
                    input.update(cx, |s, cx| s.set_value(format!("{base} 副本"), window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_copy_connection(cx),
                            _ => {}
                        },
                    );
                    self.nav_copy_input = Some(input);
                    self.nav_copy_input_for = Some(conn_id.clone());
                    self._nav_copy_sub = Some(sub);
                }
            }
            None => {
                self.nav_copy_input = None;
                self.nav_copy_input_for = None;
                self._nav_copy_sub = None;
            }
        }

        // 分组重命名输入框：打开时按名称预填，关闭时销毁。
        let rename_for = self.database_nav.borrow().group_rename_for.clone();
        match &rename_for {
            Some(group_id) => {
                if self.nav_group_input.is_none() {
                    let name = self
                        .database_nav
                        .borrow()
                        .groups
                        .iter()
                        .find(|g| &g.id == group_id)
                        .map(|g| g.name.clone())
                        .unwrap_or_default();
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("分组名"));
                    input.update(cx, |s, cx| s.set_value(name, window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_group_rename(cx),
                            _ => {}
                        },
                    );
                    self.nav_group_input = Some(input);
                    self._nav_group_sub = Some(sub);
                }
            }
            None => {
                self.nav_group_input = None;
                self._nav_group_sub = None;
            }
        }

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let accent = cx.theme().colors.primary;
        let source_filter = self.database_nav.borrow().source_filter;

        // 面板头「⟳ 刷新元数据」「断开当前连接」的作用目标：当前选中的连接。
        let current_conn = self.nav_current_connection();
        let current_connected = current_conn
            .as_deref()
            .map(|id| {
                crate::services::nav_runtime::is_connected(id)
                    || self.database_nav.borrow().connected.contains(id)
            })
            .unwrap_or(false);
        let project_root = self.project_root().map(|p| p.to_string_lossy().to_string());

        let header = div()
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.875))
            .pl_2p5()
            .pr_2()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg)
                    .child("数据源"),
            )
            .child(div().flex_1())
            // C1 预热进度（后台任务进行中时显示，可取消）。
            .when(nav_jobs::warm_active(), |h| {
                let (done, total) = nav_jobs::warm_progress();
                h.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("预热 {done}/{total}")),
                )
                .child(
                    div()
                        .id("nav-warm-cancel")
                        .px_1()
                        .rounded_md()
                        .text_xs()
                        .text_color(muted)
                        .cursor_pointer()
                        .hover({
                            let h = cx.theme().colors.list_hover;
                            move |s| s.bg(h)
                        })
                        .child("取消")
                        .on_click(|_, _, _| nav_jobs::cancel_warm()),
                )
            })
            .child(
                // 新建数据源（＋）：与编辑区「新建连接」同一条对话框入口。
                Button::new("nav-new-connection")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .on_click({
                        let entity = cx.entity();
                        let shared = self.shared.clone();
                        move |_, _, app: &mut App| {
                            shared.new_connection_request.set(true);
                            entity.update(app, |_, cx| cx.emit(SidebarEvent::NewConnectionRequest));
                        }
                    }),
            )
            .child(
                // 新建分组（🗂＋）：项目级自定义分组。
                div()
                    .id("nav-new-group")
                    .px_1()
                    .rounded_md()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .hover({
                        let h = cx.theme().colors.list_hover;
                        move |s| s.bg(h)
                    })
                    .child("\u{1f5c2}\u{ff0b}")
                    .on_click({
                        let entity = cx.entity();
                        move |_, _, app: &mut App| {
                            entity.update(app, |this, cx| this.create_group_interactive(cx));
                        }
                    }),
            )
            .child(
                // 刷新元数据（⟳）：刷新当前选中连接（设计 §4.2「单连接 = 工具栏 ⟳」，
                // 「全部」在「⋯」菜单）；未选中连接时禁用。
                Button::new("nav-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RotateCw)
                    .disabled(current_conn.is_none())
                    .on_click({
                        let entity = cx.entity();
                        let conn = current_conn.clone();
                        move |_, _, app| {
                            let Some(cid) = conn.clone() else {
                                return;
                            };
                            entity.update(app, |this, cx| {
                                this.refresh_node(&cid, &cid, Some(NavPath::Connection), cx);
                            });
                        }
                    }),
            )
            .child(
                // 断开当前连接（设计 §2.1 / 原型 title「断开当前连接」）：仅运行时已连接时
                // 可用；断开只关运行时连接，元数据缓存保留（可离线浏览 / 重连秒开）。
                Button::new("nav-disconnect")
                    .ghost()
                    .small()
                    .icon(
                        Icon::empty()
                            .path("icons/plug.svg")
                            .text_color(cx.theme().colors.danger),
                    )
                    .disabled(!current_connected)
                    .on_click({
                        let entity = cx.entity();
                        let conn = current_conn.clone();
                        let root = project_root.clone();
                        move |_, _, app| {
                            let Some(cid) = conn.clone() else {
                                return;
                            };
                            entity.update(app, |this, cx| {
                                this.toggle_connection(&cid, root.as_deref(), cx);
                            });
                        }
                    }),
            )
            .child(
                // 更多（⋯）：刷新全部元数据 / 缓存管理。
                Button::new("nav-more")
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu({
                        let entity = cx.entity();
                        let shared = self.shared.clone();
                        let show_tags = settings::SettingsService::show_tags(cx);
                        let show_scope = settings::SettingsService::show_scope(cx);
                        move |menu, _window, _cx| {
                            let e_refresh = entity.clone();
                            let shared_cache = shared.clone();
                            menu.item(PopupMenuItem::new("刷新全部元数据").on_click(
                                move |_, _, app| {
                                    e_refresh.update(app, |this, cx| this.refresh_all(cx));
                                },
                            ))
                            .separator()
                            .item(
                                PopupMenuItem::new(if show_tags {
                                    "✓ 显示标签"
                                } else {
                                    "显示标签"
                                })
                                .on_click(move |_, _, app| {
                                    settings::SettingsService::set_show_tags(!show_tags, app);
                                }),
                            )
                            .item(
                                PopupMenuItem::new(if show_scope {
                                    "✓ 显示归属域"
                                } else {
                                    "显示归属域"
                                })
                                .on_click(move |_, _, app| {
                                    settings::SettingsService::set_show_scope(!show_scope, app);
                                }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("缓存管理…").on_click(move |_, window, app| {
                                    crate::components::cache_dialog::open_cache_dialog(
                                        window,
                                        app,
                                        &shared_cache,
                                    );
                                }),
                            )
                        }
                    }),
            );

        let chips = div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_1()
            .pl_2p5()
            .pr_2()
            .pb_1p5()
            .child(self.nav_source_chip("全部", None, source_filter, fg, muted, accent, cx))
            .child(self.nav_source_chip(
                "项目",
                Some(NavSource::Project),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "全局",
                Some(NavSource::Global),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "共享",
                Some(NavSource::Shared),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ));

        let body = self.render_nav_tree(source_filter, cx);

        // 搜索行：输入框 + 「筛选 ▾ N」弹层（类型 / 驱动 / 标签；归属域由上方 chips 承担）。
        let active_facets = self.nav_active_facet_count();
        let facet_label = if active_facets > 0 {
            format!("筛选 ▾ {active_facets}")
        } else {
            "筛选 ▾".to_string()
        };
        let filters_active = self.nav_filters_active();
        let free_text = self.database_nav.borrow().search_facets.free.clone();
        let (cur_type, cur_driver, cur_tag) = {
            let view = self.database_nav.borrow();
            (
                view.type_filter.clone(),
                view.driver_filter.clone(),
                view.tag_filter.clone(),
            )
        };
        let (type_cands, driver_cands, tag_cands) = self.nav_facet_candidates();
        let tag_pairs: Vec<(String, String)> =
            tag_cands.iter().map(|t| (t.clone(), t.clone())).collect();
        let type_menu_label = match &cur_type {
            Some(t) => format!("类型：{}", nav_type_short_label(t)),
            None => "类型".to_string(),
        };
        let driver_menu_label = match &cur_driver {
            Some(d) => {
                let name = self
                    .shared
                    .driver_catalog
                    .borrow()
                    .get(d)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| d.clone());
                format!("驱动：{name}")
            }
            None => "驱动".to_string(),
        };
        let tag_menu_label = match &cur_tag {
            Some(t) => format!("标签：{t}"),
            None => "标签".to_string(),
        };

        let facet_button = Button::new("nav-facet-filter")
            .ghost()
            .small()
            .label(facet_label)
            .dropdown_menu({
                let entity = cx.entity();
                let input = self.nav_search.clone();
                let free = free_text.clone();
                let types = type_cands.clone();
                let drivers = driver_cands.clone();
                let tags = tag_pairs.clone();
                let ct = cur_type.clone();
                let cd = cur_driver.clone();
                let ctg = cur_tag.clone();
                let tl = type_menu_label.clone();
                let dl = driver_menu_label.clone();
                let gl = tag_menu_label.clone();
                move |menu, window, cx| {
                    let e_clear = entity.clone();
                    let input_clear = input.clone();
                    let free_clear = free.clone();
                    let mut menu = menu.item(
                        PopupMenuItem::new("清除筛选")
                            .disabled(!filters_active)
                            .on_click(move |_, window, app| {
                                if let Some(input) = &input_clear {
                                    let free = free_clear.clone();
                                    input
                                        .update(app, |s, cx| s.set_value(free.clone(), window, cx));
                                }
                                e_clear.update(app, |this, cx| this.clear_nav_filters(cx));
                            }),
                    );
                    menu = menu.separator();
                    menu = menu.submenu(tl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = ct.clone();
                        let c = types.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Type,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    menu = menu.submenu(dl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = cd.clone();
                        let c = drivers.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Driver,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    menu.submenu(gl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = ctg.clone();
                        let c = tags.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Tag,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    })
                }
            });

        let mut search_row = div().h_flex().items_center().w_full().gap_1().px_2().pb_2();
        if let Some(input) = &self.nav_search {
            search_row = search_row.child(div().flex_1().min_w_0().child(Input::new(input)));
        }
        search_row = search_row.child(facet_button);

        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .pt_1()
            .child(header)
            .child(search_row)
            .child(chips)
            .child(div().h(ui::HAIRLINE).w_full().bg(border))
            .child(body)
            .key_context("database-nav")
            .track_focus(&self.focus_handle)
            .on_action({
                let entity = cx.entity();
                move |_: &NavUp, _window, app| {
                    entity.update(app, |this, cx| this.nav_move(-1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavDown, _window, app| {
                    entity.update(app, |this, cx| this.nav_move(1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavExpand, _window, app| {
                    entity.update(app, |this, cx| this.nav_expand(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavCollapse, _window, app| {
                    entity.update(app, |this, cx| this.nav_collapse(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavOpenProperties, _window, app| {
                    entity.update(app, |this, cx| this.nav_open_properties(cx));
                }
            })
    }

    /// 来源筛选 chip（全部 / 项目 / 全局 / 共享）；点击切换筛选，不占一级结构。
    #[allow(clippy::too_many_arguments)]
    fn nav_source_chip(
        &self,
        label: &str,
        target: Option<NavSource>,
        current: Option<NavSource>,
        _fg: Hsla,
        muted: Hsla,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = target == current;
        let entity = cx.entity();
        let (bg, text) = if active {
            (accent, cx.theme().colors.primary_foreground)
        } else {
            // 未选中：无底色（透明），靠 hover 灰层给出可点反馈。
            (transparent_black(), muted)
        };
        let weight = if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        // 未选中态在 hover 时给一层浅底，保持“可点”的反馈。
        let hover_bg = if active {
            accent
        } else {
            cx.theme().colors.list_hover
        };
        div()
            .id(format!(
                "nav-source-{}",
                target.map(|s| s.code()).unwrap_or("all")
            ))
            .h_flex()
            .items_center()
            .px_1p5()
            .py_0p5()
            .rounded_full()
            .bg(bg)
            .text_xs()
            .font_weight(weight)
            .text_color(text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .child(label.to_string())
            .on_click(move |_, _, app| {
                entity.update(app, |this, cx| {
                    this.database_nav.borrow_mut().source_filter = target;
                    this.write_nav_filters(cx);
                    cx.notify();
                });
            })
    }

    /// 写回 facet 筛选到 `settings.json`（chips 状态为准；搜索 token 不持久化）。
    fn write_nav_filters(&self, cx: &mut Context<Self>) {
        let filters = {
            let view = self.database_nav.borrow();
            settings::model::NavigatorFilters {
                source: view.source_filter.map(|s| s.key().to_string()),
                db_type: view.type_filter.clone(),
                driver: view.driver_filter.clone(),
                tag: view.tag_filter.clone(),
            }
        };
        settings::SettingsService::set_nav_filters(filters, cx);
    }

    /// 应用某个 facet 值（`None` = 清除该项），并持久化 + 重渲染。
    fn apply_facet(&mut self, facet: NavFacet, value: Option<String>, cx: &mut Context<Self>) {
        {
            let mut view = self.database_nav.borrow_mut();
            match facet {
                NavFacet::Type => view.type_filter = value,
                NavFacet::Driver => view.driver_filter = value,
                NavFacet::Tag => view.tag_filter = value,
            }
        }
        self.write_nav_filters(cx);
        cx.notify();
    }

    /// 清除全部 facet 筛选（附加 facet + 归属域）。
    fn clear_nav_filters(&mut self, cx: &mut Context<Self>) {
        {
            let mut view = self.database_nav.borrow_mut();
            view.type_filter = None;
            view.driver_filter = None;
            view.tag_filter = None;
            view.source_filter = None;
        }
        self.write_nav_filters(cx);
        cx.notify();
    }

    /// 已生效的附加 facet 数（类型 / 驱动 / 标签；chips 与搜索 token 取并）。
    fn nav_active_facet_count(&self) -> usize {
        let view = self.database_nav.borrow();
        let mut n = 0;
        if view.type_filter.is_some() || view.search_facets.db_type.is_some() {
            n += 1;
        }
        if view.driver_filter.is_some() || view.search_facets.driver.is_some() {
            n += 1;
        }
        if view.tag_filter.is_some() || view.search_facets.tag.is_some() {
            n += 1;
        }
        n
    }

    /// 是否存在任何生效筛选（含归属域与搜索 token）；用于「清除筛选」可用性。
    fn nav_filters_active(&self) -> bool {
        let view = self.database_nav.borrow();
        view.source_filter.is_some()
            || view.type_filter.is_some()
            || view.driver_filter.is_some()
            || view.tag_filter.is_some()
            || view.search_facets.active > 0
    }

    /// 构建单个 facet 子菜单（「全部」+ 候选项，单选）。
    fn build_facet_items(
        menu: PopupMenu,
        entity: Entity<Self>,
        facet: NavFacet,
        current: Option<String>,
        candidates: Vec<(String, String)>,
    ) -> PopupMenu {
        let mut menu = menu.item(
            PopupMenuItem::new("全部")
                .checked(current.is_none())
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| {
                        entity.update(app, |this, cx| this.apply_facet(facet, None, cx));
                    }
                }),
        );
        for (value, label) in candidates {
            let checked = current.as_deref() == Some(value.as_str());
            let entity = entity.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                move |_, _, app| {
                    let value = value.clone();
                    entity.update(app, |this, cx| {
                        this.apply_facet(facet, Some(value.clone()), cx);
                    });
                },
            ));
        }
        menu
    }

    /// 计算 facet 候选清单（类型 / 驱动 / 标签），值 → 展示名，已排序去重。
    ///
    /// 类型 / 驱动以驱动目录为主、连接实际使用值为兜底（目录未就绪时不落空）；
    /// 标签来自已缓存的组织数据。
    fn nav_facet_candidates(&self) -> (Vec<(String, String)>, Vec<(String, String)>, Vec<String>) {
        use std::collections::{BTreeMap, BTreeSet};
        let mut types: BTreeMap<String, String> = BTreeMap::new();
        let mut drivers: BTreeMap<String, String> = BTreeMap::new();
        {
            let catalog = self.shared.driver_catalog.borrow();
            for (id, meta) in catalog.iter() {
                drivers
                    .entry(id.clone())
                    .or_insert_with(|| meta.name.clone());
                types
                    .entry(meta.type_id.clone())
                    .or_insert_with(|| nav_type_short_label(&meta.type_id));
            }
        }
        let conns: Vec<ConnectionItem> = self.shared.connections.borrow().iter().cloned().collect();
        {
            let catalog = self.shared.driver_catalog.borrow();
            for c in &conns {
                let tid = catalog
                    .get(&c.driver)
                    .map(|m| m.type_id.clone())
                    .unwrap_or_else(|| c.driver.clone());
                types
                    .entry(tid.clone())
                    .or_insert_with(|| nav_type_short_label(&tid));
                drivers
                    .entry(c.driver.clone())
                    .or_insert_with(|| c.driver.clone());
            }
        }
        let tags: BTreeSet<String> = self
            .database_nav
            .borrow()
            .tags
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();
        let mut types: Vec<(String, String)> = types.into_iter().collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut drivers: Vec<(String, String)> = drivers.into_iter().collect();
        drivers.sort_by(|a, b| a.1.cmp(&b.1));
        (types, drivers, tags.into_iter().collect())
    }

    /// 树主体：一级为自定义分组（含「未分组」），下设连接节点。
    ///
    /// 连接可属于多个分组（多对多）：只在**主组**全亮呈现，其余分组以**引用行**出现
    /// （`∈ 主组名`），避免多对多线性撑高树；归属无任何分组的连接收进「未分组」。
    fn render_nav_tree(&self, source_filter: Option<NavSource>, cx: &mut Context<Self>) -> Div {
        // 键盘导航的可见序列每帧重建（渲染是顺序权威来源）。
        self.nav_order.borrow_mut().clear();
        let muted = cx.theme().colors.muted_foreground;
        let fg = cx.theme().colors.foreground;
        // 分组 / 成员 / 标签尚未就绪（首次渲染由 `render_database_nav` 的 defer 加载）。
        if !self.database_nav.borrow().groups_loaded {
            return div()
                .v_flex()
                .w_full()
                .min_h_0()
                .px_1()
                .pt_1()
                .child(div().text_xs().text_color(muted).child("加载中…"));
        }
        let (filter, groups, membership, group_order, tags, type_filter, driver_filter, tag_filter) = {
            let view = self.database_nav.borrow();
            (
                view.filter.to_lowercase(),
                view.groups.clone(),
                view.membership.clone(),
                view.group_order.clone(),
                view.tags.clone(),
                view.type_filter.clone(),
                view.driver_filter.clone(),
                view.tag_filter.clone(),
            )
        };
        let conns: Vec<ConnectionItem> = self.shared.connections.borrow().iter().cloned().collect();
        let by_id: HashMap<&str, &ConnectionItem> =
            conns.iter().map(|c| (c.id.as_str(), c)).collect();

        // 搜索框 facet 语法（`scope:` / `type:` / `driver:` / `tag:`）作为额外约束叠加。
        let search_facets = self.database_nav.borrow().search_facets.clone();
        // 显式主组（连接 ID → 分组 ID）。
        let primary_explicit = self.database_nav.borrow().primary_group.clone();

        // 主组：用户**显式指定**优先；未指定时回退到分组排序最靠前的一个
        // （`membership` 按分组排序构建）。显式值若已不在所属分组（被移出）则忽略。
        // 主组用于「多组只全亮呈现一次，其余组以引用行出现」。
        let primary_gid = |conn_id: &str| -> Option<String> {
            let groups_of = membership.get(conn_id)?;
            if let Some(p) = primary_explicit.get(conn_id) {
                if groups_of.iter().any(|g| g == p) {
                    return Some(p.clone());
                }
            }
            groups_of.first().cloned()
        };

        // 单条连接是否通过归属域 chips、附加 facet（类型 / 驱动 / 标签）与搜索词（连接名 / 标签）。
        let passes = |conn: &ConnectionItem| -> bool {
            if let Some(src) = source_filter {
                if NavSource::from_conn_id(&conn.id) != src {
                    return false;
                }
            }
            if let Some(src) = search_facets.source {
                if NavSource::from_conn_id(&conn.id) != src {
                    return false;
                }
            }
            if let Some(want_type) = &type_filter {
                let actual = self
                    .shared
                    .driver_catalog
                    .borrow()
                    .get(&conn.driver)
                    .map(|m| m.type_id.clone())
                    .unwrap_or_else(|| conn.driver.clone());
                if &actual != want_type {
                    return false;
                }
            }
            if let Some(want_type) = &search_facets.db_type {
                let actual = self
                    .shared
                    .driver_catalog
                    .borrow()
                    .get(&conn.driver)
                    .map(|m| m.type_id.clone())
                    .unwrap_or_else(|| conn.driver.clone());
                if &actual != want_type {
                    return false;
                }
            }
            if let Some(want_driver) = &driver_filter {
                if &conn.driver != want_driver {
                    return false;
                }
            }
            if let Some(want_driver) = &search_facets.driver {
                if &conn.driver != want_driver {
                    return false;
                }
            }
            if let Some(want_tag) = &tag_filter {
                let hit = tags
                    .get(&conn.id)
                    .map(|ts| ts.iter().any(|t| t == want_tag))
                    .unwrap_or(false);
                if !hit {
                    return false;
                }
            }
            if let Some(want_tag) = &search_facets.tag {
                let hit = tags
                    .get(&conn.id)
                    .map(|ts| ts.iter().any(|t| t == want_tag))
                    .unwrap_or(false);
                if !hit {
                    return false;
                }
            }
            if filter.is_empty() {
                return true;
            }
            if conn.name.to_lowercase().contains(&filter) {
                return true;
            }
            tags.get(&conn.id)
                .map(|ts| ts.iter().any(|t| t.to_lowercase().contains(&filter)))
                .unwrap_or(false)
        };

        let mut column = div()
            .v_flex()
            .w_full()
            .min_h_0()
            .gap_0p5()
            .px_1()
            .pt_0p5()
            .pb_1();
        let mut shown = 0usize;

        // 运行时连接状态与错误集（分组头聚合健康度用；一次算完避免逐条查询）。
        let connected_set: HashSet<String> = {
            let mut set: HashSet<String> = self
                .database_nav
                .borrow()
                .connected
                .iter()
                .cloned()
                .collect();
            for c in &conns {
                if c.connected {
                    set.insert(c.id.clone());
                }
            }
            set
        };
        let error_set: HashSet<String> =
            self.database_nav.borrow().errors.keys().cloned().collect();

        for group in &groups {
            // 组内顺序以存储的手动排序为准（缺省无成员）。
            let members: Vec<&ConnectionItem> = group_order
                .get(&group.id)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| by_id.get(id.as_str()).copied())
                        .filter(|c| passes(c))
                        .collect()
                })
                .unwrap_or_default();
            if members.is_empty() {
                continue;
            }
            shown += members.len();
            let ok_count = members
                .iter()
                .filter(|c| connected_set.contains(&c.id))
                .count();
            let fail_count = members.iter().filter(|c| error_set.contains(&c.id)).count();
            column = column.child(self.render_group_header(
                &group.id,
                &group.name,
                members.len(),
                ok_count,
                fail_count,
                cx,
            ));
            if !self.group_collapsed(&group.id) {
                for conn in members {
                    if primary_gid(&conn.id).as_deref() == Some(group.id.as_str()) {
                        column = column.child(self.render_connection_row(conn, &group.id, cx));
                    } else {
                        // 引用行：全亮行在主组，这里只指路。
                        let primary_name = primary_gid(&conn.id)
                            .and_then(|gid| {
                                groups.iter().find(|g| g.id == gid).map(|g| g.name.clone())
                            })
                            .unwrap_or_else(|| "未分组".to_string());
                        column = column.child(self.render_reference_row(
                            conn,
                            &group.id,
                            &primary_name,
                            cx,
                        ));
                    }
                }
            }
        }

        // 「未分组」固定分组：收纳不属于任何自定义分组的连接（空则隐藏）。
        // 无存储顺序，按名称升序。
        let mut ungrouped: Vec<&ConnectionItem> = conns
            .iter()
            .filter(|c| {
                membership
                    .get(&c.id)
                    .map(|gs| gs.is_empty())
                    .unwrap_or(true)
                    && passes(c)
            })
            .collect();
        ungrouped.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if !ungrouped.is_empty() {
            shown += ungrouped.len();
            let ok_count = ungrouped
                .iter()
                .filter(|c| connected_set.contains(&c.id))
                .count();
            let fail_count = ungrouped
                .iter()
                .filter(|c| error_set.contains(&c.id))
                .count();
            column = column.child(self.render_group_header(
                GROUP_UNGROUPED,
                "未分组",
                ungrouped.len(),
                ok_count,
                fail_count,
                cx,
            ));
            if !self.group_collapsed(GROUP_UNGROUPED) {
                for conn in ungrouped {
                    column = column.child(self.render_connection_row(conn, GROUP_UNGROUPED, cx));
                }
            }
        }

        if shown == 0 {
            if conns.is_empty() {
                // 空态引导（设计 §2.4）：标题 + 说明 + 面板内「新建连接」按钮。
                let entity = cx.entity();
                let shared = self.shared.clone();
                column = column.child(
                    div()
                        .w_full()
                        .pt_5()
                        .px_3()
                        .v_flex()
                        .items_start()
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(fg)
                                .child("还没有数据源"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("当前项目与全局库均无连接。点击右上角「＋」新建。"),
                        )
                        .child(
                            Button::new("nav-empty-new-connection")
                                .primary()
                                .small()
                                .icon(IconName::Plus)
                                .label("新建连接")
                                .on_click(move |_, _, app: &mut App| {
                                    shared.new_connection_request.set(true);
                                    entity.update(app, |_, cx| {
                                        cx.emit(SidebarEvent::NewConnectionRequest)
                                    });
                                }),
                        ),
                );
            } else {
                column = column.child(
                    div()
                        .w_full()
                        .pt_5()
                        .px_3()
                        .text_xs()
                        .text_color(muted)
                        .child("没有匹配的数据源。"),
                );
            }
        }
        column
    }

    /// 分组头：左侧统一色条 + 略深底 + **健康度**（已连/总）+ 计数 + 全折叠；右键菜单（重命名 / 新建 / 删除）。
    fn render_group_header(
        &self,
        group_id: &str,
        name: &str,
        count: usize,
        connected_count: usize,
        failed_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let danger = cx.theme().colors.danger;
        let bar = cx.theme().colors.primary;
        let bg = cx.theme().colors.sidebar_accent;
        let hover = cx.theme().colors.list_hover;
        let collapsed = self.group_collapsed(group_id);
        let entity = cx.entity();
        let gid = group_id.to_string();
        let gname = name.to_string();
        let is_ungrouped = group_id == GROUP_UNGROUPED;
        let health_text = format!("{connected_count}/{count}");

        let header = div()
            .id(format!("nav-group-{group_id}"))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.5))
            .px_1()
            .gap_1()
            .rounded_md()
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click({
                let entity = entity.clone();
                let gid = gid.clone();
                move |_, _, app| {
                    let gid = gid.clone();
                    entity.update(app, |this, cx| {
                        {
                            let mut view = this.database_nav.borrow_mut();
                            if view.collapsed_groups.contains(&gid) {
                                view.collapsed_groups.remove(&gid);
                            } else {
                                view.collapsed_groups.insert(gid.clone());
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .w(ui::NAV_GROUP_BAR_WIDTH)
                    .h(rems(0.875))
                    .flex_none()
                    .rounded_full()
                    .bg(bar),
            )
            .child(
                div()
                    .w_2p5()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(if collapsed { "\u{25b8}" } else { "\u{25be}" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_ellipsis()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child(name.to_string()),
            )
            // 聚合健康度（v5）：已连接/总数；有失败时附 danger 计数点。
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if failed_count > 0 { danger } else { muted })
                    .child(if failed_count > 0 {
                        format!("{health_text} · {failed_count} 失败")
                    } else {
                        health_text
                    }),
            )
            .child(div().text_xs().text_color(muted).child(count.to_string()))
            // 全折叠：一键折叠 / 展开全部同层分组（v5）。
            .child(
                div()
                    .id(format!("nav-group-foldall-{group_id}"))
                    .w_2p5()
                    .h_2p5()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover))
                    .child("\u{21c5}")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app: &mut App| {
                            entity.update(app, |this, cx| {
                                let ids: Vec<String> = this
                                    .database_nav
                                    .borrow()
                                    .groups
                                    .iter()
                                    .map(|g| g.id.clone())
                                    .collect();
                                {
                                    let mut view = this.database_nav.borrow_mut();
                                    let all_collapsed = !ids.is_empty()
                                        && ids.iter().all(|id| view.collapsed_groups.contains(id));
                                    if all_collapsed {
                                        view.collapsed_groups.clear();
                                    } else {
                                        for id in ids {
                                            view.collapsed_groups.insert(id);
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }),
            )
            // 「未分组」是固定分组，不提供重命名 / 删除。
            .context_menu({
                let entity = entity.clone();
                let gid = gid.clone();
                let gname = gname.clone();
                move |menu, _window, _cx| {
                    if is_ungrouped {
                        return menu.item(PopupMenuItem::new("新建分组").on_click({
                            let entity = entity.clone();
                            move |_, _, app| {
                                entity.update(app, |this, cx| this.create_group_interactive(cx));
                            }
                        }));
                    }
                    let e_rename = entity.clone();
                    let gid_rename = gid.clone();
                    let e_new = entity.clone();
                    let e_del = entity.clone();
                    let gid_del = gid.clone();
                    let gname_del = gname.clone();
                    menu.item(PopupMenuItem::new("重命名分组").on_click(move |_, _, app| {
                        let gid = gid_rename.clone();
                        e_rename.update(app, |this, cx| {
                            this.database_nav.borrow_mut().group_rename_for = Some(gid.clone());
                            cx.notify();
                        });
                    }))
                    .item(PopupMenuItem::new("新建分组").on_click(move |_, _, app| {
                        e_new.update(app, |this, cx| this.create_group_interactive(cx));
                    }))
                    .separator()
                    .item(
                        PopupMenuItem::new("删除分组").on_click(move |_, window, app| {
                            let entity = e_del.clone();
                            let gid = gid_del.clone();
                            let gname = gname_del.clone();
                            window.open_alert_dialog(app, move |alert, _window, _cx| {
                                let entity = entity.clone();
                                let gid = gid.clone();
                                alert
                                    .confirm()
                                    .title("删除分组")
                                    .description(format!(
                                        "确定删除分组「{gname}」？成员连接与缓存不会被删除。"
                                    ))
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text("删除")
                                            .ok_variant(ButtonVariant::Danger)
                                            .show_cancel(true),
                                    )
                                    .on_ok(move |_, _window, app| {
                                        entity.update(app, |this, cx| this.delete_group(&gid, cx));
                                        true
                                    })
                            });
                        }),
                    )
                }
            });

        let mut wrap = div().v_flex().w_full().gap_0p5();
        wrap = wrap.child(header);
        // 重命名内联输入（打开时创建，见 `render_database_nav`）。
        if self.database_nav.borrow().group_rename_for.as_deref() == Some(group_id) {
            if let Some(input) = &self.nav_group_input {
                wrap = wrap.child(div().px_1().pb_0p5().child(Input::new(input)));
            }
        }
        wrap
    }

    /// 引用行（v6）：连接已在其**主组**全亮呈现，此分组下只做指路。
    ///
    /// 不重复状态 / 徽标 / 操作位（避免重复向）；点击展开主组并选中该连接。
    /// `group_id` 为**当前包含它的分组**（用于唯一元素 ID），`primary_name` 为主组名。
    fn render_reference_row(
        &self,
        conn: &ConnectionItem,
        group_id: &str,
        primary_name: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let entity = cx.entity();
        let conn_id = conn.id.clone();
        let primary_gid = self
            .database_nav
            .borrow()
            .membership
            .get(&conn.id)
            .and_then(|gs| gs.first().cloned());
        let label = format!("\u{2208} {primary_name}");
        div()
            .id(format!("nav-ref-{}::{}", group_id, conn.id))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.625))
            .px_1()
            .gap_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(div().w_2p5().flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(conn.name.clone()),
            )
            .child(div().flex_none().text_xs().text_color(muted).child(label))
            .on_click(move |_, _, app| {
                let cid = conn_id.clone();
                entity.update(app, |this, cx| {
                    {
                        let mut view = this.database_nav.borrow_mut();
                        // 展开主组（若已折叠）并选中该连接，使全亮行可见。
                        if let Some(gid) = &primary_gid {
                            view.collapsed_groups.remove(gid);
                        }
                        view.selected_key = Some(cid.clone());
                    }
                    cx.notify();
                });
            })
    }

    /// 连接节点行（徽标（色=状态·形=类型） + 名称 + 归属域列 + 行尾操作）。
    ///
    /// `scope_key` 为其所属分组标识：同一连接可出现在多个分组，用于生成唯一元素 ID。
    fn render_connection_row(
        &self,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let danger = cx.theme().colors.danger;

        let expanded = {
            let view = self.database_nav.borrow();
            view.expanded.contains(&conn.id)
        };
        let (connected, error, children) = {
            let view = self.database_nav.borrow();
            (
                view.connected.contains(&conn.id) || conn.connected,
                view.errors.get(&conn.id).cloned(),
                view.children.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        // 仅在**运行时已连接**时才在渲染期排后台加载。
        // 为何加这道门：展开态会跨重启从 `navigator_state` 恢复，但运行时连接不跨重启；
        // 若此时仍排队，`NavigatorService` → `MetadataService` 取不到句柄，
        // 会冒泡为用户看到的 `[CONN_NOT_FOUND]`。
        if expanded && connected {
            self.ensure_nav_loaded(&conn.id, &conn.id, NavPath::Connection, false, cx);
        }

        let source = NavSource::from_conn_id(&conn.id);
        // 来源标识：短码 `P/G/GP` 或文字（设置项，默认短码）。
        let source_text = if settings::SettingsService::source_short_code(cx) {
            source.code().to_string()
        } else {
            source.label().to_string()
        };
        let filter = self.database_nav.borrow().filter.to_lowercase();
        let match_bg = settings::product_tokens::get(cx).search_match_background(cx.theme());
        let project_root = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|p| p.root.to_string_lossy().to_string());
        let conn_id = conn.id.clone();
        let scope_key = scope_key.to_string();
        let selected = self.database_nav.borrow().selected_key.as_deref() == Some(conn.id.as_str());
        let selected_bg = cx.theme().colors.list_active;
        // 键盘导航序列（父在前，子随渲染加入）。
        self.nav_order.borrow_mut().push(NavOrderItem {
            key: conn.id.clone(),
            conn_id: conn.id.clone(),
            path: Some(NavPath::Connection),
            property: Some(PropertyRef {
                conn_id: conn.id.clone(),
                source,
                catalog: None,
                schema: None,
                parent: None,
                name: conn.name.clone(),
                kind: PropertyKind::Connection,
            }),
            has_children: true,
            expanded,
        });

        // ---- v7：行内只常驻「徽标 + 名称 + 归属域列」；`+` 与行操作仅 hover / 选中显 ----
        let nav_view = self
            .shared
            .driver_catalog
            .borrow()
            .get(&conn.driver)
            .map(|m| (m.type_id.clone(), m.name.clone()));
        let type_id = nav_view
            .as_ref()
            .map(|(t, _)| t.clone())
            .unwrap_or_else(|| conn.driver.clone());
        let driver_name = nav_view.map(|(_, n)| n);
        let nav_view = self.database_nav.borrow();
        let badge_status = if nav_view.loading.contains(&conn.id) {
            NavBadgeStatus::Connecting
        } else if error.is_some() {
            NavBadgeStatus::Failed
        } else if connected {
            NavBadgeStatus::Connected
        } else {
            NavBadgeStatus::Idle
        };
        let tag_list: Vec<String> = nav_view.tags.get(&conn.id).cloned().unwrap_or_default();
        drop(nav_view);

        let (badge_path, badge_letters) = nav_type_badge(&type_id);
        let badge_color = badge_status.color(cx.theme());
        // 双通道徽标：颜色 = 状态（能不能用），形状 = 类型（是什么库，内叠 2 字母）。
        let badge = div()
            .relative()
            .w(rems(ui::NAV_BADGE_SIZE))
            .h(rems(ui::NAV_BADGE_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::empty()
                            .path(badge_path)
                            .size(rems(ui::NAV_BADGE_SIZE))
                            .text_color(badge_color),
                    ),
            )
            .child(
                div()
                    .relative()
                    .text_size(rems(0.4))
                    .font_weight(FontWeight::BOLD)
                    .text_color(badge_color)
                    .child(badge_letters),
            );

        // 徽标 hover 卡：类型 / 状态 / 驱动的完整事实（gpui-kit 0.6.1 无通用 `.tooltip` 扩展，
        // 故用 `HoverCard` 承载；行内仍只显颜色 + 形状）。
        let badge = {
            let type_label = nav_type_label(&type_id);
            let status_label = badge_status.label();
            let driver_label = driver_name
                .clone()
                .map(|n| format!("{n} · {}", conn.driver))
                .unwrap_or_else(|| conn.driver.clone());
            let hover_id = SharedString::from(format!("nav-badge-{}::{}", scope_key, conn.id));
            nav_badge_hover_card(hover_id, badge, type_label, status_label, driver_label)
        };

        // 标签 chip（可选显示，`⋯ → 显示标签`）：默认关；开启后「≤2 chip + `+N`」。
        let tag_chips = if settings::SettingsService::show_tags(cx) && !tag_list.is_empty() {
            let chip_bg = cx.theme().colors.list_hover;
            // 标签字号用最初版小字 `text_xs`（与行内文字同尺寸，不因换行而变大）。
            let mut chips = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .text_xs();
            for t in tag_list.iter().take(2) {
                chips = chips.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .bg(chip_bg)
                        .text_color(muted)
                        .child(t.clone()),
                );
            }
            if tag_list.len() > 2 {
                chips = chips.child(
                    div()
                        .text_color(muted)
                        .child(format!("+{}", tag_list.len() - 2)),
                );
            }
            Some(chips)
        } else {
            None
        };

        // 归属域短码：右对齐固定列（可在 `⋯ → 显示归属域` 关闭）。
        let scope_visible = settings::SettingsService::show_scope(cx);
        let scope_col = if settings::SettingsService::source_short_code(cx) {
            ui::NAV_SCOPE_COL_SHORT
        } else {
            ui::NAV_SCOPE_COL_TEXT
        };
        let scope_color = match source {
            NavSource::Project => cx.theme().colors.info,
            NavSource::Global => muted,
            NavSource::Shared => cx.theme().colors.primary,
        };

        // 行尾操作（v8）：`+` 加标签 · `✎` 编辑；仅 hover / 选中显（连接 / 断开走右键菜单）。
        let ops = {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let cid = conn_id.clone();
            let hover_bg = cx.theme().colors.list_hover;
            let mut ops = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .opacity(if selected { 1.0 } else { 0.0 })
                .group_hover("nav-conn-row", |s| s.opacity(1.0));
            // `+`：仅**标签**行内编辑（`+` 只处理标签；归组走右键「移动到分组…」）。
            ops = ops.child(
                div()
                    .id(format!("nav-conn-addtag-{}::{}", scope_key, conn.id))
                    .w(rems(ui::NAV_ADD_TAG_SIZE))
                    .h(rems(ui::NAV_ADD_TAG_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover_bg))
                    .child(Icon::empty().path("icons/plus.svg").size(rems(0.75)))
                    .on_click({
                        let entity = entity.clone();
                        let cid = cid.clone();
                        move |_, _, app: &mut App| {
                            let cid = cid.clone();
                            entity.update(app, |this, cx| {
                                let mut view = this.database_nav.borrow_mut();
                                view.tag_editor_for =
                                    if view.tag_editor_for.as_deref() == Some(cid.as_str()) {
                                        None
                                    } else {
                                        Some(cid.clone())
                                    };
                                cx.notify();
                            });
                        }
                    }),
            );
            // `✎`：在对话框中编辑连接。
            ops = ops.child(
                div()
                    .id(format!("nav-conn-edit-{}::{}", scope_key, conn.id))
                    .px_1()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .rounded_md()
                    .hover(move |s| s.bg(hover_bg))
                    .child("\u{270e}")
                    .on_click({
                        let entity = entity.clone();
                        let shared = shared.clone();
                        let cid = cid.clone();
                        move |_, _, app: &mut App| {
                            *shared.open_edit.borrow_mut() = Some(cid.clone());
                            entity.update(app, |_, cx| {
                                cx.emit(SidebarEvent::EditConnection(cid.clone()));
                            });
                        }
                    }),
            );
            // 连接 / 断开不进 hover 行操作：仅右键菜单提供（避免误触与视觉噪声）。
            ops
        };

        // 徽标 tooltip 材料（接入 `Tooltip` 组件前，事实统一在属性面板展示）。
        let _ = (&driver_name, danger);

        // 「设为主组」子菜单数据（仅归组的连接出现）：所属分组 + 当前主组 + 是否显式。
        let (menu_groups, menu_primary, menu_primary_explicit) = {
            let view = self.database_nav.borrow();
            let gids = view.membership.get(&conn.id).cloned().unwrap_or_default();
            let explicit = view
                .primary_group
                .get(&conn.id)
                .filter(|p| gids.iter().any(|g| g == *p))
                .cloned();
            let primary = explicit.clone().or_else(|| gids.first().cloned());
            let names: Vec<(String, String)> = gids
                .iter()
                .map(|gid| {
                    let name = view
                        .groups
                        .iter()
                        .find(|g| &g.id == gid)
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| gid.clone());
                    (gid.clone(), name)
                })
                .collect();
            (names, primary, explicit.is_some())
        };

        let mut block = div().v_flex().w_full();
        block = block.child(
            div()
                .id(format!("nav-conn-{}::{}", scope_key, conn.id))
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(1.625))
                .px_1()
                .gap_1()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |s| s.bg(selected_bg))
                .hover(move |s| s.bg(hover))
                .on_click({
                    let entity = cx.entity();
                    let conn_id = conn_id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    let focus = self.focus_handle.clone();
                    move |ev, window, app| {
                        focus.focus(window, app);
                        // 单击选中（键盘导航基准）；双击打开属性；再次点击展开 / 折叠。
                        let sel_key = conn_id.clone();
                        entity.update(app, |this, cx| {
                            this.database_nav.borrow_mut().selected_key = Some(sel_key.clone());
                            cx.notify();
                        });
                        if ev.click_count() >= 2 {
                            let req = PropertyRequest {
                                property: PropertyRef {
                                    conn_id: conn_id.clone(),
                                    source,
                                    catalog: None,
                                    schema: None,
                                    parent: None,
                                    name: conn_name.clone(),
                                    kind: PropertyKind::Connection,
                                },
                                conn_label: conn_name.clone(),
                                driver: conn_driver.clone(),
                            };
                            entity.update(app, |this, cx| {
                                *this.shared.property_target.borrow_mut() = Some(req);
                                cx.notify();
                            });
                            return;
                        }
                        let conn_id = conn_id.clone();
                        entity.update(app, |this, cx| {
                            this.toggle_nav_node(&conn_id, &conn_id, NavPath::Connection, cx);
                            cx.notify();
                        });
                    }
                })
                .child(
                    div()
                        .w_2p5()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(if expanded { "\u{25be}" } else { "\u{25b8}" }),
                )
                .group("nav-conn-row")
                .child(badge)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(fg)
                        .text_ellipsis()
                        .child(nav_name_highlight(&conn.name, &filter, match_bg, fg)),
                )
                .when(scope_visible, |s| {
                    s.child(
                        div()
                            .w(rems(scope_col))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(scope_color)
                            .child(source_text.clone()),
                    )
                })
                // 行尾操作组（`+` / `✎` / 连接·断开）：仅 hover / 选中显（v8）。
                .child(ops)
                // 右键菜单（连接节点）：连接/断开、编辑、查看属性、分组/标签、复制、刷新。
                .context_menu({
                    let entity = cx.entity();
                    let shared = self.shared.clone();
                    let conn_id = conn.id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    let root = project_root.clone();
                    let prop = PropertyRef {
                        conn_id: conn_id.clone(),
                        source,
                        catalog: None,
                        schema: None,
                        parent: None,
                        name: conn_name.clone(),
                        kind: PropertyKind::Connection,
                    };
                    let is_connected = connected;
                    move |menu, window, cx| {
                        let e_connect = entity.clone();
                        let cid_connect = conn_id.clone();
                        let root_connect = root.clone();
                        let e_edit = entity.clone();
                        let cid_edit = conn_id.clone();
                        let e_prop = entity.clone();
                        let prop_own = prop.clone();
                        let label_prop = conn_name.clone();
                        let drv_prop = conn_driver.clone();
                        let e_org = entity.clone();
                        let cid_org = conn_id.clone();
                        let e_copy = entity.clone();
                        let shared_copy = shared.clone();
                        let name_copy = conn_name.clone();
                        let e_refresh = entity.clone();
                        let cid_refresh = conn_id.clone();
                        let name_refresh = conn_name.clone();
                        let mut menu = menu
                            .item(
                                PopupMenuItem::new(if is_connected { "断开" } else { "连接" })
                                    .on_click(move |_, _, app| {
                                        let cid = cid_connect.clone();
                                        let root = root_connect.clone();
                                        e_connect.update(app, |this, cx| {
                                            this.toggle_connection(&cid, root.as_deref(), cx)
                                        });
                                    }),
                            )
                            .item(PopupMenuItem::new("测试连接").on_click({
                                let e = entity.clone();
                                let cid = conn_id.clone();
                                let root = root.clone();
                                let name = conn_name.clone();
                                move |_, _, app| {
                                    // 独立会话探测（不注册连接池 / 不写库）；结果落面板提示。
                                    nav_jobs::enqueue_test_connection(
                                        &cid,
                                        root.as_deref(),
                                        &name,
                                    );
                                    e.update(app, |this, cx| this.ensure_nav_pump(cx));
                                }
                            }))
                            .item(PopupMenuItem::new("编辑连接…").on_click(move |_, _, app| {
                                let cid = cid_edit.clone();
                                e_edit.update(app, |this, cx| {
                                    *this.shared.open_edit.borrow_mut() = Some(cid.clone());
                                    cx.emit(SidebarEvent::EditConnection(cid.clone()));
                                });
                            }))
                            .separator()
                            .item(PopupMenuItem::new("查看属性").on_click(move |_, _, app| {
                                let prop = prop_own.clone();
                                let label = label_prop.clone();
                                let drv = drv_prop.clone();
                                e_prop.update(app, |this, cx| {
                                    *this.shared.property_target.borrow_mut() =
                                        Some(PropertyRequest {
                                            property: prop.clone(),
                                            conn_label: label.clone(),
                                            driver: drv.clone(),
                                        });
                                    cx.notify();
                                });
                            }))
                            .item(
                                PopupMenuItem::new("移动到分组…").on_click(move |_, _, app| {
                                    let cid = cid_org.clone();
                                    e_org.update(app, |this, cx| {
                                        this.database_nav.borrow_mut().group_picker_for =
                                            Some(cid.clone());
                                        cx.notify();
                                    });
                                }),
                            );
                        // 「设为主组 ▸」：仅在该连接已归组时出现（单选 + 「自动」回退）。
                        if !menu_groups.is_empty() {
                            let e_p = entity.clone();
                            let root_p = root.clone();
                            let cid_p = conn_id.clone();
                            let groups_for = menu_groups.clone();
                            let cur = menu_primary.clone();
                            let explicit = menu_primary_explicit;
                            menu = menu.submenu("设为主组", window, cx, move |m, _w, _c| {
                                let mut m = m.item(
                                    PopupMenuItem::new("自动（按分组排序）")
                                        .checked(!explicit)
                                        .on_click({
                                            let e = e_p.clone();
                                            let root = root_p.clone();
                                            let cid = cid_p.clone();
                                            move |_, _, app| {
                                                let root = root.clone();
                                                let cid = cid.clone();
                                                let root = root.as_deref().map(std::path::Path::new);
                                                e.update(app, |this, cx| {
                                                    let _ = crate::services::nav_runtime::clear_primary_group(
                                                        root,
                                                        &cid,
                                                    );
                                                    this.reload_nav_org();
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                );
                                for (gid, gname) in groups_for.clone() {
                                    let checked = cur.as_deref() == Some(gid.as_str());
                                    let e = e_p.clone();
                                    let root = root_p.clone();
                                    let cid = cid_p.clone();
                                    m = m.item(
                                        PopupMenuItem::new(gname).checked(checked).on_click(
                                            move |_, _, app| {
                                                let gid = gid.clone();
                                                let root = root.clone();
                                                let cid = cid.clone();
                                                let root = root.as_deref().map(std::path::Path::new);
                                                e.update(app, |this, cx| {
                                                    let _ =
                                                        crate::services::nav_runtime::set_primary_group(
                                                            root,
                                                            &cid,
                                                            &gid,
                                                        );
                                                    this.reload_nav_org();
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                m
                            });
                        }
                        // 复制（模板）/ 共享 / 删除：连接自身的管理动作。
                        // 共享快照（GP_）本身即副本，不提供复制。
                        if source != NavSource::Shared {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new("复制连接（模板）…").on_click(
                                    move |_, _, app| {
                                        let cid = cid.clone();
                                        e.update(app, |this, cx| {
                                            this.database_nav.borrow_mut().copy_for =
                                                Some(cid.clone());
                                            cx.notify();
                                        });
                                    },
                                ),
                            );
                        }
                        if source == NavSource::Global {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.item(
                                PopupMenuItem::new("共享至项目").on_click(move |_, _, app| {
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    e.update(app, |this, cx| {
                                        this.share_connection_to_project(&cid, &name, cx)
                                    });
                                }),
                            );
                        }
                        if source == NavSource::Shared {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.item(
                                PopupMenuItem::new("取消共享").on_click(move |_, _, app| {
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    e.update(app, |this, cx| {
                                        this.unshare_connection_from_project(&cid, &name, cx)
                                    });
                                }),
                            );
                        }
                        {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new("删除连接").on_click(move |_, window, app| {
                                    let e = e.clone();
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                                        let e = e.clone();
                                        let cid = cid.clone();
                                        // 内层 `on_ok` 是 move 闭包：先在外层建一份新绑定，
                                        // 否则会把外层闭包环境里的 `name` 移出（E0507）。
                                        let name_for_ok = name.clone();
                                        alert
                                            .confirm()
                                            .title("删除连接")
                                            .description(format!(
                                                "确定删除连接「{name}」？元数据缓存会保留（可在「缓存管理」清理）。"
                                            ))
                                            .button_props(
                                                DialogButtonProps::default()
                                                    .ok_text("删除")
                                                    .ok_variant(ButtonVariant::Danger)
                                                    .show_cancel(true),
                                            )
                                            .on_ok(move |_, _window, app| {
                                                let cid = cid.clone();
                                                let name = name_for_ok.clone();
                                                e.update(app, |this, cx| {
                                                    this.delete_connection(&cid, &name, cx)
                                                });
                                                true
                                            })
                                    });
                                }),
                            );
                        }
                        menu.item(PopupMenuItem::new("复制名称").on_click(move |_, _, app| {
                            let name = name_copy.clone();
                            app.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                            *shared_copy.notice.borrow_mut() = Some(format!("已复制：{name}"));
                            e_copy.update(app, |_, cx| cx.notify());
                        }))
                        .item(PopupMenuItem::new("刷新元数据").on_click(move |_, _, app| {
                            let cid = cid_refresh.clone();
                            let name = name_refresh.clone();
                            e_refresh.update(app, |this, cx| {
                                this.refresh_node(&cid, &cid, Some(NavPath::Connection), cx);
                                *this.shared.notice.borrow_mut() = Some(format!("已刷新：{name}"));
                            });
                        }))
                        // 通用模块入口（与连接状态无关）：SQL 编辑器 / 洞察。
                        // Mock 只针对表 / 视图，见 `render_nav_node` 的对象菜单。
                        .separator()
                        .item(PopupMenuItem::new("在 SQL 编辑器中打开").on_click({
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            move |_, _, app| {
                                let cid = cid.clone();
                                e.update(app, |_, cx| {
                                    cx.emit(SidebarEvent::OpenSqlEditor(cid));
                                });
                            }
                        }))
                        .item(PopupMenuItem::new("查看洞察").on_click({
                            let e = entity.clone();
                            move |_, _, app| {
                                e.update(app, |_, cx| {
                                    cx.emit(SidebarEvent::OpenRightPanel(RightPanel::Insight));
                                });
                            }
                        }))
                    }
                }),
        );

        // 后台加载占位（避免展开后空白，误导为已加载完）。
        if self.database_nav.borrow().loading.contains(&conn.id) {
            block = block.child(
                div()
                    .pl_6()
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child("加载中…"),
            );
        }

        // 标签（v7 修订）：显示在连接名**下一行**（不在名称行内），仅 `⋯ → 显示标签`
        // 开启时；以「≤2 chip + `+N`」呈现，避免撑爆名称行 / 挤掉归属域列。
        if let Some(chips) = tag_chips {
            block = block.child(
                div()
                    .pl_6()
                    .pb_0p5()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(chips),
            );
        }

        // 行内编辑器：归组（右键「移动到分组…」）与标签（行尾 `+`）分开，各司其职。
        let picker_open =
            self.database_nav.borrow().group_picker_for.as_deref() == Some(conn.id.as_str());
        if picker_open {
            block = block.child(self.render_group_editor(conn, &scope_key, cx));
        }
        let tag_open =
            self.database_nav.borrow().tag_editor_for.as_deref() == Some(conn.id.as_str());
        if tag_open {
            block = block.child(self.render_tag_editor(cx));
        }
        let copy_open = self.database_nav.borrow().copy_for.as_deref() == Some(conn.id.as_str());
        if copy_open {
            block = block.child(self.render_copy_editor(cx));
        }

        // 展开但未连接（如上次会话遗留的展开态）：不报错，给明下一步指引。
        let loading_here = self.database_nav.borrow().loading.contains(&conn.id);
        if expanded && !connected && children.is_empty() && error.is_none() && !loading_here {
            block = block.child(
                div()
                    .pl_6()
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child("未连接 · 右键「连接」或再次展开"),
            );
        }
        if let Some(err) = error {
            block = block.child(div().pl_6().pb_1().text_xs().text_color(danger).child(err));
        }

        if expanded {
            for child in children {
                block = block.child(self.render_nav_node(&child, 1, &scope_key, cx));
            }
        }

        block
    }

    /// 连接行内联组织编辑器：分组多选（多对多）+「新建分组」+ 标签输入。
    /// 行内**归组**编辑器（右键「移动到分组…」打开）：多选切换 + 新建分组。
    ///
    /// 只处理分组；标签由 [`Self::render_tag_editor`]（行尾 `+`）单独负责。
    fn render_group_editor(
        &self,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let hover = cx.theme().colors.list_hover;
        let accent = cx.theme().colors.primary;
        let bg = cx.theme().colors.popover;

        let (groups, membership) = {
            let view = self.database_nav.borrow();
            (
                view.groups.clone(),
                view.membership.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        let root = self.project_root();

        let mut panel = div()
            .v_flex()
            .w_full()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(div().text_xs().text_color(muted).child("归组"));

        for group in &groups {
            let checked = membership.iter().any(|g| g == &group.id);
            let gid = group.id.clone();
            let cid = conn.id.clone();
            let root = root.clone();
            panel = panel.child(
                div()
                    .id(format!(
                        "nav-org-g-{}::{}::{}",
                        scope_key, group.id, conn.id
                    ))
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .w_3()
                            .flex_none()
                            .text_xs()
                            .text_color(if checked { accent } else { muted })
                            .child(if checked { "\u{2713}" } else { "" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(fg)
                            .child(group.name.clone()),
                    )
                    .on_click({
                        let entity = cx.entity();
                        move |_, _, app: &mut App| {
                            let gid = gid.clone();
                            let cid = cid.clone();
                            let root = root.clone();
                            entity.update(app, |this, cx| {
                                let in_group = this
                                    .database_nav
                                    .borrow()
                                    .membership
                                    .get(&cid)
                                    .map(|gs| gs.iter().any(|g| g == &gid))
                                    .unwrap_or(false);
                                let result = if in_group {
                                    crate::services::nav_runtime::remove_from_group(
                                        root.as_deref(),
                                        &gid,
                                        &cid,
                                    )
                                } else {
                                    crate::services::nav_runtime::add_to_group(
                                        root.as_deref(),
                                        &gid,
                                        &cid,
                                    )
                                };
                                match result {
                                    Ok(()) => this.reload_nav_org(),
                                    Err(e) => {
                                        *this.shared.notice.borrow_mut() =
                                            Some(format!("更新分组失败: {e}"));
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }),
            );
        }

        // 新建分组并直接归入当前连接。
        let root_new = root.clone();
        let cid_new = conn.id.clone();
        panel = panel.child(
            div()
                .id(format!("nav-org-new::{}::{}", scope_key, conn.id))
                .h_flex()
                .items_center()
                .gap_1()
                .px_1()
                .py_0p5()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(
                    div()
                        .w_3()
                        .flex_none()
                        .text_xs()
                        .text_color(accent)
                        .child("+"),
                )
                .child(div().text_xs().text_color(accent).child("新建分组"))
                .on_click({
                    let entity = cx.entity();
                    move |_, _, app: &mut App| {
                        let root = root_new.clone();
                        let cid = cid_new.clone();
                        entity.update(app, |this, cx| {
                            let name = this.next_group_name();
                            match crate::services::nav_runtime::create_group(root.as_deref(), &name)
                            {
                                Ok(gid) => {
                                    let _ = crate::services::nav_runtime::add_to_group(
                                        root.as_deref(),
                                        &gid,
                                        &cid,
                                    );
                                    this.reload_nav_org();
                                }
                                Err(e) => {
                                    *this.shared.notice.borrow_mut() =
                                        Some(format!("新建分组失败: {e}"));
                                }
                            }
                            cx.notify();
                        });
                    }
                }),
        );

        panel
    }

    /// 行内**标签**编辑器（行尾 `+` 打开）：仅标签输入，回车保存。
    fn render_tag_editor(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.popover;
        let mut panel = div()
            .v_flex()
            .w_full()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("标签（逗号分隔，回车保存）"),
            );
        if let Some(input) = &self.nav_tag_input {
            panel = panel.child(Input::new(input));
        }
        panel
    }

    /// 行内「复制为模板」编辑器（右键「复制连接（模板）…」打开）：输入新名，回车提交。
    fn render_copy_editor(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.popover;
        let entity = cx.entity();
        let mut panel = div()
            .v_flex()
            .w_full()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("复制为模板（不带密码；回车提交）"),
            );
        if let Some(input) = &self.nav_copy_input {
            panel = panel.child(Input::new(input));
        }
        panel.child(
            div().h_flex().justify_end().w_full().child(
                Button::new("nav-copy-cancel")
                    .ghost()
                    .small()
                    .label("取消")
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| this.cancel_copy_connection(cx));
                    }),
            ),
        )
    }

    /// 对象树节点行（懒加载；叶子不可展开）。
    ///
    /// `scope_key` 为所属分组标识：同一连接可出现在多个分组，用于生成唯一元素 ID。
    fn render_nav_node(
        &self,
        node: &NavNode,
        depth: usize,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let pri = cx.theme().colors.primary;
        let danger = cx.theme().colors.danger;
        let icon = {
            let theme = cx.theme();
            nav_kind_color(&node.kind, theme)
        };

        let filter = self.database_nav.borrow().filter.to_lowercase();
        let expanded = {
            let view = self.database_nav.borrow();
            view.expanded.contains(&node.key)
        };
        if expanded && node.has_children {
            if let Some(p) = node.expand_path.clone() {
                self.ensure_nav_loaded(&node.connection_id, &node.key, p, false, cx);
            }
        }
        let (skip, expanded_eff, error, children) = {
            let view = self.database_nav.borrow();
            let skip = !filter.is_empty() && !nav_node_matches(&view.children, node, &filter);
            let expanded_now = view.expanded.contains(&node.key);
            (
                skip,
                expanded_now || !filter.is_empty(),
                view.errors.get(&node.key).cloned(),
                view.children.get(&node.key).cloned().unwrap_or_default(),
            )
        };
        if skip {
            return div();
        }

        let entity = cx.entity();
        let n_conn_id = node.connection_id.clone();
        let n_key = node.key.clone();
        let n_path = node.expand_path.clone();
        let n_property = node.property.clone();
        let n_conn_label = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|c| c.id == node.connection_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| node.connection_id.clone());
        let n_driver = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|c| c.id == node.connection_id)
            .map(|c| c.driver.clone())
            .unwrap_or_default();
        // 供右键菜单使用（双击处理器已消费 `n_conn_label` / `n_driver`）。
        let m_conn_label = n_conn_label.clone();
        let m_driver = n_driver.clone();

        let right_meta: Option<(String, bool)> = match &node.kind {
            NavNodeKind::Column {
                data_type,
                primary,
                foreign,
                ..
            } => {
                let mut t = data_type.clone();
                if *primary {
                    t.push_str("  PK");
                }
                if *foreign {
                    t.push_str("  FK");
                }
                Some((t, *primary))
            }
            _ => None,
        };

        // 缩进 = 基础内边距 + 层级 × 步长（设计 §2.1）。两者都是 rem 倍率，
        // 直接交给 `rems()` 换算（其基准是主题字号而非 4px，写 `/ 4.` 会放大 4 倍）。
        let indent = ui::TREE_BASE_PADDING + depth as f32 * ui::TREE_INDENT;
        let selected =
            self.database_nav.borrow().selected_key.as_deref() == Some(node.key.as_str());
        let selected_bg = cx.theme().colors.list_active;
        // 键盘导航序列（父在前，子随递归渲染加入）。
        self.nav_order.borrow_mut().push(NavOrderItem {
            key: node.key.clone(),
            conn_id: node.connection_id.clone(),
            path: node.expand_path.clone(),
            property: node.property.clone(),
            has_children: node.has_children,
            expanded: expanded_eff,
        });
        let mut block = div().v_flex().w_full();
        let mut row =
            div()
                .id(format!("nav-node-{}::{}", scope_key, node.key))
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(1.375))
                .pr_1()
                .pl(rems(indent))
                .gap_1()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |s| s.bg(selected_bg))
                .hover(move |s| s.bg(hover))
                .child(div().w_2p5().flex_none().text_xs().text_color(muted).child(
                    if node.has_children {
                        if expanded_eff { "\u{25be}" } else { "\u{25b8}" }
                    } else {
                        ""
                    },
                ))
                .child(div().w_2().h_2().flex_none().rounded_sm().bg(icon))
                .child(div().flex_1().min_w_0().text_xs().text_color(fg).child(
                    nav_name_highlight(
                        &node.name,
                        &filter,
                        settings::product_tokens::get(cx).search_match_background(cx.theme()),
                        fg,
                    ),
                ));
        if let Some((meta, is_pk)) = right_meta {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(if is_pk { pri } else { muted })
                    .child(meta),
            );
        }
        row = row.on_click({
            let focus = self.focus_handle.clone();
            move |ev, window, app| {
                focus.focus(window, app);
                let sel_key = n_key.clone();
                entity.update(app, |this, cx| {
                    this.database_nav.borrow_mut().selected_key = Some(sel_key.clone());
                    cx.notify();
                });
                if ev.click_count() >= 2 {
                    if let Some(p) = n_property.clone() {
                        let req = PropertyRequest {
                            property: p,
                            conn_label: n_conn_label.clone(),
                            driver: n_driver.clone(),
                        };
                        entity.update(app, |this, cx| {
                            *this.shared.property_target.borrow_mut() = Some(req);
                            cx.notify();
                        });
                        return;
                    }
                }
                if let Some(p) = n_path.clone() {
                    let conn_id = n_conn_id.clone();
                    let key = n_key.clone();
                    entity.update(app, |this, cx| {
                        this.toggle_nav_node(&conn_id, &key, p, cx);
                        cx.notify();
                    });
                }
            }
        });
        // 右键菜单（对象节点）：查看数据 / 属性 / 复制 / 刷新。
        block = block.child(row.context_menu({
            let entity = cx.entity();
            let shared = self.shared.clone();
            let conn_id = node.connection_id.clone();
            let nkey = node.key.clone();
            let dname = node.name.clone();
            let menu_path = node.expand_path.clone();
            let menu_prop = node.property.clone();
            let qualified = node.property.as_ref().map(nav_qualified_name);
            let conn_label = m_conn_label.clone();
            let driver = m_driver.clone();
            let data_like = matches!(&node.kind, NavNodeKind::Table { .. } | NavNodeKind::View);
            // 表 / 视图的「生成 SQL」需要列：由后台任务取（命中 L2 不发查询）。
            let dml_target = match &menu_path {
                Some(NavPath::Table {
                    catalog,
                    schema,
                    table,
                }) if data_like => Some((catalog.clone(), schema.clone(), table.clone())),
                _ => None,
            };
            let dml_root = if dml_target.is_some() {
                self.project_root().map(|p| p.to_string_lossy().to_string())
            } else {
                None
            };
            move |menu, window, cx| {
                let mut menu = menu;
                if let Some(prop0) = menu_prop.clone() {
                    let e = entity.clone();
                    let label0 = conn_label.clone();
                    let drv0 = driver.clone();
                    menu = menu.item(PopupMenuItem::new("查看属性").on_click(move |_, _, app| {
                        let prop = prop0.clone();
                        let label = label0.clone();
                        let drv = drv0.clone();
                        e.update(app, |this, cx| {
                            *this.shared.property_target.borrow_mut() = Some(PropertyRequest {
                                property: prop.clone(),
                                conn_label: label.clone(),
                                driver: drv.clone(),
                            });
                            cx.notify();
                        });
                    }));
                }
                if data_like {
                    if let Some(q) = qualified.clone() {
                        let sql = format!("SELECT * FROM {q} LIMIT 200;");
                        let e = entity.clone();
                        let shared_sql = shared.clone();
                        menu = menu.item(PopupMenuItem::new("查看数据（LIMIT 200）").on_click(
                            move |_, _, app| {
                                *shared_sql.editor_set.borrow_mut() = Some(sql.clone());
                                e.update(app, |_, cx| cx.emit(SidebarEvent::EditorSqlRequest));
                            },
                        ));
                    }
                    // 「生成 SQL ▸」：由列信息生成 INSERT / UPDATE / DELETE 模板（只注入不执行）。
                    if let Some((catalog, schema, table)) = dml_target.clone() {
                        let q = database::sql_gen::qualified_name(
                            Some(catalog.as_str()),
                            Some(schema.as_str()),
                            &table,
                        );
                        let e = entity.clone();
                        let key = nkey.clone();
                        let cid = conn_id.clone();
                        let root = dml_root.clone();
                        menu = menu.submenu("生成 SQL", window, cx, move |m, _w, _c| {
                            let mut m = m;
                            for kind in [DmlKind::Insert, DmlKind::Update, DmlKind::Delete] {
                                let e = e.clone();
                                let key = key.clone();
                                let cid = cid.clone();
                                let root = root.clone();
                                let catalog = catalog.clone();
                                let schema = schema.clone();
                                let table = table.clone();
                                let q = q.clone();
                                m = m.item(PopupMenuItem::new(kind.label()).on_click(
                                    move |_, _, app| {
                                        nav_jobs::enqueue_generate_dml(
                                            &key,
                                            &cid,
                                            root.as_deref(),
                                            &catalog,
                                            &schema,
                                            &table,
                                            &q,
                                            kind,
                                        );
                                        e.update(app, |this, cx| this.ensure_nav_pump(cx));
                                    },
                                ));
                            }
                            m
                        });
                    }
                }
                {
                    let e = entity.clone();
                    let shared_copy = shared.clone();
                    let name_copy = dname.clone();
                    menu = menu.item(PopupMenuItem::new("复制名称").on_click(move |_, _, app| {
                        let name = name_copy.clone();
                        app.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                        *shared_copy.notice.borrow_mut() = Some(format!("已复制：{name}"));
                        e.update(app, |_, cx| cx.notify());
                    }));
                }
                if let Some(q) = qualified.clone() {
                    let e = entity.clone();
                    let shared_copy = shared.clone();
                    menu =
                        menu.item(PopupMenuItem::new("复制限定名").on_click(move |_, _, app| {
                            let q = q.clone();
                            app.write_to_clipboard(ClipboardItem::new_string(q.clone()));
                            *shared_copy.notice.borrow_mut() = Some(format!("已复制：{q}"));
                            e.update(app, |_, cx| cx.notify());
                        }));
                }
                if let Some(p) = menu_path.clone() {
                    let e = entity.clone();
                    let cid = conn_id.clone();
                    let key = nkey.clone();
                    menu =
                        menu.item(PopupMenuItem::new("刷新元数据").on_click(move |_, _, app| {
                            let cid = cid.clone();
                            let key = key.clone();
                            let p = p.clone();
                            e.update(app, |this, cx| {
                                this.refresh_node(&cid, &key, Some(p.clone()), cx);
                            });
                        }));
                }
                // 通用模块入口（所有对象节点都有，与节点类型 / 连接状态无关）：
                // SQL 编辑器 / 洞察；Mock 只针对表 / 视图（`data_like`）。
                menu = menu.separator().item({
                    let e = entity.clone();
                    let cid = conn_id.clone();
                    PopupMenuItem::new("在 SQL 编辑器中打开").on_click(move |_, _, app| {
                        let cid = cid.clone();
                        e.update(app, |_, cx| cx.emit(SidebarEvent::OpenSqlEditor(cid)));
                    })
                });
                if data_like {
                    let e = entity.clone();
                    // 定向请求：连接 + 源库表（catalog / schema / 表名）——Mock 面板据此
                    // 读源库结构并预填目标表名（v1 主路径：源库结构 → 造新数据）。
                    let request = dml_target.clone().map(|(catalog, schema, table)| SchemaRequest {
                        conn_id: conn_id.clone(),
                        catalog,
                        schema,
                        table,
                    });
                    menu = menu.item(PopupMenuItem::new("生成 Mock 数据").on_click(
                        move |_, _, app| {
                            let request = request.clone();
                            e.update(app, |this, cx| {
                                this.shared.open_mock_panel(request, cx);
                            });
                        },
                    ));
                }
                menu = menu.item({
                    let e = entity.clone();
                    PopupMenuItem::new("查看洞察").on_click(move |_, _, app| {
                        e.update(app, |_, cx| {
                            cx.emit(SidebarEvent::OpenRightPanel(RightPanel::Insight));
                        });
                    })
                });
                menu
            }
        }));

        if self.database_nav.borrow().loading.contains(&node.key) {
            block = block.child(
                div()
                    .pl(rems(indent + ui::TREE_INDENT))
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child("加载中…"),
            );
        }
        if let Some(err) = error {
            block = block.child(
                div()
                    .pl(rems(indent + ui::TREE_INDENT))
                    .pb_1()
                    .text_xs()
                    .text_color(danger)
                    .child(err),
            );
        }
        if expanded_eff {
            // 类别文件夹客户端分页：万级对象时只渲染首批，其余用「加载更多」逐页展开。
            let is_folder = matches!(&node.kind, NavNodeKind::Folder(_));
            let total = children.len();
            let limit = if is_folder {
                self.folder_limit(&node.key)
            } else {
                usize::MAX
            };
            for child in children.iter().take(limit) {
                block = block.child(self.render_nav_node(child, depth + 1, scope_key, cx));
            }
            if is_folder && total > limit {
                block = block.child(self.render_more_row(&node.key, total - limit, depth, cx));
            }
        }
        block
    }

    /// 「加载更多」行（大 schema 客户端分页；点击追加一页，不重查远端）。
    fn render_more_row(
        &self,
        key: &str,
        remaining: usize,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pri = cx.theme().colors.primary;
        let indent = ui::TREE_BASE_PADDING + (depth as f32 + 1.0) * ui::TREE_INDENT;
        let entity = cx.entity();
        let k = key.to_string();
        div()
            .id(format!("nav-more-{key}"))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.375))
            .pl(rems(indent))
            .pr_1()
            .rounded_md()
            .cursor_pointer()
            .text_xs()
            .text_color(pri)
            .child(format!("加载更多（余 {remaining}）"))
            .on_click(move |_, _, app| {
                let k = k.clone();
                entity.update(app, |this, cx| {
                    let cur = {
                        let view = this.database_nav.borrow();
                        view.page_limit
                            .get(&k)
                            .copied()
                            .unwrap_or(ui::NAV_FOLDER_PAGE_SIZE)
                    };
                    this.database_nav
                        .borrow_mut()
                        .page_limit
                        .insert(k.clone(), cur + ui::NAV_FOLDER_PAGE_SIZE);
                    cx.notify();
                });
            })
    }

    /// 展开 / 折叠节点；首次展开时排队后台懒加载，并持久化展开态。
    fn toggle_nav_node(&mut self, conn_id: &str, key: &str, path: NavPath, cx: &mut Context<Self>) {
        let now_expanded = {
            let mut view = self.database_nav.borrow_mut();
            if view.expanded.contains(key) {
                view.expanded.remove(key);
                false
            } else {
                view.expanded.insert(key.to_string());
                true
            }
        };
        if now_expanded {
            // 连接根展开：未建连则先建连。否则 `NavigatorService` → `MetadataService`
            // 取不到运行时句柄，冒泡为 `[CONN_NOT_FOUND]`（用户看到的“连不上”）。
            if matches!(path, NavPath::Connection) && !self.ensure_connected_for_browse(conn_id, cx)
            {
                self.save_nav_state_for(conn_id);
                return;
            }
            self.ensure_nav_loaded(conn_id, key, path, false, cx);
        }
        self.save_nav_state_for(conn_id);
    }

    /// 展开前的隐式建连：未连接时先建连；返回是否可用（已连接 或 建连成功）。
    ///
    /// 失败时写面板提示（与 `toggle_connection` 同文案），不阻后续可重试。
    fn ensure_connected_for_browse(&mut self, conn_id: &str, cx: &mut Context<Self>) -> bool {
        let already = crate::services::nav_runtime::is_connected(conn_id)
            || self.database_nav.borrow().connected.contains(conn_id);
        if already {
            return true;
        }
        let root = self.project_root().map(|p| p.to_string_lossy().to_string());
        match crate::services::nav_runtime::connect_entry(conn_id, root.as_deref()) {
            Ok(()) => {
                {
                    let mut view = self.database_nav.borrow_mut();
                    view.connected.insert(conn_id.to_string());
                    view.prefetched.clear();
                    // 清掉上一次的负载错误（如 CONN_NOT_FOUND），以便重试加载。
                    view.errors.remove(conn_id);
                }
                nav_jobs::warm_after_connect(conn_id, root.as_deref());
                self.ensure_warm_poll(cx);
                true
            }
            Err(e) => {
                *self.shared.notice.borrow_mut() = Some(format!("连接失败: {e}"));
                false
            }
        }
    }

    /// 排队后台懒加载子节点（已请求过则跳过；render 路径不做 I/O）。
    ///
    /// `fresh`（刷新模式）跳过 L2 读缓存并重写。结果由 [`Self::apply_load_results`] 回填。
    fn ensure_nav_loaded(
        &self,
        conn_id: &str,
        key: &str,
        path: NavPath,
        fresh: bool,
        cx: &mut Context<Self>,
    ) {
        {
            let view = self.database_nav.borrow();
            if view.attempted.contains(key) {
                return;
            }
        }
        {
            let mut view = self.database_nav.borrow_mut();
            view.attempted.insert(key.to_string());
            view.loading.insert(key.to_string());
        }
        let project_root = self.project_root().map(|p| p.to_string_lossy().to_string());
        nav_jobs::enqueue_load(conn_id, project_root.as_deref(), key, path, fresh);
        self.ensure_nav_pump(cx);
    }

    /// 启动加载结果轮询（已有存活任务时不重复启动）。
    fn ensure_nav_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.nav_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let results = nav_jobs::drain_load_results();
                let had = !results.is_empty();
                if had
                    && weak
                        .update(cx, |this, cx| this.apply_load_results(results, cx))
                        .is_err()
                {
                    return;
                }
                // 生成 SQL / 测试连接：同一轮询泵回填（两者都可能在菜单触发）。
                let sql_results = nav_jobs::drain_sql_results();
                if !sql_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_sql_results(sql_results, cx))
                        .is_err()
                {
                    return;
                }
                let test_results = nav_jobs::drain_test_results();
                if !test_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_test_results(test_results, cx))
                        .is_err()
                {
                    return;
                }
                let idle = !nav_jobs::has_pending_loads()
                    && !nav_jobs::has_pending_sql()
                    && !nav_jobs::has_pending_test();
                if idle {
                    // 多等一拍确认没有新任务（render 可能刚入队）。
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !nav_jobs::has_pending_loads()
                        && !nav_jobs::has_pending_sql()
                        && !nav_jobs::has_pending_test()
                    {
                        break;
                    }
                }
            }
        });
        *self.nav_pump.borrow_mut() = Some(task);
    }

    /// 回填后台加载结果（主线程）。
    fn apply_load_results(&mut self, results: Vec<nav_jobs::LoadResult>, cx: &mut Context<Self>) {
        for r in results {
            let key = r.key.clone();
            let is_tables_folder = matches!(
                &r.path,
                NavPath::Folder {
                    folder: NavFolder::Tables,
                    ..
                }
            );
            let (catalog, schema) = match &r.path {
                NavPath::Folder {
                    catalog, schema, ..
                } => (catalog.clone(), schema.clone()),
                _ => (String::new(), String::new()),
            };
            let conn_id = r.conn_id.clone();
            let project_root = r.project_root.clone();
            {
                let mut view = self.database_nav.borrow_mut();
                view.loading.remove(&key);
                match r.result {
                    Ok(children) => {
                        view.errors.remove(&key);
                        view.children.insert(key.clone(), children);
                    }
                    Err(e) => {
                        view.errors.insert(key.clone(), e);
                    }
                }
            }
            // C2：「表」文件夹首次加载成功后，排队预取前 N 张表的列。
            if is_tables_folder {
                self.maybe_prefetch(&conn_id, &key, &catalog, &schema, project_root.as_deref());
            }
        }
        cx.notify();
    }

    /// 回填「生成 SQL」结果：成功注入编辑区（追加在草稿后），失败落提示。
    fn apply_sql_results(&mut self, results: Vec<nav_jobs::SqlGenResult>, cx: &mut Context<Self>) {
        for r in results {
            match r.result {
                Ok(sql) => {
                    *self.shared.editor_set.borrow_mut() = Some(sql);
                    cx.emit(SidebarEvent::EditorSqlRequest);
                }
                Err(e) => {
                    *self.shared.notice.borrow_mut() = Some(format!("生成 SQL 失败：{e}"));
                }
            }
        }
        cx.notify();
    }

    /// 回填「测试连接」结果（结果文案加连接名前缀，直接落面板提示）。
    fn apply_test_results(
        &mut self,
        results: Vec<nav_jobs::TestConnResult>,
        cx: &mut Context<Self>,
    ) {
        for r in results {
            let msg = match r.result {
                Ok(m) => format!("{}：{m}", r.name),
                Err(e) => format!("{}：{e}", r.name),
            };
            *self.shared.notice.borrow_mut() = Some(msg);
        }
        cx.notify();
    }

    /// C2：对刚加载完的「表」文件夹排队列预取（一次性）。
    fn maybe_prefetch(
        &self,
        conn_id: &str,
        key: &str,
        catalog: &str,
        schema: &str,
        project_root: Option<&str>,
    ) {
        let first_time = self
            .database_nav
            .borrow_mut()
            .prefetched
            .insert(key.to_string());
        if !first_time {
            return;
        }
        let targets: Vec<nav_jobs::ColumnTarget> = {
            let view = self.database_nav.borrow();
            view.children
                .get(key)
                .map(|kids| {
                    kids.iter()
                        .filter(|n| matches!(n.kind, NavNodeKind::Table { .. }))
                        .take(nav_jobs::PREFETCH_BATCH)
                        .map(|n| nav_jobs::ColumnTarget {
                            catalog: catalog.to_string(),
                            schema: schema.to_string(),
                            table: n.name.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        nav_jobs::prefetch_columns(conn_id, project_root, targets);
    }

    /// 当前项目根（项目 / 共享连接的导航状态落项目库）。
    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.shared
            .project
            .borrow()
            .as_ref()
            .map(|p| p.root.clone())
    }

    /// 首次渲染某连接时，从库中恢复其展开态。
    fn ensure_nav_state_loaded(&self, conn_id: &str) {
        {
            let view = self.database_nav.borrow();
            if view.state_loaded.contains(conn_id) {
                return;
            }
        }
        let state =
            crate::services::nav_runtime::load_nav_state(conn_id, self.project_root().as_deref());
        let prefix = format!("{conn_id}/");
        let mut view = self.database_nav.borrow_mut();
        for key in state.expanded_keys {
            if key == conn_id || key.starts_with(&prefix) {
                view.expanded.insert(key);
            }
        }
        view.state_loaded.insert(conn_id.to_string());
    }

    /// 持久化某连接的展开态。
    fn save_nav_state_for(&self, conn_id: &str) {
        let prefix = format!("{conn_id}/");
        let keys: Vec<String> = {
            let view = self.database_nav.borrow();
            view.expanded
                .iter()
                .filter(|k| *k == conn_id || k.starts_with(&prefix))
                .cloned()
                .collect()
        };
        let state = database::model::NavState {
            expanded_keys: keys,
            ..Default::default()
        };
        let _ = crate::services::nav_runtime::save_nav_state(
            conn_id,
            self.project_root().as_deref(),
            &state,
        );
    }

    /// 重载分组 / 成员关系 / 标签映射（组织变更后调用）。
    fn reload_nav_org(&self) {
        let root = self.project_root();
        let groups = crate::services::nav_runtime::list_groups(root.as_deref());
        let mut membership: HashMap<String, Vec<String>> = HashMap::new();
        let mut group_order: HashMap<String, Vec<String>> = HashMap::new();
        for group in &groups {
            let ids = crate::services::nav_runtime::list_group_members(root.as_deref(), &group.id);
            for cid in &ids {
                membership
                    .entry(cid.clone())
                    .or_default()
                    .push(group.id.clone());
            }
            group_order.insert(group.id.clone(), ids);
        }
        let tags = crate::services::nav_runtime::list_all_tags(root.as_deref());
        let primary_group = crate::services::nav_runtime::list_primary_groups(root.as_deref());
        let driver_catalog = crate::services::nav_runtime::driver_catalog();
        *self.shared.driver_catalog.borrow_mut() = driver_catalog;
        let mut view = self.database_nav.borrow_mut();
        view.groups = groups;
        view.membership = membership;
        view.group_order = group_order;
        view.primary_group = primary_group;
        view.tags = tags;
        view.groups_loaded = true;
    }

    /// 分组是否折叠（缺省展开）。
    fn group_collapsed(&self, group_id: &str) -> bool {
        self.database_nav
            .borrow()
            .collapsed_groups
            .contains(group_id)
    }

    /// 类别文件夹当前渲染条数上限（缺省 `NAV_FOLDER_PAGE_SIZE`）。
    fn folder_limit(&self, key: &str) -> usize {
        self.database_nav
            .borrow()
            .page_limit
            .get(key)
            .copied()
            .unwrap_or(ui::NAV_FOLDER_PAGE_SIZE)
    }

    /// 生成不与现有分组重名的默认分组名。
    fn next_group_name(&self) -> String {
        let groups = self.database_nav.borrow().groups.clone();
        if !groups.iter().any(|g| g.name == "新建分组") {
            return "新建分组".to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("新建分组 {n}");
            if !groups.iter().any(|g| g.name == candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// 提交连接行内联标签输入（逗号分隔 → 覆盖式保存）。
    fn commit_nav_tags(&mut self, cx: &mut Context<Self>) {
        let Some(conn_id) = self.database_nav.borrow().tag_editor_for.clone() else {
            return;
        };
        let Some(input) = self.nav_tag_input.clone() else {
            return;
        };
        let text = input.read(cx).value().to_string();
        let tags: Vec<String> = text
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let root = self.project_root();
        match crate::services::nav_runtime::set_tags(&conn_id, root.as_deref(), &tags) {
            Ok(()) => self.reload_nav_org(),
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("保存标签失败: {e}")),
        }
        cx.notify();
    }

    /// 新建分组（默认名自动去重）并重载组织数据。
    fn create_group_interactive(&self, cx: &mut Context<Self>) {
        let name = self.next_group_name();
        let root = self.project_root();
        match crate::services::nav_runtime::create_group(root.as_deref(), &name) {
            Ok(_) => self.reload_nav_org(),
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("新建分组失败: {e}")),
        }
        cx.notify();
    }

    /// 删除分组（仅解除关系，不删成员连接与缓存）。
    fn delete_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let root = self.project_root();
        match crate::services::nav_runtime::delete_group(root.as_deref(), group_id) {
            Ok(()) => {
                self.reload_nav_org();
                *self.shared.notice.borrow_mut() = Some("分组已删除（成员连接保留）".to_string());
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("删除分组失败: {e}")),
        }
        cx.notify();
    }

    /// 提交行内「复制为模板」：成功后重载连接列表，并提示新名（不含密码）。
    fn commit_copy_connection(&mut self, cx: &mut Context<Self>) {
        let Some(from_id) = self.database_nav.borrow().copy_for.clone() else {
            return;
        };
        let Some(input) = self.nav_copy_input.clone() else {
            return;
        };
        let new_name = input.read(cx).value().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let root = self.project_root().map(|p| p.to_string_lossy().to_string());
        let result = (|| -> Result<(), String> {
            let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
            let service = crate::services::data_source_service::DataSourceService::global()
                .map_err(|e| e.to_string())?;
            rt.block_on(service.duplicate_as_template(&from_id, root.as_deref(), &new_name))
                .map(|_| ())
                .map_err(|e| e.to_string())
        })();
        self.database_nav.borrow_mut().copy_for = None;
        match result {
            Ok(()) => {
                self.reload_connections(cx);
                *self.shared.notice.borrow_mut() =
                    Some(format!("已复制为模板：「{new_name}」（不含密码）"));
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("复制失败：{e}")),
        }
        cx.notify();
    }

    /// 取消行内「复制为模板」。
    fn cancel_copy_connection(&mut self, cx: &mut Context<Self>) {
        self.database_nav.borrow_mut().copy_for = None;
        cx.notify();
    }

    /// 共享至当前项目（`G_` → 项目侧 `GP_` 快照）。
    fn share_connection_to_project(&mut self, conn_id: &str, name: &str, cx: &mut Context<Self>) {
        let Some(root) = self.project_root().map(|p| p.to_string_lossy().to_string()) else {
            *self.shared.notice.borrow_mut() = Some("未打开项目：无法共享".to_string());
            cx.notify();
            return;
        };
        let result = (|| -> Result<(), String> {
            let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
            let service = crate::services::data_source_service::DataSourceService::global()
                .map_err(|e| e.to_string())?;
            rt.block_on(service.share_to_project(conn_id, &root))
                .map(|_| ())
                .map_err(|e| e.to_string())
        })();
        match result {
            Ok(()) => {
                self.reload_connections(cx);
                *self.shared.notice.borrow_mut() = Some(format!("「{name}」已共享至当前项目"));
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("共享失败：{e}")),
        }
        cx.notify();
    }

    /// 取消共享：删除项目侧 `GP_` 快照（全局定义保留）。
    fn unshare_connection_from_project(
        &mut self,
        conn_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let root = self.project_root().map(|p| p.to_string_lossy().to_string());
        let result = (|| -> Result<(), String> {
            let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
            let service = crate::services::data_source_service::DataSourceService::global()
                .map_err(|e| e.to_string())?;
            rt.block_on(service.delete(conn_id, root.as_deref()))
                .map(|_| ())
                .map_err(|e| e.to_string())
        })();
        match result {
            Ok(()) => {
                crate::services::nav_runtime::disconnect_entry(conn_id).ok();
                self.reload_connections(cx);
                *self.shared.notice.borrow_mut() =
                    Some(format!("已取消共享：「{name}」（全局定义保留）"));
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("取消共享失败：{e}")),
        }
        cx.notify();
    }

    /// 删除连接（物理删除；元数据缓存保留）。
    fn delete_connection(&mut self, conn_id: &str, name: &str, cx: &mut Context<Self>) {
        let root = self.project_root().map(|p| p.to_string_lossy().to_string());
        let result = (|| -> Result<String, String> {
            let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
            let service = crate::services::data_source_service::DataSourceService::global()
                .map_err(|e| e.to_string())?;
            let r = rt
                .block_on(service.delete(conn_id, root.as_deref()))
                .map_err(|e| e.to_string())?;
            Ok(r.message)
        })();
        // 运行时连接一并断开（缓存保留，可在「缓存管理」清理）。
        crate::services::nav_runtime::disconnect_entry(conn_id).ok();
        match result {
            Ok(msg) => {
                self.reload_connections(cx);
                *self.shared.notice.borrow_mut() = Some(format!("「{name}」：{msg}"));
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("删除连接失败：{e}")),
        }
        cx.notify();
    }

    /// 重载当前作用域可见连接（增 / 删 / 共享后调用）。
    fn reload_connections(&mut self, cx: &mut Context<Self>) {
        let root = self.project_root();
        let (items, notice) =
            crate::services::workspace_loader::load_connections_for_scope(root.as_deref());
        let len = items.len();
        *self.shared.connections.borrow_mut() = items;
        if len == 0 {
            self.shared.selected.set(None);
        } else if self.shared.selected.get().is_some_and(|i| i >= len) {
            self.shared.selected.set(Some(0));
        }
        if let Some(n) = notice {
            *self.shared.notice.borrow_mut() = Some(n);
        }
        cx.notify();
    }

    /// 提交分组内联重命名。
    fn commit_group_rename(&mut self, cx: &mut Context<Self>) {
        let Some(group_id) = self.database_nav.borrow().group_rename_for.clone() else {
            return;
        };
        let Some(input) = self.nav_group_input.clone() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if !name.is_empty() {
            let root = self.project_root();
            match crate::services::nav_runtime::rename_group(root.as_deref(), &group_id, &name) {
                Ok(()) => self.reload_nav_org(),
                Err(e) => *self.shared.notice.borrow_mut() = Some(format!("重命名失败: {e}")),
            }
        }
        self.database_nav.borrow_mut().group_rename_for = None;
        cx.notify();
    }

    /// 连接 / 断开运行时连接（保留缓存）。连接状态取运行时真值。
    fn toggle_connection(
        &mut self,
        conn_id: &str,
        project_root: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let already = crate::services::nav_runtime::is_connected(conn_id)
            || self.database_nav.borrow().connected.contains(conn_id);
        let outcome = if already {
            crate::services::nav_runtime::disconnect_entry(conn_id).map(|_| false)
        } else {
            crate::services::nav_runtime::connect_entry(conn_id, project_root).map(|_| true)
        };
        match outcome {
            Ok(is_connected) => {
                {
                    let mut view = self.database_nav.borrow_mut();
                    if is_connected {
                        view.connected.insert(conn_id.to_string());
                        // 刷新模式下预取过的标记清空（重新连接后重新预取）。
                        view.prefetched.clear();
                    } else {
                        view.connected.remove(conn_id);
                    }
                }
                if is_connected {
                    // C1：连接成功后提交后台预热（仅 catalogs/schemas），不阻塞 UI。
                    nav_jobs::warm_after_connect(conn_id, project_root);
                    self.ensure_warm_poll(cx);
                }
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("连接操作失败: {e}")),
        }
        cx.notify();
    }

    /// 启动预热进度轮询（已有存活任务时不重复启动）。
    ///
    /// 预热在工作线程上跑，进度是原子量；这里用主线程 async 任务每 300ms 轮询并重绘，
    /// 结束后再重绘一次（让「预热中」指示消失）。
    fn ensure_warm_poll(&mut self, cx: &mut Context<Self>) {
        if let Some(task) = &self.warm_poll {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            // 预热任务在队列里异步启动，`warm_active` 置位有延迟；
            // 因此用「连续两次非活动才退出」的判据，同时覆盖启动与结束。
            let mut idle = 0;
            loop {
                executor.timer(std::time::Duration::from_millis(300)).await;
                if nav_jobs::warm_active() {
                    idle = 0;
                } else {
                    idle += 1;
                    if idle >= 2 {
                        break;
                    }
                }
                if weak.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        self.warm_poll = Some(task);
    }

    /// 刷新某节点（含连接根）的元数据：清掉已缓存子节点后重新加载当前层级。
    ///
    /// 保留展开态：子节点重新渲染时会根据最新 `expand_path` 再次懒加载。缓存文件不删。
    fn refresh_node(
        &self,
        conn_id: &str,
        key: &str,
        path: Option<NavPath>,
        cx: &mut Context<Self>,
    ) {
        let prefix = format!("{key}/");
        {
            let mut view = self.database_nav.borrow_mut();
            view.children
                .retain(|k, _| k != key && !k.starts_with(&prefix));
            view.attempted
                .retain(|k| k != key && !k.starts_with(&prefix));
            view.errors
                .retain(|k, _| k != key && !k.starts_with(&prefix));
        }
        if let Some(p) = path {
            self.ensure_nav_loaded(conn_id, key, p, true, cx);
        }
        *self.shared.notice.borrow_mut() = Some("已刷新元数据".to_string());
        cx.notify();
    }

    /// 刷新全部连接的元数据（清掉已加载子节点后重载仍展开的连接根）。
    fn refresh_all(&self, cx: &mut Context<Self>) {
        let roots: Vec<String> = {
            let view = self.database_nav.borrow();
            self.shared
                .connections
                .borrow()
                .iter()
                .filter(|c| view.expanded.contains(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        {
            let mut view = self.database_nav.borrow_mut();
            view.children.clear();
            view.attempted.clear();
            view.errors.clear();
        }
        for cid in roots {
            self.ensure_nav_loaded(&cid, &cid, NavPath::Connection, true, cx);
        }
        *self.shared.notice.borrow_mut() = Some("已刷新全部元数据".to_string());
        cx.notify();
    }

    fn render_resources_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("分析资源（下一轮接入）"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 数据源连接引用"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· DuckDB 分析表"),
            )
    }

    /// 渲染草稿树单行（选中态 / 行内操作 / 右键菜单 / 展开时触发懒加载）。
    ///
    /// 同时被普通渲染与虚拟列表闭包调用，行索引按 `ctx.rows` 全局序号。
    fn scratchpad_row(&self, display: usize, ctx: &ScratchpadRowCtx, cx: &mut Context<Self>) -> AnyElement {
        // 内联新建行：插在目标文件夹首行位置（未选中文件夹时在模块根）。
        if ctx.is_edit_row(display) {
            return self
                .render_scratchpad_edit_row(ctx.edit_row_depth(display), cx)
                .into_any_element();
        }
        let Some(real) = ctx.real_index(display) else {
            return div().into_any_element();
        };
        let Some((depth, entry)) = ctx.rows.get(real) else {
            return div().into_any_element();
        };
        let colors = ctx.colors;
        let ScratchpadRowColors {
            hover_bg,
            selected_bg,
            fg,
            muted,
            folder_color,
            primary,
            info,
            success,
            active_border,
        } = colors;
        let key = entry.path.to_string_lossy().to_string();

        // 本行正在重命名 → 渲染内联输入。
        if let Some(ScratchpadEdit::Rename { path }) = &ctx.edit {
            if path == &key {
                return self.render_scratchpad_edit_row(*depth, cx).into_any_element();
            }
        }

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let is_folder = entry.kind == ScratchpadEntryKind::Folder;
        let is_selected = ctx.selected.contains(&key);
        let is_expanded = ctx.expanded.contains(&key);
        // 展开且尚未懒加载过子目录 → 触发加载。
        let needs_load =
            is_folder && !ctx.loaded.contains_key(&key) && entry.children.is_none();

        let click = {
            let view = view_handle.clone();
            let entity = entity.clone();
            let key = key.clone();
            let load_key = key.clone();
            let keys = ctx.keys.clone();
            let position = real;
            move |ev: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                let modifiers = ev.modifiers();
                let mut should_load = false;
                {
                    let mut v = view.borrow_mut();
                    if modifiers.shift {
                        if let Some(anchor) = v.anchor.clone() {
                            if let Some(a) = keys.iter().position(|k| k == &anchor) {
                                let (lo, hi) = if a <= position {
                                    (a, position)
                                } else {
                                    (position, a)
                                };
                                v.selected = keys[lo..=hi].iter().cloned().collect();
                            }
                        } else {
                            v.selected.clear();
                            v.selected.insert(key.clone());
                            v.anchor = Some(key.clone());
                        }
                    } else if modifiers.control {
                        if !v.selected.remove(&key) {
                            v.selected.insert(key.clone());
                        }
                        v.anchor = Some(key.clone());
                    } else {
                        v.selected.clear();
                        v.selected.insert(key.clone());
                        v.anchor = Some(key.clone());
                        if is_folder {
                            if v.expanded.contains(&key) {
                                v.expanded.remove(&key);
                            } else {
                                v.expanded.insert(key.clone());
                                should_load = needs_load;
                            }
                        }
                    }
                }
                // 点击即聚焦面板，使 Ctrl+A 等面板快捷键生效。
                entity.update(app, |this, cx| {
                    this.focus_handle.clone().focus(window, cx);
                    if should_load {
                        this.request_scratchpad_dir(load_key.clone(), cx);
                    } else {
                        cx.notify();
                    }
                });
            }
        };

        let chevron = if is_folder {
            if is_expanded { "▾" } else { "▸" }
        } else {
            ""
        };
        let icon_color = if is_folder {
            folder_color
        } else {
            scratchpad_icon_color(&entry.name, info, success, primary, muted)
        };

        let mut row = div()
            .id(format!("sp-row-{key}"))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::ROW_HEIGHT))
            .gap_1()
            .rounded_sm()
            .relative()
            .cursor_pointer()
            .when(is_selected, |this| this.bg(selected_bg))
            .hover(move |s| s.bg(hover_bg))
            .on_click(click)
            // 选中左侧 2px 品牌色条（原型 §3）。
            .when(is_selected, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(rems(0.))
                        .top(ui::TREE_ACTIVE_BAR_INSET)
                        .bottom(ui::TREE_ACTIVE_BAR_INSET)
                        .w(ui::TREE_ACTIVE_BAR)
                        .rounded_sm()
                        .bg(active_border),
                )
            })
            .child(div().w(rems(*depth as f32 * ui::TREE_INDENT)).flex_none())
            .child(
                div()
                    .w_2p5()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(chevron),
            )
            .child(div().w_2().h_2().flex_none().rounded_sm().bg(icon_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(fg)
                    .overflow_hidden()
                    .child(entry.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(scratchpad_meta_label(entry)),
            );

        // 选中行才显示操作（打开位置 / 重命名 / 删除）。
        if is_selected {
            let open_location = {
                let entity = entity.clone();
                let key = key.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.open_scratchpad_location(key.clone(), cx)
                    });
                }
            };
            let rename = {
                let entity = entity.clone();
                let key = key.clone();
                move |_: &gpui_kit::ClickEvent,
                      window: &mut gpui_kit::Window,
                      app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.start_scratchpad_edit(
                            ScratchpadEdit::Rename { path: key.clone() },
                            window,
                            cx,
                        )
                    });
                }
            };
            let delete = {
                let entity = entity.clone();
                let key = key.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.delete_scratchpad_entry(key.clone(), cx)
                    });
                }
            };
            row = row
                .child(
                    div()
                        .id(format!("sp-open-{key}"))
                        .w(rems(1.125))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("↗")
                        .on_click(open_location),
                )
                .child(
                    div()
                        .id(format!("sp-ren-{key}"))
                        .w(rems(1.125))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("✎")
                        .on_click(rename),
                )
                .child(
                    div()
                        .id(format!("sp-del-{key}"))
                        .w(rems(1.125))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("✕")
                        .on_click(delete),
                );
        }

        // 右键菜单（打开位置 / 重命名 / 剪切 / 复制 / 删除）。
        {
            let menu_entity = entity.clone();
            let menu_key = key.clone();
            row.context_menu(move |menu, _window, _cx| {
                let open_entity = menu_entity.clone();
                let open_key = menu_key.clone();
                let rename_entity = menu_entity.clone();
                let rename_key = menu_key.clone();
                let cut_entity = menu_entity.clone();
                let cut_key = menu_key.clone();
                let copy_entity = menu_entity.clone();
                let copy_key = menu_key.clone();
                let del_entity = menu_entity.clone();
                let del_key = menu_key.clone();
                menu.item(PopupMenuItem::new("打开位置").on_click(move |_, _, app| {
                    open_entity.update(app, |this, cx| {
                        this.open_scratchpad_location(open_key.clone(), cx)
                    });
                }))
                .item(
                    PopupMenuItem::new("重命名").on_click(move |_, window, app| {
                        rename_entity.update(app, |this, cx| {
                            this.start_scratchpad_edit(
                                ScratchpadEdit::Rename {
                                    path: rename_key.clone(),
                                },
                                window,
                                cx,
                            )
                        });
                    }),
                )
                .separator()
                .item(PopupMenuItem::new("剪切").on_click(move |_, _, app| {
                    cut_entity.update(app, |this, cx| {
                        this.scratchpad.borrow_mut().selected =
                            std::iter::once(cut_key.clone()).collect();
                        this.set_scratchpad_clipboard(ScratchpadClipboardMode::Cut, cx);
                    });
                }))
                .item(PopupMenuItem::new("复制").on_click(move |_, _, app| {
                    copy_entity.update(app, |this, cx| {
                        this.scratchpad.borrow_mut().selected =
                            std::iter::once(copy_key.clone()).collect();
                        this.set_scratchpad_clipboard(ScratchpadClipboardMode::Copy, cx);
                    });
                }))
                .separator()
                .item(PopupMenuItem::new("删除").on_click(move |_, _, app| {
                    del_entity.update(app, |this, cx| { this.delete_scratchpad_entry(del_key.clone(), cx) });
                }))
            })
            .into_any_element()
        }
    }

    /// 草稿树行高：内联编辑行用控件高，其余用树行高（与虚拟列表 `item_sizes` 保持一致）。
    ///
    /// `rem` = 当前窗口的 `rem_size`（`v_virtual_list` 需要 `Pixels`，而尺寸常量是 rem 倍率）。
    fn scratchpad_row_height(ctx: &ScratchpadRowCtx, display: usize, rem: Pixels) -> Pixels {
        let controls_high = if ctx.is_edit_row(display) {
            true
        } else {
            let renaming = match (ctx.real_index(display).and_then(|i| ctx.keys.get(i)), &ctx.edit) {
                (Some(key), Some(ScratchpadEdit::Rename { path })) => path == key,
                _ => false,
            };
            renaming
        };
        if controls_high {
            rems(ui::CONTROL_HEIGHT_SM).to_pixels(rem)
        } else {
            rems(ui::ROW_HEIGHT).to_pixels(rem)
        }
    }

    /// 内联编辑行（新建 / 重命名通用）。
    ///
    /// 新建文件时额外渲染一行模板 chip（原型 §4.1：自动补后缀 + 填充占位内容）。
    fn render_scratchpad_edit_row(&self, depth: usize, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.sidebar_accent;
        let fg = theme.colors.foreground;

        let Some(input) = self.scratchpad.borrow().name_input.clone() else {
            return div();
        };
        let entity = cx.entity();
        let edit = self.scratchpad.borrow().edit.clone();
        let new_template = self.scratchpad.borrow().new_template;

        let commit = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.commit_scratchpad_edit(window, cx));
            }
        };
        let cancel = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.cancel_scratchpad_edit(cx));
            }
        };

        let row = div()
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.625))
            .gap_1()
            .px_1()
            .child(div().w(rems(depth as f32 * ui::TREE_INDENT)).flex_none())
            .child(div().w_2p5().flex_none())
            .child(div().flex_1().min_w_0().child(Input::new(&input).w_full()))
            .child(
                div()
                    .id("sp-edit-ok")
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .w_5()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(primary)
                    .child("✓")
                    .on_click(commit),
            )
            .child(
                div()
                    .id("sp-edit-cancel")
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .w_5()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(muted)
                    .child("✕")
                    .on_click(cancel),
            );

        // 仅新建文件时展示模板 chip 行。
        if !matches!(edit, Some(ScratchpadEdit::NewFile)) {
            return row;
        }
        let mut chips = div().h_flex().items_center().gap_1().w_full().pl_1();
        for template in ScratchpadTemplate::ALL {
            let on = template == new_template;
            let handler = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.set_scratchpad_template(template, window, cx)
                    });
                }
            };
            chips = chips.child(
                div()
                    .id(format!("sp-tpl-{}", template.label()))
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .px_1()
                    .h_5()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if on { fg } else { muted })
                    .when(on, |this| this.bg(selected_bg))
                    .hover(move |s| s.bg(hover_bg))
                    .child(template.label())
                    .on_click(handler),
            );
        }
        div()
            .v_flex()
            .w_full()
            .gap_0p5()
            .py_0p5()
            .child(row)
            .child(chips)
    }

    /// 切换新建文件模板（并把已输入名字的后缀跟着换）。
    fn set_scratchpad_template(
        &mut self,
        template: ScratchpadTemplate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scratchpad.borrow_mut().new_template = template;
        if let Some(input) = self.scratchpad.borrow().name_input.clone() {
            let current = input.read(cx).value().trim().to_string();
            if !current.is_empty() {
                let next = scratchpad_apply_template_ext(&current, template);
                if next != current {
                    input.update(cx, |s, cx| s.set_value(next, window, cx));
                }
            }
        }
        cx.notify();
    }

    /// 草稿树空态（原型 §2.3）：大图标 + 标题 + 说明 + 「新建」「导入」双按钮。
    ///
    /// `filter` 非空表示是「搜索无结果」而不是「真的没有草稿」。
    fn render_scratchpad_empty_state(
        &self,
        entity: &Entity<Self>,
        filter: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let fg = theme.colors.foreground;
        let icon_color = theme.colors.border;
        let entity = entity.clone();

        if !filter.is_empty() {
            return div()
                .v_flex()
                .items_center()
                .w_full()
                .pt_6()
                .px_2()
                .text_xs()
                .text_color(muted)
                .child("没有匹配的文件");
        }

        let new_file = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                });
            }
        };
        let new_folder = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFolder, window, cx)
                });
            }
        };
        let import = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.pick_scratchpad_imports(window, cx));
            }
        };

        div()
            .v_flex()
            .items_center()
            .w_full()
            .pt_8()
            .px_2()
            .gap_2()
            .child(
                div()
                    .text_color(icon_color)
                    .text_size(rems(ui::SCRATCHPAD_EMPTY_ICON_SIZE))
                    .child("🗒"),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("草稿箱是空的"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("随手写点东西，或从外部导入文件"),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("sp-empty-new")
                            .small()
                            .primary()
                            .label("＋ 新建")
                            .on_click(new_file),
                    )
                    .child(
                        Button::new("sp-empty-folder")
                            .small()
                            .label("🗀 文件夹")
                            .on_click(new_folder),
                    )
                    .child(
                        Button::new("sp-empty-import")
                            .small()
                            .label("⬇ 导入")
                            .on_click(import),
                    ),
            )
    }

    /// 打开系统文件对话框并导入所选文件（工具栏「⬇」与空态「导入」共用）。
    fn pick_scratchpad_imports(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择要导入的文件".into()),
        });
        window
            .spawn(cx, async move |cx| {
                if let Ok(Ok(Some(paths))) = receiver.await {
                    let _ = cx.update(|_window, cx| {
                        entity.update(cx, |this, cx| this.import_scratchpad_files(paths, cx));
                    });
                }
            })
            .detach();
    }

    /// 草稿箱面板（M5）：根 = `{project}/scratchpad/`。
    ///
    /// 闭环：新建（内联）/重命名/删除→回收站+撤销栏/回收站恢复与清空/文件名过滤/外部引用移除。
    fn render_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if !self.scratchpad.borrow().loaded {
            self.request_scratchpad_load(cx);
        }
        self.ensure_scratchpad_inputs(window, cx);

        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.sidebar_accent;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let folder_color = theme.colors.warning;
        let ref_color = theme.colors.info;
        let primary = theme.colors.primary;
        let info = theme.colors.info;
        let success = theme.colors.success;
        let active_border = theme.colors.list_active_border;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();

        let (
            rows,
            error,
            external_refs,
            trash,
            trash_expanded,
            selected,
            expanded,
            loaded_children,
            sort,
            sort_desc,
            edit,
            undo,
            filter,
            has_clipboard,
            loading,
        ) = {
            let view = self.scratchpad.borrow();
            let filter = view
                .search_input
                .as_ref()
                .map(|i| i.read(cx).value().to_lowercase().trim().to_string())
                .unwrap_or_default();
            let mut flat = Vec::new();
            flatten_scratchpad(
                &view.entries,
                0,
                &view.expanded,
                &view.children,
                view.sort,
                view.sort_desc,
                &filter,
                &mut flat,
            );
            (
                flat,
                view.error.clone(),
                view.external_refs.clone(),
                view.trash.clone(),
                view.trash_expanded,
                view.selected.clone(),
                view.expanded.clone(),
                view.children.clone(),
                view.sort,
                view.sort_desc,
                view.edit.clone(),
                view.undo.clone(),
                filter,
                view.clipboard.is_some(),
                view.loading,
            )
        };

        // ── 工具栏（新建文件 / 新建文件夹 / 刷新）──
        let start_new_file = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                });
            }
        };
        let start_new_folder = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFolder, window, cx)
                });
            }
        };
        let refresh = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                view.borrow_mut().loaded = false;
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let cycle_sort = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    let (mut next_sort, mut next_desc) = (v.sort, v.sort_desc);
                    scratchpad_cycle_sort(&mut next_sort, &mut next_desc);
                    v.sort = next_sort;
                    v.sort_desc = next_desc;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let cut_selection = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.set_scratchpad_clipboard(ScratchpadClipboardMode::Cut, cx)
                });
            }
        };
        let copy_selection = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.set_scratchpad_clipboard(ScratchpadClipboardMode::Copy, cx)
                });
            }
        };
        let paste_clipboard = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.paste_scratchpad_clipboard(cx));
            }
        };
        let delete_selection = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.delete_scratchpad_selection(cx));
            }
        };
        let import_files = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.pick_scratchpad_imports(window, cx));
            }
        };
        let add_reference = {
            let entity_template = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                let entity = entity_template.clone();
                // 引用 = 只记路径、不复制，因此**文件或目录**均可（区别于导入）。
                let receiver = app.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: false,
                    prompt: Some("选择要引用的文件或目录".into()),
                });
                window
                    .spawn(app, async move |cx| {
                        if let Ok(Ok(Some(paths))) = receiver.await {
                            if let Some(path) = paths.into_iter().next() {
                                let _ = cx.update(|window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.start_scratchpad_edit(
                                            ScratchpadEdit::NewReference { path },
                                            window,
                                            cx,
                                        )
                                    });
                                });
                            }
                        }
                    })
                    .detach();
            }
        };

        let tool_btn = |id: &'static str,
                        glyph: &'static str,
                        handler: Box<
            dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
        >| {
            div()
                .id(id)
                .h_flex()
                .items_center()
                .justify_center()
                .w_6()
                .h_6()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(muted)
                .hover(move |s| s.bg(hover_bg))
                .on_click(move |ev, window, app| handler(ev, window, app))
                .child(glyph)
        };

        let has_selection = !selected.is_empty();
        let mut toolbar = div().v_flex().w_full().gap_1().px_1p5().py_1();
        toolbar = toolbar.child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .child(tool_btn("sp-new-file", "＋", Box::new(start_new_file)))
                .child(tool_btn("sp-new-folder", "🗀", Box::new(start_new_folder)))
                .child(tool_btn("sp-import", "⬇", Box::new(import_files)))
                .child(tool_btn("sp-add-ref", "🔗", Box::new(add_reference)))
                .child(div().flex_1())
                .child(tool_btn("sp-sort", "⇅", Box::new(cycle_sort)))
                .child(tool_btn("sp-refresh", "↻", Box::new(refresh))),
        );
        if has_selection || has_clipboard {
            toolbar = toolbar.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .child(tool_btn("sp-cut", "✂", Box::new(cut_selection)))
                    .child(tool_btn("sp-copy", "⧉", Box::new(copy_selection)))
                    .child(tool_btn("sp-paste", "📋", Box::new(paste_clipboard)))
                    .child(tool_btn("sp-delete", "🗑", Box::new(delete_selection)))
                    .child(div().flex_1())
                    .child(div().id("sp-sel-count").text_xs().text_color(muted).child(
                        if has_selection {
                            format!("{} 项", selected.len())
                        } else {
                            "剪贴板".to_string()
                        },
                    )),
            );
        }

        // ── 搜索（文件名过滤 / 内容搜索）──
        let (search_mode, search_regex, search_case) = {
            let v = self.scratchpad.borrow();
            (v.search_mode, v.search_regex, v.search_case)
        };
        let toggle_mode = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_mode = if v.search_mode == ScratchpadSearchMode::Content {
                        ScratchpadSearchMode::Name
                    } else {
                        ScratchpadSearchMode::Content
                    };
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let toggle_regex = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_regex = !v.search_regex;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let toggle_case = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_case = !v.search_case;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let run_search = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.run_scratchpad_content_search(cx));
            }
        };

        let mode_label = if search_mode == ScratchpadSearchMode::Content {
            "内容"
        } else {
            "文件名"
        };
        let mode_on = search_mode == ScratchpadSearchMode::Content;

        let mut search_row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .pb_1();
        search_row = search_row.child(
            div()
                .id("sp-mode")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if mode_on { fg } else { muted })
                .when(mode_on, |this| this.bg(selected_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(mode_label)
                .on_click(toggle_mode),
        );
        if let Some(input) = self.scratchpad.borrow().search_input.clone() {
            search_row =
                search_row.child(div().flex_1().min_w_0().child(Input::new(&input).w_full()));
        }
        if search_mode == ScratchpadSearchMode::Content {
            search_row = search_row
                .child(
                    div()
                        .id("sp-regex")
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .px_1()
                        .h_5()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(if search_regex { fg } else { muted })
                        .when(search_regex, |this| this.bg(selected_bg))
                        .hover(move |s| s.bg(hover_bg))
                        .child(".*")
                        .on_click(toggle_regex),
                )
                .child(
                    div()
                        .id("sp-case")
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .px_1()
                        .h_5()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(if search_case { fg } else { muted })
                        .when(search_case, |this| this.bg(selected_bg))
                        .hover(move |s| s.bg(hover_bg))
                        .child("Aa")
                        .on_click(toggle_case),
                )
                .child(
                    div()
                        .id("sp-run")
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .px_1()
                        .h_5()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("⏎")
                        .on_click(run_search),
                );
        }

        let group_header = |label: &str, count: usize| {
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .h(rems(1.375))
                .px_1p5()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(label.to_string())
                .child(div().flex_1())
                .child(count.to_string())
        };

        let mut panel = div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .child(toolbar)
            .child(search_row);

        if let Some(err) = &error {
            return panel.child(
                div()
                    .flex_1()
                    .w_full()
                    .px_2p5()
                    .py_3()
                    .text_xs()
                    .text_color(muted)
                    .child(err.clone()),
            );
        }

        // 顶部内联新建（仅「新建引用」：文件/文件夹的内联行插在目标文件夹下）。
        if matches!(edit.as_ref(), Some(ScratchpadEdit::NewReference { .. })) {
            panel = panel.child(div().px_1().child(self.render_scratchpad_edit_row(0, cx)));
        }

        // ── 草稿树（面板唯一滚动区）──
        let row_count = rows.len();
        let file_count = rows
            .iter()
            .filter(|(_, e)| e.kind == ScratchpadEntryKind::File)
            .count();
        let folder_count = row_count - file_count;
        // 内联新建的文件/文件夹行定位：在目标文件夹下一行（未选中文件夹则列表首行）。
        let new_target = self.scratchpad.borrow().new_target.clone();
        let edit_insert: Option<(usize, usize)> = match edit.as_ref() {
            Some(ScratchpadEdit::NewFile) | Some(ScratchpadEdit::NewFolder) => {
                if new_target.is_empty() {
                    Some((0, 0))
                } else {
                    rows.iter()
                        .position(|(_, e)| e.path.to_string_lossy() == new_target)
                        .map(|i| (i + 1, rows[i].0 + 1))
                        .or(Some((0, 0)))
                }
            }
            _ => None,
        };
        let display_count = row_count + usize::from(edit_insert.is_some());
        let row_ctx = ScratchpadRowCtx {
            keys: Rc::new(
                rows.iter()
                    .map(|(_, e)| e.path.to_string_lossy().to_string())
                    .collect(),
            ),
            rows: Rc::new(rows),
            edit: edit.clone(),
            edit_insert,
            selected: selected.clone(),
            expanded: expanded.clone(),
            loaded: loaded_children.clone(),
            colors: ScratchpadRowColors {
                hover_bg,
                selected_bg,
                fg,
                muted,
                folder_color,
                primary,
                info,
                success,
                active_border,
            },
        };

        let mut drafts = div().v_flex().flex_1().min_h_0().w_full().gap_1().px_1();
        if display_count == 0 {
            // 加载中不显示空态引导，避免「草稿箱是空的」闪现。
            if loading {
                drafts = drafts.child(
                    div()
                        .v_flex()
                        .items_center()
                        .w_full()
                        .pt_6()
                        .px_2()
                        .text_xs()
                        .text_color(muted)
                        .child("加载中…"),
                );
            } else {
                drafts = drafts.child(self.render_scratchpad_empty_state(&entity, &filter, cx));
            }
        } else {
            if row_count > 0 {
                drafts = drafts.child(group_header("草稿", row_count));
            }
            let sizes: Rc<Vec<Size<Pixels>>> = Rc::new(
                (0..display_count)
                    .map(|i| {
                        Size::new(
                            Pixels::ZERO,
                            Self::scratchpad_row_height(&row_ctx, i, window.rem_size()),
                        )
                    })
                    .collect(),
            );
            let list_ctx = row_ctx.clone();
            let scroll = self.scratchpad.borrow().list_scroll.clone();
            let list = v_virtual_list(
                entity.clone(),
                "sp-drafts",
                sizes,
                move |this, range: std::ops::Range<usize>, _window, cx| {
                    range
                        .map(|i| this.scratchpad_row(i, &list_ctx, cx))
                        .collect::<Vec<AnyElement>>()
                },
            )
            .track_scroll(scroll.handle());
            drafts = drafts.child(div().flex_1().min_h_0().w_full().child(list));
        }
        panel = panel.child(drafts);

        // ── 底部固定区（引用 / 回收站 / 撤销栏 / 状态；不随草稿树滚动）──
        let mut body = div().v_flex().w_full().gap_1().px_1().pb_1();


        // ── 外部引用（链接：改名 / 打开 / 移除；不复制文件）──
        if !external_refs.is_empty() {
            body = body.child(group_header("外部引用", external_refs.len()));
            for r in &external_refs {
                // 本引用正在改名 → 行内输入。
                if let Some(ScratchpadEdit::RenameReference { alias }) = &edit {
                    if alias == &r.alias {
                        body = body.child(self.render_scratchpad_edit_row(0, cx));
                        continue;
                    }
                }

                let rename_ref = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.start_scratchpad_edit(
                                ScratchpadEdit::RenameReference { alias: alias.clone() },
                                window,
                                cx,
                            )
                        });
                    }
                };
                let open_ref = {
                    let entity = entity.clone();
                    let path = r.path.to_string_lossy().to_string();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.open_scratchpad_location(path.clone(), cx)
                        });
                    }
                };
                let remove = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.remove_scratchpad_reference(alias.clone(), cx)
                        });
                    }
                };
                // 失效引用提供「重新引用」（选择新路径后只改路径）。
                let relink = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.relink_scratchpad_reference(alias.clone(), window, cx)
                        });
                    }
                };
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .h(rems(1.375))
                        .px_1p5()
                        .child(div().w_2().h_2().flex_none().rounded_sm().bg(ref_color))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(if r.exists { fg } else { muted })
                                .child(if r.exists {
                                    r.alias.clone()
                                } else {
                                    format!("{}（丢失）", r.alias)
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(r.path.to_string_lossy().to_string()),
                        )
                        .child(
                            div()
                                .id(format!("sp-ref-open-{}", r.alias))
                                .w(rems(1.125))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .hover(move |s| s.bg(hover_bg))
                                .child("↗")
                                .on_click(open_ref),
                        )
                        .child(
                            div()
                                .id(format!("sp-ref-ren-{}", r.alias))
                                .w(rems(1.125))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .hover(move |s| s.bg(hover_bg))
                                .child("✎")
                                .on_click(rename_ref),
                        )
                        .when(!r.exists, |this| {
                            this.child(
                                div()
                                    .id(format!("sp-ref-relink-{}", r.alias))
                                    .w(rems(1.125))
                                    .h_flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(info)
                                    .hover(move |s| s.bg(hover_bg))
                                    .child("⟲")
                                    .on_click(relink),
                            )
                        })
                        .child(
                            div()
                                .id(format!("sp-ref-{}", r.alias))
                                .w(rems(1.125))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .hover(move |s| s.bg(hover_bg))
                                .child("✕")
                                .on_click(remove),
                        ),
                );
            }
        }

        // ── 回收站（展开 / 还原 / 清空）──
        {
            let toggle_trash = {
                let view = view_handle.clone();
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    {
                        let mut v = view.borrow_mut();
                        v.trash_expanded = !v.trash_expanded;
                    }
                    entity.update(app, |_, cx| cx.notify());
                }
            };
            let chevron = if trash_expanded { "▾" } else { "▸" };
            let mut header = div()
                .id("sp-trash-head")
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .h(rems(1.375))
                .px_1p5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .hover(move |s| s.bg(hover_bg))
                .on_click(toggle_trash)
                .child(div().w_2p5().flex_none().child(chevron))
                .child("回收站")
                .child(div().flex_1())
                .child(trash.len().to_string());

            if !trash.is_empty() {
                let empty = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| this.empty_scratchpad_trash(cx));
                    }
                };
                header = header.child(
                    div()
                        .id("sp-trash-empty")
                        .cursor_pointer()
                        .text_xs()
                        .text_color(primary)
                        .child("清空")
                        .on_click(empty),
                );
            }
            body = body.child(header);

            if trash_expanded {
                for t in &trash {
                    let restore = {
                        let entity = entity.clone();
                        let id = t.manifest.id.clone();
                        move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                            entity.update(app, |this, cx| {
                                this.restore_scratchpad_trash(id.clone(), cx)
                            });
                        }
                    };
                    body =
                        body.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .h(rems(1.375))
                                .pl(rems(1.125))
                                .pr_1p5()
                                .child(
                                    div().flex_1().min_w_0().text_xs().text_color(muted).child(
                                        format!("{} · {}", t.manifest.name, t.manifest.origin),
                                    ),
                                )
                                .child(
                                    div()
                                        .id(format!("sp-trash-{}", t.manifest.id))
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(primary)
                                        .child("还原")
                                        .on_click(restore),
                                ),
                        );
                }
            }
        }

        // 引用 / 回收站限高可滚，保证草稿树始终有可用高度。
        panel = panel.child(
            div()
                .v_flex()
                .w_full()
                .max_h(rems(ui::SCRATCHPAD_GROUP_MAX_HEIGHT))
                .overflow_y_scrollbar()
                .child(body),
        );

        // ── 撤销栏 ──
        if let Some(undo) = &undo {
            let undo_click = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| this.undo_scratchpad_delete(cx));
                }
            };
            panel = panel.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .px_2()
                    .py(rems(1.25))
                    .bg(hover_bg)
                    .rounded_sm()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(fg)
                            .child(format!("已删除 {}", undo.label)),
                    )
                    .child(
                        div()
                            .id("sp-undo")
                            .cursor_pointer()
                            .text_xs()
                            .text_color(primary)
                            .child("撤销")
                            .on_click(undo_click),
                    ),
            );
        }

        // ── 底部状态 ──
        panel = panel.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(format!(
                    "{}{file_count} 个文件 · {folder_count} 个文件夹 · {} 项引用 · {} 项回收站 · 排序 {}",
                    if loading { "加载中… · " } else { "" },
                    external_refs.len(),
                    trash.len(),
                    scratchpad_sort_label(sort, sort_desc)
                )),
        );

        panel
            .key_context("scratchpad")
            .track_focus(&self.focus_handle)
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadSelectAll, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.select_all_scratchpad(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadRename, window: &mut gpui_kit::Window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.rename_scratchpad_selection(window, cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadDelete, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.delete_scratchpad_selection(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadCancelEdit, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.cancel_scratchpad_edit(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadUp, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.scratchpad_move(-1, cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadDown, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.scratchpad_move(1, cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadOpen, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.scratchpad_open_selection(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadNewFile, window: &mut gpui_kit::Window, cx: &mut App| {
                    entity.update(cx, |this, cx| {
                        this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                    });
                }
            })
    }

    fn render_plugin_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("插件"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 插件市场（下一轮接入）"),
            )
    }
}

impl EventEmitter<BasePanelEvent> for SidebarPanel {}

impl EventEmitter<SidebarEvent> for SidebarPanel {}

impl Focusable for SidebarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidebarPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().colors.background;
        let fg = cx.theme().colors.foreground;
        let active = self.shared.active_left.get();
        let content: Div = match active {
            LeftPanel::Draft => self.render_scratchpad(window, cx),
            LeftPanel::Database => self.render_database_nav(window, cx),
            LeftPanel::Resources => self.render_resources_placeholder(fg),
            LeftPanel::Plugin => self.render_plugin_placeholder(fg),
        };
        div().v_flex().size_full().min_h_0().bg(bg).child(content)
    }
}

impl BasePanel for SidebarPanel {
    fn panel_name(&self) -> &'static str {
        "sidebar"
    }
}

impl ComponentPanel for SidebarPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.shared.active_left.get().label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(self.shared.active_left.get().label())
    }
}

/// 中央内容区面板。
pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    // Phase A：新建连接对话框状态（懒创建，open_dialog 由"新建连接"按钮触发）。
    // `Rc` 包装：对话框内部（暂存列表 / 管理器等）需把状态句柄 clone 进回调。
    dialog: Option<Rc<connection_dialog::ConnectionDialogState>>,
    // Round 26：SQL 查询区（受控输入 + 结果集）。
    sql_textarea: Option<Entity<TextareaState>>,
    query_result: Rc<RefCell<Option<QueryOutput>>>,
    // Round 29：SQL 历史（最新在前，跨会话持久化）。
    sql_history: Rc<RefCell<Vec<String>>>,
    /// 最近一次成功执行的 SQL（用于判断编辑区是否有未保存草稿）。
    last_executed: Rc<RefCell<String>>,
    /// SQL 输入订阅句柄（Change 事件 → 脏状态）。
    _sql_sub: Option<Subscription>,
    /// 连接对话框项目下拉确认订阅（由 `ensure_dialog_subscription` 持有；句柄释放即取消）。
    _dialog_sub: Option<Subscription>,
    // Phase B：右侧停靠属性面板状态。
    property: Rc<RefCell<PropertyState>>,
    /// 属性面板宽度（rem 倍率；拖拽时更新，关闭时持久化到 settings.json）。
    property_width: Rc<Cell<f32>>,
    /// 正在轮询属性加载结果的后台任务。
    props_pump: RefCell<Option<Task<()>>>,
    /// M5 草稿箱内容搜索的「替换为」输入框（懒创建，有搜索结果时才建）。
    scratchpad_replace: Option<Entity<InputState>>,
    _scratchpad_replace_sub: Option<Subscription>,
}

impl EditorPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            dialog: None,
            sql_textarea: None,
            query_result: Rc::new(RefCell::new(None)),
            sql_history: Rc::new(RefCell::new(crate::services::query_history::load_history())),
            last_executed: Rc::new(RefCell::new(String::new())),
            _sql_sub: None,
            _dialog_sub: None,
            property: Rc::new(RefCell::new(PropertyState::default())),
            property_width: Rc::new(Cell::new(
                settings::load_settings().navigator.property_panel_width,
            )),
            props_pump: RefCell::new(None),
            scratchpad_replace: None,
            _scratchpad_replace_sub: None,
        }
    }

    /// 懒创建草稿箱搜索结果的「替换为」输入框（有结果时才建）并订阅回车执行。
    fn ensure_scratchpad_replace_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scratchpad_replace.is_some() || self.shared.scratchpad_search.borrow().is_none() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("替换为…"));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _e, ev: &InputEvent, _w, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.replace_scratchpad_all(cx),
                _ => {}
            },
        );
        self.scratchpad_replace = Some(input);
        self._scratchpad_replace_sub = Some(sub);
    }

    /// 在草稿箱内容搜索结果上「全部替换」：逐文件原子写回 → 刷新结果。
    ///
    /// 若文件夹路径覆盖多个文件，按去重后的命中文件列表逐个替换（与侧栏搜索开关一致）。
    fn replace_scratchpad_all(&mut self, cx: &mut Context<Self>) {
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许替换草稿内容".to_string());
            cx.notify();
            return;
        }
        let replacement = self
            .scratchpad_replace
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        if replacement.is_empty() {
            *self.shared.notice.borrow_mut() = Some("请先输入替换内容".to_string());
            cx.notify();
            return;
        }
        let (query, is_regex, case_sensitive, files) = {
            let search = self.shared.scratchpad_search.borrow();
            match search.as_ref() {
                Some(s) => {
                    let mut files: Vec<String> =
                        s.hits.iter().map(|h| h.file.clone()).collect();
                    files.sort();
                    files.dedup();
                    (s.query.clone(), s.is_regex, s.case_sensitive, files)
                }
                None => return,
            }
        };

        let outcome = (|| -> Result<(usize, usize), String> {
            let (store, rt) = self.shared.scratchpad_store()?;
            let mut total = 0usize;
            let mut changed_files = 0usize;
            for file in &files {
                let r = rt
                    .block_on(store.replace_in_file(
                        file,
                        &query,
                        &replacement,
                        is_regex,
                        case_sensitive,
                    ))
                    .map_err(|e| format!("{file}: {e}"))?;
                if r.replaced > 0 {
                    changed_files += 1;
                    total += r.replaced;
                }
            }
            // 写回后刷新结果，避免“已替换但仍显示旧命中”。
            let view = run_scratchpad_search(&store, &rt, &query, case_sensitive, is_regex)?;
            *self.shared.scratchpad_search.borrow_mut() = Some(view);
            Ok((total, changed_files))
        })();

        match outcome {
            Ok((total, changed_files)) => {
                *self.shared.notice.borrow_mut() =
                    Some(format!("已替换 {total} 处（{changed_files} 个文件）"));
            }
            Err(e) => *self.shared.notice.borrow_mut() = Some(format!("替换失败: {e}")),
        }
        self.shared.notify_host(cx);
        cx.notify();
    }

    /// 清空 SQL 编辑区（保存 / 放弃未保存草稿后由宿主命令调用；在事件上下文中执行，非 render）。
    pub fn clear_sql(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ta) = &self.sql_textarea {
            ta.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.last_executed.borrow_mut().clear();
        self.shared.editor_dirty.set(false);
        self.shared.editor_sql.borrow_mut().clear();
        cx.notify();
    }

    /// 连接对话框状态（懒创建：首次 `request_*` 时建立；宿主 / 测试只读访问）。
    pub fn dialog_state(&self) -> Option<Rc<connection_dialog::ConnectionDialogState>> {
        self.dialog.clone()
    }

    /// 打开「新建数据源连接」对话框（编辑区按钮入口）。
    ///
    /// 打开后必须通知宿主重绘：对话框层挂在 `WorkbenchView::render` 上，
    /// 而 `Root` 的 notify 不会让子视图重建元素树。
    pub fn request_new_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
            // 新状态 → 旧订阅（若有）失去意义，重建。
            self._dialog_sub = None;
        }
        let dialog = self.dialog.clone().expect("dialog initialized");
        // 重新打开：重置元数据标记，强制下一次渲染重新拉取引用 / 类型 / 驱动目录。
        dialog.meta_refreshed.set(false);
        self.ensure_dialog_subscription(&dialog, window, cx);
        dialog.open(cx.entity(), self.shared.clone(), None, window, cx);
        self.shared.notify_host(cx);
    }

    /// 打开「编辑数据源连接」对话框（侧边栏「编辑」入口）。
    pub fn request_edit_connection(
        &mut self,
        conn_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
            self._dialog_sub = None;
        }
        let dialog = self.dialog.clone().expect("dialog initialized");
        // 重新打开：重置元数据标记，强制下一次渲染重新拉取（引用 / 类型 / 驱动目录）。
        dialog.meta_refreshed.set(false);
        self.ensure_dialog_subscription(&dialog, window, cx);
        dialog.open(cx.entity(), self.shared.clone(), Some(conn_id), window, cx);
        self.shared.notify_host(cx);
    }

    /// 确保项目下拉确认订阅已建立（只建一次；订阅句柄由本面板持有）。
    ///
    /// 订阅建立放在面板入口而非 `ConnectionDialogState::open`：`open` 在面板 `update`
    /// 上下文内被调用，在那里再 `update` 面板会触发重入 panic。
    fn ensure_dialog_subscription(
        &mut self,
        dialog: &Rc<connection_dialog::ConnectionDialogState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._dialog_sub.is_none() {
            self._dialog_sub = Some(dialog.subscribe_project_confirm(&self.shared, window, cx));
        }
    }

    /// 启动属性结果轮询（已有存活任务时不重复启动）。
    fn ensure_props_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.props_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let results = nav_jobs::drain_props_results();
                let had = !results.is_empty();
                if had
                    && weak
                        .update(cx, |this, cx| this.apply_props_results(results, cx))
                        .is_err()
                {
                    return;
                }
                if !nav_jobs::has_pending_props() {
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !nav_jobs::has_pending_props() {
                        break;
                    }
                }
            }
        });
        *self.props_pump.borrow_mut() = Some(task);
    }

    /// 回填属性加载结果（主线程；key 不匹配的过期结果丢弃）。
    fn apply_props_results(&mut self, results: Vec<nav_jobs::PropsResult>, cx: &mut Context<Self>) {
        for r in results {
            let mut st = self.property.borrow_mut();
            if st.loaded_for.as_deref() != Some(r.key.as_str()) {
                continue;
            }
            st.loading = false;
            match r.result {
                Ok(p) => {
                    st.props = Some(p);
                    st.error = None;
                }
                Err(e) => {
                    st.props = None;
                    st.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    /// 右侧停靠属性面板（DBeaver 式：属性网格 + 子实体表格）。
    fn render_property_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(target) = self.shared.property_target.borrow().clone() else {
            return div();
        };

        let key = format!(
            "{}|{:?}|{}|{:?}",
            target.property.conn_id,
            target.property.kind,
            target.property.name,
            target.property.parent
        );
        let needs_load = {
            let mut st = self.property.borrow_mut();
            if st.loaded_for.as_deref() != Some(key.as_str()) {
                st.loaded_for = Some(key.clone());
                st.props = None;
                st.error = None;
                st.loading = true;
                true
            } else {
                false
            }
        };
        if needs_load {
            // 后台加载：render 不做 I/O；结果由 `apply_props_results` 回填。
            // 驱动目录已缓存（随组织数据一次加载）：把「数据库类型 + 驱动友好名」一并传给属性面板。
            let (driver_display, db_type) = {
                let catalog = self.shared.driver_catalog.borrow();
                match catalog.get(&target.driver) {
                    Some(meta) => (
                        format!("{} · {}", meta.name, target.driver),
                        Some(meta.type_id.clone()),
                    ),
                    None => (target.driver.clone(), None),
                }
            };
            nav_jobs::enqueue_properties(
                &key,
                target.property.clone(),
                &target.conn_label,
                &driver_display,
                db_type.as_deref(),
            );
            self.ensure_props_pump(cx);
        }

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.background;
        let tabbar = cx.theme().colors.tab_bar;
        let danger = cx.theme().colors.danger;

        let st = self.property.borrow();
        let title = st
            .props
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| target.property.name.clone());
        let object_type = st
            .props
            .as_ref()
            .map(|p| p.object_type.clone())
            .unwrap_or_default();

        let mut body = div().v_flex().w_full().gap_1();
        if st.loading {
            body = body.child(div().text_xs().text_color(muted).child("加载中…"));
        }
        if let Some(err) = &st.error {
            body = body.child(div().text_xs().text_color(danger).child(err.clone()));
        }
        if let Some(props) = &st.props {
            for row in &props.properties {
                body = body.child(
                    div()
                        .h_flex()
                        .items_start()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .w(rems(4.5))
                                .flex_none()
                                .text_color(muted)
                                .child(row.label.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(fg)
                                .child(row.value.clone()),
                        ),
                );
            }
            for section in &props.sections {
                body = body.child(
                    div()
                        .pt_2()
                        .pb_1()
                        .w_full()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(section.label.clone()),
                );
                body = body.child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .text_color(muted)
                        .children(
                            section
                                .table
                                .headers
                                .iter()
                                .map(|h| div().flex_1().min_w_0().child(h.clone())),
                        ),
                );
                for data_row in &section.table.rows {
                    body = body.child(
                        div()
                            .h_flex()
                            .w_full()
                            .gap_2()
                            .text_xs()
                            .text_color(fg)
                            .children(
                                data_row
                                    .iter()
                                    .map(|c| div().flex_1().min_w_0().child(c.clone())),
                            ),
                    );
                }
            }
        }

        div()
            .v_flex()
            .w_full()
            .h_full()
            .flex_none()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .h(rems(2.125))
                    .px_3()
                    .border_1()
                    .border_color(border)
                    .bg(tabbar)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(title),
                    )
                    .child(div().text_xs().text_color(muted).child(object_type))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("property-close")
                            .px_1()
                            .text_xs()
                            .text_color(muted)
                            .cursor_pointer()
                            .child("关闭")
                            .on_click({
                                let entity = cx.entity();
                                let shared = self.shared.clone();
                                let width = self.property_width.clone();
                                move |_, _, app: &mut App| {
                                    *shared.property_target.borrow_mut() = None;
                                    // 关闭时持久化拖拽宽度（拖拽过程不写盘）。
                                    settings::SettingsService::set_property_panel_width(
                                        width.get(),
                                        app,
                                    );
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .id("property-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px_3()
                    .py_2()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

impl EventEmitter<BasePanelEvent> for EditorPanel {}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 受控输入懒创建（render 首次初始化，需要 window）；SQL 查询区保留。
        if self.sql_textarea.is_none() {
            let ta = cx.new(|cx| TextareaState::new(window, cx));
            // M1：脏状态由输入事件驱动（render 只读状态，不做写副作用——遵循编码指南）。
            let shared = self.shared.clone();
            let last_executed = self.last_executed.clone();
            let ta_for_read = ta.clone();
            let sub = cx.subscribe_in(
                &ta,
                window,
                move |_this, _ta, ev: &InputEvent, _window, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    let val = ta_for_read.read(cx).value().to_string();
                    let dirty = !val.trim().is_empty() && val != *last_executed.borrow();
                    shared.editor_dirty.set(dirty);
                    *shared.editor_sql.borrow_mut() = val;
                },
            );
            self._sql_sub = Some(sub);
            self.sql_textarea = Some(ta);
        }
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
        }

        // 消费侧边栏「编辑」请求（open_edit 置位后在此打开对话框）。
        let edit_request = self.shared.open_edit.borrow_mut().take();
        if let Some(cid) = edit_request {
            self.request_edit_connection(cid, window, cx);
        }

        // 消费导航面板头「＋」/ 空态「新建连接」请求。
        if self.shared.new_connection_request.take() {
            self.request_new_connection(window, cx);
        }

        // 消费导航右键「新建查询 / 查看数据」注入的 SQL：追加到当前草稿后。
        // 用 `set_value`（不发事件）并手动同步 dirty / editor_sql，与 `clear_sql` 同策略，
        // 避免在 render 内同步派发 InputEvent 引发重入。
        let sql_request = self.shared.editor_set.borrow_mut().take();
        if let Some(sql) = sql_request {
            if let Some(ta) = &self.sql_textarea {
                let current = ta.read(cx).value().to_string();
                let combined = if current.trim().is_empty() {
                    sql
                } else {
                    format!("{}\n{}", current.trim_end(), sql)
                };
                ta.update(cx, |s, cx| s.set_value(combined.clone(), window, cx));
                self.shared.editor_dirty.set(true);
                *self.shared.editor_sql.borrow_mut() = combined;
            }
        }

        // M5 草稿箱搜索结果：替换输入框懒创建（须在 `let theme = cx.theme()` 之前）。
        self.ensure_scratchpad_replace_input(window, cx);

        let theme = cx.theme();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        let mut content = div()
            .v_flex()
            .size_full()
            .pt_4()
            .pl_4()
            .pr_4()
            .gap_3()
            .bg(theme.colors.background)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.colors.foreground)
                            .child("数据源连接"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("选中连接查看真实元数据；本地加速（DuckDB Secret）将在连接建立后自动注册"),
                    ),
            );

        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
            let shared = self.shared.clone();
            let entity = entity.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();
            let status = if item.connected {
                "已连接"
            } else {
                "未连接"
            };
            let status_color = if item.connected {
                theme.colors.success
            } else {
                theme.colors.muted
            };
            let fields: Vec<(&'static str, String)> = vec![
                ("名称", item.name.clone()),
                ("驱动", item.driver.clone()),
                ("主机", item.host.clone().unwrap_or_else(|| "-".to_string())),
                (
                    "端口",
                    item.port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "数据库",
                    item.database.clone().unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "Schema",
                    item.schema.clone().unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "DuckDB 联邦",
                    if item.use_duckdb_fed {
                        "开启（本地加速）".to_string()
                    } else {
                        "关闭".to_string()
                    },
                ),
                (
                    "描述",
                    item.description.clone().unwrap_or_else(|| "-".to_string()),
                ),
                ("创建时间", item.created_at.clone()),
                ("更新时间", item.updated_at.clone()),
            ];

            let mut rows = div().v_flex().gap_1();
            for (label, value) in fields {
                rows = rows.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w_24()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.colors.foreground)
                                .child(value),
                        ),
                );
            }
            content = content.child(
                div().v_flex().gap_2().w_full().rounded_md().pl_3().pr_3().pt_2p5().pb_2p5()
                    .border_1().border_color(theme.colors.border)
                    .child(
                        div().h_flex().items_center().gap_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(item.name))
                            .child(
                                div().h_flex().items_center().gap_1()
                                    .child(div().w_2().h_2().rounded_full().bg(status_color))
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child(status)),
                            ),
                    )
                    .child(rows)
                    .child(
                        div().h_flex().items_center().gap_2().mt_2()
                            .child(
                                div()
                                    .id("delete-connection")
                                    .cursor_pointer()
                                    .on_click(move |_, _, app| {
                                        // 项目作用域删除需要当前项目根（未打开项目时传 None）。
                                        let project_root = shared
                                            .project
                                            .borrow()
                                            .as_ref()
                                            .map(|p| p.root.to_string_lossy().to_string());
                                        match crate::services::workspace_loader::delete_connection(
                                            &selected_id,
                                            project_root.as_deref(),
                                        ) {
                                            Ok(()) => {
                                                let (items, _) =
                                                    crate::services::workspace_loader::load_connections_for_scope(
                                                        project_root
                                                            .as_deref()
                                                            .map(std::path::Path::new),
                                                    );
                                                *shared.connections.borrow_mut() = items;
                                                shared.selected.set(None);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("连接「{}」已删除", selected_name));
                                            }
                                            Err(e) => {
                                                *shared.notice.borrow_mut() = Some(format!("删除失败: {}", e));
                                            }
                                        }
                                        entity.update(app, |_, cx| cx.notify());
                                    })
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.colors.danger)
                                            .child("删除连接"),
                                    ),
                            ),
                    )
                    .child(
                        div().v_flex().gap_1().mt_2().pt_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("数据库导航（DuckDB 分析库）")),
                    ),
            );
        }

        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                if self.shared.nav_for.borrow().as_deref() != Some(item.id.as_str()) {
                    let path = crate::services::workspace_loader::global_analysis_db_path();
                    let tree = crate::services::db_navigator::load_navigator_tree(&path)
                        .unwrap_or_default();
                    *self.shared.nav_tables.borrow_mut() = tree;
                    *self.shared.nav_for.borrow_mut() = Some(item.id.clone());
                }
                let nav = self.shared.nav_tables.borrow();
                let mut nav_content = div().v_flex().gap_1().mt_1p5();
                if nav.is_empty() {
                    nav_content = nav_content.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("分析库暂无表——运行 seed_demo 或导入数据"),
                    );
                } else {
                    for table in nav.iter() {
                        nav_content =
                            nav_content.child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(div().h_flex().items_center().gap_2().child(
                                        div().text_xs().font_weight(FontWeight::MEDIUM).child(
                                            format!("{}（{} 列）", table.name, table.columns.len()),
                                        ),
                                    ))
                                    .child(div().v_flex().gap_1().pl_3().children(
                                        table.columns.iter().map(|col| {
                                            div()
                                                .h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .w(rems(8.125))
                                                        .flex_none()
                                                        .text_xs()
                                                        .child(col.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_xs()
                                                        .text_color(theme.colors.muted_foreground)
                                                        .child(col.data_type.clone()),
                                                )
                                                .child(if col.is_primary_key {
                                                    div()
                                                        .text_xs()
                                                        .text_color(theme.colors.info)
                                                        .child("PK")
                                                } else {
                                                    div().text_xs().child("")
                                                })
                                        }),
                                    )),
                            );
                    }
                }
                content = content.child(nav_content);
            }
        }

        content = content.child(div().h_6());

        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                let sql_state = self.sql_textarea.clone().expect("lazy init");
                let sql_state_hist = sql_state.clone();
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let shared_view = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
                let entity_hist = entity.clone();
                let conn_id = item.id.clone();
                let last_exec = self.last_executed.clone();

                let mut sql_ui = div()
                    .v_flex()
                    .gap_1()
                    .mt_2()
                    .pt_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("SQL 查询（DuckDB 分析库）"),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .child(Textarea::new(&sql_state).h_24())
                            .child(div().h_flex().justify_end().w_full().child(
                                Button::new("run-sql").secondary().label("执行").on_click(
                                    move |_, _, app| {
                                        // M1：只读打开时禁止执行（禁止写分析库）。
                                        if shared.project_ui.borrow().read_only {
                                            *shared.notice.borrow_mut() =
                                                Some("只读模式：不允许执行 SQL".to_string());
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let sql = sql_state.read(app).value().to_string();
                                        let path =
                                            crate::services::workspace_loader::global_analysis_db_path();
                                        let ok = match crate::services::query_runner::execute_sql(
                                            &path, &sql,
                                        ) {
                                            Ok(out) => {
                                                let n = out.row_count;
                                                *qr_closure.borrow_mut() = Some(out);
                                                *last_exec.borrow_mut() = sql.clone();
                                                *shared.sql_for.borrow_mut() =
                                                    Some(conn_id.clone());
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", n));
                                                true
                                            }
                                            Err(e) => {
                                                *qr_closure.borrow_mut() = None;
                                                *shared.sql_for.borrow_mut() = None;
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询失败: {}", e));
                                                false
                                            }
                                        };
                                        if ok {
                                            entity.update(app, |this, cx| {
                                                if let Ok(hist) =
                                                    crate::services::query_history::append_history(
                                                        &sql,
                                                    )
                                                {
                                                    *this.sql_history.borrow_mut() = hist;
                                                }
                                                cx.notify();
                                            });
                                        } else {
                                            entity.update(app, |_, cx| cx.notify());
                                        }
                                    },
                                ),
                            )),
                    );

                // Round 29：SQL 历史——点击回填到编辑器（最新在前，截断预览）。
                {
                    let history = self.sql_history.borrow();
                    if !history.is_empty() {
                        let mut hist_ui = div().v_flex().gap_1().mt_1();
                        hist_ui = hist_ui.child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.colors.muted_foreground)
                                .child("历史"),
                        );
                        for (i, sql) in history.iter().enumerate() {
                            let preview: String = if sql.chars().count() > 42 {
                                let mut s: String = sql.chars().take(42).collect();
                                s.push('…');
                                s
                            } else {
                                sql.clone()
                            };
                            let sql_clone = sql.clone();
                            let st = sql_state_hist.clone();
                            let e_hist = entity_hist.clone();
                            hist_ui = hist_ui.child(
                                div()
                                    .id(ElementId::Name(SharedString::from(format!("hist-{i}"))))
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .cursor_pointer()
                                    .child(preview)
                                    .on_click(move |_, window, app| {
                                        st.update(app, |s, cx| {
                                            s.set_value(sql_clone.clone(), window, cx)
                                        });
                                        e_hist.update(app, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        sql_ui = sql_ui.child(hist_ui);
                    }
                }

                let result_visible =
                    shared_view.sql_for.borrow().as_deref() == Some(item.id.as_str());
                let result = if result_visible {
                    query_result.borrow().clone()
                } else {
                    None
                };
                if let Some(out) = result {
                    if out.columns.is_empty() {
                        sql_ui = sql_ui.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("无结果（DDL/无返回行）"),
                        );
                    } else {
                        let mut table = div().v_flex().gap_1();
                        // 列头
                        let mut head = div().h_flex().gap_2();
                        for col in &out.columns {
                            head = head.child(
                                div()
                                    .w(rems(9.375))
                                    .flex_none()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(col.clone()),
                            );
                        }
                        table = table.child(head);
                        // 行
                        for row in &out.rows {
                            let mut row_div = div().h_flex().gap_2();
                            for v in row {
                                row_div = row_div.child(
                                    div()
                                        .w(rems(9.375))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(v.clone()),
                                );
                            }
                            table = table.child(row_div);
                        }
                        sql_ui = sql_ui.child(table);

                        // Round 27：导出 CSV——结果落盘到全局目录 results/ 并按时间戳命名。
                        let out_clone = out.clone();
                        sql_ui = sql_ui.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(format!("共 {} 行", out.rows.len())),
                                )
                                .child(
                                    Button::new("export-csv")
                                        .ghost()
                                        .label("导出 CSV")
                                        .on_click(move |_, _, app| {
                                            match crate::services::query_export::export_to_default(
                                                &out_clone,
                                            ) {
                                                Ok(path) => {
                                                    *shared_export.notice.borrow_mut() =
                                                        Some(format!("已导出: {}", path.display()));
                                                }
                                                Err(e) => {
                                                    *shared_export.notice.borrow_mut() = Some(e);
                                                }
                                            }
                                            entity_export.update(app, |_, cx| cx.notify());
                                        }),
                                ),
                        );
                    }
                }
                sql_ui = sql_ui.child(
                    div().v_flex().gap_1().child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Mock 数据（生成测试数据）"),
                    ),
                );

                content = content.child(sql_ui);
            }
        }

        // M5 草稿箱内容搜索结果（原型 §4.3/§4.5：结果与替换栏落中央编辑区）。
        {
            let search = self.shared.scratchpad_search.borrow();
            if let Some(search) = search.as_ref() {
                let shared = self.shared.clone();
                let replace_input = self.scratchpad_replace.clone();
                let replace_filled = replace_input
                    .as_ref()
                    .map(|i| !i.read(cx).value().trim().is_empty())
                    .unwrap_or(false);
                let clear = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        *shared.scratchpad_search.borrow_mut() = None;
                        shared.notify_host(app);
                        entity.update(app, |_, cx| cx.notify());
                    }
                };
                let replace_all = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| this.replace_scratchpad_all(cx));
                    }
                };
                // 借用顺序：`theme` 已从 cx 借出，这里不再调用 `entity.update(cx, …)`。
                let match_bg = settings::product_tokens::get(cx).search_match_background(theme);
                content = content.child(render_scratchpad_search_pane(
                    search,
                    theme,
                    match_bg,
                    replace_input.as_ref(),
                    replace_filled,
                    clear,
                    replace_all,
                ));
            }
        }

        content = content.child(
            Button::new("mock-generate-btn")
                .primary()
                .label("生成 Mock")
                .on_click({
                    let shared = self.shared.clone();
                    move |_, _, app| {
                        // 单一入口：目标表 / 行数 / 列配置都在右 Dock 的 Mock 面板里确定。
                        shared.open_mock_panel(None, app);
                    }
                }),
        );

        content = content.child(
            Button::new("new-connection")
                .primary()
                .icon(IconName::Plus)
                .label("新建连接")
                .on_click({
                    let entity = entity.clone();
                    move |_, window, app| {
                        entity.update(app, |editor, cx| {
                            editor.request_new_connection(window, cx);
                        });
                    }
                }),
        );

        if notice.is_some() {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.info)
                    .child(notice.unwrap()),
            );
        }

        // `h_flex()` 默认交叉轴居中：不写 items_stretch，内容列会按内容高度被竖直居中，
        // 高于面板的部分上下同时被裁。
        let mut root = div().h_flex().items_stretch().size_full();
        if self.shared.property_target.borrow().is_some() {
            // 属性面板停靠右侧，可拖拽调宽（宽度记忆到 settings.json）。
            let font_size = theme.font_size;
            let width = font_size * self.property_width.get();
            let property = self.render_property_panel(cx);
            let width_cell = self.property_width.clone();
            root = root.child(
                h_resizable("editor-property-split")
                    .child(resizable_panel().child(div().flex_1().min_w_0().child(content)))
                    .child(
                        resizable_panel()
                            .size(width)
                            .size_range(
                                font_size * ui::PROPERTY_PANEL_MIN_WIDTH
                                    ..font_size * ui::PROPERTY_PANEL_MAX_WIDTH,
                            )
                            .flex_none()
                            .child(property),
                    )
                    .on_resize(move |state, _window, app| {
                        let sizes = state.read(app).sizes();
                        if let Some(last) = sizes.last() {
                            width_cell.set(last.as_f32() / app.theme().font_size.as_f32());
                        }
                    }),
            );
        } else {
            root = root.child(div().flex_1().min_w_0().child(content));
        }
        root
    }
}

impl BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "editor"
    }
}

impl ComponentPanel for EditorPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("工作台".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child("工作台")
    }
}

/// 右侧边栏面板：洞察 / Mock 生成 / 历史。
///
/// Mock 面板视图由 mock crate 自持（`mock::mock_view::MockPanel`，与 project / settings 同例）；
/// 本面板只负责：**构造期**创建实体、注入宿主桥（`components::mock_host`）、登记句柄。
/// 洞察与历史仍为占位视图（各自模块轮次接入）。
pub struct RightSidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// Mock 面板实体（随面板构造期创建，无 I/O；视图与状态均在 mock crate）
    mock_panel: Entity<MockPanel>,
}

impl RightSidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        // 构造期创建 Mock 面板实体（不触 I/O）：导航右键定向导入结构需要在事件路径
        // 拿到句柄，懒创建（首帧 render）会让「先右键、后面板尚未渲染」的路径丢目标。
        let host = crate::components::mock_host::build_host(&shared);
        let mock_panel = cx.new(|cx| MockPanel::new(host, cx));
        *shared.mock_panel.borrow_mut() = Some(mock_panel.downgrade());
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            mock_panel,
        }
    }

    fn render_insight_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("洞察"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 库/表/列画像（占位）"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 数据质量建议（占位）"),
            )
    }

    /// M7：Mock 生成面板——视图与状态在 mock crate（`mock::mock_view::MockPanel`），
    /// 本面板只做「转发渲染」。
    fn render_mock_panel(&mut self, cx: &mut Context<Self>) -> Div {
        let panel = self.mock_panel.clone();
        let _ = cx;
        div().v_flex().size_full().min_h_0().child(panel)
    }

    fn render_history_placeholder(&self, fg: Hsla) -> Div {
        let mut panel = div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("历史"),
            );
        let history = crate::services::query_history::load_history();
        if history.is_empty() {
            panel = panel.child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("暂无历史记录"),
            );
        } else {
            for (i, sql) in history.iter().take(20).enumerate() {
                let preview: String = if sql.chars().count() > 42 {
                    let mut s: String = sql.chars().take(42).collect();
                    s.push('…');
                    s
                } else {
                    sql.clone()
                };
                panel = panel.child(
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "right-hist-{i}"
                        ))))
                        .h_6()
                        .pl_2()
                        .pr_2()
                        .text_xs()
                        .text_color(fg)
                        .child(preview),
                );
            }
        }
        panel
    }
}

impl EventEmitter<BasePanelEvent> for RightSidebarPanel {}

impl Focusable for RightSidebarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RightSidebarPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().colors.background;
        let active = self.shared.active_right.get();
        // 各分支自己取色：Mock 分支要 `&mut cx`（创建输入框 / 取实体句柄），
        // 不能先 `let fg = cx.theme()…` 把 `cx` 借出去。
        let content: Div = match active {
            RightPanel::Insight => {
                let fg = cx.theme().colors.foreground;
                self.render_insight_placeholder(fg)
            }
            RightPanel::Mock => self.render_mock_panel(cx),
            RightPanel::History => {
                let fg = cx.theme().colors.foreground;
                self.render_history_placeholder(fg)
            }
        };
        div().v_flex().size_full().min_h_0().bg(bg).child(content)
    }
}

impl BasePanel for RightSidebarPanel {
    fn panel_name(&self) -> &'static str {
        "right_sidebar"
    }
}

impl ComponentPanel for RightSidebarPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.shared.active_right.get().label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(self.shared.active_right.get().label())
    }
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`use gpui_kit::*` 会把 gpui 的 `test` 宏带入作用域）。
    use super::nav_type_badge;
    use super::{nav_type_short_label, parse_nav_search};
    use database::model::NavSource;

    #[test]
    fn type_badge_maps_known_types_and_falls_back() {
        // 已知类型：形状与 2 字母按映射表（原型设计 §2.3）。
        assert_eq!(
            nav_type_badge("postgresql"),
            ("icons/database.svg", "PG".into())
        );
        assert_eq!(nav_type_badge("sqlite"), ("icons/file.svg", "SQ".into()));
        assert_eq!(nav_type_badge("redis"), ("icons/braces.svg", "RD".into()));
        // 目录外类型：回退通用形状 + 类型名首 2 字母（大写）。
        let (path, letters) = nav_type_badge("snowflake");
        assert_eq!(path, "icons/database.svg");
        assert_eq!(letters, "SN");
        // 空类型 id：不做空字母，回退 `DB`。
        assert_eq!(nav_type_badge("").1, "DB");
    }

    #[test]
    fn search_facets_parse_tokens_and_free_text() {
        let p = parse_nav_search("prod scope:global type:mysql tag:核心");
        assert_eq!(p.free, "prod");
        assert_eq!(p.source, Some(NavSource::Global));
        assert_eq!(p.db_type.as_deref(), Some("mysql"));
        assert_eq!(p.tag.as_deref(), Some("核心"));
        assert_eq!(p.active, 3);
        // `source:` 为 `scope:` 历史别名，短码亦可。
        let p = parse_nav_search("source:P driver:postgres_native");
        assert_eq!(p.source, Some(NavSource::Project));
        assert_eq!(p.driver.as_deref(), Some("postgres_native"));
        assert!(p.free.is_empty());
        // 空值 / 未识别前缀不吞字（保留在自由文本）。
        let p = parse_nav_search("tag: foo:bar");
        assert!(p.tag.is_none());
        assert_eq!(p.free, "tag: foo:bar");
        assert_eq!(p.active, 0);
    }

    #[test]
    fn type_short_label_strips_category_suffix() {
        assert_eq!(nav_type_short_label("postgresql"), "PostgreSQL");
        // 未知类型回退原 id。
        assert_eq!(nav_type_short_label("snowflake"), "snowflake");
    }
}
