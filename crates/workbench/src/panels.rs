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
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::components::connection_dialog;

use scratchpad::{
    ExternalReferenceStatus, ScratchpadEntry, ScratchpadEntryKind, ScratchpadStore, TrashEntry,
};

use database::model::{
    NavNode, NavNodeKind, NavPath, NavScope, NavSource, PropertyKind, PropertyRef,
};
use database::navigator_service::NavigatorService;

use crate::services::db_navigator::NavTable;
use crate::services::query_runner::QueryOutput;
use crate::view::{ConnectionItem, LeftPanel, RightPanel, SidebarMode};

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
    /// 连接对话框的项目下拉选中「＋ 新增项目」→ 宿主打开项目新建入口（由 `WorkbenchView` 消费）。
    pub project_new_request: Rc<Cell<bool>>,
    /// P0：当前项目会话（草稿箱根 / 项目作用域连接 / 标题栏项目名共用）。
    pub project: Rc<RefCell<Option<project::ui::OpenProject>>>,
    /// M1 项目管理 UI 状态（选择器 / 菜单 / 对话框 / 设置 / 项目锁）。
    pub project_ui: Rc<RefCell<project::ui::ProjectUiState>>,
    /// 编辑区是否存在未保存草稿（切换 / 关闭项目拦截信号）。
    pub editor_dirty: Rc<Cell<bool>>,
    /// 编辑区当前 SQL 文本（未保存草稿保存时使用）。
    pub editor_sql: Rc<RefCell<String>>,
    /// 清空编辑区的宿主命令（保存 / 放弃未保存草稿后调用；事件上下文执行，非 render）。
    pub editor_clear: Rc<RefCell<Option<Rc<dyn Fn(&mut Window, &mut App)>>>>,
    /// Phase B：属性面板请求（数据源导航双击对象 → 编辑区右侧面板）。
    pub property_target: Rc<RefCell<Option<PropertyRequest>>>,
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
            project_new_request: Rc::new(Cell::new(false)),
            project: Rc::new(RefCell::new(None)),
            project_ui: Rc::new(RefCell::new(Default::default())),
            editor_dirty: Rc::new(Cell::new(false)),
            editor_sql: Rc::new(RefCell::new(String::new())),
            editor_clear: Rc::new(RefCell::new(None)),
            property_target: Rc::new(RefCell::new(None)),
            host_redraw: Rc::new(RefCell::new(None)),
        }
    }

    pub fn new() -> Self {
        Self::with_connections(Vec::new(), None)
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
}

/// 侧边栏面板发出的事件（由工作台订阅）。
#[derive(Clone, Debug)]
pub enum SidebarEvent {
    /// 用户点击了连接条目。
    SelectConnection(usize),
    /// 用户点击了连接「编辑」。
    EditConnection(String),
}

/// 侧边栏面板：按活动工具渲染内容。
pub struct SidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// M5 草稿箱面板状态（首次渲染触发加载）。
    scratchpad: Rc<RefCell<ScratchpadView>>,
    /// M4 数据源导航面板状态（懒加载对象树）。
    database_nav: Rc<RefCell<DatabaseNavView>>,
    /// 数据源导航搜索框（懒创建）。
    nav_search: Option<Entity<InputState>>,
}

/// 内联编辑（新建 / 重命名）。
#[derive(Clone)]
enum ScratchpadEdit {
    NewFile,
    NewFolder,
    Rename { path: String },
}

/// 删除撤销（底部撤销栏）。
#[derive(Clone)]
struct ScratchpadUndo {
    label: String,
    trash_id: String,
}

/// 草稿箱面板视图状态。
///
/// 数据来自 `rds-scratchpad` 存储；根 = 当前项目会话下的模块目录 `{project}/scratchpad/`。
/// 本次闭环：新建（内联）/重命名/删除→回收站+撤销/回收站恢复与清空/文件名过滤/外部引用移除。
#[derive(Default)]
struct ScratchpadView {
    /// 是否已尝试加载（首次渲染触发一次）。
    loaded: bool,
    /// 加载错误（未打开项目 / 运行时或存储错误）。
    error: Option<String>,
    /// 模块根下的条目树（depth=4 全量；懒加载后续再做）。
    entries: Vec<ScratchpadEntry>,
    /// 外部引用（来自草稿箱配置，含路径可用性）。
    external_refs: Vec<ExternalReferenceStatus>,
    /// 项目级回收站条目。
    trash: Vec<TrashEntry>,
    /// 回收站分组是否展开。
    trash_expanded: bool,
    /// 已展开的文件夹路径集合。
    expanded: HashSet<String>,
    /// 当前选中条目路径。
    selected: Option<String>,
    /// 内联编辑状态（新建/重命名）。
    edit: Option<ScratchpadEdit>,
    /// 删除撤销栏。
    undo: Option<ScratchpadUndo>,
    /// 内联名称输入（懒创建）。
    name_input: Option<Entity<InputState>>,
    _name_sub: Option<Subscription>,
    /// 文件名过滤输入（懒创建）。
    search_input: Option<Entity<InputState>>,
    _search_sub: Option<Subscription>,
}

/// 条目是否命中过滤（自身命中或子树命中）。
fn scratchpad_entry_matches(entry: &ScratchpadEntry, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    if entry.name.to_lowercase().contains(filter) {
        return true;
    }
    entry
        .children
        .as_ref()
        .map(|kids| kids.iter().any(|c| scratchpad_entry_matches(c, filter)))
        .unwrap_or(false)
}

/// 将条目树按展开状态压平成 `(缩进层级, 条目)` 行序列；有过滤时自动展开命中子树。
fn flatten_scratchpad(
    entries: &[ScratchpadEntry],
    depth: usize,
    expanded: &HashSet<String>,
    filter: &str,
    out: &mut Vec<(usize, ScratchpadEntry)>,
) {
    for entry in entries {
        if !scratchpad_entry_matches(entry, filter) {
            continue;
        }
        let key = entry.path.to_string_lossy().to_string();
        let is_folder = matches!(entry.kind, ScratchpadEntryKind::Folder);
        out.push((depth, entry.clone()));
        let open = !filter.is_empty() || expanded.contains(&key);
        if is_folder && open {
            if let Some(children) = &entry.children {
                flatten_scratchpad(children, depth + 1, expanded, filter, out);
            }
        }
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
    /// 当前来源标签页（项目 / 全局）。
    scope: NavScope,
    /// 搜索过滤词（本地筛选）。
    filter: String,
    /// 已展开节点 key。
    expanded: HashSet<String>,
    /// 父节点 key → 已加载子节点。
    children: HashMap<String, Vec<NavNode>>,
    /// 已尝试加载的节点 key（避免重复请求）。
    attempted: HashSet<String>,
    /// 节点 key → 加载失败原因。
    errors: HashMap<String, String>,
    /// 本会话运行时已连接的连接 ID。
    connected: HashSet<String>,
    /// 已从库中恢复过导航状态的连接 ID。
    state_loaded: HashSet<String>,
}

/// 懒加载导航子节点（阻塞式，与现有服务调用模式一致；Phase C 迁后台任务 + 缓存编排）。
fn load_nav_children(conn_id: &str, path: &NavPath) -> Result<Vec<NavNode>, String> {
    let service = NavigatorService::new(engine::get_connection_manager().clone());
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
    rt.block_on(service.load_children(conn_id, path))
        .map_err(|e| e.to_string())
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
}

impl SidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            scratchpad: Rc::new(RefCell::new(ScratchpadView::default())),
            database_nav: Rc::new(RefCell::new(DatabaseNavView::default())),
            nav_search: None,
        }
    }

    /// 加载草稿箱数据（阻塞式，与 workbench 现有服务调用模式一致）。
    fn load_scratchpad(&self) {
        let root = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.root.clone());

        let mut view = self.scratchpad.borrow_mut();
        view.loaded = true;
        view.error = None;
        view.entries.clear();
        view.external_refs.clear();
        view.trash.clear();

        let Some(root) = root else {
            view.error = Some("未打开项目：草稿箱根即项目目录，请先打开项目。".to_string());
            return;
        };

        let store = ScratchpadStore::new(root);
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                view.error = Some(format!("运行时错误: {e}"));
                return;
            }
        };

        let result = rt.block_on(async {
            let entries = store.list_local_entries(4).await?;
            let refs = store.external_reference_status().await?;
            let trash = store.list_trash().await?;
            Ok::<_, shared::error::CoreError>((entries, refs, trash))
        });

        match result {
            Ok((entries, refs, trash)) => {
                view.entries = entries;
                view.external_refs = refs;
                view.trash = trash;
            }
            Err(e) => view.error = Some(format!("加载草稿箱失败: {e}")),
        }
    }

    /// 构建草稿箱存储 + 运行时（未打开项目时报错）。
    fn scratchpad_store(&self) -> Result<(ScratchpadStore, tokio::runtime::Runtime), String> {
        let root = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.root.clone())
            .ok_or_else(|| "未打开项目".to_string())?;
        let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
        Ok((ScratchpadStore::new(root), rt))
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
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索文件…"));
        let search_sub = cx.subscribe_in(
            &search_input,
            window,
            |_this, _e, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
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
        let initial = match &edit {
            ScratchpadEdit::Rename { path } => std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
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
                    ScratchpadEdit::NewFile => rt.block_on(store.create_entry(&name, None, false)),
                    ScratchpadEdit::NewFolder => rt.block_on(store.create_entry(&name, None, true)),
                    ScratchpadEdit::Rename { path } => rt.block_on(store.rename_entry(path, &name)),
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

    /// 删除条目 → 项目级回收站，并记录撤销。
    fn delete_scratchpad_entry(&mut self, relative_path: String, cx: &mut Context<Self>) {
        // M1：只读打开时禁止删除草稿。
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许删除草稿".to_string());
            cx.notify();
            return;
        }
        let label = relative_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&relative_path)
            .to_string();

        let delete_result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.delete_entry(&relative_path))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };

        match delete_result {
            Ok(()) => {
                // 刚删除的条目应是回收站中删除时间最新的一条（用于撤销）。
                let trash_id = match self.scratchpad_store() {
                    Ok((store, rt)) => rt
                        .block_on(store.list_trash())
                        .ok()
                        .and_then(|v| v.into_iter().next())
                        .map(|e| e.manifest.id),
                    Err(_) => None,
                };
                let mut view = self.scratchpad.borrow_mut();
                view.undo = trash_id.map(|trash_id| ScratchpadUndo { label, trash_id });
                view.loaded = false;
                view.error = None;
            }
            Err(e) => self.scratchpad.borrow_mut().error = Some(format!("删除失败: {e}")),
        }
        cx.notify();
    }

    /// 撤销上一次删除（从回收站还原）。
    fn undo_scratchpad_delete(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.scratchpad.borrow().undo.clone() else {
            return;
        };
        let result = match self.scratchpad_store() {
            Ok((store, rt)) => rt
                .block_on(store.restore_from_trash(&undo.trash_id))
                .map(|_| ())
                .map_err(|e| e.to_string()),
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

    /// 旧连接列表（保留供后续参考；正式导航见 `render_database_nav`）。
    #[allow(dead_code)]
    fn render_connection_list(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let selected = self.shared.selected.get();
        let mut list = div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .pt_1()
            .pb_1()
            .pl_1()
            .pr_1()
            .gap_1();

        if self.shared.connections.borrow().is_empty() {
            let notice = self.shared.notice.borrow().clone();
            return div()
                .v_flex()
                .w_full()
                .h_full()
                .items_center()
                .pt_6()
                .pl_3()
                .pr_3()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(
                    notice.unwrap_or_else(|| "暂无连接，请先在「数据源连接」中创建。".to_string()),
                );
        }

        for (idx, item) in self.shared.connections.borrow().iter().enumerate() {
            let entity = cx.entity();
            let item = item.clone();
            let is_selected = selected == Some(idx);
            let edit_conn = {
                let shared = self.shared.clone();
                let entity = entity.clone();
                let cid = item.id.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    *shared.open_edit.borrow_mut() = Some(cid.clone());
                    entity.update(app, |_, cx| {
                        cx.emit(SidebarEvent::EditConnection(cid.clone()));
                    });
                }
            };
            list = list.child(
                div()
                    .id(format!("conn-{}", idx))
                    .h_flex()
                    .items_center()
                    .w_full()
                    .h_7()
                    .pl_2()
                    .pr_2()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(is_selected, |this| this.bg(theme.colors.list_active))
                    .on_click(move |_, _, app| {
                        entity.update(app, |_, cx| cx.emit(SidebarEvent::SelectConnection(idx)));
                    })
                    .child(
                        div()
                            .w_2()
                            .h_2()
                            .flex_none()
                            .rounded_full()
                            .bg(if item.connected {
                                theme.colors.success
                            } else {
                                theme.colors.muted
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(if is_selected {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(item.name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(item.driver),
                    )
                    .child(
                        div()
                            .id(format!("conn-edit-{}", idx))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .px_1()
                            .child("编辑")
                            .on_click(edit_conn),
                    ),
            );
        }
        list
    }

    /// 数据源导航面板（M4）：面板头 + 来源标签页 + 搜索 + 对象树。
    fn render_database_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.nav_search.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| {
                s.set_placeholder("筛选数据源 / 表 / 列…", window, cx)
            });
            self.nav_search = Some(state);
        }
        let filter = self
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default();
        self.database_nav.borrow_mut().filter = filter;

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let accent = cx.theme().colors.primary;
        let scope = self.database_nav.borrow().scope;

        let header = div()
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.875))
            .pl_2p5()
            .pr_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg)
                    .child("数据源"),
            )
            .child(div().flex_1());

        let tabs = div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_3()
            .pl_2p5()
            .pr_2()
            .pb_1p5()
            .child(self.nav_scope_tab(
                "项目",
                NavScope::Project,
                scope,
                border,
                accent,
                fg,
                muted,
                cx,
            ))
            .child(self.nav_scope_tab(
                "全局",
                NavScope::Global,
                scope,
                border,
                accent,
                fg,
                muted,
                cx,
            ));

        let body = self.render_nav_tree(scope, cx);

        let mut search_row = div().v_flex().w_full().px_2().pb_2();
        if let Some(input) = &self.nav_search {
            search_row = search_row.child(Input::new(input));
        }

        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .pt_1()
            .child(header)
            .child(tabs)
            .child(search_row)
            .child(body)
    }

    /// 来源标签页（项目 / 全局）。
    #[allow(clippy::too_many_arguments)]
    fn nav_scope_tab(
        &self,
        label: &str,
        target: NavScope,
        current: NavScope,
        border: Hsla,
        accent: Hsla,
        fg: Hsla,
        muted: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = target == current;
        let entity = cx.entity();
        let state = self.database_nav.clone();
        let underline = if active { accent } else { border };
        let text = if active { fg } else { muted };
        let weight = if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        div()
            .id(format!("nav-scope-{}", target.as_str()))
            .v_flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .on_click(move |_, _, app| {
                state.borrow_mut().scope = target;
                entity.update(app, |_, cx| cx.notify());
            })
            .child(
                div()
                    .text_xs()
                    .font_weight(weight)
                    .text_color(text)
                    .child(label.to_string()),
            )
            .child(div().h_0p5().w_full().bg(underline))
    }

    /// 树主体：按当前标签页渲染连接节点（受搜索词过滤）。
    fn render_nav_tree(&self, scope: NavScope, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let filter = self.database_nav.borrow().filter.to_lowercase();
        let mut column = div()
            .v_flex()
            .w_full()
            .min_h_0()
            .gap_1()
            .px_1()
            .pt_0p5()
            .pb_1();
        let conns: Vec<ConnectionItem> = self.shared.connections.borrow().iter().cloned().collect();
        let mut shown = 0usize;
        for conn in &conns {
            if !scope.contains(NavSource::from_conn_id(&conn.id)) {
                continue;
            }
            if !filter.is_empty() && !conn.name.to_lowercase().contains(&filter) {
                continue;
            }
            shown += 1;
            self.ensure_nav_state_loaded(&conn.id);
            column = column.child(self.render_connection_row(conn, cx));
        }
        if shown == 0 {
            column = column.child(
                div()
                    .w_full()
                    .pt_5()
                    .px_3()
                    .text_xs()
                    .text_color(muted)
                    .child("暂无数据源，请点击标题栏「新建连接」。"),
            );
        }
        column
    }

    /// 连接节点行（状态点 + 名称 + 来源短码 + 驱动 + 连接/断开）。
    fn render_connection_row(&self, conn: &ConnectionItem, cx: &mut Context<Self>) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let ok = cx.theme().colors.success;
        let off = cx.theme().colors.muted;
        let danger = cx.theme().colors.danger;
        let pri = cx.theme().colors.primary;

        let expanded = {
            let view = self.database_nav.borrow();
            view.expanded.contains(&conn.id)
        };
        if expanded {
            self.ensure_nav_loaded(&conn.id, &conn.id, NavPath::Connection);
        }
        let (connected, error, children) = {
            let view = self.database_nav.borrow();
            (
                view.connected.contains(&conn.id) || conn.connected,
                view.errors.get(&conn.id).cloned(),
                view.children.get(&conn.id).cloned().unwrap_or_default(),
            )
        };

        let source = NavSource::from_conn_id(&conn.id);
        let project_root = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|p| p.root.to_string_lossy().to_string());
        let conn_id = conn.id.clone();

        let mut block = div().v_flex().w_full();
        block = block.child(
            div()
                .id(format!("nav-conn-{}", conn.id))
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(1.625))
                .px_1()
                .gap_1()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click({
                    let entity = cx.entity();
                    let conn_id = conn_id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    move |ev, _, app| {
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
                            this.toggle_nav_node(&conn_id, &conn_id, NavPath::Connection);
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
                .child(
                    div()
                        .w_2()
                        .h_2()
                        .flex_none()
                        .rounded_full()
                        .bg(if connected { ok } else { off }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(fg)
                        .child(conn.name.clone()),
                )
                .child(div().text_xs().text_color(muted).child(source.code()))
                .child(div().text_xs().text_color(muted).child(conn.driver.clone()))
                // 「在对话框中编辑」入口（C7）：复用 shared.open_edit → 编辑区打开数据源对话框。
                .child(
                    div()
                        .id(format!("nav-conn-edit-{}", conn.id))
                        .px_1()
                        .text_xs()
                        .text_color(muted)
                        .cursor_pointer()
                        .child("\u{270e}")
                        .on_click({
                            let entity = cx.entity();
                            let shared = self.shared.clone();
                            let cid = conn_id.clone();
                            move |_, _, app: &mut App| {
                                *shared.open_edit.borrow_mut() = Some(cid.clone());
                                entity.update(app, |_, cx| {
                                    cx.emit(SidebarEvent::EditConnection(cid.clone()));
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .id(format!("nav-conn-toggle-{}", conn.id))
                        .px_1()
                        .text_xs()
                        .text_color(pri)
                        .cursor_pointer()
                        .child(if connected { "断开" } else { "连接" })
                        .on_click({
                            let entity = cx.entity();
                            let conn_id = conn_id.clone();
                            let root = project_root.clone();
                            move |_, _, app: &mut App| {
                                let conn_id = conn_id.clone();
                                let root = root.clone();
                                entity.update(app, |this, cx| {
                                    let already =
                                        this.database_nav.borrow().connected.contains(&conn_id);
                                    let outcome = if already {
                                        crate::services::nav_runtime::disconnect_entry(&conn_id)
                                            .map(|_| false)
                                    } else {
                                        crate::services::nav_runtime::connect_entry(
                                            &conn_id,
                                            root.as_deref(),
                                        )
                                        .map(|_| true)
                                    };
                                    match outcome {
                                        Ok(is_connected) => {
                                            let mut view = this.database_nav.borrow_mut();
                                            if is_connected {
                                                view.connected.insert(conn_id.clone());
                                            } else {
                                                view.connected.remove(&conn_id);
                                            }
                                        }
                                        Err(e) => {
                                            *this.shared.notice.borrow_mut() =
                                                Some(format!("连接操作失败: {e}"));
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                ),
        );

        if let Some(err) = error {
            block = block.child(div().pl_6().pb_1().text_xs().text_color(danger).child(err));
        }

        if expanded {
            for child in children {
                block = block.child(self.render_nav_node(&child, 1, cx));
            }
        }

        block
    }

    /// 对象树节点行（懒加载；叶子不可展开）。
    fn render_nav_node(&self, node: &NavNode, depth: usize, cx: &mut Context<Self>) -> Div {
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
                self.ensure_nav_loaded(&node.connection_id, &node.key, p);
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

        let indent = 10.0 + depth as f32 * 12.0;
        let mut block = div().v_flex().w_full();
        let mut row = div()
            .id(format!("nav-node-{}", node.key))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.375))
            .pr_1()
            .pl(rems(indent / 4.))
            .gap_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(div().w_2p5().flex_none().text_xs().text_color(muted).child(
                if node.has_children {
                    if expanded_eff { "\u{25be}" } else { "\u{25b8}" }
                } else {
                    ""
                },
            ))
            .child(div().w_2().h_2().flex_none().rounded_sm().bg(icon))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(fg)
                    .child(node.name.clone()),
            );
        if let Some((meta, is_pk)) = right_meta {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(if is_pk { pri } else { muted })
                    .child(meta),
            );
        }
        row = row.on_click(move |ev, _, app| {
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
                    this.toggle_nav_node(&conn_id, &key, p);
                    cx.notify();
                });
            }
        });
        block = block.child(row);

        if let Some(err) = error {
            block = block.child(
                div()
                    .pl(rems((indent + 18.0) / 4.))
                    .pb_1()
                    .text_xs()
                    .text_color(danger)
                    .child(err),
            );
        }
        if expanded_eff {
            for child in children {
                block = block.child(self.render_nav_node(&child, depth + 1, cx));
            }
        }
        block
    }

    /// 展开 / 折叠节点；首次展开时懒加载子节点，并持久化展开态。
    fn toggle_nav_node(&self, conn_id: &str, key: &str, path: NavPath) {
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
            self.ensure_nav_loaded(conn_id, key, path);
        }
        self.save_nav_state_for(conn_id);
    }

    /// 懒加载子节点（已加载则跳过）。
    fn ensure_nav_loaded(&self, conn_id: &str, key: &str, path: NavPath) {
        {
            let view = self.database_nav.borrow();
            if view.attempted.contains(key) {
                return;
            }
        }
        self.database_nav
            .borrow_mut()
            .attempted
            .insert(key.to_string());
        let result = load_nav_children(conn_id, &path);
        let mut view = self.database_nav.borrow_mut();
        match result {
            Ok(children) => {
                view.errors.remove(key);
                view.children.insert(key.to_string(), children);
            }
            Err(e) => {
                view.errors.insert(key.to_string(), e);
            }
        }
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

    /// 内联编辑行（新建 / 重命名通用）。
    fn render_scratchpad_edit_row(&self, depth: usize, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;

        let Some(input) = self.scratchpad.borrow().name_input.clone() else {
            return div();
        };
        let entity = cx.entity();

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

        div()
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.625))
            .gap_1()
            .px_1()
            .child(div().w(rems(depth as f32 * 3.0)).flex_none())
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
            )
    }

    /// 草稿箱面板（M5）：根 = `{project}/scratchpad/`。
    ///
    /// 闭环：新建（内联）/重命名/删除→回收站+撤销栏/回收站恢复与清空/文件名过滤/外部引用移除。
    fn render_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if !self.scratchpad.borrow().loaded {
            self.load_scratchpad();
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
            edit,
            undo,
            filter,
        ) = {
            let view = self.scratchpad.borrow();
            let filter = view
                .search_input
                .as_ref()
                .map(|i| i.read(cx).value().to_lowercase().trim().to_string())
                .unwrap_or_default();
            let mut flat = Vec::new();
            flatten_scratchpad(&view.entries, 0, &view.expanded, &filter, &mut flat);
            (
                flat,
                view.error.clone(),
                view.external_refs.clone(),
                view.trash.clone(),
                view.trash_expanded,
                view.selected.clone(),
                view.expanded.clone(),
                view.edit.clone(),
                view.undo.clone(),
                filter,
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

        let toolbar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .py_1()
            .child(tool_btn("sp-new-file", "＋", Box::new(start_new_file)))
            .child(tool_btn("sp-new-folder", "🗀", Box::new(start_new_folder)))
            .child(div().flex_1())
            .child(tool_btn("sp-refresh", "↻", Box::new(refresh)));

        // ── 搜索（文件名过滤）──
        let mut search_row = div().w_full().px_1p5().pb_1();
        if let Some(input) = self.scratchpad.borrow().search_input.clone() {
            search_row = search_row.child(Input::new(&input).w_full());
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

        let mut body = div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .gap_1()
            .px_1()
            .pb_1();

        // 顶部内联新建（新建文件/文件夹）。
        if matches!(
            edit.as_ref(),
            Some(ScratchpadEdit::NewFile) | Some(ScratchpadEdit::NewFolder)
        ) {
            body = body.child(self.render_scratchpad_edit_row(0, cx));
        }

        if rows.is_empty() {
            let hint = if filter.is_empty() {
                "用上方「＋」新建草稿。"
            } else {
                "没有匹配的文件。"
            };
            body = body.child(
                div()
                    .v_flex()
                    .items_center()
                    .w_full()
                    .pt_6()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .child("草稿箱还没有文件")
                    .child(div().mt_1().child(hint)),
            );
        } else {
            body = body.child(group_header("草稿", rows.len()));
            for (depth, entry) in &rows {
                let key = entry.path.to_string_lossy().to_string();

                // 本行正在重命名 → 渲染内联输入。
                if let Some(ScratchpadEdit::Rename { path }) = &edit {
                    if path == &key {
                        body = body.child(self.render_scratchpad_edit_row(*depth, cx));
                        continue;
                    }
                }

                let is_folder = entry.kind == ScratchpadEntryKind::Folder;
                let has_children = entry
                    .children
                    .as_ref()
                    .map(|c| !c.is_empty())
                    .unwrap_or(false);
                let is_selected = selected.as_deref() == Some(key.as_str());
                let is_expanded = expanded.contains(&key);

                let click = {
                    let view = view_handle.clone();
                    let entity = entity.clone();
                    let key = key.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        {
                            let mut v = view.borrow_mut();
                            v.selected = Some(key.clone());
                            if is_folder {
                                if v.expanded.contains(&key) {
                                    v.expanded.remove(&key);
                                } else {
                                    v.expanded.insert(key.clone());
                                }
                            }
                        }
                        entity.update(app, |_, cx| cx.notify());
                    }
                };

                let chevron = if is_folder && has_children {
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
                    .h_6()
                    .gap_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .when(is_selected, |this| this.bg(selected_bg))
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(click)
                    .child(div().w(rems(*depth as f32 * 3.0)).flex_none())
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
                            .child(entry.name.clone()),
                    );

                // 选中行才显示操作（重命名 / 删除）。
                if is_selected {
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

                body = body.child(row);
            }
        }

        // ── 外部引用（移除）──
        if !external_refs.is_empty() {
            body = body.child(group_header("外部引用", external_refs.len()));
            for r in &external_refs {
                let remove = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.remove_scratchpad_reference(alias.clone(), cx)
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

        panel = panel.child(body);

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
        let file_count = rows
            .iter()
            .filter(|(_, e)| e.kind == ScratchpadEntryKind::File)
            .count();
        let folder_count = rows.len() - file_count;
        panel = panel.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(format!(
                    "{file_count} 个文件 · {folder_count} 个文件夹 · {} 项引用 · {} 项回收站",
                    external_refs.len(),
                    trash.len()
                )),
        );

        panel
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
        }
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
                true
            } else {
                false
            }
        };
        if needs_load {
            let service = NavigatorService::new(engine::get_connection_manager().clone());
            let loaded = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(service.load_properties(
                    &target.property,
                    &target.conn_label,
                    &target.driver,
                )),
                Err(e) => Err(shared::error::CoreError::common(
                    shared::error::CommonError::General(format!("运行时错误: {e}")),
                )),
            };
            let mut st = self.property.borrow_mut();
            match loaded {
                Ok(p) => st.props = Some(p),
                Err(e) => st.error = Some(e.to_string()),
            }
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
            .w(rems(24.5))
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
                                move |_, _, app: &mut App| {
                                    *shared.property_target.borrow_mut() = None;
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

        content = content.child(
            Button::new("mock-generate-btn")
                .primary()
                .label("生成 Mock")
                .on_click(|_, _, _| {}),
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

        let mut root = div().h_flex().size_full();
        root = root.child(div().flex_1().min_w_0().child(content));
        if self.shared.property_target.borrow().is_some() {
            root = root.child(self.render_property_panel(cx));
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

/// 右侧边栏面板：洞察 / Mock 生成 / 历史（占位视图，业务下一轮接入）。
pub struct RightSidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl RightSidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
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

    fn render_mock_placeholder(&self, fg: Hsla) -> Div {
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
                    .child("Mock 生成"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 表结构模板（占位）"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 生成任务（占位）"),
            )
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
        let fg = cx.theme().colors.foreground;
        let active = self.shared.active_right.get();
        let content: Div = match active {
            RightPanel::Insight => self.render_insight_placeholder(fg),
            RightPanel::Mock => self.render_mock_placeholder(fg),
            RightPanel::History => self.render_history_placeholder(fg),
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
