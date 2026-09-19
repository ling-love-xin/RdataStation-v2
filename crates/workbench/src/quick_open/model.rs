//! Quick Open 的**纯逻辑**：查询解析（模式前缀）、行匹配与评分、命中区间、结果分组与上限。
//!
//! 不依赖 GPUI / `Shared`，输入输出都是纯数据，因此可直接单测
//! （`cargo test -p rds-workbench --lib quick_open`）。
//! 渲染与副作用在 `quick_open/delegate.rs` 与 `view.rs`（宿主 render / 事件路径）；
//! 命令目录在 `quick_open/commands.rs`（本模块只消费它的 `command_rows()`）。
//!
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`（§5 模式 / §6 元数据两档 / §8 匹配排序）。

use editor::model::EditorMode;

use crate::view::{LeftPanel, RightPanel};

/// 命令目录转发：目录本体在 [`crate::quick_open::commands`]（那里有 id / keywords / 动作），
/// 本模块只做「行 + 匹配」这一层（`filter_ranked` 要认识 `match_text`）。
pub(crate) use crate::quick_open::commands::command_rows;

/// 元数据命中（跨连接索引搜索的一行；由 `database::nav_jobs::SearchHit` 映射而来）。
///
/// 行里带上**可直接发给属性面板的请求**：执行侧就不需要再认识 `SearchHit`，
/// 映射口径与导航搜索结果一致（`nav_view::nav_search_hit_property`）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MetaObject {
    /// 统一引用（`engine::ObjectRef`）：行的**业务键**与跨屏定位都从它来。
    pub object: engine::ObjectRef,
    pub request: database::model::PropertyRequest,
    /// 行主文本：`schema.name`（表 / 视图 / 模式）或 `parent.column`（列）。
    pub title: String,
    pub kind: RowKind,
    /// 内容档的命中片段（FTS5 `snippet()` 产出，含 `<mark>` 标记）；名称档为 `None`。
    pub snippet: Option<String>,
}

/// 搜索模式：由输入的首字符决定（对齐 VSCode 的 Quick Open / Command Palette）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// 默认：连接 + 命令（+ 元数据 / 文件，Phase 1 接入）。
    Default,
    /// `>`：仅命令。
    Command,
    /// `#`：元数据全文档（注释 / 类型 / 定义）——Phase 1 接线 FTS 后才会有结果。
    FullText,
}

/// 名称档最小词长（本地源不受限；对齐导航搜索框的口径）。
pub(crate) const MIN_NEEDLE_LEN: usize = 2;
/// 内容档最小词长：trigram 分词器下不足 **3** 字（一个 trigram）必然无命中。
pub(crate) const MIN_FULLTEXT_LEN: usize = 3;

/// 解析后的查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    pub mode: Mode,
    /// 去掉前缀与首尾空白后的词（**保留大小写**：高亮要按原文定位）。
    pub needle: String,
}

impl Query {
    /// 本档最小词长（内容档比名称档高：trigram ≥ 3）。
    pub(crate) fn min_len(&self) -> usize {
        match self.mode {
            Mode::FullText => MIN_FULLTEXT_LEN,
            _ => MIN_NEEDLE_LEN,
        }
    }

    /// 词长是否够发一次搜索（命令档不发；本地源始终可搜）。
    pub(crate) fn async_ready(&self) -> bool {
        if self.mode == Mode::Command {
            return false;
        }
        self.needle.chars().count() >= self.min_len()
    }
}

/// 按首字符判定模式。只认**半角**符号（全角 `＞` / `＃` 当普通搜索词，避免输入法误触发）。
pub(crate) fn parse(raw: &str) -> Query {
    let t = raw.trim_start();
    if let Some(rest) = t.strip_prefix('>') {
        Query {
            mode: Mode::Command,
            needle: rest.trim().to_string(),
        }
    } else if let Some(rest) = t.strip_prefix('#') {
        Query {
            mode: Mode::FullText,
            needle: rest.trim().to_string(),
        }
    } else {
        Query {
            mode: Mode::Default,
            needle: t.trim().to_string(),
        }
    }
}

/// 行类别（决定类型标签；图标集不在第一批扩，缺则用文本标签）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    Table,
    View,
    Column,
    Schema,
    Routine,
    Sequence,
    Trigger,
    Connection,
    Command,
    File,
}

impl RowKind {
    /// 行首的类型短标签（类型双通道：文本 + 位置，不只靠颜色）。
    pub(crate) fn label(self) -> &'static str {
        match self {
            RowKind::Table => "表",
            RowKind::View => "视图",
            RowKind::Column => "列",
            RowKind::Schema => "模式",
            RowKind::Routine => "例程",
            RowKind::Sequence => "序列",
            RowKind::Trigger => "触发器",
            RowKind::Connection => "连接",
            RowKind::Command => "命令",
            RowKind::File => "文件",
        }
    }
}

/// 行被确认（↵ / 点击 / Ctrl+↵）时要执行的动作。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    /// 新建编辑器文档（查询 / 笔记 / 文件）。
    NewDocument(EditorMode),
    /// 打开属性面板并定位对象（元数据命中；请求已就绪，执行侧不再拼字段）。
    ShowProperties(Box<database::model::PropertyRequest>),
    /// 打开左侧面板。
    OpenLeftPanel(LeftPanel),
    /// 打开右侧面板。
    OpenRightPanel(RightPanel),
    /// 打开设置页。
    OpenSettings,
    /// 完全隐藏两侧边栏。
    HideSidebars,
    /// 恢复两侧边栏（还原隐藏前的模式）。
    RestoreSidebars,
    /// 选中数据源连接（切到导航并选中；**不自动连接**）。
    SelectConnection(usize),
    /// 在中央编辑区打开一份草稿（绝对路径；可写 / 只读由编辑器按路径自己判定）。
    OpenDocument(std::path::PathBuf),
}

/// 一行结果。
#[derive(Debug, Clone)]
pub(crate) struct Row {
    /// 业务键（选中跨重算跟随；**不用下标**）。
    pub key: String,
    pub kind: RowKind,
    /// 主文本（不含高亮标记；高亮区间在渲染期按查询词现算）。
    pub title: String,
    /// 参与**匹配**的文本（展示面之外的补充：命令的英文名 / 别名）。
    ///
    /// 匹配面可以比展示面宽，但高亮永远按 `title` 算——不把用户看不到的词标黄。
    pub match_text: String,
    /// 右侧次级信息（连接：驱动；命令：快捷键）。
    pub secondary: String,
    /// 内容档的命中片段（含 `<mark>` 标记）；名称档为 `None`。
    pub snippet: Option<String>,
    /// 「为什么命中」标签（内容档才填：名称命中 / 内容命中——名称是名称档的默认，不标）。
    pub why: Option<&'static str>,
    pub action: Action,
}

/// 草稿箱文件命中（跨进程边界前的一层映射；源数据是 `scratchpad::FlatFile`）。
///
/// 只带行要用到的四个字段：**不做 IO**（打开在动作执行侧，浮层不认识盘）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileObject {
    /// 模块内相对路径（`/` 分隔）：行的业务键与**替换匹配面**（`drafts/notes` 这样搜也能中）。
    pub relative_path: String,
    /// 所在目录（根下为空串）：行右侧展示用。
    pub folder: String,
    /// 文件名：行主文本（高亮按它算）。
    pub name: String,
    /// 绝对路径：打开时交给编辑器。
    pub path: std::path::PathBuf,
}

/// 一个分组（渲染为 `List` 的一个 section）。
#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub title: &'static str,
    /// 组头右侧的补充（如「搜索中…」；空则不显示）。
    pub note: String,
    pub rows: Vec<Row>,
    /// 被截掉的条数（超出单组 / 总量上限的部分；>0 时组尾显示「还有 N 条」）。
    pub hidden: usize,
}

/// 行的数据源（纯数据；I/O 全在宿主一侧）。
///
/// 为什么用结构体而不是继续加参数：这里已有四路来源（元数据 / 文件 / 连接 / 命令），
/// 都是 `&[..]` / `bool` / `&str`——位置参数迟早会被写反（换个位置编译器一声不吭）。
#[derive(Default)]
pub(crate) struct Sources<'a> {
    /// 连接（名称, 驱动）。
    pub connections: &'a [(String, String)],
    /// 元数据命中（名称档 / 内容档共用一份槽位，由 `mode` 决定怎么用）。
    pub meta: &'a [MetaObject],
    /// 元数据搜索进行中（组头显示「搜索中…」）。
    pub meta_searching: bool,
    /// 草稿箱文件（扁平清单；未打开项目时为空）。
    pub files: &'a [FileObject],
    /// 文件清单被后台截断时的说明（空 = 没有这回事）。
    pub files_note: String,
}

/// 组装结果分组：**空组不出现**（组头也不渲染）。
///
/// 元数据（跨连接名称档）由宿主从后台回填后以 `meta` 传入；本函数是纯函数，不做 I/O。
/// 元数据是核心：搜索中也要出组头（否则「搜不到」与「还在搜」分不清）。
pub(crate) fn build_groups(q: &Query, sources: &Sources<'_>) -> Vec<Group> {
    let groups = match q.mode {
        Mode::Command => non_empty("命令", filter_ranked(command_rows(), &q.needle), "")
            .into_iter()
            .collect(),
        // Phase 1：内容档（FTS）——引擎侧已完成匹配与排序，这里**不再按标题过滤**：
        // 否则「只在注释里命中」的行会被「标题不含该词」误杀。`why` 标签区分两者。
        Mode::FullText => {
            let note = if sources.meta_searching && q.async_ready() {
                "搜索中…"
            } else {
                ""
            };
            non_empty(
                "元数据全文（注释 / 定义）",
                fulltext_rows(sources.meta, &q.needle),
                note,
            )
            .into_iter()
            .collect()
        }
        Mode::Default => {
            let mut groups = Vec::new();
            let note = if sources.meta_searching && q.async_ready() {
                "搜索中…"
            } else {
                ""
            };
            groups.extend(non_empty(
                "元数据（表 · 视图 · 列）",
                filter_ranked(meta_rows(sources.meta), &q.needle),
                note,
            ));
            // 文件排在元数据之后、连接之前（原型设计 §7.2 的固定顺序）：
            // 对象是「查数库」的主线，文件是「手边的草稿」。
            groups.extend(non_empty(
                "文件（草稿箱）",
                filter_ranked(file_rows(sources.files), &q.needle),
                &sources.files_note,
            ));
            groups.extend(non_empty(
                "连接",
                filter_ranked(connection_rows(sources.connections), &q.needle),
                "",
            ));
            groups.extend(non_empty(
                "命令",
                filter_ranked(command_rows(), &q.needle),
                "",
            ));
            groups
        }
    };
    cap_groups(groups)
}

fn non_empty(title: &'static str, rows: Vec<Row>, note: &str) -> Option<Group> {
    if rows.is_empty() {
        None
    } else {
        Some(Group {
            title,
            note: note.to_string(),
            rows,
            hidden: 0,
        })
    }
}

/// 施加渲染上限：单组 ≤ `QUICK_OPEN_MAX_ROWS_PER_GROUP`、总计 ≤ `QUICK_OPEN_MAX_ROWS`。
///
/// 被截的条数记在 `hidden` 上（组尾一行「还有 N 条」），**不静默丢**：
/// 后台侧另有上限（单连接 50 / 总计 300），那是防止把渲染拖垮的硬闸，
/// 这里是呈现层的闸，两者都不是「结果少」的借口。
fn cap_groups(mut groups: Vec<Group>) -> Vec<Group> {
    let mut budget = crate::ui::QUICK_OPEN_MAX_ROWS;
    for group in &mut groups {
        let per_group = crate::ui::QUICK_OPEN_MAX_ROWS_PER_GROUP;
        let allowed = budget.min(per_group).min(group.rows.len());
        group.hidden = group.rows.len() - allowed;
        group.rows.truncate(allowed);
        budget = budget.saturating_sub(allowed);
    }
    groups
}

/// 文件行（次级信息 = 所在目录；没有目录的（模块根下）留空）。
///
/// 匹配面用**相对路径**而不是文件名：敲 `drafts/notes` 或只敲目录名也能命中
/// （高亮仍按文件名算——见 `Row::match_text`）。
fn file_rows(files: &[FileObject]) -> Vec<Row> {
    files
        .iter()
        .map(|file| Row {
            key: format!("file:{}", file.relative_path),
            kind: RowKind::File,
            title: file.name.clone(),
            match_text: file.relative_path.clone(),
            secondary: file.folder.clone(),
            snippet: None,
            why: None,
            action: Action::OpenDocument(file.path.clone()),
        })
        .collect()
}

/// 元数据行（次级信息 = 连接名 · 驱动）。
fn meta_rows(meta: &[MetaObject]) -> Vec<Row> {
    meta.iter()
        .map(|m| Row {
            key: meta_key(&m.object),
            kind: m.kind,
            title: m.title.clone(),
            match_text: m.title.clone(),
            secondary: format!("{} · {}", m.request.conn_label, m.request.driver),
            snippet: None,
            why: None,
            action: Action::ShowProperties(Box::new(m.request.clone())),
        })
        .collect()
}

/// 内容档行：多带 snippet（含命中标记）与「为什么命中」标签。
///
/// 不按标题过滤：引擎已经把「注释 / 数据类型命中」的对象排好了，标题过滤会把它们错杀。
fn fulltext_rows(meta: &[MetaObject], needle: &str) -> Vec<Row> {
    let needle_lower = needle.to_lowercase();
    meta.iter()
        .map(|m| {
            let matched_name =
                !needle_lower.is_empty() && m.title.to_lowercase().contains(&needle_lower);
            Row {
                key: meta_key(&m.object),
                kind: m.kind,
                title: m.title.clone(),
                match_text: m.title.clone(),
                secondary: format!("{} · {}", m.request.conn_label, m.request.driver),
                snippet: m.snippet.clone(),
                why: Some(if matched_name { "名称" } else { "内容" }),
                action: Action::ShowProperties(Box::new(m.request.clone())),
            }
        })
        .collect()
}

/// 把 FTS5 `snippet()` 的 `<mark>…</mark>` 标记切成「普通 / 命中」段（纯函数，可单测）。
///
/// 不闭会的标记退化为普通文本（引擎不该产出这种，但别让 UI panic）。
pub(crate) fn markup_segments(markup: &str) -> Vec<(String, bool)> {
    const OPEN: &str = "<mark>";
    const CLOSE: &str = "</mark>";
    let mut out = Vec::new();
    let mut rest = markup;
    loop {
        let Some(start) = rest.find(OPEN) else {
            if !rest.is_empty() {
                out.push((rest.to_string(), false));
            }
            return out;
        };
        if start > 0 {
            out.push((rest[..start].to_string(), false));
        }
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            out.push((after.to_string(), false));
            return out;
        };
        out.push((after[..end].to_string(), true));
        rest = &after[end + CLOSE.len()..];
    }
}

/// 元数据行的业务键：`meta:` + 统一引用的 key。
///
/// 用 `ObjectRef::key()`（连接 / catalog / schema / 父对象 / 名字）而不是自己拼：
/// ① 与导航树节点、搜索命中同构——跨屏说的是同一个对象；
/// ② 历史上这里只拼「连接 + 种类 + 父对象 + 名字」，**不含 catalog / schema**，
///    于是 `sales.orders` 与 `archive.orders` 会撞成同一个键（选中跟错行）。
fn meta_key(object: &engine::ObjectRef) -> String {
    format!("meta:{}", object.key())
}

/// 索引命中 → 元数据对象（`None` = 该类别不立行）。
///
/// 引用的构造（类别串解析、NULL 段收敛）与导航搜索结果走**同一个**入口
/// （`ObjectRef::from_index_hit`）；类别 → 属性面板类别的映射也共用
/// `database::model::property_ref_of`。本函数只负责本屏关心的两件事：
/// 哪些类别**上屏**、行主文本怎么写。
///
/// **上屏范围**：除 `Catalog` 外全部可寻址类别（表 / 视图 / 列 / 模式 / 例程 / 序列 / 触发器）。
/// 例程这三类曾经被白名单挡掉（当时以为“暂无定位能力”），但属性面板早已支持
/// （`property_panel::load_properties` 有 Routine / Sequence / Trigger 三支），
/// 而 `#` 全文档档的 FTS 索引**本就会写入例程**——挡住它们等于“搜到了却什么也不显示”。
pub(crate) fn meta_object(hit: &database::nav_jobs::SearchHit) -> Option<MetaObject> {
    let object = engine::ObjectRef::from_index_hit(
        &hit.conn_id,
        &hit.object_type,
        &hit.object_name,
        hit.parent_name.as_deref(),
        hit.catalog.as_deref(),
        hit.schema.as_deref(),
    )?;
    let kind = match object.kind {
        engine::ObjectKind::Table => RowKind::Table,
        engine::ObjectKind::View => RowKind::View,
        engine::ObjectKind::Column => RowKind::Column,
        engine::ObjectKind::Schema => RowKind::Schema,
        engine::ObjectKind::Routine => RowKind::Routine,
        engine::ObjectKind::Sequence => RowKind::Sequence,
        engine::ObjectKind::Trigger => RowKind::Trigger,
        // Catalog 不立行：它不是“对象详情”的入口（导航里点库得到的是列表），
        // 且索引 / FTS 两侧都不写 catalog 行。
        engine::ObjectKind::Catalog => return None,
    };
    let title = match kind {
        RowKind::Column | RowKind::Trigger => match hit.parent_name.as_deref() {
            Some(parent) if !parent.is_empty() => format!("{parent}.{}", hit.object_name),
            _ => hit.object_name.clone(),
        },
        _ => match hit.schema.as_deref() {
            Some(schema) if !schema.is_empty() => format!("{schema}.{}", hit.object_name),
            _ => hit.object_name.clone(),
        },
    };
    Some(MetaObject {
        request: database::model::PropertyRequest {
            property: database::model::property_ref_of(&object),
            conn_label: hit.conn_label.clone(),
            driver: hit.driver.clone(),
        },
        object,
        title,
        kind,
        snippet: hit.snippet.clone(),
    })
}

/// 草稿箱扁平行 → 行数据（跨 crate 映射的唯一一处，字段同名不同源，便于对照）。
///
/// 为什么不让浮层直接拿着 `scratchpad::FlatFile`：`model` 是纯数据层（可单测、不认识宿主），
/// 映射集中在这里，将来换文件源（如资产库存档）只需再写一个这样的函数。
pub(crate) fn file_object(file: &scratchpad::FlatFile) -> FileObject {
    FileObject {
        relative_path: file.relative_path.clone(),
        folder: file.folder.clone(),
        name: file.name.clone(),
        path: file.path.clone(),
    }
}

/// 连接行（下标即 `Shared::selected` 的位置）。
fn connection_rows(connections: &[(String, String)]) -> Vec<Row> {
    connections
        .iter()
        .enumerate()
        .map(|(ix, (name, driver))| Row {
            key: format!("conn:{ix}:{name}"),
            kind: RowKind::Connection,
            title: name.clone(),
            match_text: name.clone(),
            secondary: driver.clone(),
            snippet: None,
            why: None,
            action: Action::SelectConnection(ix),
        })
        .collect()
}

/// 过滤 + 排序：先按匹配档，再按命中长度、标题长度，最后按标题字典序（稳定）。
fn filter_ranked(rows: Vec<Row>, needle: &str) -> Vec<Row> {
    let mut hits: Vec<(Rank, Row)> = rows
        .into_iter()
        .filter_map(|row| rank_row(&row, needle).map(|r| (r, row)))
        .collect();
    hits.sort_by(|(a, ra), (b, rb)| {
        a.tier
            .cmp(&b.tier)
            .then(a.span_len.cmp(&b.span_len))
            .then_with(|| ra.title.len().cmp(&rb.title.len()))
            .then_with(|| ra.title.cmp(&rb.title))
    });
    hits.into_iter().map(|(_, row)| row).collect()
}

/// 行的匹配档：`title` 与 `match_text` 各算一次，取更优的那个。
///
/// 为什么两处都要算：命令的 `match_text` 是「展示名 + 关键词」的拼接串，
/// 精确 / 前缀两档在拼接串上命中不了（`打开设置 settings` ≠ `打开设置`）；
/// 只看拼接串，会把「输入完整展示名」的精确命中降级成子串命中。
fn rank_row(row: &Row, needle: &str) -> Option<Rank> {
    let title = rank(&row.title, needle);
    // 补充匹配面要够长才算数：单字符对英文关键词是噪声（凡是关键词含 `s` 的命令都会命中），
    // 而展示名照旧不受限（本地源支持单字符搜——那是「连接名第一个字母」这种真实用法）。
    if needle.chars().count() < MIN_NEEDLE_LEN {
        return title;
    }
    [title, rank(&row.match_text, needle)]
        .into_iter()
        .flatten()
        .min_by_key(|r| (r.tier, r.span_len, r.span_start))
}

/// 匹配档（越小越优先）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tier {
    /// 完全相同。
    Exact,
    /// 名称前缀。
    Prefix,
    /// 词首命中（`snake_case` / `camelCase` 分段首字母）。
    WordStart,
    /// 子串命中（同档内按位置靠前）。
    Substring,
}

/// 一次命中的排序信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rank {
    pub tier: Tier,
    /// 命中区间长度（同档内短者优先）。
    pub span_len: usize,
    /// 命中起点（同档内靠前者优先）。
    pub span_start: usize,
}

/// 纯排序信息（不含高亮区间计算）。
fn rank(title: &str, needle: &str) -> Option<Rank> {
    let span = match_span(title, needle)?;
    let tier = if needle.is_empty() {
        Tier::Substring
    } else if title.eq_ignore_ascii_case(needle) {
        Tier::Exact
    } else if span.0 == 0 {
        Tier::Prefix
    } else if is_word_start(title, span.0) {
        Tier::WordStart
    } else {
        Tier::Substring
    };
    Some(Rank {
        tier,
        span_len: span.1.saturating_sub(span.0),
        span_start: span.0,
    })
}

/// 命中区间（字节范围；空词返回 `(0, 0)`）。
pub(crate) fn match_span(title: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return Some((0, 0));
    }
    // 大小写折叠只按 ASCII 生效；折叠后长度变化（罕见 Unicode）时放弃定位，退化为「整串可搜」。
    let (hay, ndl) = (title.to_lowercase(), needle.to_lowercase());
    if hay.len() != title.len() || ndl.len() != needle.len() {
        return if title.contains(needle) {
            Some((0, 0))
        } else {
            None
        };
    }
    hay.find(&ndl).map(|pos| (pos, pos + ndl.len()))
}

/// 命中位置是否落在某个分段的词首（`order_items` 的 `items`、`orderItems` 的 `Items`）。
fn is_word_start(title: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let prev = title[..pos].chars().next_back();
    let cur = title[pos..].chars().next();
    match (prev, cur) {
        (Some(p), Some(c)) => {
            p == '_' || p == '-' || p == ' ' || p == '.' || (p.is_lowercase() && c.is_uppercase())
        }
        _ => false,
    }
}

/// `LIKE` 通配符转义与 FTS5 `MATCH` 词清洗**不在这里**：
/// - `LIKE` 的转义已在写侧（`engine::persistence::MetadataCacheOps::like_escape`，带单测）；
/// - FTS 的词清洗归 Phase 1 接线的 `search_fts`（现状是 `format!("{}*", q)` 裸拼，
///   会被用户输入里的 `"` / `*` / `NEAR` 破坏语法）——修在它自己的 crate，避免两处各写一套。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quick_open::commands::command_specs;
    use database::model::PropertyKind;

    #[test]
    fn parse_reads_mode_from_first_char_only() {
        assert_eq!(parse("order").mode, Mode::Default);
        assert_eq!(parse("order").needle, "order");
        assert_eq!(parse(">新建").mode, Mode::Command);
        assert_eq!(parse(">新建").needle, "新建");
        assert_eq!(parse("> 新建 查询 ").needle, "新建 查询");
        assert_eq!(parse("#渠道").mode, Mode::FullText);
        // 前缀不是首字符 → 当普通词
        assert_eq!(parse("订单 > 明细").mode, Mode::Default);
        assert_eq!(parse("   >撤销").mode, Mode::Command);
        // 全角符号不当前缀（输入法误触发防护）
        assert_eq!(parse("＞撤销").mode, Mode::Default);
    }

    #[test]
    fn async_ready_threshold_depends_on_mode() {
        // 名称档（默认）：≥ 2 字（中文按字符计数，不按字节）
        assert!(!parse("o").async_ready());
        assert!(parse("or").async_ready());
        assert!(parse("订单").async_ready());
        // 内容档（`#`）：trigram 下 ≥ 3 字（2 字不足一个 trigram，必无命中）
        assert!(!parse("#渠").async_ready());
        assert!(!parse("#渠道").async_ready());
        assert!(parse("#渠道与").async_ready());
        // 命令档不发元数据搜索
        assert!(!parse(">新建").async_ready());
        assert!(!parse(">").async_ready());
        // 门槛自描述（UI 用它算“还差几个字”）
        assert_eq!(parse("o").min_len(), 2);
        assert_eq!(parse("#渠").min_len(), 3);
    }

    #[test]
    fn match_span_reports_byte_range_and_tolerates_non_ascii_case() {
        assert_eq!(match_span("order_items", "items"), Some((6, 11)));
        assert_eq!(match_span("OrderItems", "order"), Some((0, 5)));
        assert_eq!(match_span("订单明细", "明细"), Some((6, 12)));
        assert_eq!(match_span("orders", ""), Some((0, 0)));
        assert_eq!(match_span("orders", "zzz"), None);
        // 非 ASCII 大小写折叠后长度变化 → 放弃定位
        assert_eq!(match_span("İstanbul", "istanbul"), None);
    }

    #[test]
    fn rank_prefers_exact_then_prefix_then_word_start_then_substring() {
        let ranked = |needle: &str| {
            filter_ranked(
                ["order_items", "pre_order", "order", "order_id"]
                    .iter()
                    .map(|t| Row {
                        key: (*t).to_string(),
                        kind: RowKind::Command,
                        title: (*t).to_string(),
                        match_text: (*t).to_string(),
                        secondary: String::new(),
                        snippet: None,
                        why: None,
                        action: Action::OpenSettings,
                    })
                    .collect(),
                needle,
            )
            .into_iter()
            .map(|r| r.title)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            ranked("order"),
            vec!["order", "order_id", "order_items", "pre_order"]
        );
        // 词首档：`items` 只命中 order_items（下划线之后）
        assert_eq!(ranked("items"), vec!["order_items"]);
        // 空词 → 全部保留（排序退化为长度 + 字典序）
        assert_eq!(ranked("").len(), 4);
    }

    fn conns() -> Vec<(String, String)> {
        vec![
            ("营销分析（生产）".to_string(), "postgres".to_string()),
            ("分析库".to_string(), "duckdb".to_string()),
        ]
    }

    /// 造一条索引命中（真实形状：`database::nav_jobs::SearchHit`）。
    fn hit(object_type: &str, name: &str, parent: Option<&str>) -> database::nav_jobs::SearchHit {
        database::nav_jobs::SearchHit {
            conn_id: "P_conn".to_string(),
            conn_label: "营销分析（生产）".to_string(),
            driver: "postgres".to_string(),
            object_type: object_type.to_string(),
            object_name: name.to_string(),
            parent_name: parent.map(|p| p.to_string()),
            catalog: Some("main".to_string()),
            schema: Some("public".to_string()),
            snippet: None,
        }
    }

    /// 造一份草稿箱文件（真实形状：`scratchpad::FlatFile` 经 `file_object` 映射）。
    fn draft(relative_path: &str) -> FileObject {
        let (folder, name) = match relative_path.rsplit_once('/') {
            Some((dir, name)) => (dir.to_string(), name.to_string()),
            None => (String::new(), relative_path.to_string()),
        };
        file_object(&scratchpad::FlatFile {
            relative_path: relative_path.to_string(),
            folder,
            name,
            path: std::path::PathBuf::from("C:/proj/scratchpad").join(relative_path),
        })
    }

    #[test]
    fn build_groups_filters_and_drops_empty_groups() {
        let conns = conns();
        // 命令模式：只有命令组
        let cmd = build_groups(
            &parse(">设置"),
            &Sources {
                connections: &conns,
                ..Default::default()
            },
        );
        assert_eq!(cmd.len(), 1);
        assert_eq!(cmd[0].title, "命令");
        assert!(cmd[0].rows.iter().all(|r| r.title.contains("设置")));

        // 默认模式：命中连接则不出现命令组
        let hit_conn = build_groups(
            &parse("分析库"),
            &Sources {
                connections: &conns,
                ..Default::default()
            },
        );
        assert_eq!(hit_conn.len(), 1);
        assert_eq!(hit_conn[0].title, "连接");
        assert_eq!(hit_conn[0].rows[0].action, Action::SelectConnection(1));

        // `#` 内容档：没有回填命中时为空（空态由委托给说明文案）
        assert!(
            build_groups(
                &parse("#渠道与"),
                &Sources {
                    connections: &conns,
                    ..Default::default()
                }
            )
            .is_empty()
        );

        // 无匹配：一个组都没有
        assert!(
            build_groups(
                &parse("zzzz"),
                &Sources {
                    connections: &conns,
                    ..Default::default()
                }
            )
            .is_empty()
        );
    }

    /// 文件源：相对路径是业务键与匹配面，主文本是文件名，动作带可打开的绝对路径。
    #[test]
    fn file_rows_match_on_name_and_path_but_highlight_the_name() {
        let files = [draft("notes/a.md"), draft("queries/last.sql")];
        let groups = build_groups(
            &parse("last"),
            &Sources {
                files: &files,
                ..Default::default()
            },
        );
        // 文件排在元数据之后、连接之前（原型设计 §7.2）；这里没有元数据与连接，它就是第一组
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].title, "文件（草稿箱）");
        let row = &groups[0].rows[0];
        assert_eq!(row.title, "last.sql", "主文本是文件名");
        assert_eq!(row.secondary, "queries", "右侧是所在目录");
        assert_eq!(row.key, "file:queries/last.sql", "键是相对路径");
        assert_eq!(row.kind.label(), "文件");
        match &row.action {
            Action::OpenDocument(path) => {
                assert!(
                    path.ends_with("queries/last.sql"),
                    "动作要带绝对路径：{path:?}"
                )
            }
            other => panic!("文件行动作应是打开文档，实际 {other:?}"),
        }

        // 按**目录**也能搜到（匹配面是相对路径），但高亮仍按文件名算（文件名里没有 `notes`）
        let by_folder = build_groups(
            &parse("notes"),
            &Sources {
                files: &files,
                ..Default::default()
            },
        );
        assert_eq!(by_folder[0].rows.len(), 1);
        assert!(match_span(&by_folder[0].rows[0].title, "notes").is_none());

        // 清单被后台截断：组头要说「没全列」而不是安静地少几行
        let note = build_groups(
            &parse("a"),
            &Sources {
                files: &files,
                files_note: "文件较多，只列前 500 个（继续输入缩小范围）".to_string(),
                ..Default::default()
            },
        );
        assert!(
            note[0].note.contains("只列前"),
            "截断要说清：{}",
            note[0].note
        );

        // 没打开项目（文件源为空）→ 连空组都不出现
        assert!(build_groups(&parse("last"), &Sources::default()).is_empty());
    }

    #[test]
    fn meta_rows_come_first_and_carry_properties_request() {
        let conns = conns();
        let objects: Vec<MetaObject> = [
            hit("table", "orders", None),
            hit("column", "order_id", Some("orders")),
            hit("routine", "fn_x", None), // 例程也立行（属性面板支持；FTS 会命中它）
            hit("index", "idx_a", None),  // 不可寻址类别（索引）→ 不立行
        ]
        .iter()
        .filter_map(meta_object)
        .collect();
        assert_eq!(objects.len(), 3, "索引行应被过滤掉，例程应保留");

        let groups = build_groups(
            &parse("order"),
            &Sources {
                connections: &conns,
                meta: &objects,
                ..Default::default()
            },
        );
        // 元数据排在连接 / 命令之前
        assert_eq!(groups[0].title, "元数据（表 · 视图 · 列）");
        assert_eq!(groups[0].rows.len(), 2, "`order` 只命中表与列两行");
        // 表行标题是 `schema.name`，列行是 `父表.列`
        assert!(groups[0].rows.iter().any(|r| r.title == "public.orders"));
        assert!(groups[0].rows.iter().any(|r| r.title == "orders.order_id"));
        // 列行的业务键带父表（否则同名列会撞键）
        let col = groups[0]
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Column)
            .expect("列行");
        assert!(col.key.contains("orders"), "列键要含父表：{}", col.key);
        match &col.action {
            Action::ShowProperties(request) => {
                assert_eq!(request.property.parent.as_deref(), Some("orders"));
                assert_eq!(request.property.conn_id, "P_conn");
                assert_eq!(request.conn_label, "营销分析（生产）");
            }
            other => panic!("列行动作应是属性面板，实际 {other:?}"),
        }

        // 例程行：类目标签与属性类别都要对（曾经被白名单挡掉）
        let routine = objects
            .iter()
            .find(|m| m.kind == RowKind::Routine)
            .expect("例程应立行");
        assert_eq!(routine.request.property.kind, PropertyKind::Routine);
        assert_eq!(routine.kind.label(), "例程");
    }

    #[test]
    fn searching_note_shows_only_when_query_is_ready() {
        let conns = conns();
        let objects: Vec<MetaObject> = [hit("table", "orders", None)]
            .iter()
            .filter_map(meta_object)
            .collect();
        // 搜索中 + 词长够 → 组头带「搜索中…」
        let busy = build_groups(
            &parse("or"),
            &Sources {
                connections: &conns,
                meta: &objects,
                meta_searching: true,
                ..Default::default()
            },
        );
        assert_eq!(busy[0].note, "搜索中…");
        // 单字符（未达门槛）→ 不发搜索，也不显示搜索中
        let short = build_groups(
            &parse("o"),
            &Sources {
                connections: &conns,
                meta: &objects,
                meta_searching: true,
                ..Default::default()
            },
        );
        assert!(short.iter().all(|g| g.note.is_empty()));
    }

    /// 内容档行：只在注释里命中的对象**不被标题过滤误杀**，并标「内容」/「名称」。
    #[test]
    fn fulltext_rows_keep_content_hits_and_tag_the_reason() {
        let mut content_hit = hit("table", "orders", None);
        content_hit.snippet = Some("…含<mark>渠道</mark>与优惠…".to_string());
        let content = meta_object(&content_hit).expect("表命中");
        let rows = build_groups(
            &parse("#含渠道"),
            &Sources {
                meta: &[content],
                ..Default::default()
            },
        );
        assert_eq!(rows[0].title, "元数据全文（注释 / 定义）");
        let row = &rows[0].rows[0];
        assert_eq!(row.title, "public.orders");
        assert!(
            row.snippet
                .as_deref()
                .unwrap_or_default()
                .contains("<mark>"),
            "snippet 要带命中标记（UI 靠它上色）"
        );
        assert_eq!(row.why, Some("内容"), "标题不含词 → 内容命中");

        // 标题也含词 → 标「名称」
        let mut name_hit = hit("table", "orders", None);
        name_hit.snippet = Some("<mark>ord</mark>ers".to_string());
        let name = meta_object(&name_hit).expect("表命中");
        let rows = build_groups(
            &parse("#ord"),
            &Sources {
                meta: &[name],
                ..Default::default()
            },
        );
        assert_eq!(rows[0].rows[0].why, Some("名称"));
    }

    #[test]
    fn markup_segments_splits_highlight_marks() {
        assert_eq!(
            markup_segments("a<mark>渠道</mark>b"),
            vec![
                ("a".to_string(), false),
                ("渠道".to_string(), true),
                ("b".to_string(), false)
            ]
        );
        assert_eq!(markup_segments("plain"), vec![("plain".to_string(), false)]);
        assert_eq!(
            markup_segments("<mark>x</mark>"),
            vec![("x".to_string(), true)]
        );
        assert!(markup_segments("").is_empty());
        // 不闭会的标记退化为普通文本（不 panic）
        assert_eq!(
            markup_segments("a<mark>b"),
            vec![("a".to_string(), false), ("b".to_string(), false)]
        );
    }

    /// 渲染上限：单组 ≤ 8 行、总计 ≤ 50 行；被截的条数落在 `hidden` 上（组尾显示）。
    #[test]
    fn groups_are_capped_and_report_what_was_hidden() {
        let many: Vec<(String, String)> = (0..20)
            .map(|i| (format!("conn{i:02}"), "postgres".to_string()))
            .collect();
        let groups = build_groups(
            &parse(""),
            &Sources {
                connections: &many,
                ..Default::default()
            },
        );
        let conns = groups.iter().find(|g| g.title == "连接").expect("连接组");
        assert_eq!(conns.rows.len(), 8, "单组上限 8 行");
        assert_eq!(conns.hidden, 12, "被截的条数要记下来（组尾显示）");

        // 总量上限：命令档只有一组，构造一个命中很多的词也超不过 50；
        // 这里直接验证「总计不超过 50」这条不变量。
        let total: usize = groups.iter().map(|g| g.rows.len()).sum();
        assert!(total <= 50, "总渲染量不得超过 50 行：{total}");
    }

    /// 命令的英文别名（`keywords`）参与匹配，但高亮仍按展示名算。
    ///
    /// 这层是英文 / 拼音输入习惯的兼容：中文用户敲 `settings` / `database`
    /// 也能找到对应条目（且命中不靠“给每条命令记英文展示名”）。
    #[test]
    fn command_keywords_match_without_painting_highlights_on_the_label() {
        let groups = build_groups(&parse(">settings"), &Sources::default());
        assert_eq!(groups.len(), 1, "命令档只出命令组");
        assert_eq!(groups[0].rows.len(), 1, "`settings` 只应命中打开设置");
        let row = &groups[0].rows[0];
        assert_eq!(row.title, "打开设置");
        assert_eq!(row.key, "cmd:app.settings", "键跟着稳定 id 走");
        // 展示名里没有 `settings` → 不该算出高亮区间（否则会标出莫名其妙的黄块）
        assert!(match_span(&row.title, "settings").is_none());

        let groups = build_groups(&parse(">database"), &Sources::default());
        let hit = groups[0]
            .rows
            .iter()
            .find(|r| r.title == "打开数据库导航")
            .expect("`database` 应命中数据库导航");
        assert_eq!(hit.key, "cmd:panel.database");

        // 中文输入照旧按展示名命中（关键词不抢展示名的主位）
        let groups = build_groups(&parse(">设置"), &Sources::default());
        assert_eq!(groups[0].rows[0].title, "打开设置");

        // 单字符不进关键词匹配面：否则 `s` 会把一半命令都捞出来（噪声）
        let groups = build_groups(&parse(">s"), &Sources::default());
        assert!(
            groups.is_empty(),
            "单字符不应命中英文关键词：{:?}",
            groups.iter().map(|g| g.title).collect::<Vec<_>>()
        );
    }

    /// 命令目录的 id 与行键（`cmd:{id}`）都要稳定且唯一。
    ///
    /// `id` 是选中跟随的键，也是将来跨 crate 登记表的键：`label` 可以改、`id` 不能改
    /// （改了会让「记住了上次选中的命令」这类能力失效）。
    #[test]
    fn command_ids_and_row_keys_are_stable_and_unique() {
        let specs = command_specs();
        let mut ids: Vec<&str> = specs.iter().map(|s| s.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "命令 id 必须唯一");

        let rows = command_rows();
        assert_eq!(rows.len(), total, "每条命令都要出一行");
        let mut keys: Vec<&String> = rows.iter().map(|r| &r.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total, "命令键必须唯一（选中跟随依赖它）");
        assert_eq!(rows[0].action, Action::NewDocument(EditorMode::Sql));
        // 关键词不能漏在匹配面之外：目录里写了就必须能被搜到
        for spec in specs {
            assert!(
                spec.keywords.trim().is_empty()
                    || spec.keywords.split_whitespace().all(|kw| {
                        filter_ranked(command_rows(), kw)
                            .iter()
                            .any(|r| r.key == format!("cmd:{}", spec.id))
                    }),
                "命令 {} 的 keywords 应当个个都能命中自己",
                spec.id
            );
        }
    }
}
