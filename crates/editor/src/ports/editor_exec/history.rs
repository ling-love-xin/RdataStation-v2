//! 执行历史落库（加速档与源库档共用同一条流水）。

use super::*;

/// 加速档的执行也进同一条历史流水（与源库档一致）
///
/// `db_type` 写**源库的真实类型**、`channel` 写 `accelerated`：两个维度分开记——
/// “数据是什么库的”与“语句在哪儿跑的”是两件事，历史面板按它们拼出 `MYSQL·加速`。
pub(crate) fn record_accel_history(
    source: &accel::AccelSource,
    sql: &str,
    elapsed_ms: u64,
    outcome: &Result<QueryResult, shared::error::CoreError>,
    lineage: Option<&str>,
) {
    let entry = match outcome {
        Ok(result) => SqlHistoryEntry {
            conn_id: Some(source.conn_id.clone()),
            db_type: Some(source.kind.label().to_string()),
            elapsed_ms,
            success: true,
            error_message: None,
            rows_returned: Some(result.total_rows() as u64),
            rows_affected: None,
            channel: Some(crate::channel::ExecChannel::Accelerated.code().to_string()),
            sources: None,
            lineage: lineage.map(str::to_string),
            // 【D8】加速档的语句是 **DuckDB 引擎**跑的；源库的库种仍在 `db_type`
            driver_id: Some("duckdb".to_string()),
        },
        Err(error) => SqlHistoryEntry {
            conn_id: Some(source.conn_id.clone()),
            db_type: Some(source.kind.label().to_string()),
            elapsed_ms,
            success: false,
            error_message: Some(error.to_string()),
            rows_returned: None,
            rows_affected: None,
            channel: Some(crate::channel::ExecChannel::Accelerated.code().to_string()),
            sources: None,
            lineage: lineage.map(str::to_string),
            driver_id: Some("duckdb".to_string()),
        },
    };
    if let Err(error) = history_store::save_sql_history(sql, &entry) {
        tracing::error!(error = %error, "本地加速的执行未记入历史");
    }
}
