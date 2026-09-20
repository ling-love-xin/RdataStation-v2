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
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::base::Disableable as _;
use gpui_kit::base::{StyledExt, VirtualListScrollHandle, v_virtual_list};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable as _};
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
pub use search::{
    ScratchpadDiffView, ScratchpadSearchView, render_scratchpad_diff_pane,
    render_scratchpad_search_pane,
};

/// 冲突：同一份草稿在编辑器里有未保存修改，磁盘上又被外部改了。
struct ScratchpadConflict {
    /// 模块内相对路径（同时是树上的身份）。
    relative: String,
    /// 绝对路径（问编辑器 / 让它重载时用）。
    absolute: std::path::PathBuf,
    /// `None` = 差异还在后台算（先摆提示，不让用户干等）。
    diff: Option<DiffResult>,
}

/// 草稿箱面板视图状态。
///
/// 数据来自 `rds-scratchpad` 存储；根 = 当前项目会话下的模块目录 `{project}/scratchpad/`。
/// 闭环：新建/重命名/删除→回收站+撤销/回收站恢复与清空/文件名过滤/引用移除/懒加载/排序。
#[derive(Default)]
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
    /// 冲突（C-4）：同一份草稿在编辑器里有未保存修改，磁盘上又被外部改了。
    conflicts: Vec<ScratchpadConflict>,
    /// 当前在中央编辑区展示差异的那份草稿（关闭 / 忽略时知道该关谁）。
    shown_diff: Option<String>,
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
}

impl ScratchpadViewState {
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
        Self {
            host,
            focus_handle: cx.focus_handle(),
            scratchpad: Rc::new(RefCell::new(ScratchpadViewState::default())),
            scratchpad_pump: RefCell::new(None),
            scratchpad_watch: None,
            scratchpad_watch_poll: RefCell::new(None),
            active_search: None,
            dirty_seen: RefCell::new(Rc::new(HashSet::new())),
        }
    }
}

impl Focusable for ScratchpadView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScratchpadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_scratchpad(window, cx)
    }
}

#[cfg(test)]
mod tests;
