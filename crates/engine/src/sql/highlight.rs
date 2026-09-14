//! SQL 词法高亮（tokenizer 驱动，**不需要 tree-sitter**）
//!
//! ## 为什么用 sqlglot 的 tokenizer
//!
//! 1. **依赖已经存在**：`core/sql` 是 sqlglot-rust 的唯一接入点，tokenizer 随它一起进来
//!    （`sqlglot_rust::tokens`），不必再引入 grammar 包、highlights 查询与构建产物；
//! 2. **高亮只需要词法层**：关键字 / 字面量 / 注释 / 占位符的着色不需要语法树；
//! 3. **编辑中的 SQL 常常写不完**：语法树会解析失败，词法扫描仍能给出可用着色。
//!
//! ## 产出
//!
//! `Vec<HighlightSpan>`（**字节区间 + 类别**）。颜色与主题由视图层决定——本模块不认识颜色、
//! 不做渲染，与 `split` 一样是纯函数。
//!
//! ## 已知限制（均有测试固定行为）
//!
//! - **词法失败即返回空**（如未闭合字符串）：视图层退化为无高亮，不 panic、不截断内容。
//!   真机核对后若不可接受，可复用 `split` 的状态机补一层"粗粒度兜底着色"。
//! - **方言不参与**：sqlglot 的 `Tokenizer` 构造只接受文本；各方言关键字都能被识别，
//!   但少数方言特有 token 会落入 `Keyword`。
//! - **函数名是启发式**：词法层分不清类型名与函数名，按"标识符紧跟 `(`" 判定。

use sqlglot_rust::tokens::{Token, TokenType, Tokenizer};

/// 高亮类别（视图层映射到主题的语法色板）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    /// 关键字（SELECT / FROM / WHERE / JOIN …）
    Keyword,
    /// 类型名（INT / VARCHAR / TIMESTAMPTZ …）
    Type,
    /// 函数名（`count(` / `sum(` …；启发式判定）
    Function,
    /// 字符串字面量（含引号本身）
    String,
    /// 数字字面量
    Number,
    /// 注释（行注释 / 块注释）
    Comment,
    /// 占位符（`$1` / `:name` / `?`）
    Parameter,
    /// 标识符（表名 / 列名 / 别名）
    Identifier,
    /// 运算符（`+ - * / = <> || ::` …）
    Operator,
    /// 标点（`( ) , ; .` …）
    Punctuation,
}

/// 一段高亮区间（字节偏移，始终落在字符边界上）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub class: TokenClass,
}

impl HighlightSpan {
    /// 取该区间的原文
    pub fn text<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.start..self.end]
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// 常见内置类型名（用于把 `Type` 从关键字里分出来）
const TYPE_NAMES: &[&str] = &[
    "int",
    "integer",
    "bigint",
    "smallint",
    "tinyint",
    "serial",
    "bigserial",
    "decimal",
    "numeric",
    "real",
    "double",
    "float",
    "boolean",
    "bool",
    "char",
    "varchar",
    "nvarchar",
    "text",
    "blob",
    "bytea",
    "date",
    "time",
    "timestamp",
    "timestamptz",
    "interval",
    "json",
    "jsonb",
    "uuid",
    "money",
    "inet",
];

/// 计算 SQL 的高亮区间（按出现顺序、互不重叠）
pub fn highlight_spans(sql: &str) -> Vec<HighlightSpan> {
    let tokens = match Tokenizer::with_comments(sql).tokenize() {
        Ok(tokens) => tokens,
        // 词法失败（未闭合字符串等）：不 panic、不猜内容，交给视图层退化为纯文本
        Err(_) => return Vec::new(),
    };

    let mut spans: Vec<HighlightSpan> = Vec::new();
    let mut cursor = 0usize;

    for token in &tokens {
        let Some((start, end)) = locate(sql, cursor, token) else {
            continue;
        };
        cursor = end;

        if let Some(class) = class_of(token) {
            if start < end && sql.is_char_boundary(start) && sql.is_char_boundary(end) {
                spans.push(HighlightSpan { start, end, class });
            }
        }
    }

    mark_functions(sql, &mut spans);
    spans
}

/// 从 `cursor` 起定位 token 的字节区间
///
/// 不直接使用 `Token::position`：该字段的基准（字节 / 字符）未在文档中承诺，
/// 而"从游标向后查找 token 原文"与基准无关，且能容忍 tokenizer 跳过空白。
fn locate(sql: &str, cursor: usize, token: &Token) -> Option<(usize, usize)> {
    if token.value.is_empty() {
        return None;
    }
    let rest = sql.get(cursor..)?;
    let idx = rest.find(token.value.as_str())?;

    let mut start = cursor + idx;
    let mut end = start + token.value.len();

    // 带引号的 token：value 可能是"去引号后的内容"，把引号一起纳入着色范围
    if token.quote_char != '\0' {
        let quote = token.quote_char as u8;
        let bytes = sql.as_bytes();
        if start > 0 && bytes[start - 1] == quote {
            start -= 1;
        }
        if end < bytes.len() && bytes[end] == quote {
            end += 1;
        }
    }

    Some((start, end))
}

/// 判定 token 的高亮类别
///
/// 只有字面量 / 注释 / 占位符 / 标识符按 `TokenType` 判定（这几类语义明确）；
/// 其余按**文本形态**分类——枚举 sqlglot 的两百多个关键字变体既脆弱又无收益。
fn class_of(token: &Token) -> Option<TokenClass> {
    match token.token_type {
        TokenType::LineComment | TokenType::BlockComment => return Some(TokenClass::Comment),
        TokenType::String
        | TokenType::NationalString
        | TokenType::BitString
        | TokenType::HexString => return Some(TokenClass::String),
        TokenType::Number => return Some(TokenClass::Number),
        TokenType::Parameter => return Some(TokenClass::Parameter),
        TokenType::Identifier => return Some(TokenClass::Identifier),
        TokenType::Whitespace | TokenType::Eof => return None,
        _ => {}
    }

    let value = token.value.as_str();
    if value.chars().all(|c| c.is_ascii_punctuation()) {
        // 纯符号：括号 / 逗号 / 分号 / 点是标点，其余按运算符
        return Some(match value {
            "(" | ")" | "[" | "]" | "{" | "}" | "," | ";" | "." => TokenClass::Punctuation,
            _ => TokenClass::Operator,
        });
    }

    if TYPE_NAMES.contains(&value.to_ascii_lowercase().as_str()) {
        return Some(TokenClass::Type);
    }

    Some(TokenClass::Keyword)
}

/// 函数名启发式：标识符紧接着 `(` → 改判为函数
fn mark_functions(sql: &str, spans: &mut [HighlightSpan]) {
    let bytes = sql.as_bytes();
    for span in spans.iter_mut() {
        if span.class != TokenClass::Identifier {
            continue;
        }
        let mut i = span.end;
        while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) == Some(&b'(') {
            span.class = TokenClass::Function;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(sql: &str) -> Vec<(String, TokenClass)> {
        highlight_spans(sql)
            .iter()
            .map(|s| (s.text(sql).to_string(), s.class))
            .collect()
    }

    fn class_of_text(sql: &str, needle: &str) -> Option<TokenClass> {
        classes(sql)
            .into_iter()
            .find(|(text, _)| text == needle)
            .map(|(_, class)| class)
    }

    #[test]
    fn keywords_are_classified() {
        let sql = "SELECT a FROM t WHERE b = 1";
        assert_eq!(class_of_text(sql, "SELECT"), Some(TokenClass::Keyword));
        assert_eq!(class_of_text(sql, "FROM"), Some(TokenClass::Keyword));
        assert_eq!(class_of_text(sql, "WHERE"), Some(TokenClass::Keyword));
    }

    #[test]
    fn string_span_includes_quotes() {
        let sql = "SELECT 'hello' AS greeting";
        let spans = highlight_spans(sql);
        let s = spans
            .iter()
            .find(|s| s.class == TokenClass::String)
            .expect("string span");
        assert_eq!(s.text(sql), "'hello'");
    }

    #[test]
    fn number_and_parameter_are_classified() {
        let sql = "SELECT * FROM t LIMIT 10";
        assert_eq!(class_of_text(sql, "10"), Some(TokenClass::Number));

        // 占位符用 MySQL 风格的 `?`（不依赖方言特有写法，避免不同词法分支）
        let sql = "SELECT * FROM t WHERE id = ?";
        assert_eq!(class_of_text(sql, "?"), Some(TokenClass::Parameter));
    }

    #[test]
    fn comments_are_classified() {
        // 注释 token 的 value 可能包含/不包含行尾，断言用前缀而不写死全文
        let sql = "SELECT 1 -- 说明\n";
        let spans = highlight_spans(sql);
        let line = spans
            .iter()
            .find(|s| s.class == TokenClass::Comment)
            .expect("行注释应有高亮区间");
        assert!(line.text(sql).starts_with("--"), "{}", line.text(sql));

        let sql = "SELECT /* 块注释 */ 1";
        let spans = highlight_spans(sql);
        let block = spans
            .iter()
            .find(|s| s.class == TokenClass::Comment)
            .expect("块注释应有高亮区间");
        assert!(block.text(sql).starts_with("/*"), "{}", block.text(sql));
    }

    #[test]
    fn identifiers_and_functions_are_distinguished() {
        let sql = "SELECT count(*) FROM orders";
        assert_eq!(class_of_text(sql, "count"), Some(TokenClass::Function));
        assert_eq!(class_of_text(sql, "orders"), Some(TokenClass::Identifier));
    }

    #[test]
    fn types_are_separated_from_keywords() {
        let sql = "CREATE TABLE t (id INT, name VARCHAR(20))";
        assert_eq!(class_of_text(sql, "INT"), Some(TokenClass::Type));
        assert_eq!(class_of_text(sql, "VARCHAR"), Some(TokenClass::Type));
        assert_eq!(class_of_text(sql, "TABLE"), Some(TokenClass::Keyword));
    }

    #[test]
    fn punctuation_and_operators_are_classified() {
        let sql = "SELECT a, b FROM t WHERE a = 1";
        assert_eq!(class_of_text(sql, ","), Some(TokenClass::Punctuation));
        assert_eq!(class_of_text(sql, "="), Some(TokenClass::Operator));
    }

    #[test]
    fn spans_are_ascending_and_non_overlapping() {
        let sql = "SELECT a, count(*) FROM t WHERE x = 'v' -- c";
        let spans = highlight_spans(sql);
        assert!(!spans.is_empty());
        for pair in spans.windows(2) {
            assert!(pair[0].end <= pair[1].start, "{pair:?}");
        }
    }

    #[test]
    fn multibyte_text_keeps_char_boundaries() {
        let sql = "SELECT '中文' AS 名称 FROM 订单 WHERE 城市 = '北京'";
        let spans = highlight_spans(sql);
        assert!(!spans.is_empty());
        for span in &spans {
            assert!(sql.is_char_boundary(span.start), "{span:?}");
            assert!(sql.is_char_boundary(span.end), "{span:?}");
        }
        assert_eq!(class_of_text(sql, "'中文'"), Some(TokenClass::String));
    }

    #[test]
    fn unterminated_input_degrades_without_panic() {
        for sql in [
            "SELECT 'abc",
            "SELECT /* unclosed",
            "SELECT \"abc",
            "SELECT $$abc",
            "",
        ] {
            let spans = highlight_spans(sql);
            for span in &spans {
                assert!(sql.is_char_boundary(span.start) && sql.is_char_boundary(span.end));
                assert!(span.start < span.end);
            }
        }
    }

    #[test]
    fn span_helpers_report_length_and_text() {
        let sql = "SELECT 1";
        let spans = highlight_spans(sql);
        let first = spans[0];
        assert_eq!(first.text(sql), "SELECT");
        assert_eq!(first.len(), 6);
        assert!(!first.is_empty());
    }
}
