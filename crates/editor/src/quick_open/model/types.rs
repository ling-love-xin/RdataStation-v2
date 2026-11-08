//! 数据类型与模式解析（模式前缀 → Query，行类别 / 动作 / 组 / 源槽位）。

use crate::model::EditorMode;
use workbench_shell::model::{LeftPanel, RightPanel};

pub struct MetaObject {
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
pub enum Mode {
    /// 默认：连接 + 命令（+ 元数据 / 文件，Phase 1 接入）。
    Default,
    /// `>`：仅命令。
    Command,
    /// `#`：元数据全文档（注释 / 类型 / 定义）——Phase 1 接线 FTS 后才会有结果。
    FullText,
    /// `Ctrl+Shift+O` 唤起的「跳转到对象」：仅元数据、不含列，主动作 = 回树定位。
    ///
    /// 不由文本前缀进入（`parse` 不会产出本模式）：由打开动作置位，本档内敲 `>` / `#`
    /// 也不切换到命令 / 全文档，避免三模式前缀互相污染。
    Goto,
}

/// 名称档最小词长（本地源不受限；对齐导航搜索框的口径）。
pub const MIN_NEEDLE_LEN: usize = 2;
/// 内容档最小词长：trigram 分词器下不足 **3** 字（一个 trigram）必然无命中。
pub const MIN_FULLTEXT_LEN: usize = 3;

/// 解析后的查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub mode: Mode,
    /// 去掉前缀与首尾空白后的词（**保留大小写**：高亮要按原文定位）。
    pub needle: String,
}

impl Query {
    /// 本档最小词长（内容档比名称档高：trigram ≥ 3）。
    pub fn min_len(&self) -> usize {
        match self.mode {
            Mode::FullText => MIN_FULLTEXT_LEN,
            _ => MIN_NEEDLE_LEN,
        }
    }

    /// 词长是否够发一次搜索（命令档不发；本地源始终可搜）。
    pub fn async_ready(&self) -> bool {
        if self.mode == Mode::Command {
            return false;
        }
        self.needle.chars().count() >= self.min_len()
    }
}

/// 按首字符判定模式。只认**半角**符号（全角 `＞` / `＃` 当普通搜索词，避免输入法误触发）。
pub fn parse(raw: &str) -> Query {
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
pub enum RowKind {
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
    pub fn label(self) -> &'static str {
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
pub enum Action {
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
    /// 在导航树中定位一个对象（Quick Open 元数据命中行；`⌥↵`）。
    ///
    /// 带的是**统一引用**而不是搜索命中：执行侧（工作台）只需把它转给导航面板，
    /// 不必认识 `SearchHit`。
    RevealMetadata(engine::ObjectRef),
    /// 在中央编辑区打开一份草稿（绝对路径；可写 / 只读由编辑器按路径自己判定）。
    OpenDocument(std::path::PathBuf),
}

/// 一行结果。
#[derive(Debug, Clone)]
pub struct Row {
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
    /// 这行能不能「在树中定位」（元数据行才有：带统一引用）。
    ///
    /// 与 `action` 分开而不是从它反推：属性面板请求里的是 `PropertyRef`（另一族类型），
    /// 反推要再写一份 `PropertyKind → ObjectKind` 映射，两处口径早晚会分叉。
    pub locate: Option<engine::ObjectRef>,
    pub action: Action,
    /// 次级动作（`Alt+Enter` 触发；`None` = 该行没有次级动作）。
    ///
    /// Goto 档元数据行：主动作 = 回树定位（`RevealMetadata`），次级 = 属性面板（`ShowProperties`）；
    /// 其余档位 / 命令 / 文件 / 连接行均为 `None`。与 `action` 分开：主动作与次级动作面向不同
    /// 键位，塞进同一个枚举变体就得在执行侧猜「按的是哪个键」。
    pub secondary_action: Option<Action>,
}

/// 草稿箱文件命中（跨进程边界前的一层映射；源数据是 `scratchpad::FlatFile`）。
///
/// 只带行要用到的四个字段：**不做 IO**（打开在动作执行侧，浮层不认识盘）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileObject {
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
pub struct Group {
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
pub struct Sources<'a> {
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
