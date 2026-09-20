//! 纯函数与值模型（无 `self`、无 I/O、可单测）。
//!
//! 为什么要独立成模块：模板 / 排序 / 压平 / 名称与元信息文案 / 脏点判据 / 类型色点与面板状态
//! 无关，此前混在 4100 行的单一文件里，改一处要在一屏里找半天；搬出后根只留
//! 「状态 + 行输入模型 + 视图协议」。
//!
//! 可见性：搬出的项加 `pub(super)`（= 在 `crate::scratchpad_view` 及其子孙模块可见），
//! 根用 `use self::primitives::*;` 收回来，**调用点一字未改**。

use super::*;

/// 内联编辑（新建 / 重命名 / 新建引用 / 引用改名）。
#[derive(Clone)]
pub(super) enum ScratchpadEdit {
    /// 新建文件（使用 `ScratchpadView::new_template` 选中的模板）。
    NewFile,
    NewFolder,
    /// 已选定外部路径，待输入别名（引用不复制文件，只记路径）。
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

/// 新建文件的起步模板（原型 §4.1：自动补后缀 + 填充占位内容）。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ScratchpadTemplate {
    #[default]
    Blank,
    Sql,
    Python,
    Markdown,
    Json,
}

impl ScratchpadTemplate {
    /// 全部模板（渲染 chip 行的顺序）。
    pub(super) const ALL: [ScratchpadTemplate; 5] = [
        ScratchpadTemplate::Blank,
        ScratchpadTemplate::Sql,
        ScratchpadTemplate::Python,
        ScratchpadTemplate::Markdown,
        ScratchpadTemplate::Json,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            ScratchpadTemplate::Blank => "空白",
            ScratchpadTemplate::Sql => "SQL",
            ScratchpadTemplate::Python => "Python",
            ScratchpadTemplate::Markdown => "Markdown",
            ScratchpadTemplate::Json => "JSON",
        }
    }

    /// 默认后缀（空白模板不加后缀）。
    pub(super) fn extension(self) -> Option<&'static str> {
        match self {
            ScratchpadTemplate::Blank => None,
            ScratchpadTemplate::Sql => Some(".sql"),
            ScratchpadTemplate::Python => Some(".py"),
            ScratchpadTemplate::Markdown => Some(".md"),
            ScratchpadTemplate::Json => Some(".json"),
        }
    }

    /// 占位内容（创建后写入，可直接编辑）。
    pub(super) fn content(self, name: &str) -> String {
        match self {
            ScratchpadTemplate::Blank => String::new(),
            ScratchpadTemplate::Sql => {
                format!("-- {name}\n-- 草稿：随手 SQL，Ctrl+S 保存回草稿箱\nSELECT 1;\n")
            }
            ScratchpadTemplate::Python => {
                format!(
                    "# {name}\n\n\ndef main() -> None:\n    pass\n\n\nif __name__ == \"__main__\":\n    main()\n"
                )
            }
            ScratchpadTemplate::Markdown => format!("# {name}\n\n- \n"),
            ScratchpadTemplate::Json => "{\n  \n}\n".to_string(),
        }
    }
}

/// 按模板补后缀：用户自写的后缀保留；模板补的后缀则随模板切换替换。
pub(super) fn scratchpad_apply_template_ext(name: &str, template: ScratchpadTemplate) -> String {
    // 已知模板后缀视为“模板补的”，可替换。
    pub(super) const TEMPLATE_EXTS: [&str; 4] = [".sql", ".py", ".md", ".json"];
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
#[derive(Clone)]
pub(super) struct ScratchpadClipboard {
    pub(super) mode: ScratchpadClipboardMode,
    pub(super) paths: Vec<String>,
}

/// 搜索模式（文件名过滤 / 内容搜索）。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ScratchpadSearchMode {
    #[default]
    Name,
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

/// 条目是否命中过滤（自身命中，或已加载子树命中）。
pub(super) fn scratchpad_entry_matches(
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
pub(super) fn flatten_scratchpad(
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
pub(super) fn scratchpad_basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// 该条目要不要画脏点（编辑器里有未保存修改）：**只有文件**画，文件夹不画。
///
/// 判据是**绝对路径**——条目路径即绝对路径，编辑器与草稿箱之间只交换绝对路径。
pub(super) fn scratchpad_shows_dirty_dot(dirty: &HashSet<String>, entry: &ScratchpadEntry) -> bool {
    entry.kind == ScratchpadEntryKind::File && dirty.contains(entry.path.to_string_lossy().as_ref())
}

/// 行尾元信息：相对时间（< 7 天）或日期；文件附加可读大小。
pub(super) fn scratchpad_meta_label(entry: &ScratchpadEntry) -> String {
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
