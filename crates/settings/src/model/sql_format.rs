//! SQL 格式化设置 model（格式化输出风格的十一个旋钮）。
//!
//! 对应 `crates/sql/src/formatter.rs` 的 [`FormatOptions`]（rds-sql 侧类型为枚举，
//! 这里按框架惯例落盘字符串，消费方按文本解析；service 侧白名单校验拒绝坏值）。
//!
//! ## 旋钮与 lax 的对应关系
//!
//! `crates/sql` 的排版引擎是 **lax-sql**（lossless，只调空白），本结构的每个字段都直接
//! 映射到它的 `Configuration`（映射表在 `crates/editor/src/support/format.rs::options_from_settings`）：
//!
//! - `keyword_case`：关键字大小写（`"preserve"` / `"upper"` / `"lower"`）
//!   —— **唯一不走 lax 的一项**：lax 的关键字表只有 85 个通用词，本项目用 sqlglot 词法器
//!   （200+ 变体、跨方言并集）判定，见 `crates/sql/src/formatter.rs` 模块头；
//! - `indent_char` / `indent_width`：缩进字符与宽度 → lax `useTabs` / `indentWidth`；
//! - `line_width`：行宽 → lax `lineWidth`（默认 120，与 dprint 推荐值一致）；
//! - `clause_style`：子句布局（`"fill"` / `"expanded"`）→ lax `clauseStyle`；
//! - `comma_style`：逗号风格（`"trailing"` / `"leading"`）—— **lax 无此开关**，由本项目的
//!   行级后处理实现（只迁行尾逗号）；
//! - `new_line_kind`：换行符（`"auto"` / `"lf"` / `"crlf"`）→ lax `newLineKind`
//!   （`auto` 由 `crates/sql` 按整篇多数换行解析，保证语句体与语句间隔同源）；
//! - `ignore_node_comment_text` / `ignore_file_comment_text`：忽略标记
//!   → lax `ignoreNodeCommentText` / `ignoreFileCommentText`；
//! - `blank_lines_between`：语句间空行数（0 / 1 / 2）—— 由语句间隔规整负责，与 lax 不冲突；
//! - `format_on_save`：Ctrl+S 落盘前先格式化（默认关）。
//!
//! ## 排版不按方言
//!
//! 排版是**方言无关**的纯空白决策（lax 的 tokenizer 也不接受方言）。方言只用于转译与
//! 语义分析（`crates/sql/src/transpiler.rs`），不参与格式化；因此本节没有方言设置项。
//! 旧的"方言自动跟随文档绑定的连接"口径已作废。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlFormat {
    #[serde(default = "default_keyword_case")]
    pub keyword_case: String,
    #[serde(default = "default_indent_char")]
    pub indent_char: String,
    #[serde(default = "default_indent_width")]
    pub indent_width: i64,
    #[serde(default = "default_line_width")]
    pub line_width: i64,
    #[serde(default = "default_clause_style")]
    pub clause_style: String,
    #[serde(default = "default_comma_style")]
    pub comma_style: String,
    #[serde(default = "default_new_line_kind")]
    pub new_line_kind: String,
    #[serde(default = "default_ignore_node_comment_text")]
    pub ignore_node_comment_text: String,
    #[serde(default = "default_ignore_file_comment_text")]
    pub ignore_file_comment_text: String,
    #[serde(default = "default_blank_lines_between")]
    pub blank_lines_between: i64,
    #[serde(default = "default_format_on_save")]
    pub format_on_save: bool,
    /// 运算符两侧补空格（`a=1` → `a = 1`；默认关，只补不删）。
    #[serde(default = "default_normalize_operators")]
    pub normalize_operators: bool,
}

impl Default for SqlFormat {
    fn default() -> Self {
        Self {
            keyword_case: default_keyword_case(),
            indent_char: default_indent_char(),
            indent_width: default_indent_width(),
            line_width: default_line_width(),
            clause_style: default_clause_style(),
            comma_style: default_comma_style(),
            new_line_kind: default_new_line_kind(),
            ignore_node_comment_text: default_ignore_node_comment_text(),
            ignore_file_comment_text: default_ignore_file_comment_text(),
            blank_lines_between: default_blank_lines_between(),
            format_on_save: default_format_on_save(),
            normalize_operators: default_normalize_operators(),
        }
    }
}

/// 关键字大小写默认：保持原样（排版不改作者写法）。
fn default_keyword_case() -> String {
    "preserve".to_string()
}

/// 缩进字符默认：空格。
fn default_indent_char() -> String {
    "space".to_string()
}

/// 缩进宽度默认：2 空格。
fn default_indent_width() -> i64 {
    2
}

/// 行宽默认：120。
///
/// 取 dprint-core `RECOMMENDED_GLOBAL_CONFIGURATION.line_width` 的值，不擅自偏离上游默认
/// （lax 未显式配置时也解析成这个数）。需要更紧凑在设置里调 80 / 100。
fn default_line_width() -> i64 {
    120
}

/// 子句布局默认：展开（一行一项，DataGrip 的经典观感）。
fn default_clause_style() -> String {
    "expanded".to_string()
}

/// 逗号风格默认：尾随（`col1,\n  col2`）。
fn default_comma_style() -> String {
    "trailing".to_string()
}

/// 换行符默认：跟随原文（不改用户文档的换行风格）。
fn default_new_line_kind() -> String {
    "auto".to_string()
}

/// 节点级忽略标记默认：`dprint-ignore`（lax 的默认值，与 dprint 生态一致）。
fn default_ignore_node_comment_text() -> String {
    "dprint-ignore".to_string()
}

/// 文件级忽略标记默认：`dprint-ignore-file`（lax 的默认值）。
fn default_ignore_file_comment_text() -> String {
    "dprint-ignore-file".to_string()
}

/// 语句间空行数默认：1（`;\n\n`）。
fn default_blank_lines_between() -> i64 {
    1
}

/// 保存时自动格式化默认：关。
fn default_format_on_save() -> bool {
    false
}

/// 运算符两侧补空格默认：关。
///
/// 默认关的理由是**可预期性**：它改变用户写下的字符（不只是空白布局），
/// 且一元/通配形态（`-1` / `count(*)` / `select *` / `t.*`）靠上下文判定，
/// 先让用户看过预览再自己打开。
fn default_normalize_operators() -> bool {
    false
}
