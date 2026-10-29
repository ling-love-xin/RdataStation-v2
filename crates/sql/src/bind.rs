//! 参数绑定（DBeaver 式 `:name`）：扫描 + 字面替换纯函数。
//!
//! 口径（backlog §二「DBeaver 式参数绑定」方案要点）：
//! - 只在 SQL **正文**识别 `:name`（单冒号 + 标识符），排除：
//!   - `::`（PostgreSQL 类型转换，`a::int` 不是参数）
//!   - 单引号字符串里的 `:`（那是数据不是参数；`''` 转义按 SQL 处理）
//!   - `--` 行注释 / `/* */` 块注释里的 `:`
//! - 替换一律转成字符串字面量（`'...'`，内部单引号 `''` 转义）；
//!   空值替换为 `NULL`。不做类型推断（与 DBeaver 的类型化参数不同，
//!   本方案是「字面替换后执行」）。

use std::collections::HashMap;

/// 一次扫描出的参数出现位置（同一名字多处出现 = 多个 span，替换时全部换掉）
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParamSpan {
    start: usize,
    end: usize,
    name: String,
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 词法状态机扫出所有 `:name` 区间（跳过字符串 / 行注释 / 块注释 / `::`）
fn collect_spans(sql: &str) -> Vec<ParamSpan> {
    let bytes = sql.as_bytes();
    let n = bytes.len();
    let mut spans = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut line_comment = false;
    let mut block_comment = false;
    while i < n {
        let c = bytes[i];
        if block_comment {
            if c == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if line_comment {
            if c == b'\n' {
                line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_string {
            // `''` 是转义的单引号（SQL 约定），连着跳两个
            if c == b'\'' {
                if i + 1 < n && bytes[i + 1] == b'\'' {
                    i += 2;
                } else {
                    in_string = false;
                    i += 1;
                }
            } else {
                i += 1;
            }
            continue;
        }
        match c {
            b'\'' => {
                in_string = true;
                i += 1;
            }
            b'-' if i + 1 < n && bytes[i + 1] == b'-' => {
                line_comment = true;
                i += 2;
            }
            b'/' if i + 1 < n && bytes[i + 1] == b'*' => {
                block_comment = true;
                i += 2;
            }
            b':' => {
                // 前一字符也是 `:`（`::` 类型转换）不收；后一个必须是标识符起始
                let prev_is_colon = i > 0 && bytes[i - 1] == b':';
                if !prev_is_colon && i + 1 < n && is_ident_start(bytes[i + 1]) {
                    let mut j = i + 2;
                    while j < n && is_ident_cont(bytes[j]) {
                        j += 1;
                    }
                    spans.push(ParamSpan {
                        start: i,
                        end: j,
                        name: sql[i + 1..j].to_string(),
                    });
                    i = j;
                    continue;
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    spans
}

/// 从 SQL 里取出参数名（按首次出现顺序去重；表单按这个顺序摆输入框）
pub fn param_names(sql: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for span in collect_spans(sql) {
        if !seen.iter().any(|name| *name == span.name) {
            seen.push(span.name);
        }
    }
    seen
}

/// 把 SQL 里的 `:name` 替换成字面量：
/// - 有值（非空）→ `'值'`（内部单引号 `''` 转义）
/// - 空值 / 未提供 → `NULL`
/// 字符串与注释里的 `:` 原样保留；`::` 原样保留；无参数时原样返回。
pub fn substitute_params(sql: &str, values: &HashMap<String, String>) -> String {
    let spans = collect_spans(sql);
    if spans.is_empty() {
        return sql.to_string();
    }
    let mut parts = Vec::with_capacity(spans.len() * 2 + 1);
    let mut last = 0;
    for span in &spans {
        parts.push(sql[last..span.start].to_string());
        match values.get(&span.name) {
            Some(value) if !value.is_empty() => {
                parts.push(format!("'{}'", value.replace('\'', "''")));
            }
            _ => parts.push("NULL".to_string()),
        }
        last = span.end;
    }
    parts.push(sql[last..].to_string());
    parts.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 基本：`SELECT * FROM t WHERE id = :id AND name = :name`
    #[test]
    fn collects_named_parameters_in_order() {
        let sql = "SELECT * FROM t WHERE id = :id AND name = :name";
        assert_eq!(param_names(sql), vec!["id".to_string(), "name".to_string()]);
    }

    /// 同名多处出现只出一个输入项（替换时全部换掉）
    #[test]
    fn same_name_appears_once_in_form_but_all_replaced() {
        let sql = "SELECT * FROM t WHERE a = :x OR b = :x";
        assert_eq!(param_names(sql), vec!["x".to_string()]);
        let mut values = HashMap::new();
        values.insert("x".to_string(), "7".to_string());
        assert_eq!(
            substitute_params(sql, &values),
            "SELECT * FROM t WHERE a = '7' OR b = '7'"
        );
    }

    /// `::`（PostgreSQL 类型转换）不是参数
    #[test]
    fn cast_colon_is_not_a_parameter() {
        let sql = "SELECT id::text FROM t WHERE id > :min";
        assert_eq!(param_names(sql), vec!["min".to_string()]);
    }

    /// 字符串里的 `:` 是数据不是参数（含 `''` 转义）
    #[test]
    fn string_literals_are_skipped() {
        let sql = "SELECT ':a', 'it''s :b' FROM t WHERE id = :id";
        assert_eq!(param_names(sql), vec!["id".to_string()]);
    }

    /// 行注释 / 块注释里的 `:` 不识别
    #[test]
    fn comments_are_skipped() {
        let sql = "-- :a\nSELECT 1 /* :b */ WHERE id = :id";
        assert_eq!(param_names(sql), vec!["id".to_string()]);
    }

    /// 空值 → `NULL`；未提供的名字 → `NULL`（同名已在表单里，正常不会缺）
    #[test]
    fn empty_value_becomes_null() {
        let sql = "SELECT * FROM t WHERE a = :a AND b = :b";
        let mut values = HashMap::new();
        values.insert("a".to_string(), String::new());
        assert_eq!(
            substitute_params(sql, &values),
            "SELECT * FROM t WHERE a = NULL AND b = NULL"
        );
    }

    /// 值里的单引号按 SQL 约定 `''` 转义
    #[test]
    fn quotes_inside_value_are_escaped() {
        let sql = "SELECT * FROM t WHERE name = :name";
        let mut values = HashMap::new();
        values.insert("name".to_string(), "o'brien".to_string());
        assert_eq!(
            substitute_params(sql, &values),
            "SELECT * FROM t WHERE name = 'o''brien'"
        );
    }

    /// 无参数原样返回
    #[test]
    fn no_parameters_returns_input_unchanged() {
        let sql = "SELECT 1";
        assert_eq!(substitute_params(sql, &HashMap::new()), sql);
    }
}
