//! 草稿箱面板（M5）：文件树 / 内联编辑 / 剪贴板 / 回收站 / 内容搜索 / 替换。
//!
//! 本 crate（`scratchpad`）自带视图；宿主能力经 [`ScratchpadHost`] 注入（`ScratchpadView::new`），
//! 与 `mock` / `insight` / `analytics_resource` / `database` 同形。
//!
//! ## 模块地图（2026-09-20 拆分；总入口仍是本文件）
//!
//! 拆分前是一个 4101 行的单文件——改一处 UI 要在一屏里找半天，而且两个写入者同时改极易冲突
//! （本文件有并发写损坏史，见 `database-nav-dev-plan.md` 的「并行写冲突记录」）。现在按
//! 「这一行长什么样 / 状态怎么变 / 外壳」分家：
//!
//! | 文件 | 职责 |
//! | --- | --- |
//! | `scratchpad_view.rs`（本文件） | `ScratchpadView` 结构与协议 + 状态类型 + 行输入模型 |
//! | `scratchpad_view/primitives.rs` | 纯函数与值模型（模板 / 排序 / 压平 / 文案 / 脏点 / 类型色点） |
//! | `scratchpad_view/search.rs` | 内容搜索 / 替换 / 冲突 Diff（中央编辑区两个面板，跨 crate 公开面） |
//! | `scratchpad_view/rows.rs` | 草稿树的行（行高预算 / 单行渲染 / 内联编辑行 / 空态） |
//! | `scratchpad_view/chrome.rs` | 面板外壳（工具栏 / 搜索行 / 树装配 / 引用 / 回收站 / 撤销栏 / 状态行） |
//! | `scratchpad_view/actions.rs` | 状态变更与后台接线（加载 / 监控 / 编辑 / 删除 / 剪贴板 / 引用） |
//! | `scratchpad_view/dnd.rs` | 拖拽幽灵（拖入编辑器时跟着鼠标的胶囊） |
//! | `scratchpad_view/tests.rs` | 单测（自本文件整体搬出） |
//!
//! 两条拆分规则（后续扩展请沿用）：
//! 1. **文件名不变**：保留 `scratchpad_view.rs` 作模块根（Rust 2018 允许 `foo.rs` + `foo/` 并存），
//!    文档与 skills 里的路径引用因此零改动；
//! 2. **跨模块项加 `pub(super)`**：子模块能看见祖先的私有项，但根与兄弟模块看不见子模块的私有项；
//!    `pub` 是**跨 crate 公开面**（`new` / `render_scratchpad` / `ensure_scratchpad_pump` /
//!    两个中央区渲染函数），拆分时不得降级。状态类型与行输入模型（`ScratchpadViewState` /
//!    `ScratchpadRowCtx` / `ScratchpadRowColors`）留在根，字段可见性因此无需改动。
//!
//! 依赖分工（下沉后不再有 `Shared`）：
//! - 项目根 / 只读判定 / 提示 / 宿主重绘 / 搜索结果落地 / 在编辑器中打开 → [`ScratchpadHost`]；
//! - 草稿库、回收站、文件监控 → 本 crate（`store` / `trash` / `watch`）；
//! - 重操作（读盘 / 导入 / 搜索 / 替换）→ [`crate::jobs`] 的工作线程，本模块只入队与回填。
//!
//! 结构设计：`docs/architecture/layout/panels-coupling-plan.md` §9。

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::base::Disableable as _;
use gpui_kit::base::{StyledExt, VirtualListScrollHandle, v_virtual_list};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::commands::{
    ScratchpadCancelEdit, ScratchpadDelete, ScratchpadDown, ScratchpadNewFile, ScratchpadOpen,
    ScratchpadRename, ScratchpadSelectAll, ScratchpadUp,
};

use crate::{
    DiffLineKind, DiffResult, ExternalReferenceStatus, ScratchpadEntry, ScratchpadEntryKind,
    ScratchpadStore, TrashEntry,
};

use crate::jobs as scratchpad_jobs;

use crate::ScratchpadWatcher;

use workbench_shell::tree;
use workbench_shell::ui;

use crate::host::ScratchpadHost;

// ===== 子模块（拆分见 `tools/split_scratchpad_view.py`） =====
//
// 为什么保留 `scratchpad_view.rs` 这个文件名并另建同名目录：文档与 skills 里有多处路径引用，
// 改文件名等于同步改文档；Rust 2018 允许 `foo.rs` + `foo/` 并存，于是**路径稳定**、拆分零外部改动。
// 脚本是一次性迁移工具（已执行，**勿重复运行**）。

mod actions;
mod chrome;
mod dnd;
mod primitives;
mod rows;
mod search;

use dnd::*;
use primitives::*;
use search::*;

// 跨 crate 公开面：路径保持 `scratchpad_view::{...}` 不变（`lib.rs` 与编辑器都在用）。
// 内容搜索面板已迁 `content_search::view`（第二阶段）：旧名重导保持调用点不换路径；
// 渲染函数签名比原版多一个 `type_icon` 参数（调用方注入文件类型图标）。
// 注意：本模块自有子模块 `search`（草稿箱的搜索面板），以下 `search::` 指子模块；
// 外部内容搜索 crate 只经 `content_search::view` 路径引用。
pub use search::{
    ScratchpadDiffView, ScratchpadFileInfoView,
    render_scratchpad_diff_pane, render_scratchpad_file_info_pane,
};
pub use content_search::view::{
    SearchHitTarget as ScratchpadSearchHitTarget,
    SearchView as ScratchpadSearchView,
    render_search_pane as render_scratchpad_search_pane,
};
// 类型图标：`content_search::view` 的 `type_icon` 由调用方注入，workbench 从这里取。
pub use primitives::scratchpad_type_icon;

/// 冲突：同一份草稿在编辑器里有未保存修改，磁盘上又被外部改了。
struct ScratchpadConflict {
    /// 模块内相对路径（同时是树上的身份）。
    relative: String,
    /// 绝对路径（问编辑器 / 让它重载时用）。
    absolute: std::path::PathBuf,
    /// `None` = 差异还在后台算（先摆提示，不让用户干等）。
    diff: Option<DiffResult>,
}

/// 普通视图工具栏按钮（⋯ 更多设置可单独显示/隐藏与左/右换位；⋯ 本身恒显）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolbarButtonKey {
    Search,
    NewFile,
    NewFolder,
    Reference,
    Import,
    Sort,
    Refresh,
}

/// 工具栏按钮的持久化配置（settings 模型类型别名：落盘 `{ visible, side }`）。
pub type ToolbarButtonCfg = settings::model::ToolbarButtonCfg;

impl ToolbarButtonKey {
    /// 全部按钮（⋯ 菜单顺序 = 原型 647-653）。
    pub const ALL: [ToolbarButtonKey; 7] = [
        Self::Search,
        Self::NewFile,
        Self::NewFolder,
        Self::Reference,
        Self::Import,
        Self::Sort,
        Self::Refresh,
    ];

    /// 落盘稳定标识（settings 键；与 `from_str` 互逆）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::NewFile => "new_file",
            Self::NewFolder => "new_folder",
            Self::Reference => "reference",
            Self::Import => "import",
            Self::Sort => "sort",
            Self::Refresh => "refresh",
        }
    }

    /// 稳定标识 → 按钮；未知标识返回 `None`（旧配置里出现陌生键时忽略）。
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "search" => Self::Search,
            "new_file" => Self::NewFile,
            "new_folder" => Self::NewFolder,
            "reference" => Self::Reference,
            "import" => Self::Import,
            "sort" => Self::Sort,
            "refresh" => Self::Refresh,
            _ => return None,
        })
    }

    /// 菜单文案（原型 647-653）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Search => "搜索入口",
            Self::NewFile => "新建文件",
            Self::NewFolder => "新建目录",
            Self::Reference => "引用入口",
            Self::Import => "导入入口",
            Self::Sort => "排序",
            Self::Refresh => "刷新",
        }
    }

    /// 默认所在侧（原型 shortcut 文本：左 / 右）。
    pub fn side(self) -> &'static str {
        match self {
            Self::Search | Self::NewFile | Self::NewFolder | Self::Reference | Self::Import => "左",
            Self::Sort | Self::Refresh => "右",
        }
    }

    /// 默认侧的落盘值（`left` / `right`）。
    pub fn default_side_text(self) -> &'static str {
        match self.side() {
            "左" => "left",
            _ => "right",
        }
    }
}

/// 草稿箱面板视图状态。
///
/// 数据来自 `rds-scratchpad` 存储；根 = 当前项目会话下的模块目录 `{project}/scratchpad/`。
/// 闭环：新建/重命名/删除→回收站+撤销/回收站恢复与清空/文件名过滤/引用移除/懒加载/排序。
pub struct ScratchpadViewState {
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
    /// 草稿主体分组是否展开（Collapsible 承载，默认展开）。
    drafts_expanded: bool,
    /// 引用分组是否展开（Collapsible 承载，默认展开）。
    refs_expanded: bool,
    /// 底部引用/回收站区域的最大高度（拖拽分区 handle 调整，进程内；默认 7.5rem）。
    group_max_h: f32,
    /// 更多菜单拖拽排序进行中的按钮（on_drag 开始写入，on_drag_move 每帧重排）。
    /// 注：拖拽排序尚未接线（调整面板四栏的自绘 grip 已就位），字段暂只写不读 → 预留。
    #[allow(dead_code)]
    toolbar_drag: Option<ToolbarButtonKey>,
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
    /// 搜索模式与开关（内容模式才有正则 / 大小写 / 全字）。
    search_mode: ScratchpadSearchMode,
    search_regex: bool,
    search_case: bool,
    /// 全字匹配开关（`ab`，2026-10-11 原型差异补齐：侧栏 + 中央都有三个开关 Aa/ab/.*）。
    search_word: bool,
    /// 替换框内 AB 开关（保留大小写，2026-10-11 原型对齐；替换语义后续接入，
    /// 先 UI+状态落地）。
    search_replace_case: bool,
    /// 搜索态整体切换（2026-10-11 用户点名，VSCode 对齐）：`true` = 侧栏只显示搜索面板
    /// （普通工具栏 / 目录树 / 底部引用回收站整体隐藏）；由普通视图工具栏 ⌕ 进入、
    /// 搜索面板 ← 返回退出。
    search_panel_open: bool,
    /// 高级选项（VSCode 搜索面板形态）：包含 / 排除文件 glob；`search_advanced`
    /// 折叠开关（默认**收起**——2026-10-11 用户点名「搜索状态默认隐藏包含/排除字段」，
    /// 由替换行右侧 ⋮ 展开）。`show_include_hint` / `show_exclude_hint`：
    /// glob 样例 hint 是否显示（点 ✎/👁/⚙ 切换；输入框聚焦也会显示）。
    search_advanced: bool,
    show_include_hint: bool,
    show_exclude_hint: bool,
    /// 最近一次内容搜索的完整结果视图（侧栏搜索结果树的直接数据源；中央面板由宿主投递）。
    /// 由 `apply_scratchpad_ops` 在结果回填时落地；`None` = 尚未搜索或已清空。
    search_view: Option<ScratchpadSearchView>,
    /// 侧栏搜索结果树中已展开命中明细的文件（相对路径）。
    search_expanded: HashSet<String>,
    /// 最近一次内容搜索的统计（文件数, 命中数, 扫描文件数）：侧栏搜索面板统计行用。
    /// 由 `apply_scratchpad_ops` 在结果回填时更新；`None` = 尚未搜索。
    search_stats: Option<(usize, usize, usize)>,
    /// 冲突（C-4）：同一份草稿在编辑器里有未保存修改，磁盘上又被外部改了。
    conflicts: Vec<ScratchpadConflict>,
    /// 当前在中央编辑区展示差异的那份草稿（关闭 / 忽略时知道该关谁）。
    shown_diff: Option<String>,
    /// 搜索输入（懒创建；初始态 = 按文件名 / 文件夹名过滤）。
    search_input: Option<Entity<InputState>>,
    _search_sub: Option<Subscription>,
    /// 内容搜索查询输入（懒创建；搜索面板打开后独立于文件过滤框，
    /// placeholder「搜索文件内容」——两处搜索域不同、提示词不同）。
    content_search_input: Option<Entity<InputState>>,
    _content_search_sub: Option<Subscription>,
    /// 替换输入（侧栏搜索框正下方，VSCode 形态；内容搜索时才显示）。
    search_replace_input: Option<Entity<InputState>>,
    /// 包含 / 排除文件 glob 输入（高级选项展开后显示）。
    search_include_input: Option<Entity<InputState>>,
    search_exclude_input: Option<Entity<InputState>>,
    _search_replace_sub: Option<Subscription>,
    /// 添加外部引用弹窗（2026-10-12 原型「弹窗输入路径+别名」对齐）：
    /// 打开标志 + 路径/别名两个输入框（懒创建，视图构建时建）。
    ref_dialog_open: bool,
    ref_path_input: Option<Entity<InputState>>,
    ref_alias_input: Option<Entity<InputState>>,
    /// 草稿树的滚动句柄（虚拟列表内部滚动；键盘导航可滚到选中项）。
    list_scroll: ScratchpadListScroll,
    /// 是否正有 OS 文件拖入面板上方（`on_drag_move::<ExternalPaths>` 置位，drop / exit 复位）。
    external_drag_over: bool,
    /// 拖入的文件数量（提示条展示「松开以导入 N 个文件」）。
    external_drag_count: usize,
    /// 最近一次置位时刻：`on_drag_move` 是 Capture 阶段全局捕获，而
    /// `on_file_drop_exit` 只在元素仍 hover 时派发（gpui-pre `div.rs` 的
    /// `is_hovered_ignoring_last_input` 门控）——鼠标拖出窗口时 hover 先丢，
    /// Exited 不再送达，提示条会残留。此字段配合轮询兜底复位。
    external_drag_since: Option<std::time::Instant>,
    /// 工具栏按钮配置（⋯ 更多设置，2026-10-11 原型 644-658 对齐 + 持久化接线）：
    /// `key → { visible, side }`；缺省 = 显示 + 默认位置（左 5 右 2），⋯ 恒显。
    /// 空表 = 全默认（与 `settings.scratchpad.toolbar` 空表语义一致）。
    toolbar_cfg: BTreeMap<ToolbarButtonKey, ToolbarButtonCfg>,
    /// 标签筛选（统一规格 §7.3）：`Some(tag_id)` = 只显示打了该标签的草稿文件（平铺）；
    /// `None` = 全部。与文件名过滤相互独立（可叠加：两者都过才显示）。
    tag_filter: Option<String>,
    /// 草稿条目 → 活跃标签（键 = 模块内相对路径；加载树后回填，打标/去标后更新）。
    entry_tags: std::collections::HashMap<String, Vec<tag::Tag>>,
    /// 项目标签字典（scope=project，加载时读取；筛选下拉与打标弹窗共用，新建后更新）。
    project_tags: Vec<tag::Tag>,
    /// 标签编辑弹窗：正在打标的条目相对路径（`None` = 关闭）。
    tag_dialog: Option<String>,
    /// 标签编辑弹窗里的「新建标签」名称输入（懒创建）。
    tag_new_input: Option<Entity<InputState>>,
    _tag_new_sub: Option<Subscription>,
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
    /// 有未保存修改的文件（绝对路径）：名称前打脏点。
    dirty: Rc<HashSet<String>>,
    /// 进行中的行内编辑（重命名行改为渲染输入框）。
    edit: Option<ScratchpadEdit>,
    /// 内联新建行的插入位置（显示序号, 缩进层级）——`None` 表示无内联新建。
    edit_insert: Option<(usize, usize)>,
    selected: HashSet<String>,
    expanded: HashSet<String>,
    loaded: HashMap<String, Vec<ScratchpadEntry>>,
    colors: ScratchpadRowColors,
    /// 过滤态（用户点名 2026-10-10：过滤态树行显示命中数角标，对齐 VSCode 搜索面板左侧的
    /// 每文件命中数）。`filter` 空 = 未过滤；`mode/regex/case_sensitive` 与 flatten 同一套口径。
    filter: Rc<String>,
    mode: ScratchpadSearchMode,
    regex: bool,
    case_sensitive: bool,
}

impl Default for ScratchpadViewState {
    fn default() -> Self {
        // 手写全字段：`..Default::default()` 会解析到本 impl（自递归，编译告警 + 运行时栈溢出）。
        Self {
            loaded: false,
            loading: false,
            load_seq: 0,
            error: None,
            entries: Vec::new(),
            children: HashMap::new(),
            sort: ScratchpadSort::default(),
            sort_desc: false,
            external_refs: Vec::new(),
            trash: Vec::new(),
            trash_expanded: false,
            drafts_expanded: true,
            refs_expanded: true,
            group_max_h: 7.5,
            toolbar_drag: None,
            expanded: HashSet::new(),
            selected: HashSet::new(),
            anchor: None,
            clipboard: None,
            edit: None,
            new_target: String::new(),
            new_template: ScratchpadTemplate::default(),
            undo: None,
            name_input: None,
            _name_sub: None,
            search_mode: ScratchpadSearchMode::default(),
            search_regex: false,
            search_case: false,
            search_word: false,
            search_replace_case: false,
            // 搜索态默认关闭（VSCode 对齐：工具栏 ⌕ 进入搜索面板，← 返回普通视图）。
            search_panel_open: false,
            // 用户点名（2026-10-11）：搜索状态默认隐藏包含/排除字段，⋮ 展开。
            search_advanced: false,
            show_include_hint: false,
            show_exclude_hint: false,
            search_view: None,
            search_expanded: HashSet::new(),
            search_stats: None,
            conflicts: Vec::new(),
            shown_diff: None,
            search_input: None,
            _search_sub: None,
            content_search_input: None,
            _content_search_sub: None,
            search_replace_input: None,
            search_include_input: None,
            search_exclude_input: None,
            _search_replace_sub: None,
            ref_dialog_open: false,
            ref_path_input: None,
            ref_alias_input: None,
            list_scroll: ScratchpadListScroll::default(),
            external_drag_over: false,
            external_drag_count: 0,
            external_drag_since: None,
            // 工具栏按钮配置（⋯ 更多设置）：空表 = 全部显示 + 默认位置（缺省语义）。
            toolbar_cfg: BTreeMap::new(),
            tag_filter: None,
            entry_tags: HashMap::new(),
            project_tags: Vec::new(),
            tag_dialog: None,
            tag_new_input: None,
            _tag_new_sub: None,
        }
    }
}

impl ScratchpadViewState {
    /// 在已加载数据（根 + 懒加载子目录）中查找条目类型。
    fn kind_of(&self, key: &str) -> Option<ScratchpadEntryKind> {        fn walk(entries: &[ScratchpadEntry], key: &str) -> Option<ScratchpadEntryKind> {
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

/// 草稿箱面板实体。
///
/// 视图归本 crate；宿主能力经 [`ScratchpadHost`] 注入（与 `mock` / `insight` /
/// `analytics_resource` / `database` 同形）。状态分两处：**实体字段**（宿主句柄、轮询任务、
/// 目录监控）+ [`ScratchpadViewState`]（树 / 选择 / 编辑 / 搜索）。
pub struct ScratchpadView {
    /// 宿主端口（项目根 / 只读 / 提示 / 重绘 / 搜索结果落地 / 在编辑器中打开）。
    host: Rc<dyn ScratchpadHost>,
    focus_handle: FocusHandle,
    /// 面板视图状态。
    scratchpad: Rc<RefCell<ScratchpadViewState>>,
    /// 正在轮询草稿箱加载结果的后台任务（避免重复启动）。
    scratchpad_pump: RefCell<Option<Task<()>>>,
    /// 草稿箱目录监控器（外部改动 → 去抖重拉；每个项目根一个）。
    scratchpad_watch: Option<ScratchpadWatcher>,
    /// 监控轮询任务（常驻，每 ~1.2 s 探查一次变更标记）。
    scratchpad_watch_poll: RefCell<Option<Task<()>>>,
    /// 当前内容搜索的参数（query / 正则 / 大小写）：自己发起、自己留底，
    /// 外部改动后重跑搜索用；展示数据由编辑区持有（经端口投递）。
    active_search: Option<(String, bool, bool)>,
    /// 上次看到的「编辑器脏文档」（绝对路径）：脏点在树上是**外部状态**，
    /// 由轮询一拍一次地取回（不在 `render` 里去问宿主），有变化才重绘。
    dirty_seen: RefCell<Rc<HashSet<String>>>,
}
impl ScratchpadView {
    /// 构造：注入宿主端口。
    pub fn new(host: Rc<dyn ScratchpadHost>, cx: &mut Context<Self>) -> Self {
        // ⋯ 更多设置持久化（2026-10-11 接线）：启动时读入用户配置的工具栏布局；
        // `try_global` 兼容测试环境（未 init Settings global 时用全默认）。
        let toolbar_cfg = cx
            .try_global::<settings::model::Settings>()
            .map(|s| {
                s.scratchpad
                    .toolbar
                    .iter()
                    .filter_map(|(key, cfg)| {
                        ToolbarButtonKey::from_str(key).map(|k| (k, cfg.clone()))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let mut state = ScratchpadViewState::default();
        state.toolbar_cfg = toolbar_cfg;
        Self {
            host,
            focus_handle: cx.focus_handle(),
            scratchpad: Rc::new(RefCell::new(state)),
            scratchpad_pump: RefCell::new(None),
            scratchpad_watch: None,
            scratchpad_watch_poll: RefCell::new(None),
            active_search: None,
            dirty_seen: RefCell::new(Rc::new(HashSet::new())),
        }
    }

    /// 宿主主动要求重载草稿箱（项目切换 / 外部事件）：置 `loaded=false`，
    /// 下一次 render 触发按新项目根重新加载。
    pub fn request_refresh(&self, cx: &mut Context<Self>) {
        self.scratchpad.borrow_mut().loaded = false;
        cx.notify();
    }
}

impl Focusable for ScratchpadView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScratchpadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.render_scratchpad(window, cx))
    }
}

#[cfg(test)]
mod tests;
