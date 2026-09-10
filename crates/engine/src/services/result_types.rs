//! 结果集类型（占位于此）
//! TODO(migration): 自 v1 `core/services/result_service.rs` 抽取，
//! 供 engine 内部 sql/duckdb/execution 服务引用；随 workbench 轮次归属确认。

use specta::Type;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Type)]
pub struct ResultSet {
    pub columns: Vec<String>,
    #[specta(skip)]
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: u32,
    pub elapsed_ms: u32,
    pub temp_table: String,
}
