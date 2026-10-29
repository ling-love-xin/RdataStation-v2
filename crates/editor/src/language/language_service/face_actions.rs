//! code action 面（code_actions + report/line_column）
//!
//! 自 language/language_service.rs 拆分（2026-10-29 Step 9）：按能力域切分。

use engine::sql::{
    HighlightSpan, SqlStatement, TokenClass, highlight_spans,
    split_statements,
};

use crate::channel::{ExecChannel, statement_allowed};
use super::*;

pub fn code_actions(request: &Request) -> Vec<CodeAction> {
    let text = request.text;
    if text.len() > SCAN_MAX_BYTES {
        return Vec::new();
    }
    let Some(statement) = target_statement(text, request) else {
        return Vec::new();
    };
    let sql = statement.text(text);
    let spans = highlight_spans(sql);
    // 这一句扫不动（用户还在写字符串 / 注释）→ 连"是不是 UPDATE"都说不准，不给
    if spans.is_empty() {
        return Vec::new();
    }

    let mut found = Vec::new();
    if let Some(action) = add_where_action(sql, &spans, statement.end, request.channel) {
        found.push(action);
    }
    if let Some(action) = add_limit_action(sql, &spans, statement.end) {
        found.push(action);
    }
    found
}

/// 这次改写作用在哪条语句上
///
/// - **选区优先**（与执行族「选区 > 当前语句」同一口径）：非空选区必须整段落在同一条语句里，
///   跨语句就回绝（改哪条判不出）。首尾的空白与分号不算判据（拖一整行常拖到分号）。
/// - 否则看光标：落在某条语句里（含两端紧邻的位置）就用它。
/// - 光标落在语句之外时，**只有脚本末尾**认最后那条（中间只剩空白与分号）——那正是用户
///   正在往下写的位置；两条语句之间的空隙里前后两条都有可能，不猜。
fn target_statement(text: &str, request: &Request) -> Option<SqlStatement> {
    let statements = split_statements(text);

    if let Some((start, end)) = selection_core(text, request) {
        return statements
            .iter()
            .copied()
            .find(|one| one.start <= start && end <= one.end);
    }

    let cursor = clamp_byte(text, request.cursor);
    if let Some(found) = statements
        .iter()
        .copied()
        .find(|one| one.start <= cursor && cursor <= one.end)
    {
        return Some(found);
    }

    let last = statements.last().copied()?;
    let gap = text.get(last.end..cursor)?;
    gap.chars()
        .all(|ch| ch.is_whitespace() || ch == ';')
        .then_some(last)
}

/// 选区的**判据区间**：掐掉首尾的空白与分号；掐完是空的（等于没选）就给 `None`
fn selection_core(text: &str, request: &Request) -> Option<(usize, usize)> {
    let (start, end) = request.selection?;
    let (start, end) = (clamp_byte(text, start), clamp_byte(text, end));
    if end <= start {
        return None;
    }
    let raw = &text[start..end];
    let trimmable = |ch: char| ch.is_whitespace() || ch == ';';
    let core = raw.trim_matches(trimmable);
    if core.is_empty() {
        return None;
    }
    let lead = raw.len() - raw.trim_start_matches(trimmable).len();
    Some((start + lead, start + lead + core.len()))
}

/// 加 `WHERE 1 = 0`：只给**没有 WHERE 的 UPDATE / DELETE**
///
/// 目的不是修 SQL，是给一个"先不匹配任何行"的落点：跑一遍看影响面，确认之后再删掉 `1 = 0`。
fn add_where_action(
    sql: &str,
    spans: &[HighlightSpan],
    insert_at: usize,
    channel: ExecChannel,
) -> Option<CodeAction> {
    if !head_is(spans, sql, "update") && !head_is(spans, sql, "delete") {
        return None;
    }
    if has_word(spans, sql, "where") {
        return None;
    }
    // 末尾不是 WHERE 的合法落点（后面还有别的子句）→ 追加会拼出坏 SQL
    if has_any_word(spans, sql, &AFTER_WHERE) {
        return None;
    }
    safe_tail(sql, spans)?;
    // 这条语句在这条通道上本来就跑不了（加速 / 联邦对源库是只读挂载）→ 改写没有意义
    if statement_allowed(channel, sql).is_err() {
        return None;
    }
    Some(CodeAction {
        id: ADD_WHERE,
        title: "加上 WHERE 1 = 0（先不匹配任何行）".to_string(),
        edit: TextEdit {
            start: insert_at,
            end: insert_at,
            replacement: " WHERE 1 = 0".to_string(),
        },
    })
}

/// 加 `LIMIT 100`：只给**首关键字是 SELECT 且还没有行数上限**的语句
///
/// 追加在语句末尾对 SELECT 是合法落点（`LIMIT` 排在 WHERE / GROUP BY / ORDER BY 之后）；
/// `WITH` 开头的一律不给（可能是数据修改型 CTE，末尾能不能加 `LIMIT` 判不出）。
///
/// 这里**不过通道闸**：头关键字既然是 SELECT，`channel::statement_allowed` 恒放行
/// （它只拦作用源库对象的写语句）。
fn add_limit_action(sql: &str, spans: &[HighlightSpan], insert_at: usize) -> Option<CodeAction> {
    if !head_is(spans, sql, "select") {
        return None;
    }
    // 已经有上限（LIMIT / FETCH / TOP），或者末尾落点不合法（OFFSET / FOR UPDATE / INTO）
    if has_any_word(spans, sql, &AFTER_LIMIT) {
        return None;
    }
    safe_tail(sql, spans)?;
    Some(CodeAction {
        id: ADD_LIMIT,
        title: "加上 LIMIT 100（先看前 100 行）".to_string(),
        edit: TextEdit {
            start: insert_at,
            end: insert_at,
            replacement: " LIMIT 100".to_string(),
        },
    })
}

/// 末尾能不能安全追加子句；不能就回绝
fn safe_tail(sql: &str, spans: &[HighlightSpan]) -> Option<()> {
    if ends_mid_expression(sql, spans) || ends_with_line_comment(sql, spans) {
        return None;
    }
    Some(())
}

/// 末尾还挂着一句没写完（追加子句只会拼出更坏的 SQL）
fn ends_mid_expression(sql: &str, spans: &[HighlightSpan]) -> bool {
    let Some(last) = last_code_token(spans) else {
        return true; // 看不见任何 token（拿不准）→ 当没写完
    };
    let word = last.text(sql);
    match last.class {
        TokenClass::Operator => true,
        // `)` / `]` / `}` 能收尾（`count(*)` / `a[1]` / 复合字面量都是），别的标点不能
        TokenClass::Punctuation => !matches!(word, ")" | "]" | "}"),
        TokenClass::Keyword => DANGLING.iter().any(|one| word.eq_ignore_ascii_case(one)),
        _ => false,
    }
}

/// 语句末尾是不是一条**行注释**：往里插代码会被注释吃掉
/// （`DELETE FROM t -- 删` 之后再追 `WHERE`，那句 `WHERE` 会落进注释里）。
/// 块注释结尾不算：插在 `*/` 之后是干净的。
fn ends_with_line_comment(sql: &str, spans: &[HighlightSpan]) -> bool {
    let Some(last) = spans.iter().max_by_key(|span| span.end) else {
        return false;
    };
    last.class == TokenClass::Comment && last.text(sql).starts_with("--")
}

/// 语句的第一个可见 token 是不是这个关键字（注释不算）
fn head_is(spans: &[HighlightSpan], sql: &str, word: &str) -> bool {
    matches!(
        first_code_token(spans).map(|span| (span.class, span.text(sql))),
        Some((TokenClass::Keyword, text)) if text.eq_ignore_ascii_case(word)
    )
}

/// 语句里有没有这个词（**词法层**：字符串与注释里的同名词不算）
///
/// 按**文本**比而不按 `Keyword` 类比：sqlglot 会把 `FOR` 这类词在某些位置扫成标识符
/// （`SELECT … FOR UPDATE` 里的 `FOR` 就是 `Identifier`），只认 `Keyword` 会漏。
fn has_word(spans: &[HighlightSpan], sql: &str, word: &str) -> bool {
    spans.iter().any(|span| match span.class {
        TokenClass::String | TokenClass::Comment => false,
        _ => span.text(sql).eq_ignore_ascii_case(word),
    })
}

fn has_any_word(spans: &[HighlightSpan], sql: &str, words: &[&str]) -> bool {
    words.iter().any(|word| has_word(spans, sql, word))
}

/// 第一个非注释的 token
fn first_code_token(spans: &[HighlightSpan]) -> Option<&HighlightSpan> {
    spans.iter().find(|span| span.class != TokenClass::Comment)
}

/// 最后一个非注释的 token
fn last_code_token(spans: &[HighlightSpan]) -> Option<&HighlightSpan> {
    spans
        .iter()
        .filter(|span| span.class != TokenClass::Comment)
        .max_by_key(|span| span.end)
}

/// `WHERE` 之后才会出现的子句：末尾不是它们的合法落点
///
/// 保守取法：只要出现就回绝（`UPDATE t SET x = (SELECT … WHERE …)` 这种子查询里的 `WHERE`
/// 也会让我们回绝——那是"宁可这次不给"的方向，见模块头）。
const AFTER_WHERE: [&str; 11] = [
    "order",
    "group",
    "having",
    "limit",
    "offset",
    "fetch",
    "returning",
    "union",
    "except",
    "intersect",
    "qualify",
];

/// `LIMIT` 之后（或者必须排在 `LIMIT` 前）的子句：出现就回绝
const AFTER_LIMIT: [&str; 6] = ["limit", "fetch", "top", "offset", "for", "into"];

/// 末尾挂着它 = 这句还没写完（后面必须跟东西）
///
/// **故意不在里面**的词：`ASC` / `DESC` / `NULL` / `TRUE` / `FALSE` / `DEFAULT` / `END`——
/// 它们能合法收尾（`ORDER BY x DESC`、`SET x = NULL`），列进来会把好好的语句挡掉。
const DANGLING: [&str; 35] = [
    "select", "from", "where", "set", "join", "inner", "left", "right", "full", "cross", "outer",
    "on", "by", "and", "or", "not", "in", "is", "like", "between", "exists", "into", "values",
    "using", "union", "all", "distinct", "when", "then", "else", "case", "group", "order",
    "having", "offset",
];

// ---------------------------------------------------------------------------
// 一次拿齐（测试与工具用）
// ---------------------------------------------------------------------------

/// 三面一次算完
///
/// 三个面**各扫各的**（补全不解词法，诊断与动作各扫一到几遍），因为它们在界面上本来就是
/// **三个时机**：打字（补全 / 诊断）、显式调出（动作）。所以别在每个按键里都调它。
pub fn report(request: &Request) -> ServiceReport {
    ServiceReport {
        completion: complete(request),
        diagnostics: diagnostics(request),
        code_actions: code_actions(request),
    }
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

/// 字节偏移 → **0 基**行列（列按**字符**数）
///
/// 行按 `\n` 数（与内核、`crate::diagnostics` 同一口径），列按字符数（不是字节，也不是
/// UTF-16 码元——纯 ASCII 时三者一致，中文文本下按字符数才对得上用户看到的位置）。
pub fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = clamp_byte(text, offset);
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    (line, text[line_start..offset].chars().count())
}

/// 把字节偏移钳进文本并落到字符边界（内核给的偏移可能越界或落在多字节字符中间）
pub(crate) fn clamp_byte(text: &str, offset: usize) -> usize {
    text.floor_char_boundary(offset.min(text.len()))
}
