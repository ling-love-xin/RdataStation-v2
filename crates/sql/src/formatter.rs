//! SQL 格式化
//!
//! ## 分工：语义走 sqlglot，排版走 lax
//!
//! 这是 `crates/sql` 作为 **SQL 语言服务唯一接入点** 的排版半边：
//!
//! - **语义**（解析 / 切分 / 高亮 / 关键字判定 / 转译 / 方言体检）→ `sqlglot-rust`；
//! - **排版**（空白：换行、缩进、行宽、子句布局、逗号位置）→ `lax-sql`（dprint-core 的
//!   lossless SQL formatter，`cargo` 里的 crate 名是 `lax-sql`，仓库名是 `bartlomieju/lax`）。
//!
//! 为什么不继续用 `sqlglot-rust::generate_pretty`：
//!
//! - `generate_pretty` 是**生成器**（AST → 文本），不是格式化器：它只 emit 结构，丢掉
//!   "用户怎么写" 的一切（行内 / 尾随注释的**位置**、原始换行意图），并且**解析失败就整条
//!   不格式化**。实测台账见 `crates/sql/tests/sqlglot_capabilities.rs` 的探针 9：
//!   行内块注释、尾随行注释、关键字之间的注释、语句末尾注释**全部解析失败**。
//! - lax 是 **lossless** 的：只调整空白，**永不拒绝输入**。这直接解锁了本项目最需要的
//!   能力——**选区片段格式化**：用户选中 `where x = 1 AND b = 2` 这种半截片段，sqlglot
//!   解析不了（`parse_statements_with_comments` 返回 `Err`），旧实现只能原样返回（按了没
//!   反应）；lax 照样能排版。
//!
//! ## 关键取舍
//!
//! - **不按方言排版**：lax 的 tokenizer 方言无关（`Tokenizer::with_comments(sql)` 也不接受
//!   方言）。排版是纯空白决策，与 `LIMIT` / `TOP` 这类方言语义无关。`dialect` 参数保留在
//!   API 上（调用方无需改），但**不参与排版**。
//! - **关键字大小写不走 lax**：lax 的 `generation/keywords.rs` 只有 **85** 个通用词
//!   （`pub const KEYWORDS: &[&str]`，`is_keyword()` 硬引用，无注入点）；而 sqlglot 的
//!   `TokenType` 是 **200+ 变体的跨方言并集**。词表宽 2.4 倍 → 关键字判定沿用
//!   [`super::highlight`]（sqlglot 词法器），lax 侧永远传 `KeywordCase::Preserve`。
//!   这是"释放 lax 全部能力"的唯一例外，理由已在此记录。
//! - **不补尾分号**：分号归属原文（`split_statements` 把分号留在 gap 里），格式化不改写
//!   用户没写的东西。
//! - **注释保真兜底**：lax 承诺 lossless，但用户的注释丢不起。整篇输出做一次
//!   [`comment_mark_count`] 比对，不一致就**整篇回退原文**——防的是上游行为变化，不是
//!   当前行为（当前 lax 不会丢）。

use std::ops::Range;
use std::path::Path;

use dprint_core::configuration::NewLineKind as DprintNewLineKind;
use lax_sql::configuration::{
    ClauseStyle as LaxClauseStyle, Configuration as LaxConfig, KeywordCase as LaxKeywordCase,
};

use super::engine::SqlDialect;
use super::highlight::{HighlightSpan, TokenClass, highlight_spans};
use super::rewrite::rewrite_statements_with_gap_nl;

/// 传给 lax 的虚拟路径。
///
/// `lax_sql::format_text(path, ...)` 的 `path` 参数在实现里是 `_path`（**完全不使用**），
/// 只影响上游把它当作哪类文件；SQL 侧传一个稳定的假名即可，不引入任何文件系统交互。
const LAX_VIRTUAL_PATH: &str = "statement.sql";

/// 关键字大小写
///
/// 由 sqlglot 词法器（[`super::highlight`]）判定，**不**交给 lax 的 85 词表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordCase {
    /// 保持作者写法（默认）
    #[default]
    Preserve,
    /// 关键字统一大写（SELECT / FROM / WHERE）
    Uppercase,
    /// 关键字统一小写
    Lowercase,
}

/// 缩进字符（映射到 lax 的 `useTabs`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentChar {
    /// 空格缩进（默认；宽度由 [`FormatOptions::indent_width`] 决定）
    #[default]
    Space,
    /// Tab 缩进（每级一个 `\t`，`indent_width` 只影响对齐口径）
    Tab,
}

/// 逗号风格
//
/// lax 的 `ClauseStyle` 已决定逗号落在哪一侧（`Expanded` 为一行一项、逗号在行尾）；
/// 这里的前导迁移是**行级后处理**，只迁行尾逗号，口径见 [`migrate_trailing_commas`]。
/// 与 `ClauseStyle::Expanded` 叠加时以 lax 的布局为准，本项只负责把行尾逗号挪到行首。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommaStyle {
    /// 尾随逗号（默认，`col1,\n  col2`）
    #[default]
    Trailing,
    /// 前导逗号（`col1\n  , col2`；只迁移**行尾**逗号，行内 / 字符串 / 注释内逗号不动）
    Leading,
}

/// 子句布局（映射到 lax 的 `clauseStyle`）
///
/// - [`ClauseStyle::Fill`]：每个子句独占一行，子句内部按 `line_width` 打包，超出才折行。
/// - [`ClauseStyle::Expanded`]：子句关键字独占一行、子句体缩进其下、逗号分隔项**一行一项**
///   （DataGrip 的经典观感）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClauseStyle {
    /// 紧凑：子句内按行宽打包
    Fill,
    /// 展开：一行一项（**默认**，贴 DataGrip）
    #[default]
    Expanded,
}

/// 换行符（映射到 lax 的 `newLineKind`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NewLineKind {
    /// 跟随原文最后一处换行（默认：不改用户文档的换行风格）
    #[default]
    Auto,
    /// 强制 `\n`
    Lf,
    /// 强制 `\r\n`
    Crlf,
}

/// SQL 格式化选项
//
/// 字段与 lax `Configuration` 的映射关系写在 [`lax_config`]，这里只说明**语义**。
///
/// 注意：本结构**故意不实现 `Copy`** —— 两个 ignore 文本是 `String`。调用方在互斥分支里
/// 各 move 一次是允许的；需要复用请 `clone()`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    /// 关键字大小写（**sqlglot 词法器驱动**，不走 lax）
    pub keyword_case: KeywordCase,
    /// 缩进宽度（空格数 / Tab 模式的对齐口径）→ lax `indentWidth`
    pub indent_width: u8,
    /// 缩进字符 → lax `useTabs`
    pub indent_char: IndentChar,
    /// 逗号风格（后处理，非 lax 原生开关）
    pub comma_style: CommaStyle,
    /// 语句之间空行数（0 / 1 / 2；由 gap 规整负责，与 lax 不冲突）
    pub blank_lines_between: u8,
    /// 行宽（超过则折行）→ lax `lineWidth`
    ///
    /// 默认 120：与 dprint-core 的 `RECOMMENDED_GLOBAL_CONFIGURATION.line_width` 一致，
    /// 不擅自偏离上游默认。需要更紧凑（80/100）在设置里调。
    pub line_width: u16,
    /// 子句布局 → lax `clauseStyle`
    pub clause_style: ClauseStyle,
    /// 换行符 → lax `newLineKind`
    pub new_line_kind: NewLineKind,
    /// 节点级忽略标记 → lax `ignoreNodeCommentText`
    ///
    /// 命中该注释的语法节点**原样保留**（lax 不做排版），用于"这段别动我"的场景。
    pub ignore_node_comment_text: String,
    /// 文件级忽略标记 → lax `ignoreFileCommentText`
    ///
    /// 整篇命中则不排版（返回"无变化"）。默认 `dprint-ignore-file`。
    pub ignore_file_comment_text: String,
    /// 运算符两侧补空格（**后处理**，非 lax 能力；默认关）
    ///
    /// lax 只调结构性空白（换行 / 缩进），明确不规范化 token 间距，所以 `a=1` 会保持
    /// `a=1`。打开后由 [`normalize_operators`] 补空格：只补不删，一元 / 通配形态不动。
    pub normalize_operators: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::Preserve,
            indent_width: 2,
            indent_char: IndentChar::Space,
            comma_style: CommaStyle::Trailing,
            blank_lines_between: 1,
            line_width: 120,
            clause_style: ClauseStyle::Expanded,
            new_line_kind: NewLineKind::Auto,
            ignore_node_comment_text: "dprint-ignore".to_string(),
            ignore_file_comment_text: "dprint-ignore-file".to_string(),
            normalize_operators: false,
        }
    }
}

/// 格式化的结果（给界面用：**改了几条 / 有没有保留**都要说得清）
///
/// 与旧实现的语义差异（lax 永不拒绝输入，所以这两个计数的含义变了）：
///
/// - `formatted` = 经过排版的语句数；
/// - `kept_verbatim` = **逐字保留**的语句数。lax 侧只有两种情况会走到这里：
///   ① `format_text` 返回 `Err`（防御，当前不发生）；② 输出与输入相同
///   （`Ok(None)`）**不算**——那属于"没必要改"，仍是成功处理。
//   因此正常输入下 `kept_verbatim` 恒为 0。界面判断"有没有变化"请看
//   `FormatPlan::changes`（比对 `text` 与 `formatted`），不要依赖本字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatReport {
    /// 格式化后的整篇文本（未处理的语句**逐字保留**）
    pub text: String,
    /// 经过排版的语句数
    pub formatted: usize,
    /// 逐字保留的语句数（lax 下正常为 0）
    pub kept_verbatim: usize,
}

/// 逐条格式化（**语句为单位**）
//
/// 词法切分（[`rewrite_statements_with_gap`] → `split_statements`）拿到每条语句的字节区间，
/// 逐条排版再**回填原位**：区间外的内容（注释、空行、分隔符）由 gap 规整统一处理，
/// 不会因为某一条排版没写对而连带整篇。
//
/// `dialect` 保留在签名上（[`crate::SqlEngine::format_report`] 的既有契约），但**不参与排版**：
/// 排版是方言无关的空白决策。需要方言的是转译（`transpiler`），走另一条路。
pub fn format_with_report(sql: &str, dialect: SqlDialect, options: FormatOptions) -> FormatReport {
    // 只为了让"签名不改、语义变清楚"这件事在代码里可见：排版不看方言。
    let _ = dialect;
    // 换行符在这一层就解析成具体值：语句体（lax）与语句之间的 gap 必须同源，
    // 否则同一篇里混着 `\r\n` 和 `\n`。理由见 [`effective_newline`]。
    let newline = effective_newline(sql, options.new_line_kind);
    let blank_lines = options.blank_lines_between;
    let rewritten = rewrite_statements_with_gap_nl(sql, blank_lines, newline, |body| {
        format_single(body, &options, newline)
    });

    // 注释 / 逗号保真兜底：lax 承诺 lossless（只调空白），但用户的内容丢不起。
    // 不一致 → 整篇回退，宁可"没格式化"也不"改坏 SQL"。
    //
    // 为什么要连**逗号数量**一起看（2026-11-22 实测）：`select a -- 注释\n, b` 这类
    // **行尾注释紧邻逗号**的输入，lax 会把逗号排到注释文本之后（`a -- 注释,`），
    // 逗号被行注释吞掉 → 变成非法 SQL。只数 `--` / `/*` / `#` 抓不到这种损失。
    // 这一步防的是上游边角行为，不是常规路径（常规路径 lax 不丢东西）。
    let text = rewritten.text;
    if !same_content_signature(sql, &text) {
        return FormatReport {
            text: sql.to_string(),
            formatted: 0,
            kept_verbatim: 0,
        };
    }

    FormatReport {
        text,
        formatted: rewritten.rewritten,
        kept_verbatim: rewritten.kept_verbatim,
    }
}

/// 整篇生效的换行符
///
/// 为什么不用 lax 自己的 `NewLineKind::Auto`：那个 Auto 看的是**单条语句体最后的换行**，
/// 而语句体通常只有一行（没有换行）→ 恒选 LF；于是语句体用 `\n`、语句之间的 gap 用 `\r\n`
/// （或反之），同一篇文件里出现两种换行。所以 Auto 在**这一层**先按整篇多数换行解析成
/// 具体换行符，再同时交给 lax（语句体内）与 gap 规整（语句之间）。
fn effective_newline(sql: &str, kind: NewLineKind) -> &'static str {
    match kind {
        NewLineKind::Lf => "\n",
        NewLineKind::Crlf => "\r\n",
        NewLineKind::Auto => {
            let crlf = sql.matches("\r\n").count();
            let lf = sql.matches('\n').count();
            let bare_lf = lf.saturating_sub(crlf);
            // 裸 LF 不超过半数 → 判定为 CRLF 文档
            if crlf > 0 && bare_lf * 2 <= lf {
                "\r\n"
            } else {
                "\n"
            }
        }
    }
}

/// 单独排版一条语句（内部用；`None` = 逐字保留）
//
/// 与旧实现的根本区别：**不再解析**。`generate_pretty` 需要 AST，AST 需要完整语句；
/// lax 只需要 token 流，所以半截片段（`where x = 1 AND b = 2`）也能排。
fn format_single(body: &str, options: &FormatOptions, newline: &str) -> Option<String> {
    if body.trim().is_empty() {
        return None;
    }
    let config = lax_config(options, newline);
    // lax 0.3.0 的已知缺陷（2026-11-22 直连实测，绕开本项目管线可复现）：
    // **行注释**在 CRLF 输入下会把行尾 `\r` 留在注释 token 里，再追加配置换行，
    // 得到 `<注释>\r\r\n`；重复格式化时 `\r` 逐轮累加（`\r\r\n` → `\r\r\r\n` …）。
    // 块注释、无注释文本、LF 输入均不受影响。这里只摘掉「行注释之后紧跟的那个 `\r`」：
    // 字符串 / 块注释 / 正文一个字节都不动。
    let lax_body = strip_line_comment_cr(body);
    let formatted = match lax_sql::format_text(Path::new(LAX_VIRTUAL_PATH), &lax_body, &config) {
        Ok(Some(text)) => text,
        // `Ok(None)` = 输出与输入完全一致（例如整篇命中 `ignoreFileCommentText`）。
        // 这里映射成"原文"而不是"失败"：没有变化 ≠ 处理失败。
        Ok(None) => body.to_string(),
        // `Err` = lax 内部异常（当前不发生）。逐字保留并计入 kept_verbatim 是正确兜底。
        Err(_) => return None,
    };

    // 防御：lax 的 `format_text_inner` 在"解析不出任何语句"时直接返回空串
    // （`if statements.is_empty() { return Ok(String::new()) }`）。
    // 纯注释片段（`-- 说明` 单行）会命中这条分支 —— 绝不能把用户的注释清空。
    if formatted.trim().is_empty() && !body.trim().is_empty() {
        return Some(body.to_string());
    }

    let mut out = apply_options(&formatted, options);
    // 后处理会**改长度**（补空格、逗号迁移）。行一旦被撑过 `line_width`，lax 下一轮就会
    // 再折一次行 —— 那样「格式化一次」与「格式化两次」结果不同（真实迁移语料实测：
    // 85 个文件里有 2 个，`tokenize = 'trigram'` → `tokenize =\n  'trigram'`）。
    // 所以在后处理确实改动过文本时，再让 lax 按新宽度排一次，迭代到不动点（最多 2 轮）。
    //
    // **只在 `trailing` 下做**：`leading` 与 lax 折行方向相反（lax 会把行首逗号拉回行尾），
    // 迭代只会让两轮来回摆（实测 85 个文件里振荡面从 1 个扩大到 35 个）。
    if options.affects_text() && out != formatted && options.comma_style == CommaStyle::Trailing {
        for _ in 0..2 {
            let Ok(Some(next)) = lax_sql::format_text(Path::new(LAX_VIRTUAL_PATH), &out, &config)
            else {
                break;
            };
            let post = apply_options(&next, options);
            if post == out {
                break;
            }
            out = post;
        }
    }
    // lax 的输出**带结尾换行**（`"select\n  1\n"`）。语句体是从 `split_statements` 的区间
    // 切出来的（区间不含 `;`），如果保留这个尾换行，回填后分隔符就被顶到下一行
    // （`1\n;`，分号独占一行）。所以裁掉结尾空白；正文内部的换行不受影响。
    while out.ends_with('\n') || out.ends_with('\r') {
        out.pop();
    }
    Some(out)
}

impl FormatOptions {
    /// 后处理是否会改动文本（三项全默认 = 原样返回，不需要再排一轮）
    fn affects_text(&self) -> bool {
        self.keyword_case != KeywordCase::Preserve
            || self.comma_style != CommaStyle::Trailing
            || self.normalize_operators
    }
}

/// [`FormatOptions`] → lax `Configuration`
//
/// 字段一一对应，只有两处例外：
/// - `keyword_case` 固定 `Preserve`：大小写在 [`apply_options`] 里用 sqlglot 词法器做
///   （词表 200+ vs lax 85，理由见模块头）；
/// - `new_line_kind` 由 [`effective_newline`] 解析后传入（`Auto` 已在上一层落地）。
fn lax_config(options: &FormatOptions, newline: &str) -> LaxConfig {
    LaxConfig {
        line_width: u32::from(options.line_width.max(1)),
        use_tabs: matches!(options.indent_char, IndentChar::Tab),
        indent_width: options.indent_width.max(1),
        new_line_kind: if newline == "\r\n" {
            DprintNewLineKind::CarriageReturnLineFeed
        } else {
            DprintNewLineKind::LineFeed
        },
        // 固定 Preserve：关键字大小写由 sqlglot 词法器在 apply_options 里做
        keyword_case: LaxKeywordCase::Preserve,
        clause_style: match options.clause_style {
            ClauseStyle::Fill => LaxClauseStyle::Fill,
            ClauseStyle::Expanded => LaxClauseStyle::Expanded,
        },
        ignore_node_comment_text: options.ignore_node_comment_text.clone(),
        ignore_file_comment_text: options.ignore_file_comment_text.clone(),
    }
}

/// 摘掉**行注释**之后紧跟的 `\r`（lax 0.3.0 CRLF 缺陷的规避，详见 [`format_single`]）
///
/// 判据来自实测（`probe_comment_span_shape`）：sqlglot 的行注释 span **不含**行尾 `\r`
/// （`"-- a\r\nselect 1"` 的 span 是 `0..4` 的 `"-- a"`），块注释 span 含内部换行
/// （`"/* 块\r\n注释 */"`）。所以只需看 `span.end` 之后那个字符是不是 `\r`：
/// - 行注释（`--` / `#`，不以 `/*` 开头）→ 删掉那个 `\r`；
/// - 块注释 → 跳过（内部换行是注释内容本身）；
/// - 字符串 / 正文 → 根本不看（只在 Comment span 上判断）。
///
/// 词法失败（`highlight_spans` 为空）时不做任何处理——此时 `\r` 累加缺陷仍在，
/// 但误删字符串里的 `\r` 风险更大。该边界已记入架构文档。
fn strip_line_comment_cr(body: &str) -> String {
    let spans = highlight_spans(body);
    let mut targets = Vec::new();
    for span in &spans {
        if span.class != TokenClass::Comment {
            continue;
        }
        if span.text(body).starts_with("/*") {
            continue;
        }
        if body[span.end..].starts_with('\r') {
            targets.push(span.end);
        }
    }
    if targets.is_empty() {
        return body.to_string();
    }
    let mut out = body.to_string();
    for at in targets.into_iter().rev() {
        out.remove(at);
    }
    out
}

/// 对**已排版**的语句文本应用后处理（字符串 / 注释区间不碰）
//
/// 缩进已由 lax 按 `indentWidth` / `useTabs` 排好，**不再重排**（旧实现第 3 步已删除）。
/// 剩下三步，顺序有讲究——**改写长度的步骤依次往后**：
///
/// 1. **关键字大小写**（同长 ASCII 改写，区间偏移不动）：复用高亮词法器
///    （[`highlight_spans`]），只改写 `TokenClass::Keyword` 区间。标识符 / 字符串 /
///    注释天然排除（`TokenType` 语义判定，不是文本猜测）——`select 'select'` 里的
///    字符串不会被改。
/// 2. **逗号前导迁移**（长度会变）：只认**行尾** `,` 且不在字符串 / 注释保护区间内。
/// 3. **运算符两侧补空格**（长度会变，所以放最后，并**重新取 spans**）：
///    见 [`normalize_operators`]。默认关。
fn apply_options(text: &str, options: &FormatOptions) -> String {
    let untouched = options.keyword_case == KeywordCase::Preserve
        && options.comma_style == CommaStyle::Trailing
        && !options.normalize_operators;
    if untouched {
        return text.to_string();
    }

    let spans = highlight_spans(text);
    let protected: Vec<Range<usize>> = spans
        .iter()
        .filter(|s| matches!(s.class, TokenClass::String | TokenClass::Comment))
        .map(|s| s.start..s.end)
        .collect();

    let mut out = text.to_string();

    // 1) 关键字大小写（ASCII 改写同长，区间偏移不移动；从后往前保险）
    if options.keyword_case != KeywordCase::Preserve {
        let keyword_spans: Vec<Range<usize>> = spans
            .iter()
            .filter(|s| s.class == TokenClass::Keyword)
            .map(|s| s.start..s.end)
            .collect();
        for range in keyword_spans.into_iter().rev() {
            let (head, tail) = out.split_at(range.start);
            let (mid, rest) = tail.split_at(range.end - range.start);
            let rewritten = match options.keyword_case {
                KeywordCase::Uppercase => mid.to_ascii_uppercase(),
                KeywordCase::Lowercase => mid.to_ascii_lowercase(),
                KeywordCase::Preserve => mid.to_string(),
            };
            out = format!("{head}{rewritten}{rest}");
        }
    }

    // 2) 逗号前导迁移（第 1 步同长，`protected` 偏移仍然有效）
    if options.comma_style == CommaStyle::Leading {
        out = migrate_trailing_commas(&out, &protected);
    }

    // 3) 运算符两侧补空格（第 2 步改过长度，必须重新取 spans）
    if options.normalize_operators {
        let out_spans = highlight_spans(&out);
        out = normalize_operators(&out, &out_spans);
    }

    out
}

/// 双目运算符白名单（两侧补空格）
///
/// **形态必须按实测定**（2026-11-22 探针 `probe_operator_tokens`）：`<>` `!=` `<=` `>=`
/// `::` `->` `->>` `#>` `#>>` `||` 都是**单个 token**，所以「惯例不空格」的 `::` / `->`
/// 这类整词可以直接不进白名单；而 `@@` 是**两个** `@`（各自 `Operator`），同样天然不处理。
const SPACING_OPERATORS: [&str; 13] = [
    "=", "<>", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "||",
];

/// 一元 / 通配形态判据：只看**前一个 token**
///
/// - 前面没有 token（行首）→ 一元
/// - 前一个是关键字（`select -1` / `select *` / `between -1`）→ 一元
/// - 前一个是运算符（`a=-1` / `a - -1`）→ 一元
/// - 前一个是标点 `(` `[` `,` `.`（`count(*)` / `IN (-1)` / `t.*`）→ 通配 / 一元
///
/// 只有 `- + *` 有这种形态；`= <> != <= >= < > / % ||` 恒为双目。
fn is_unary_or_wildcard(value: &str, prev: Option<&HighlightSpan>, text: &str) -> bool {
    if !matches!(value, "-" | "+" | "*") {
        return false;
    }
    let Some(prev) = prev else {
        return true;
    };
    match prev.class {
        TokenClass::Keyword | TokenClass::Operator => true,
        TokenClass::Punctuation => matches!(prev.text(text), "(" | "[" | "," | "."),
        _ => false,
    }
}

/// 运算符两侧补空格（**只补不删**）
///
/// 为什么只补不删：本功能的目标是「没空格的补上」，不是「统一成一种间距」。
/// 删多余空格会动到用户手工对齐（`a  =  1` 这类），风险大于收益。
///
/// 为什么从后往前：每次插入都会移动后面的偏移，倒序处理后**前面的区间仍然有效**，
/// 可以一直用原始的 `spans` 坐标（与 [`apply_options`] 第 1 步同策略）。
fn normalize_operators(text: &str, spans: &[HighlightSpan]) -> String {
    let mut out = text.to_string();
    for (index, span) in spans.iter().enumerate().rev() {
        if span.class != TokenClass::Operator {
            continue;
        }
        let value = span.text(text);
        if !SPACING_OPERATORS.contains(&value) {
            continue;
        }
        let prev = index.checked_sub(1).map(|i| &spans[i]);
        if is_unary_or_wildcard(value, prev, text) {
            continue;
        }
        let needs_left = span.start > 0 && !left_char_is_blank(text, span.start);
        let needs_right = span.end < text.len() && !right_char_is_blank(text, span.end);
        if needs_right {
            out.insert(span.end, ' ');
        }
        if needs_left {
            out.insert(span.start, ' ');
        }
    }
    out
}

/// `at` 之前是否有空白（`at` 必须在字符边界上）
fn left_char_is_blank(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace)
}

/// `at` 之后是否有空白（`at` 必须在字符边界上）
fn right_char_is_blank(text: &str, at: usize) -> bool {
    text[at..].chars().next().is_none_or(char::is_whitespace)
}

/// 尾随逗号 → 前导逗号（行级迁移；`protected` 是**与 `text` 同一偏移**的区间表）
///
/// lax 的输出形状是「每个列表项一行、行尾带 `,`」；前导风格把逗号挪到下一行
/// 第一个非空白字符前（`  col1\n  , col2`）。只认**行尾** `,` 且不在保护区间内：
/// 行内逗号（GROUP BY 单行列表）、多行字符串里的 `,\n`（行尾逗号在字符串区间内）、
/// 行尾注释里的逗号（同理在注释区间内）一律不动。末行 / 下一行为空的行尾逗号不迁（防御）。
fn migrate_trailing_commas(text: &str, protected: &[Range<usize>]) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    // 先按**原偏移**逐行判定（判定互不影响），再统一重建
    let mut migrate = vec![false; lines.len()];
    let mut offset = 0usize;
    for i in 0..lines.len() {
        let line = lines[i];
        let line_start = offset;
        offset += line.len();
        if i + 1 >= lines.len() {
            continue; // 末行无处可迁
        }
        let body_end = line.trim_end_matches(['\n', '\r']).len();
        let trimmed = line[..body_end].trim_end();
        let Some(comma_offset) = trimmed.len().checked_sub(1) else {
            continue;
        };
        if !trimmed.ends_with(',') {
            continue;
        }
        let comma_at = line_start + comma_offset;
        if protected.iter().any(|r| r.contains(&comma_at)) {
            continue;
        }
        if lines[i + 1].trim().is_empty() {
            continue;
        }
        migrate[i] = true;
    }
    if !migrate.iter().any(|&m| m) {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        let mut owned = line.to_string();
        if migrate[i] {
            // 摘掉行尾逗号（连同逗号之后的空白；换行保留）
            let body_end = owned.trim_end_matches(['\n', '\r']).len();
            let trimmed_end = owned[..body_end].trim_end().len();
            let comma_idx = trimmed_end - 1;
            let newline_suffix = owned.split_off(body_end); // `\n`（或空）
            owned.truncate(comma_idx);
            owned.push_str(&newline_suffix);
        }
        if i > 0 && migrate[i - 1] {
            // 在第一个非空白字符前插 `, `（紧跟缩进之后）
            if let Some(pos) = owned.find(|ch: char| !ch.is_whitespace()) {
                owned.insert_str(pos, ", ");
            }
        }
        out.push_str(&owned);
    }
    out
}

/// 格式化 SQL（多语句脚本安全；**任何输入都不会被拒绝**）
//
/// 保留这个短签名给不需要报告的调用点；语义与 [`format_with_report`] 的 `.text` 一致
/// （含注释保真兜底）。
pub fn format(sql: &str, dialect: SqlDialect) -> String {
    format_with_report(sql, dialect, FormatOptions::default()).text
}

/// 内容保真判据：注释标记数、逗号数、**行注释里的逗号数**都与原文一致
///
/// 排版只动空白，所以这几个计数**理应不变**；变了说明上游动了内容，此时整篇回退原文。
/// 计数口径粗（会数到字符串里的 `--` / `,`），但**两侧同口径比较**，字符串里的出现次数
/// 一致时不会误判。
fn same_content_signature(before: &str, after: &str) -> bool {
    comment_mark_count(before) == comment_mark_count(after)
        && comma_mark_count(before) == comma_mark_count(after)
        && commas_in_line_comments(before) == commas_in_line_comments(after)
}

/// 注释标记计数（行注释 `--` / 块注释 `/*` / MySQL `#`）
fn comment_mark_count(text: &str) -> usize {
    text.matches("--").count() + text.matches("/*").count() + text.matches('#').count()
}

/// 逗号计数
fn comma_mark_count(text: &str) -> usize {
    text.matches(',').count()
}

/// 落在**行注释里**的逗号数
///
/// 单独统计它的原因（2026-11-22 实测）：lax 对「前导逗号 + 行尾注释」这种写法
/// （`select a -- 注释\n, b`）会把逗号排到注释文本之后（变成 `a -- 注释,`），
/// 逗号被行注释吞掉 —— **总逗号数一点没变**，只有"行注释里的逗号数"变了。
/// 这是唯一能识别"逗号被注释吞掉、SQL 已变坏"的廉价判据。
fn commas_in_line_comments(text: &str) -> usize {
    text.lines()
        .filter_map(line_comment_tail)
        .map(|tail| tail.matches(',').count())
        .sum()
}

/// 一行的行注释部分（`--` 或 MySQL `#` 起头，取更靠前者）
fn line_comment_tail(line: &str) -> Option<&str> {
    let dash = line.find("--");
    let hash = line.find('#');
    match (dash, hash) {
        (Some(d), Some(h)) => Some(&line[d.min(h)..]),
        (Some(d), None) => Some(&line[d..]),
        (None, Some(h)) => Some(&line[h..]),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlglot_rust::Dialect;
    use sqlglot_rust::parse_statements_with_comments as parse_many;

    /// 【探针】看词法器把各类运算符切成什么（`--nocapture` 看）
    ///
    /// 为什么必须先做这一步：运算符归一要不要补空格，取决于词法器把 `->>` / `::` /
    /// `@` / `!=` 切成**一个** token 还是几个 —— 猜错就会把 `data->>'k'` 拆成
    /// `data - > > 'k'`，那是把用户 SQL 改坏。
    #[test]
    fn probe_operator_tokens() {
        let cases = [
            "select a=1 from t",
            "select a<>b, a!=b, a<=b, a>=b from t",
            "select -1, +2, a-1, a * b, a % b from t",
            "select count(*) from t",
            "select * from t",
            "select t.* from t",
            "select a.b from t",
            "select a::int from t",
            "select data->'k', data->>'k', data#>'{a}', data#>>'{a}' from t",
            "select @x, @@version from t",
            "select a||b from t",
            "where a=-1",
            "select !a, ~a from t",
        ];
        for sql in cases {
            println!("--- {sql}");
            for span in highlight_spans(sql) {
                println!(
                    "  [{:?}] {}..{} {:?}",
                    span.class,
                    span.start,
                    span.end,
                    span.text(sql)
                );
            }
        }
    }

    /// 【运算符间距】白名单内的双目运算符补空格；一元 / 通配 / `::` / `->` 都不动
    ///
    /// 期望值来自 2026-11-22 `probe_operator_tokens` 的实测 token 形态
    /// （`<>` `!=` `::` `->>` `#>>` 各是**一个** token；`@@` 是**两个** `@`）。
    #[test]
    fn operators_get_spaces_only_when_binary() {
        let cases: &[(&str, &str)] = &[
            ("select a=1 from t", "select a = 1 from t"),
            (
                "select a<>b, a!=b, a<=b, a>=b from t",
                "select a <> b, a != b, a <= b, a >= b from t",
            ),
            ("select a||b, a%b from t", "select a || b, a % b from t"),
            ("select a-1, a/2 from t", "select a - 1, a / 2 from t"),
            // 一元 / 通配：一个空格都不加
            ("select -1, +2 from t", "select -1, +2 from t"),
            (
                "select count(*), sum(*) from t",
                "select count(*), sum(*) from t",
            ),
            ("select * from t", "select * from t"),
            ("select t.* from t", "select t.* from t"),
            ("where a=-1", "where a = -1"),
            // 惯例不空格的整体 token
            ("select a::int from t", "select a::int from t"),
            (
                "select data->'k', data->>'k' from t",
                "select data->'k', data->>'k' from t",
            ),
            ("select @x from t", "select @x from t"),
            // 字符串与注释里的 `=` 不动
            ("select 'a=1' as v -- a=1", "select 'a=1' as v -- a=1"),
            // 只补不删：已有空格原样（不替用户重排对齐）
            ("select a  =  1 from t", "select a  =  1 from t"),
            // 跨行：换行本身是空白，不被替换成空格
            ("select a\n=1", "select a\n= 1"),
        ];
        for (input, expected) in cases {
            let spans = highlight_spans(input);
            assert_eq!(
                normalize_operators(input, &spans),
                *expected,
                "输入：{input:?}"
            );
        }
    }

    /// 幂等：补过之后再跑一遍不变（否则保存两次会越补越多）
    #[test]
    fn operator_spacing_is_idempotent() {
        let input = "select a=1, b<>2 from t";
        let once = normalize_operators(input, &highlight_spans(input));
        assert_eq!(once, "select a = 1, b <> 2 from t");
        let twice = normalize_operators(&once, &highlight_spans(&once));
        assert_eq!(once, twice, "运算符补空格必须幂等");
    }

    /// 开关打开：整条链路（lax 排版 → 后处理）也补空格；默认关时 `a=1` 原样
    #[test]
    fn operator_spacing_is_opt_in_and_works_through_lax() {
        let off = format_with_report(
            "select a=1 from t",
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        assert!(off.text.contains("a=1"), "默认关时不该补：{:?}", off.text);

        let on = format_with_report(
            "select a=1 from t",
            SqlDialect::Ansi,
            FormatOptions {
                normalize_operators: true,
                ..FormatOptions::default()
            },
        );
        assert!(
            on.text.contains("a = 1"),
            "开关打开后应补空格：{:?}",
            on.text
        );
        assert_eq!(on.formatted, 1, "{:?}", on.text);
    }

    /// 【探针】注释 span 的形态：行注释是否把行尾 `\r` 吞进 token
    #[test]
    fn probe_comment_span_shape() {
        let cases: &[&str] = &[
            "-- a\r\nselect 1",
            "select a -- 列说明\r\nfrom t",
            "select /* 块\r\n注释 */ 1\r\nfrom t",
            "-- 末行注释\r",
        ];
        for body in cases {
            println!("--- IN {body:?}");
            for span in highlight_spans(body) {
                if span.class == TokenClass::Comment {
                    println!(
                        "  Comment {:?} = {:?}",
                        span.start..span.end,
                        span.text(body)
                    );
                }
            }
        }
    }

    /// 【探针】`\r` 每轮累加的根因：绕开本项目管线，直接看 lax 对 CRLF 输入的处理
    #[test]
    fn probe_lax_crlf() {
        fn run(label: &str, text: &str, kind: DprintNewLineKind) {
            let config = LaxConfig {
                line_width: 120,
                use_tabs: false,
                indent_width: 2,
                new_line_kind: kind,
                keyword_case: LaxKeywordCase::Preserve,
                clause_style: LaxClauseStyle::Expanded,
                ignore_node_comment_text: "dprint-ignore".to_string(),
                ignore_file_comment_text: "dprint-ignore-file".to_string(),
            };
            match lax_sql::format_text(Path::new(LAX_VIRTUAL_PATH), text, &config) {
                Ok(Some(out)) => println!("--- {label}\nIN : {text:?}\nOUT: {out:?}"),
                Ok(None) => println!("--- {label}\nIN : {text:?}\nOUT: None（与输入一致）"),
                Err(err) => println!("--- {label}\nIN : {text:?}\nERR: {err}"),
            }
        }

        // 同一段内容，四种"输入换行 × 配置换行"组合
        let lf_input = "select 1\nfrom t";
        let crlf_input = "select 1\r\nfrom t";
        run("LF 输入 + LF 配置", lf_input, DprintNewLineKind::LineFeed);
        run(
            "LF 输入 + CRLF 配置",
            lf_input,
            DprintNewLineKind::CarriageReturnLineFeed,
        );
        run(
            "CRLF 输入 + LF 配置",
            crlf_input,
            DprintNewLineKind::LineFeed,
        );
        run(
            "CRLF 输入 + CRLF 配置",
            crlf_input,
            DprintNewLineKind::CarriageReturnLineFeed,
        );
        // 关键对照：**带注释**的 CRLF 输入（此前只测了无注释输入）
        let comment_crlf = "-- a\r\nselect 1\r\nfrom t";
        run(
            "注释+CRLF 输入 + CRLF 配置",
            comment_crlf,
            DprintNewLineKind::CarriageReturnLineFeed,
        );
        run(
            "注释+CRLF 输入 + LF 配置",
            comment_crlf,
            DprintNewLineKind::LineFeed,
        );
        run(
            "行尾注释+CRLF 输入 + CRLF 配置",
            "select a -- 列说明\r\nfrom t",
            DprintNewLineKind::CarriageReturnLineFeed,
        );
        run(
            "块注释+CRLF 输入 + CRLF 配置",
            "select /* 块\r\n注释 */ 1\r\nfrom t",
            DprintNewLineKind::CarriageReturnLineFeed,
        );
    }

    /// 【探针】`dprint-ignore` 三种写法到底哪种生效
    ///
    /// 此前只验证了"行尾写法不生效"（文档 §7 #6 明确写着"不能断言独立行一定生效"）。
    /// 这个探针把行尾 / 独立行 / 块注释三种写法摆在一起，一次看清。
    #[test]
    fn probe_dprint_ignore_placement() {
        let cases: &[(&str, &str)] = &[
            ("行尾注释", "select    1; -- dprint-ignore"),
            ("独立行-语句前", "-- dprint-ignore\nselect    1;"),
            ("独立行-块注释", "/* dprint-ignore */\nselect    1;"),
            (
                "独立行-两条语句",
                "-- dprint-ignore\nselect    1;\nselect    2;",
            ),
            ("无标记对照", "select    1;"),
        ];
        for (name, sql) in cases {
            let out = format_with_report(sql, SqlDialect::Ansi, FormatOptions::default()).text;
            println!("--- {name}\nIN : {sql:?}\nOUT: {out:?}");
        }
    }

    /// CRLF + 行注释：`\r` 不得逐轮累加（lax 0.3.0 缺陷的回归测试）
    #[test]
    fn crlf_line_comments_do_not_accumulate_cr() {
        let options = FormatOptions::default();
        let input =
            "-- 头注释\r\nselect 1\r\nfrom t;\r\n\r\n-- 段间注释\r\nselect 2 -- 行尾\r\nfrom u;";
        let first = format_with_report(input, SqlDialect::Ansi, options.clone()).text;
        let second = format_with_report(&first, SqlDialect::Ansi, options.clone()).text;
        let third = format_with_report(&second, SqlDialect::Ansi, options.clone()).text;
        assert!(
            !first.contains("\r\r"),
            "行注释后不得出现 `\\r\\r`：{first:?}"
        );
        assert_eq!(
            first, second,
            "第二轮必须与第一轮相同：{first:?} / {second:?}"
        );
        assert_eq!(second, third, "第三轮必须稳定：{second:?} / {third:?}");
        // CRLF 输入必须整篇 CRLF：不留裸 LF
        let bare_lf = first.matches('\n').count() - first.matches("\r\n").count();
        assert_eq!(bare_lf, 0, "CRLF 输入不应留下裸 LF：{first:?}");
    }

    /// 真实语料形状：中文注释 + CRLF + 两条 DDL（取自 `store/migrations/001_init.sql` 首段）
    #[test]
    fn real_migration_fragment_is_idempotent() {
        let input = "-- 迁移版本：001\r\n-- 数据库类型：SQLite\r\n-- 作用：每个连接独立的元数据存储\r\n\r\ncreate table if not exists meta (\r\n  id integer primary key,\r\n  name text not null\r\n);\r\n\r\ncreate index if not exists idx_meta_name on meta(name);";
        let options = FormatOptions::default();
        let first = format_with_report(input, SqlDialect::Ansi, options.clone()).text;
        let second = format_with_report(&first, SqlDialect::Ansi, options.clone()).text;
        assert_eq!(
            first, second,
            "真实迁移片段必须幂等：\n第一轮：{first:?}\n第二轮：{second:?}"
        );
        assert!(!first.contains("\r\r"), "不得出现双 `\\r`：{first:?}");
        assert!(
            same_content_signature(input, &first),
            "保真签名不应变化：{first:?}"
        );
    }

    /// 【探针】真实语料对拍：`crates/store/migrations` 下全部手写 DDL
    ///
    /// 对应文档 §9 遗留的"200 条真实语料对拍"的低成本版——用仓库自带迁移脚本
    /// （真实工程 SQL：事务、注释、引号、索引、触发器）。只打印汇总与失败样本；
    /// 长期守卫由内联语料的断言测试承担（不依赖目录布局）。
    #[test]
    fn probe_real_corpus() {
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "sql") {
                    out.push(path);
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../store/migrations");
        let mut files = Vec::new();
        collect(&root, &mut files);
        files.sort();
        if files.is_empty() {
            println!("语料目录为空：{}", root.display());
            return;
        }

        let combos: Vec<(&str, FormatOptions)> = vec![
            ("default", FormatOptions::default()),
            (
                "ops-on",
                FormatOptions {
                    normalize_operators: true,
                    ..FormatOptions::default()
                },
            ),
            (
                "all-on",
                FormatOptions {
                    keyword_case: KeywordCase::Uppercase,
                    comma_style: CommaStyle::Leading,
                    normalize_operators: true,
                    line_width: 80,
                    blank_lines_between: 0,
                    ..FormatOptions::default()
                },
            ),
        ];

        println!("--- 语料：{} 个迁移文件", files.len());
        for (name, options) in combos {
            let mut formatted = 0usize;
            let mut fallback = 0usize;
            let mut idem_fail = Vec::new();
            let mut sig_fail = Vec::new();

            for path in &files {
                let Ok(input) = std::fs::read_to_string(path) else {
                    continue;
                };
                let first = format_with_report(&input, SqlDialect::Ansi, options.clone());
                if first.formatted == 0 {
                    fallback += 1;
                } else {
                    formatted += 1;
                }
                if !same_content_signature(&input, &first.text) {
                    sig_fail.push(path.display().to_string());
                }
                let second = format_with_report(&first.text, SqlDialect::Ansi, options.clone());
                if second.text != first.text {
                    idem_fail.push(path.display().to_string());
                }
            }

            println!(
                "[{name}] formatted={formatted} fallback={fallback} idem_fail={} sig_fail={}",
                idem_fail.len(),
                sig_fail.len()
            );
            for label in idem_fail.iter().take(5) {
                println!("  idem 失败：{label}");
            }
            for label in sig_fail.iter().take(5) {
                println!("  shape 失败：{label}");
            }
        }
    }

    /// 【探针】把典型输入的排版结果打出来（`--nocapture` 看），用于校准断言。
    ///
    /// 保留在测试集里：换 lax 版本时先跑它，一眼能看出排版风格有没有漂。
    #[test]
    fn probe_lax_output() {
        let cases: &[(&str, &str)] = &[
            ("basic-expanded", "select a,b from t where x=1"),
            (
                "order-limit",
                "select id, count(*) as n from orders group by id order by n desc limit 10",
            ),
            ("lists", "select a,b,c from t where a=1 and b=2 and c=3"),
            ("subquery", "select a from t where x = (select y from u)"),
            ("leading-comment", "-- 说明\nselect 1"),
            ("trailing-comment", "select 1 -- 尾随"),
            ("inline-comment", "select /* 行内 */ 1"),
            ("mid-statement-comment", "select a -- 列说明\nfrom t"),
            (
                "join",
                "select u.id, u.name, o.total from users u join orders o on o.user_id = u.id where o.status = 'paid'",
            ),
            ("fragment-where", "where x = 1 AND b = 2"),
            ("fragment-list", "a, b, c"),
            ("comment-only", "-- 只有注释"),
            ("empty", ""),
            ("junk", "NOT VALID SQL"),
            ("multiline-string", "select 'a\n  b' as v"),
            (
                "case-expr",
                "select case when a=1 then 'x' else 'y' end as k from t",
            ),
            ("cte", "with c as (select 1 as a) select a from c"),
        ];

        for (name, sql) in cases {
            let out = format_with_report(sql, SqlDialect::Ansi, FormatOptions::default()).text;
            println!("--- {name}\nIN : {sql:?}\nOUT: {out:?}");
        }

        let fill = format_with_report(
            "select a,b,c from t where a=1 and b=2 and c=3",
            SqlDialect::Ansi,
            FormatOptions {
                clause_style: ClauseStyle::Fill,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- fill-style\nOUT: {fill:?}");

        let wide = format_with_report(
            "select a, b from t where x = 1",
            SqlDialect::Ansi,
            FormatOptions {
                indent_width: 4,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- indent-4\nOUT: {wide:?}");

        let tabbed = format_with_report(
            "select a from t where x = (select y from u)",
            SqlDialect::Ansi,
            FormatOptions {
                indent_char: IndentChar::Tab,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- tab\nOUT: {tabbed:?}");

        let narrow = format_with_report(
            "select u.id, u.name, o.total from users u join orders o on o.user_id = u.id",
            SqlDialect::Ansi,
            FormatOptions {
                line_width: 40,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- width-40\nOUT: {narrow:?}");

        let leading = format_with_report(
            "select a, b from t",
            SqlDialect::Ansi,
            FormatOptions {
                comma_style: CommaStyle::Leading,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- leading-comma\nOUT: {leading:?}");

        let ignore_node = format_with_report(
            "select a -- dprint-ignore\n, b from t",
            SqlDialect::Ansi,
            FormatOptions::default(),
        )
        .text;
        println!("--- ignore-node\nOUT: {ignore_node:?}");

        // ---- 以下为第二轮探针（拼接 / 保真回退 / 选区缩进 / 换行符）----

        let multi = format_with_report(
            "select 1; select 2;",
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        println!("--- multi-stmt\nOUT: {:?}", multi.text);

        // 行尾注释紧邻逗号：逗号会被行注释吞掉 → 应触发保真回退，输出等于输入
        let comment_comma = format_with_report(
            "select a -- 注释\n, b from t",
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        println!(
            "--- comment-then-comma (comma_before={} comma_after={})\nOUT: {:?}",
            comma_mark_count("select a -- 注释\n, b from t"),
            comma_mark_count(&comment_comma.text),
            comment_comma.text
        );

        // 选区片段带前导缩进（编辑器里从行首缩进处开始选）
        let indented = format_with_report(
            "    where x = 1 and b = 2",
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        println!("--- indented-fragment\nOUT: {:?}", indented.text);

        // CRLF 原文 + Auto（应保持 CRLF）
        let crlf = format_with_report(
            "select 1;\r\nselect 2;",
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        println!("--- crlf-auto\nOUT: {:?}", crlf.text);

        // 强制 CRLF
        let crlf_forced = format_with_report(
            "select 1;\nselect 2;",
            SqlDialect::Ansi,
            FormatOptions {
                new_line_kind: NewLineKind::Crlf,
                ..FormatOptions::default()
            },
        );
        println!("--- crlf-forced\nOUT: {:?}", crlf_forced.text);

        // 文件级忽略
        let file_ignore = format_with_report(
            "-- dprint-ignore-file\nselect a,b from t",
            SqlDialect::Ansi,
            FormatOptions::default(),
        );
        println!("--- file-ignore\nOUT: {:?}", file_ignore.text);

        // ---- 第三轮探针（大小写 / 前导逗号幂等 / fill 折行 / 组合）----

        let upper = format_with_report(
            "-- where\nselect id, 'select' as s from t where id = 1",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Uppercase,
                ..FormatOptions::default()
            },
        );
        println!("--- uppercase\nOUT: {:?}", upper.text);

        let lower = format_with_report(
            "SELECT ID FROM T",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Lowercase,
                ..FormatOptions::default()
            },
        );
        println!("--- lowercase\nOUT: {:?}", lower.text);

        // 前导逗号幂等：把上一轮前导逗号结果再格式化一遍
        let leading_once = format_with_report(
            "select a, b from t",
            SqlDialect::Ansi,
            FormatOptions {
                comma_style: CommaStyle::Leading,
                ..FormatOptions::default()
            },
        )
        .text;
        let leading_twice = format_with_report(
            &leading_once,
            SqlDialect::Ansi,
            FormatOptions {
                comma_style: CommaStyle::Leading,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- leading-idempotent\nONCE : {leading_once:?}\nTWICE: {leading_twice:?}");

        // fill + 窄行宽：长 JOIN 是否折行
        let narrow_fill = format_with_report(
            "select u.id, u.name, o.total from users u join orders o on o.user_id = u.id",
            SqlDialect::Ansi,
            FormatOptions {
                clause_style: ClauseStyle::Fill,
                line_width: 40,
                ..FormatOptions::default()
            },
        )
        .text;
        println!("--- narrow-fill\nOUT: {:?}", narrow_fill);

        // 组合：大写 + 4 空格 + Tab + 前导逗号 + 0 空行，跑两遍看幂等
        let combined_opts = FormatOptions {
            keyword_case: KeywordCase::Uppercase,
            indent_width: 4,
            indent_char: IndentChar::Tab,
            comma_style: CommaStyle::Leading,
            blank_lines_between: 0,
            ..FormatOptions::default()
        };
        let combined_once = format_with_report(
            "select a, b from t; select c",
            SqlDialect::Ansi,
            combined_opts.clone(),
        );
        let combined_twice =
            format_with_report(&combined_once.text, SqlDialect::Ansi, combined_opts);
        println!(
            "--- combined\nONCE : {:?}\nTWICE: {:?}\nEQUAL: {}",
            combined_once.text,
            combined_twice.text,
            combined_once.text == combined_twice.text
        );

        // 再解析能力：默认 expanded 输出能否被 sqlglot 再解析
        let reparse = format_with_report(
            "select id, count(*) as n from orders group by id order by n desc limit 10",
            SqlDialect::Ansi,
            FormatOptions::default(),
        )
        .text;
        println!(
            "--- reparsable: {:?} → {:?}",
            reparse,
            sqlglot_rust::parse_statements_with_comments(&reparse, sqlglot_rust::Dialect::Ansi)
                .is_ok()
        );
    }

    // =====================================================================
    // 正式断言（全部数值来自 2026-11-22 对 lax-sql 0.3.0 的实测输出）
    // =====================================================================

    /// 默认（`ClauseStyle::Expanded`）：子句关键字独占一行，列表一行一项
    ///
    /// 注意 `x=1` 保持原样：**lax 只调整结构性空白（换行 / 缩进），不规范化 token 之间的
    /// 间距**。`x=1` 不会变成 `x = 1`，`x = 1` 也不会被压紧。这是 lax 的既有边界，
    /// 不是本项目的 bug；需要运算符间距归一属于后续增强（见架构文档"已知差距"）。
    #[test]
    fn formats_into_readable_sql() {
        assert_eq!(
            format("select a,b from t where x=1", SqlDialect::Ansi),
            "select\n  a,\n  b\nfrom\n  t\nwhere\n  x=1"
        );
        assert_eq!(
            format(
                "select id, count(*) as n from orders group by id order by n desc limit 10",
                SqlDialect::Ansi
            ),
            "select\n  id,\n  count(*) as n\nfrom\n  orders\ngroup by\n  id\norder by\n  n desc\nlimit\n  10"
        );
    }

    /// 排版结果必须仍是 sqlglot 能解析的 SQL（不能为了排版把语法拆坏）
    #[test]
    fn formatted_output_can_be_reparsed() {
        let out = format(
            "select id, count(*) as n from orders group by id order by n desc limit 10",
            SqlDialect::Ansi,
        );
        assert!(
            parse_many(&out, Dialect::Ansi).is_ok(),
            "格式化结果必须能再次解析：{out}"
        );
    }

    /// 多语句：语句之间 `;` + 两个换行，分号**留在语句行末**（不独占一行）
    #[test]
    fn multi_statement_script_keeps_every_statement() {
        let out = format("select 1; select 2;", SqlDialect::Ansi);
        assert_eq!(out, "select\n  1;\n\nselect\n  2;");
        assert_eq!(out.matches("select").count(), 2, "{out}");
        assert!(!out.contains(";;"), "不应出现连续分号：{out}");
    }

    /// 空 / 纯空白输入原样返回（不凭空造内容）
    #[test]
    fn blank_inputs_come_back_unchanged() {
        assert_eq!(format("", SqlDialect::Ansi), "");
        assert_eq!(format("   \n", SqlDialect::Ansi), "   \n");
    }

    /// **行为变化（相对 sqlglot 时代）**：不完整 / 无效的片段不再"原样返回"，而是被排版。
    ///
    /// 旧实现 `parse_statements_with_comments` 解析失败就整条不动；lax 永不拒绝输入，
    /// `NOT VALID SQL` 会被当作 `NOT` 子句 + 体。这是换取"片段可排版"的代价：
    /// 用户正在输入的半截语句也会被重排。这里锁住"不拒绝、不丢内容"，
    /// **不**声称输出一定更好看。
    #[test]
    fn incomplete_fragment_is_rewritten_not_rejected() {
        let out = format("NOT VALID SQL", SqlDialect::Ansi);
        assert_eq!(out, "NOT\n  VALID SQL");
        assert!(out.contains("VALID"), "内容不能丢：{out}");
    }

    /// **本次改造的核心收益**：注释无论落在哪里都保真（sqlglot pretty 只有语句前注释能活）
    #[test]
    fn comments_survive_anywhere() {
        // 尾随行注释
        assert_eq!(
            format("select 1 -- 尾随", SqlDialect::Ansi),
            "select\n  1 -- 尾随"
        );
        // 行内块注释
        assert_eq!(
            format("select /* 行内 */ 1", SqlDialect::Ansi),
            "select\n  /* 行内 */ 1"
        );
        // 关键字之间的行注释
        assert_eq!(
            format("select a -- 列说明\nfrom t", SqlDialect::Ansi),
            "select\n  a -- 列说明\nfrom\n  t"
        );
        // 语句前注释（旧实现唯一支持的位置，不能退化）
        assert_eq!(
            format("-- 说明\nselect 1", SqlDialect::Ansi),
            "-- 说明\nselect\n  1"
        );
    }

    /// **本次改造的核心收益**：选区的半截片段能排版（旧实现解析失败 → 原样返回 → 按了没反应）
    #[test]
    fn fragment_selection_is_formatted() {
        // 只选中 WHERE 子句
        assert_eq!(
            format("where x = 1 AND b = 2", SqlDialect::Ansi),
            "where\n  x = 1 AND b = 2"
        );
        // 选区带着行首缩进（编辑器里从缩进处开始选）
        assert_eq!(
            format("    where x = 1 and b = 2", SqlDialect::Ansi),
            "where\n  x = 1 and b = 2"
        );
    }

    /// 方言参数保留在 API 上但**不参与排版**：十个方言排出来一字不差
    #[test]
    fn dialect_does_not_change_typesetting() {
        for dialect in [
            SqlDialect::Ansi,
            SqlDialect::Mysql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::Duckdb,
            SqlDialect::MsSQL,
            SqlDialect::Oracle,
            SqlDialect::Snowflake,
            SqlDialect::BigQuery,
            SqlDialect::Redshift,
        ] {
            assert_eq!(
                format("select 1", dialect),
                "select\n  1",
                "排版应方言无关：{dialect:?}"
            );
        }
    }

    /// 关键字大小写由 sqlglot 词法器判定：字符串 / 注释 / 标识符一律不动
    #[test]
    fn keyword_case_only_touches_keywords() {
        let upper = format_with_report(
            "-- where\nselect id, 'select' as s from t where id = 1",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Uppercase,
                ..FormatOptions::default()
            },
        )
        .text;
        // `'select'` 是字符串、`id`/`t`/`s` 是标识符、`-- where` 是注释 —— 全部保持原样
        assert_eq!(
            upper,
            "-- where\nSELECT\n  id,\n  'select' AS s\nFROM\n  t\nWHERE\n  id = 1"
        );

        let lower = format_with_report(
            "SELECT ID FROM T",
            SqlDialect::Ansi,
            FormatOptions {
                keyword_case: KeywordCase::Lowercase,
                ..FormatOptions::default()
            },
        )
        .text;
        assert_eq!(lower, "select\n  ID\nfrom\n  T");
    }

    /// 缩进宽度由 lax 排：行首缩进按配置，多行字符串内部不碰
    #[test]
    fn indent_width_is_applied_by_lax() {
        let wide = format_with_report(
            "select a, b from t where x = 1",
            SqlDialect::Ansi,
            FormatOptions {
                indent_width: 4,
                ..FormatOptions::default()
            },
        )
        .text;
        assert_eq!(wide, "select\n    a,\n    b\nfrom\n    t\nwhere\n    x = 1");

        // 字符串内部的换行与缩进是**内容**，不能被当成待排版的行首
        let quoted = format_with_report(
            "select 'a\n  b' as v",
            SqlDialect::Ansi,
            FormatOptions {
                indent_width: 4,
                ..FormatOptions::default()
            },
        )
        .text;
        assert!(quoted.contains("'a\n  b'"), "字符串内容原样：{quoted:?}");
    }

    /// 前导逗号：只把**行尾**逗号挪到下一行内容前，且格式化幂等
    #[test]
    fn leading_commas_are_idempotent() {
        let options = FormatOptions {
            comma_style: CommaStyle::Leading,
            ..FormatOptions::default()
        };
        let once = format_with_report("select a, b from t", SqlDialect::Ansi, options.clone());
        assert_eq!(once.text, "select\n  a\n  , b\nfrom\n  t");
        assert!(
            parse_many(&once.text, Dialect::Ansi).is_ok(),
            "{}",
            once.text
        );

        let twice = format_with_report(&once.text, SqlDialect::Ansi, options);
        assert_eq!(once.text, twice.text, "前导逗号格式化应当幂等");
    }

    /// 前导逗号不碰多行字符串里的 `,\\n`
    #[test]
    fn leading_commas_never_touch_multiline_strings() {
        let text = format_with_report(
            "select 'a,\nb' as v, c from t",
            SqlDialect::Ansi,
            FormatOptions {
                comma_style: CommaStyle::Leading,
                ..FormatOptions::default()
            },
        )
        .text;
        assert!(text.contains("'a,\nb'"), "多行字符串内容原样：{text:?}");
        assert!(!text.contains("\n,b'"), "字符串内逗号不能被迁走：{text:?}");
    }

    /// `ClauseStyle::Fill`：每个子句一行，子句体在行内（与 Expanded 的"一行一项"相对）
    #[test]
    fn clause_style_fill_keeps_clause_body_on_one_line() {
        let text = format_with_report(
            "select a,b,c from t where a=1 and b=2 and c=3",
            SqlDialect::Ansi,
            FormatOptions {
                clause_style: ClauseStyle::Fill,
                ..FormatOptions::default()
            },
        )
        .text;
        assert_eq!(text, "select a,b,c\nfrom t\nwhere a=1 and b=2 and c=3");
    }

    /// Tab 缩进：每级一个 `\t`，不出现空格缩进行
    #[test]
    fn tab_indent_uses_one_tab_per_level() {
        let text = format_with_report(
            "select a from t where x = (select y from u)",
            SqlDialect::Ansi,
            FormatOptions {
                indent_char: IndentChar::Tab,
                ..FormatOptions::default()
            },
        )
        .text;
        assert_eq!(
            text,
            "select\n\ta\nfrom\n\tt\nwhere\n\tx = (select y from u)"
        );
        assert!(
            text.lines().all(|line| !line.starts_with(' ')),
            "不应出现空格缩进行：{text:?}"
        );
    }

    /// 语句间空行数可配
    #[test]
    fn blank_lines_between_statements_is_configurable() {
        let compact = format_with_report(
            "select 1; select 2;",
            SqlDialect::Ansi,
            FormatOptions {
                blank_lines_between: 0,
                ..FormatOptions::default()
            },
        )
        .text;
        assert_eq!(compact, "select\n  1;\nselect\n  2;");

        let airy = format_with_report(
            "select 1; select 2;",
            SqlDialect::Ansi,
            FormatOptions {
                blank_lines_between: 2,
                ..FormatOptions::default()
            },
        )
        .text;
        assert!(
            airy.contains(";\n\n\nselect"),
            "2 空行应拉开三个换行：{airy:?}"
        );
    }

    /// 换行符：CRLF 文档格式化后仍是**整篇 CRLF**（语句体与语句间隔一致，不混排）
    ///
    /// 这一条是实打实的回归防线：`rewrite.rs` 的 gap 规整曾经硬编码 `\n`，
    /// lax 的 `NewLineKind::Auto` 又只看单条语句体（通常单行 → 恒选 LF），
    /// 两边一叠加就是"语句体 CRLF、语句之间 LF"的混排文件。
    #[test]
    fn crlf_document_stays_crlf() {
        let text = format("select 1;\r\nselect 2;", SqlDialect::Ansi);
        assert_eq!(text, "select\r\n  1;\r\n\r\nselect\r\n  2;");
        // 去掉合法的 CRLF 后不应再残留任何裸 \n
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "不应混入裸 LF：{text:?}"
        );

        // 强制 CRLF（原文是 LF）
        let forced = format_with_report(
            "select 1;\nselect 2;",
            SqlDialect::Ansi,
            FormatOptions {
                new_line_kind: NewLineKind::Crlf,
                ..FormatOptions::default()
            },
        )
        .text;
        assert_eq!(forced, "select\r\n  1;\r\n\r\nselect\r\n  2;");
    }

    /// **保真兜底**：行尾注释会把后续前导逗号吞掉时（lax 的边角行为），整篇回退原文
    ///
    /// `select a -- 注释\n, b` 交给 lax 会排成 `a -- 注释,` —— 逗号进入行注释，
    /// 总逗号数不变但 SQL 已经变坏。`commas_in_line_comments` 是唯一能识别这种情况的判据。
    #[test]
    fn comma_swallowed_by_line_comment_falls_back_to_input() {
        let sql = "select a -- 注释\n, b from t";
        assert_eq!(format(sql, SqlDialect::Ansi), sql, "变坏的内容必须回退");
    }

    /// 文件级忽略注释：整篇不排版
    #[test]
    fn file_ignore_comment_returns_input() {
        let sql = "-- dprint-ignore-file\nselect a,b from t";
        assert_eq!(format(sql, SqlDialect::Ansi), sql);
    }

    /// 语句之间的空白规整成 `;` + 两个换行，注释一字不动，且整体幂等
    #[test]
    fn gaps_are_normalized_and_idempotent() {
        let out = format("select 1; select 2;", SqlDialect::Ansi);
        assert!(out.contains(";\n\nselect"), "语句之间要拉开：{out:?}");

        let commented = format("select 1; -- 这句得留着\nselect 2;", SqlDialect::Ansi);
        assert!(
            commented.contains("-- 这句得留着"),
            "注释不能被吃掉：{commented}"
        );

        let once = format("select 1; select 2;", SqlDialect::Ansi);
        let twice = format(&once, SqlDialect::Ansi);
        assert_eq!(once, twice, "格式化应当是幂等的");
    }

    /// 全旋钮组合：大写 + 4 缩进 + Tab + 前导逗号 + 0 空行，结果可再解析且幂等
    #[test]
    fn combined_options_still_reparse_and_idempotent() {
        let options = FormatOptions {
            keyword_case: KeywordCase::Uppercase,
            indent_width: 4,
            indent_char: IndentChar::Tab,
            comma_style: CommaStyle::Leading,
            blank_lines_between: 0,
            ..FormatOptions::default()
        };
        let once = format_with_report(
            "select a, b from t; select c",
            SqlDialect::Ansi,
            options.clone(),
        );
        assert_eq!(once.text, "SELECT\n\ta\n\t, b\nFROM\n\tt;\nSELECT\n\tc");
        assert!(
            parse_many(&once.text, Dialect::Ansi).is_ok(),
            "组合结果必须能再次解析：{}",
            once.text
        );
        let twice = format_with_report(&once.text, SqlDialect::Ansi, options);
        assert_eq!(once.text, twice.text, "组合选项应当幂等");
    }
}
