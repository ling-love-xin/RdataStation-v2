//! SQL 补全的**候选与上下文**（B9 的纯函数部分）
//!
//! ## 分层
//!
//! 这里只有两件事，都不碰 GPUI、也不做 I/O：
//!
//! 1. [`request_at`]：**光标处要补什么**（表 / 列 / 限定名后的列 / 通用）——判据是光标前的文本；
//! 2. [`candidates`]：在候选目录里按前缀挑、按相关度排、去重、截断。
//!
//! 内核那半（把结果翻译成 LSP `CompletionItem`、装到 `EditorState`）在
//! `view/completion.rs`；候选从哪来（连接级元数据缓存 / 通道限定名）由宿主端口回答
//! （[`CompletionPort`]）。
//!
//! ## 为什么候选要**按通道**给限定名
//!
//! 同一个表在源库档写 `schema.表`、在联邦档写 `别名.schema.表`——**给错候选比不给更糟**
//! （用户会照着写，然后拿到 `Catalog does not exist`）。所以目录由宿主按通道组装好，
//! 编辑器只负责挑（见 `workbench/src/services/editor_completion.rs`）。
//!
//! ## 为什么不做模糊匹配 / 语义分析
//!
//! 补全的第一要务是**不碍事**：前缀匹配 + 大小写不敏感就够了；解析器那套（sqlglot）留给
//! 格式化 / 转译 / 诊断（它们的判错代价不一样）。认不出上下文就退到 [`Request::Any`]，
//! 不猜“你想写什么”。

use std::rc::Rc;

use crate::channel::ExecChannel;

/// 候选类别（决定图标与排序，不决定正确性）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    /// 列（最常用，排前面）
    Column,
    /// 表
    Table,
    /// 视图
    View,
    /// 函数
    Function,
    /// 关键字
    Keyword,
}

impl CandidateKind {
    /// 界面那一列的字（LSP `detail` 之外的一层人话）
    pub fn label(self) -> &'static str {
        match self {
            Self::Column => "列",
            Self::Table => "表",
            Self::View => "视图",
            Self::Function => "函数",
            Self::Keyword => "关键字",
        }
    }
}

/// 一条候选
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 真正补进文本的东西（表的**限定名**、列名、关键字…）
    pub label: String,
    pub kind: CandidateKind,
    /// 列表右侧的说明（表注释 / 列类型 / 函数签名）；没有就不摆
    pub detail: Option<String>,
}

impl Candidate {
    pub fn new(label: impl Into<String>, kind: CandidateKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !detail.is_empty() {
            self.detail = Some(detail);
        }
        self
    }
}

/// 候选目录（**宿主给的快照**：内存读，编辑路径每敲一个字都会问它）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    /// 表 / 视图（`label` **已按当前通道的写法**给好限定名）
    pub objects: Vec<Candidate>,
    /// 列：（表的限定名, 列名, 类型说明）
    pub columns: Vec<(String, String, Option<String>)>,
    /// 到上限被截断了吗（如实标记；界面暂不摆，留给状态/诊断用）
    pub truncated: bool,
}

impl Catalog {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.columns.is_empty()
    }
}

/// 候选来源（宿主注入；未注入 = 只给关键字与函数——**如实不假装有元数据**）
pub trait CompletionPort: 'static {
    /// 这条连接上的候选目录（`conn_id = None` = 未绑定 → 空目录）
    ///
    /// 实现必须**同步 + 内存**：这是编辑路径（每次按键）。真实元数据由宿主在后台预载，
    /// 没载好就返回空——**宁可这次不补，也不在按键里做 I/O**。
    fn catalog(&self, conn_id: Option<&str>, channel: ExecChannel) -> Catalog;

    /// 【B9 切片二】模板片段（宿主从 `sql_template_store` 读；未接 = 空）
    ///
    /// 与 [`Self::catalog`] 同样要求**同步 + 内存**：菜单弹出时会读它，所以实现在启动时
    /// 一次性预载好（见 `workbench/src/services/editor_completion.rs`）。
    /// 默认实现给空：不接模板的宿主照样能用补全其余部分。
    fn templates(&self) -> Vec<TemplateSnippet> {
        Vec::new()
    }
}

/// 共享句柄
pub type CompletionHandle = Rc<dyn CompletionPort>;

/// 【B9 切片二】一条模板片段（内容里的 `{table}` 是占位符，插入后会**被选中**）
///
/// 为什么模板不带「按方言过滤」：`sql_template_store` 里的 `db_type` 是**可空**的，而库里
/// 那 6 条内置全是通用写法——按方言挑剔反而会让用户在 MySQL 连接上看不到模板。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSnippet {
    pub id: String,
    pub name: String,
    pub content: String,
    /// 列表里的说明（可为空）
    pub description: Option<String>,
}

/// 模板内容里第一个 `{table}` 占位符的**字节区间**（相对内容起点）
///
/// 插入之后按它选中那一小段：用户接着敲表名就把它覆写了，不用先去找到那 7 个字符。
pub fn placeholder_range(content: &str) -> Option<(usize, usize)> {
    const MARK: &str = "{table}";
    content
        .find(MARK)
        .map(|start| (start, start + MARK.len()))
}

/// 【B9 切片二】光标前那段标识符的起点（手动触发 `Ctrl+Space` 时的替换范围）
///
/// 只吃标识符字符（**不含 `.`**）：`public.ord` 上按快捷键该补的是 `ord`，
/// 替换掉 `ord` 而留住 `public.`——点了 `public.orders` 之后就得到 `public.orders`。
/// 光标前不是标识符（空格 / 括号 / 行首）就给光标本身（此时是“从光标处开始插”）。
pub fn word_start(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    if !text.is_char_boundary(offset) {
        return offset;
    }
    let mut start = offset;
    while start > 0 {
        let ch = text[..start].chars().next_back().expect("非空");
        if ch.is_alphanumeric() || ch == '_' || ch == '$' {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }
    start
}

/// 光标处要补什么
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// 表 / 视图位（`FROM` / `JOIN` / `INTO` / `UPDATE` / `TABLE` 之后）
    Object { prefix: String },
    /// 列位（`SELECT` / `WHERE` / `ON` / `SET` / `BY` / `HAVING` 之后）
    Column { prefix: String },
    /// `限定名.` 之后：`o.` → 那个表的列；`sqlite_src.` → 那个源的 schema / 表
    Qualified { qualifier: String, prefix: String },
    /// 认不出上下文（句首、运算符之后…）：关键字 + 全量候选
    Any { prefix: String },
}

impl Request {
    /// 已经敲出来的那一段（比较与高亮都用它）
    pub fn prefix(&self) -> &str {
        match self {
            Self::Object { prefix }
            | Self::Column { prefix }
            | Self::Qualified { prefix, .. }
            | Self::Any { prefix } => prefix,
        }
    }
}

/// `FROM` 这类**后面跟表**的关键字（只看紧邻的那个词）
const OBJECT_KEYWORDS: [&str; 10] = [
    "from", "join", "into", "update", "table", "truncate", "describe", "desc", "use", "analyze",
];

/// `SELECT` 这类**后面跟列 / 表达式**的关键字
const COLUMN_KEYWORDS: [&str; 12] = [
    "select", "where", "on", "and", "or", "set", "by", "having", "using", "returning", "case",
    "when",
];

/// 常用关键字（`Any` 与句首用；子句层面的短语按最常用的几条给）
pub const KEYWORDS: [&str; 48] = [
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT JOIN",
    "INNER JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "CROSS JOIN",
    "ON",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "IS NULL",
    "IS NOT NULL",
    "DISTINCT",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "CREATE TABLE",
    "CREATE VIEW",
    "CREATE INDEX",
    "DROP TABLE",
    "ALTER TABLE",
    "TRUNCATE TABLE",
    "WITH",
    "UNION",
    "UNION ALL",
    "EXPLAIN",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
];

/// 常用函数（四个内置方言都有的那一批；**不按方言分支**——那是转译的活）
pub const FUNCTIONS: [&str; 40] = [
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "coalesce",
    "nullif",
    "cast",
    "abs",
    "round",
    "ceil",
    "floor",
    "length",
    "substr",
    "substring",
    "trim",
    "ltrim",
    "rtrim",
    "upper",
    "lower",
    "replace",
    "concat",
    "now",
    "current_date",
    "current_timestamp",
    "date_trunc",
    "extract",
    "strftime",
    "row_number",
    "rank",
    "dense_rank",
    "lag",
    "lead",
    "first_value",
    "last_value",
    "json_extract",
    "array_agg",
    "string_agg",
    "greatest",
    "least",
];

/// 光标处要补什么（**纯函数**：只看 `text[..offset]`）
///
/// `offset` 是**字节**偏移（与内核一致）；越界或不在字符边界时就地钳制（不 panic）。
pub fn request_at(text: &str, offset: usize) -> Request {
    let before = &text[..clamp_to_char_boundary(text, offset)];

    // ① 正在敲的那个词（标识符字符；带引号的标识符把引号也吃进来，但标签不带引号）
    let word_start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_word_char(*ch))
        .map(|(ix, ch)| ix + ch.len_utf8())
        .unwrap_or(0);
    let prefix = before[word_start..].trim_matches('"').to_string();

    // ② `限定名.` 之后？（前缀前面就是 `.`；带引号的限定名也要认得）
    if let Some(dot) = before[..word_start].strip_suffix('.') {
        // 先去掉收尾的引号，再从后往前取名字（`select "orders".` → `orders`）
        let trimmed_end = dot.trim_end_matches(['"', '`']).len();
        let start = dot[..trimmed_end]
            .char_indices()
            .rev()
            .find(|(_, ch)| !is_word_char(*ch) && *ch != '"' && *ch != '`')
            .map(|(ix, ch)| ix + ch.len_utf8())
            .unwrap_or(0);
        let qualifier = dot[start..trimmed_end].trim_matches(['"', '`']).to_string();
        if !qualifier.is_empty() {
            return Request::Qualified { qualifier, prefix };
        }
    }

    // ③ 往前找**最近的一个子句关键字**（隔着 `id,` 这种也要能找到 `select`）
    //
    // 只看紧邻那个词是不够的：`select id, nam` 里紧邻的是 `id`（列名），但那明明是列位。
    // 所以沿着**分号**切出当前语句，从后往前找第一个认识的子句关键字（最多看 12 个词）。
    let head = before[..word_start].trim_end();
    let statement = head.rsplit(';').next().unwrap_or(head);
    let mut seen = 0usize;
    for word in statement
        .split(|ch: char| !is_word_char(ch))
        .filter(|word| !word.is_empty())
        .rev()
    {
        seen += 1;
        if seen > 12 {
            break;
        }
        let word = word.to_ascii_lowercase();
        if OBJECT_KEYWORDS.contains(&word.as_str()) {
            return Request::Object { prefix };
        }
        if COLUMN_KEYWORDS.contains(&word.as_str()) {
            return Request::Column { prefix };
        }
    }
    Request::Any { prefix }
}

/// 在目录里挑候选：前缀命中优先，其次包含命中；同类里按类别排；去重并截断到 `limit`
pub fn candidates(catalog: &Catalog, request: &Request, limit: usize) -> Vec<Candidate> {
    let prefix = request.prefix().to_ascii_lowercase();
    let mut out: Vec<(u8, Candidate)> = Vec::new();

    /// 前缀过滤 + 计分（前缀命中 = 0，包含命中 = 1，无前缀 = 2）；不命中的直接丢
    fn push(out: &mut Vec<(u8, Candidate)>, prefix: &str, candidate: Candidate, needle: &str) {
        let label = candidate.label.to_ascii_lowercase();
        let rank = if prefix.is_empty() {
            2
        } else if needle.to_ascii_lowercase().starts_with(prefix) {
            0
        } else if label.contains(prefix) {
            1
        } else {
            return;
        };
        out.push((rank, candidate));
    }

    match request {
        // `o.` → 那个限定名下的列（限定名匹配表的**全名或最后一段**）
        Request::Qualified { qualifier, .. } => {
            let qualifier = qualifier.to_ascii_lowercase();
            for (table, column, detail) in &catalog.columns {
                if !table_matches(table, &qualifier) {
                    continue;
                }
                push(&mut out, &prefix, column_candidate(column, detail), column);
            }
            // `源.` 之后也可能是 schema / 表（联邦档的 `mysql_src.`）
            for object in &catalog.objects {
                if object.label.to_ascii_lowercase().starts_with(&format!("{qualifier}.")) {
                    let tail = object.label.split('.').skip(1).collect::<Vec<_>>().join(".");
                    push(&mut out, &prefix, Candidate::new(tail, object.kind), &object.label);
                }
            }
            // 限定名认不出来（大概率是语句里的**表别名** `o.`，我们不解析 SQL）→ 给全部列。
            // 宁可多给（用户自己认），也不给一个错的子集。
            if out.is_empty() {
                for (_, column, detail) in &catalog.columns {
                    push(&mut out, &prefix, column_candidate(column, detail), column);
                }
            }
        }
        Request::Object { .. } => {
            for object in &catalog.objects {
                push(&mut out, &prefix, object.clone(), last_segment(&object.label));
            }
        }
        Request::Column { .. } => {
            for (_, column, detail) in &catalog.columns {
                push(&mut out, &prefix, column_candidate(column, detail), column);
            }
            for function in FUNCTIONS {
                push(
                    &mut out,
                    &prefix,
                    Candidate::new(function, CandidateKind::Function),
                    function,
                );
            }
        }
        Request::Any { .. } => {
            for object in &catalog.objects {
                push(&mut out, &prefix, object.clone(), last_segment(&object.label));
            }
            for (_, column, _) in &catalog.columns {
                push(
                    &mut out,
                    &prefix,
                    Candidate::new(column.clone(), CandidateKind::Column),
                    column,
                );
            }
            for function in FUNCTIONS {
                push(
                    &mut out,
                    &prefix,
                    Candidate::new(function, CandidateKind::Function),
                    function,
                );
            }
            for keyword in KEYWORDS {
                push(
                    &mut out,
                    &prefix,
                    Candidate::new(keyword, CandidateKind::Keyword),
                    keyword,
                );
            }
        }
    }

    // 前缀命中 → 类别（列 / 表 / 视图 / 函数 / 关键字）→ 短的在前（**只在真有前缀时**：
    // `ord` 命中时 `orders` 比 `order_items` 更可能是用户要的；无前缀的全量列表则按字典序，
    // 那更好找）→ 字典序；同标签只留一条
    out.sort_by(|(rank_a, a), (rank_b, b)| {
        rank_a
            .cmp(rank_b)
            .then(a.kind.cmp(&b.kind))
            .then_with(|| {
                if *rank_a == 0 {
                    a.label.len().cmp(&b.label.len())
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| a.label.cmp(&b.label))
    });
    let mut seen: Vec<String> = Vec::new();
    let mut result: Vec<Candidate> = Vec::new();
    for (_, candidate) in out {
        if seen.iter().any(|label| label == &candidate.label) {
            continue;
        }
        seen.push(candidate.label.clone());
        result.push(candidate);
        if result.len() >= limit {
            break;
        }
    }
    result
}

/// 列候选（带类型说明）
fn column_candidate(column: &str, detail: &Option<String>) -> Candidate {
    match detail {
        Some(detail) => Candidate::new(column.to_string(), CandidateKind::Column)
            .with_detail(detail.clone()),
        None => Candidate::new(column.to_string(), CandidateKind::Column),
    }
}

/// 表的限定名与用户敲的前缀对不对得上（全名或最后一段）
fn table_matches(table: &str, qualifier: &str) -> bool {
    let table = table.to_ascii_lowercase();
    table == qualifier || last_segment(&table) == qualifier || table.ends_with(&format!(".{qualifier}"))
}

/// 限定名的最后一段（`schema.表` → `表`；`别名.schema.表` → `表`）
fn last_segment(qualified: &str) -> &str {
    qualified.rsplit('.').next().unwrap_or(qualified)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

/// 把字节偏移钳到字符边界（内核给的偏移）并在越界时就地回退
fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        Candidate, CandidateKind, Catalog, Request, candidates, placeholder_range, request_at,
        word_start,
    };

    /// 光标放在句尾：`request_at(&format!("{sql}|"), sql.len())` 的简写
    fn at(sql: &str) -> Request {
        request_at(sql, sql.len())
    }

    #[test]
    fn the_context_says_what_is_being_completed() {
        assert_eq!(
            at("select * from ord"),
            Request::Object {
                prefix: "ord".to_string()
            }
        );
        assert_eq!(
            at("SELECT * FROM orders o JOIN cus"),
            Request::Object {
                prefix: "cus".to_string()
            }
        );
        assert_eq!(
            at("select id, nam"),
            Request::Column {
                prefix: "nam".to_string()
            }
        );
        assert_eq!(
            at("select * from t where sta"),
            Request::Column {
                prefix: "sta".to_string()
            }
        );
        // 限定名之后（带引号 / 不带都认）
        assert_eq!(
            at("select o."),
            Request::Qualified {
                qualifier: "o".to_string(),
                prefix: String::new()
            }
        );
        assert_eq!(
            at("select \"orders\".to"),
            Request::Qualified {
                qualifier: "orders".to_string(),
                prefix: "to".to_string()
            }
        );
        // 逗号 / 括号之后仍按**最近的子句关键字**判：`select a,(` 还在选择列表里 → 列位
        assert_eq!(
            at("select a,("),
            Request::Column {
                prefix: String::new()
            }
        );
        // 一句里没有认识的关键字 → 不猜，退到 Any
        assert_eq!(
            at("with x as ("),
            Request::Any {
                prefix: String::new()
            }
        );
        assert_eq!(
            at("sel"),
            Request::Any {
                prefix: "sel".to_string()
            }
        );
    }

    #[test]
    fn offsets_are_clamped_to_char_boundaries() {
        // 中文 SQL：偏移落在汉字中间时不能 panic（就地回退到上一个边界）
        let sql = "select 名称";
        let request = request_at(sql, sql.len() - 1);
        assert!(matches!(request, Request::Column { .. } | Request::Any { .. }));
        // 越界
        let _ = request_at(sql, 9999);
    }

    fn catalog() -> Catalog {
        Catalog {
            objects: vec![
                Candidate::new("public.orders", CandidateKind::Table).with_detail("订单"),
                Candidate::new("public.order_items", CandidateKind::Table),
                Candidate::new("public.customers", CandidateKind::View),
            ],
            columns: vec![
                ("public.orders".to_string(), "id".to_string(), None),
                (
                    "public.orders".to_string(),
                    "total".to_string(),
                    Some("numeric".to_string()),
                ),
                (
                    "public.order_items".to_string(),
                    "order_id".to_string(),
                    None,
                ),
            ],
            truncated: false,
        }
    }

    #[test]
    fn objects_are_matched_by_their_last_segment() {
        let found = candidates(
            &catalog(),
            &at("select * from ord"),
            50,
        );
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["public.orders", "public.order_items"],
            "按最后一段前缀命中，且短的在前：{labels:?}"
        );
        assert!(found.iter().all(|c| c.kind == CandidateKind::Table));
    }

    #[test]
    fn a_qualifier_narrows_to_that_tables_columns() {
        // 限定名认得出来（全名 / 最后一段）→ 只给那个表的列
        let found = candidates(&catalog(), &at("select orders."), 50);
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["id", "total"], "限定名按最后一段匹配：{labels:?}");
        assert_eq!(found[1].detail.as_deref(), Some("numeric"), "列类型带上");

        // 全名也行（`public.orders.`）
        let found = candidates(&catalog(), &at("select public.orders."), 50);
        assert_eq!(found.len(), 2);

        // 认不出来（大概率是语句里的表别名 `o.`）→ 给全部列，并给出提示性说明
        let found = candidates(&catalog(), &at("select o."), 50);
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["id", "order_id", "total"], "别名给全量：{labels:?}");
    }

    #[test]
    fn an_alias_qualifier_offers_the_sources_schemas() {
        // 联邦档：`mysql_src.` → 该源的 schema.表（去掉别名前缀）
        let catalog = Catalog {
            objects: vec![Candidate::new("mysql_src.orders_db.orders", CandidateKind::Table)],
            columns: vec![],
            truncated: false,
        };
        let found = candidates(&catalog, &at("select * from mysql_src."), 50);
        assert_eq!(found[0].label, "orders_db.orders");
    }

    #[test]
    fn column_and_keyword_lists_come_out_for_generic_positions() {
        let found = candidates(&catalog(), &at("sel"), 100);
        let has_select = found.iter().any(|c| c.label == "SELECT");
        assert!(has_select, "句首该有关键字：{:?}", found.len());

        let found = candidates(&catalog(), &at("select * from t where ord"), 100);
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"order_id"), "列位该给列：{labels:?}");
        assert!(
            !labels.contains(&"ORDER BY"),
            "列位不给关键字（关键字只在 Any 与句首）：{labels:?}"
        );
    }

    #[test]
    fn results_are_deduped_and_capped() {
        let duplicated = Catalog {
            objects: vec![
                Candidate::new("public.orders", CandidateKind::Table),
                Candidate::new("public.orders", CandidateKind::Table),
            ],
            columns: vec![],
            truncated: false,
        };
        let found = candidates(&duplicated, &at("select * from ord"), 50);
        assert_eq!(found.len(), 1, "同标签只留一条");

        let capped = candidates(&catalog(), &Request::Any { prefix: String::new() }, 3);
        assert_eq!(capped.len(), 3, "上限生效");
    }

    /// 【B9 切片二】手动触发时替换哪一段：光标前的标识符（**不含 `.`**）
    #[test]
    fn the_word_start_marks_what_manual_completion_should_replace() {
        let text = "select id, total from public.ord";
        assert_eq!(word_start(text, text.len()), text.len() - 3, "`ord`");
        assert_eq!(word_start(text, 29), 29, "`public.` 之后就空了（`.` 不算标识符）");
        // 光标在 `public` 里面：整个词都算进去（它才是当前在敲的那个词）
        assert_eq!(word_start(text, 28), 22, "`public` 从 22 开始");
        let spaced = "select * from ";
        assert_eq!(word_start(spaced, spaced.len()), spaced.len(), "空格后就从光标开始插");
        assert_eq!(word_start("", 0), 0);
        assert_eq!(word_start("select 1", 999), 7, "越界钳到文本末尾，词是 `1`");
        // 中文标识符（可当列名用）不该被切坏
        let cjk = "select 名字 from t";
        assert_eq!(word_start(cjk, 10), 7, "`名字` 6 字节，从 7 开始");
        // 落在多字节字符中间：不动它（宁可把替换范围放光标处）
        assert_eq!(word_start(cjk, 8), 8);
    }

    /// 【B9 切片二】模板里的 `{table}` 要能被选中（没占位符就不选）
    #[test]
    fn the_table_placeholder_is_located_within_the_template() {
        assert_eq!(
            placeholder_range("SELECT * FROM {table};"),
            Some((14, 21))
        );
        assert_eq!(placeholder_range("SELECT 1;"), None);
        // 只认第一个：替换一个就够，剩下的交给用户
        assert_eq!(placeholder_range("{table} {table}"), Some((0, 7)));
    }
}
