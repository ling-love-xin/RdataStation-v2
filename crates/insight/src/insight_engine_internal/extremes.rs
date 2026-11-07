//! 数值列极端值检测

use crate::model::types::ExtremeValue;

/// 数值列极端值检测（右上界启发式）。
///
/// 判定条件：标准差为正，且极差既大于 10 倍标准差、又大于 1000——
/// 两者同时成立才算，避免小量级数据的轻微离群被报成异常。
///
/// 归属变更（M8 Phase 0 / 0.2）：本函数自 `engine/services/duckdb_service.rs` 迁入。
/// 它实现的是**洞察的业务启发式**（阈值属领域知识），且返回洞察领域类型，
/// 住在 engine 会造成方向倒置（低层持有上层的业务规则与词汇）。
pub fn detect_extremes(min: f64, max: f64, stddev: f64) -> Vec<ExtremeValue> {
    let mut results = Vec::new();
    if stddev > 0.0 && max > 0.0 {
        let range = max - min;
        if range > 10.0 * stddev && range > 1000.0 {
            results.push(ExtremeValue {
                value: max,
                kind: "outlier_high".to_string(),
            });
        }
    }
    results
}
