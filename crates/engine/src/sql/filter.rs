//! 下发源库的筛选改写（B14）
//!
//! 「下发源库」= 把本地筛选词翻译成一条 **WHERE 重查**：好处是能看见未抓取的行与最新数据，
//! 代价是重跑一次查询、并产生**新结果集**（原型 §5.5 的两档口径）。
//!
//! ## 生成的形状
//!
//! ```sql
//! SELECT * FROM (
//!   <原查询，去掉顶层 LIMIT>
//! ) AS rds_filtered
//! WHERE (CAST(col_a AS TEXT) LIKE '%词%' ESCAPE '!')
//!    OR (CAST(col_b AS TEXT) LIKE '%词%' ESCAPE '!')
//! ORDER BY <原查询的顶层 ORDER BY，若有>
//! ```
//!
//! ## 取舍（都写进测试盯住）
//!
//! - **包一层子查询**：筛选命中的是**结果集的列**（原查询的输出列），只有包起来才引用得到；
//! - **去掉 LIMIT**：留着的话“筛选”只在被截断的那几行里找，那就不叫下发了；去掉这件事要在
//!   界面上说明（返回 [`FilterRewrite::dropped_limit`]，场景 34）；
//! - **ORDER BY 提到外层**：子查询里的 ORDER BY 在外层**不保证顺序**，提到外层才是真的有序；
//! - **CAST 成字符串**再 `LIKE`：数字 / 日期列也要能按文本筛（与本地筛选同语义）；
//!   目标类型由调用方按方言给（MySQL 没有 `TEXT` 这个 CAST 目标，用 `CHAR`）；
//! - **`ESCAPE '!'`**：`%` / `_` 要能被当普通字符搜（用户输入的 `100%` 不该变成通配符），
//!   而转义符用 `!` 是为了避开 MySQL 字符串里反斜杠的语义（`'\\'` 在那边不是标准写法）。
//!
//! 顶层子句的定位走 sqlglot 的 **tokenizer**（与语法高亮同一个入口）：字符串字面量或注释里的
//! `limit` / `order by` 不算子句——词法扫描最容易错的正是这件事。

use sqlglot_rust::tokens::{TokenType, Tokenizer};

/// 改写结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterRewrite {
    /// 能直接发给源库的 SQL
    pub sql: String,
    /// 被去掉的 LIMIT 原文（`Some` 时界面要提示“将去掉 LIMIT”）
    pub dropped_limit: Option<String>,
}

/// 把「结果集的列 + 筛选词」拼成一条重查语句
///
/// `cast_type` 是 CAST 的目标类型（通常 `TEXT`；MySQL 用 `CHAR`）。
pub fn rewrite_with_filter(
    original: &str,
    columns: &[String],
    needle: &str,
    cast_type: &str,
) -> Result<FilterRewrite, String> {
    let needle = needle.trim();
    if original.trim().is_empty() {
        return Err("没有可下发的查询".to_string());
    }
    if needle.is_empty() {
        return Err("筛选词为空，没什么可下发的".to_string());
    }
    if columns.is_empty() {
        return Err("这份结果没有列，无法拼下发条件".to_string());
    }

    let clauses = top_level_clauses(original);
    let body_end = clauses.body_end(original.len());
    let body = original[..body_end].trim_end();
    let order_text = clauses.order_text(original);
    let dropped_limit = clauses.limit_text(original);

    let pattern = like_literal(needle);
    let condition = columns
        .iter()
        .map(|column| {
            format!(
                "(CAST({} AS {}) LIKE {pattern} ESCAPE '!')",
                quote_ident(column),
                cast_type
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");

    let mut sql = format!("SELECT * FROM (\n{body}\n) AS rds_filtered\nWHERE {condition}");
    if let Some(order) = order_text {
        sql.push('\n');
        sql.push_str(&order);
    }

    Ok(FilterRewrite { sql, dropped_limit })
}

/// 顶层（括号外）子句的位置
struct TopLevelClauses {
    /// 顶层 ORDER BY 起点（字节）
    order_at: Option<usize>,
    /// 顶层 LIMIT 起点（字节）
    limit_at: Option<usize>,
}

impl TopLevelClauses {
    /// 子查询正文的结束位置（ORDER BY / LIMIT 之前）
    fn body_end(&self, sql_len: usize) -> usize {
        [self.order_at, self.limit_at]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(sql_len)
    }

    /// 顶层 ORDER BY 的原文（原样搬到外层）
    fn order_text(&self, sql: &str) -> Option<String> {
        let start = self.order_at?;
        let end = self
            .limit_at
            .filter(|limit| *limit > start)
            .unwrap_or(sql.len());
        let text = sql[start..end].trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// 被去掉的 LIMIT 原文
    fn limit_text(&self, sql: &str) -> Option<String> {
        let start = self.limit_at?;
        let text = sql[start..].trim();
        (!text.is_empty()).then(|| text.to_string())
    }
}

/// 扫一遍 token，只认**括号外**的 `ORDER BY` 与 `LIMIT`
fn top_level_clauses(sql: &str) -> TopLevelClauses {
    let mut depth = 0usize;
    let mut order_at = None;
    let mut limit_at = None;
    let mut pending_order: Option<usize> = None;

    let tokens = match Tokenizer::new(sql).tokenize() {
        Ok(tokens) => tokens,
        // 词法都扫不动（截断的 SQL 之类）：当作没有顶层子句，让包一层那步自己去面对
        Err(_) => {
            return TopLevelClauses {
                order_at: None,
                limit_at: None,
            };
        }
    };

    for token in &tokens {
        if matches!(
            token.token_type,
            TokenType::Whitespace | TokenType::Eof | TokenType::LineComment | TokenType::BlockComment
        ) {
            continue;
        }
        let value = token.value.as_str();
        match value {
            "(" | "[" => {
                depth += 1;
                continue;
            }
            ")" | "]" => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if depth > 0 {
            continue;
        }
        let at = byte_offset_of(sql, token.position);
        match value.to_ascii_uppercase().as_str() {
            "ORDER" => pending_order = Some(at),
            // `ORDER` 后面紧跟的不是 `BY`（比如 `ORDER` 当列名）时不算子句
            "BY" => {
                if let Some(start) = pending_order.take() {
                    order_at = order_at.or(Some(start));
                }
            }
            "LIMIT" => limit_at = limit_at.or(Some(at)),
            _ => pending_order = None,
        }
    }

    TopLevelClauses { order_at, limit_at }
}

/// tokenizer 给的是**字符**位置，这里换算成字节偏移（视图层的切片按字节走）
fn byte_offset_of(sql: &str, char_index: usize) -> usize {
    sql.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(sql.len())
}

/// LIKE 的模式串：`%词%`，并把词里的 `!` `%` `_` 转义（配合 `ESCAPE '!'`）
fn like_literal(needle: &str) -> String {
    let mut escaped = String::with_capacity(needle.len() + 2);
    escaped.push('%');
    for ch in needle.chars() {
        match ch {
            '!' | '%' | '_' => {
                escaped.push('!');
                escaped.push(ch);
            }
            // SQL 字符串里的单引号双写
            '\'' => escaped.push_str("''"),
            ch => escaped.push(ch),
        }
    }
    escaped.push('%');
    format!("'{escaped}'")
}

/// 标识符：简单名字原样，其余用双引号包裹（与导出那边的口径一致）
fn quote_ident(name: &str) -> String {
    let simple = !name.is_empty()
        && name.chars().enumerate().all(|(ix, ch)| {
            ch == '_' || (ch.is_ascii_alphanumeric() && !(ix == 0 && ch.is_ascii_digit()))
        });
    if simple {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    use super::{rewrite_with_filter, top_level_clauses};

    fn columns() -> Vec<String> {
        vec!["id".to_string(), "name".to_string()]
    }

    #[test]
    fn a_plain_query_gets_a_wrapped_where() {
        let rewritten = rewrite_with_filter("SELECT * FROM orders", &columns(), "ab", "TEXT")
            .expect("能改写");
        assert_eq!(
            rewritten.sql,
            "SELECT * FROM (\nSELECT * FROM orders\n) AS rds_filtered\n\
             WHERE (CAST(id AS TEXT) LIKE '%ab%' ESCAPE '!') OR (CAST(name AS TEXT) LIKE '%ab%' ESCAPE '!')"
        );
        assert_eq!(rewritten.dropped_limit, None);
    }

    #[test]
    fn order_by_moves_outside_the_subquery() {
        // 子查询里的 ORDER BY 在外层不保证顺序：要搬到外面（场景 34）
        let rewritten =
            rewrite_with_filter("SELECT * FROM t ORDER BY created_at DESC", &columns(), "x", "TEXT")
                .expect("能改写");
        assert!(
            rewritten.sql.starts_with("SELECT * FROM (\nSELECT * FROM t\n) AS rds_filtered"),
            "{}",
            rewritten.sql
        );
        assert!(
            rewritten.sql.ends_with("ORDER BY created_at DESC"),
            "ORDER BY 要提到外层：{}",
            rewritten.sql
        );
        assert_eq!(
            rewritten.sql.matches("ORDER BY").count(),
            1,
            "子查询里不该再留一份：{}",
            rewritten.sql
        );
    }

    #[test]
    fn limit_is_dropped_and_reported() {
        let rewritten =
            rewrite_with_filter("SELECT * FROM t LIMIT 10 OFFSET 5", &columns(), "x", "TEXT")
                .expect("能改写");
        assert!(
            !rewritten.sql.contains("LIMIT"),
            "去掉 LIMIT 才叫下发：{}",
            rewritten.sql
        );
        assert_eq!(rewritten.dropped_limit.as_deref(), Some("LIMIT 10 OFFSET 5"));
    }

    #[test]
    fn comments_and_strings_do_not_look_like_clauses() {
        // 字符串与注释里的 limit / order by 不是子句（词法扫描最容易错的地方）
        let sql = "SELECT 'limit 5' AS a, 1 AS b /* order by x */ FROM t";
        let rewritten = rewrite_with_filter(sql, &columns(), "x", "TEXT").expect("能改写");
        assert_eq!(rewritten.dropped_limit, None, "{}", rewritten.sql);
        assert!(rewritten.sql.contains("'limit 5'"), "{}", rewritten.sql);

        let sql = "SELECT * FROM t -- order by id\nWHERE a = 1";
        let clauses = top_level_clauses(sql);
        assert!(clauses.order_at.is_none(), "行注释里的不算");
    }

    #[test]
    fn grouping_and_subqueries_are_not_confused_with_clauses() {
        // 子查询里的 ORDER BY / LIMIT 属于内层，不能搬到外层
        let sql = "SELECT * FROM (SELECT * FROM t LIMIT 3) AS inner_q";
        let rewritten = rewrite_with_filter(sql, &columns(), "x", "TEXT").expect("能改写");
        assert!(
            rewritten.sql.contains("LIMIT 3"),
            "内层的 LIMIT 要留着（它属于子查询）：{}",
            rewritten.sql
        );
        assert_eq!(rewritten.dropped_limit, None);
    }

    #[test]
    fn the_needle_is_escaped_for_like() {
        let rewritten =
            rewrite_with_filter("SELECT * FROM t", &["a%b".to_string()], "100%_!", "TEXT")
                .expect("能改写");
        assert!(
            rewritten.sql.contains("LIKE '%100!%!_!!%' ESCAPE '!'"),
            "`%` `_` `!` 都要转义：{}",
            rewritten.sql
        );
        assert!(
            rewritten.sql.contains("CAST(\"a%b\" AS TEXT)"),
            "特殊列名要引号：{}",
            rewritten.sql
        );
    }

    #[test]
    fn single_quotes_in_the_needle_do_not_break_the_string() {
        let rewritten =
            rewrite_with_filter("SELECT * FROM t", &columns(), "o'brien", "TEXT").expect("能改写");
        assert!(
            rewritten.sql.contains("LIKE '%o''brien%'"),
            "单引号要双写：{}",
            rewritten.sql
        );
    }

    #[test]
    fn empty_inputs_are_refused_with_a_reason() {
        assert!(rewrite_with_filter("", &columns(), "x", "TEXT").is_err());
        assert!(rewrite_with_filter("SELECT 1", &columns(), "   ", "TEXT").is_err());
        assert!(rewrite_with_filter("SELECT 1", &[], "x", "TEXT").is_err());
    }
}
