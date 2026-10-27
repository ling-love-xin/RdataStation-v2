//! 语言编辑规则定制（B4）：为 SQL / 脚本语言注册编辑配置
//!
//! ## 为什么需要它
//!
//! gpui-component 的 `init` 已 `set_language_provider(ComponentLanguages)`：text /
//! json / python 有专属 config，其余语言（含 SQL）落到 **fallback 默认**
//! （`()[]{}` + 引号配对，`not_in([String, Comment])`）。问题是：
//!
//! 1. **SQL 没有专属 config**：输入 `(` 会配对 `)`（fallback 已给），但方言级
//!    auto_close_before、缩进规则都是通用默认；
//! 2. **`not_in` 不生效**：syntax context 只来自全局 `LanguageProvider` 的
//!    `syntax_context_provider(name)`，它查 tree-sitter grammar registry——本仓
//!    SQL 高亮走**自研词法**（`engine::sql::highlight`），不注册 grammar，所以
//!    SQL 的 `not_in([String, Comment])` 永不命中（`context_at` 恒为 `Code`）。
//!
//! ## 方案与取舍
//!
//! - **不替换全局 LanguageProvider**：`ComponentLanguages` 是 gpui-component 内部
//!   私有实现，替换会破坏 text / json / python 的既有默认。SQL 的 `not_in`
//!   在 **A 方案落地后命中**：vendor patch 给 gpui-component 加了
//!   `register_syntax_context_provider` 注册点（优先于 grammar 生效），SQL 挂
//!   自研词法判定（`lsp::sql_syntax_context::SqlSyntaxContext`，与高亮同源），
//!   字符串 / 注释内输入引号不再配对——首版「不命中」取舍已解除。
//! - **追加注册**：`set_language_config` 是**叠加**的（`Languages::configs` 按
//!   注册顺序倒序查找），不覆盖 provider，安全。SQL 用方言无关的通用编辑规则
//!   （SQL 括号语义各库一致：`()` 函数 / 分组，`[]` 数组 / 标识符，`{}` 仅少数
//!   方言），脚本语言保持 ComponentLanguages 默认（text / json / python 已有）。

use gpui_kit::component::input::{
    AutoClosingPair, BracketPair, SyntaxContext, register_syntax_context_provider,
    set_language_config,
};
// `LanguageConfig` 未从 `component::input` 重导出（只导出 AutoClosingPair / BracketPair
// / IndentationRules），走 gpui_kit::base（= gpui-base）全路径。
use gpui_kit::base::input::language_config::LanguageConfig;
use gpui_kit::App;

/// SQL 编辑规则（纯函数，测试可直接断言字段形状）
///
/// 括号配对：`()`（函数 / 分组）、`[]`（数组 / 标识符引用）、`{}`（部分方言）。
/// auto_close_before 用内核默认（`";:.,=}])>"`）——SQL 里这些字符后跟闭合括号
/// 是常见形态（`WHERE id = (…)`、`SELECT (…)`）。
pub fn sql_language_config() -> LanguageConfig {
    LanguageConfig::default()
        .brackets([
            BracketPair::new("(", ")"),
            BracketPair::new("[", "]"),
            BracketPair::new("{", "}"),
        ])
        .auto_closing_pairs([
            AutoClosingPair::new("(", ")")
                .not_in([SyntaxContext::String, SyntaxContext::Comment]),
            AutoClosingPair::new("[", "]")
                .not_in([SyntaxContext::String, SyntaxContext::Comment]),
            AutoClosingPair::new("{", "}")
                .not_in([SyntaxContext::String, SyntaxContext::Comment]),
            // SQL 单引号是字符串字面量定界符：配对后 `''` 转义写法仍可手打。
            // 双引号在 PG / DuckDB 是标识符引用（`"select"` 列名）。
            AutoClosingPair::new("'", "'")
                .not_in([SyntaxContext::String, SyntaxContext::Comment]),
            AutoClosingPair::new("\"", "\"")
                .not_in([SyntaxContext::String, SyntaxContext::Comment]),
        ])
}

/// 安装 SQL 编辑规则（应用启动 / 面板初始化时调用一次；幂等——重复注册同键覆盖）。
///
/// 分析模式单元的 SQL 单元用同一个键（`cell.language.as_key()` → "sql"），
/// 与主编辑器共用规则，无需单独注册。
pub fn install(cx: &mut App) {
    set_language_config("sql", sql_language_config(), cx);
    // A 方案：SQL 语法上下文挂自研 provider（vendor patch 注册点）。
    // 工厂每次现造（provider 无状态，内核会缓存）；优先于 tree-sitter grammar
    // 生效——字符串 / 注释判定与高亮同源（同一 sqlglot Tokenizer）。
    register_syntax_context_provider(
        "sql",
        std::sync::Arc::new(|| {
            Some(
                std::rc::Rc::new(crate::lsp::sql_syntax_context::SqlSyntaxContext)
                    as std::rc::Rc<dyn gpui_kit::base::input::SyntaxContextProvider>,
            )
        }),
    );
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入
    use super::sql_language_config;

    #[test]
    fn sql_config_has_three_bracket_pairs_and_quotes() {
        let config = sql_language_config();
        assert_eq!(config.brackets.len(), 3);
        let pairs = config.auto_closing_pairs.expect("配置了配对");
        // 括号 3 对 + 单引号 + 双引号 = 5 对
        assert_eq!(pairs.len(), 5, "{pairs:?}");
        // 每对都声明了 not_in（字符串 / 注释内不配对——即便当前语法上下文
        // provider 不命中，声明保持正确，装上 grammar 即生效）
        assert!(pairs.iter().all(|p| !p.not_in.is_empty()));
        // 单引号对在列表里（SQL 字符串定界符）
        assert!(pairs.iter().any(|p| p.open.as_ref() == "'"));
    }

    #[test]
    fn sql_config_keeps_default_auto_close_before() {
        // auto_close_before 不覆盖 → 用内核默认（`";:.,=}])>"`，SQL 常见形态）
        let config = sql_language_config();
        assert_eq!(config.auto_close_before.as_ref(), ";:.,=}])>");
    }
}
