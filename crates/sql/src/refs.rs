//! SQL 引用定位与收集的公共原语（#54 语义层共享：semantic / hover / jump）
//!
//! 统一口径（语义层三模块保持一致，避免各写一份漂移）：
//!
//! - **位置**：sqlglot AST 不带 position（0.10.33 实测），一律 token 层定位——
//!   `Token::position` 是字符下标，`token_byte_spans` 换算成字节区间
//!   （顺序扫描 `char_indices`，多字节注释前缀不跑偏）。
//! - **表引用**：只收集顶层 FROM / JOIN（LATERAL 内层也收），子查询 / CTE 内部
//!   不在本版范围（编辑器打字期价值在顶层）。
//! - **列引用**：顶层 SELECT 表达式层；`Expr::walk` visitor 是 HRTB（for<'a>），
//!   闭包里借不出外部生命周期引用，收集时 clone 值。
//! - **同表判定**：目录 `public.orders` 与引用裸名 `orders` 末段相同即认。

use sqlglot_rust::ast::{Expr, SelectItem, SelectStatement, TableRef, TableSource};
use sqlglot_rust::tokens::{Token, TokenType, Tokenizer};
use sqlglot_rust::Dialect;

use super::engine::SqlDialect;

/// 分词（失败返回空：词法失败 = 语义能力退化，与高亮同口径）
pub fn tokenize(sql: &str) -> Vec<Token> {
    let mut tokenizer = Tokenizer::new(sql);
    tokenizer.tokenize().unwrap_or_default()
}

/// 每个 token 的字节区间（`Token::position` 是字符下标；换算同
/// `highlight::byte_offsets`——顺序扫描 char_indices）
pub fn token_byte_spans(sql: &str, tokens: &[Token]) -> Vec<(usize, usize)> {
    let mut spans = Vec::with_capacity(tokens.len());
    let mut byte = 0usize;
    let mut char_indices = sql.char_indices();
    let mut next_byte = char_indices.next().map(|(b, _)| b);
    for token in tokens {
        while byte < token.position {
            next_byte = char_indices.next().map(|(b, _)| b);
            byte += 1;
        }
        let start = next_byte.unwrap_or(sql.len());
        spans.push((start, start + token.value.len()));
    }
    spans
}

/// 找 `name`（未加引号标识符，大小写不敏感）在语句中的**首次**字节区间
///
/// 限定名（`schema.t`）取末段匹配——FROM 处 `public.orders` 命中 `orders` token。
pub fn locate(sql: &str, tokens: &[Token], name: &str) -> Option<(usize, usize)> {
    let needle = name.rsplit('.').next().unwrap_or(name).to_lowercase();
    let spans = token_byte_spans(sql, tokens);
    tokens
        .iter()
        .zip(spans)
        .find(|(token, _)| {
            token.token_type == TokenType::Identifier
                && token.value.to_lowercase() == needle
        })
        .map(|(_, span)| span)
}

/// 顶层表引用收集（LATERAL 内层也收；子查询 / 表函数 / UNNEST 不在本版范围）
pub fn collect_table_refs<'a>(source: &'a TableSource, out: &mut Vec<&'a TableRef>) {
    match source {
        TableSource::Table(table) => out.push(table),
        TableSource::Lateral { source } => collect_table_refs(source, out),
        _ => {}
    }
}

/// 表引用 → 限定名（`catalog.schema.table` 截到 `schema.table`，与
/// `parser::single_from_table` 同口径——NavCache 键是 `schema.表` 真实名）
pub fn qualify_table_name(table: &TableRef) -> String {
    match (&table.schema, &table.catalog) {
        (Some(schema), _) => format!("{}.{}", schema, table.name),
        (None, Some(catalog)) => format!("{}.{}", catalog, table.name),
        (None, None) => table.name.clone(),
    }
}

/// 目录表名与引用名同表判定：目录 `public.orders`、引用裸名 `orders` 也认
/// （两边末段相同即可——不同 schema 的同名表在本目录粒度上不区分）
pub fn same_table(catalog: &str, referenced: &str) -> bool {
    if catalog == referenced {
        return true;
    }
    catalog
        .rsplit('.')
        .next()
        .zip(referenced.rsplit('.').next())
        .map(|(a, b)| a == b)
        .unwrap_or(false)
}

/// 顶层 SELECT 表达式层里的列引用（带限定符），子查询 / CTE 内部不收集
///
/// 返回 owned（列名, 限定表名）——`Expr::walk` visitor 是 HRTB（for<'a>），
/// 从闭包参数里借不出与外部生命周期匹配的引用，收集时 clone 值。
pub fn collect_column_refs(
    select: &SelectStatement,
    out: &mut Vec<(String, Option<String>)>,
) {
    let mut exprs: Vec<&Expr> = Vec::new();
    for item in &select.columns {
        if let SelectItem::Expr { expr, .. } = item {
            exprs.push(expr);
        }
    }
    for join in &select.joins {
        if let Some(on) = &join.on {
            exprs.push(on);
        }
    }
    if let Some(w) = &select.where_clause {
        exprs.push(w);
    }
    for g in &select.group_by {
        exprs.push(g);
    }
    if let Some(h) = &select.having {
        exprs.push(h);
    }
    for o in &select.order_by {
        exprs.push(&o.expr);
    }
    for expr in exprs {
        expr.walk(&mut |e| {
            if let Expr::Column { table, name, .. } = e {
                out.push((name.clone(), table.clone()));
            }
            true
        });
    }
}

/// 方言映射到 sqlglot（各模块统一入口）
pub fn to_inner_dialect(dialect: SqlDialect) -> Dialect {
    match dialect {
        SqlDialect::Ansi => Dialect::Ansi,
        SqlDialect::Mysql => Dialect::Mysql,
        SqlDialect::Postgres => Dialect::Postgres,
        SqlDialect::Sqlite => Dialect::Sqlite,
        SqlDialect::Duckdb => Dialect::DuckDb,
        SqlDialect::MsSQL => Dialect::Tsql,
        SqlDialect::Oracle => Dialect::Oracle,
        SqlDialect::Snowflake => Dialect::Snowflake,
        SqlDialect::BigQuery => Dialect::BigQuery,
        SqlDialect::Redshift => Dialect::Redshift,
    }
}
