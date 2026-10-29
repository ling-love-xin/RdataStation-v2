//! SQL 字面量与标识符转义工具
//!
//! 自 engine `driver::utils` 下沉（2026-10-29，engine 瘦身第二刀前置小手术）：
//! `persistence`（随 rds-store 迁出）与 `driver` 两侧共用，放 shared 避免
//! rds-store → rds-driver 的依赖环。

/// 安全转义 SQL 字符串字面量中的单引号
///
/// 将输入中的 `'` 替换为 `''`，这是 ANSI SQL 标准的转义方式，
/// 适用于所有主流数据库（MySQL/PostgreSQL/SQLite/DuckDB）。
///
/// 同时移除了空字节 `\0`，防止字符串截断攻击。
///
/// # 用法
/// ```ignore
/// let sql = format!("WHERE name = '{}'", escape_sql_string(input));
/// ```
pub fn escape_sql_string(input: &str) -> String {
    input.replace('\'', "''").replace('\0', "")
}

/// 使用数据库方言对应的引号包裹标识符（表名/列名/数据库名）
///
/// 将引号字符在标识符内双写后，用该引号包裹整体。
///
/// | 数据库 | 引号 | 示例 |
/// |--------|------|------|
/// | MySQL | `` ` `` | `` `table``name` `` |
/// | PostgreSQL | `"` | `"table""name"` |
/// | SQLite | `"` | `"table""name"` |
/// | DuckDB | `"` | `"table""name"` |
///
/// # 用法
/// ```ignore
/// let sql = format!("PRAGMA table_info(\"{}\")", quote_identifier(table, '"'));
/// ```
pub fn quote_identifier(input: &str, quote_char: char) -> String {
    let escaped = input.replace(quote_char, &format!("{}{}", quote_char, quote_char));
    format!("{}{}{}", quote_char, escaped, quote_char)
}

#[cfg(test)]
mod tests {
    use super::{escape_sql_string, quote_identifier};

    #[test]
    fn escape_sql_string_doubles_single_quotes_and_drops_nul() {
        assert_eq!(escape_sql_string("it's"), "it''s");
        assert_eq!(escape_sql_string("a\0b"), "ab");
        assert_eq!(escape_sql_string("plain"), "plain");
    }

    #[test]
    fn quote_identifier_doubles_the_quote_char_inside() {
        assert_eq!(quote_identifier("table`name", '`'), "`table``name`");
        assert_eq!(quote_identifier("table\"name", '"'), "\"table\"\"name\"");
        assert_eq!(quote_identifier("simple", '"'), "\"simple\"");
    }
}
