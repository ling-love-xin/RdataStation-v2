//! 洞察模型 · 列展示辅助类型（自 model.rs 拆分，纯位移）

use super::*;
// ==================== 列画像视图模型 ====================

/// 列类型族（决定四区的渲染口径）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Numeric,
    Text,
    DateTime,
    Boolean,
    /// 类型未识别（BLOB / ARRAY / 全空等）：只给基础计数，不渲染分布与类型专属统计
    Unknown,
}

impl ColumnKind {
    pub fn of(detail: &ColumnStatsDetail) -> Self {
        match detail {
            ColumnStatsDetail::Numeric(_) => ColumnKind::Numeric,
            ColumnStatsDetail::Text(_) => ColumnKind::Text,
            ColumnStatsDetail::DateTime(_) => ColumnKind::DateTime,
            ColumnStatsDetail::Boolean(_) => ColumnKind::Boolean,
            ColumnStatsDetail::Unknown => ColumnKind::Unknown,
        }
    }

    /// 类型徐标文案
    pub fn label(self) -> &'static str {
        match self {
            ColumnKind::Numeric => "数值",
            ColumnKind::Text => "文本",
            ColumnKind::DateTime => "时间",
            ColumnKind::Boolean => "布尔",
            ColumnKind::Unknown => "未识别",
        }
    }

    /// 类型族的**英文名**：与规则 `applies_to` 里的取值对齐（`Numeric` / `Text` /
    /// `DateTime` / `Boolean`），用于「这条规则吃不吃这几列」的判定。
    ///
    /// 与 [`Self::label`] 分开：展示用中文，**判定不能拿展示文案当键**
    /// （改文案不应改变规则筛选结果）。
    pub fn family_name(self) -> &'static str {
        match self {
            ColumnKind::Numeric => "Numeric",
            ColumnKind::Text => "Text",
            ColumnKind::DateTime => "DateTime",
            ColumnKind::Boolean => "Boolean",
            ColumnKind::Unknown => "Unknown",
        }
    }

    /// 由**类型名**推类型族（表探查只拿得到元数据，没有统计结果可看）。
    ///
    /// 判定复用 [`is_numeric_type`] 等谓词：它们与列画像的分派共用一套口径，
    /// 两处各写一份「什么算数值」迟早分岔。
    pub fn of_type_name(data_type: &str) -> Self {
        let dt = data_type.to_lowercase();
        if is_numeric_type(&dt) {
            ColumnKind::Numeric
        } else if is_datetime_type(&dt) {
            ColumnKind::DateTime
        } else if dt == "boolean" || dt == "bool" {
            ColumnKind::Boolean
        } else if dt.is_empty() || is_binary_type(&dt) || is_array_type(&dt) {
            // 二进制 / 数组族不参与列统计，与列画像的 `Unknown` 同口径
            ColumnKind::Unknown
        } else {
            ColumnKind::Text
        }
    }
}

// ==================== 类型族判定（唯一来源） ====================
//
// 归属变更（Phase 3.1）：自 `insight_engine` 迁入。它同时服务两处消费者——
// 列画像（决定走哪套统计规则）与表探查（列的类型徽标），而两者都在本文件下游；
// 放进 `model` 后依赖方向变得单一（算法层 → 领域词汇），不再反向。
//
// `typeof()` 会带参数或后缀（`DECIMAL(12,2)` / `VARCHAR(255)` / `TIMESTAMP WITH TIME ZONE` /
// `TIMESTAMP_NS`），所以先取基名再比较：精确比较曾把 **DECIMAL 列当文本列**统计。

/// 取类型基名（去参数与后缀）
pub fn type_base(dt_lower: &str) -> &str {
    dt_lower.split(['(', ' ']).next().unwrap_or(dt_lower).trim()
}

/// 数值族（含无符号整型：自 Parquet / 外部源读入的列可能是 `UBIGINT` 等）
pub fn is_numeric_type(dt_lower: &str) -> bool {
    matches!(
        type_base(dt_lower),
        "bigint"
            | "integer"
            | "int"
            | "smallint"
            | "tinyint"
            | "double"
            | "float"
            | "hugeint"
            | "decimal"
            | "numeric"
            | "real"
            | "utinyint"
            | "usmallint"
            | "uinteger"
            | "ubigint"
    )
}

/// 时间族。按**前缀**判而不是基名：`timestamp_ns` / `timestamp_us` 的基名仍是它自己，
/// 但它们都是时间戳。
pub fn is_datetime_type(dt_lower: &str) -> bool {
    let base = type_base(dt_lower);
    base == "date"
        || base == "datetime"
        || base.starts_with("timestamp")
        || base.starts_with("time")
}

/// 二进制族（不参与列统计）
pub fn is_binary_type(dt_lower: &str) -> bool {
    matches!(
        type_base(dt_lower),
        "blob" | "bytea" | "binary" | "varbinary"
    )
}

/// 数组族：DuckDB 的 `INTEGER[]`，以及外部源的 `ARRAY` / `LIST` 写法
pub fn is_array_type(dt_lower: &str) -> bool {
    dt_lower.starts_with('[')
        || dt_lower.ends_with(']')
        || dt_lower.contains("list")
        || dt_lower.contains("array")
}

/// 值的强调语义（视图映射到主题角色，模型不持色值）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Normal,
    Muted,
    Warning,
}

/// 基础统计的一行
#[derive(Debug, Clone, PartialEq)]
pub struct StatRow {
    pub label: &'static str,
    pub value: String,
    pub emphasis: Emphasis,
}

/// 数据分布的一条
#[derive(Debug, Clone, PartialEq)]
pub struct DistributionBar {
    pub label: String,
    /// 0.0–1.0（条长比例）
    pub ratio: f64,
    /// 条右侧文案（占比）
    pub text: String,
}

/// 质量提示的语气
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteLevel {
    Info,
    Warning,
}

/// 数据质量区的一条提示
#[derive(Debug, Clone, PartialEq)]
pub struct QualityNote {
    pub level: NoteLevel,
    pub text: String,
}

/// 样本数据的一行（`None` = NULL；NULL 要显式显示，不能留空）
#[derive(Debug, Clone, PartialEq)]
pub struct SampleCell {
    pub index: usize,
    pub value: Option<String>,
}