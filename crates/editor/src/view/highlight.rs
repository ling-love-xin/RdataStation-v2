//! SQL 语义着色（A4）：把 `engine::sql::highlight` 的词法区间喂给编辑内核
//!
//! ## 为什么走「语义 token」而不是注册 grammar
//!
//! 组件库自带的高亮器**不含 SQL grammar**（只有 go / js / json / rust / markdown 等），而我们的
//! 词法区间是现成的（`engine::sql::highlight`，sqlglot tokenizer 驱动，见架构 D12）。gpui-base
//! 正好提供了外部语义着色接口 `DocumentRangeSemanticTokensProvider`：本模块只声明**区间 + 类型名**，
//! **颜色由活跃 `HighlightTheme` 按名字解析**——主题切换自动重着色，视图层不出现任何色值
//! （零裸色值约束由此天然满足）。
//!
//! ## 映射表（`TokenClass` → 主题词汇）
//!
//! | TokenClass | 主题词汇 | 说明 |
//! | --- | --- | --- |
//! | Keyword | `keyword` | |
//! | Type | `type` | |
//! | Function | `function` | 由「标识符后跟 `(`」启发式判定 |
//! | String | `string` | |
//! | Number | `number` | |
//! | Comment | `comment` | |
//! | Operator | `operator` | |
//! | Punctuation | `punctuation` | |
//! | Parameter | `variable` | 主题词汇里没有 `parameter`，退到最接近的 `variable` |
//! | Identifier | **不上色** | 与主流 SQL 客户端一致：用正文色，避免整篇花绿 |
//!
//! ## 边界
//!
//! - **大文件不上色**（`HIGHLIGHT_MAX_BYTES`）：词法扫描是 O(n)，超过阈值直接返回空 token
//!   （大文件档位属 A13，这里只做降级，不拖慢每帧）。
//! - 词法失败（未闭合字符串等）→ `highlight_spans` 返回空 → 无着色，不影响编辑。

use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{
    DocumentRangeSemanticTokensProvider, EditorState, Rope, RopeExt as _,
};
use gpui_kit::*;
use lsp_types::{SemanticToken, SemanticTokens, SemanticTokensLegend, SemanticTokenType};

use engine::sql::{HighlightSpan, TokenClass, highlight_spans};

use crate::model::EditorMode;

/// 超过此字节数不做着色（大文件档位）
const HIGHLIGHT_MAX_BYTES: usize = 1_000_000;

/// 图例里的 token 类型名（顺序即 `token_type` 的取值下标）
///
/// 名字必须落在活跃 `HighlightTheme` 的词汇里，否则编辑器会跳过该 token（不报错）。
const TOKEN_NAMES: [&str; 9] = [
    "keyword",
    "type",
    "function",
    "string",
    "number",
    "comment",
    "operator",
    "punctuation",
    "variable",
];

/// 词法类别 → 主题词汇；`None` 表示**不上色**（Identifier）
fn token_name(class: TokenClass) -> Option<&'static str> {
    match class {
        TokenClass::Keyword => Some("keyword"),
        TokenClass::Type => Some("type"),
        TokenClass::Function => Some("function"),
        TokenClass::String => Some("string"),
        TokenClass::Number => Some("number"),
        TokenClass::Comment => Some("comment"),
        TokenClass::Operator => Some("operator"),
        TokenClass::Punctuation => Some("punctuation"),
        TokenClass::Parameter => Some("variable"),
        TokenClass::Identifier => None,
    }
}

/// 名字在 `TOKEN_NAMES` 里的下标（= LSP 的 `token_type`）
fn token_type_index(name: &str) -> Option<u32> {
    TOKEN_NAMES
        .iter()
        .position(|candidate| *candidate == name)
        .map(|index| index as u32)
}

/// 该模式是否启用 SQL 语义着色
///
/// 文本模式是**纯记事本**：不解析、不上色（能力表里连语言服务都没有）。
/// 分析模式是逐单元语言（SQL / Markdown / 后续 python），留 1c 在单元层决定，
/// 现在不给整篇笔记上 SQL 色——那会把 Markdown 单元也按 SQL 着色。
pub fn is_enabled(mode: EditorMode) -> bool {
    matches!(mode, EditorMode::Sql)
}

/// SQL 语义着色 provider（无状态：随时可建、可共享）
pub struct SqlSemanticTokensProvider;

impl DocumentRangeSemanticTokensProvider for SqlSemanticTokensProvider {
    fn legend(&self) -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: TOKEN_NAMES
                .iter()
                .map(|name| SemanticTokenType::new(name))
                .collect(),
            // 修饰符（readonly / deprecated…）暂不用：主题侧也未映射
            token_modifiers: Vec::new(),
        }
    }

    fn semantic_tokens(
        &self,
        text: &Rope,
        range: Range<usize>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<SemanticTokens>> {
        // 词法扫描是纯计算、毫秒级：直接就绪返回，不必上后台（编辑器会缓存结果）
        Task::ready(Ok(tokens_for(text, &range)))
    }
}

/// 把 provider 装到某个文档的编辑内核上（`EditorState::lsp_mut`）
///
/// 每个文档一份 `EditorState` → 一份 provider：着色只依赖文本，无跨文档状态。
pub fn install(state: &mut EditorState) {
    state.lsp_mut().semantic_tokens_provider = Some(Rc::new(SqlSemanticTokensProvider));
}

/// 纯函数：文本 + 可视区间 → LSP 语义 token（delta 编码）
///
/// 三条约定来自 LSP 规范，越界会被编辑器静默丢弃：
/// 1. token **必须单行** → 跨行区间按行切分；
/// 2. `delta_line` / `delta_start` 相对**上一个 token**（同一行时 `delta_start` 为列差）；
/// 3. 列以**字符**计（与 `RopeExt::offset_to_position` 一致，编辑器用的是同一个换算）。
pub fn tokens_for(text: &Rope, range: &Range<usize>) -> SemanticTokens {
    let mut data: Vec<SemanticToken> = Vec::new();
    if text.len() > HIGHLIGHT_MAX_BYTES {
        return SemanticTokens {
            result_id: None,
            data,
        };
    }

    let source = text.to_string();
    let spans = highlight_spans(&source);
    let (mut prev_line, mut prev_start) = (0u32, 0u32);

    for span in spans {
        let Some(kind) = token_name(span.class).and_then(token_type_index) else {
            continue;
        };
        // 只产出与请求区间相交的 token（编辑器按可视范围分批取）
        if span.end <= range.start || span.start >= range.end || span.start >= text.len() {
            continue;
        }
        for (start, end) in split_by_line(text, &source, &span) {
            let from = text.offset_to_position(start);
            let to = text.offset_to_position(end);

            let (delta_line, delta_start) = if data.is_empty() {
                (from.line, from.character)
            } else if from.line == prev_line {
                (0, from.character.saturating_sub(prev_start))
            } else {
                (from.line - prev_line, from.character)
            };

            data.push(SemanticToken {
                delta_line,
                delta_start,
                length: to.character.saturating_sub(from.character).max(1),
                token_type: kind,
                token_modifiers_bitset: 0,
            });

            prev_line = from.line;
            prev_start = from.character;
        }
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}

/// 把一个词法区间按行切成若干**单行**字节区间（区间内不含行尾换行）
///
/// 行号用 `RopeExt::offset_to_point`（字节列）取，行尾直接在源文本里找 `\n`：
/// 避免依赖 ropey 的 `len_lines` / 单位差异（ropey 各版本的 API 与“字符还是字节”并不一致）。
fn split_by_line(text: &Rope, source: &str, span: &HighlightSpan) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let start = span.start.min(source.len());
    let end = span.end.min(source.len());
    if start >= end {
        return parts;
    }

    let first_row = text.offset_to_point(start).row;
    let last_row = text.offset_to_point(end - 1).row;
    for row in first_row..=last_row {
        let line_start = text.line_start_offset(row).min(source.len());
        let line_end = match source[line_start..].find('\n') {
            Some(offset) => line_start + offset,
            None => source.len(),
        };
        let from = start.max(line_start);
        // 行尾换行（含 CRLF 的 `\r`）不算 token 内容
        let to = end.min(trim_cr(source, line_start, line_end));
        if from < to {
            parts.push((from, to));
        }
    }

    parts
}

/// 去掉行尾的 `\r`（CRLF 文件）
fn trim_cr(source: &str, from: usize, to: usize) -> usize {
    if to > from && source.as_bytes().get(to - 1) == Some(&b'\r') {
        to - 1
    } else {
        to
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**——`use super::*` 会把 gpui 的 `test` 宏带进来，
    // `#[test]` 因而解析到它自己（recursion limit reached）。依赖显式列举。
    use engine::sql::TokenClass;
    use gpui_kit::component::input::{DocumentRangeSemanticTokensProvider as _, Rope};
    use lsp_types::SemanticToken;

    use super::{HIGHLIGHT_MAX_BYTES, SqlSemanticTokensProvider, TOKEN_NAMES, token_name, tokens_for};

    fn tokens(sql: &str) -> Vec<SemanticToken> {
        let rope = Rope::from(sql);
        tokens_for(&rope, &(0..sql.len())).data
    }

    /// 把 delta 编码还原成 (行, 列, 长度, 类型名) 便于断言
    fn decoded(sql: &str) -> Vec<(u32, u32, u32, &'static str)> {
        let mut line = 0u32;
        let mut start = 0u32;
        tokens(sql)
            .into_iter()
            .map(|token| {
                if token.delta_line == 0 {
                    start += token.delta_start;
                } else {
                    line += token.delta_line;
                    start = token.delta_start;
                }
                (
                    line,
                    start,
                    token.length,
                    TOKEN_NAMES[token.token_type as usize],
                )
            })
            .collect()
    }

    #[test]
    fn legend_covers_every_theme_token_we_emit() {
        let provider = SqlSemanticTokensProvider;
        let legend = provider.legend();
        assert_eq!(legend.token_types.len(), TOKEN_NAMES.len());
        assert!(legend.token_modifiers.is_empty());
        // 主题认识的词汇：keyword / type / function / string / number / comment / operator / punctuation / variable
        for name in TOKEN_NAMES {
            assert!(
                legend
                    .token_types
                    .iter()
                    .any(|t| t.as_str() == name),
                "图例缺少 {name}"
            );
        }
    }

    #[test]
    fn identifiers_are_not_coloured() {
        assert_eq!(token_name(TokenClass::Identifier), None);
        // 表名 / 列名都不出现 token（正文色）
        let decoded = decoded("SELECT id FROM orders");
        let names: Vec<&str> = decoded.iter().map(|(_, _, _, name)| *name).collect();
        assert_eq!(names, vec!["keyword", "keyword"]);
    }

    #[test]
    fn simple_statement_is_delta_encoded() {
        // SELECT 1 → 0 行 0 列 长 6 的 keyword；同一行第 7 列长 1 的 number
        assert_eq!(
            decoded("SELECT 1"),
            vec![(0, 0, 6, "keyword"), (0, 7, 1, "number")]
        );
    }

    #[test]
    fn string_includes_quotes_and_cjk_counts_by_chars() {
        // 列以字符计：`'中文'` 从第 7 列开始，长 4（引号 + 2 汉字 + 引号）
        assert_eq!(
            decoded("SELECT '中文' FROM t"),
            vec![
                (0, 0, 6, "keyword"),
                (0, 7, 4, "string"),
                (0, 12, 4, "keyword"),
            ]
        );
    }

    #[test]
    fn multiline_comment_is_split_per_line() {
        // 跨行区间必须按行切开（LSP 的 token 单行）；行尾换行不算 token 内容
        let decoded = decoded("/* 头\n   注 */SELECT 1");
        assert_eq!(
            decoded,
            vec![
                (0, 0, 4, "comment"), // `/* 头` = 4 个字符
                (1, 0, 7, "comment"), // `   注 */`
                (1, 7, 6, "keyword"),
                (1, 14, 1, "number"),
            ]
        );
    }

    #[test]
    fn types_functions_and_punctuation_map_to_theme_names() {
        let names: Vec<&str> = decoded("CREATE TABLE t (id INT, n count(*))")
            .into_iter()
            .map(|(_, _, _, name)| name)
            .collect();
        assert!(names.contains(&"type"), "INT 应映射为 type：{names:?}");
        assert!(names.contains(&"function"), "count( 应映射为 function");
        assert!(names.contains(&"punctuation"));
    }

    #[test]
    fn tokens_outside_requested_range_are_dropped() {
        let sql = "SELECT 1 FROM t";
        let rope = Rope::from(sql);
        // 只要 `FROM` 之后的区间：`FROM` 起于第 9 字节
        let tail = tokens_for(&rope, &(9..sql.len()));
        assert!(!tail.data.is_empty());
        assert_eq!(TOKEN_NAMES[tail.data[0].token_type as usize], "keyword");
        assert_eq!(tail.data[0].delta_start, 9, "首个 token 仍在第 9 列");
    }

    #[test]
    fn large_text_degrades_to_no_tokens() {
        let big = "SELECT 1;".repeat(HIGHLIGHT_MAX_BYTES / 9 + 2);
        let rope = Rope::from(big.as_str());
        assert!(
            tokens_for(&rope, &(0..rope.len())).data.is_empty(),
            "大文件应降级为不上色，而不是逐帧全量扫描"
        );
    }

    #[test]
    fn unterminated_literal_keeps_editor_usable() {
        // 词法失败 → 空 token（不 panic、不影响编辑）
        let rope = Rope::from("SELECT 'abc");
        let tokens = tokens_for(&rope, &(0..rope.len()));
        let _ = tokens.data;
    }
}
