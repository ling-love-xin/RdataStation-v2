//! 编辑器设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Editor {
    /// 编辑区是否显示行号（即时生效；内核 `LayoutMode::CodeEditor.line_number`）。
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
    /// 是否启用代码折叠（2026-10-27 起开关移到设置页；默认开）。
    /// 即时生效：编辑器 render 权威同步点把设置值同步到 `shared.show_folds` →
    /// `sync_fold` 重跑（install / set_folding(false)）。
    #[serde(default = "default_show_folds")]
    pub show_folds: bool,
    /// 是否启用基于缩进的折叠（VS Code 风格；默认开）。
    /// 开启后只要下一行缩进比当前行大，就会出现折叠按钮。
    /// 即时生效：编辑器 render 权威同步点重新计算折叠候选。
    #[serde(default = "default_indent_folding")]
    pub indent_folding: bool,
    /// 自定义开块关键字列表（折叠用；逗号分隔，大写匹配）。
    /// 默认：`CASE,BEGIN,IF,LOOP,WHILE,FOR,REPEAT`
    /// 即时生效：编辑器 render 权威同步点重新计算折叠候选。
    #[serde(default = "default_fold_open_keywords")]
    pub fold_open_keywords: String,
    /// 自定义闭块关键字列表（折叠用；逗号分隔，大写匹配）。
    /// 默认：`END`（含 `END IF` / `END CASE` 等复合词）
    /// 即时生效：编辑器 render 权威同步点重新计算折叠候选。
    #[serde(default = "default_fold_close_keywords")]
    pub fold_close_keywords: String,
    /// 是否软换行（默认开，与内核 `EditorState` 默认一致；关闭后长行横向滚动）。
    /// 即时生效：编辑器 render 权威同步点把设置值同步到内核 `set_soft_wrap`。
    #[serde(default = "default_soft_wrap")]
    pub soft_wrap: bool,
    /// 是否自动闭合括号 / 引号（默认开，与内核一致；关闭后输入 `(` 不再补 `)`）。
    /// 即时生效：编辑器 render 权威同步点同步内核 `set_auto_close`。
    #[serde(default = "default_auto_close")]
    pub auto_close: bool,
    /// 是否智能缩进（默认开；Enter 后按括号结构 / 语言规则选缩进）。
    /// 即时生效：编辑器 render 权威同步点同步内核 `set_smart_indent`。
    #[serde(default = "default_smart_indent")]
    pub smart_indent: bool,
    /// AI 补全（Inline Completion）开关——**当前版本置灰**（registry 登记 disabled），
    /// 内核槽位 `CompletionProvider::inline_completion` 未挂任何 AI 源，恒不产出建议。
    /// 默认关；未来接入 AI 源后放开 disabled，此值立即生效。
    #[serde(default = "default_inline_completion")]
    pub inline_completion: bool,
    /// 新文档默认缩进宽度（空格数；文档级「重设 ▾」可改单个文档）。
    #[serde(default = "default_tab_size")]
    pub default_tab_size: i64,
    /// 新文档默认是否用制表符缩进（false = 空格）。
    #[serde(default = "default_hard_tabs")]
    pub default_hard_tabs: bool,
    /// 新文档默认换行（`"LF"` / `"CRLF"`；落盘值，消费方按文本解析）。
    #[serde(default = "default_line_ending")]
    pub default_line_ending: String,
    /// 新文档默认编码（`"UTF-8"` / `"UTF-8-BOM"` / `"GBK"` / `"UTF-16LE"` / `"UTF-16BE"`；
    /// 落盘值，消费方按文本解析）。
    #[serde(default = "default_encoding")]
    pub default_encoding: String,
    /// SQL 格式化关键字大小写（`"preserve"` / `"upper"` / `"lower"`；消费方按文本解析）。
    /// 优先级：SQL 格式化增强（2026-10-28 用户定高优先级）。
    #[serde(default = "default_format_keyword_case")]
    pub format_keyword_case: String,
    /// SQL 格式化缩进宽度（空格数；2 = sqlglot 原生）。
    #[serde(default = "default_format_indent_width")]
    pub format_indent_width: i64,
    /// 保存时自动格式化 SQL（默认关；开启后 Ctrl+S 落盘前先跑格式化）。
    #[serde(default = "default_format_on_save")]
    pub format_on_save: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            show_line_numbers: default_show_line_numbers(),
            show_folds: default_show_folds(),
            indent_folding: default_indent_folding(),
            fold_open_keywords: default_fold_open_keywords(),
            fold_close_keywords: default_fold_close_keywords(),
            soft_wrap: default_soft_wrap(),
            auto_close: default_auto_close(),
            smart_indent: default_smart_indent(),
            inline_completion: default_inline_completion(),
            default_tab_size: default_tab_size(),
            default_hard_tabs: default_hard_tabs(),
            default_line_ending: default_line_ending(),
            default_encoding: default_encoding(),
            format_keyword_case: default_format_keyword_case(),
            format_indent_width: default_format_indent_width(),
            format_on_save: default_format_on_save(),
        }
    }
}

fn default_show_line_numbers() -> bool {
    true
}

/// 折叠默认开（2026-10-27：能力表置回 true 后，开关只控制是否显示，不关掉能力）
fn default_show_folds() -> bool {
    true
}

/// 基于缩进的折叠默认开（VS Code 风格，用户习惯）
fn default_indent_folding() -> bool {
    true
}

/// 折叠开块关键字默认列表（逗号分隔）
fn default_fold_open_keywords() -> String {
    "CASE,BEGIN,IF,LOOP,WHILE,FOR,REPEAT".to_string()
}

/// 折叠闭块关键字默认列表（逗号分隔）
fn default_fold_close_keywords() -> String {
    "END".to_string()
}

/// 软换行默认开（与内核 `EditorState::new` 的 `soft_wrap: true` 一致）
fn default_soft_wrap() -> bool {
    true
}

/// 自动闭合默认开（与内核 `LayoutMode::code_editor` 的 `auto_close: true` 一致）
fn default_auto_close() -> bool {
    true
}

/// 智能缩进默认开（与内核 `LayoutMode::code_editor` 的 `smart_indent: true` 一致）
fn default_smart_indent() -> bool {
    true
}

/// AI 补全默认关（当前版本置灰不可用；放开后接入 AI 源再改默认）
fn default_inline_completion() -> bool {
    false
}

fn default_tab_size() -> i64 {
    4
}

fn default_hard_tabs() -> bool {
    false
}

fn default_line_ending() -> String {
    "LF".to_string()
}

fn default_encoding() -> String {
    "UTF-8".to_string()
}

/// SQL 格式化关键字大小写默认：保持原样（sqlglot 原生输出）
fn default_format_keyword_case() -> String {
    "preserve".to_string()
}

/// SQL 格式化缩进宽度默认：2 空格（= sqlglot pretty 原生）
fn default_format_indent_width() -> i64 {
    2
}

/// 保存时自动格式化默认：关
fn default_format_on_save() -> bool {
    false
}
