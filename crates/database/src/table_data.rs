//! M4 表数据预览（属性面板「数据」页签）：源库某表的**分页抓取**。
//!
//! 与结果集（`rds-editor`）的关系见 `docs/architecture/editor/editor-table-editing-plan.md` §1：
//! 结果集是一次执行的产物（editor 侧，驻临时表），表数据浏览是源库某表的分页抓取（M4 侧）；
//! 两侧**只共享**「怎么画一个单元格」的网格渲染层，状态与取数不合并。
//! 本模块只做**只读预览**；编辑回写（行身份 / 事务 / 暂存）是 `table_editor` 能力位的事，
//! 驱动未声明前不提前实现（能力字典 `Stage::NotBuilt`）。
//!
//! 分页语义（对应 table-editing-plan 验收表）：
//! - **有序**：`ORDER BY` = 用户排序 + 主键（主键跟在后面，不替换用户排序）—— A11
//! - **探测**：`limit + 1` 多取一行判 `has_more`，不为它多跑一次 `COUNT(*)` —— A12
//! - **行数**：真 `COUNT(*)` 单独一问，拿不到如实留空（不编 0）—— A13
//! - **诚实排序**：无用户排序且无主键时如实说「本页无定义顺序」—— A14
//! - **标识符转义**：表名 / 列名不能参数化，一律引号转义（双写内嵌引号）—— A15

use crate::metadata_service::MetadataService;
use engine::services::duckdb_service::rows_as_json;
use property::{TableDataColumn, TableDataPage};
use shared::error::CoreError;

/// 每页默认行数（首屏）。
pub const DEFAULT_PAGE_LIMIT: usize = 200;

/// 表数据分页请求。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableDataRequest {
    /// 跳过行数（「加载更多 / 上一页 / 下一页」改这个）。
    pub offset: usize,
    /// 每页行数（0 = 用 [`DEFAULT_PAGE_LIMIT`]）。
    pub limit: usize,
    /// 用户排序（列名, 升序）；主键自动跟在后面补全序。
    pub order_by: Vec<(String, bool)>,
}

/// 引号转义标识符（表名 / 列名不能参数化；内嵌引号双写——A15 敌意标识符用例）。
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 引号转义的限定名（跳过空段；catalog 与 schema 相同时去重——与 `sql_gen::qualified_name` 同规则，
/// 但本函数输出**可执行 SQL** 的引用形态，那个函数是展示用）。
fn quoted_qualified(catalog: &str, schema: &str, table: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !catalog.is_empty() {
        parts.push(quote_ident(catalog));
    }
    if !schema.is_empty() && schema != catalog {
        parts.push(quote_ident(schema));
    }
    parts.push(quote_ident(table));
    parts.join(".")
}

/// 单元格 → 展示文本：JSON `null` → `None`（渲染置灰），字符串取内部值，
/// 其余按类型字符串化（精确数值保持原样——`Number` 不经浮点）。
///
/// 注意：`serde_json::Value::String` 的 `to_string()` 是 **JSON 序列化**（自带引号），
/// 所以文本列要取内部值；数值 / 布尔列的 `to_string()` 无引号，直接可用。
fn cell_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// 加载表数据的一页（只读）。
///
/// 表 / 视图都走这条（视图无主键时 `is_ordered` 由用户排序决定）。
pub async fn load_table_data(
    metadata: &MetadataService,
    conn_id: &str,
    catalog: &str,
    schema: &str,
    table: &str,
    req: &TableDataRequest,
) -> Result<TableDataPage, CoreError> {
    let limit = if req.limit == 0 { DEFAULT_PAGE_LIMIT } else { req.limit };

    // 1) 列定义 + 主键（排序补齐用）。列是数据网格的骨架，取不到直接报错（不给空表）。
    let columns = metadata
        .list_columns(conn_id, catalog, schema, table)
        .await?;
    if columns.is_empty() {
        return Ok(TableDataPage::empty());
    }
    let pk: Vec<&str> = columns
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| c.name.as_str())
        .collect();

    // 2) SELECT：标识符引号转义 + 用户排序在前、主键补齐在后 + limit+1 探测。
    let select_list = columns
        .iter()
        .map(|c| quote_ident(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let from = quoted_qualified(catalog, schema, table);
    let mut sql = format!("SELECT {select_list} FROM {from}");
    let mut order: Vec<String> = Vec::new();
    for (col, asc) in &req.order_by {
        order.push(format!(
            "{} {}",
            quote_ident(col),
            if *asc { "ASC" } else { "DESC" }
        ));
    }
    for p in &pk {
        if !req.order_by.iter().any(|(c, _)| c == p) {
            order.push(format!("{} ASC", quote_ident(p)));
        }
    }
    if !order.is_empty() {
        sql.push_str(" ORDER BY ");
        sql.push_str(&order.join(", "));
    }
    sql.push_str(&format!(" LIMIT {} OFFSET {}", limit + 1, req.offset));

    let result = metadata.execute_query(conn_id, catalog, &sql).await?;
    let json_rows = rows_as_json(&result);
    let mut rows: Vec<Vec<Option<String>>> = json_rows
        .iter()
        .map(|row| row.iter().map(cell_text).collect())
        .collect();
    let has_more = rows.len() > limit;
    rows.truncate(limit);

    // 3) 行数：真 COUNT 单独一问（拿不到如实留空——不编 0，A13）。
    let count_sql = format!("SELECT count(*) FROM {from}");
    let total = metadata
        .execute_query(conn_id, catalog, &count_sql)
        .await
        .ok()
        .and_then(|r| {
            rows_as_json(&r)
                .first()
                .and_then(|row| row.first())
                .and_then(|v| v.as_u64())
        });

    Ok(TableDataPage {
        columns: columns
            .iter()
            .map(|c| TableDataColumn {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
            })
            .collect(),
        rows,
        total,
        offset: req.offset,
        has_more,
        // 有用户排序或有主键才算「有定义顺序」（A14：无键表如实说明）。
        is_ordered: !req.order_by.is_empty() || !pk.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 敌意标识符：列名 / 表名里的引号必须双写，且整个被引号包住（A15）。
    #[test]
    fn quote_ident_doubles_embedded_quotes() {
        assert_eq!(quote_ident("orders"), "\"orders\"");
        assert_eq!(quote_ident("o\"rder\"s"), "\"o\"\"rder\"\"s\"");
    }

    /// 限定名：去重 catalog / schema（SQLite / DuckDB 把 schema 传成 catalog）。
    #[test]
    fn quoted_qualified_deduplicates_schema_equals_catalog() {
        assert_eq!(
            quoted_qualified("main", "main", "orders"),
            "\"main\".\"orders\""
        );
        assert_eq!(
            quoted_qualified("cat", "sch", "orders"),
            "\"cat\".\"sch\".\"orders\""
        );
        assert_eq!(quoted_qualified("", "", "orders"), "\"orders\"");
    }

    /// 单元格：JSON null → None，字符串取内部值（避免 JSON 序列化自带的引号），
    /// 数值按原文（Number 不走浮点）。
    #[test]
    fn cell_text_maps_null_and_scalars() {
        assert_eq!(cell_text(&serde_json::Value::Null), None);
        assert_eq!(
            cell_text(&serde_json::Value::String("a\"b".into())),
            Some("a\"b".to_string())
        );
        assert_eq!(
            cell_text(&serde_json::Value::Number(serde_json::Number::from(42))),
            Some("42".to_string())
        );
    }
}
