//! 复制列名「选项」对话框（`copy_names.rs`，2026-11-26）：参数模型 + 转义 + 拼接（纯函数）
//!
//! 真值（`org.jkiss.dbeaver.ui.editors.data`）：
//!
//! | 事实 | 出处 |
//! | --- | --- |
//! | 只在**选中多列**时弹对话框；单列直接拷 | `ResultSetHandlerMain.java:333-357` |
//! | 对话框 = `ResultSetHandlerCopySpecial.CopyConfigDialog(shell, "CopyGridNamesOptionsDialog")` | `:352` |
//! | 默认 列 `\t` / 行 `\n` / 引号 `"` | `ResultSetHandlerCopySpecial.java:129-131` |
//! | 候选 列 `\t` `;` `,` / 行 `\n` `|` `^` `,` / 引号 `"` `'` | `:154-156` |
//! | 打开时按上次保存回填（`DialogSettings` 按 dialogId） | `:132-140` |
//! | 确定时 `unescapeDisplayString` 后存回设置 | `:177-185` |
//! | 列名拼接**只用列分隔符**（行分隔符/引号对列名无作用） | `ResultSetHandlerMain.java:359-372` |
//! | 列名取值 `attr.getLabel()`，空则 `getName()`；有方言时 `getQuotedIdentifier` | `:363-371` |
//!
//! 本仓口径：
//! - 「多列」= 列浏览器清单的多选集合（本仓结果集表格尚无列多选，见 `result-panel.md` §9.73.3）；
//! - 设置**只在会话内记忆**（面板字段），不写盘——真值存 Eclipse `DialogSettings`，本仓没有
//!   该层（与 AutoRefresh 配置同口径，能力缺口已登记）。

/// 对话框标题（真值 `ResultSetMessages.copy_special_options` zh = 「选项」，用户截图同文）
pub(crate) const COPY_NAMES_OPTIONS_TITLE: &str = "选项";

/// 字面量文案（真值 `ResultSetMessages.copy_special_*`，zh bundle 与截图一致）
pub(crate) const COPY_NAMES_COLUMN_DELIMITER_LABEL: &str = "列分隔符";
pub(crate) const COPY_NAMES_ROW_DELIMITER_LABEL: &str = "行分隔符";
pub(crate) const COPY_NAMES_QUOTE_LABEL: &str = "引号字符";
/// 提示行（真值 `copy_special_hint_for_hotkey`；快捷键是 `CMD_COPY_SPECIAL_LAST` 的键位描述，
/// 本仓没有「重复上次复制特殊」命令，按真值文案固定显示 Ctrl+Alt+Shift+C）
pub(crate) const COPY_NAMES_HOTKEY_HINT: &str =
    "将来在此设置下可以使用快捷键：Ctrl+Alt+Shift+C";

/// 列分隔符候选（真值 `ResultSetHandlerCopySpecial.java:154`）
pub(crate) const COLUMN_DELIMITER_CANDIDATES: [&str; 3] = ["\t", ";", ","];
/// 行分隔符候选（`:155`）
pub(crate) const ROW_DELIMITER_CANDIDATES: [&str; 4] = ["\n", "|", "^", ","];
/// 引号候选（`:156`）
pub(crate) const QUOTE_CANDIDATES: [&str; 2] = ["\"", "'"];

/// 复制列名用的样式（真值 `ResultSetCopySettings` 的三个字符串参数，只取列名相关三项）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyNamesSettings {
    pub column_delimiter: String,
    pub row_delimiter: String,
    pub quote_string: String,
}

impl Default for CopyNamesSettings {
    fn default() -> Self {
        Self {
            // 真值 `CopyConfigDialog` 构造函数里的三行默认（`:129-131`）
            column_delimiter: "\t".to_string(),
            row_delimiter: "\n".to_string(),
            quote_string: "\"".to_string(),
        }
    }
}

/// 下拉里显示的形态：控制字符转成可见转义（真值 `UIWidgets.createDelimiterCombo` 同口径，
/// 截图里列分隔符显示成 `\t`、行分隔符显示成 `\n`）
pub(crate) fn escape_display_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out
}

/// 选择结果落回真值：把 `\t` / `\n` 之类的转义还原成控制字符
/// （真值 `CommonUtils.unescapeDisplayString`，`CopyConfigDialog.okPressed:178-180`）
pub(crate) fn unescape_display_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some('t') => {
                chars.next();
                out.push('\t');
            }
            Some('n') => {
                chars.next();
                out.push('\n');
            }
            Some('r') => {
                chars.next();
                out.push('\r');
            }
            Some('\\') => {
                chars.next();
                out.push('\\');
            }
            // 不认识的转义原样保留（不要吞字符）
            _ => out.push('\\'),
        }
    }
    out
}

/// 列名拼接（真值 `ResultSetHandlerMain.java:359-372`：只用列分隔符；空列表 = 空串）
pub(crate) fn join_column_names(names: &[String], column_delimiter: &str) -> String {
    names.join(column_delimiter)
}

/// 要不要弹「选项」对话框：真值 `if (attrs.size() > 1)`（`ResultSetHandlerMain.java:351`）
pub(crate) fn copy_names_needs_dialog(count: usize) -> bool {
    count > 1
}

/// 「复制列名」命令的结果（面板回给宿主；宿主要按它决定状态行说什么）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyColumnNamesOutcome {
    /// 已经写进剪贴板（单列；上游 `attrs.size() <= 1` 的路径，`:359-374`）
    Copied(usize),
    /// 多列 → 已打开「选项」浮层，等用户确定；**此时剪贴板还没动**
    Options(usize),
}

/// 对话框里可编辑的三个字段（下拉选中时用）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyNamesField {
    ColumnDelimiter,
    RowDelimiter,
    QuoteString,
}

impl CopyNamesField {
    /// 把选中值写进样式（值来自下拉候选或用户输入，已 `unescape` 过）
    pub fn apply(self, settings: &mut CopyNamesSettings, value: String) {
        match self {
            CopyNamesField::ColumnDelimiter => settings.column_delimiter = value,
            CopyNamesField::RowDelimiter => settings.row_delimiter = value,
            CopyNamesField::QuoteString => settings.quote_string = value,
        }
    }

    /// 当前值（对话框打开时回填 / 渲染选中态）
    pub fn value(self, settings: &CopyNamesSettings) -> &str {
        match self {
            CopyNamesField::ColumnDelimiter => &settings.column_delimiter,
            CopyNamesField::RowDelimiter => &settings.row_delimiter,
            CopyNamesField::QuoteString => &settings.quote_string,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_reference_dialog() {
        let settings = CopyNamesSettings::default();
        assert_eq!(settings.column_delimiter, "\t");
        assert_eq!(settings.row_delimiter, "\n");
        assert_eq!(settings.quote_string, "\"");
    }

    #[test]
    fn display_escaping_round_trips_control_characters() {
        assert_eq!(escape_display_string("\t"), "\\t");
        assert_eq!(escape_display_string("\n"), "\\n");
        assert_eq!(escape_display_string("\\"), "\\\\");
        for raw in ["\t", "\n", ";", "|", "*", "\\", "\\\\", "\r"] {
            assert_eq!(unescape_display_string(&escape_display_string(raw)), raw);
        }
        // 不认识的转义不吞字符
        assert_eq!(unescape_display_string("\\x"), "\\x");
    }

    #[test]
    fn join_uses_only_the_column_delimiter() {
        let names = vec!["id".to_string(), "name".to_string(), "amount".to_string()];
        assert_eq!(join_column_names(&names, "\t"), "id\tname\tamount");
        assert_eq!(join_column_names(&names, ","), "id,name,amount");
        assert_eq!(join_column_names(&[], "\t"), "");
        assert_eq!(join_column_names(&names[..1], "\t"), "id");
    }

    #[test]
    fn dialog_only_for_several_columns() {
        assert!(!copy_names_needs_dialog(0));
        assert!(!copy_names_needs_dialog(1));
        assert!(copy_names_needs_dialog(2));
        assert!(copy_names_needs_dialog(9));
    }

    #[test]
    fn field_apply_and_read_back() {
        let mut settings = CopyNamesSettings::default();
        CopyNamesField::ColumnDelimiter.apply(&mut settings, ",".to_string());
        CopyNamesField::QuoteString.apply(&mut settings, "'".to_string());
        assert_eq!(CopyNamesField::ColumnDelimiter.value(&settings), ",");
        assert_eq!(CopyNamesField::RowDelimiter.value(&settings), "\n");
        assert_eq!(CopyNamesField::QuoteString.value(&settings), "'");
    }
}
