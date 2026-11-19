//! SQL 格式化设置 model（格式化输出风格的六个旋钮，2026-11-19 自 editor 域迁出并扩全）。
//!
//! 对应 `crates/sql/src/formatter.rs` 的 [`FormatOptions`]（rds-sql 侧类型为枚举，
//! 这里按框架惯例落盘字符串，消费方按文本解析；service 侧白名单校验拒绝坏值）：
//!
//! - `keyword_case`：关键字大小写（`"preserve"` / `"upper"` / `"lower"`）；
//! - `indent_char`：缩进字符（`"space"` / `"tab"`；Tab 模式每级一个 `\t`，宽度不参与）；
//! - `indent_width`：缩进宽度（空格数 2 / 4 / 8，仅空格缩进时生效）；
//! - `comma_style`：逗号风格（`"trailing"` 尾随 = sqlglot 原生 / `"leading"` 前导）；
//! - `blank_lines_between`：语句间空行数（0 / 1 / 2）；
//! - `format_on_save`：Ctrl+S 落盘前先格式化（默认关）。
//!
//! 格式化方言不在设置里：自动跟随文档绑定的连接（引号风格 / 保留字按方言输出），
//! 未绑定连接时按 ANSI。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlFormat {
    #[serde(default = "default_keyword_case")]
    pub keyword_case: String,
    #[serde(default = "default_indent_char")]
    pub indent_char: String,
    #[serde(default = "default_indent_width")]
    pub indent_width: i64,
    #[serde(default = "default_comma_style")]
    pub comma_style: String,
    #[serde(default = "default_blank_lines_between")]
    pub blank_lines_between: i64,
    #[serde(default = "default_format_on_save")]
    pub format_on_save: bool,
}

impl Default for SqlFormat {
    fn default() -> Self {
        Self {
            keyword_case: default_keyword_case(),
            indent_char: default_indent_char(),
            indent_width: default_indent_width(),
            comma_style: default_comma_style(),
            blank_lines_between: default_blank_lines_between(),
            format_on_save: default_format_on_save(),
        }
    }
}

/// 关键字大小写默认：保持原样（sqlglot 原生输出）。
fn default_keyword_case() -> String {
    "preserve".to_string()
}

/// 缩进字符默认：空格。
fn default_indent_char() -> String {
    "space".to_string()
}

/// 缩进宽度默认：2 空格（= sqlglot pretty 原生）。
fn default_indent_width() -> i64 {
    2
}

/// 逗号风格默认：尾随（sqlglot 原生）。
fn default_comma_style() -> String {
    "trailing".to_string()
}

/// 语句间空行数默认：1（`;\n\n`）。
fn default_blank_lines_between() -> i64 {
    1
}

/// 保存时自动格式化默认：关。
fn default_format_on_save() -> bool {
    false
}
