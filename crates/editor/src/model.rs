//! 编辑器领域模型（文档 / 模式 / 能力表 / 只读）
//!
//! **模式不是三个编辑器实现**：三种模式共用同一个编辑器内核，只在「挂哪些服务、显示哪些
//! chrome、结果去哪」上不同。这些差异**集中在一张能力表里**（`Capabilities::for_mode`），
//! 避免 v1 那样把同一套模式判定复制到三处（其规则表还有两条不可达规则）。

use std::fmt;

/// 文档 / 笔记的稳定标识
///
/// **一经生成永不变**，也不得用路径或下标替代：v1 用 `filePath` 派生面板 id，另存为后
/// 新旧 key 失配（tab 找不到、结果错位），是"用路径当身份"的经典代价。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 编辑器模式（通用 = 无连接档（文件编辑 + 本地脚本执行），与 SQL 平行）
///
/// 「本地脚本执行」走 **2b 独立链**（`EnvPort` + `run_script` → script_card），
/// 不走能力表的 `execute` 字段——后者只表示**数据库执行**（通用模式恒 false，
/// 保证 `talks_to_database()` 为假、不碰连接）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    /// 通用：带高亮的记事本（无连接档；文件编辑 + 本地脚本执行）
    #[default]
    General,
    /// SQL：脚本 + 连接 + 执行 + 结果
    Sql,
    /// 分析：单元 + 会话 + 输出（`.rdsnote`）
    Analysis,
}

impl EditorMode {
    /// 三个模式的声明顺序（= 模式指示器菜单顺序）
    pub const ALL: [EditorMode; 3] = [Self::General, Self::Sql, Self::Analysis];

    /// 界面展示名（与原型文档用词一致）
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Sql => "SQL",
            Self::Analysis => "分析",
        }
    }

    /// 状态栏 / 标签用的短标签
    pub fn short_label(self) -> &'static str {
        match self {
            Self::General => "通用",
            Self::Sql => "SQL",
            Self::Analysis => "NOTE",
        }
    }

    /// 持久化用的键（存库 / 存配置）
    ///
    /// 与展示名（`label()`）分开：展示名可以随时改文案，存库的值不能。
    /// 通用模式的键沿用历史 `"text"`（旧会话兼容；变体名改了，存储键不动）。
    pub fn as_key(self) -> &'static str {
        match self {
            Self::General => "text",
            Self::Sql => "sql",
            Self::Analysis => "analysis",
        }
    }

    /// 未命名文档的另存为默认文件名（扩展名与模式一致，另存后判定规则不会跟模式打架）
    pub fn default_file_name(self) -> &'static str {
        match self {
            Self::General => "未命名.txt",
            Self::Sql => "未命名.sql",
            Self::Analysis => "未命名.rdsnote",
        }
    }

    /// 从持久化键还原
    ///
    /// 认不出来的值当作 SQL（老库默认值就是 `sql`）：**不 panic，也不丢掉整份会话**。
    pub fn from_key(key: &str) -> Self {
        match key {
            "text" => Self::General,
            "analysis" => Self::Analysis,
            _ => Self::Sql,
        }
    }
}

/// 文档类型：文本与 SQL 模式共用 `Document`，分析模式是 `NoteBook`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// 单一文本缓冲（含 SQL 脚本）
    Document,
    /// 单元序列 + 输出
    NoteBook,
}

/// 结果去向（结果集是唯一权威，两个视图是它的投影）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTarget {
    /// 无停靠结果区（通用模式：数据库无输出；本地脚本的 script_card 内嵌在
    /// 结果区独立通道渲染，不经该字段，见 `EditorMode` 文档）
    None,
    /// 停靠结果面板（SQL 模式的默认视图）
    DockPanel,
    /// 单元内联输出（分析模式的默认视图）
    Inline,
}

/// 语言服务档位
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageService {
    /// 无语言服务（纯文本 / 按扩展名高亮）
    None,
    /// SQL：高亮 + 补全 + 诊断 + 格式化 + 转译
    Sql,
    /// 逐单元语言（sql / markdown / 后续 python、rust）
    PerCell,
}

/// 工具栏形态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarStyle {
    /// 通用档：模式指示 + 查找 + 脚本执行（环境选择器 + 运行按钮；Markdown 另有
    /// 预览切换）。不是「无执行」——本地脚本执行走 2b 链，见 `EditorMode` 文档。
    Minimal,
    /// 完整（执行族 + 文档级动作 + 执行位置 + 连接）
    Full,
    /// 笔记级（会话 / 全部运行 / 重启）+ 单元级动作；**笔记级动作当前未实现**
    /// （1c 留白：宿主工具栏只渲染模式指示器，见 render_toolbar 的 Notebook 分支）
    Notebook,
}

/// 只读的**两个独立维度**（语义完全不同，不可合并）
///
/// - 编辑器只读：不可输入（查看历史快照 / 分析资源锁定 / 大文件只读预览）
/// - 连接只读：可输入、可执行 SELECT，写语句被拦（连接策略 / 项目只读）
///
/// v2 现状把两者混在 `Shared::project_ui.read_only` 一个标志里，导致"不能编辑"与
/// "不能写库"无法分别表达。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadOnly {
    /// 编辑器只读（不可输入）
    pub editor: bool,
    /// 连接只读（写语句被拦）
    pub connection: bool,
}

impl ReadOnly {
    /// 可编辑 + 可写
    pub fn none() -> Self {
        Self::default()
    }

    /// 仅编辑器只读（查看快照等）
    pub fn editor_only() -> Self {
        Self {
            editor: true,
            connection: false,
        }
    }

    /// 是否可以输入
    pub fn can_edit(self) -> bool {
        !self.editor
    }
}

/// 能力表：模式差异的**唯一**分叉点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub kind: DocumentKind,
    pub language: LanguageService,
    /// 本地 SQL 目录补全档（SQL 模式为真值；通用模式恒 false——.py 的 LSP 补全
    /// 走独立判定 lsp_available，与该位无关；分析模式为 1c 预留，逐单元补全
    /// 尚未装 provider，见 sync_completion_provider 注释）
    pub completion: bool,
    /// 是否启用折叠（大文件档位下会被运行时降级；候选由我们自己算，见 `fold` 模块）
    pub folding: bool,
    /// 是否允许**数据库执行**（文本模式恒为 false；通用模式的本地脚本执行走
    /// 2b 独立链，不经过该字段，见 `EditorMode` 文档）
    pub execute: bool,
    pub output: OutputTarget,
    /// 是否显示「执行位置」（源库 / 本地加速 / 联邦）
    pub channel: bool,
    pub toolbar: ToolbarStyle,
    /// 是否有连接绑定（文本模式恒为 false）
    pub connection_binding: bool,
}

impl Capabilities {
    /// 取某模式的能力集
    pub fn for_mode(mode: EditorMode) -> Self {
        match mode {
            EditorMode::General => Self {
                kind: DocumentKind::Document,
                language: LanguageService::None,
                completion: false,
                // 折叠是缓冲区的编辑能力（与语言服务无关）：记事本也该能折括号块
                folding: true,
                execute: false,
                output: OutputTarget::None,
                channel: false,
                toolbar: ToolbarStyle::Minimal,
                connection_binding: false,
            },
            EditorMode::Sql => Self {
                kind: DocumentKind::Document,
                language: LanguageService::Sql,
                completion: true,
                folding: true,
                execute: true,
                output: OutputTarget::DockPanel,
                channel: true,
                toolbar: ToolbarStyle::Full,
                connection_binding: true,
            },
            EditorMode::Analysis => Self {
                kind: DocumentKind::NoteBook,
                language: LanguageService::PerCell,
                completion: true,
                // 笔记的折叠属单元层结构（1c）：整篇笔记先不开
                folding: false,
                execute: true,
                output: OutputTarget::Inline,
                channel: true,
                toolbar: ToolbarStyle::Notebook,
                connection_binding: true,
            },
        }
    }

    /// 是否与数据库通信（文本模式为硬约束：一律禁止）
    pub fn talks_to_database(&self) -> bool {
        self.connection_binding || self.execute
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_keys_round_trip_and_are_not_the_display_names() {
        for mode in [EditorMode::General, EditorMode::Sql, EditorMode::Analysis] {
            assert_eq!(EditorMode::from_key(mode.as_key()), mode);
        }
        // 键是存库值：与展示文案解耦（展示文案改了不影响老库）
        assert_eq!(EditorMode::General.as_key(), "text");
        assert_eq!(EditorMode::General.label(), "通用");
        // 认不出的值回到 SQL（老库默认值），不 panic
        assert_eq!(EditorMode::from_key(""), EditorMode::Sql);
        assert_eq!(EditorMode::from_key("notebook-v2"), EditorMode::Sql);
    }

    #[test]
    fn text_mode_never_touches_the_database() {
        let caps = Capabilities::for_mode(EditorMode::General);
        assert!(!caps.talks_to_database());
        assert!(!caps.execute);
        assert!(!caps.connection_binding);
        assert!(!caps.channel);
        assert_eq!(caps.output, OutputTarget::None);
        assert_eq!(caps.toolbar, ToolbarStyle::Minimal);
        // 折叠是编辑能力、不是语言服务：记事本也有（与 `language: None` 不矛盾）
        assert!(caps.folding);
        assert_eq!(caps.language, LanguageService::None);
    }

    #[test]
    fn sql_mode_binds_connection_and_reports_to_dock() {
        let caps = Capabilities::for_mode(EditorMode::Sql);
        assert!(caps.talks_to_database());
        assert!(caps.execute && caps.completion && caps.channel);
        assert_eq!(caps.kind, DocumentKind::Document);
        assert_eq!(caps.output, OutputTarget::DockPanel);
    }

    #[test]
    fn analysis_mode_is_a_notebook_with_inline_output() {
        let caps = Capabilities::for_mode(EditorMode::Analysis);
        assert_eq!(caps.kind, DocumentKind::NoteBook);
        assert_eq!(caps.language, LanguageService::PerCell);
        assert_eq!(caps.output, OutputTarget::Inline);
        assert_eq!(caps.toolbar, ToolbarStyle::Notebook);
        // 折叠属单元层（1c）：整篇笔记不开
        assert!(!caps.folding);
    }

    #[test]
    fn read_only_dimensions_are_independent() {
        assert!(ReadOnly::none().can_edit());
        assert!(!ReadOnly::editor_only().can_edit());
        // 编辑器只读不影响"连接是否只读"的判断（反之亦然）
        assert!(!ReadOnly::editor_only().connection);
        let both = ReadOnly {
            editor: true,
            connection: true,
        };
        assert!(!both.can_edit());
        assert!(both.connection);
    }

    #[test]
    fn document_id_is_stable_and_displayable() {
        let id = DocumentId::new("doc-1");
        assert_eq!(id.as_str(), "doc-1");
        assert_eq!(id.to_string(), "doc-1");
        assert_eq!(id, DocumentId::new("doc-1"));
    }
}
