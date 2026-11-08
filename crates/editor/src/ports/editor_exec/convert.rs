//! 结果与单元格的形状换算：引擎 QueryResult → 编辑器 QueryData。

use super::*;

/// 网格里的一个字符串 → JSON 值（桥接给 DuckDB 用）
///
/// `NULL` 是**字面量**（网格与导出同一个判据）→ 真 `null`；其余都是字符串。
pub(crate) fn cell_json(cell: &str) -> serde_json::Value {
    if cell == "NULL" {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(cell.to_string())
    }
}

/// DuckDB 回来的一个 JSON 值 → 网格里的字符串（与 `NULL` 字面量口径一致）
pub(crate) fn json_cell_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// 引擎结果 → 编辑器数据（列 + 字符串化行；不读 `rows` 字段）
///
/// 【A3】同时构造事实帧候选（方案跳 1）：`batches` 非空才挂 `fact`；空批 / 形状不一致
/// → `None` 走显示矩阵降级档。渲染第一期不读 `fact`，分析 / 带类型导出在 A 阶段后消费。
pub(crate) fn to_data(result: &QueryResult, elapsed_ms: u64, truncated: bool) -> QueryData {
    let fact = FactFrame::from_query_result(result).map(Arc::new);
    QueryData {
        columns: column_names(result),
        // 【Q6】列类型：`QueryResult` 已由驱动/Arrow schema 填好（空 = 报不出，界面按无类型档渲染）
        column_types: result.column_types(),
        rows: result
            .to_rows()
            .iter()
            .map(|row| row.iter().map(cell_text).collect())
            .collect(),
        elapsed_ms,
        truncated,
        // B5：写语句的影响行数由驱动报（拿不到就是 `None`，不编造 0）
        affected_rows: result.affected_rows,
        // 非分段路径没有“下一段”可言；分段抓取由调用方按“拿没拿满”标
        has_more: false,
        // 【B14】下发筛选的“已去掉 LIMIT”之类提示由调用方另设
        notice: None,
        fact,
        // 【B4】源库直取回执不是分析产物：不带物化回执信号
        source_fact_table: None,
        source_fact_release: false,
        source_fact_set: 0,
    }
}

/// 列名：优先驱动填的 `columns`，否则从 Arrow schema 取
///
/// 驱动并不保证填 `columns`（架构 §12 #22），所以这里要有兜底，否则网格会出现无名列。
pub(crate) fn column_names(result: &QueryResult) -> Vec<String> {
    if !result.columns.is_empty() {
        return result.columns.clone();
    }
    result
        .batches
        .first()
        .map(|batch| {
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 单元格文案
///
/// 直接走 `Value` 的 `Display`（`NULL` / 数字 / 文本都已定义好）。将来若要按类型区分
/// “NULL”与字符串 “NULL”（1b 的类型化网格），改这里一处并补测试。
pub(crate) fn cell_text(value: &Value) -> String {
    value.to_string()
}
