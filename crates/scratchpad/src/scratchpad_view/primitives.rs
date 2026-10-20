//! 纯函数与值模型（无 `self`、无 I/O、可单测）。
//!
//! 为什么要独立成模块：模板 / 排序 / 压平 / 名称与元信息文案 / 脏点判据 / 类型色点与面板状态
//! 无关，此前混在 4100 行的单一文件里，改一处要在一屏里找半天；搬出后根只留
//! 「状态 + 行输入模型 + 视图协议」。
//!
//! 可见性：搬出的项加 `pub(super)`（= 在 `crate::scratchpad_view` 及其子孙模块可见），
//! 根用 `use self::primitives::*;` 收回来，**调用点一字未改**。

use super::*;

use std::path::Path;

/// 内联编辑（新建 / 重命名 / 新建引用 / 引用改名）。
#[derive(Clone)]
pub(super) enum ScratchpadEdit {
    /// 新建文件（使用 `ScratchpadView::new_template` 选中的模板）。
    NewFile,
    NewFolder,
    /// 已选定外部路径，待输入别名（引用不复制文件，只记路径）。
    /// 注：`start_scratchpad_edit` / `commit_scratchpad_edit` 已处理该变体，但「新建引用」
    /// 的 UI 入口（引用弹窗确认后的 edit 装配）尚未接线，暂无人构造 → 按预留保留。
    #[allow(dead_code)]
    NewReference {
        path: std::path::PathBuf,
    },
    Rename {
        path: String,
    },
    RenameReference {
        alias: String,
    },
}

/// 新建文件的起步模板。
///
/// 用户点名：去掉 SQL / Markdown / Python / JSON 模板——草稿箱是**临时探索工作区**，
/// 起步模板反而诱导“正经格式”的预期；统一空白起步，让编辑器自然接管。
/// 未来的“分析文件”类型（自设计）实现后再在此枚举加新变体 + 简单引导。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ScratchpadTemplate {
    #[default]
    Blank,
}

impl ScratchpadTemplate {
    /// 模板显示名（「去模板」后仅剩 Blank；未来「分析文件」类型加回时再接线）。
    #[allow(dead_code)]
    pub(super) fn label(self) -> &'static str {
        match self {
            ScratchpadTemplate::Blank => "空白",
        }
    }

    /// 默认后缀（空白模板不加后缀）。
    pub(super) fn extension(self) -> Option<&'static str> {
        match self {
            ScratchpadTemplate::Blank => None,
        }
    }

    /// 占位内容（创建后写入，可直接编辑）。
    pub(super) fn content(self, _name: &str) -> String {
        match self {
            ScratchpadTemplate::Blank => String::new(),
        }
    }
}

/// 按模板补后缀：用户自写的后缀保留；模板补的后缀则随模板切换替换。
pub(super) fn scratchpad_apply_template_ext(name: &str, template: ScratchpadTemplate) -> String {
    // 已知模板后缀视为“模板补的”，可替换（模板去重后为空数组；未来「分析文件」
    // 类型加回时在此登记其专属后缀）。
    pub(super) const TEMPLATE_EXTS: [&str; 0] = [];
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
pub(super) struct ScratchpadUndo {
    pub(super) label: String,
    pub(super) trash_ids: Vec<String>,
}

/// 剪贴板模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScratchpadClipboardMode {
    Cut,
    Copy,
}

/// 文件剪贴板（路径相对模块根）。
/// 注：剪切/复制已接线（`set_scratchpad_clipboard`），粘贴入口（`paste_scratchpad_clipboard`）
/// 尚未接 UI → `mode` / `paths` 暂只写不读，按预留保留。
#[derive(Clone)]
#[allow(dead_code)]
pub(super) struct ScratchpadClipboard {
    pub(super) mode: ScratchpadClipboardMode,
    pub(super) paths: Vec<String>,
}

/// 搜索模式（文件名过滤 / 内容搜索）。
///
/// 默认**内容搜索**：与主流编辑器的全局搜索直觉一致（输入即搜内容、命中可跳行），
/// 文件名过滤作为轻量模式保留，用户可经 chip 切回。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ScratchpadSearchMode {
    Name,
    #[default]
    Content,
}

/// 文件树排序键。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ScratchpadSort {
    #[default]
    Name,
    Size,
    Modified,
}

/// 排序标签（底部状态展示）。
pub(super) fn scratchpad_sort_label(sort: ScratchpadSort, desc: bool) -> &'static str {
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
pub(super) fn scratchpad_cycle_sort(sort: &mut ScratchpadSort, desc: &mut bool) {
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
pub(super) fn scratchpad_sort_entries(
    entries: &mut [ScratchpadEntry],
    sort: ScratchpadSort,
    desc: bool,
) {
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

/// 文本是否命中过滤（正则 / 大小写由调用方传入，Name 模式沿用旧口径：小写不敏感）。
fn scratchpad_text_matches(hay: &str, needle: &str, regex: bool, case_sensitive: bool) -> bool {
    if regex {
        let pattern = if case_sensitive {
            needle.to_string()
        } else {
            format!("(?i){}", needle)
        };
        regex::Regex::new(&pattern).map_or(false, |re| re.is_match(hay))
    } else if case_sensitive {
        hay.contains(needle)
    } else {
        hay.to_lowercase().contains(&needle.to_lowercase())
    }
}

/// Content 模式：读文件内容（限制读取头部 256KB，防大文件拖慢树过滤）匹配。
///
/// 草稿箱是「项目私有小文件」场景（日志 / SQL / 脚本），头部截断足够覆盖命中判断；
/// 目录极大时可在 `ScratchpadView` 层加内容命中缓存（按 query+mode+case+regex 键控）——
/// 当前规模（≤几十个文件）直接同步读，避免异步状态机把树过滤复杂化。
fn scratchpad_content_matches(
    path: &std::path::Path,
    needle: &str,
    regex: bool,
    case_sensitive: bool,
) -> bool {
    const MAX_READ: u64 = 256 * 1024;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let read_len = meta.len().min(MAX_READ) as usize;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read as _;
    let mut buf = vec![0u8; read_len];
    let Ok(n) = f.read(&mut buf) else {
        return false;
    };
    scratchpad_text_matches(
        &String::from_utf8_lossy(&buf[..n]),
        needle,
        regex,
        case_sensitive,
    )
}

/// Content 模式：统计文件内容中**命中的行数**（过滤态树行角标，对齐搜索面板逐行口径）。
///
/// 与 `scratchpad_content_matches` 同一读取边界（头部 256KB）；行命中判定与
/// 搜索任务（`store::search_open`）一致——正则 / 大小写由调用方传入，逐行 `is_match`。
/// 文件名命中但内容零命中的文件**不显示角标**（避免误导性的「0」），由调用方决定何时渲染。
pub(super) fn scratchpad_content_hit_count(
    path: &std::path::Path,
    needle: &str,
    regex: bool,
    case_sensitive: bool,
) -> usize {
    const MAX_READ: u64 = 256 * 1024;
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    let read_len = meta.len().min(MAX_READ) as usize;
    let Ok(mut f) = std::fs::File::open(path) else {
        return 0;
    };
    use std::io::Read as _;
    let mut buf = vec![0u8; read_len];
    let Ok(n) = f.read(&mut buf) else {
        return 0;
    };
    String::from_utf8_lossy(&buf[..n])
        .lines()
        .filter(|line| scratchpad_text_matches(line, needle, regex, case_sensitive))
        .count()
}

/// 条目是否命中过滤（自身命中，或已加载子树命中）。
///
/// `mode == Content` 时：文件名命中 **或** 文件内容命中（用户点名：输入即搜内容）。
/// 文件夹的内容命中走子树递归（文件夹本身不读内容）。
#[allow(clippy::too_many_arguments)]
pub(super) fn scratchpad_entry_matches(
    entry: &ScratchpadEntry,
    filter: &str,
    mode: ScratchpadSearchMode,
    regex: bool,
    case_sensitive: bool,
    loaded: &HashMap<String, Vec<ScratchpadEntry>>,
) -> bool {
    if filter.is_empty() {
        return true;
    }
    if scratchpad_text_matches(&entry.name, filter, regex, case_sensitive) {
        return true;
    }
    if mode == ScratchpadSearchMode::Content
        && entry.kind == ScratchpadEntryKind::File
        && scratchpad_content_matches(&entry.path, filter, regex, case_sensitive)
    {
        return true;
    }
    let key = entry.path.to_string_lossy().to_string();
    let kids = loaded.get(&key).or(entry.children.as_ref());
    kids.map(|kids| {
        kids.iter().any(|c| {
            scratchpad_entry_matches(c, filter, mode, regex, case_sensitive, loaded)
        })
    })
    .unwrap_or(false)
}

/// 将条目树按展开状态压平成 `(缩进层级, 条目)` 行序列。
///
/// 子目录优先取懒加载缓存 `loaded`，无缓存时用 `entry.children`；每层按排序设置排列。
/// 有过滤时自动展开命中子树。
/// 标签筛选（`tag_filter` = 标签 id）独立于文件名过滤：生效时**平铺**只收打了该标签的
/// 文件行（depth=0，目录跳过）——「筛选某标签」的语义是看这批文件，不是保留树形。
#[allow(clippy::too_many_arguments)]
pub(super) fn flatten_scratchpad(
    entries: &[ScratchpadEntry],
    depth: usize,
    expanded: &HashSet<String>,
    loaded: &HashMap<String, Vec<ScratchpadEntry>>,
    sort: ScratchpadSort,
    sort_desc: bool,
    filter: &str,
    mode: ScratchpadSearchMode,
    regex: bool,
    case_sensitive: bool,
    module_root: Option<&Path>,
    tag_filter: Option<&str>,
    entry_tags: &HashMap<String, Vec<tag::Tag>>,
    out: &mut Vec<(usize, ScratchpadEntry)>,
) {
    let mut sorted: Vec<ScratchpadEntry> = entries.to_vec();
    scratchpad_sort_entries(&mut sorted, sort, sort_desc);
    for entry in &sorted {
        // 标签筛选分支：文件且命中 → 平铺；其余（目录 / 未命中）跳过，不递归。
        if let Some(tag_id) = tag_filter {
            if entry.kind == ScratchpadEntryKind::File {
                let rel = module_root
                    .and_then(|root| entry.path.strip_prefix(root).ok())
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                let hit = entry_tags
                    .get(&rel)
                    .map(|tags| tags.iter().any(|t| t.id == tag_id))
                    .unwrap_or(false);
                if hit {
                    out.push((0, entry.clone()));
                }
            }
            continue;
        }
        if !scratchpad_entry_matches(entry, filter, mode, regex, case_sensitive, loaded) {
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
                    mode,
                    regex,
                    case_sensitive,
                    module_root,
                    tag_filter,
                    entry_tags,
                    out,
                );
            }
        }
    }
}

/// 取路径末段文件名。
pub(super) fn scratchpad_basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// 该条目要不要画脏点（编辑器里有未保存修改）：**只有文件**画，文件夹不画。
///
/// 判据是**绝对路径**——条目路径即绝对路径，编辑器与草稿箱之间只交换绝对路径。
pub(super) fn scratchpad_shows_dirty_dot(dirty: &HashSet<String>, entry: &ScratchpadEntry) -> bool {
    entry.kind == ScratchpadEntryKind::File && dirty.contains(entry.path.to_string_lossy().as_ref())
}

/// 文件类型图标（Lucide 全量嵌入，`gpui_kit::assets::AllAssets` 可加载任意 `icons/*.svg`）：
/// 文件名左侧的类型微标（用户点名要 zed / VS Code 那种**图标**，不是文本标签）。
///
/// 无扩展名兜底通用文件图标；文件夹图标在行渲染里单独处理（`icons/folder.svg`）。
/// `pub`：第二阶段搜索面板迁 `rds_search::view` 后，由 workbench 作 `type_icon` 注入。
pub fn scratchpad_type_icon(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "sql" => "icons/database.svg",
        "log" => "icons/file-terminal.svg",
        "py" => "icons/file-code.svg",
        "md" | "markdown" => "icons/file-text.svg",
        "json" => "icons/braces.svg",
        "txt" => "icons/file-text.svg",
        "csv" | "tsv" => "icons/table.svg",
        "toml" => "icons/settings.svg",
        "yaml" | "yml" => "icons/file-code.svg",
        "html" | "htm" => "icons/code.svg",
        "css" => "icons/palette.svg",
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "rs" | "go" | "java" | "c" | "h"
        | "cpp" | "hpp" => "icons/file-code.svg",
        _ => "icons/file.svg",
    }
}

/// 行尾元信息（用户点名 2026-10-10：时间尽量简洁显示、可取消；**大小保留**）。
///
/// 文件只显示大小；文件夹显示子项计数「N 项」（原型 390-391 文件夹行 meta「3 项」，
/// 2026-10-11 对齐；懒加载目录未展开时 children 为 None → 不显示，避免误导性「0 项」）。
/// 修改/创建时间挪进「属性面板」（右键 → 属性），不再占树行。
pub(super) fn scratchpad_meta_label(entry: &ScratchpadEntry) -> String {
    match entry.kind {
        ScratchpadEntryKind::File => scratchpad_size_label(entry.size),
        ScratchpadEntryKind::Folder => entry
            .children
            .as_ref()
            .map(|c| format!("{} 项", c.len()))
            .unwrap_or_default(),
    }
}

/// RFC3339 → 相对时间（刚刚 / N 分钟 / N 小时 / N 天 / 日期）。
/// 注：暂无调用方（行内相对时间展示预留），保留纯函数供后续行 meta 接线。
#[allow(dead_code)]
pub(super) fn scratchpad_relative_time(rfc3339: &str) -> Option<String> {
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

/// RFC3339 → 绝对时间（属性面板用，对齐原型 705-706 的绝对时间格式：YYYY-MM-DD HH:MM）。
pub(super) fn scratchpad_absolute_time(rfc3339: &str) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    Some(parsed.format("%Y-%m-%d %H:%M").to_string())
}

/// 字节数 → 可读大小（B / KB / MB / GB）。
pub(super) fn scratchpad_size_label(size: u64) -> String {
    pub(super) const KB: f64 = 1024.0;
    pub(super) const MB: f64 = 1024.0 * 1024.0;
    pub(super) const GB: f64 = 1024.0 * 1024.0 * 1024.0;
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

/// 拆分文件名与扩展名（`foo.sql` → (`foo`, `.sql`)）。
pub(super) fn scratchpad_split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// 拼接模块内相对路径（父目录为空串时退化为子项名）。
pub(super) fn join_scratchpad_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent.trim_end_matches(['/', '\\']), name)
    }
}

/// 扩展名 → 图标点色（复用主题标准色，代码零裸色）。
///
/// 颜色以参数传入（局部拷贝），避免在渲染函数里长期持有 `theme` 借用，
/// 便于后续调用 `&mut cx` 方法。
pub(super) fn scratchpad_icon_color(
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

#[cfg(test)]
mod tests {
    // 不 `use super::*`：通配会把 gpui 的 `test` 宏带进作用域（gpui-kit-dev §窗口测试）。
    use super::{
        flatten_scratchpad, ScratchpadEntry, ScratchpadEntryKind, ScratchpadSearchMode, ScratchpadSort,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    fn entry(name: &str, kind: ScratchpadEntryKind) -> ScratchpadEntry {
        ScratchpadEntry {
            name: name.to_string(),
            path: PathBuf::from(format!("C:/proj/scratchpad/{name}")),
            kind,
            size: 0,
            modified_at: None,
            children: None,
        }
    }

    fn tag(id: &str, name: &str) -> tag::Tag {
        tag::Tag {
            id: id.to_string(),
            name: name.to_string(),
            color: None,
            icon: None,
            scope: tag::SCOPE_PROJECT.to_string(),
            created_at: String::new(),
            deleted_at: None,
        }
    }

    /// 标签筛选（统一规格 §7.3）：命中文件平铺（depth=0），目录与未命中文件跳过。
    #[test]
    fn flatten_with_tag_filter_flat_lists_only_tagged_files() {
        let root = Path::new("C:/proj/scratchpad");
        let entries = vec![
            entry("a.sql", ScratchpadEntryKind::File),
            entry("b.csv", ScratchpadEntryKind::File),
            entry("sub", ScratchpadEntryKind::Folder),
        ];
        let mut entry_tags = HashMap::new();
        entry_tags.insert("a.sql".to_string(), vec![tag("t1", "报表")]);
        let mut out = Vec::new();
        flatten_scratchpad(
            &entries,
            0,
            &HashSet::new(),
            &HashMap::new(),
            ScratchpadSort::Name,
            false,
            "",
            ScratchpadSearchMode::Name,
            false,
            false,
            Some(root),
            Some("t1"),
            &entry_tags,
            &mut out,
        );
        assert_eq!(out.len(), 1, "只收打了该标签的文件");
        assert_eq!(out[0].0, 0, "标签筛选平铺，depth=0");
        assert_eq!(out[0].1.name, "a.sql");
    }

    /// 未命中标签 → 空列表（目录即使"已展开"也不输出）。
    #[test]
    fn flatten_with_tag_filter_miss_returns_empty() {
        let root = Path::new("C:/proj/scratchpad");
        let entries = vec![entry("a.sql", ScratchpadEntryKind::File)];
        let entry_tags = HashMap::new();
        let mut out = Vec::new();
        flatten_scratchpad(
            &entries,
            0,
            &HashSet::new(),
            &HashMap::new(),
            ScratchpadSort::Name,
            false,
            "",
            ScratchpadSearchMode::Name,
            false,
            false,
            Some(root),
            Some("t2"),
            &entry_tags,
            &mut out,
        );
        assert!(out.is_empty());
    }

    /// 无标签筛选时保持树形原语义（文件夹递归，depth 递增）。
    #[test]
    fn flatten_without_tag_filter_keeps_tree_shape() {
        let root = Path::new("C:/proj/scratchpad");
        let mut sub = entry("sub", ScratchpadEntryKind::Folder);
        sub.children = Some(vec![entry("c.sql", ScratchpadEntryKind::File)]);
        let entries = vec![sub, entry("a.sql", ScratchpadEntryKind::File)];
        let mut out = Vec::new();
        flatten_scratchpad(
            &entries,
            0,
            &HashSet::new(),
            &HashMap::new(),
            ScratchpadSort::Name,
            false,
            "",
            ScratchpadSearchMode::Name,
            false,
            false,
            Some(root),
            None,
            &HashMap::new(),
            &mut out,
        );
        assert_eq!(out.len(), 2, "无筛选 = 原树压平（文件夹在前，未展开不递归）");
        assert_eq!(out[0].0, 0);
        assert_eq!(out[0].1.name, "sub");
        assert_eq!(out[1].0, 0);
        assert_eq!(out[1].1.name, "a.sql");
    }
}
