use super::*;
use chrono::Utc;
use serde_json::Value;
use shared::error::{CoreError, StorageError};

/// 归档资源表的固定列顺序：单行读取、列表、分页三处必须一致。
///
/// v1 三处各写一遍，且 JSON 解析策略不同（列表宽容 / 单行硬报错）——同一份坏数据会
/// "列表里看得到、点进去报错"。现统一走 [`map_resource_row`]：降级为 `null` 并记 warn，
/// 一行坏数据不应该毁掉整个列表。
pub(crate) const RESOURCE_COLUMNS: &str = "\
    id, resource_type, name, alias, config, scope, row_count, column_count, file_size, \
    version, parent_version_id, parent_resource_id, source_query, created_at, updated_at, \
    created_by, deleted_at, \
    kind, content_hash, file_rel_path, readonly, promoted_from, source_connection_id, \
    source_table, definition_sql, archived_at, \
    payload_mode, source_path";

/// 给固定列名加上表别名前缀（`r.id, r.resource_type, …`）。
///
/// JOIN 查询（如按标签反查资源）复用同一份列顺序，避免又抄一遍。
pub(crate) fn qualified_resource_columns(alias: &str) -> String {
    RESOURCE_COLUMNS
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 行 → 领域对象（列顺序与 [`RESOURCE_COLUMNS`] 严格一致）。
///
/// `pub(crate)`：回收站与标签查询同样需要读整行，**不得各自再抄一份映射**
/// （v1 抄了 4 份，加一列就得改 4 处，且 JSON 策略各不相同）。
pub(crate) fn map_resource_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalyticsResource> {
    let config_str: String = row.get(4)?;
    let config: Value = serde_json::from_str(&config_str).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "资源 config JSON 解析失败，降级为 null");
        Value::Null
    });

    Ok(AnalyticsResource {
        id: row.get(0)?,
        resource_type: row.get(1)?,
        name: row.get(2)?,
        alias: row.get(3)?,
        config,
        scope: row.get(5)?,
        row_count: row.get::<_, Option<i64>>(6)?.map(|v| v as i32),
        column_count: row.get(7)?,
        file_size: row.get::<_, Option<i64>>(8)?.map(|v| v as i32),
        version: row.get(9)?,
        parent_version_id: row.get(10)?,
        parent_resource_id: row.get(11)?,
        source_query: row.get(12)?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(13)?)?,
        updated_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(14)?)?,
        created_by: row.get(15)?,
        deleted_at: row
            .get(16)
            .ok()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
        kind: row.get(17)?,
        content_hash: row.get(18)?,
        file_rel_path: row.get(19)?,
        readonly: row.get(20)?,
        promoted_from: row.get(21)?,
        source_connection_id: row.get(22)?,
        source_table: row.get(23)?,
        definition_sql: row.get(24)?,
        archived_at: row
            .get::<_, Option<String>>(25)
            .ok()
            .flatten()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
        payload_mode: row.get(26)?,
        source_path: row.get(27)?,
    })
}

/// 统一的持久层错误构造（v1 每处手写一遍四元组）。
fn persistence_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 列在 SQLite 里是 64 位，而模型（v1 遗留）是 `i32`——直接把大文件的值写进去，
/// 读回时 `as i32` 会变出一个负数体积。超过 2 GiB 就记为 2 GiB（显示与排序都不失真到不可读）。
fn clamp_file_size(bytes: Option<i64>) -> Option<i64> {
    bytes.map(|value| value.clamp(0, i32::MAX as i64))
}

/// 规模（行数）从 `i64` 落库为 `i32`：同样夹紧到 `i32` 上限。
///
/// 行数过 `i32` 上限的表本身就不适合归档为“存档”（回执与详情都放不下），
/// 但**不报错**：夹紧比拒绝归档更不意外（与体积同一取舍，见 `clamp_file_size`）。
fn clamp_row_count(rows: Option<i64>) -> Option<i32> {
    rows.map(|value| value.clamp(0, i32::MAX as i64) as i32)
}

impl AnalyticsResourceStore {
}

mod read;
mod trash;
mod write;
