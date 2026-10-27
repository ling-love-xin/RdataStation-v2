//! SQL 语法上下文提供者（A 方案）：自研词法器判定字符串 / 注释
//!
//! ## 为什么自建（替代 tree-sitter grammar 路径）
//!
//! 内核 `LanguageProvider::syntax_context_provider` 只认 tree-sitter grammar
//! registry（无 grammar → `context_at` 恒为 `Code`，SQL 的 `not_in([String,
//! Comment])` 永不命中）。本仓 SQL 高亮走自研词法（`engine::sql::highlight`，
//! 基于 sqlglot `Tokenizer`），再引 tree-sitter-sql 会引入**第二套语法理解**
//! （方言差异、维护成本），且与自研高亮可能不一致。
//!
//! A 方案：vendor patch 给 gpui-component 加了 `register_syntax_context_provider`
//! 注册点（优先于 grammar 生效），本 provider 把判定**收口到自研词法器**——
//! 「高亮理解」与「编辑上下文理解」同源，杜绝偏差。
//!
//! ## 无状态与性能
//!
//! `context_at` 每次对全文做词法判定（与高亮同一 Tokenizer，非二次解析）。
//! 内核 `EditorLanguage` 缓存 provider 与语言选择；编辑路径才调用，文档量级
//! 下开销可接受（后续若遇大文档卡顿，再考虑增量 / 缓存）。

use gpui_kit::base::input::{Rope, SyntaxContext, SyntaxContextProvider};

use engine::sql::ContextKind;

/// SQL 语法上下文提供者：`context_at` 每次对全文词法判定（无内部状态）
#[derive(Default)]
pub struct SqlSyntaxContext;

impl SyntaxContextProvider for SqlSyntaxContext {
    fn context_at(&self, text: &Rope, offset: usize) -> SyntaxContext {
        let sql = text.to_string();
        match engine::sql::context_kind_at(&sql, offset) {
            ContextKind::String => SyntaxContext::String,
            ContextKind::Comment => SyntaxContext::Comment,
            ContextKind::Code => SyntaxContext::Code,
        }
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入
    use gpui_kit::base::input::{Rope, SyntaxContext, SyntaxContextProvider};

    use super::SqlSyntaxContext;

    fn at(sql: &str, offset: usize) -> SyntaxContext {
        SqlSyntaxContext.context_at(&Rope::from_str(sql), offset)
    }

    #[test]
    fn sql_string_and_comment_are_recognized() {
        use SyntaxContext::{Code, Comment, String as S};
        // 字符串内 / 注释内 / 代码区
        assert_eq!(at("SELECT 'abc'", 8), S, "字符串内");
        assert_eq!(at("SELECT 'abc'", 12), Code, "字符串闭合后");
        assert_eq!(at("SELECT -- c\n1", 10), Comment, "行注释内");
        assert_eq!(at("SELECT /* c */ 1", 9), Comment, "块注释内");
        assert_eq!(at("SELECT /* c */ 1", 14), Code, "块注释闭合后");
        assert_eq!(at("SELECT 1", 3), Code, "代码区");
    }

    #[test]
    fn sql_multibyte_and_unterminated_are_safe() {
        use SyntaxContext::{Code, String as S};
        assert_eq!(at("SELECT '中文'", 9), S, "中文字符串内（字节偏移）");
        // 词法失败（未闭合字符串）：不猜内容，整体 Code
        assert_eq!(at("SELECT 'abc", 11), Code);
    }
}
