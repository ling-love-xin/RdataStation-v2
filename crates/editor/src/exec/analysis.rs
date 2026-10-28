//! DuckDB 分析入口的**纯模型**（B15 切片一）
//!
//! ## 这件事在做什么
//!
//! 结果集 → 「分析 ▾」→ 在**本地 DuckDB** 上对这份结果跑一条聚合 SQL → 落**新结果集**。
//! 原结果一行不动（与下发筛选 / 排序同一口径：产生新结果集，不就地替换）。
//!
//! ## 数据从哪来（**优先走事实档 batches 直灌，显示矩阵为降级档**）
//!
//! 分析**不重跑源库查询**：优先把当前结果集的事实档 Arrow batches 直灌本地 DuckDB
//! 常驻表（类型来自驱动上报的 schema，不猜型）；事实档缺失（失败 / 写语句 / 加速档 /
//! 联邦档 / JSON 往返）才回退到显示矩阵字符串逐行建表（internal 猜型降级档，界面与报告
//! 须显式标注）。两条路径都只覆盖**已抓到的行**，与「只导已抓取的行」（B7）口径一致——
//! 没抓到的行不参与，并在结果里说清楚。
//!
//! 降级档失真边界（事实档缺失时才出现）：值按展示文本进 DuckDB（`NULL` → 真 NULL，
//! 其余是字符串），`infer_type` 只认 JSON 值类型、整列落 `VARCHAR`——数字列被收窄成
//! 文本、类型失真；要按数值比较就在分析 SQL 里 `CAST(... AS DOUBLE)`。事实档直灌路径
//! 不存在这条失真。
//!
//! ## 为什么只给这几条预置
//!
//! 菜单**只摆真能跑、且结果可预期**的项：计数、逐列分组计数。更复杂的需求属于“自己写 SQL”
//! （切片二的自定义分析 SQL），而不是在这里猜用户想算什么。
//!
//! ## 拆分（2026-10-23）
//!
//! 纯模型部分（`AnalysisRequest` / `AnalysisItem` / `menu_items` / `request_for` 等）随
//! 结果面板下移 `rds-result-panel`；依赖 editor 执行模型的 `result_entry`（把 `QueryData`
//! 落成结果记录）留在本文件——result_panel 不反向依赖 editor。

use crate::execution::QueryData;
use crate::model::DocumentId;
use crate::store::ResultEntry;

// 纯模型下移结果面板：菜单 / 桥接请求 / 常量都在 result_panel 侧，这里重导出保
// 持 `crate::analysis::…` 路径不变
pub use result_panel::analysis::*;

/// 一张分析结果的空壳（执行器回填用：把 `QueryData` 落成结果记录）
///
/// 放在这里而不是面板里：**“分析结果长什么样”是模型问题**（标题 / 通道 / 桥接说明），
/// 面板只负责画。
pub fn result_entry(
    document: &DocumentId,
    request: &AnalysisRequest,
    data: &QueryData,
) -> ResultEntry {
    ResultEntry::success(
        document.clone(),
        request.sql.clone(),
        data.elapsed_ms,
        data.truncated,
        data.columns.clone(),
        data.rows.clone(),
    )
    .with_has_more(false)
    .with_analysis(true)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{AnalysisRequest, result_entry};
    use crate::execution::QueryData;
    use crate::model::DocumentId;
    use ::shared::StoragePref;

    #[test]
    fn an_analysis_result_is_marked_and_titled_by_the_caller() {
        let request = AnalysisRequest {
            sql: "SELECT count(*) FROM {table}".to_string(),
            columns: vec!["id".to_string()],
            rows: vec![vec!["1".to_string()]],
            dropped_rows: 0,
            fact: None,
            pref: StoragePref::Auto,
            source_set: 0,
        };
        let data = QueryData {
            columns: vec!["行数".to_string()],
            rows: vec![vec!["1".to_string()]],
            elapsed_ms: 4,
            truncated: false,
            notice: None,
            ..Default::default()
        };
        let result = result_entry(&DocumentId::new("doc"), &request, &data);
        assert!(result.analysis, "分析结果要有标记（工具栏据此不摆“刷新”）");
        assert!(!result.can_fetch_more(), "分析结果没有“下一段”");
        assert_eq!(result.row_count(), 1);
    }
}
