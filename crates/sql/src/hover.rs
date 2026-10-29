//! SQL hover 信息（#54 ②）：编辑器悬停时展示引用信息
//!
//! ## 数据来源（engine 内纯函数，无 I/O）
//!
//! - **函数**：`functions::builtin_signatures`（签名）+ `function_manual`（中文描述/示例）
//! - **表**：调用方目录（`CatalogTable`，与 `semantic` 同款）→ 列列表
//! - **列**：AST 里的列引用 → 所属表；**类型暂不展示**——目录只有列名没有列类型，
//!   `annotate_types` 会全给 `Unknown`（MappingSchema 无类型信息），无展示价值；
//!   待目录携带列类型后再接（文档：`annotate_types(stmt, &schema) -> TypeAnnotations`）。
//!
//! ## 定位口径（与 semantic 一致）
//!
//! sqlglot AST 不带 position（0.10.33 实测），悬停目标靠 **token 层**定位：
//! 给定字节位置 → 落在哪个标识符 token → 取词 → 与「收集到的引用」
//! （函数名 / 表名 / 列名，大小写不敏感）匹配。同名优先：**函数 > 表 > 列**
//! （函数调用结构最明确；表 / 列同名时 AST 角色判定留后续版本，文档注明）。
//!
//! ## 限制
//!
//! - 只做顶层 SELECT（FROM/JOIN 表 + 表达式层列/函数）；子查询 / CTE 内部不收集
//!   ——与 semantic「只做顶层」口径一致，打字期价值在顶层。
//! - 无目录时表 hover 退化为「仅表名」（列列表不可得），函数 hover 不受影响。

use sqlglot_rust::ast::{DataType, Expr, SelectItem, Statement, TableRef};
use sqlglot_rust::optimizer::qualify_columns::qualify_columns;
use sqlglot_rust::schema::MappingSchema;
use sqlglot_rust::tokens::TokenType;
use sqlglot_rust::parse;

use super::engine::SqlDialect;
use super::function_manual::search_manual;
use super::functions::builtin_signatures;
use super::refs::{
    collect_column_refs, collect_table_refs, qualify_table_name, same_table, to_inner_dialect,
    token_byte_spans, tokenize,
};
use super::semantic::CatalogTable;

/// hover 目标类别（UI 决定图标/配色）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoverKind {
    Table,
    Column,
    Function,
}

/// 一条 hover 信息（**字节区间**，与 semantic 同构）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    pub kind: HoverKind,
    /// 标题（引用词本身）
    pub title: String,
    /// 内容行（签名 / 列列表 / 描述 / 示例）
    pub lines: Vec<String>,
    pub start: usize,
    pub end: usize,
}

impl HoverInfo {
    fn new(kind: HoverKind, title: String, lines: Vec<String>, start: usize, end: usize) -> Self {
        Self {
            kind,
            title,
            lines,
            start,
            end,
        }
    }
}

/// hover 入口：给定字节位置，返回该处引用的信息（无引用 / 词法失败 → None）
///
/// `tables` 为空 = 未连接 / 缓存未载：表与列 hover 退化，函数 hover 照常。
pub fn hover_at(
    sql: &str,
    dialect: SqlDialect,
    tables: &[CatalogTable],
    byte_pos: usize,
) -> Option<HoverInfo> {
    let tokens = tokenize(sql);
    if tokens.is_empty() {
        return None;
    }
    // 字符下标 → 字节区间（与 semantic::locate 同款换算）
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
    // 有目录时先 qualify（按值接收）：裸列归位成 `t.col`，列 hover 才有所属表
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

    // 同名优先：函数（内置收录）> 表 > 列（qualified）> 函数（AST 确认未收录）
    if let Some(lines) = function_hover(&word) {
        return Some(HoverInfo::new(
            HoverKind::Function,
            tokens[idx].value.clone(),
            lines,
            start,
            end,
        ));
    }
    if let Some(lines) = table_hover(&word, select, tables) {
        return Some(HoverInfo::new(
            HoverKind::Table,
            tokens[idx].value.clone(),
            lines,
            start,
            end,
        ));
    }
    if let Some(lines) = column_hover(&word, select) {
        return Some(HoverInfo::new(
            HoverKind::Column,
            tokens[idx].value.clone(),
            lines,
            start,
            end,
        ));
    }
    // AST 确认是函数调用（UDF / 引擎特有，内置表与手册没收录）→ 兜底说明
    if is_function_call(select, &word) {
        return Some(HoverInfo::new(
            HoverKind::Function,
            tokens[idx].value.clone(),
            vec![format!("函数「{word}」——内置签名表未收录，以当前连接的系统表为准")],
            start,
            end,
        ));
    }
    None
}

/// 函数 hover：签名（内置兜底表）+ 中文描述 + 示例
///
/// 只在**内置表或中文手册有收录**时返回 `Some`（否则交给表/列判定）——
/// 未收录但 AST 确认是函数的兜底说明由调用方（`is_function_call`）负责。
fn function_hover(word: &str) -> Option<Vec<String>> {
    let sigs = builtin_signatures();
    let sig = sigs
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(word));
    let manual = search_manual(word)
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(word))
        .copied();
    if sig.is_none() && manual.is_none() {
        return None;
    }

    let mut lines = Vec::new();
    if let Some(sig) = sig {
        let params: Vec<&str> = sig
            .params
            .iter()
            .map(|p| p.name.as_deref().unwrap_or("?"))
            .collect();
        lines.push(format!(
            "{}({}) → {}",
            sig.name,
            params.join(", "),
            sig.return_type.as_deref().unwrap_or("?")
        ));
    }
    if let Some(manual) = manual {
        lines.push(manual.zh.to_string());
        lines.push(format!("示例：{}", manual.example));
    }
    Some(lines)
}

/// AST 里是否有名为 `word` 的函数调用（顶层表达式层；判断「未收录兜底」用）
fn is_function_call(select: &sqlglot_rust::ast::SelectStatement, word: &str) -> bool {
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
    exprs.iter().any(|expr| {
        let mut found = false;
        expr.walk(&mut |e| {
            if let Expr::Function { name, .. } = e {
                if name.eq_ignore_ascii_case(word) {
                    found = true;
                }
            }
            !found
        });
        found
    })
}

/// 表 hover：限定名 + 列列表（目录里查得到才给列）
fn table_hover(word: &str, select: &sqlglot_rust::ast::SelectStatement, tables: &[CatalogTable]) -> Option<Vec<String>> {
    let mut sources: Vec<&TableRef> = Vec::new();
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
    // 展示名优先取目录里的真实限定名（原文可能只写裸名 customers）
    let catalog_match = tables.iter().find(|t| same_table(t.qualified_name, &qualified));
    let display_name = catalog_match
        .map(|t| t.qualified_name.to_string())
        .unwrap_or_else(|| qualified.clone());

    let mut lines = vec![format!("表 {display_name}")];
    if let Some(catalog) = catalog_match {
        let columns: Vec<&str> = catalog.columns.iter().copied().collect();
        if columns.is_empty() {
            lines.push("（目录无列信息）".to_string());
        } else {
            let shown = columns.len().min(10);
            lines.push(format!(
                "列 {}：{}",
                columns.len(),
                columns[..shown].join(", ") + if columns.len() > shown { "…" } else { "" }
            ));
        }
    } else {
        lines.push("（表不在当前目录，无法给出列列表）".to_string());
    }
    Some(lines)
}

/// 列 hover：所属表（AST 限定符）+ 提示（类型待目录携带后补）
fn column_hover(word: &str, select: &sqlglot_rust::ast::SelectStatement) -> Option<Vec<String>> {
    let mut columns: Vec<(String, Option<String>)> = Vec::new();
    collect_column_refs(select, &mut columns);
    // 收集时保留限定符：`o.total` 与裸 `total` 都算 total
    let hit = columns
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))?;
    let mut lines = vec![format!("列 {word}")];
    if let Some(qual) = &hit.1 {
        lines.push(format!("所属表：{qual}"));
    }
    lines.push("类型信息待目录携带列类型后补充".to_string());
    Some(lines)
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

    /// 字节位置辅助：SQL 原文里第 n 个 `word` 出现处（测试用它模拟悬停点）
    fn pos_of(sql: &str, word: &str, nth: usize) -> usize {
        sql.match_indices(word).nth(nth).map(|(i, _)| i).expect("词存在")
    }

    fn span_text<'a>(sql: &'a str, h: &HoverInfo) -> &'a str {
        &sql[h.start..h.end]
    }

    #[test]
    fn function_hover_shows_signature_and_chinese() {
        let sql = "SELECT UPPER(name) FROM customers";
        let h = hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "UPPER", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Function);
        assert_eq!(span_text(sql, &h), "UPPER");
        assert!(h.lines.iter().any(|l| l.contains("UPPER")), "{:?}", h.lines);
        assert!(h.lines.iter().any(|l| l.contains("大写")), "{:?}", h.lines);
        assert!(h.lines.iter().any(|l| l.contains("示例")), "{:?}", h.lines);
    }

    #[test]
    fn table_hover_lists_columns_from_catalog() {
        let sql = "SELECT * FROM orders";
        let h = hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "orders", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Table);
        assert!(h.lines.iter().any(|l| l.contains("id") && l.contains("total")), "{:?}", h.lines);
    }

    #[test]
    fn schema_qualified_table_hover_resolves_by_last_segment() {
        let sql = "SELECT * FROM customers";
        let h = hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "customers", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Table);
        assert!(h.lines.iter().any(|l| l.contains("public.customers")), "{:?}", h.lines);
    }

    #[test]
    fn column_hover_shows_owning_table() {
        let sql = "SELECT total FROM orders";
        let h = hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "total", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Column);
        assert!(h.lines.iter().any(|l| l.contains("orders")), "{:?}", h.lines);
    }

    #[test]
    fn function_hover_works_without_catalog() {
        // 未连接也能 hover 函数（内置签名 + 中文手册不依赖目录）
        let sql = "SELECT TRIM(name) FROM x";
        let h = hover_at(sql, SqlDialect::Ansi, &[], pos_of(sql, "TRIM", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Function);
        assert!(h.lines.iter().any(|l| l.contains("去")), "{:?}", h.lines);
    }

    #[test]
    fn keyword_and_literal_positions_return_none() {
        let sql = "SELECT * FROM orders";
        // SELECT 关键字与 * 的位置不是标识符 → None
        assert!(hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "SELECT", 0)).is_none());
        assert!(hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "*", 0)).is_none());
    }

    #[test]
    fn multibyte_prefix_does_not_shift_positions() {
        // 多字节注释在目标词之前：字符下标 → 字节偏移换算不能跑偏
        let sql = "SELECT id -- 中文注释\nFROM orders";
        let h = hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "orders", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Table);
        assert_eq!(span_text(sql, &h), "orders");
    }

    #[test]
    fn function_precedence_over_same_named_column() {
        // 同名时函数优先（口径固定：函数调用结构最明确）
        let sql = "SELECT count(*) FROM orders";
        let h = hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "count", 0))
            .expect("有 hover");
        assert_eq!(h.kind, HoverKind::Function);
    }

    #[test]
    fn non_select_and_parse_error_produce_nothing() {
        assert!(hover_at("INSERT INTO orders VALUES (1)", SqlDialect::Ansi, &tables(), 2).is_none());
        assert!(hover_at("SELECT FROM", SqlDialect::Ansi, &tables(), 3).is_none());
    }

    #[test]
    fn unknown_word_in_select_is_none() {
        // 词不是函数 / 表 / 列（如字面量别名）→ None
        let sql = "SELECT 1 AS x FROM orders";
        assert!(hover_at(sql, SqlDialect::Ansi, &tables(), pos_of(sql, "x", 0)).is_none());
    }
}
