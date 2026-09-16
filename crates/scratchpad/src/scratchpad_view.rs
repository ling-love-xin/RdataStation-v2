//! 草稿箱面板（M5）：文件树 / 内联编辑 / 剪贴板 / 回收站 / 内容搜索 / 替换。
//!
//! 本 crate（`scratchpad`）自带视图；宿主能力经 [`ScratchpadHost`] 注入（`ScratchpadView::new`），
//! 与 `mock` / `insight` / `analytics_resource` / `database` 同形。内容分三段：
//! 1. 状态类型与纯函数辅助（`ScratchpadViewState` / 模板 / 排序 / 压平 / 搜索结果视图）；
//! 2. 草稿箱方法（加载 / 编辑 / 删除 / 剪贴板 / 引用 / 监控轮询）；
//! 3. 草稿箱渲染实现（行渲染 / 内联编辑行 / 空态 / 主视图 / 搜索面板）。
//!
//! 依赖分工（下沉后不再有 `Shared`）：
//! - 项目根 / 只读判定 / 提示 / 宿主重绘 / 搜索结果落地 / 在编辑器中打开 → [`ScratchpadHost`]；
//! - 草稿库、回收站、文件监控 → 本 crate（`store` / `trash` / `watch`）；
//! - 重操作（读盘 / 导入 / 搜索 / 替换）→ [`crate::jobs`] 的工作线程，本模块只入队与回填。
//!
//! 结构设计：`docs/architecture/layout/panels-coupling-plan.md` §9。

use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
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
    ExternalReferenceStatus, ScratchpadEntry, ScratchpadEntryKind, ScratchpadStore, TrashEntry,
};

use crate::jobs as scratchpad_jobs;

use crate::ScratchpadWatcher;

use workbench_shell::ui;

use crate::host::ScratchpadHost;

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
    pub query: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
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

/// 该条目要不要画脏点（编辑器里有未保存修改）：**只有文件**画，文件夹不画。
///
/// 判据是**绝对路径**——条目路径即绝对路径，编辑器与草稿箱之间只交换绝对路径。
fn scratchpad_shows_dirty_dot(dirty: &HashSet<String>, entry: &ScratchpadEntry) -> bool {
    entry.kind == ScratchpadEntryKind::File
        && dirty.contains(entry.path.to_string_lossy().as_ref())
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

/// 把后台搜索任务的载荷转成结果视图。
///
/// 搜索/替换任务的开关与查询词由任务回传，避免再从侧栏输入框反向读状态。
fn search_view_from_payload(
    query: String,
    is_regex: bool,
    case_sensitive: bool,
    payload: scratchpad_jobs::SearchPayload,
) -> ScratchpadSearchView {
    ScratchpadSearchView {
        query,
        is_regex,
        case_sensitive,
        scanned: payload.scanned,
        truncated: payload.truncated,
        hits: payload
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
    }
}

/// 内容搜索结果面板（中央编辑区）：头部（查询/命中数/开关标记）+ 命中列表（含上下文）。
pub fn render_scratchpad_search_pane(
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

impl ScratchpadView {
    /// 请求重载草稿箱（渲染与事件路径共用）：**只入队 + 起轮询，不做 I/O**。
    ///
    /// 实际读盘在 `scratchpad_jobs` 的工作线程；结果由 [`Self::apply_scratchpad_loads`] 回填。
    /// 重载时保留已展开子目录（在后台重新拉取），避免「操作后展开态看起来空了」。
    fn request_scratchpad_load(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            // 项目关闭：停掉监控（不再关心旧项目的目录事件）。
            self.scratchpad_watch = None;
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
        self.ensure_scratchpad_watch(&root, cx);
        let parents: Vec<String> = self.scratchpad.borrow().children.keys().cloned().collect();
        let seq = scratchpad_jobs::enqueue_root_load(&root, parents);
        // 本次重拉已覆盖“此刻之前的全部改动”（含我们自己刚写的文件），
        // 清掉标记避免紧接着再来一次多余的重拉。
        if let Some(watcher) = &self.scratchpad_watch {
            let _ = watcher.take_changed();
        }
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

    /// 确保草稿箱目录监控在跑（项目根变化时换监控点）。
    ///
    /// 注册监控是一次轻量 OS 调用（不读盘），且只在「项目根首次出现或变化」时发生，
    /// 因此放在请求加载路径（不在渲染循环里反复执行）。
    fn ensure_scratchpad_watch(&mut self, root: &std::path::Path, cx: &mut Context<Self>) {
        let dir = root.join(crate::MODULE_DIR_NAME);
        if self.scratchpad_watch
            .as_ref()
            .map(|w| w.dir() == dir.as_path())
            .unwrap_or(false)
        {
            return;
        }
        match crate::ScratchpadWatcher::start(dir) {
            Ok(watcher) => self.scratchpad_watch = Some(watcher),
            Err(e) => {
                // 监控失败不影响使用（只是不能自动刷新）：降级为手动 `↻`。
                tracing::warn!("[Scratchpad] 目录监控启动失败，将退化为手动刷新: {e}");
                self.scratchpad_watch = None;
                return;
            }
        }
        self.ensure_scratchpad_watch_poll(cx);
    }

    /// 刷新「编辑器脏文档」缓存；有变化返回 `true`（调用方据此重绘）。
    ///
    /// 脏点的判据在编辑器手里（哪些文档有未保存修改），草稿箱只经宿主端口要一份
    /// 绝对路径集合——不依赖 `editor` crate。与目录监控同拍调用，不在 `render` 里碰宿主。
    fn refresh_dirty_cache(&self) -> bool {
        let now: HashSet<String> = self
            .host
            .dirty_files()
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let mut cache = self.dirty_seen.borrow_mut();
        if **cache == now {
            return false;
        }
        *cache = Rc::new(now);
        true
    }

    /// 启动监控轮询（常驻任务：每 ~1.2 s 看一次变更标记，有变化就重拉一次）。
    ///
    /// 轮询而非“事件驱动立即重拉”，是为了**去抖**：编辑器保存一次常触发多条 OS 事件，
    /// 立即刷新会把 UI 打成刷新循环。
    ///
    /// 同一拍里若结果面板还开着，还会用同一套查询/开关重跑一次搜索（K4）。
    fn ensure_scratchpad_watch_poll(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.scratchpad_watch_poll.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| loop {
            executor.timer(std::time::Duration::from_millis(1200)).await;
            let action = weak.update(cx, |this, cx| {
                // 脏点与目录监控同一拍：编辑器里存/改都会让这个集合变。
                let dirty_changed = this.refresh_dirty_cache();
                let changed = this
                    .scratchpad_watch
                    .as_ref()
                    .map(|w| w.take_changed())
                    .unwrap_or(false);
                if !changed {
                    return dirty_changed;
                }
                // 正在内联编辑（新建/重命名）或已有加载在途：本次不打断，留给下一拍。
                {
                    let view = this.scratchpad.borrow();
                    if view.edit.is_some() || view.loading {
                        return dirty_changed;
                    }
                }
                this.scratchpad.borrow_mut().loaded = false;
                // 结果面板若还开着，顺带重跑一次搜索：外部改动后旧的命中列表已是快照。
                let pending_search = this.active_search.clone();
                if let Some((query, is_regex, case_sensitive)) = pending_search {
                    if let Some(root) = this.host.project_root() {
                        scratchpad_jobs::enqueue_search(&root, &query, case_sensitive, is_regex);
                        this.ensure_scratchpad_pump(cx);
                    }
                }
                true
            });
            match action {
                Ok(true) => {
                    if weak.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
                Ok(false) => {}
                Err(_) => return,
            }
        });
        *self.scratchpad_watch_poll.borrow_mut() = Some(task);
    }

    /// 启动草稿箱加载结果轮询（已有存活任务时不重复启动）。
    pub fn ensure_scratchpad_pump(&self, cx: &mut Context<Self>) {
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
                let ops = scratchpad_jobs::drain_ops();
                if !ops.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_scratchpad_ops(ops, cx))
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

    /// 回填写操作 / 搜索替换结果（主线程）。
    ///
    /// 文案、刷新与通知都在这里统一处理：任务层只回传「做了什么 + 成功/失败」。
    fn apply_scratchpad_ops(&mut self, results: Vec<scratchpad_jobs::OpResult>, cx: &mut Context<Self>) {
        let mut reload = false;
        let mut notice: Option<String> = None;
        let mut error: Option<String> = None;
        let mut search_view: Option<Option<ScratchpadSearchView>> = None;

        for result in results {
            match result {
                scratchpad_jobs::OpResult::Import { outcome } => match outcome {
                    Ok(()) => {
                        reload = true;
                        notice = Some("已导入所选文件".to_string());
                    }
                    Err(e) => {
                        // 部分成功是可能的（逐个导入遇错即停），仍要刷新一次。
                        reload = true;
                        error = Some(format!("导入失败: {e}"));
                    }
                },
                scratchpad_jobs::OpResult::Paste { cut, outcome } => match outcome {
                    Ok(()) => {
                        reload = true;
                        if cut {
                            // 剪切粘贴成功才收起剪贴板（与同步版语义一致）。
                            self.scratchpad.borrow_mut().clipboard = None;
                        }
                        notice = Some("已粘贴".to_string());
                    }
                    Err(e) => {
                        reload = true;
                        error = Some(format!("粘贴失败: {e}"));
                    }
                },
                scratchpad_jobs::OpResult::EmptyTrash { outcome } => match outcome {
                    Ok(()) => {
                        reload = true;
                        self.scratchpad.borrow_mut().trash_expanded = false;
                        notice = Some("回收站已清空".to_string());
                    }
                    Err(e) => error = Some(format!("清空回收站失败: {e}")),
                },
                scratchpad_jobs::OpResult::Search {
                    query,
                    is_regex,
                    case_sensitive,
                    replaced,
                    outcome,
                } => match outcome {
                    Ok(payload) => {
                        if let Some((total, files)) = replaced {
                            notice = Some(format!("已替换 {total} 处（{files} 个文件）"));
                        }
                        search_view = Some(Some(search_view_from_payload(
                            query,
                            is_regex,
                            case_sensitive,
                            payload,
                        )));
                        error = None;
                    }
                    Err(e) => match replaced {
                        // 替换任务失败：提示走通知栏（结果栏保持旧内容）。
                        Some(_) => notice = Some(format!("替换失败: {e}")),
                        None => {
                            search_view = Some(None);
                            error = Some(format!("搜索失败: {e}"));
                        }
                    },
                },
            }
        }

        {
            let mut view = self.scratchpad.borrow_mut();
            if reload {
                view.loaded = false;
                if error.is_none() {
                    view.error = None;
                }
            }
            if let Some(e) = error {
                view.error = Some(e);
            }
        }
        if let Some(view) = search_view {
            self.active_search = view
                .as_ref()
                .map(|v| (v.query.clone(), v.is_regex, v.case_sensitive));
            self.host.show_search_results(view, cx);
            self.host.notify_host(cx);
        }
        if let Some(text) = notice {
            self.host.notice(text, cx);
        }
        cx.notify();
    }

    /// 构建草稿箱存储 + 运行时（未打开项目时报错）。
    fn scratchpad_store(&self) -> Result<(ScratchpadStore, tokio::runtime::Runtime), String> {
        let root = self
            .host
            .project_root()
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
        if self.host.read_only() {
            self.host.notice("只读模式：不允许修改草稿".to_string(), cx);
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
        if self.host.read_only() {
            self.host.notice("只读模式：不允许修改草稿".to_string(), cx);
            cx.notify();
            return;
        }
        let name = self.scratchpad
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
        if self.host.read_only() {
            self.host.notice("只读模式：不允许删除草稿".to_string(), cx);
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
    ///
    /// 复制可能搬运大量字节（递归复制），故入队到后台；结果由 `apply_scratchpad_ops` 回填。
    fn paste_scratchpad_clipboard(&mut self, cx: &mut Context<Self>) {
        // M1：只读打开时禁止写入草稿。
        if self.host.read_only() {
            self.host.notice("只读模式：不允许粘贴草稿".to_string(), cx);
            cx.notify();
            return;
        }
        let Some(clipboard) = self.scratchpad.borrow().clipboard.clone() else {
            return;
        };
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        let target = self.scratchpad_paste_target();
        let cut = clipboard.mode == ScratchpadClipboardMode::Cut;
        scratchpad_jobs::enqueue_paste(&root, cut, clipboard.paths.clone(), &target);
        {
            let mut view = self.scratchpad.borrow_mut();
            view.selected.clear();
            view.anchor = None;
        }
        self.ensure_scratchpad_pump(cx);
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

    /// 清空回收站（删的是可能很大的 payload，故入队后台）。
    fn empty_scratchpad_trash(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_empty_trash(&root);
        self.ensure_scratchpad_pump(cx);
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
        if self.host.read_only() {
            self.host.notice("只读模式：不允许修改引用".to_string(), cx);
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

    /// 请求在中央编辑器中打开草稿文件（双击 / Enter / 右键「打开」共用）。
    ///
    /// 只把**绝对路径**交给宿主（`ScratchpadHost::open_in_editor`）；编辑器无根，
    /// 自己按路径判定模式与只读等级（Phase C 契约）。
    fn request_open_scratchpad_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.host.open_in_editor(std::path::PathBuf::from(path));
        self.host.notify_host(cx);
        cx.notify();
    }

    /// 懒加载子目录（展开文件夹时调用）：只入队 + 起轮询，结果由 `apply_scratchpad_dirs` 回填。
    fn request_scratchpad_dir(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error =
                Some("未打开项目：草稿箱根即项目目录，请先打开项目。".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_dir_load(&root, &path);
        self.ensure_scratchpad_pump(cx);
    }

    /// 导入外部文件到草稿箱（复制进来；可能拷 GB 级文件，故入队后台）。
    fn import_scratchpad_files(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_import(&root, paths);
        self.ensure_scratchpad_pump(cx);
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

    /// Enter / →：文件夹展开折叠（展开时顺带懒加载）；文件在中央编辑器中打开。
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
        let is_folder = self.scratchpad
            .borrow()
            .kind_of(&path)
            .map(|k| k == ScratchpadEntryKind::Folder)
            .unwrap_or(false);
        if !is_folder {
            // 文件：交给中央编辑器（同路径已打开只激活，不重读）。
            self.request_open_scratchpad_file(path.clone(), cx);
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

    /// 运行内容搜索（经端口投递到中央编辑区展示；参数留存供外部改动后重跑）。
    ///
    /// 搜索要遍历全树，故入队后台；结果由 `apply_scratchpad_ops` 回填。
    fn run_scratchpad_content_search(&mut self, cx: &mut Context<Self>) {
        let query = self.scratchpad
            .borrow()
            .search_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            self.active_search = None;
            self.host.show_search_results(None, cx);
            self.host.notify_host(cx);
            cx.notify();
            return;
        }
        let (is_regex, case_sensitive) = {
            let v = self.scratchpad.borrow();
            (v.search_regex, v.search_case)
        };
        let Some(root) = self.host.project_root() else {
            self.scratchpad.borrow_mut().error = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_search(&root, &query, case_sensitive, is_regex);
        self.ensure_scratchpad_pump(cx);
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
                let matches = this.scratchpad
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

}

impl ScratchpadView {
    /// 渲染草稿树单行（选中态 / 行内操作 / 右键菜单 / 展开时触发懒加载）。
    ///
    /// 同时被普通渲染与虚拟列表闭包调用，行索引按 `ctx.rows` 全局序号。
    fn scratchpad_row(&self, display: usize, ctx: &ScratchpadRowCtx, cx: &mut Context<Self>) -> AnyElement {
        // 内联新建行：插在目标文件夹首行位置（未选中文件夹时在模块根）。
        if ctx.is_edit_row(display) {
            return self.render_scratchpad_edit_row(ctx.edit_row_depth(display), cx)
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
            // 双击文件 = 在编辑器中打开（与 Enter 同一语义）。
            let open_host = self.host.clone();
            move |ev: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                let modifiers = ev.modifiers();
                if ev.click_count() >= 2 && !is_folder {
                    open_host.open_in_editor(std::path::PathBuf::from(&key));
                }
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
            // 编辑器里有未保存修改 → 名称前一个脏点（VS Code 口径：只有文件）。
            .when(scratchpad_shows_dirty_dot(&ctx.dirty, entry), |this| {
                this.child(
                    div()
                        .w_2()
                        .h_2()
                        .flex_none()
                        .rounded_full()
                        .bg(primary),
                )
            })
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
            // 文件夹没有「打开」（双击/Enter 的语义是展开）。
            let menu_open = entity.clone();
            let menu_open_key = key.clone();
            row.context_menu(move |menu, _window, _cx| {
                let open_doc_entity = menu_open.clone();
                let open_doc_key = menu_open_key.clone();
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
                let mut menu = menu;
                if !is_folder {
                    menu = menu.item(PopupMenuItem::new("打开").on_click(move |_, _, app| {
                        open_doc_entity.update(app, |this, cx| {
                            this.request_open_scratchpad_file(open_doc_key.clone(), cx)
                        });
                    }));
                }
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
    pub fn render_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
            dirty: self.dirty_seen.borrow().clone(),
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

}

#[cfg(test)]
mod tests {
    /// M5 草稿箱：面板内的纯函数语义（排序 / 压平 / 模板后缀 / 请求消费 / 搜索结果映射）。
    ///
    /// 窗口级交互（真点击、真轮询）不在这一层，见 `scratchpad-user-guide.md` §9 验收清单。
    use super::{
        ScratchpadSearchView, ScratchpadSort, ScratchpadTemplate, ScratchpadEntryKind,
        flatten_scratchpad, join_scratchpad_rel, scratchpad_apply_template_ext,
        scratchpad_entry_matches, scratchpad_shows_dirty_dot, scratchpad_size_label,
        scratchpad_split_name, scratchpad_sort_entries,
        search_view_from_payload, ScratchpadEntry,
    };
    use crate::jobs::SearchPayload;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn file(name: &str, size: u64, modified: &str) -> ScratchpadEntry {
        ScratchpadEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/p/{name}")),
            kind: ScratchpadEntryKind::File,
            size,
            modified_at: Some(modified.to_string()),
            children: None,
        }
    }

    fn folder(name: &str, modified: &str, kids: Option<Vec<ScratchpadEntry>>) -> ScratchpadEntry {
        ScratchpadEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("/p/{name}")),
            kind: ScratchpadEntryKind::Folder,
            size: 0,
            modified_at: Some(modified.to_string()),
            children: kids,
        }
    }

    #[test]
    fn dirty_dot_marks_dirty_files_only() {
        let mut dirty = HashSet::new();
        dirty.insert("/p/a.sql".to_string());

        assert!(
            scratchpad_shows_dirty_dot(&dirty, &file("a.sql", 1, "0")),
            "编辑器里改过的文件要打点"
        );
        assert!(
            !scratchpad_shows_dirty_dot(&dirty, &file("b.sql", 1, "0")),
            "没改过的文件不打点"
        );
        assert!(
            !scratchpad_shows_dirty_dot(&dirty, &folder("a.sql", "0", None)),
            "文件夹不画脏点（即使路径命中）"
        );
        assert!(
            !scratchpad_shows_dirty_dot(&HashSet::new(), &file("a.sql", 1, "0")),
            "没有脏文档时谁都不打点"
        );
    }

    #[test]
    fn template_suffix_rules() {
        // 无后缀：补模板后缀。
        assert_eq!(
            scratchpad_apply_template_ext("note", ScratchpadTemplate::Sql),
            "note.sql"
        );
        // 上一次由模板补的后缀：随模板切换替换。
        assert_eq!(
            scratchpad_apply_template_ext("note.md", ScratchpadTemplate::Sql),
            "note.sql"
        );
        // 用户自写的后缀：尊重不动。
        assert_eq!(
            scratchpad_apply_template_ext("note.txt", ScratchpadTemplate::Sql),
            "note.txt"
        );
        // 空白模板：不补也不剥。
        assert_eq!(
            scratchpad_apply_template_ext("note", ScratchpadTemplate::Blank),
            "note"
        );
        assert_eq!(
            scratchpad_apply_template_ext("note.sql", ScratchpadTemplate::Blank),
            "note"
        );
    }

    #[test]
    fn name_split_and_relative_join() {
        assert_eq!(
            scratchpad_split_name("a.sql"),
            ("a".to_string(), ".sql".to_string())
        );
        // 前导点的隐藏名不当作后缀（面板不展示隐藏项，但函数本身应一致）。
        assert_eq!(
            scratchpad_split_name(".env"),
            (".env".to_string(), String::new())
        );
        assert_eq!(join_scratchpad_rel("", "a.sql"), "a.sql");
        assert_eq!(join_scratchpad_rel("dir/", "a.sql"), "dir/a.sql");
    }

    #[test]
    fn sort_keeps_folders_first_and_follows_direction() {
        let mut rows = vec![
            file("b.sql", 200, "2026-01-02T00:00:00Z"),
            folder("dir", "2026-01-03T00:00:00Z", None),
            file("a.sql", 100, "2026-01-04T00:00:00Z"),
        ];
        scratchpad_sort_entries(&mut rows, ScratchpadSort::Name, false);
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["dir", "a.sql", "b.sql"], "文件夹恒在前，其余按名称升序");

        scratchpad_sort_entries(&mut rows, ScratchpadSort::Size, true);
        let names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["dir", "b.sql", "a.sql"], "大小降序（文件夹仍在前）");
    }

    #[test]
    fn flatten_respects_expand_and_filter() {
        let tree = vec![folder(
            "dir",
            "t",
            Some(vec![file("a.sql", 1, "t"), file("b.csv", 2, "t")]),
        )];
        let loaded = HashMap::new();

        // 未展开：只出行本身。
        let mut out = Vec::new();
        flatten_scratchpad(
            &tree,
            0,
            &HashSet::new(),
            &loaded,
            ScratchpadSort::Name,
            false,
            "",
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 0, "根层级缩进为 0");

        // 展开：子项深度 +1。
        let mut expanded = HashSet::new();
        expanded.insert("/p/dir".to_string());
        let mut out = Vec::new();
        flatten_scratchpad(
            &tree,
            0,
            &expanded,
            &loaded,
            ScratchpadSort::Name,
            false,
            "",
            &mut out,
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].0, 1);

        // 过滤命中子项：父目录保留（且强制展开），命中项在内。
        let mut out = Vec::new();
        flatten_scratchpad(
            &tree,
            0,
            &HashSet::new(),
            &loaded,
            ScratchpadSort::Name,
            false,
            "b.csv",
            &mut out,
        );
        let names: Vec<&str> = out.iter().map(|(_, e)| e.name.as_str()).collect();
        assert_eq!(names, vec!["dir", "b.csv"]);
    }

    #[test]
    fn entry_matches_self_or_loaded_subtree() {
        let mut loaded = HashMap::new();
        loaded.insert("/p/dir".to_string(), vec![file("deep.csv", 1, "t")]);
        let dir = folder("dir", "t", None);

        assert!(scratchpad_entry_matches(&dir, "", &loaded), "空过滤全通过");
        assert!(scratchpad_entry_matches(&dir, "di", &loaded), "自身命中");
        assert!(
            scratchpad_entry_matches(&dir, "deep", &loaded),
            "懒加载子树命中"
        );
        assert!(!scratchpad_entry_matches(&dir, "nope", &loaded));
    }

    #[test]
    fn size_label_uses_readable_units() {
        assert_eq!(scratchpad_size_label(512), "512 B");
        assert_eq!(scratchpad_size_label(2048), "2.0 KB");
        assert_eq!(scratchpad_size_label(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn search_payload_maps_into_view() {
        let payload = SearchPayload {
            scanned: 4,
            truncated: true,
            matches: vec![crate::SearchMatch {
                file: "a.sql".to_string(),
                line_number: 7,
                line_content: "select id".to_string(),
                before_context: vec!["before".to_string()],
                after_context: vec!["after".to_string()],
                match_spans: vec![(7, 9)],
            }],
            replaced: None,
        };
        let view: ScratchpadSearchView =
            search_view_from_payload("id".to_string(), true, false, payload);
        assert_eq!(view.query, "id");
        assert!(view.is_regex && !view.case_sensitive);
        assert_eq!(view.scanned, 4);
        assert!(view.truncated);
        assert_eq!(view.hits.len(), 1);
        // 命中区间与上下文逐字段带过来（高亮靠它们渲染）。
        assert_eq!(view.hits[0].spans, vec![(7, 9)]);
        assert_eq!(view.hits[0].line, 7);
        assert_eq!(view.hits[0].before, vec!["before".to_string()]);
        assert_eq!(view.hits[0].after, vec!["after".to_string()]);
    }
}
