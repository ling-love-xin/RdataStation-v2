//! 项目管理视图（M1）：选择器 / 标题栏项目菜单 / 项目设置 / 语义对话框。
//!
//! 本模块随 `project` crate 一同发布（GPUI-kit 指南：model / service / view 同 crate），
//! 不依赖 workbench 与 settings——宿主（工作台）通过 [`ProjectUiHost`] 注入状态句柄、
//! 重绘桥、编辑区桥、排序偏好与打开后回调。
//!
//! 选择器与项目设置由宿主渲染为受控叠加层（宿主 render 是权威同步点）；标题栏项目菜单
//! 用语义 `Button::dropdown_menu`（方向键导航 / Escape / 焦点恢复由组件负责），创建 /
//! 打开 / 删除 / 重定位 / 锁占用 / 未保存拦截一律用语义组件 `Dialog` / `AlertDialog`
//! （焦点陷阱、Escape、点击遮罩关闭由组件负责）。
//!
//! 语义见 `docs/architecture/project/project-prototype-design.md`。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{Disableable as _, Icon, IconName, Sizable as _, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// 结构尺寸与树/列表行原语：统一走 workbench_shell（shell 不依赖特性 crate，方向安全）
use workbench_shell::{tree, ui};

use crate::service::{
    self as project_service, OpenOutcome, ProjectSummary, ProjectVersionRow, TargetDirState,
};

/// 选择器视图 Tab。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerTab {
    Recent,
    All,
    Removed,
}

impl PickerTab {
    pub fn label(self) -> &'static str {
        match self {
            PickerTab::Recent => "最近",
            PickerTab::All => "全部",
            PickerTab::Removed => "已移除",
        }
    }

    /// 稳定标识键（ElementId 用，不用本地化 label）。
    pub fn key(self) -> &'static str {
        match self {
            PickerTab::Recent => "recent",
            PickerTab::All => "all",
            PickerTab::Removed => "removed",
        }
    }
}

/// 排序方式（跨会话保留由设置层负责，本期先会话内生效）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSort {
    LastOpened,
    Name,
    Created,
}

impl ProjectSort {
    /// 全部排序（下拉菜单展示顺序）。
    pub const ALL: [ProjectSort; 3] = [
        ProjectSort::LastOpened,
        ProjectSort::Name,
        ProjectSort::Created,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ProjectSort::LastOpened => "最近打开",
            ProjectSort::Name => "名称",
            ProjectSort::Created => "创建时间",
        }
    }

    /// 持久化键（写入 settings.projects.sort_mode）。
    pub fn key(self) -> &'static str {
        match self {
            ProjectSort::LastOpened => "last_opened",
            ProjectSort::Name => "name",
            ProjectSort::Created => "created",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "name" => ProjectSort::Name,
            "created" => ProjectSort::Created,
            _ => ProjectSort::LastOpened,
        }
    }
}

/// 选择器状态筛选（原型 R4；不持久化，属临时视图状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    #[default]
    All,
    Active,
    Archived,
    Offline,
    Syncing,
}

impl StatusFilter {
    /// 全部状态（下拉菜单展示顺序）。
    pub const ALL: [StatusFilter; 5] = [
        StatusFilter::All,
        StatusFilter::Active,
        StatusFilter::Archived,
        StatusFilter::Offline,
        StatusFilter::Syncing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StatusFilter::All => "全部",
            StatusFilter::Active => "Active",
            StatusFilter::Archived => "Archived",
            StatusFilter::Offline => "Offline",
            StatusFilter::Syncing => "Syncing",
        }
    }

    /// 稳定标识键（ElementId 用，不用本地化 label）。
    pub fn key(self) -> &'static str {
        match self {
            StatusFilter::All => "all",
            StatusFilter::Active => "active",
            StatusFilter::Archived => "archived",
            StatusFilter::Offline => "offline",
            StatusFilter::Syncing => "syncing",
        }
    }

    /// 名册条目是否命中（`All` 命中任意状态）。
    fn matches(self, status: &str) -> bool {
        match self {
            StatusFilter::All => true,
            StatusFilter::Active => status == "active",
            StatusFilter::Archived => status == "archived",
            StatusFilter::Offline => status == "offline",
            StatusFilter::Syncing => status == "syncing",
        }
    }
}

/// 状态筛选菜单项描述（渲染与测试共用同一份判据，避免断言菜单内部结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusFilterMenuSpec {
    pub filter: StatusFilter,
    /// 当前筛选（勾选并置灰，单选语义）。
    pub checked: bool,
}

/// 状态筛选菜单项（顺序 = [`StatusFilter::ALL`]；当前项勾选置灰）。
pub fn status_filter_menu_entries(current: StatusFilter) -> Vec<StatusFilterMenuSpec> {
    StatusFilter::ALL
        .into_iter()
        .map(|filter| StatusFilterMenuSpec {
            filter,
            checked: filter == current,
        })
        .collect()
}

/// 排序菜单项描述（渲染与测试共用同一份判据，避免断言菜单内部结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortMenuSpec {
    pub sort: ProjectSort,
    /// 当前排序（勾选并置灰，单选语义）。
    pub checked: bool,
}

/// 排序菜单项（顺序 = [`ProjectSort::ALL`]；当前项勾选置灰）。
pub fn sort_menu_entries(current: ProjectSort) -> Vec<SortMenuSpec> {
    ProjectSort::ALL
        .into_iter()
        .map(|sort| SortMenuSpec {
            sort,
            checked: sort == current,
        })
        .collect()
}

/// 选择器搜索的 facet 解析结果（`状态:` / `固定:` / `锁:`）。
///
/// 与搜索框旁边的状态筛选按钮**叠加（AND）**：facet 只做额外约束，不写回筛选按钮
/// （避免“输入框回写 → 重解析”的反馈环与光标跳动）；口径对齐 database 导航面板。
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    /// 自由文本（去掉 facet token 后的剩余）：匹配名称 / 描述 / 路径子串。
    pub free: String,
    /// `状态:active|archived|offline|syncing`；「全部」/「all」= 不约束。
    pub status: Option<String>,
    /// `固定:是|否`。
    pub pinned: Option<bool>,
    /// `锁:是|否`（已被另一实例占用）。
    pub locked: Option<bool>,
}

impl SearchQuery {
    /// 是否含 facet（选择器可用来提示「已按 facet 收窄」）。
    pub fn has_facet(&self) -> bool {
        self.status.is_some() || self.pinned.is_some() || self.locked.is_some()
    }
}

/// 解析选择器搜索文本：只有**已知键 + 可识别值**才算 facet。
///
/// 认不出的 token（包括正在输入中的半截如 `状态:离`）**原样留在自由文本**里，与
/// database 导航面板同一口径——否则光标还在输入框里，列表就会被一个半成品 token 清空。
/// 注：Windows 路径里的 `C:` 不会被误当 facet（键不在已知集合里）。
pub fn parse_search(raw: &str) -> SearchQuery {
    fn boolean(value: &str) -> Option<bool> {
        match value.to_ascii_lowercase().as_str() {
            "是" | "true" | "yes" | "1" => Some(true),
            "否" | "false" | "no" | "0" => Some(false),
            _ => None,
        }
    }

    let mut query = SearchQuery::default();
    let mut free: Vec<&str> = Vec::new();
    for token in raw.split_whitespace() {
        let Some((key, value)) = token.split_once(':') else {
            free.push(token);
            continue;
        };
        if value.is_empty() {
            free.push(token);
            continue;
        }
        match key {
            "状态" => match value.to_ascii_lowercase().as_str() {
                "全部" | "all" => query.status = None,
                status @ ("active" | "archived" | "offline" | "syncing") => {
                    query.status = Some(status.to_string());
                }
                _ => free.push(token),
            },
            "固定" => match boolean(value) {
                Some(pinned) => query.pinned = Some(pinned),
                None => free.push(token),
            },
            "锁" => match boolean(value) {
                Some(locked) => query.locked = Some(locked),
                None => free.push(token),
            },
            _ => free.push(token),
        }
    }
    query.free = free.join(" ").trim().to_string();
    query
}

/// 选择器可见条目：facet 约束（`状态:` / `固定:` / `锁:`）+ 自由文本子串
/// （名称 / 描述 / 路径，大小写不敏感）+ 状态筛选按钮。
///
/// 抽成纯函数以便单测；渲染时直接调用（避免每次输入都重查数据库）。
fn visible_items(
    items: &[ProjectSummary],
    needle: &str,
    status: StatusFilter,
) -> Vec<ProjectSummary> {
    let query = parse_search(needle);
    let text = query.free.to_lowercase();
    items
        .iter()
        .filter(|p| {
            let description = p.description.as_deref().unwrap_or_default().to_lowercase();
            status.matches(&p.status)
                && query.status.as_deref().is_none_or(|s| p.status == s)
                && query.pinned.is_none_or(|pinned| p.is_pinned == pinned)
                && query.locked.is_none_or(|locked| p.lock.is_some() == locked)
                && (text.is_empty()
                    || p.name.to_lowercase().contains(&text)
                    || description.contains(&text)
                    || p.path.to_string_lossy().to_lowercase().contains(&text))
        })
        .cloned()
        .collect()
}

/// 选择器状态。
#[non_exhaustive]
pub struct PickerState {
    pub tab: PickerTab,
    pub sort: ProjectSort,
    /// 状态筛选（原型 R4）；不持久化。
    pub status_filter: StatusFilter,
    pub items: Vec<ProjectSummary>,
    pub loaded: bool,
    pub error: Option<String>,
}

impl Default for PickerState {
    fn default() -> Self {
        Self {
            tab: PickerTab::Recent,
            sort: ProjectSort::LastOpened,
            status_filter: StatusFilter::All,
            items: Vec::new(),
            loaded: false,
            error: None,
        }
    }
}

/// 待处理的项目动作（未保存草稿拦截用）。
///
/// `CreateProject` / `OpenFolder` 携带输入实体：确认后**直接**打开目标对话框，
/// 不用用户回选择器再点一次（#4）。
#[derive(Debug, Clone)]
pub enum PendingAction {
    OpenPath(PathBuf),
    /// 切换到另一个项目：**先释放当前项目**（锁 + 宿主清理）再打开目标。
    /// 与 `OpenPath` 的区别是后者假定当前没有项目（打开即接管）。
    SwitchTo(PathBuf),
    Close,
    CreateProject(ProjectInputs),
    OpenFolder(ProjectInputs),
}

// ==================== 宿主桥 ====================

/// 当前打开的项目（根目录 + 显示名）。
///
/// 宿主（工作台）与项目视图共用；命名沿用产品语义「一个实例一个项目」。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OpenProject {
    pub root: PathBuf,
    pub name: String,
}

impl OpenProject {
    pub fn new(root: PathBuf, name: impl Into<String>) -> Self {
        Self {
            root,
            name: name.into(),
        }
    }

    /// 由根目录推导显示名（取末级目录名，空则「未命名项目」）。
    pub fn from_root(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "未命名项目".to_string());
        Self { root, name }
    }
}

/// 宿主重绘桥（宿主把自身视图实体的 notify 注入）。
pub trait ProjectUiNotifier: 'static {
    fn notify(&self, cx: &mut App);
}

/// 编辑区桥：未保存草稿判定 / 草稿内容 / 清空编辑区。
pub trait ProjectEditorBridge: 'static {
    fn is_dirty(&self) -> bool;
    fn sql(&self) -> String;
    fn clear(&self, window: &mut Window, cx: &mut App);
    /// 清除脏标记（打开 / 关闭项目、草稿落盘后）。
    fn mark_clean(&self);
}

/// 未接编辑区的空实现（测试 / 无编辑区宿主）。
impl ProjectEditorBridge for () {
    fn is_dirty(&self) -> bool {
        false
    }

    fn sql(&self) -> String {
        String::new()
    }

    fn clear(&self, _window: &mut Window, _cx: &mut App) {}

    fn mark_clean(&self) {}
}

/// 宿主注入的能力集合：项目视图的全部外部依赖都从这里取，
/// 宿主桥回调类型（等价于原来的 `Rc<dyn Fn…>`，抽成别名只为可读性与签名统一）。
pub type SortSaver = Rc<dyn Fn(ProjectSort, &mut App)>;
/// 宿主侧「打开 / 关闭后的刷新」回调。
pub type HostAction = Rc<dyn Fn(&mut App)>;
/// 默认连接候选来源。
pub type ConnectionProvider = Rc<dyn Fn(&App) -> Vec<ConnectionOption>>;
/// 当前构建标识来源。
pub type BuildTagProvider = Rc<dyn Fn() -> String>;
/// 「上次新建用的父目录」读取。
pub type CreateLocationGetter = Rc<dyn Fn(&App) -> Option<String>>;
/// 「上次新建用的父目录」写入。
pub type CreateLocationSaver = Rc<dyn Fn(&str, &mut App)>;

/// 需要**主窗口**才能完成的动作。
///
/// 项目列表现在既能挂在主窗口的中央 tab（无项目时），也能嵌进全局设置的
/// 「项目」页（有项目时）。后者点击这些入口时，对话框（未保存草稿 / 只读 /
/// 锁占用 / 新建 / 选目录）与切换副作用（`do_close` → 关会话、`open_path` →
/// 重配 Dock）都应该发生在主窗口，故交给宿主投递。
#[derive(Clone, Debug)]
pub enum HomeAction {
    /// 切到某个项目（未保存草稿时先弹拦截）。
    Switch(PathBuf),
    /// 新建项目（创建成功后自动切过去）。
    CreateProject,
    /// 打开现有目录（同上）。
    OpenFolder,
    /// 建示例项目（同上）。
    Sample,
    /// 关闭当前项目。
    Close,
}

/// 动作投递口：`Some` 时 [`HomeAction`] 交给宿主**在主窗口**执行。
///
/// 入参带 `&ProjectUiHost`（调用方自身）——宿主实现里直接复用它调
/// `request_switch` 等内置函数，不必再持一份 host 造成自引用。
pub type HomeDispatch = Rc<dyn Fn(&ProjectUiHost, HomeAction, &mut App)>;

/// 「项目中心」入口（= 全局设置的「项目」页）：标题栏菜单 / 选择器菜单点它跳过去。
///
/// 无 Dock 时代项目中心不再是一层覆盖，宿主把它接到设置窗口（窗口已开着则切节）。
pub type HubOpener = Rc<dyn Fn(&mut Window, &mut App)>;

/// 宿主桥：视图只从这里取能力，不直接依赖 workbench / 设置服务。
///
/// `project` crate 因而不依赖 workbench 与 settings。
#[derive(Clone)]
#[non_exhaustive]
pub struct ProjectUiHost {
    /// 项目 UI 状态（选择器 / 菜单 / 设置 / 对话框错误 / 锁 / 只读 / 世代）。
    pub state: Rc<RefCell<ProjectUiState>>,
    /// 当前会话（根 + 名称）。
    pub session: Rc<RefCell<Option<OpenProject>>>,
    /// 重绘宿主视图。
    pub notifier: Rc<dyn ProjectUiNotifier>,
    /// 编辑区桥。
    pub editor: Rc<dyn ProjectEditorBridge>,
    /// 排序偏好持久化（宿主接设置服务）。
    pub save_sort: SortSaver,
    /// 打开项目后的宿主刷新（连接列表 / 导航缓存 / 结果归属）。
    pub on_opened: HostAction,
    /// 关闭项目后的宿主清理（#21：连接列表按无项目作用域刷新，清掉残留 P_/GP_）。
    pub on_closed: HostAction,
    /// 默认连接的候选来源（U3）：宿主接连接列表，project crate 只读结果。
    pub connections: ConnectionProvider,
    /// 当前构建标识（宿主接应用版本）。
    ///
    /// `None` = 未注入：打开项目时不记录 `rds.build_tag`（测试与无版本场景走这条）。
    /// project crate 不知道应用版本，值只能由宿主给。
    pub build_tag: Option<BuildTagProvider>,
    /// 上次新建项目用的父目录（宿主接设置服务）；`None` = 回退用户主目录。
    pub last_create_location: CreateLocationGetter,
    /// 记住本次新建用的父目录（创建成功后调用）。
    pub save_create_location: CreateLocationSaver,
    /// 需要主窗口的动作投递口（设置窗口里嵌列表时由宿主填；`None` = 就地执行）。
    pub home: Rc<RefCell<Option<HomeDispatch>>>,
    /// 「项目中心」（全局设置「项目」页）入口（宿主填；`None` = 未接，点了不动作）。
    pub on_open_hub: Rc<RefCell<Option<HubOpener>>>,
}

impl ProjectUiHost {
    /// 最小构造：宿主只需提供状态与重绘时使用；其余能力用 `with_*` 补。
    pub fn new(
        state: Rc<RefCell<ProjectUiState>>,
        session: Rc<RefCell<Option<OpenProject>>>,
        notifier: Rc<dyn ProjectUiNotifier>,
    ) -> Self {
        Self {
            state,
            session,
            notifier,
            editor: Rc::new(()),
            save_sort: Rc::new(|_, _| {}),
            on_opened: Rc::new(|_| {}),
            on_closed: Rc::new(|_| {}),
            connections: Rc::new(|_| Vec::new()),
            build_tag: None,
            last_create_location: Rc::new(|_| None),
            save_create_location: Rc::new(|_, _| {}),
            home: Rc::new(RefCell::new(None)),
            on_open_hub: Rc::new(RefCell::new(None)),
        }
    }

    /// 注入「需要主窗口的动作」投递口（设置窗口里嵌列表时用，见 [`HomeAction`]）。
    pub fn with_home(self, home: HomeDispatch) -> Self {
        *self.home.borrow_mut() = Some(home);
        self
    }

    /// 注入「项目中心」入口（= 打开全局设置的「项目」页）。
    pub fn with_hub_opener(self, opener: HubOpener) -> Self {
        *self.on_open_hub.borrow_mut() = Some(opener);
        self
    }

    pub fn with_editor(mut self, editor: Rc<dyn ProjectEditorBridge>) -> Self {
        self.editor = editor;
        self
    }

    pub fn with_sort_saver(mut self, save_sort: SortSaver) -> Self {
        self.save_sort = save_sort;
        self
    }

    pub fn with_on_opened(mut self, on_opened: HostAction) -> Self {
        self.on_opened = on_opened;
        self
    }

    /// 注入关闭项目后的宿主清理回调（#21：`do_close` 末尾触发）。
    pub fn with_on_closed(mut self, on_closed: HostAction) -> Self {
        self.on_closed = on_closed;
        self
    }

    /// 注入默认连接候选来源（U3）；不注入时下拉里只有「不设默认」。
    pub fn with_connections(mut self, connections: ConnectionProvider) -> Self {
        self.connections = connections;
        self
    }

    /// 注入当前构建标识（打开项目时写进 `ProjectConfig.extensions`，只写不展示）。
    pub fn with_build_tag(mut self, build_tag: BuildTagProvider) -> Self {
        self.build_tag = Some(build_tag);
        self
    }

    /// 注入「上次新建用的父目录」读写（宿主接设置服务）。
    pub fn with_create_location(
        mut self,
        get: CreateLocationGetter,
        save: CreateLocationSaver,
    ) -> Self {
        self.last_create_location = get;
        self.save_create_location = save;
        self
    }

    /// 请求宿主重绘。
    pub fn notify(&self, cx: &mut App) {
        self.notifier.notify(cx);
    }

    /// 当前会话快照。
    pub fn current(&self) -> Option<OpenProject> {
        self.session.borrow().clone()
    }

    /// 记录当前会话。
    pub fn set_current(&self, project: Option<OpenProject>) {
        *self.session.borrow_mut() = project;
    }

    /// 当前项目根目录（未打开时为 `None`）。
    pub fn root(&self) -> Option<PathBuf> {
        self.session.borrow().as_ref().map(|s| s.root.clone())
    }
}

/// 默认连接的候选项（U3）：宿主把系统级 / 项目级连接列表投影成这一层最小信息。
///
/// `project` crate 不依赖 database / workbench，所以只认识 id + 名称；名字是给用户看的，
/// id 才是写回 `settings.json` 的值（重名连接靠 id 区分）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionOption {
    pub id: String,
    pub name: String,
}

/// 项目 UI 状态（挂在宿主共享状态与 [`ProjectUiHost`] 上）。
#[derive(Default)]
#[non_exhaustive]
pub struct ProjectUiState {
    pub picker: PickerState,
    pub settings_open: bool,
    /// 当前对话框的校验错误：对话框 builder 每帧读取，提交失败时写入并重绘。
    pub dialog_error: Option<String>,
    /// 当前持有的项目写锁（只读打开时为 `None`）。
    pub lock: Option<crate::ProjectLock>,
    pub read_only: bool,
    /// 版本台账行内删除的二次确认：`Some(id)` 时该行显示「确认删除 / 取消」。
    pub version_delete_confirm: Option<String>,
    pub notice: Option<String>,
    /// 设置面板的磁盘 / 名册快照（B2 / B3；事件路径填充，render 纯读）。
    pub settings: SettingsSnapshot,
}

/// 项目相关输入实体（由宿主懒创建并持有）。
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ProjectInputs {
    pub search: Entity<InputState>,
    pub create_name: Entity<InputState>,
    pub create_location: Entity<InputState>,
    pub create_desc: Entity<InputState>,
    pub delete_confirm: Entity<InputState>,
    /// 项目设置中的名称编辑（重命名）。
    pub rename: Entity<InputState>,
    /// 项目设置中的描述编辑（U2）。
    pub description: Entity<InputState>,
    /// 项目设置中的版本说明输入。
    pub version_msg: Entity<InputState>,
}

impl ProjectInputs {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            search: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("搜索名称 / 描述 / 路径（可用 状态:archived 固定:是 锁:否）")
            }),
            create_name: cx.new(|cx| InputState::new(window, cx)),
            create_location: cx.new(|cx| InputState::new(window, cx)),
            create_desc: cx.new(|cx| InputState::new(window, cx)),
            delete_confirm: cx.new(|cx| InputState::new(window, cx)),
            rename: cx.new(|cx| InputState::new(window, cx)),
            description: cx.new(|cx| InputState::new(window, cx)),
            version_msg: cx.new(|cx| InputState::new(window, cx)),
        }
    }
}

/// 默认位置（用户主目录）。
fn default_location() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string())
}

// ==================== 子模块 ====================
//
// `ui.rs` 只留公共面（类型、宿主、输入实体），实现按职责拆成六个子模块，
// 靠末尾的 `pub use` 保持 `project::ui::*` 的对外路径不变。
// 依赖方向单向：子模块 -> 父模块（`use super::*`），子模块之间不互相 `use`。
//

mod actions;
mod dialogs;
mod host;
mod picker;
mod settings;

pub use actions::*;
pub use dialogs::*;
// `host` 里只有 crate 内辅助（只读守卫 / 反馈通道 / 名册反查），不对外导出
use host::*;
pub use picker::*;
pub use settings::*;

#[cfg(test)]
mod tests;
