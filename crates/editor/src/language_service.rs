//! 编辑器的**语言服务面**：补全 / 诊断 / code action **三面同源**（纯函数层，不碰 GPUI）
//!
//! ## 为什么收成一个面（而不是三套）
//!
//! 三面要判的**是同一件事的不同投影**：都得先回答"这段文本在词法上是什么"——哪个词是关键字、
//! `where` 是写在字符串里还是真的子句、那个 `(` 是代码还是字面量的一部分。只是问答不同：
//!
//! | 面 | 问的问题 | 取用请求里的哪几段 |
//! | --- | --- | --- |
//! | [`complete`] | 光标处要补什么 | 文本 + 光标 + 目录 + 连接 |
//! | [`diagnostics`] | 这篇文本哪里写不下去 | 文本（**整篇**，与光标无关） |
//! | [`code_actions`] | 光标 / 选区那条语句能怎么安全改写 | 文本 + 光标 + 选区 + 通道 |
//!
//! 收在一处的三个理由：
//!
//! 1. **一致性**：同一段文本在三面得到同一个词法结论。各扫各的，迟早出现"补全认得 `where`
//!    是关键字、诊断却把字符串里的 `where` 当成子句"这种分歧（那种 bug 极难查）；
//! 2. **一处演进**：加一条诊断 / 加一条改写只动这个模块；词法或目录口径变了，三面一起跟上；
//! 3. **UI 零驱动分支**：界面只构造一次请求（[`Request`]）、拿一个 [`ServiceReport`]，
//!    不需要知道"这个库能补什么、能报什么"——dbflux 把三面收进一个 `LanguageService`
//!    交给驱动，同一个理由（界面不判驱动类型）。
//!
//! ## 词法只有一个来源
//!
//! 凡是要 token 的地方一律问 [`highlight_spans`]（sqlglot tokenizer；与高亮 / 折叠 /
//! 通道闸同一份词法）：字符串里的 `where` 不算子句、引号里的 `(` 不算括号、注释里的 `limit`
//! 不算已有上限。**只有它整篇扫不动的时候**（词法失败 → 空区间）本模块才退到字节层的
//! 保守检查，而那一步只会让结论**更弱**（找不到闭合符才报），不会凭空造出结论。
//!
//! ## 三面各自的边界（判不出就不报）
//!
//! - **补全**：上下文判定与挑排**不在这里重写**，只转发给 [`crate::completion`]（`request_at`
//!   + `candidates`）。**未绑定连接**时不采用目录里那部分——没有目标库就谈不上"按通道给
//!   限定名"，宁可这次只给关键字与函数（与 `CompletionPort` 的契约同源，见 [`complete`]）。
//! - **诊断**：**只有词法层判得动的错**——未闭合的引号字面量 / 引号标识符 / 块注释 /
//!   美元引用，以及括号不平衡。不解析语法树，不报"这里该有个 FROM"这种要文法才说得清的话：
//!   半写着的 SQL 天天在编辑器里，押注式报错会把注意力耗光。报不出位置就一条都不报。
//! - **code action**：**只有目标位置确定时才给**（改哪一段、换成什么都要说得出）。目标是
//!   "光标（选区优先）所在的**那一整条语句**"，判据与执行族同源（`split_statements`）；
//!   两条动作都是**追加在语句末尾**，所以末尾挂不稳（还挂着运算符 / `FROM` / `(`）、
//!   末尾是行注释（插进去会被注释吃掉）、或者这条语句在这条通道上根本跑不了（通道闸会拒）
//!   时一律**回绝**。
//!
//! ## 有意的边界（都有测试钉住）
//!
//! - **不认方言**：`LIMIT` / `#` 注释 / `TOP` 这些方言差异本模块不判（源方言要从连接来，
//!   见 [`crate::translate`] 的口径）；`#` 之后的东西按 `engine::sql::split` 的同一选择
//!   当成代码而不是注释。
//! - **未闭合的美元引用按 token 判**：词法层有时会把 `$$abc` 接住成一个字符串 token
//!   （不是失败），我们按 token 的形状判它没闭合；词法层自己没扫出来的怪字符，一条都不报。
//! - **括号按语句判**，一条语句只报一处（后面的错互相牵连，报多了是噪音）。
//! - 超过 [`SCAN_MAX_BYTES`] 不扫（与折叠 / 着色同一量级：大文件不每键全扫）。

use engine::sql::{HighlightSpan, SqlStatement, TokenClass, highlight_spans, split_statements};

use crate::channel::{ExecChannel, statement_allowed};
use crate::completion::{self, Candidate, Catalog};

/// 补全面最多给几条（与 `view/completion.rs` 的 `MAX_ITEMS` 同量级：够选就行）
pub const MAX_CANDIDATES: usize = 60;

/// 超过这个字节数不做打字期扫描（诊断 / 动作都走这个门槛）
pub const SCAN_MAX_BYTES: usize = 1_000_000;

/// 定位未闭合构造时最多问几次词法（每次都要扫一遍前缀，预算要小）
const PROBE_BUDGET: usize = 12;

/// 一次语言服务请求
///
/// 三面共用这一个形状：界面从自己手上已有的东西（编辑内核的文本 / 光标 / 选区、宿主给的
/// 目录与绑定）一次摆好，**不必为每个面各造一份输入**——那正是"驱动分支"的入口。
#[derive(Debug)]
pub struct Request<'a> {
    /// 整篇文档文本（下面所有字节偏移都在这个坐标系里）
    pub text: &'a str,
    /// 光标（字节偏移；越界或落在多字节字符中间会就地钳到字符边界）
    pub cursor: usize,
    /// 选区（字节区间；`None` = 没有选区）。首尾的空白与分号不算判据
    pub selection: Option<(usize, usize)>,
    /// 候选目录快照（宿主按 (连接, 通道) 组装好；见 [`completion::CompletionPort`]）
    pub catalog: &'a Catalog,
    /// 文档绑定的连接 id（`None` = 跟随当前连接）
    pub connection: Option<&'a str>,
    /// 执行通道（限定名按它给 + 写语句在它上面可能被拒）
    pub channel: ExecChannel,
}

/// 一次请求的三类结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceReport {
    pub completion: Vec<Candidate>,
    pub diagnostics: Vec<Diagnostic>,
    pub code_actions: Vec<CodeAction>,
}

/// 一条打字期诊断（**字节区间**，行列为派生）
///
/// 没有 `severity` 字段：本模块只报有把握的错，拿不准的一律不报，所以没有第二档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// 稳定标识（接线时直接给 `lsp_types::Diagnostic::code`；测试按它断言）
    pub code: &'static str,
    /// 说给人听的一句（说的是**词法层**看到的事，不假装是语法分析）
    pub message: String,
    pub start: usize,
    pub end: usize,
}

impl Diagnostic {
    fn new(code: &'static str, message: &str, start: usize, end: usize) -> Self {
        Self {
            code,
            message: message.to_string(),
            start,
            end,
        }
    }

    /// 落进内核诊断要的行列：`((行, 列), (行, 列))`，**0 基**、列按**字符**数
    ///
    /// 与 `crate::diagnostics` 的 1 基显示口径同一套换算（那里给人看，这里给 `Position`）。
    pub fn positions(&self, text: &str) -> ((usize, usize), (usize, usize)) {
        (line_column(text, self.start), line_column(text, self.end))
    }
}

/// 一段改写：把 `[start, end)` 换成 `replacement`（`start == end` = 纯插入）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// 一条改写型动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    /// 稳定标识（菜单排序、接线、测试都用它；与标题分开，标题随时可以改文案）
    pub id: &'static str,
    /// 菜单上那一行
    pub title: String,
    pub edit: TextEdit,
}

/// 加 `WHERE 1 = 0`：先不匹配任何行，确认影响面之后再改掉
const ADD_WHERE: &str = "add-where-match-nothing";
/// 加 `LIMIT 100`：先看一眼前 100 行
const ADD_LIMIT: &str = "add-limit-100";

// ---------------------------------------------------------------------------
// 面一：补全
// ---------------------------------------------------------------------------

/// 光标处要补什么
///
/// **上下文判定与挑排全部转发给 [`crate::completion`]**，这里只做两件事：钳坐标、把
/// "未绑定连接就不摆元数据候选"这条口径钉在纯函数层。
pub fn complete(request: &Request) -> Vec<Candidate> {
    let text = request.text;
    let offset = clamp_byte(text, request.cursor);
    let ask = completion::request_at(text, offset);

    // 未绑定连接 = 没有目标库：目录里那部分一律不采用。宿主的端口实现本来就按这条口径
    // 返回空目录（见 `workbench/src/services/editor_completion.rs`），在这里再钉一次是让
    // "没有目标库就不摆元数据候选"这件事**在纯函数层可测**，而不是只活在端口契约的注释里。
    let empty = Catalog::default();
    let catalog = match request.connection {
        Some(_) => request.catalog,
        None => &empty,
    };

    completion::candidates(catalog, &ask, MAX_CANDIDATES)
}

// ---------------------------------------------------------------------------
// 面二：诊断（词法层判得动的那些）
// ---------------------------------------------------------------------------

/// 打字期诊断（按位置升序；**整篇都判**，与光标无关）
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

    found.sort_by(|a, b| (a.start, a.end, a.code).cmp(&(b.start, b.end, b.code)));
    found
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

/// 光标（**选区优先**）所在那条语句能怎么改写；判不出目标位置就空着
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
fn clamp_byte(text: &str, offset: usize) -> usize {
    text.floor_char_boundary(offset.min(text.len()))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        ADD_LIMIT, ADD_WHERE, Catalog, CodeAction, Diagnostic, ExecChannel, Request, code_actions,
        complete, diagnostics, line_column, report,
    };
    use crate::completion::{Candidate, CandidateKind};

    /// 起手：光标在文末、绑了连接、源库通道、无选区（测试里按需改字段）
    fn ask<'a>(text: &'a str, catalog: &'a Catalog) -> Request<'a> {
        Request {
            text,
            cursor: text.len(),
            selection: None,
            catalog,
            connection: Some("P_orders"),
            channel: ExecChannel::Source,
        }
    }

    /// 诊断只要 code（位置与文案另有测试盯着）
    fn codes(text: &str) -> Vec<&'static str> {
        diagnostics(&ask(text, &Catalog::default()))
            .into_iter()
            .map(|one| one.code)
            .collect()
    }

    fn found(text: &str) -> Vec<Diagnostic> {
        diagnostics(&ask(text, &Catalog::default()))
    }

    /// 动作（默认源库通道、光标在文末）
    fn actions(text: &str, cursor: usize) -> Vec<CodeAction> {
        code_actions(&Request {
            cursor,
            ..ask(text, &Catalog::default())
        })
    }

    fn ids(text: &str, cursor: usize) -> Vec<&'static str> {
        actions(text, cursor)
            .into_iter()
            .map(|one| one.id)
            .collect()
    }

    fn catalog_with_orders() -> Catalog {
        Catalog {
            objects: vec![Candidate::new("public.orders", CandidateKind::Table)],
            columns: vec![(
                "public.orders".to_string(),
                "id".to_string(),
                Some("INT".to_string()),
            )],
            truncated: false,
        }
    }

    // ---------------- 诊断：该报 ----------------

    #[test]
    fn unterminated_literals_and_comments_are_reported() {
        // (文本, 该报的 code)
        let cases: [(&str, &[&str]); 9] = [
            ("SELECT 'abc", &["unterminated-string"]),
            ("SELECT 1;\nSELECT 'abc", &["unterminated-string"]),
            ("SELECT \"abc", &["unterminated-quoted-identifier"]),
            ("SELECT `abc", &["unterminated-quoted-identifier"]),
            ("SELECT /* abc", &["unterminated-comment"]),
            ("SELECT 1 /* a\n/* b */ ", &["unterminated-comment"]),
            ("SELECT $tag$abc", &["unterminated-dollar-quote"]),
            ("SELECT $$abc", &["unterminated-dollar-quote"]),
            ("SELECT (a", &["unbalanced-paren"]),
        ];
        for (text, expected) in cases {
            assert_eq!(codes(text), expected.to_vec(), "{text}");
        }
    }

    #[test]
    fn stray_close_parens_are_reported_at_the_first_offender() {
        assert_eq!(codes("SELECT a)"), vec!["stray-close-paren"]);
        assert_eq!(codes("SELECT a)) "), vec!["stray-close-paren"]);
        let one = found("SELECT 1)) ").remove(0);
        assert_eq!(one.start, "SELECT 1".len(), "指第一处多出来的 )");
    }

    #[test]
    fn the_unmatched_open_paren_is_the_innermost_one() {
        let text = "SELECT (a, (b";
        let one = found(text).remove(0);
        assert_eq!(one.code, "unbalanced-paren");
        assert_eq!(one.start, text.rfind('(').expect("有"));
        assert_eq!(one.end, one.start + 1);
    }

    #[test]
    fn the_unterminated_thing_is_the_last_opener_not_the_first_quote() {
        // 第一个 `'` 是闭合的（`'fine'`）：扫不动的是第二个
        let text = "SELECT 'fine' FROM t;\nSELECT 'abc";
        let one = found(text).remove(0);
        assert_eq!(one.code, "unterminated-string");
        assert_eq!(one.start, text.rfind('\'').expect("有"));
        assert_eq!(one.end, one.start + 1);
    }

    #[test]
    fn a_quote_inside_a_comment_is_not_an_unterminated_string() {
        // 注释里的撇号：整篇扫得动（词法层把它当注释），不该报
        assert!(codes("SELECT 1 -- 别写成 don't\n").is_empty());
        assert!(codes("SELECT 1 /* don't */").is_empty());
    }

    #[test]
    fn parens_are_judged_per_statement() {
        // 整篇配平、单条不配平：按语句判才看得见（`;` 两边各自是执行的单位）
        assert_eq!(
            codes("SELECT (a;\nSELECT b);"),
            vec!["unbalanced-paren", "stray-close-paren"]
        );
    }

    // ---------------- 诊断：不该报 ----------------

    #[test]
    fn clean_sql_gets_no_diagnostics() {
        let cases = [
            "",
            "   \n\t ",
            ";;;",
            "-- 只有注释\n",
            "/* 只有注释 */",
            "SELECT 1",
            "SELECT 'it''s' AS s",
            "SELECT '中文' AS 名称 FROM 订单",
            "SELECT count(*) FROM t",
            "SELECT (a), (b) FROM t WHERE x IN (1, 2)",
            "SELECT '(' AS c",
            "SELECT * FROM t -- (",
            "SELECT /* ( */ 1",
            "SELECT $$abc$$",
            "SELECT $tag$abc$tag$",
            "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;",
            "SELECT 1 + 1;\nUPDATE t SET x = 1;\nDELETE FROM t WHERE id = 3;",
        ];
        for text in cases {
            assert!(codes(text).is_empty(), "不该报：{text:?}");
        }
    }

    #[test]
    fn a_parameter_is_not_a_dollar_quote() {
        // `$1` 是占位符，不是美元引用的开头
        assert!(codes("SELECT * FROM t WHERE id = $1").is_empty());
    }

    #[test]
    fn diagnostics_are_sorted_and_carry_positions() {
        let text = "SELECT 'a';\nSELECT (b;\nSELECT 'c";
        let all = found(text);
        assert_eq!(
            all.iter().map(|one| one.code).collect::<Vec<_>>(),
            vec!["unbalanced-paren", "unterminated-string"]
        );
        for pair in all.windows(2) {
            assert!(pair[0].start <= pair[1].start, "{pair:?}");
        }
        // `positions` 给的是 0 基行列（内核 `Position` 直接用）：第 2 行（下标 1）的第 8 个字符
        assert_eq!(all[0].positions(text), ((1, 7), (1, 8)));
        assert_eq!(line_column(text, 0), (0, 0));
        assert_eq!(line_column(text, text.len()), (2, 9));
    }

    #[test]
    fn huge_documents_are_not_scanned() {
        let text = format!("{}SELECT 'abc", "SELECT 1;\n".repeat(200_000));
        assert!(text.len() > super::SCAN_MAX_BYTES);
        assert!(codes(&text).is_empty(), "超过门槛就不扫（宁可一条不报）");
    }

    // ---------------- code action：给 ----------------

    #[test]
    fn a_select_without_a_cap_gets_a_limit() {
        let text = "SELECT * FROM orders";
        let out = actions(text, text.len());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].id, ADD_LIMIT);
        assert_eq!(out[0].edit.start, text.len(), "追加在语句末尾");
        assert_eq!(out[0].edit.end, text.len(), "纯插入：范围零宽");
        assert_eq!(out[0].edit.replacement, " LIMIT 100");
    }

    #[test]
    fn the_limit_lands_before_the_semicolon_and_outside_literals() {
        // 语句末尾不含 `;`：追加落在分号前
        let text = "SELECT 1;";
        let out = actions(text, 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].edit.start, 8);
        assert_eq!(
            format!("{}{};", &text[..8], out[0].edit.replacement),
            "SELECT 1 LIMIT 100;"
        );

        // 字面量里的分号不会切句（切分器与词法是同一份口径）
        let text = "SELECT 'a;b' FROM t";
        let out = actions(text, 8);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].edit.start, text.len());
    }

    #[test]
    fn an_unfiltered_update_or_delete_gets_the_safety_where() {
        for (text, cursor) in [
            ("UPDATE t SET x = 1", 0),
            ("DELETE FROM t", 12),
            ("DELETE FROM t /* 备注 */", 5),
        ] {
            let out = actions(text, cursor);
            assert_eq!(out.len(), 1, "{text}");
            assert_eq!(out[0].id, ADD_WHERE, "{text}");
            assert_eq!(out[0].edit.start, text.len(), "{text}");
            assert_eq!(out[0].edit.replacement, " WHERE 1 = 0", "{text}");
        }
    }

    #[test]
    fn a_word_inside_a_literal_or_comment_is_not_a_clause() {
        // 词法层说了算：字符串 / 块注释里的 `where` 不算已有 WHERE → 照样给动作
        for text in [
            "UPDATE t SET note = 'where'",
            "UPDATE t SET x = 1 /* where */",
        ] {
            assert_eq!(ids(text, 0), vec![ADD_WHERE], "{text}");
        }
        // 反过来：字面量里的 `limit` 不算已有上限 → 照样给 LIMIT
        assert_eq!(
            ids("SELECT 'limit' AS note FROM t", 0),
            vec![ADD_LIMIT],
            "字面量里的 limit 不算上限"
        );
    }

    #[test]
    fn the_selection_wins_over_the_cursor() {
        let text = "SELECT 1;\nSELECT * FROM t;";
        let second = text.find("SELECT *").expect("有");
        let out = code_actions(&Request {
            cursor: 0,
            selection: Some((second, text.len())),
            ..ask(text, &Catalog::default())
        });
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].id, ADD_LIMIT);
        assert_eq!(
            out[0].edit.start, 25,
            "改的是选中的第二条（末尾分号不算判据，追加落在它前面）"
        );
    }

    #[test]
    fn a_cursor_after_the_last_statement_targets_it() {
        let text = "SELECT 1;\nSELECT * FROM t;\n";
        let out = actions(text, text.len());
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].edit.start, 25, "中间只剩空白与分号 → 认最后那条");
    }

    #[test]
    fn a_cursor_inside_the_statement_includes_its_edges() {
        let text = "SELECT * FROM t";
        assert_eq!(ids(text, 0), vec![ADD_LIMIT], "语句起点");
        assert_eq!(ids(text, text.len()), vec![ADD_LIMIT], "语句末尾");
        assert_eq!(ids(text, 8), vec![ADD_LIMIT], "语句中间");
    }

    // ---------------- code action：回绝 ----------------

    #[test]
    fn the_limit_action_declines_what_it_cannot_place() {
        let cases: [(&str, &str); 14] = [
            ("SELECT * FROM t LIMIT 10", "已经有上限"),
            ("SELECT TOP 10 * FROM t", "T-SQL 的 TOP"),
            ("SELECT * FROM t FETCH FIRST 10 ROWS ONLY", "FETCH"),
            ("SELECT * FROM t OFFSET 10", "OFFSET 必须排在 LIMIT 前"),
            ("SELECT * FROM t FOR UPDATE", "FOR UPDATE 也得排在 LIMIT 前"),
            ("SELECT * FROM t INTO OUTFILE 'x'", "INTO 的落点判不出"),
            (
                "SELECT * FROM t UNION ALL SELECT * FROM u LIMIT 1",
                "UNION 那条已有上限",
            ),
            (
                "WITH x AS (SELECT 1) SELECT * FROM x",
                "首关键字不是 SELECT",
            ),
            ("INSERT INTO t VALUES (1)", "不是 SELECT"),
            ("SELECT * FROM", "末尾挂着 FROM（还没写完）"),
            ("SELECT * FROM t,", "末尾是逗号"),
            ("SELECT * FROM t WHERE x =", "末尾是运算符"),
            ("SELECT", "只有一个 SELECT"),
            ("SELECT * FROM t -- 尾巴", "末尾是行注释（插进去会被吃掉）"),
        ];
        for (text, why) in cases {
            assert!(
                !ids(text, text.len()).contains(&ADD_LIMIT),
                "{text}（{why}）"
            );
        }
        // 这几条连别的动作也不该有（不是 UPDATE / DELETE，也不是能改写的 SELECT）
        for text in ["INSERT INTO t VALUES (1)", "SELECT * FROM", "SELECT"] {
            assert!(actions(text, text.len()).is_empty(), "{text}");
        }
    }

    #[test]
    fn the_where_action_declines_what_it_cannot_place() {
        let cases: [(&str, &str); 11] = [
            ("UPDATE t SET x = 1 WHERE id = 2", "已经有 WHERE"),
            ("DELETE FROM t WHERE 1 = 1", "已经有 WHERE"),
            ("UPDATE t SET x = 1 ORDER BY id", "ORDER BY 在 WHERE 之后"),
            ("DELETE FROM t LIMIT 1", "LIMIT 在 WHERE 之后"),
            ("DELETE FROM t RETURNING id", "RETURNING 在 WHERE 之后"),
            (
                "UPDATE t SET x = (SELECT 1 FROM u WHERE u.id = 1)",
                "子查询里有 WHERE（保守回绝）",
            ),
            ("SELECT * FROM t", "不是 UPDATE / DELETE"),
            ("INSERT INTO t VALUES (1)", "不是 UPDATE / DELETE"),
            ("WITH x AS (SELECT 1) DELETE FROM t", "首关键字不是 DELETE"),
            ("DELETE FROM", "末尾挂着 FROM"),
            ("UPDATE t SET", "末尾挂着 SET"),
        ];
        for (text, why) in cases {
            assert!(
                !ids(text, text.len()).contains(&ADD_WHERE),
                "{text}（{why}）"
            );
        }
        // 首关键字不对 / 还没写完的那几条：连 LIMIT 也不该有（一个动作都没有）
        for text in [
            "INSERT INTO t VALUES (1)",
            "WITH x AS (SELECT 1) DELETE FROM t",
            "DELETE FROM",
            "UPDATE t SET",
        ] {
            assert!(actions(text, text.len()).is_empty(), "{text}");
        }
    }

    #[test]
    fn an_unfinished_statement_gets_no_action() {
        // 还在写字符串 / 注释：连首关键字都说不准
        assert!(actions("SELECT 'abc", 0).is_empty());
        assert!(actions("DELETE FROM t /* 备注", 0).is_empty());
    }

    #[test]
    fn a_rewrite_is_not_offered_on_a_channel_where_the_statement_cannot_run() {
        let text = "DELETE FROM t";
        let on_local = code_actions(&Request {
            channel: ExecChannel::Accelerated,
            ..ask(text, &Catalog::default())
        });
        assert!(
            on_local.is_empty(),
            "本地加速通道上这条 DELETE 会被通道闸拒，改写没有意义"
        );
        let on_source = code_actions(&Request {
            channel: ExecChannel::Source,
            ..ask(text, &Catalog::default())
        });
        assert_eq!(on_source.len(), 1, "源库通道上照给");

        // SELECT 在任何通道上都跑得动（通道闸只拦写源库对象的语句）→ 不受影响
        let select = "SELECT * FROM t";
        let on_local = code_actions(&Request {
            channel: ExecChannel::Accelerated,
            ..ask(select, &Catalog::default())
        });
        assert_eq!(on_local.len(), 1);
    }

    #[test]
    fn a_cursor_between_statements_targets_nothing() {
        let text = "SELECT 1;\nSELECT * FROM t;\n";
        let between = text.find('\n').expect("有");
        assert!(
            actions(text, between).is_empty(),
            "两条语句之间的空隙：前后都可能，不猜"
        );
        assert!(actions("", 0).is_empty(), "空文档");
        assert!(actions("   \n", 2).is_empty(), "只有空白");
        assert!(actions("-- 只有注释\n", 3).is_empty(), "只有注释");
    }

    #[test]
    fn a_selection_that_spans_statements_targets_nothing() {
        let text = "SELECT 1;\nSELECT 2;";
        let out = code_actions(&Request {
            selection: Some((0, text.len())),
            ..ask(text, &Catalog::default())
        });
        assert!(out.is_empty(), "跨语句的选区：改哪条判不出");
    }

    // ---------------- 面一：补全 ----------------

    #[test]
    fn the_completion_face_asks_the_completion_module() {
        let catalog = catalog_with_orders();
        let out = complete(&ask("SELECT * FROM ord", &catalog));
        assert!(
            out.iter().any(|one| one.label == "public.orders"),
            "{out:?}"
        );
    }

    #[test]
    fn metadata_candidates_are_not_taken_without_a_bound_connection() {
        let catalog = catalog_with_orders();
        let bound = complete(&ask("SELECT * FROM ord", &catalog));
        assert!(bound.iter().any(|one| one.label == "public.orders"));

        let unbound = complete(&Request {
            connection: None,
            ..ask("SELECT * FROM ord", &catalog)
        });
        assert!(
            !unbound.iter().any(|one| one.label == "public.orders"),
            "未绑定连接 = 没有目标库：不摆元数据候选（如实，不假装有）"
        );

        // 没元数据 ≠ 没有补全：关键字照给
        let keywords = complete(&Request {
            connection: None,
            ..ask("sel", &catalog)
        });
        assert!(
            keywords.iter().any(|one| one.label == "SELECT"),
            "{keywords:?}"
        );
    }

    #[test]
    fn out_of_range_offsets_are_clamped_not_panicked() {
        let catalog = catalog_with_orders();
        // 越界偏移 + 落在中文中间（`名` 的第二、三个字节）
        let text = "SELECT 名称 FROM ord";
        for cursor in [0, 1, 8, 9, text.len() + 99] {
            let _ = complete(&Request {
                cursor,
                ..ask(text, &catalog)
            });
        }
    }

    // ---------------- 一次拿齐 ----------------

    #[test]
    fn the_report_carries_all_three_faces() {
        let catalog = catalog_with_orders();
        let out = report(&ask("DELETE FROM ord", &catalog));
        assert!(out.diagnostics.is_empty(), "这句词法上没问题");
        assert_eq!(
            out.completion.first().map(|one| one.label.as_str()),
            Some("public.orders")
        );
        assert_eq!(out.code_actions.len(), 1);
        assert_eq!(out.code_actions[0].id, ADD_WHERE);

        // 扫不动的那句：诊断有、动作没有（连"是不是 DELETE"都说不准）
        let broken = report(&ask("DELETE FROM ord 'abc", &catalog));
        assert_eq!(broken.diagnostics.len(), 1, "{:?}", broken.diagnostics);
        assert!(broken.code_actions.is_empty());
    }

    #[test]
    fn the_same_lexis_answers_all_three_faces() {
        // 一句话把三面同源摆出来：`'where'` 是字符串 → 不产生诊断、也照样挡不住动作；
        // 而真有 `WHERE` 时动作就没了
        let catalog = catalog_with_orders();
        let out = report(&ask("DELETE FROM t WHERE note = 'where'", &catalog));
        assert!(out.diagnostics.is_empty());
        assert!(out.code_actions.is_empty(), "已经有 WHERE");

        let out = report(&ask("DELETE FROM t", &catalog));
        assert_eq!(out.code_actions.len(), 1);
        assert_eq!(out.code_actions[0].id, ADD_WHERE);
    }
}
