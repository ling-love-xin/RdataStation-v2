//! 语句级表引用抽取（发往 DuckDB 前的预检输入侧）。
//!
//! 与 [`super::refs::collect_table_refs`] 的分工：那一份是**编辑器打字期**的顶层原语
//! （只给悬停 / 跳转用，CTE 与子查询不深入）；这一份是**执行前**用的，必须覆盖整棵树：
//!
//! - 顶层 FROM / JOIN，`LATERAL` 内层；
//! - 派生表（`FROM (SELECT …) x`）——递归进去，**别名本身不算表**；
//! - CTE：名字进作用域（含继承自外层的），body 递归收集；引用 CTE 的地方跳过；
//! - `UNION` / `INSERT … SELECT` / `UPDATE … FROM` / `DELETE … USING` / `CTAS` / `CREATE VIEW`；
//! - 表达式里的标量子查询与 `EXISTS`。
//!
//! 表函数（`read_csv(…)` / `range(…)`）不是表，不收集——它们没有别名可校验。
//!
//! 只抽「引用了哪些名字」，判定（存在性 / 歧义 / 候选）归调用方：
//! 语言层不该知道 catalog 的事。

use sqlglot_rust::ast::{Expr, InsertSource, SelectItem, Statement, TableRef, TableSource};
use sqlglot_rust::parse;

use super::engine::SqlDialect;
use super::refs::to_inner_dialect;

/// 一条表引用：`parts` 就是原文写了几段（`t` / `schema.t` / `catalog.schema.t`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePath {
    pub parts: Vec<String>,
}

impl TablePath {
    /// 末段（表名本身）
    pub fn last(&self) -> &str {
        self.parts.last().map(String::as_str).unwrap_or("")
    }

    /// 原文写了几段（1 = 裸名，≥2 = 带了前缀）
    pub fn depth(&self) -> usize {
        self.parts.len()
    }

    /// 按原样拼回去（报错文案里给用户看的形状）
    pub fn qualified(&self) -> String {
        self.parts.join(".")
    }
}

/// 抽出语句里所有真实表引用（CTE 名 / 派生别名 / 表函数已排除）。
///
/// 解析失败直接把解析器原话返回：预检的错误出口就在这里（执行期的错误留给执行）。
pub fn table_paths(sql: &str, dialect: SqlDialect) -> Result<Vec<TablePath>, String> {
    let statement = parse(sql, to_inner_dialect(dialect)).map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    collect_statement(&statement, &[], &mut out);
    Ok(out)
}

/// CTE 名作用域：本层声明的 + 从外层继承的（子查询里引用外层 CTE 是合法写法）
fn collect_statement(statement: &Statement, inherited: &[String], out: &mut Vec<TablePath>) {
    match statement {
        Statement::Select(select) => {
            let mut scope: Vec<String> = inherited.to_vec();
            for cte in &select.ctes {
                scope.push(cte.name.to_lowercase());
                collect_statement(&cte.query, &scope, out);
            }
            if let Some(from) = &select.from {
                collect_source(&from.source, &scope, out);
            }
            for join in &select.joins {
                collect_source(&join.table, &scope, out);
                if let Some(on) = &join.on {
                    collect_expr(on, &scope, out);
                }
            }
            for item in &select.columns {
                if let SelectItem::Expr { expr, .. } = item {
                    collect_expr(expr, &scope, out);
                }
            }
            if let Some(expr) = &select.where_clause {
                collect_expr(expr, &scope, out);
            }
            for expr in &select.group_by {
                collect_expr(expr, &scope, out);
            }
            if let Some(expr) = &select.having {
                collect_expr(expr, &scope, out);
            }
            for item in &select.order_by {
                collect_expr(&item.expr, &scope, out);
            }
            if let Some(expr) = &select.qualify {
                collect_expr(expr, &scope, out);
            }
            if let Some(expr) = &select.top {
                collect_expr(expr, &scope, out);
            }
            for expr in select.limit.iter().chain(select.offset.iter()) {
                collect_expr(expr, &scope, out);
            }
        }
        Statement::SetOperation(set) => {
            collect_statement(&set.left, inherited, out);
            collect_statement(&set.right, inherited, out);
            for item in &set.order_by {
                collect_expr(&item.expr, inherited, out);
            }
        }
        Statement::Insert(insert) => {
            if let InsertSource::Query(query) = &insert.source {
                collect_statement(query, inherited, out);
            }
        }
        Statement::Update(update) => {
            push_table(&update.table, inherited, out);
            if let Some(from) = &update.from {
                collect_source(&from.source, inherited, out);
            }
            for join in &update.joins {
                collect_source(&join.table, inherited, out);
            }
            for (_, expr) in &update.assignments {
                collect_expr(expr, inherited, out);
            }
            if let Some(expr) = &update.where_clause {
                collect_expr(expr, inherited, out);
            }
        }
        Statement::Delete(delete) => {
            push_table(&delete.table, inherited, out);
            if let Some(using) = &delete.using {
                collect_source(&using.source, inherited, out);
            }
            if let Some(expr) = &delete.where_clause {
                collect_expr(expr, inherited, out);
            }
        }
        Statement::CreateTable(create) => {
            if let Some(query) = &create.as_select {
                collect_statement(query, inherited, out);
            }
        }
        Statement::CreateView(view) => collect_statement(&view.query, inherited, out),
        Statement::Merge(merge) => {
            push_table(&merge.target, inherited, out);
            collect_source(&merge.source, inherited, out);
            collect_expr(&merge.on, inherited, out);
        }
        Statement::Expression(expr) => collect_expr(expr, inherited, out),
        // DDL / 事务 / USE / 命令：没有可校验的表引用
        _ => {}
    }
}

fn collect_source(source: &TableSource, scope: &[String], out: &mut Vec<TablePath>) {
    match source {
        TableSource::Table(table) => push_table(table, scope, out),
        TableSource::Subquery { query, .. } => collect_statement(query, scope, out),
        TableSource::Lateral { source } => collect_source(source, scope, out),
        TableSource::Pivot { source, .. } | TableSource::Unpivot { source, .. } => {
            collect_source(source, scope, out)
        }
        TableSource::TableFunction { .. } | TableSource::Unnest { .. } => {}
    }
}

/// 表达式里的标量子查询 / `EXISTS`：递归进嵌套语句，表达式本身不产出表引用。
///
/// `Expr::walk` 只走 `Expr` 树、不进 `Statement`，所以这里不会重复收集。
fn collect_expr(expr: &Expr, scope: &[String], out: &mut Vec<TablePath>) {
    expr.walk(&mut |node| {
        match node {
            Expr::Subquery(statement) => collect_statement(statement, scope, out),
            Expr::Exists { subquery, .. } => collect_statement(subquery, scope, out),
            _ => {}
        }
        true
    });
}

/// 一段 `TableRef` → 表引用；裸名且在本层 CTE 作用域里 = 派生表，跳过。
fn push_table(table: &TableRef, scope: &[String], out: &mut Vec<TablePath>) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(catalog) = &table.catalog {
        parts.push(catalog.clone());
    }
    if let Some(schema) = &table.schema {
        parts.push(schema.clone());
    }
    parts.push(table.name.clone());
    if parts.len() == 1 && scope.iter().any(|name| name == &parts[0].to_lowercase()) {
        return;
    }
    out.push(TablePath { parts });
}

#[cfg(test)]
mod tests {
    use super::{table_paths, TablePath};
    use crate::SqlDialect;

    fn shapes(sql: &str) -> Vec<String> {
        table_paths(sql, SqlDialect::Duckdb)
            .expect("应当能解析")
            .into_iter()
            .map(|path| path.qualified())
            .collect()
    }

    #[test]
    fn plain_from_and_join() {
        assert_eq!(
            shapes("SELECT * FROM orders o JOIN public.customers c ON o.id = c.id"),
            vec!["orders", "public.customers"]
        );
    }

    #[test]
    fn cte_reference_is_not_a_table() {
        assert_eq!(
            shapes("WITH c AS (SELECT id FROM orders) SELECT * FROM c"),
            vec!["orders"],
            "CTE 名不是表；body 里的 orders 才是"
        );
    }

    #[test]
    fn derived_table_alias_is_not_a_table() {
        assert_eq!(
            shapes("SELECT * FROM (SELECT id FROM orders) x JOIN erp.stock s ON x.id = s.id"),
            vec!["orders", "erp.stock"]
        );
    }

    #[test]
    fn set_operation_collects_both_sides() {
        assert_eq!(
            shapes("SELECT * FROM a.orders UNION ALL SELECT * FROM b.orders"),
            vec!["a.orders", "b.orders"]
        );
    }

    #[test]
    fn insert_select_and_update_from() {
        assert_eq!(
            shapes("INSERT INTO t SELECT * FROM src"),
            vec!["src"],
            "写入目标不校验（预检只管读到的表）"
        );
        assert_eq!(
            shapes("UPDATE t SET x = 1 FROM other o WHERE t.id = o.id"),
            vec!["t", "other"]
        );
    }

    #[test]
    fn scalar_subquery_and_exists_are_collected() {
        // 顺序 = 关系层（FROM / JOIN）在前、表达式层（列 / WHERE）在后；预检不看顺序
        assert_eq!(
            shapes("SELECT (SELECT max(x) FROM a) FROM b WHERE EXISTS (SELECT 1 FROM c)"),
            vec!["b", "a", "c"]
        );
    }

    #[test]
    fn table_function_is_skipped() {
        assert_eq!(shapes("SELECT * FROM read_csv('a.csv')"), Vec::<String>::new());
    }

    #[test]
    fn parse_error_is_reported_verbatim() {
        assert!(table_paths("SELECT FROM", SqlDialect::Duckdb).is_err());
    }

    #[test]
    fn path_keeps_the_original_shape() {
        let paths = table_paths("SELECT * FROM shop.main.orders", SqlDialect::Duckdb).unwrap();
        assert_eq!(
            paths,
            vec![TablePath {
                parts: vec![
                    "shop".to_string(),
                    "main".to_string(),
                    "orders".to_string()
                ]
            }]
        );
        assert_eq!(paths[0].depth(), 3);
        assert_eq!(paths[0].last(), "orders");
    }
}
