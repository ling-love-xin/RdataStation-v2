//! SQL 跳转定义（#54 ④）：编辑器内表 / 列引用 → 目标定位信息
//!
//! ## 语义
//!
//! 「跳转定义」= 编辑器里 Ctrl+Click / F12 悬停在表名或列名上 → 定位到该对象的
//! 定义处。本产品的「定义」是**导航树里的对象节点**（+ 可选的表结构视图/DDL），
//! 因此 engine 侧只负责把引用解析成目标（限定表名 / 列名），导航树定位由
//! workbench 侧接线（表目录来源 = NavCache，与诊断 / hover 同源）。
//!
//! ## 与 hover 的分工
//!
//! `hover` 给「说明信息」（签名 / 列列表 / 中文描述），`jump` 给「目标定位」。
//! 复用同一套 token 定位与引用收集原语（`refs`），口径一致：
//!
//! - 表：顶层 FROM / JOIN 的表引用（大小写不敏感），目标 = 目录里的真实限定名
//!   （原文裸名 `customers` → 目录 `public.customers`）
//! - 列：qualify 归位后的列引用，目标 = 所属表 + 列名
//! - 函数：**无定义**（内置函数 / UDF 都不跳——UDF 定义需系统表查，后续版本）
//! - 无目录时：表引用仍可跳（目标 = 引用限定名），列引用退化为 None
//!
//! 优先级（同名）：**表 > 列**（跳转场景按 AST 角色判定，不做函数兜底）。

use sqlglot_rust::ast::{DataType, Statement};
use sqlglot_rust::optimizer::qualify_columns::qualify_columns;
use sqlglot_rust::schema::MappingSchema;
use sqlglot_rust::tokens::TokenType;
use sqlglot_rust::parse;

use super::engine::SqlDialect;
use super::refs::{
    collect_column_refs, collect_table_refs, qualify_table_name, same_table, to_inner_dialect,
    token_byte_spans, tokenize,
};
use super::semantic::CatalogTable;

/// 跳转目标类别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpKind {
    Table,
    Column,
}

/// 一个跳转目标（**字节区间**为源引用位置，UI 用它高亮 / 定位起点）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpTarget {
    pub kind: JumpKind,
    /// 目标表限定名（目录真实名；无目录时 = 引用限定名）
    pub table: String,
    /// 目标列名（仅 Column）
    pub column: Option<String>,
    pub start: usize,
    pub end: usize,
}

/// 跳转定义入口：给定字节位置，返回该处引用的跳转目标（无目标 → None）
pub fn jump_at(
    sql: &str,
    dialect: SqlDialect,
    tables: &[CatalogTable],
    byte_pos: usize,
) -> Option<JumpTarget> {
    let tokens = tokenize(sql);
    if tokens.is_empty() {
        return None;
    }
    let spans = token_byte_spans(sql, &tokens);
    let idx = spans.iter().position(|(s, e)| byte_pos >= *s && byte_pos < *e)?;
    if tokens[idx].token_type != TokenType::Identifier {
        return None;
    }
    let word = tokens[idx].value.to_lowercase();
    let (start, end) = spans[idx];

    let inner = to_inner_dialect(dialect);
    let Ok(mut stmt) = parse(sql, inner) else {
        return None;
    };
    // 有目录先 qualify：裸列归位成 `t.col`，列跳转才有所属表
    if !tables.is_empty() {
        let mut schema = MappingSchema::new(inner);
        for table in tables {
            let path: Vec<&str> = table.qualified_name.split('.').collect();
            let columns = table
                .columns
                .iter()
                .map(|c| ((*c).to_string(), DataType::Unknown(String::new())))
                .collect();
            let _ = schema.replace_table(&path, columns);
        }
        stmt = qualify_columns(stmt, &schema);
    }
    let Statement::Select(select) = &stmt else {
        return None;
    };

    // 表 > 列（同名时按 AST 角色；函数不参与跳转）
    if let Some(target) = table_jump(&word, select, tables, start, end) {
        return Some(target);
    }
    if let Some(target) = column_jump(&word, select, start, end) {
        return Some(target);
    }
    None
}

fn table_jump(
    word: &str,
    select: &sqlglot_rust::ast::SelectStatement,
    tables: &[CatalogTable],
    start: usize,
    end: usize,
) -> Option<JumpTarget> {
    let mut sources: Vec<&sqlglot_rust::ast::TableRef> = Vec::new();
    if let Some(from) = &select.from {
        collect_table_refs(&from.source, &mut sources);
    }
    for join in &select.joins {
        collect_table_refs(&join.table, &mut sources);
    }
    let referenced = sources.iter().find(|t| {
        let qualified = qualify_table_name(t).to_lowercase();
        qualified == word || qualified.rsplit('.').next() == Some(word)
    })?;
    let qualified = qualify_table_name(referenced);
    // 目标表名优先取目录真实限定名（跳转落点要精确到 schema）
    let table = tables
        .iter()
        .find(|t| same_table(t.qualified_name, &qualified))
        .map(|t| t.qualified_name.to_string())
        .unwrap_or_else(|| qualified);
    Some(JumpTarget {
        kind: JumpKind::Table,
        table,
        column: None,
        start,
        end,
    })
}

fn column_jump(
    word: &str,
    select: &sqlglot_rust::ast::SelectStatement,
    start: usize,
    end: usize,
) -> Option<JumpTarget> {
    let mut columns: Vec<(String, Option<String>)> = Vec::new();
    collect_column_refs(select, &mut columns);
    let (name, table) = columns
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))?;
    // 裸列（qualify 前无目录时不归位）没有所属表 → 无法跳
    let table = table.clone()?;
    Some(JumpTarget {
        kind: JumpKind::Column,
        table,
        column: Some(name.clone()),
        start,
        end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> Vec<CatalogTable<'static>> {
        vec![
            CatalogTable {
                qualified_name: "orders",
                columns: vec!["id", "total", "status"],
            },
            CatalogTable {
                qualified_name: "public.customers",
                columns: vec!["id", "name"],
            },
        ]
    }

    fn pos_of(sql: &str, word: &str, nth: usize) -> usize {
        sql.match_indices(word).nth(nth).map(|(i, _)| i).expect("词存在")
    }

    fn span_text<'a>(sql: &'a str, t: &JumpTarget) -> &'a str {
        &sql[t.start..t.end]
    }

    #[test]
    fn bare_table_jumps_to_catalog_qualified_name() {
        let sql = "SELECT * FROM customers";
        let t = jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "customers", 0))
            .expect("有目标");
        assert_eq!(t.kind, JumpKind::Table);
        assert_eq!(t.table, "public.customers", "目标取目录真实限定名");
        assert_eq!(span_text(sql, &t), "customers");
    }

    #[test]
    fn qualified_table_jump_resolves() {
        let sql = "SELECT * FROM public.orders";
        let t = jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "orders", 0))
            .expect("有目标");
        assert_eq!(t.kind, JumpKind::Table);
        assert_eq!(t.table, "orders");
    }

    #[test]
    fn column_jump_carries_owning_table() {
        let sql = "SELECT total FROM orders";
        let t = jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "total", 0))
            .expect("有目标");
        assert_eq!(t.kind, JumpKind::Column);
        assert_eq!(t.column.as_deref(), Some("total"));
        assert_eq!(t.table, "orders", "qualify 归位后的所属表");
    }

    #[test]
    fn alias_qualified_column_jump_keeps_qualifier() {
        // 别名场景：o.total → 所属表展示别名 o（qualify 保留限定符）
        let sql = "SELECT o.total FROM orders o";
        let t = jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "total", 0))
            .expect("有目标");
        assert_eq!(t.kind, JumpKind::Column);
        assert_eq!(t.table, "o");
    }

    #[test]
    fn no_catalog_table_jump_still_works() {
        // 未连接时表引用仍可跳（目标 = 引用限定名）；裸列跳转退化
        let sql = "SELECT * FROM customers";
        let t = jump_at(sql, SqlDialect::Ansi, &[], pos_of(sql, "customers", 0))
            .expect("表可跳");
        assert_eq!(t.table, "customers");
        assert!(jump_at("SELECT total FROM orders", SqlDialect::Ansi, &[], pos_of("SELECT total FROM orders", "total", 0)).is_none());
    }

    #[test]
    fn function_and_keyword_positions_do_not_jump() {
        let sql = "SELECT count(*) FROM orders";
        // 函数不跳（内置/UDF 无定义）；SELECT 关键字不跳
        assert!(jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "count", 0)).is_none());
        assert!(jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "SELECT", 0)).is_none());
    }

    #[test]
    fn multibyte_prefix_does_not_shift_target() {
        let sql = "SELECT id -- 中文注释\nFROM orders";
        let t = jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "orders", 0))
            .expect("有目标");
        assert_eq!(span_text(sql, &t), "orders");
    }

    #[test]
    fn non_select_and_parse_error_produce_nothing() {
        assert!(jump_at("INSERT INTO orders VALUES (1)", SqlDialect::Ansi, &tables(), 2).is_none());
        assert!(jump_at("SELECT FROM", SqlDialect::Ansi, &tables(), 3).is_none());
    }

    #[test]
    fn table_precedence_over_same_named_column() {
        // 表 > 列（FROM 处 orders 是表引用，即使表达式里也有列名 orders）
        let sql = "SELECT orders FROM orders";
        let t = jump_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "orders", 1))
            .expect("有目标");
        assert_eq!(t.kind, JumpKind::Table);
    }
}
