//! 默认表名 / 文件名（词法级解析）——自 export.rs 拆分（2026-10-29）
//!
//! 从 SQL 文本里猜首个 `FROM` 表名（[`table_from_sql`]）→ 默认表名（[`default_table_name`]）
//! → 默认文件名（[`default_file_name`]）；猜不出回退 `result_N`。

use super::ExportFormat;
/// 默认表名：从结果集的 SQL 里猜首个 `FROM` 表，猜不出用 `result_<序号>`
///
/// 词法级（不认识 AST）：`FROM (SELECT …)` 这种子查询**放弃猜**并回退到 `result_N`——
/// 猜错表名比给个中性的默认名更糟（用户会照着一个不存在的表去插）。
pub fn default_table_name(sql: Option<&str>, index: usize) -> String {
    sql.and_then(table_from_sql)
        .unwrap_or_else(|| format!("result_{index}"))
}

/// 默认文件名（带扩展名）：`orders.csv` / `result_1.csv`
pub fn default_file_name(sql: Option<&str>, index: usize, format: ExportFormat) -> String {
    format!("{}.{}", default_table_name(sql, index), format.extension())
}

/// 从 SQL 文本里取首个 `FROM` 后面的表名（词法级）
///
/// - 大小写不敏感，`FROM` 必须是独立单词（`performance.fromage` 不算）
/// - 去掉 schema / catalog 前缀（`public.orders` → `orders`）
/// - 后面紧跟 `(` 的当子查询，返回 `None`
pub fn table_from_sql(sql: &str) -> Option<String> {
    let bytes = sql.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        // 找 `FROM`（按字节扫：SQL 里的关键字都是 ASCII）
        if !bytes[ix].eq_ignore_ascii_case(&b'f') {
            ix += 1;
            continue;
        }
        if ix + 4 > bytes.len() || !bytes[ix..ix + 4].eq_ignore_ascii_case(b"from") {
            ix += 1;
            continue;
        }
        let before_ok = ix == 0 || !is_ident_byte(bytes[ix - 1]);
        let after = ix + 4;
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if !before_ok || !after_ok {
            ix += 1;
            continue;
        }
        let rest = &sql[after..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with('(') {
            // 子查询 / 表函数：`FROM (SELECT …)` —— 不猜
            return None;
        }
        return table_name_from(trimmed);
    }
    None
}

/// 从 `FROM` 后面那段文本里读出表名（可带 `schema.` 前缀，可为带引号的标识符）
///
/// 逐段读：`public.orders` / `"sch"."Orders"` 都读到最后一段——**最后一段才是表名**
/// （INSERT 的目标名用的是表本身，不带 schema）。
fn table_name_from(text: &str) -> Option<String> {
    let mut rest = text;
    let mut last: Option<String> = None;
    loop {
        let segment = if let Some(closing) = rest
            .chars()
            .next()
            .filter(|ch| matches!(ch, '"' | '`' | '['))
            .map(|quote| if quote == '[' { ']' } else { quote })
        {
            // 带引号的标识符：读到闭合引号（要记住多消耗掉的那一个）
            let mut name = String::new();
            let mut consumed = rest.chars().next().map(char::len_utf8).unwrap_or(0);
            for ch in rest.chars().skip(1) {
                consumed += ch.len_utf8();
                if ch == closing {
                    break;
                }
                name.push(ch);
            }
            rest = &rest[consumed..];
            name
        } else {
            let name: String = rest.chars().take_while(|ch| is_ident_char(*ch)).collect();
            rest = &rest[name.len()..];
            name
        };
        if segment.is_empty() {
            break;
        }
        last = Some(segment);
        let after_dot = rest.trim_start();
        match after_dot.strip_prefix('.') {
            Some(tail) => rest = tail.trim_start(),
            None => break,
        }
    }
    last
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

#[cfg(test)]
mod tests {
    // 安全模式：显式列举（避免与 gpui-kit 测试宏冲突）
    use super::{ExportFormat, default_file_name, default_table_name, table_from_sql};
    #[test]
    fn table_name_comes_from_the_first_from() {
        assert_eq!(
            table_from_sql("SELECT * FROM orders WHERE id = 1").as_deref(),
            Some("orders")
        );
        assert_eq!(
            table_from_sql("select * from public.orders o").as_deref(),
            Some("orders"),
            "schema 前缀去掉"
        );
        assert_eq!(
            table_from_sql("SELECT * FROM \"Orders\"").as_deref(),
            Some("Orders"),
            "引号要去掉"
        );
        assert_eq!(
            table_from_sql("SELECT * FROM `orders` o").as_deref(),
            Some("orders"),
            "MySQL 的反引号同样认"
        );
        assert_eq!(
            table_from_sql("SELECT * FROM \"public\".\"Orders\"").as_deref(),
            Some("Orders"),
            "逐段读、取最后一段"
        );
        assert_eq!(
            table_from_sql("SELECT 1 FROM\n  orders").as_deref(),
            Some("orders"),
            "换行与空白没关系"
        );
        // 子查询：猜不出就不猜（宁可用 result_N）
        assert!(table_from_sql("SELECT * FROM (SELECT 1) x").is_none());
        // 没有 FROM / 只是名字里带 from
        assert!(table_from_sql("SELECT 1").is_none());
        assert!(table_from_sql("SELECT fromage FROM t").as_deref() == Some("t"));
    }

    #[test]
    fn default_names_fall_back_to_result_index() {
        assert_eq!(
            default_table_name(Some("SELECT * FROM orders"), 1),
            "orders"
        );
        assert_eq!(default_table_name(Some("SELECT 1"), 2), "result_2");
        assert_eq!(default_table_name(None, 3), "result_3");
        assert_eq!(
            default_file_name(Some("SELECT * FROM orders"), 1, ExportFormat::Json),
            "orders.json"
        );
        assert_eq!(
            default_file_name(None, 2, ExportFormat::Insert),
            "result_2.sql"
        );
    }
}
