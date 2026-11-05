//! 列洞察 / 表画像的领域类型。
//!
//! 本文件的 struct / enum 同时承载两种契约：
//! - **宿主 ↔ insight 引擎**的数据载荷（列画像、表探查、结构报告的读取结果）；
//! - **持久化**（store 的 body / meta 行）与 **tauri IPC 契约**（`specta::Type` 导出到 TS）。
//!
//! 已从 v1 `services/result_service.rs` 整体抽出落位（Phase 0.2 边界归位完成），
//! 不再有 pending migration。

use specta::Type;

// ==================== 洞察体系 — 顶层结构 ====================

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct ColumnInsightFull {
    pub stats: ColumnStats,
    #[specta(skip)]
    pub sample: Vec<serde_json::Value>,
    pub histogram: Option<Vec<DistributionBin>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct ColumnStats {
    pub column_name: String,
    pub data_type: String,
    pub total_count: u32,
    pub null_count: u32,
    pub null_rate: f64,
    pub unique_count: Option<u32>,
    pub stats_detail: ColumnStatsDetail,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
#[serde(tag = "kind")]
pub enum ColumnStatsDetail {
    Numeric(NumericStats),
    Text(TextStats),
    DateTime(DateTimeStats),
    Boolean(BooleanStats),
    Unknown,
}

// ==================== 数值列统计 ====================

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct NumericStats {
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub median: f64,
    pub p25: f64,
    pub p75: f64,
    pub sum: f64,
    pub stddev: Option<f64>,
    pub skewness: Option<f64>,
    pub kurtosis: Option<f64>,
    /// 0 值个数（2026-11-06 YData 密度对齐；旧快照无此字段 → None）
    #[serde(default)]
    pub zeros: Option<u32>,
    /// 负值个数（同上）
    #[serde(default)]
    pub negatives: Option<u32>,
    /// P95 分位（同上）
    #[serde(default)]
    pub p95: Option<f64>,
    pub is_extreme: Vec<ExtremeValue>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct ExtremeValue {
    pub value: f64,
    pub kind: String,
}

// ==================== 文本列统计 ====================

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct TextStats {
    pub min_length: u32,
    pub max_length: u32,
    pub top_values: Vec<TextFrequency>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct TextFrequency {
    pub value: String,
    pub count: u32,
    pub ratio: f64,
}

// ==================== 日期时间列统计 ====================

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct DateTimeStats {
    pub earliest: String,
    pub latest: String,
    pub span_days: i32,
    pub monthly_distribution: Vec<TextFrequency>,
}

// ==================== 布尔列统计 ====================

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct BooleanStats {
    pub true_count: u32,
    pub false_count: u32,
    pub true_ratio: f64,
}

// ==================== 分箱直方图 ====================

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Type)]
pub struct DistributionBin {
    pub label: String,
    pub count: u32,
    pub ratio: f64,
}

// ==================== 表探查 ====================

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct TableProfile {
    pub table_name: String,
    pub db_type: String,
    pub columns: Vec<TableColumnMeta>,
    pub row_count: Option<i32>,
    pub schema_name: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Type)]
pub struct TableColumnMeta {
    pub column_name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub ordinal_position: i32,
}

// ==================== 历史质量模型（兼容保留） ====================
// 下列结构仅用于：①历史画像快照 JSON schema 只读兼容（旧快照不重算分）；
// ②Schema 结构健康分独立域（`schema_analyzer` 现算 `Grade`）。
// 表 / 列画像的会话级 EDA 视图模型已不合成总分 / 等级 / 加权评分（见 `model.rs`）。

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct QualityScore {
    pub column_name: String,
    pub overall_score: f64,
    pub level: String,
    pub dimensions: Vec<QualityDimension>,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct QualityDimension {
    pub name: String,
    pub score: f64,
    pub weight: f64,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct TableQuality {
    pub table_name: String,
    pub overall_score: f64,
    pub level: String,
    pub column_scores: Vec<ColumnQualityEntry>,
    pub summary: String,
    pub scored_count: u32,
    pub total_columns: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct ColumnQualityEntry {
    pub column_name: String,
    pub quality_score: f64,
    pub level: String,
    pub null_rate: f64,
}
