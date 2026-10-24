//! SQL 语义诊断（#54 ①）：基于 sqlglot `qualify_columns` 的限定前后对比
//!
//! 只报「有把握的错」，与编辑器打字期诊断口径一致（`editor::language_service`
//! 的 `ServiceReport.diagnostics` 注释：拿不准的一律不报）：
//!
//! - **未限定列**（`unqualified_column`）：qualify 后仍是 `Column { table: None }`
//!   ——解析器在任一来源里都解析不到它（`qualify_columns.rs` L482
//!   「Column not found in any source — leave unqualified」）。单表 FROM 的裸列
//!   会归位成 `t.col`、join 的裸列能解析到唯一源也会归位，**留下的 None 才是真
//!   查不到来源**。
//! - **未知表**（`unknown_table`）：顶层 SELECT 的 FROM / JOIN 表引用不在提供的
//!   表目录里（`qualify_columns.rs` L332「Unknown table — preserve as-is」）。
//!   CTE 名排除——CTE 是派生表，不在目录里是正常。
//!
//! ## 位置
//!
//! sqlglot AST 不带 position（`Expr::Column` 无位置字段，0.10.33 实测），诊断区间
//! 用 **token 层**匹配：取列名 / 表名在语句中的**首次**出现（大小写不敏感；
//! `Token::position` 是字符下标，字节偏移换算与 `highlight.rs::byte_offsets` 一致）。
//!
//! ## 已知限制（口径固定，测试锁定）
//!
//! - **无目录即静默退化**：`tables` 为空 = 调用方没有表级元数据（未连接 / 缓存未
//!   载），返回空——宁缺毋滥，不做「全表未知」的误报。
//! - **只做顶层 SELECT 的表达式层**：子查询 / CTE 内部的表与列留给后续版本
//!   （递归遍历嵌套 `Statement` 的成本高、收益低，编辑器打字期的价值在顶层）。
//! - 非 SELECT（INSERT / UPDATE / DDL / 解析失败）不产语义诊断，与
//!   `projection_sources` 同口径。

use sqlglot_rust::ast::{DataType, Expr, SelectItem, Statement, TableRef};
use sqlglot_rust::optimizer::qualify_columns::qualify_columns;
use sqlglot_rust::schema::MappingSchema;
use sqlglot_rust::parse;

use super::engine::SqlDialect;
use super::refs::{collect_table_refs, locate, qualify_table_name, same_table, to_inner_dialect, tokenize};

/// 一条语义诊断（**字节区间**，行列为派生，与 `editor::Diagnostic` 同构）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIssue {
    /// 稳定标识（接线时给诊断面板 / LSP `code`；测试按它断言）
    pub code: &'static str,
    /// 说给人听的一句
    pub message: String,
    pub start: usize,
    pub end: usize,
}

impl SemanticIssue {
    fn new(code: &'static str, message: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            code,
            message: message.into(),
            start,
            end,
        }
    }
}

/// 调用方提供的表目录（表级即可：语义诊断只查「表存在 / 列存在」，不看类型）
pub struct CatalogTable<'a> {
    /// 限定表名：`t` / `schema.t` / `catalog.schema.t`（与 NavCache 键同形）
    pub qualified_name: &'a str,
    /// 列名（owned 容器 + 借用元素：调用方可从自己的元数据直接 borrow）
    pub columns: Vec<&'a str>,
}

/// 语义诊断入口
///
/// `tables` 为空 → 返回空（无目录的退化，见模块注释）。
pub fn diagnose(sql: &str, dialect: SqlDialect, tables: &[CatalogTable]) -> Vec<SemanticIssue> {
    if tables.is_empty() {
        return Vec::new();
    }
    let inner = to_inner_dialect(dialect);
    let Ok(stmt) = parse(sql, inner) else {
        return Vec::new();
    };
    let Statement::Select(select) = &stmt else {
        return Vec::new();
    };

    // 表目录 → sqlglot schema（列类型给 Unknown：诊断只问存在性）
    let mut schema = MappingSchema::new(inner);
    for table in tables {
        let path: Vec<&str> = table.qualified_name.split('.').collect();
        let columns = table
            .columns
            .iter()
            .map(|c| ((*c).to_string(), DataType::Unknown(String::new())))
            .collect();
        // 路径段数非法（>3 或 0）→ 跳过该表，不影响其余目录
        let _ = schema.replace_table(&path, columns);
    }

    let mut issues: Vec<SemanticIssue> = Vec::new();
    let tokens = tokenize(sql);

    // ① 未知表：顶层 FROM / JOIN 的表引用不在目录里（CTE 名排除，去重）
    //    同时收集「别名 → 真实限定名」映射（供②限定列反查来源表）
    let cte_names: Vec<&str> = select.ctes.iter().map(|cte| cte.name.as_str()).collect();
    let mut sources: Vec<&TableRef> = Vec::new();
    if let Some(from) = &select.from {
        collect_table_refs(&from.source, &mut sources);
    }
    for join in &select.joins {
        collect_table_refs(&join.table, &mut sources);
    }
    let mut alias_map: Vec<(String, String)> = Vec::new();
    let mut seen_tables: Vec<String> = Vec::new();
    for table in &sources {
        if let Some(alias) = &table.alias {
            alias_map.push((alias.clone(), qualify_table_name(table)));
        }
        let qualified = qualify_table_name(table);
        if cte_names.contains(&qualified.as_str()) {
            continue;
        }
        let known = tables.iter().any(|t| same_table(t.qualified_name, &qualified));
        if !known && !seen_tables.contains(&qualified) {
            seen_tables.push(qualified.clone());
            if let Some((start, end)) = locate(sql, &tokens, &qualified) {
                issues.push(SemanticIssue::new(
                    "unknown_table",
                    format!("未知表：{qualified}（目录里没有这张表）"),
                    start,
                    end,
                ));
            }
        }
    }

    // ② 未限定列：qualify 后仍是 `Column { table: None }`（解析不到来源）；
    //    限定列 `o.missing`：qualify 原样保留限定符（0.10.33 `transform` 只处理
    //    裸列，实测），按别名反查来源表，列不在该表列集同样报。
    //    **口径**：顶层含裸 `SELECT *`（`SelectItem::Wildcard`）时跳过裸列诊断——
    //    qualify 会把 * 展开成 `Column { table: None }`（qualify_select 步骤 5
    //    实证），与「真查不到来源的裸列」无法从 AST 区分，宁缺毋滥不误报；
    //    限定列（`o.missing`）不受影响，照常诊断。
    let has_bare_wildcard = select
        .columns
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard));
    let qualified = qualify_columns(stmt, &schema);
    let mut seen_columns: Vec<String> = Vec::new();
    for (name, source_table) in collect_unqualified_columns(&qualified, tables, &alias_map) {
        if has_bare_wildcard && source_table.is_none() {
            continue;
        }
        if !seen_columns.contains(&name) {
            seen_columns.push(name.to_string());
            // 裸列 / 限定列都用全语句 token 定位（列名首次出现；限定列在
            // 原文里有 token，* 展开列因 has_bare_wildcard 已被跳过）
            if let Some((start, end)) = locate(sql, &tokens, &name) {
                let message = match source_table {
                    None => format!("列「{name}」在任一来源表里都找不到"),
                    Some(table) => format!("列「{name}」在表 {table} 中不存在"),
                };
                issues.push(SemanticIssue::new("unqualified_column", message, start, end));
            }
        }
    }

    issues
}

/// qualify 后遍历：收集仍是 `Column { table: None }` 的列（解析不到来源），
/// 以及限定列（`o.missing`）反查来源表后列不在该表列集的。返回
/// `(列名, 来源表名)`——`None` 表名 = 任一来源都没有；`Some` = 在具体表里不存在。
fn collect_unqualified_columns(
    statement: &Statement,
    tables: &[CatalogTable],
    alias_map: &[(String, String)],
) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    walk(statement, tables, alias_map, &mut out);
    out
}

/// 顶层 SELECT 表达式层遍历（不深入 CTE / 子查询内部的嵌套 Statement——
/// 模块注释「只做顶层」口径）
fn walk(
    statement: &Statement,
    tables: &[CatalogTable],
    alias_map: &[(String, String)],
    out: &mut Vec<(String, Option<String>)>,
) {
    let Statement::Select(select) = statement else {
        return;
    };
    for item in &select.columns {
        walk_select_item(item, tables, alias_map, out);
    }
    for join in &select.joins {
        // JOIN ON 的表达式位（ON 里的列同属来源层）
        if let Some(on) = &join.on {
            walk_expr(on, tables, alias_map, out);
        }
    }
    if let Some(where_clause) = &select.where_clause {
        walk_expr(where_clause, tables, alias_map, out);
    }
    for expr in &select.group_by {
        walk_expr(expr, tables, alias_map, out);
    }
    if let Some(having) = &select.having {
        walk_expr(having, tables, alias_map, out);
    }
    for item in &select.order_by {
        walk_expr(&item.expr, tables, alias_map, out);
    }
    if let Some(limit) = &select.limit {
        walk_expr(limit, tables, alias_map, out);
    }
    if let Some(offset) = &select.offset {
        walk_expr(offset, tables, alias_map, out);
    }
}

fn walk_select_item(
    item: &SelectItem,
    tables: &[CatalogTable],
    alias_map: &[(String, String)],
    out: &mut Vec<(String, Option<String>)>,
) {
    if let SelectItem::Expr { expr, .. } = item {
        walk_expr(expr, tables, alias_map, out);
    }
}

/// 表达式层遍历：精确匹配 0.10.33 实测变体（types.rs `pub enum Expr`），
/// 容器变体深入、列变体判定、其余（字面量 / 嵌套 Statement 的 subquery /
/// Wildcard 等）跳过。**不要凭 sqlglot-python 记忆加变体**。
fn walk_expr(
    expr: &Expr,
    tables: &[CatalogTable],
    alias_map: &[(String, String)],
    out: &mut Vec<(String, Option<String>)>,
) {
    match expr {
        // 裸列 qualify 后仍是 None = 任一来源都解析不到（真错）
        Expr::Column { table: None, name, .. } => {
            out.push((name.clone(), None));
        }
        // 限定列：qualify 原样保留限定符（0.10.33 transform 只处理裸列）——
        // 反查来源真实表名（别名 → 表），列不在该表列集 = 错
        Expr::Column {
            table: Some(table),
            name,
            ..
        } => {
            let resolved = alias_map
                .iter()
                .find(|(alias, _)| alias == table)
                .map(|(_, real)| real.clone())
                .unwrap_or_else(|| table.clone());
            // 来源表在目录里才判列存在性（不在目录：unknown_table 已报，不重复）
            if tables.iter().any(|t| same_table(t.qualified_name, &resolved)) {
                let has_column = tables.iter().any(|t| {
                    same_table(t.qualified_name, &resolved)
                        && t.columns.iter().any(|c| c.eq_ignore_ascii_case(name))
                });
                if !has_column {
                    out.push((name.clone(), Some(resolved)));
                }
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            walk_expr(left, tables, alias_map, out);
            walk_expr(right, tables, alias_map, out);
        }
        Expr::UnaryOp { expr, .. } => walk_expr(expr, tables, alias_map, out),
        Expr::Function { args, .. } => {
            for arg in args {
                walk_expr(arg, tables, alias_map, out);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            walk_expr(expr, tables, alias_map, out);
            walk_expr(low, tables, alias_map, out);
            walk_expr(high, tables, alias_map, out);
        }
        Expr::InList { expr, list, .. } => {
            walk_expr(expr, tables, alias_map, out);
            for item in list {
                walk_expr(item, tables, alias_map, out);
            }
        }
        // 子查询（Statement）内部不在本版范围：只遍历子查询外的左侧表达式
        Expr::InSubquery { expr, .. } => walk_expr(expr, tables, alias_map, out),
        Expr::AnyOp { expr, right, .. } | Expr::AllOp { expr, right, .. } => {
            walk_expr(expr, tables, alias_map, out);
            walk_expr(right, tables, alias_map, out);
        }
        Expr::IsNull { expr, .. } | Expr::IsBool { expr, .. } => {
            walk_expr(expr, tables, alias_map, out)
        }
        Expr::Like { expr, pattern, .. } | Expr::ILike { expr, pattern, .. } => {
            walk_expr(expr, tables, alias_map, out);
            walk_expr(pattern, tables, alias_map, out);
        }
        Expr::Case {
            operand,
            when_clauses,
            else_clause,
        } => {
            if let Some(operand) = operand {
                walk_expr(operand, tables, alias_map, out);
            }
            for (when, then) in when_clauses {
                walk_expr(when, tables, alias_map, out);
                walk_expr(then, tables, alias_map, out);
            }
            if let Some(else_clause) = else_clause {
                walk_expr(else_clause, tables, alias_map, out);
            }
        }
        Expr::Cast { expr, .. } | Expr::TryCast { expr, .. } => {
            walk_expr(expr, tables, alias_map, out)
        }
        Expr::Nested(expr) => walk_expr(expr, tables, alias_map, out),
        // 字面量 / 嵌套 Statement 的 subquery / Wildcard / 其他变体：跳过
        _ => {}
    }
}

// ---------- token 层定位（原语在 refs：tokenize / locate / to_inner_dialect） ----------


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

    fn codes(issues: &[SemanticIssue]) -> Vec<&'static str> {
        issues.iter().map(|i| i.code).collect()
    }

    /// 区间指向原文里的目标词（校验位置映射没跑偏）
    fn span_text<'a>(sql: &'a str, issue: &SemanticIssue) -> &'a str {
        &sql[issue.start..issue.end]
    }

    #[test]
    fn single_table_bare_columns_resolve_clean() {
        let issues = diagnose("SELECT id, total FROM orders", SqlDialect::Ansi, &tables());
        assert_eq!(issues, Vec::new(), "裸列能归位就不是错");
    }

    #[test]
    fn unknown_column_is_reported_with_position() {
        let sql = "SELECT nope FROM orders";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unqualified_column"]);
        assert_eq!(span_text(sql, &issues[0]), "nope", "区间指向列名本身");
    }

    #[test]
    fn qualified_unknown_column_is_reported() {
        let sql = "SELECT o.missing FROM orders o";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unqualified_column"]);
        assert!(issues[0].message.contains("missing"));
    }

    #[test]
    fn unknown_table_is_reported_with_position() {
        let sql = "SELECT * FROM missing_table";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unknown_table"]);
        assert_eq!(span_text(sql, &issues[0]), "missing_table");
    }

    #[test]
    fn qualified_wildcard_unknown_table() {
        let sql = "SELECT m.* FROM missing m";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unknown_table"]);
    }

    #[test]
    fn join_bare_column_with_no_source_is_reported() {
        let sql = "SELECT a.id, y FROM orders a JOIN public.customers b ON a.id = b.id";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unqualified_column"]);
        assert!(issues[0].message.contains("y"));
    }

    #[test]
    fn schema_qualified_table_is_known_by_last_segment() {
        let sql = "SELECT name FROM customers";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(issues, Vec::new(), "目录里是 public.customers，引用裸名 customers 也认");
    }

    #[test]
    fn cte_is_not_reported_as_unknown_table() {
        let sql = "WITH c AS (SELECT id FROM orders) SELECT * FROM c";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(issues, Vec::new(), "CTE 是派生表，不在目录里是正常");
    }

    #[test]
    fn where_and_order_and_function_columns_are_checked() {
        // WHERE 的未知列、ORDER BY 的未知列都要报；函数参数里的列也遍历
        let sql = "SELECT count(*) FROM orders WHERE nope = 1 ORDER BY also_missing";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unqualified_column", "unqualified_column"]);
    }

    #[test]
    fn non_select_and_parse_error_produce_nothing() {
        assert!(diagnose("INSERT INTO orders VALUES (1)", SqlDialect::Ansi, &tables()).is_empty());
        assert!(diagnose("SELECT FROM", SqlDialect::Ansi, &tables()).is_empty());
    }

    #[test]
    fn no_catalog_degrades_to_empty() {
        // 无目录 = 调用方没有表级元数据：宁缺毋滥，不做全表未知的误报
        assert!(diagnose("SELECT * FROM anything", SqlDialect::Ansi, &[]).is_empty());
    }

    #[test]
    fn case_expression_columns_are_checked() {
        let sql =
            "SELECT CASE WHEN status = 'x' THEN nope ELSE id END FROM orders";
        let issues = diagnose(sql, SqlDialect::Ansi, &tables());
        assert_eq!(codes(&issues), vec!["unqualified_column"]);
    }
}
