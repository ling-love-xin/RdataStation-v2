//! 语义着色（A4）：把词法区间喂给编辑内核
//!
//! ## 为什么走「语义 token」而不是注册 grammar
//!
//! 组件库**带** SQL grammar（`tree-sitter-sequel`，见 `gpui-component` 的 `tree-sitter-sql`
//! feature），但它是**单方言**的：本仓要同时面对 MySQL / PG / SQLite / DuckDB 的语法差异，
//! 且表名 / 列名补全本来就要靠 schema 目录。SQL 词法区间是现成的（`engine::sql::highlight`，
//! sqlglot tokenizer 驱动），**跟补全共用同一份词法**——所以走语义 token，而不是再引一个
//! grammar（两套词法必然漂）。gpui-base 正好提供了外部语义着色接口
//! `DocumentRangeSemanticTokensProvider`：本模块只声明**区间 + 类型名**，
//! **颜色由活跃 `HighlightTheme` 按名字解析**——主题切换自动重着色，视图层不出现任何色值
//! （零裸色值约束由此天然满足）。
//!
//! ## 脚本类型高亮（遗留 #14，2026-10-15）
//!
//! 通用模式的脚本文件（Python / R / Rust / Markdown / JSON / Log）此前只有**文件类型门控**
//! （状态栏「类型」段），没有上色本体。现在词法器在 `engine::script`（自研轻量、纯函数，
//! 零新依赖——路线选择见该模块头与调研记录），本模块按文档类型**分派**：
//!
//! - 文档扩展名命中 `mode::SCRIPT_FILE_TYPES` → 用对应语言词法器（CSV 例外：走**表格展示**，
//!   不上文本色，分派为 `None`）；
//! - 未识别扩展名 / 未命名文档 → 维持 SQL 着色（通用模式里写得最多的就是 SQL 脚本；
//!   UIST 反馈），与历史行为一致；
//! - 分析模式仍逐单元语言，整篇不上色。
//!
//! 图例词汇随脚本需求扩到 14 项（追加 constant/boolean/title/tag/property——主题资产
//! `rds-theme.json` 的 `highlight.syntax` 本来就声明了这 5 项，契约用例自动兼容）。
//!
//! ## 映射表（`TokenClass` → 主题词汇）
//!
//! | TokenClass | 主题词汇 | 说明 |
//! | --- | --- | --- |
//! | Keyword | `keyword` | |
//! | Type | `type` | Rust 生命周期 `'a` 也归此类 |
//! | Function | `function` | 由「标识符后跟 `(`」启发式判定；Rust 宏 `println!` |
//! | String | `string` | |
//! | Number | `number` | Log 时间戳也归此类 |
//! | Comment | `comment` | Log DEBUG/TRACE 级别也归此类 |
//! | Operator | `operator` | |
//! | Punctuation | `punctuation` | |
//! | `Parameter` | `variable` | 主题词汇里没有 `parameter`，退到最接近的 `variable` |
//! | Variable | `variable` | Log 的 `key=value` 键 |
//! | Constant | `constant` | Log 错误级别 / JSON `null` / R 常量 |
//! | Boolean | `boolean` | JSON `true/false`、Rust 布尔字面量 |
//! | Title | `title` | Markdown ATX 标题 |
//! | Tag | `tag` | Markdown 围栏 / 引用 / 列表标记、Rust 属性 |
//! | Property | `property` | JSON 对象键 |
//! | `Identifier` | **不上色** | 与主流编辑器一致：用正文色，避免整篇花绿 |
//!
//! **图例里的名字必须在主题资产里有对应项**（`assets/themes/rds-theme.json` 的 `highlight.syntax`，
//! 明暗两套都要有）：查不到时内核**静默跳过**该 token（不报错、不警告），看到的现象是
//! “这几个词就是没颜色”，而原因在资产里而不是代码里。两边不漂由契约用例盯着
//! （`workbench/tests/ui_contract.rs` 的图例↔资产那条）。
//!
//! ## 边界
//!
//! - **大文件不上色**（`HIGHLIGHT_MAX_BYTES`）：词法扫描是 O(n)，超过阈值直接返回空 token
//!   （大文件档位属 A13，这里只做降级，不拖慢每帧）。
//! - 词法失败（未闭合字符串等）→ 词法器按「到行尾」收尾 → 不影响编辑。
//! - 脚本词法器是**轻量词法层**：不做上下文语义（类型 / 变量作用域），覆盖有限、可后续迭代。

use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{
    DocumentRangeSemanticTokensProvider, EditorState, Rope, RopeExt as _,
};
use gpui_kit::*;
use lsp_types::{SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend};

use engine::sql::{HighlightSpan, TokenClass, highlight_spans};

use crate::mode::ScriptFileType;
use crate::model::EditorMode;

/// 超过此字节数不做着色（大文件档位）
const HIGHLIGHT_MAX_BYTES: usize = 1_000_000;

/// 图例里的 token 类型名（顺序即 `token_type` 的取值下标）
///
/// **前 9 项是 SQL 词汇，下标必须保持稳定**（历史数据 / delta 编码依赖顺序）；
/// 后 5 项为脚本词法器追加（constant/boolean/title/tag/property），名字必须落在活跃
/// `HighlightTheme` 的词汇里，否则编辑器会跳过该 token（不报错）——资产侧的对应用项
/// 与回归见模块头的「图例里的名字必须在主题资产里有对应项」一段。
pub const TOKEN_NAMES: [&str; 14] = [
    "keyword",
    "type",
    "function",
    "string",
    "number",
    "comment",
    "operator",
    "punctuation",
    "variable",
    "constant",
    "boolean",
    "title",
    "tag",
    "property",
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
        TokenClass::Variable => Some("variable"),
        TokenClass::Constant => Some("constant"),
        TokenClass::Boolean => Some("boolean"),
        TokenClass::Title => Some("title"),
        TokenClass::Tag => Some("tag"),
        TokenClass::Property => Some("property"),
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

/// 该模式是否启用语义着色
///
/// 通用与 SQL 两个**单缓冲**模式都上色：通用模式里写的就是 SQL 脚本 / 片段（`未命名.txt`
/// 也常是 SQL），用户在 UIST 里明确要求通用模式一并着色。通用模式内部再按文件类型分派
/// （见 [`kind_for_document`]）。分析模式是逐单元语言（SQL / Markdown / 后续 python），
/// 留 1c 在单元层决定；现在不给整篇笔记上色（与 `view::fold::is_enabled` 同口径，
/// 折叠已对通用 + SQL 两模式开启）。
pub fn is_enabled(mode: EditorMode) -> bool {
    matches!(mode, EditorMode::Sql | EditorMode::General)
}

/// 文档高亮类型：SQL 词法 / 通用模式的脚本类型 / 不上色
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    /// SQL 模式，或通用模式的**未识别扩展名**文档（维持 SQL 着色）
    Sql,
    /// 通用模式的脚本文件（Python / R / Rust / Markdown / JSON / Log）
    Script(ScriptFileType),
    /// 不上色：分析模式 / CSV 等走表格展示的数据文件
    None,
}

/// 安装时从文档路径 + 模式计算高亮类型（纯函数）
///
/// CSV 走表格展示（见遗留 #14 决策），不上文本色；其余脚本类型按词法器分派；
/// 未识别扩展名（`.txt` / 未命名）保持 SQL 着色——与「通用模式当 SQL 写」的历史行为一致。
pub fn kind_for_document(path: Option<&Path>, mode: EditorMode) -> HighlightKind {
    if !is_enabled(mode) {
        return HighlightKind::None;
    }
    match crate::mode::script_file_type(path.unwrap_or(Path::new(""))) {
        Some(ScriptFileType::Csv) => HighlightKind::None,
        Some(kind) => HighlightKind::Script(kind),
        None => HighlightKind::Sql,
    }
}

/// 按语言取词法区间；CSV / `None` 返回空（不上色）
fn spans_for(text: &str, kind: HighlightKind) -> Vec<HighlightSpan> {
    match kind {
        HighlightKind::Sql => highlight_spans(text),
        HighlightKind::Script(kind) => script_spans(kind, text),
        HighlightKind::None => Vec::new(),
    }
}

/// 脚本类型 → 词法器（复用 `engine::script` 各语言纯函数）
fn script_spans(kind: ScriptFileType, text: &str) -> Vec<HighlightSpan> {
    use engine::script::{json_spans, log_spans, markdown_spans, python_spans, r_spans, rust_spans};
    match kind {
        ScriptFileType::Python => python_spans(text),
        ScriptFileType::R => r_spans(text),
        ScriptFileType::Rust => rust_spans(text),
        ScriptFileType::Markdown => markdown_spans(text),
        ScriptFileType::Json => json_spans(text),
        ScriptFileType::Log => log_spans(text),
        // CSV 走表格展示，不上文本色（`kind_for_document` 已把 CSV 分派为 `None`，
        // 这里兜底返回空，双保险）
        ScriptFileType::Csv => Vec::new(),
    }
}

/// 语义着色 provider（无状态：随时可建、可共享；按 `kind` 分派词法器）
pub struct ModeSemanticTokensProvider {
    kind: HighlightKind,
}

impl ModeSemanticTokensProvider {
    pub fn new(kind: HighlightKind) -> Self {
        Self { kind }
    }
}

impl DocumentRangeSemanticTokensProvider for ModeSemanticTokensProvider {
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
        Task::ready(Ok(tokens_for(text, &range, self.kind)))
    }
}

/// 把 provider 装到某个文档的编辑内核上，并**立刻请求一次刷新**（`InputBaseState::refresh`）。
///
/// 每个文档一份 `EditorState` → 一份 provider：着色只依赖文本与文档类型，无跨文档状态。
/// `kind = None`（CSV / 分析模式）时摘除 provider。
///
/// **装完必须刷一次**：内核只在**文本变更**时拉取语义 token（`editor/mod.rs` 的
/// `refresh_language_features` → `Lsp::update` → `update_semantic_tokens`）。文档刚打开、
/// 或刚切到着色模式时都没有文本变更，不刷就会一直「不上色」，直到用户敲下第一个键。
/// `refresh` 把 `_pending_update` 置真，下一帧 `Render::render` 里跑一遍
/// `refresh_language_features` → `update_semantic_tokens` 取到 provider 并取数。
/// （这正是 gpui-base 在 `InputBaseState::refresh` 文档里给的用法：设完 LSP provider 就 `refresh`。）
pub fn install(state: &mut EditorState, cx: &mut Context<EditorState>, kind: HighlightKind) {
    state.lsp_mut().semantic_tokens_provider = match kind {
        HighlightKind::None => None,
        _ => Some(Rc::new(ModeSemanticTokensProvider::new(kind))),
    };
    if !matches!(kind, HighlightKind::None) {
        state.refresh(cx);
    }
}

/// 纯函数：文本 + 可视区间 → LSP 语义 token（delta 编码）
///
/// 三条约定来自 LSP 规范，越界会被编辑器静默丢弃：
/// 1. token **必须单行** → 跨行区间按行切分；
/// 2. `delta_line` / `delta_start` 相对**上一个 token**（同一行时 `delta_start` 为列差）；
/// 3. 列以**字符**计（与 `RopeExt::offset_to_position` 一致，编辑器用的是同一个换算）。
pub fn tokens_for(text: &Rope, range: &Range<usize>, kind: HighlightKind) -> SemanticTokens {
    let mut data: Vec<SemanticToken> = Vec::new();
    if text.len() > HIGHLIGHT_MAX_BYTES {
        return SemanticTokens {
            result_id: None,
            data,
        };
    }

    let source = text.to_string();
    let spans = spans_for(&source, kind);
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

    use super::{
        HIGHLIGHT_MAX_BYTES, HighlightKind, ModeSemanticTokensProvider, TOKEN_NAMES, is_enabled,
        kind_for_document, token_name, tokens_for,
    };

    fn tokens(sql: &str, kind: HighlightKind) -> Vec<lsp_types::SemanticToken> {
        let rope = Rope::from(sql);
        tokens_for(&rope, &(0..sql.len()), kind).data
    }

    fn decoded(sql: &str, kind: HighlightKind) -> Vec<(u32, u32, u32, &'static str)> {
        let mut line = 0u32;
        let mut start = 0u32;
        tokens(sql, kind)
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

    fn path(name: &str) -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from(name))
    }

    #[test]
    fn sql_colouring_covers_the_single_buffer_modes_only() {
        use crate::model::EditorMode;
        assert!(is_enabled(EditorMode::Sql), "SQL 模式必须着色");
        assert!(
            is_enabled(EditorMode::General),
            "通用模式也要着色（通用里写得最多的就是 SQL；UIST 反馈）"
        );
        assert!(
            !is_enabled(EditorMode::Analysis),
            "分析模式是逐单元语言，整篇笔记不上 SQL 色"
        );
    }

    #[test]
    fn legend_covers_every_theme_token_we_emit() {
        let provider = ModeSemanticTokensProvider::new(HighlightKind::Sql);
        let legend = provider.legend();
        assert_eq!(legend.token_types.len(), TOKEN_NAMES.len());
        assert!(legend.token_modifiers.is_empty());
        // 主题认识的词汇：14 项全部在 `rds-theme.json` 的 `highlight.syntax` 里
        //（ui_contract 的图例↔资产契约单独盯着）
        for name in TOKEN_NAMES {
            assert!(
                legend.token_types.iter().any(|t| t.as_str() == name),
                "图例缺少 {name}"
            );
        }
    }

    #[test]
    fn identifiers_are_not_coloured() {
        assert_eq!(token_name(TokenClass::Identifier), None);
        // 表名 / 列名都不出现 token（正文色）
        let decoded = decoded("SELECT id FROM orders", HighlightKind::Sql);
        let names: Vec<&str> = decoded.iter().map(|(_, _, _, name)| *name).collect();
        assert_eq!(names, vec!["keyword", "keyword"]);
    }

    #[test]
    fn simple_statement_is_delta_encoded() {
        // SELECT 1 → 0 行 0 列 长 6 的 keyword；同一行第 7 列长 1 的 number
        assert_eq!(
            decoded("SELECT 1", HighlightKind::Sql),
            vec![(0, 0, 6, "keyword"), (0, 7, 1, "number")]
        );
    }

    #[test]
    fn string_includes_quotes_and_cjk_counts_by_chars() {
        // 列以字符计：`'中文'` 从第 7 列开始，长 4（引号 + 2 汉字 + 引号）
        assert_eq!(
            decoded("SELECT '中文' FROM t", HighlightKind::Sql),
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
        let decoded = decoded("/* 头\n   注 */SELECT 1", HighlightKind::Sql);
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
        let names: Vec<&str> = decoded("CREATE TABLE t (id INT, n count(*))", HighlightKind::Sql)
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
        let tail = tokens_for(&rope, &(9..sql.len()), HighlightKind::Sql);
        assert!(!tail.data.is_empty());
        assert_eq!(TOKEN_NAMES[tail.data[0].token_type as usize], "keyword");
        assert_eq!(tail.data[0].delta_start, 9, "首个 token 仍在第 9 列");
    }

    #[test]
    fn large_text_degrades_to_no_tokens() {
        let big = "SELECT 1;".repeat(HIGHLIGHT_MAX_BYTES / 9 + 2);
        let rope = Rope::from(big.as_str());
        assert!(
            tokens_for(&rope, &(0..rope.len()), HighlightKind::Sql)
                .data
                .is_empty(),
            "大文件应降级为不上色，而不是逐帧全量扫描"
        );
    }

    #[test]
    fn unterminated_literal_keeps_editor_usable() {
        // 词法失败 → 空 token（不 panic、不影响编辑）
        let rope = Rope::from("SELECT 'abc");
        let _ = tokens_for(&rope, &(0..rope.len()), HighlightKind::Sql).data;
    }

    // ---- 脚本类型分派（遗留 #14）----

    #[test]
    fn kind_for_document_dispatches_by_extension() {
        use crate::model::EditorMode;
        // 脚本类型 → Script
        assert_eq!(
            kind_for_document(path("a.py").as_deref(), EditorMode::General),
            HighlightKind::Script(crate::mode::ScriptFileType::Python)
        );
        assert_eq!(
            kind_for_document(path("b.rs").as_deref(), EditorMode::General),
            HighlightKind::Script(crate::mode::ScriptFileType::Rust)
        );
        // CSV → 不上色（走表格展示）
        assert_eq!(
            kind_for_document(path("c.csv").as_deref(), EditorMode::General),
            HighlightKind::None
        );
        // 未识别扩展名 → SQL（通用模式写 SQL 的现状）
        assert_eq!(
            kind_for_document(path("d.txt").as_deref(), EditorMode::General),
            HighlightKind::Sql
        );
        // SQL / 分析模式
        assert_eq!(
            kind_for_document(path("e.sql").as_deref(), EditorMode::Sql),
            HighlightKind::Sql
        );
        assert_eq!(
            kind_for_document(path("f.note").as_deref(), EditorMode::Analysis),
            HighlightKind::None
        );
    }

    #[test]
    fn python_script_is_highlighted_with_python_lexer() {
        let got = decoded("# 注释\ndef f(x):\n    return x", HighlightKind::Script(
            crate::mode::ScriptFileType::Python,
        ));
        let names: Vec<&str> = got.iter().map(|(_, _, _, name)| *name).collect();
        assert!(names.contains(&"comment"), "注释应上色：{got:?}");
        assert!(names.contains(&"keyword"), "def/return 应上色：{got:?}");
        assert!(names.contains(&"function"), "f( 应映射 function：{got:?}");
    }

    #[test]
    fn rust_script_is_highlighted_with_rust_lexer() {
        let got = decoded("let x = 42; // 数字", HighlightKind::Script(
            crate::mode::ScriptFileType::Rust,
        ));
        let names: Vec<&str> = got.iter().map(|(_, _, _, name)| *name).collect();
        assert!(names.contains(&"keyword"), "let 应上色：{got:?}");
        assert!(names.contains(&"number"), "42 应上色：{got:?}");
        assert!(names.contains(&"comment"), "注释应上色：{got:?}");
    }

    #[test]
    fn csv_and_none_kinds_emit_no_tokens() {
        let csv = "a,b,c\n1,2,3";
        assert!(
            tokens(csv, HighlightKind::None).is_empty(),
            "None 类型不应产出 token"
        );
        assert!(
            tokens(csv, HighlightKind::Script(crate::mode::ScriptFileType::Csv)).is_empty(),
            "CSV 走表格展示，不上文本色"
        );
    }

    #[test]
    fn new_theme_words_map_to_token_names() {
        for (class, name) in [
            (TokenClass::Constant, "constant"),
            (TokenClass::Boolean, "boolean"),
            (TokenClass::Title, "title"),
            (TokenClass::Tag, "tag"),
            (TokenClass::Property, "property"),
        ] {
            assert_eq!(token_name(class), Some(name), "{class:?} 应映射 {name}");
        }
    }
}
