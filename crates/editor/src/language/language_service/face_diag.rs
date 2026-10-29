//! 诊断面（diagnostics + 词法辅助）
//!
//! 自 language/language_service.rs 拆分（2026-10-29 Step 9）：按能力域切分。

use engine::sql::{
    CatalogTable, HighlightSpan, TokenClass, diagnose, highlight_spans,
    split_statements,
};

use crate::completion::{CandidateKind, Catalog};
use super::*;

pub fn diagnostics(request: &Request) -> Vec<Diagnostic> {
    let text = request.text;
    if text.len() > SCAN_MAX_BYTES || text.trim().is_empty() {
        return Vec::new();
    }

    let mut found = dollar_quote_diagnostics(text);
    found.extend(paren_diagnostics(text));
    // 整篇扫不动：词法层只说"扫不动"，说不出是哪儿——那正是下面这条要补的那点信息
    if highlight_spans(text).is_empty() {
        if let Some(one) = unterminated_diagnostic(text) {
            found.push(one);
        }
    }

    // 语义层：有表目录才做（未知表 / 未知列）。目录为空 = 调用方没有元数据
    // （scan / 未绑定连接）→ 跳过，与 engine::sql::diagnose 的无目录退化同口径
    if !request.catalog.is_empty() {
        let tables = catalog_to_semantic(request.catalog);
        for issue in diagnose(text, request.dialect, &tables) {
            found.push(Diagnostic::new(issue.code, &issue.message, issue.start, issue.end));
        }
    }

    found.sort_by(|a, b| (a.start, a.end, a.code).cmp(&(b.start, b.end, b.code)));
    found
}

/// 补全目录 → 语义诊断的表目录（只取表/视图类候选；列按限定名聚到表下）
///
/// 目录列出的 `table` 限定名与 `objects.label` 同形（宿主按当前通道写好），
/// 列找不到所属表就丢弃（目录截断场景宁缺毋滥，不产出孤儿列诊断）。
fn catalog_to_semantic(catalog: &Catalog) -> Vec<CatalogTable<'_>> {
    catalog
        .objects
        .iter()
        .filter(|c| matches!(c.kind, CandidateKind::Table | CandidateKind::View))
        .map(|obj| CatalogTable {
            qualified_name: &obj.label,
            columns: catalog
                .columns
                .iter()
                .filter(|(table, _, _)| table == &obj.label)
                .map(|(_, column, _)| column.as_str())
                .collect(),
        })
        .collect()
}

/// 括号不平衡（**按语句判**：语句是执行的单位，整篇配平可能只是两条错互相抵消）
///
/// 一条语句只报一处：第一处之后就说不清后面的括号该跟谁配对了。
fn paren_diagnostics(text: &str) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    for statement in split_statements(text) {
        let sql = statement.text(text);
        let spans = highlight_spans(sql);
        // 这一句自己就扫不动（未闭合的字符串 / 注释）→ 交给那条规则，不在这里猜
        if spans.is_empty() {
            continue;
        }
        if let Some(one) = paren_diagnostic(sql, &spans, statement.start) {
            found.push(one);
        }
    }
    found
}

/// 一条语句里的第一处括号不平衡（偏移回到**文档**坐标系）
fn paren_diagnostic(sql: &str, spans: &[HighlightSpan], base: usize) -> Option<Diagnostic> {
    let mut open: Vec<(usize, usize)> = Vec::new();
    for span in spans {
        // 只有标点类的 `(` / `)` 算括号：字符串与注释里的括号在词法层已经是别的类
        if span.class != TokenClass::Punctuation {
            continue;
        }
        match span.text(sql) {
            "(" => open.push((span.start, span.end)),
            ")" => {
                if open.pop().is_none() {
                    return Some(Diagnostic::new(
                        "stray-close-paren",
                        "这个「)」没有配对的「(」",
                        base + span.start,
                        base + span.end,
                    ));
                }
            }
            _ => {}
        }
    }
    // 还有没闭合的：指**最靠里的那个**（用户多半正在写它，范围也最小最有用）
    let (start, end) = open.last().copied()?;
    Some(Diagnostic::new(
        "unbalanced-paren",
        "这个「(」没有闭合",
        base + start,
        base + end,
    ))
}

/// 未闭合的美元引用（**token 级**：`$tag$…` 开了头却没以同一个标签收尾）
///
/// 为什么要这一条：词法层有时会把 `$$abc` 直接接住成一个字符串 token（不是失败），
/// 于是"整篇扫不动"那条规则看不到它。判据仍然只用词法层给的 token（不看 token 内部），
/// 所以不会和词法层打架。
fn dollar_quote_diagnostics(text: &str) -> Vec<Diagnostic> {
    highlight_spans(text)
        .iter()
        .filter_map(|span| {
            // 只有这两种类可能出现 `$tag$` 开头：字符串（`$$a`），标识符（`$tag$a`）
            if !matches!(span.class, TokenClass::String | TokenClass::Identifier) {
                return None;
            }
            let raw = span.text(text);
            let tag = dollar_tag(raw)?;
            // 收尾了就不算：token 里既要有开标签又要有闭标签（`$$$$` 这种空体也算收尾）
            if raw.len() >= tag.len() * 2 && raw.ends_with(tag) {
                return None;
            }
            Some(Diagnostic::new(
                "unterminated-dollar-quote",
                "美元引用没有闭合（开头的标签之后没再出现同一个标签）",
                span.start,
                span.start + tag.len(),
            ))
        })
        .collect()
}

/// 整篇扫不动时定位那个没闭合的构造
///
/// 推理链（词法是**顺序**扫的）：扫不动说明某个 opener 一直没闭合；它之前的构造都是闭合的
/// （否则早就在那儿失败了），它之后的每个位置都还"在它里面"。于是——
///
/// > 从文末往前找 opener，**第一个"它之前的前缀扫得动"的那个**就是它。
///
/// 每问一次词法都要扫一遍前缀，所以有 [`PROBE_BUDGET`] 上限：费不起就一条都不报（不猜）。
fn unterminated_diagnostic(text: &str) -> Option<Diagnostic> {
    let mut probes = 0usize;
    for (start, _) in text.char_indices().rev() {
        let Some(opener) = Opener::at(text, start) else {
            continue;
        };
        probes += 1;
        if probes > PROBE_BUDGET {
            return None;
        }
        // 这个 opener 之前就已经扫不动了 → 不是它，往更早找
        if !prefix_scans(text, start) {
            continue;
        }
        // 后面还有能合上的东西，整篇却扫不动 → 不是"没闭合"这一条管的事，不猜
        if has_closer(text, start) {
            return None;
        }
        return Some(Diagnostic::new(
            opener.code,
            opener.message,
            start,
            start + opener.len,
        ));
    }
    None
}

/// 未闭合构造的开头
struct Opener {
    code: &'static str,
    message: &'static str,
    /// 开头的字节长度（诊断范围就指这一段）
    len: usize,
}

impl Opener {
    /// `start` 处是不是一个能开启构造的东西
    fn at(text: &str, start: usize) -> Option<Self> {
        let rest = text.get(start..)?;
        let bytes = rest.as_bytes();
        let first = *bytes.first()?;
        match first {
            b'\'' => Some(Self {
                code: "unterminated-string",
                message: "字符串字面量没有闭合",
                len: 1,
            }),
            b'"' | b'`' => Some(Self {
                code: "unterminated-quoted-identifier",
                message: "引号标识符没有闭合",
                len: 1,
            }),
            b'/' if bytes.get(1) == Some(&b'*') => Some(Self {
                code: "unterminated-comment",
                message: "块注释没有闭合",
                len: 2,
            }),
            b'$' => Some(Self {
                code: "unterminated-dollar-quote",
                message: "美元引用没有闭合",
                len: dollar_tag(rest)?.len(),
            }),
            _ => None,
        }
    }
}

/// `text[..end]` 这段前缀**词法层扫得动**吗（纯空白没有 token，也算扫得动）
fn prefix_scans(text: &str, end: usize) -> bool {
    let prefix = &text[..end];
    prefix.trim().is_empty() || !highlight_spans(prefix).is_empty()
}

/// `start` 处的 opener 之后还有没有闭合符（**保守检查**：有就不报）
///
/// 判据与 `engine::sql::split` 的引号口径一致：双写算转义、**不认反斜杠**；
/// 块注释按 PostgreSQL 语义可嵌套。这一步只做"还能不能合上"的判断，不做词法。
fn has_closer(text: &str, start: usize) -> bool {
    let bytes = text.as_bytes();
    match bytes.get(start) {
        Some(&b'\'') | Some(&b'"') | Some(&b'`') => {
            let quote = bytes[start];
            let mut index = start + 1;
            while index < bytes.len() {
                if bytes[index] == quote {
                    if bytes.get(index + 1) == Some(&quote) {
                        index += 2; // 双写 = 转义，还在字面量里
                        continue;
                    }
                    return true;
                }
                index += 1;
            }
            false
        }
        Some(&b'/') => {
            let mut depth = 0usize;
            let mut index = start;
            while index < bytes.len() {
                match (bytes[index], bytes.get(index + 1)) {
                    (b'/', Some(&b'*')) => {
                        depth += 1;
                        index += 2;
                    }
                    (b'*', Some(&b'/')) => {
                        depth = depth.saturating_sub(1);
                        index += 2;
                        if depth == 0 {
                            return true;
                        }
                    }
                    _ => index += 1,
                }
            }
            false
        }
        Some(&b'$') => match dollar_tag(&text[start..]) {
            Some(tag) => text
                .get(start + tag.len()..)
                .is_some_and(|rest| rest.contains(tag)),
            None => false,
        },
        _ => false,
    }
}

/// 开头的美元标签（`$$` 或 `$tag$`）；不是标签就 `None`（`$1` 这类参数不会被认成引用）
fn dollar_tag(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'$') {
        return None;
    }
    let mut index = 1usize;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
        index += 1;
    }
    (bytes.get(index) == Some(&b'$')).then(|| &text[..index + 1])
}

// ---------------------------------------------------------------------------
// 面三：code action（改写型动作）
// ---------------------------------------------------------------------------
