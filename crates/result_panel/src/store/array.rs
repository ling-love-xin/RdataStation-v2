//! Postgres 数组字面量解析（`store/` 第四块，2026-11-26，刀1）
//!
//! 为什么要有：类型只告诉我们"这一列是数组"（[`crate::store::ColumnKind::Array`]），
//! 单元格里拿到的是驱动给的**值文本**（PG 的数组输出语法：`{a,b}` / `{}` /
//! `{{1,2},{3,4}}`）。要复刻 DBeaver 的展开行为（收起态 `755 [+2]`、展开态 `[3]` +
//! 子行 `1.1` / `1.2` / `1.3`，见 `ArrayCellHintProvider.java:48-68` /
//! `GridRowNested.java:64-70`），必须先能把这段文本拆成元素，而且拆得对：
//!
//! - 元素里可以带逗号 / 花括号（PG 会用双引号包起来，内部 `""` 表示一个字面引号）；
//! - 数组里可以有 `NULL` 元素（未加引号的 `NULL` 是 null，加了引号的 `"NULL"` 是字符串）；
//! - 多维数组是嵌一层 `{}`（DBeaver 允许多层 `GridRowNested`，`hasParentArrayRow`）。
//!
//! 边界：**只认 PG 字面量语法，认不出就返回 `None`**，调用方按普通文本处理——
//! 宁可少一次展开，也不把 `{不是数组}` 这类东西硬拆成元素。

/// 数组字面量里的一层元素
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrayItem {
    /// 未加引号的 `NULL`（PG 输出里的 null 元素）
    Null,
    /// 标量（引号已剥、转义已还原；内部统一存字符串，数值语义不在这里判）
    Scalar(String),
    /// 再嵌一层（多维数组）
    Nested(Vec<ArrayItem>),
}

impl ArrayItem {
    /// 元素的展示文本（子行里就是用这个）：`NULL` / 原文 / 用 `{a,b}` 把嵌套拼回去
    pub fn display(&self) -> String {
        match self {
            Self::Null => "NULL".to_string(),
            Self::Scalar(text) => text.clone(),
            Self::Nested(items) => format_array_items(items),
        }
    }
}

/// 把元素列表拼回 PG 字面量（`{a,b}` / `{{1,2},{3,4}}`；空 = `{}`）
///
/// 拼回时**不重新加引号**：这是展示用途（子行 / tooltip），不是发回数据库的 SQL 字面量；
/// 真要发回去得按 PG 的引号规则重新转义，不在本文件职责内。
pub fn format_array_items(items: &[ArrayItem]) -> String {
    let mut out = String::from("{");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&item.display());
    }
    out.push('}');
    out
}

/// 解析 PG 数组字面量（`{...}`）
///
/// 返回 `None` = 不是合法字面量（空串 / 无花括号 / 元素为空 / 尾部有多余字符…），
/// `Some(vec![])` = 空数组 `{}`。前后空白忽略。
pub fn parse_pg_array_literal(text: &str) -> Option<Vec<ArrayItem>> {
    let mut cursor = Cursor::new(text.trim());
    let items = parse_array(&mut cursor)?;
    // 尾部只允许空白（`{a,b} 垃圾` 不是字面量）
    while let Some(byte) = cursor.peek() {
        if byte.is_ascii_whitespace() {
            cursor.at += 1;
        } else {
            return None;
        }
    }
    Some(items)
}

/// 字节游标：结构字符（`{ } , "`）全是 ASCII，切点天然落在 UTF-8 边界上，
/// 所以按下标切片是安全的（元素内容原样进 `String`，不按字节解码）。
struct Cursor<'a> {
    src: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            src: text.as_bytes(),
            at: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    /// 当前是 `byte` 就吃掉并返回 true
    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }
}

fn parse_array(cursor: &mut Cursor) -> Option<Vec<ArrayItem>> {
    if !cursor.eat(b'{') {
        return None;
    }
    if cursor.eat(b'}') {
        return Some(Vec::new());
    }
    // 容忍 `{ }`（人工手填的字面量会带空格；PG 输出不带）
    let mark = cursor.at;
    while cursor.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
        cursor.at += 1;
    }
    if cursor.eat(b'}') {
        return Some(Vec::new());
    }
    cursor.at = mark;

    let mut items = Vec::new();
    loop {
        items.push(parse_item(cursor)?);
        match cursor.bump()? {
            b',' => continue,
            b'}' => break,
            _ => return None,
        }
    }
    Some(items)
}

fn parse_item(cursor: &mut Cursor) -> Option<ArrayItem> {
    match cursor.peek()? {
        b'{' => Some(ArrayItem::Nested(parse_array(cursor)?)),
        b'"' => parse_quoted(cursor),
        _ => parse_bare(cursor),
    }
}

/// 加引号的元素：读到 `"` 结束；`""` 是字面引号；`\"` 也认（COPY 文本格式，部分驱动会给）
fn parse_quoted(cursor: &mut Cursor) -> Option<ArrayItem> {
    cursor.bump()?; // 开引号
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        match cursor.bump()? {
            b'"' => {
                if cursor.peek() == Some(b'"') {
                    cursor.bump();
                    buffer.push(b'"');
                } else {
                    break;
                }
            }
            b'\\' => match cursor.bump() {
                Some(escaped) => buffer.push(escaped),
                None => return None,
            },
            byte => buffer.push(byte),
        }
    }
    let text = String::from_utf8(buffer).ok()?;
    Some(ArrayItem::Scalar(text))
}

/// 未加引号的元素：读到 `,` 或 `}` 为止，trim 后判 `NULL`
fn parse_bare(cursor: &mut Cursor) -> Option<ArrayItem> {
    let start = cursor.at;
    while let Some(byte) = cursor.peek() {
        if byte == b',' || byte == b'}' {
            break;
        }
        cursor.at += 1;
    }
    let raw = std::str::from_utf8(&cursor.src[start..cursor.at])
        .ok()?
        .trim();
    if raw.is_empty() {
        // `{,}`：空元素不是合法数组（PG 不会产生；手填的空元素按非法处理，
        // 避免"看起来展开成两个空格子"的假象）
        return None;
    }
    if raw.eq_ignore_ascii_case("NULL") {
        Some(ArrayItem::Null)
    } else {
        Some(ArrayItem::Scalar(raw.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalars(items: &[ArrayItem]) -> Vec<&str> {
        items
            .iter()
            .map(|item| match item {
                ArrayItem::Scalar(text) => text.as_str(),
                ArrayItem::Null => "<null>",
                ArrayItem::Nested(_) => "<nested>",
            })
            .collect()
    }

    #[test]
    fn parses_flat_literals() {
        assert_eq!(parse_pg_array_literal("{}"), Some(Vec::new()));
        assert_eq!(parse_pg_array_literal("{ }"), Some(Vec::new()));
        let ints = parse_pg_array_literal("{755,847,78}").expect("整数数组");
        assert_eq!(ints.len(), 3);
        assert_eq!(scalars(&ints), vec!["755", "847", "78"]);
        let tags = parse_pg_array_literal("{vip,trial}").expect("文本数组");
        assert_eq!(scalars(&tags), vec!["vip", "trial"]);
    }

    #[test]
    fn trims_padding_and_accepts_spaces() {
        let items = parse_pg_array_literal("  { 1 , 2 }  ").expect("带空白");
        assert_eq!(scalars(&items), vec!["1", "2"]);
    }

    #[test]
    fn keeps_quoted_elements_with_separators() {
        // 引号里可以有逗号、花括号；`""` 是一个字面引号
        let items = parse_pg_array_literal(r#"{"a,b","c""d","{x}"}"#).expect("带引号");
        assert_eq!(scalars(&items), vec!["a,b", "c\"d", "{x}"]);
    }

    #[test]
    fn separates_unquoted_null_from_quoted_null() {
        let items = parse_pg_array_literal(r#"{NULL,"NULL",1}"#).expect("null 元素");
        assert_eq!(scalars(&items), vec!["<null>", "NULL", "1"]);
        assert_eq!(items[0], ArrayItem::Null);
        assert_eq!(items[1], ArrayItem::Scalar("NULL".to_string()));
    }

    #[test]
    fn parses_nested_dimensions() {
        let items = parse_pg_array_literal("{{1,2},{3,4}}").expect("二维");
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0],
            ArrayItem::Nested(vec![
                ArrayItem::Scalar("1".to_string()),
                ArrayItem::Scalar("2".to_string())
            ])
        );
        // 拼回去与输入一致（展示口径）
        assert_eq!(format_array_items(&items), "{{1,2},{3,4}}");
    }

    #[test]
    fn accepts_utf8_elements() {
        let items = parse_pg_array_literal("{破损,尺码不合适}").expect("中文元素");
        assert_eq!(scalars(&items), vec!["破损", "尺码不合适"]);
    }

    #[test]
    fn rejects_non_literals() {
        assert_eq!(parse_pg_array_literal(""), None);
        assert_eq!(parse_pg_array_literal("NULL"), None);
        assert_eq!(parse_pg_array_literal("[1,2,3]"), None);
        assert_eq!(parse_pg_array_literal("{a,b"), None, "缺右括号");
        assert_eq!(parse_pg_array_literal("{a,}"), None, "空元素");
        assert_eq!(parse_pg_array_literal("{,a}"), None, "空元素在头");
        assert_eq!(parse_pg_array_literal("{a}b"), None, "尾随垃圾");
        assert_eq!(parse_pg_array_literal("{\"未闭合}"), None, "引号未闭合");
        assert_eq!(parse_pg_array_literal("{a,,b}"), None, "连续逗号");
    }
}
