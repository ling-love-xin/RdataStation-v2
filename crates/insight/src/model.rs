//! 洞察模块的**领域模型与视图模型**。
//!
//! | 归属 | 内容 |
//! | --- | --- |
//! | [`types`] | 领域类型：列画像、表画像、质量（分析链路的输入输出契约，快照持久化也用） |
//! | 本文件 | 视图模型：面板 Tab / 目标 / 四态 / 列画像视图（只服务界面） |
//!
//! 分开放的理由：两者变更节奏不同——改界面不该看起来像改契约。
//!
//! 视图模型**不持色值**：只给语义（[`Emphasis`] / [`NoteLevel`] / [`ColumnKind`]），
//! 由视图映射到 `cx.theme().colors.*`。这样阈值与文案可以脱离窗口单测。

pub mod types;

use std::fmt::Write as _;

use types::{
    BooleanStats, ColumnInsightFull, ColumnStatsDetail, DateTimeStats, NumericStats, QualityScore,
    TableProfile, TableQuality, TextStats,
};

// 等级是 `quality_scorer` 的定义（阈值与文案的唯一来源），这里只借用类型
use crate::quality_scorer::Grade;
use crate::rule_types::{ExecutionResult, QualityReport};

// ==================== 阈值（原型 §3.1） ====================

/// 空值率超过此值：数值行转 warning 强调
pub const NULL_RATE_WARN: f64 = 0.05;
/// 空值率超过此值：追加「高空值率」质量提示
pub const NULL_RATE_NOTE: f64 = 0.10;
/// |偏度| 超过此值：认为分布明显偏斜
pub const SKEW_NOTABLE: f64 = 1.0;
/// 布尔列某一侧占比超过此值：提示取值不平衡
pub const BOOLEAN_IMBALANCE: f64 = 0.95;
/// Top 值类数达到此值：提示字段更像类别型
pub const TOP_VALUE_CATEGORY_HINT: usize = 5;
/// 数据质量区里极端值最多逐条列出几个（其余折叠为计数）
pub const EXTREME_LIST_LIMIT: usize = 2;

// ==================== 面板骨架 ====================

/// 面板的五个 Tab（顺序固定；标签一律 2 字，17.5rem 面板宽内五项不溢出）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    /// 列画像（Phase 1 落地）
    Column,
    /// 表探查（Phase 3）
    Table,
    /// 多列分析（Phase 3）
    MultiColumn,
    /// 结构洞察（Phase 4）
    Schema,
    /// 快照历史与版本对比（Phase 5）
    History,
}

impl PanelTab {
    /// 展示顺序
    pub const ALL: [PanelTab; 5] = [
        PanelTab::Column,
        PanelTab::Table,
        PanelTab::MultiColumn,
        PanelTab::Schema,
        PanelTab::History,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Column => "列",
            PanelTab::Table => "表",
            PanelTab::MultiColumn => "多列",
            PanelTab::Schema => "结构",
            PanelTab::History => "历史",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn from_index(ix: usize) -> Option<Self> {
        Self::ALL.get(ix).copied()
    }
}

/// 面板当前指向的分析目标。
///
/// 只装**定位信息**（临时表名 / 连接 ID / 列名），不装数据：洞察不自己取数（D20），
/// 取数由宿主在收到目标后发起。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsightTarget {
    /// 列画像：结果表（临时表）中的一列
    Column {
        temp_table: String,
        column: String,
        data_type: String,
    },
    /// 表探查：结果表整体
    Table {
        temp_table: String,
        table_name: String,
    },
    /// 多列分析
    MultiColumn {
        temp_table: String,
        columns: Vec<String>,
    },
    /// 结构洞察：连接 / schema 级
    Schema {
        conn_id: String,
        schema: Option<String>,
    },
}

impl InsightTarget {
    /// 该目标默认落到哪个 Tab（入口点击后直接切过去）
    pub fn default_tab(&self) -> PanelTab {
        match self {
            InsightTarget::Column { .. } => PanelTab::Column,
            InsightTarget::Table { .. } => PanelTab::Table,
            InsightTarget::MultiColumn { .. } => PanelTab::MultiColumn,
            InsightTarget::Schema { .. } => PanelTab::Schema,
        }
    }

    /// 目标头主标题
    pub fn title(&self) -> String {
        match self {
            InsightTarget::Column { column, .. } => column.clone(),
            InsightTarget::Table { table_name, .. } => table_name.clone(),
            InsightTarget::MultiColumn { columns, .. } => format!("{} 列", columns.len()),
            InsightTarget::Schema { schema, .. } => schema.clone().unwrap_or_else(|| "全部结构".into()),
        }
    }

    /// 目标头副标题（定位信息：列的声明类型 / 临时表名）
    pub fn detail(&self) -> Option<String> {
        match self {
            InsightTarget::Column {
                data_type,
                temp_table,
                ..
            } => Some(format!("{data_type} · {temp_table}")),
            InsightTarget::Table { temp_table, .. } => Some(temp_table.clone()),
            InsightTarget::MultiColumn { temp_table, .. } => Some(temp_table.clone()),
            InsightTarget::Schema { conn_id, .. } => Some(conn_id.clone()),
        }
    }
}

/// 面板内容状态（四态：空 / 加载 / 错误 / 数据）
#[derive(Debug, Clone, PartialEq)]
pub enum InsightPanelState {
    /// 无目标，或目标当前 Tab 尚无内容（后者由视图补期次提示）
    Empty,
    /// 计算中（视图渲染骨架）
    Loading,
    Error {
        message: String,
        /// 是否值得让用户重试（临时表失效 / 连接断开可重试；语法类错误不可）
        retryable: bool,
    },
    Data(PanelData),
}

/// 数据态里装的是什么：目标种类决定渲染哪一支。
///
/// 四种目标（列 / 表 / 多列 / 结构）各有自己的视图模型，因此 `Data` 必须是和类型——
/// 「数据态只装列画像」在 Phase 3 起就不够用了。
#[derive(Debug, Clone, PartialEq)]
pub enum PanelData {
    Column(ColumnProfileView),
    Table(TableProfileView),
}

impl PanelData {
    pub fn as_column(&self) -> Option<&ColumnProfileView> {
        match self {
            PanelData::Column(profile) => Some(profile),
            PanelData::Table(_) => None,
        }
    }

    pub fn as_table(&self) -> Option<&TableProfileView> {
        match self {
            PanelData::Table(profile) => Some(profile),
            PanelData::Column(_) => None,
        }
    }
}

impl InsightPanelState {
    /// 正在计算
    pub fn is_loading(&self) -> bool {
        matches!(self, InsightPanelState::Loading)
    }

    /// 出错（含不可重试）
    pub fn is_error(&self) -> bool {
        matches!(self, InsightPanelState::Error { .. })
    }

    /// 已出数
    pub fn is_data(&self) -> bool {
        matches!(self, InsightPanelState::Data(_))
    }

    /// 数据态里的列画像（不是列画像则为 `None`）
    pub fn column(&self) -> Option<&ColumnProfileView> {
        match self {
            InsightPanelState::Data(data) => data.as_column(),
            _ => None,
        }
    }

    /// 数据态里的表探查（不是表探查则为 `None`）
    pub fn table(&self) -> Option<&TableProfileView> {
        match self {
            InsightPanelState::Data(data) => data.as_table(),
            _ => None,
        }
    }
}

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
    base == "date" || base == "datetime" || base.starts_with("timestamp") || base.starts_with("time")
}

/// 二进制族（不参与列统计）
pub fn is_binary_type(dt_lower: &str) -> bool {
    matches!(type_base(dt_lower), "blob" | "bytea" | "binary" | "varbinary")
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

/// 质量评分卡（Phase 2）：总分 + 等级 + 四维。
///
/// 等级是类型（[`Grade`]）而不是字符串：视图按它取主题色，
/// 而阈值与文案由 `quality_scorer` 一处提供，不在这里再写一份。
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreView {
    pub overall: f64,
    pub grade: Grade,
    pub summary: String,
    pub dimensions: Vec<DimensionView>,
}

/// 评分卡的一维（完整性 / 唯一性 / 类型一致 / 分布均匀）
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionView {
    pub name: String,
    /// 0–100
    pub score: f64,
    /// 权重（四维合计 1.0）
    pub weight: f64,
    pub detail: String,
}

// ==================== 表探查视图模型（Phase 3.1） ====================

/// 表探查（「表」Tab）：列元数据表 + 行数 + 表级质量（评估后才有）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableProfileView {
    /// 逻辑表名（目标里的 `table_name`，用于展示与快照归属）
    pub table_name: String,
    /// 数据源标签（当前恒为「DuckDB 临时表」：面板只认临时表，见 D20）
    pub source_label: &'static str,
    pub row_count: u64,
    pub columns: Vec<TableColumnView>,
    /// 表级质量：`None` = 还没「评估全表」（不是 0 分）
    pub quality: Option<TableQualityView>,
    /// 评估进度：进行中才有
    pub progress: Option<TableEvalProgress>,
}

/// 表探查里的一列
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumnView {
    /// 序号（从 1 起，与元数据里的 `ordinal_position` 一致）
    pub index: i32,
    pub name: String,
    pub data_type: String,
    pub kind: ColumnKind,
    pub nullable: bool,
    pub primary_key: bool,
    /// 该列的质量分：未评估为 `None`（不显示假分值）
    pub score: Option<f64>,
}

impl TableColumnView {
    pub fn grade(&self) -> Option<Grade> {
        self.score.map(Grade::of)
    }
}

/// 表级质量摘要（由各列分数聚合而来）
#[derive(Debug, Clone, PartialEq)]
pub struct TableQualityView {
    pub overall: f64,
    pub grade: Grade,
    pub summary: String,
    /// 评分低于 [`TABLE_PROBLEM_SCORE`] 的列数
    pub problem_columns: usize,
    pub scored_columns: usize,
}

/// 「评估全表」的进度（串行评分，避免撞后端并发上限）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableEvalProgress {
    pub done: usize,
    pub total: usize,
}

impl TableEvalProgress {
    pub fn ratio(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
    }
}

/// 表级「需要关注」的分数阀：与评分卡四档同源（低于「一般」即算问题列）
pub const TABLE_PROBLEM_SCORE: f64 = 50.0;

impl TableProfileView {
    /// 从领域结果构建（纯函数）。
    ///
    /// 类型族由类型名推导：表探查只看元数据，不跑列级统计——
    /// 那些统计要逐列过规则，正是「评估全表」要干的事。
    pub fn from_profile(profile: &TableProfile, table_name: &str) -> Self {
        let columns = profile
            .columns
            .iter()
            .map(|column| TableColumnView {
                index: column.ordinal_position,
                name: column.column_name.clone(),
                data_type: column.data_type.clone(),
                kind: ColumnKind::of_type_name(&column.data_type),
                nullable: column.is_nullable,
                primary_key: column.is_primary_key,
                score: None,
            })
            .collect();
        Self {
            table_name: table_name.to_string(),
            source_label: "DuckDB 临时表",
            row_count: profile.row_count.unwrap_or(0).max(0) as u64,
            columns,
            quality: None,
            progress: None,
        }
    }

    /// 评估进行中的中间态（不丢已有分数：用户看着分数一列列长出来）
    pub fn evaluating(&self, done: usize, total: usize) -> Self {
        let mut next = self.clone();
        next.progress = Some(TableEvalProgress { done, total });
        next
    }

    /// 写入一列的分数
    pub fn with_column_score(&self, column: &str, score: f64) -> Self {
        let mut next = self.clone();
        if let Some(row) = next.columns.iter_mut().find(|row| row.name == column) {
            row.score = Some(score);
        }
        next
    }

    /// 评估完成：写表级摘要。
    ///
    /// 列分数不在这里重算——它与 `compute_table_quality` 内部的打分同源
    /// （都是 `compute_column_quality`），两处各算一次迟早在阈值上分岔。
    pub fn evaluated(&self, quality: &TableQuality) -> Self {
        let mut next = self.clone();
        next.progress = None;
        next.quality = Some(TableQualityView {
            overall: quality.overall_score,
            grade: Grade::of(quality.overall_score),
            summary: quality.summary.clone(),
            problem_columns: quality
                .column_scores
                .iter()
                .filter(|entry| entry.quality_score < TABLE_PROBLEM_SCORE)
                .count(),
            scored_columns: quality.scored_count as usize,
        });
        next
    }
}

// ==================== 多列分析视图模型（Phase 3.2 / 3.3） ====================

/// 「多列」Tab 的一条候选规则。
///
/// 列清单与规则清单都是**数据**（从服务层来），选中的列/规则是**面板状态**
/// （住 `InsightView`），所以这里只放展示与判定需要的东西。
#[derive(Debug, Clone, PartialEq)]
pub struct MultiRuleView {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 期望的列类型族（`applies_to`），长度 = 需要的列数
    pub applies_to: Vec<String>,
    /// 顺序参数（除 `table` 外）：`col1` / `col2` …
    pub column_params: Vec<String>,
    /// `single`（默认）/ `list`
    pub result_type: Option<String>,
}

impl MultiRuleView {
    /// 从 `list_insight_rules` 的 JSON 转视图模型（缺关键字段则视为不可用）。
    ///
    /// 服务层返回的是 JSON（v1 起就如此，供前端直接吃），转换放在这里：
    /// 类型化后上层就不必到处 `["name"].as_str().unwrap_or(...)`。
    pub fn from_rule_json(value: &serde_json::Value) -> Option<Self> {
        let id = value.get("id")?.as_str()?.to_string();
        let column_params: Vec<String> = value
            .get("parameters")
            .and_then(|p| p.as_array())
            .map(|params| {
                params
                    .iter()
                    .filter_map(|p| p.as_str())
                    // `table` 由面板自己填（恒为当前临时表），不占用户的列位
                    .filter(|p| *p != "table")
                    .map(|p| p.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            id,
            name: value.get("name")?.as_str()?.to_string(),
            description: value
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or_default()
                .to_string(),
            applies_to: value
                .get("applies_to")
                .and_then(|a| a.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|t| t.as_str())
                        .map(|t| t.to_string())
                        .collect()
                })
                .unwrap_or_default(),
            column_params,
            result_type: value
                .get("result_type")
                .and_then(|r| r.as_str())
                .map(|r| r.to_string()),
        })
    }

    /// 该规则需要几列
    pub fn arity(&self) -> usize {
        self.column_params.len()
    }

    /// 选中的列是否满足规则的类型要求。
    ///
    /// `Any` 放行；`applies_to` 比参数少时不补全（按位比较，缺位视为不限制）。
    /// 不做「类型不符就报错」——SQL 自己会拒，界面上先把不可用的选法标出来就够了。
    pub fn accepts(&self, kinds: &[ColumnKind]) -> bool {
        if kinds.len() != self.arity() {
            return false;
        }
        kinds.iter().enumerate().all(|(ix, kind)| match self.applies_to.get(ix) {
            None => true,
            Some(expected) => {
                expected.eq_ignore_ascii_case("any")
                    || expected.eq_ignore_ascii_case(kind.family_name())
            }
        })
    }

    /// 列表里那一行右侧的类型提示（如 `数值 · 数值`）
    pub fn types_hint(&self) -> String {
        if self.applies_to.is_empty() {
            return "通用".to_string();
        }
        self.applies_to.join(" · ")
    }
}

/// 多列规则的结果（两种形态：单值键值行 / 列表表格）
#[derive(Debug, Clone, PartialEq)]
pub enum MultiResultView {
    /// 一行一个字段（如相关系数 / 协方差 / 样本量）
    Single(Vec<KeyValueRow>),
    /// 表格（如交叉频次表）
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

/// 键值行（字段名来自规则输出，是**动态字符串**，所以不复用 `StatRow`——
/// 后者的 `label` 是 `&'static str`，专给固定词汇的统计行）
#[derive(Debug, Clone, PartialEq)]
pub struct KeyValueRow {
    pub label: String,
    pub value: String,
}

impl MultiResultView {
    /// 从领域执行结果转视图模型（按**数据形态**分派，不看 `result_type`）。
    ///
    /// 为什么按形态：`result_type` 是规则的声明，而真实返回才是事实——
    /// 两者不一致时按事实渲染，不会出现「声明 list 却只拿到一个数」时的空白表。
    pub fn from_execution(result: &ExecutionResult) -> Self {
        match &result.data {
            serde_json::Value::Array(items) => Self::table_from_items(items),
            serde_json::Value::Object(map) => Self::Single(
                map.iter()
                    .map(|(key, value)| KeyValueRow {
                        label: key.clone(),
                        value: json_cell(value),
                    })
                    .collect(),
            ),
            other => Self::Single(vec![KeyValueRow {
                label: "结果".to_string(),
                value: json_cell(other),
            }]),
        }
    }

    /// 列表结果 → 表格：表头按**首次出现顺序**收集（各行键不一致时不丢列）
    fn table_from_items(items: &[serde_json::Value]) -> Self {
        let mut headers: Vec<String> = Vec::new();
        for item in items {
            if let Some(map) = item.as_object() {
                for key in map.keys() {
                    if !headers.contains(key) {
                        headers.push(key.clone());
                    }
                }
            }
        }
        if headers.is_empty() {
            // 空结果：给空表格（视图据此说「没有数据」），而不是假造一行
            return Self::Table {
                headers: Vec::new(),
                rows: Vec::new(),
            };
        }
        let rows = items
            .iter()
            .map(|item| {
                headers
                    .iter()
                    .map(|h| {
                        item.get(h)
                            .map(json_cell)
                            .unwrap_or_else(|| "—".to_string())
                    })
                    .collect()
            })
            .collect();
        Self::Table { headers, rows }
    }

    /// 空结果（没跑过 / 规则未返回行）
    pub fn is_empty(&self) -> bool {
        match self {
            MultiResultView::Single(rows) => rows.is_empty(),
            MultiResultView::Table { rows, .. } => rows.is_empty(),
        }
    }
}

/// JSON 单元格 → 展示文案（数值仍走 `fmt_num`：`1.5` 不写成 `1.5000`）
pub fn json_cell(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "—".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(fmt_num)
            .unwrap_or_else(|| n.to_string()),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 质量门控 → 提示行（这一波多列规则自带 `quality` 时才有）
///
/// 通过的检查不占行：提示区只用来说「哪里不对」，全通过时是空的。
pub fn quality_notes(report: Option<&QualityReport>) -> Vec<QualityNote> {
    let Some(report) = report else {
        return Vec::new();
    };
    report
        .checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| QualityNote {
            level: NoteLevel::Warning,
            text: match &check.message {
                message if !message.trim().is_empty() => message.clone(),
                _ => match check.actual {
                    Some(actual) => format!("{} 未过 {}（实际 {}）", check.field, check.rule, fmt_num(actual)),
                    None => format!("{} 未过 {}", check.field, check.rule),
                },
            },
        })
        .collect()
}

/// 「多列」Tab 的数据：列清单 + 候选规则 + 最近一次结果
#[derive(Debug, Clone, PartialEq)]
pub struct MultiColumnView {
    /// 逻辑表名（结果集名），用于展示
    pub table_name: String,
    /// 临时表的**真实**列元数据（v1 的 `availableColumns` 恒空是该项从未跑通的根因）
    pub columns: Vec<TableColumnView>,
    /// `category = multi` 的规则
    pub rules: Vec<MultiRuleView>,
    /// 最近一次执行结果（没跑过为 `None`）
    pub result: Option<MultiResultView>,
    /// 最近一次执行的质量门控提示
    pub notes: Vec<QualityNote>,
}

impl MultiColumnView {
    pub fn from_profile(profile: &TableProfile, table_name: &str, rules: Vec<MultiRuleView>) -> Self {
        let table = TableProfileView::from_profile(profile, table_name);
        Self {
            table_name: table.table_name,
            columns: table.columns,
            rules,
            result: None,
            notes: Vec::new(),
        }
    }

    pub fn column(&self, name: &str) -> Option<&TableColumnView> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// 选中的列（按给定顺序）对应的类型族：供 [`MultiRuleView::accepts`] 判定
    pub fn kinds_of(&self, selected: &[String]) -> Vec<ColumnKind> {
        selected
            .iter()
            .filter_map(|name| self.column(name).map(|c| c.kind))
            .collect()
    }

    /// 写下一次执行结果（保留列清单与规则清单）
    pub fn with_result(&self, result: MultiResultView, notes: Vec<QualityNote>) -> Self {
        let mut next = self.clone();
        next.result = Some(result);
        next.notes = notes;
        next
    }
}

/// 列画像（「列」Tab 的四区内容 + 目标头所需字段）
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnProfileView {
    pub column: String,
    pub data_type: String,
    pub kind: ColumnKind,
    pub total_count: u32,
    pub null_count: u32,
    pub null_rate: f64,
    /// 四区之一：基础统计
    pub basics: Vec<StatRow>,
    /// 四区之二：数据分布（空 = 不渲染条，视图按类型给提示）
    pub distribution: Vec<DistributionBar>,
    /// 四区之三：数据质量
    pub notes: Vec<QualityNote>,
    /// 四区之四：样本数据
    pub sample: Vec<SampleCell>,
    /// 评分卡：`None` 表示**无数据不产假分数**（列全空时）
    pub score: Option<ScoreView>,
}

impl ColumnProfileView {
    /// 从领域结果构建视图模型。
    ///
    /// 纯函数：不读 I/O、不碰主题、不依赖窗口——阈值与文案因此可单测。
    pub fn from_domain(full: &ColumnInsightFull) -> Self {
        let stats = &full.stats;
        let kind = ColumnKind::of(&stats.stats_detail);
        let mut view = Self {
            column: stats.column_name.clone(),
            data_type: stats.data_type.clone(),
            kind,
            total_count: stats.total_count,
            null_count: stats.null_count,
            null_rate: stats.null_rate,
            basics: Vec::new(),
            distribution: Vec::new(),
            notes: Vec::new(),
            sample: sample_cells(&full.sample),
            score: None,
        };

        view.basics.push(StatRow {
            label: "总行数",
            value: fmt_int(stats.total_count),
            emphasis: Emphasis::Normal,
        });
        view.basics.push(StatRow {
            label: "非空值",
            value: fmt_int(stats.total_count.saturating_sub(stats.null_count)),
            emphasis: Emphasis::Normal,
        });
        view.basics.push(StatRow {
            label: "空值",
            value: format!("{}（{}）", fmt_int(stats.null_count), fmt_pct(stats.null_rate)),
            emphasis: if stats.null_rate > NULL_RATE_WARN {
                Emphasis::Warning
            } else {
                Emphasis::Normal
            },
        });
        view.basics.push(StatRow {
            label: "唯一值",
            value: stats
                .unique_count
                .map(fmt_int)
                .unwrap_or_else(|| "—".to_string()),
            emphasis: Emphasis::Normal,
        });

        match &stats.stats_detail {
            ColumnStatsDetail::Numeric(n) => fill_numeric(&mut view, n, full.histogram.as_deref()),
            ColumnStatsDetail::Text(t) => fill_text(&mut view, t),
            ColumnStatsDetail::DateTime(d) => fill_datetime(&mut view, d),
            ColumnStatsDetail::Boolean(b) => fill_boolean(&mut view, b),
            ColumnStatsDetail::Unknown => {
                view.notes.push(QualityNote {
                    level: NoteLevel::Info,
                    text: "类型未识别：仅给出计数，分布与类型统计不可用".into(),
                });
            }
        }

        if stats.null_rate > NULL_RATE_NOTE {
            view.notes.insert(
                0,
                QualityNote {
                    level: NoteLevel::Warning,
                    text: format!(
                        "高空值率：{} 的值为空，结论需谨慎",
                        fmt_pct(stats.null_rate)
                    ),
                },
            );
        }

        // 评分卡（Phase 2）：列全空时不给分数（「表为空或无数据不产假分数」的口径）
        view.score = (stats.total_count > 0)
            .then(|| score_view(&crate::quality_scorer::compute_column_quality(full)));

        view
    }
}

/// 评分卡的领域结果 → 视图模型（等级按分数现算，不去解析 `level` 字符串）
fn score_view(score: &QualityScore) -> ScoreView {
    ScoreView {
        overall: score.overall_score,
        grade: Grade::of(score.overall_score),
        summary: score.summary.clone(),
        dimensions: score
            .dimensions
            .iter()
            .map(|d| DimensionView {
                name: d.name.clone(),
                score: d.score,
                weight: d.weight,
                detail: d.detail.clone(),
            })
            .collect(),
    }
}

// ==================== 各类型族的填充 ====================

fn fill_numeric(
    view: &mut ColumnProfileView,
    n: &NumericStats,
    histogram: Option<&[types::DistributionBin]>,
) {
    for (label, value) in [
        ("平均", n.avg),
        ("中位", n.median),
        ("最小", n.min),
        ("最大", n.max),
        ("P25", n.p25),
        ("P75", n.p75),
    ] {
        view.basics.push(StatRow {
            label,
            value: fmt_num(value),
            emphasis: Emphasis::Normal,
        });
    }
    if let Some(stddev) = n.stddev {
        view.basics.push(StatRow {
            label: "标准差",
            value: fmt_num(stddev),
            emphasis: Emphasis::Normal,
        });
    }
    if let Some(skew) = n.skewness {
        view.basics.push(StatRow {
            label: "偏度",
            value: format!("{}（{}）", fmt_num(skew), skew_direction(skew)),
            emphasis: Emphasis::Normal,
        });
        if skew.abs() > SKEW_NOTABLE {
            view.notes.push(QualityNote {
                level: NoteLevel::Info,
                text: format!("分布{}（偏度 {}）", skew_direction(skew), fmt_num(skew)),
            });
        }
    }
    if !n.is_extreme.is_empty() {
        let listed: Vec<String> = n
            .is_extreme
            .iter()
            .take(EXTREME_LIST_LIMIT)
            .map(|e| format!("{} {}", e.kind, fmt_num(e.value)))
            .collect();
        let rest = n.is_extreme.len().saturating_sub(EXTREME_LIST_LIMIT);
        let tail = if rest > 0 {
            format!("，另有 {rest} 个")
        } else {
            String::new()
        };
        view.notes.push(QualityNote {
            level: NoteLevel::Warning,
            text: format!("检测到极端值：{}{tail}", listed.join("、")),
        });
    }

    // 分布：直方图由 `ColumnInsightFull::histogram` 携带（引擎在样本行数低于下限时不产），
    // 空结果不做补救——不造假分布。
    if let Some(bins) = histogram {
        view.distribution = bins
            .iter()
            .map(|b| DistributionBar {
                label: b.label.clone(),
                ratio: b.ratio,
                text: fmt_pct(b.ratio),
            })
            .collect();
    }
}

fn fill_text(view: &mut ColumnProfileView, t: &TextStats) {
    view.basics.push(StatRow {
        label: "长度范围",
        value: format!("{} ~ {}", t.min_length, t.max_length),
        emphasis: Emphasis::Normal,
    });
    view.distribution = t
        .top_values
        .iter()
        .map(|f| DistributionBar {
            label: f.value.clone(),
            ratio: f.ratio,
            text: format!("{} · {}", fmt_int(f.count), fmt_pct(f.ratio)),
        })
        .collect();
    if t.top_values.len() >= TOP_VALUE_CATEGORY_HINT {
        view.notes.push(QualityNote {
            level: NoteLevel::Info,
            text: format!(
                "高频取值 ≥ {} 个：字段更像类别型，建议按类别分组统计",
                t.top_values.len()
            ),
        });
    }
}

fn fill_datetime(view: &mut ColumnProfileView, d: &DateTimeStats) {
    view.basics.push(StatRow {
        label: "最早",
        value: d.earliest.clone(),
        emphasis: Emphasis::Normal,
    });
    view.basics.push(StatRow {
        label: "最晚",
        value: d.latest.clone(),
        emphasis: Emphasis::Normal,
    });
    view.basics.push(StatRow {
        label: "跨度",
        value: format!("{} 天", d.span_days),
        emphasis: Emphasis::Normal,
    });
    view.distribution = d
        .monthly_distribution
        .iter()
        .map(|f| DistributionBar {
            label: f.value.clone(),
            ratio: f.ratio,
            text: fmt_pct(f.ratio),
        })
        .collect();
}

fn fill_boolean(view: &mut ColumnProfileView, b: &BooleanStats) {
    view.basics.push(StatRow {
        label: "True",
        value: format!("{}（{}）", fmt_int(b.true_count), fmt_pct(b.true_ratio)),
        emphasis: Emphasis::Normal,
    });
    view.basics.push(StatRow {
        label: "False",
        value: fmt_int(b.false_count),
        emphasis: Emphasis::Normal,
    });
    view.distribution = vec![DistributionBar {
        label: "True 占比".into(),
        ratio: b.true_ratio,
        text: fmt_pct(b.true_ratio),
    }];
    let imbalance = b.true_ratio > BOOLEAN_IMBALANCE || b.true_ratio < 1.0 - BOOLEAN_IMBALANCE;
    if imbalance {
        view.notes.push(QualityNote {
            level: NoteLevel::Warning,
            text: format!("取值高度不平衡（True 占比 {}）", fmt_pct(b.true_ratio)),
        });
    }
}

// ==================== 小工具 ====================

/// 千分位整数（画像里的行数动辄百万，裸数字难读）
pub fn fmt_int(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 行数文案：万 / 亿 缩写（`124000` → `12.4 万`）
///
/// 只在**展示**上缩写：内部与快照里仍是精确值（`u64`），不给用户看“约”字。
pub fn fmt_rows(n: u64) -> String {
    const WAN: u64 = 10_000;
    const YI: u64 = 100_000_000;
    if n < WAN {
        fmt_int(n as u32)
    } else if n < YI {
        format!("{:.1} 万", n as f64 / WAN as f64)
    } else {
        format!("{:.1} 亿", n as f64 / YI as f64)
    }
}

/// 数值文案：整数不带小数点，小数最多 4 位且去尾零
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// 占比文案（0.1234 → 12.3%）
pub fn fmt_pct(ratio: f64) -> String {
    let mut s = String::new();
    let _ = write!(s, "{:.1}%", ratio * 100.0);
    s
}

/// 偏度方向（|偏度| ≤ 阈值时称近似对称）
pub fn skew_direction(skew: f64) -> &'static str {
    if skew > SKEW_NOTABLE {
        "右偏"
    } else if skew < -SKEW_NOTABLE {
        "左偏"
    } else {
        "近似对称"
    }
}

/// 样本值 → 单元格（`None` = NULL；对象 / 数组退化为紧凑 JSON）
fn sample_cells(sample: &[serde_json::Value]) -> Vec<SampleCell> {
    sample
        .iter()
        .enumerate()
        .map(|(i, v)| SampleCell {
            index: i + 1,
            value: match v {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            },
        })
        .collect()
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::types::{ColumnQualityEntry, DistributionBin, ExtremeValue, TableColumnMeta, TextFrequency};
    use super::*;
    use crate::rule_types::{ExecutionResult, QualityCheck, QualityReport};

    fn base_stats(detail: ColumnStatsDetail) -> ColumnInsightFull {
        ColumnInsightFull {
            stats: types::ColumnStats {
                column_name: "amount".into(),
                data_type: "DECIMAL(12,2)".into(),
                total_count: 1000,
                null_count: 20,
                null_rate: 0.02,
                unique_count: Some(480),
                stats_detail: detail,
            },
            sample: vec![serde_json::json!(1), serde_json::Value::Null],
            histogram: None,
        }
    }

    fn numeric(skewness: Option<f64>, extremes: Vec<ExtremeValue>) -> NumericStats {
        NumericStats {
            min: 0.5,
            max: 9999.0,
            avg: 123.4567,
            median: 100.0,
            p25: 25.0,
            p75: 200.0,
            sum: 123456.0,
            stddev: Some(12.5),
            skewness,
            kurtosis: None,
            is_extreme: extremes,
        }
    }

    #[test]
    fn tab_labels_are_short_and_ordered() {
        let labels: Vec<&str> = PanelTab::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(labels, vec!["列", "表", "多列", "结构", "历史"]);
        for (i, tab) in PanelTab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), i, "index() 必须与 ALL 顺序一致");
            assert_eq!(PanelTab::from_index(i), Some(*tab));
            assert!(
                tab.label().chars().count() <= 2,
                "标签一律 2 字，否则 17.5rem 内五项溢出"
            );
        }
        assert_eq!(PanelTab::from_index(9), None);
    }

    #[test]
    fn target_default_tab_matches_kind() {
        let col = InsightTarget::Column {
            temp_table: "t1".into(),
            column: "amount".into(),
            data_type: "DECIMAL".into(),
        };
        assert_eq!(col.default_tab(), PanelTab::Column);
        assert_eq!(col.title(), "amount");
        assert_eq!(col.detail().as_deref(), Some("DECIMAL · t1"));

        let table = InsightTarget::Table {
            temp_table: "t1".into(),
            table_name: "orders".into(),
        };
        assert_eq!(table.default_tab(), PanelTab::Table);
        assert_eq!(table.title(), "orders");

        let multi = InsightTarget::MultiColumn {
            temp_table: "t1".into(),
            columns: vec!["a".into(), "b".into()],
        };
        assert_eq!(multi.default_tab(), PanelTab::MultiColumn);
        assert_eq!(multi.title(), "2 列");

        let schema = InsightTarget::Schema {
            conn_id: "G_1".into(),
            schema: None,
        };
        assert_eq!(schema.default_tab(), PanelTab::Schema);
        assert_eq!(schema.title(), "全部结构");
    }

    #[test]
    fn numeric_column_fills_basic_stats() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(
            numeric(Some(2.5), Vec::new()),
        )));
        assert_eq!(view.kind, ColumnKind::Numeric);
        let labels: Vec<&str> = view.basics.iter().map(|r| r.label).collect();
        assert_eq!(
            labels,
            vec![
                "总行数", "非空值", "空值", "唯一值", "平均", "中位", "最小", "最大", "P25", "P75",
                "标准差", "偏度"
            ]
        );
        assert_eq!(view.basics[0].value, "1,000");
        assert_eq!(view.basics[1].value, "980");
        assert_eq!(view.basics[2].value, "20（2.0%）");
        assert_eq!(view.basics[3].value, "480");
        assert_eq!(view.basics[4].value, "123.4567");
        assert_eq!(view.basics[5].value, "100");
        assert_eq!(view.basics[9].value, "200");
    }

    #[test]
    fn low_null_rate_keeps_normal_emphasis() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(
            numeric(None, Vec::new()),
        )));
        assert_eq!(view.basics[2].emphasis, Emphasis::Normal);
        assert!(
            view.notes.iter().all(|n| !n.text.contains("高空值率")),
            "2% 空值率不该触发高空值率提示"
        );
    }

    #[test]
    fn high_null_rate_warns_and_notes() {
        let mut full = base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        full.stats.null_count = 320;
        full.stats.null_rate = 0.32;
        let view = ColumnProfileView::from_domain(&full);
        assert_eq!(view.basics[2].emphasis, Emphasis::Warning);
        // 高空值率提示置顶（比类型专属提示更该先看到）
        assert!(view.notes[0].text.contains("高空值率"));
        assert_eq!(view.notes[0].level, NoteLevel::Warning);
    }

    #[test]
    fn skew_direction_text_and_note() {
        assert_eq!(skew_direction(2.0), "右偏");
        assert_eq!(skew_direction(-3.0), "左偏");
        assert_eq!(skew_direction(0.4), "近似对称");

        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(
            numeric(Some(-2.0), Vec::new()),
        )));
        let skew_row = view.basics.iter().find(|r| r.label == "偏度").unwrap();
        assert_eq!(skew_row.value, "-2（左偏）");
        assert!(view.notes.iter().any(|n| n.text.contains("分布左偏")));
    }

    #[test]
    fn extreme_values_are_listed_with_overflow_count() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(
            numeric(
                None,
                vec![
                    ExtremeValue {
                        value: 9999.0,
                        kind: "高".into(),
                    },
                    ExtremeValue {
                        value: 0.5,
                        kind: "低".into(),
                    },
                    ExtremeValue {
                        value: 8888.0,
                        kind: "高".into(),
                    },
                ],
            ),
        )));
        let note = view
            .notes
            .iter()
            .find(|n| n.text.contains("极端值"))
            .expect("应有极端值提示");
        assert_eq!(note.level, NoteLevel::Warning);
        assert!(note.text.contains("高 9999"));
        assert!(note.text.contains("另有 1 个"), "超出上限应折叠为计数");
    }

    #[test]
    fn text_column_reports_length_range_and_top_values() {
        let detail = ColumnStatsDetail::Text(TextStats {
            min_length: 2,
            max_length: 18,
            top_values: (0..5)
                .map(|i| TextFrequency {
                    value: format!("v{i}"),
                    count: 10 + i,
                    ratio: 0.1,
                })
                .collect(),
        });
        let view = ColumnProfileView::from_domain(&base_stats(detail));
        assert_eq!(view.kind, ColumnKind::Text);
        assert!(view.basics.iter().any(|r| r.value == "2 ~ 18"));
        assert_eq!(view.distribution.len(), 5);
        assert_eq!(view.distribution[0].text, "10 · 10.0%");
        assert!(
            view.notes.iter().any(|n| n.text.contains("类别型")),
            "Top 值达到 5 类应提示类别型"
        );
    }

    #[test]
    fn datetime_column_reports_span_and_monthly_distribution() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::DateTime(
            DateTimeStats {
                earliest: "2024-01-01".into(),
                latest: "2024-03-31".into(),
                span_days: 90,
                monthly_distribution: vec![TextFrequency {
                    value: "2024-01".into(),
                    count: 40,
                    ratio: 0.4,
                }],
            },
        )));
        assert_eq!(view.kind, ColumnKind::DateTime);
        assert!(view.basics.iter().any(|r| r.label == "跨度" && r.value == "90 天"));
        assert_eq!(view.distribution.len(), 1);
    }

    #[test]
    fn boolean_imbalance_is_flagged_both_ways() {
        let balanced = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Boolean(
            BooleanStats {
                true_count: 500,
                false_count: 500,
                true_ratio: 0.5,
            },
        )));
        assert!(balanced.notes.iter().all(|n| !n.text.contains("不平衡")));
        assert_eq!(balanced.distribution[0].label, "True 占比");

        let lopsided = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Boolean(
            BooleanStats {
                true_count: 990,
                false_count: 10,
                true_ratio: 0.99,
            },
        )));
        assert!(lopsided.notes.iter().any(|n| n.text.contains("不平衡")));
    }

    #[test]
    fn unknown_kind_keeps_counts_only() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Unknown));
        assert_eq!(view.kind, ColumnKind::Unknown);
        assert_eq!(view.kind.label(), "未识别");
        assert_eq!(
            view.basics.len(),
            4,
            "未识别类型只给四行基础计数，不追加类型专属统计"
        );
        assert!(view.distribution.is_empty());
        assert!(view.notes.iter().any(|n| n.text.contains("类型未识别")));
    }

    #[test]
    fn sample_marks_null_explicitly() {
        let mut full = base_stats(ColumnStatsDetail::Unknown);
        full.sample = vec![serde_json::json!("abc"), serde_json::Value::Null];
        let view = ColumnProfileView::from_domain(&full);
        assert_eq!(view.sample.len(), 2);
        assert_eq!(view.sample[0].index, 1);
        assert_eq!(view.sample[0].value.as_deref(), Some("abc"));
        assert_eq!(view.sample[1].value, None, "NULL 要显式表达，不能当空串");
    }

    /// DuckDB 的 `typeof()` 会带参数或后缀，精确比较会把列判错族
    /// （曾把 `DECIMAL(12,2)` 当文本列做统计）
    #[test]
    fn parameterised_types_keep_their_family() {
        assert!(is_numeric_type("decimal(12,2)"));
        assert!(is_numeric_type("numeric(18,4)"));
        assert!(is_numeric_type("bigint"));
        // 无符号整型：自 Parquet / 外部源读入的列可能是这些
        assert!(is_numeric_type("ubigint"));
        assert!(is_numeric_type("utinyint"));

        assert!(is_datetime_type("timestamp with time zone"));
        assert!(is_datetime_type("timestamptz"));
        assert!(is_datetime_type("timestamp_ns"));
        assert!(is_datetime_type("time with time zone"));
        assert!(is_datetime_type("date"));

        assert!(is_binary_type("blob"));
        assert!(is_array_type("integer[]"));
        assert!(is_array_type("list"));
    }

    /// 文本族不得漏进其它族（否则会去做不可能的统计）
    #[test]
    fn text_like_types_stay_out_of_other_families() {
        assert!(!is_numeric_type("varchar"));
        assert!(!is_numeric_type("varchar(255)"));
        assert!(!is_datetime_type("varchar"));
        assert!(!is_binary_type("varchar"));
        assert!(!is_array_type("varchar"));
        // `timestamp` 开头但不是时间类型的反例不存在；保留一条边界：空串
        assert!(!is_numeric_type(""));
        assert!(!is_datetime_type(""));
    }

    /// 表探查只拿得到类型名：类型名 → 类型族要与列画像的路由同口径
    #[test]
    fn type_name_maps_to_the_same_families() {
        assert_eq!(ColumnKind::of_type_name("DECIMAL(12,2)"), ColumnKind::Numeric);
        assert_eq!(ColumnKind::of_type_name("BIGINT"), ColumnKind::Numeric);
        assert_eq!(ColumnKind::of_type_name("TIMESTAMP_NS"), ColumnKind::DateTime);
        assert_eq!(ColumnKind::of_type_name("BOOLEAN"), ColumnKind::Boolean);
        assert_eq!(ColumnKind::of_type_name("VARCHAR(255)"), ColumnKind::Text);
        assert_eq!(ColumnKind::of_type_name("BLOB"), ColumnKind::Unknown);
        assert_eq!(ColumnKind::of_type_name("INTEGER[]"), ColumnKind::Unknown);
        assert_eq!(ColumnKind::of_type_name(""), ColumnKind::Unknown);
    }

    // ==================== 表探查视图模型（Phase 3.1） ====================

    fn table_profile_fixture() -> TableProfile {
        TableProfile {
            table_name: "t_result_1".into(),
            db_type: "DuckDB".into(),
            columns: vec![
                TableColumnMeta {
                    column_name: "id".into(),
                    data_type: "BIGINT".into(),
                    is_nullable: false,
                    is_primary_key: true,
                    ordinal_position: 1,
                },
                TableColumnMeta {
                    column_name: "amount".into(),
                    data_type: "DECIMAL(12,2)".into(),
                    is_nullable: true,
                    is_primary_key: false,
                    ordinal_position: 2,
                },
                TableColumnMeta {
                    column_name: "payload".into(),
                    data_type: "BLOB".into(),
                    is_nullable: true,
                    is_primary_key: false,
                    ordinal_position: 3,
                },
            ],
            row_count: Some(124_000),
            schema_name: None,
        }
    }

    #[test]
    fn table_view_lists_columns_with_families_and_no_fake_scores() {
        let view = TableProfileView::from_profile(&table_profile_fixture(), "orders");
        assert_eq!(view.table_name, "orders");
        assert_eq!(view.row_count, 124_000);
        assert_eq!(view.columns.len(), 3);
        assert_eq!(view.columns[0].kind, ColumnKind::Numeric);
        assert!(view.columns[0].primary_key);
        assert!(!view.columns[0].nullable);
        assert_eq!(view.columns[2].kind, ColumnKind::Unknown, "BLOB 不参与列统计");
        assert!(
            view.columns.iter().all(|c| c.score.is_none()),
            "没评估就是没分，不给假分值"
        );
        assert!(view.quality.is_none());
        assert!(view.progress.is_none());
    }

    #[test]
    fn evaluation_progress_is_monotonic_and_keeps_scores() {
        let base = TableProfileView::from_profile(&table_profile_fixture(), "orders");
        let started = base.evaluating(0, 3);
        assert_eq!(started.progress, Some(TableEvalProgress { done: 0, total: 3 }));
        assert_eq!(started.progress.unwrap().ratio(), 0.0);

        let one = started.with_column_score("id", 95.0);
        let two = one
            .with_column_score("amount", 72.0)
            .evaluating(2, 3);
        assert_eq!(two.columns[0].score, Some(95.0), "先算出来的分不得被覆盖");
        assert_eq!(two.columns[1].score, Some(72.0));
        assert_eq!(two.columns[0].grade(), Some(Grade::Excellent));
        assert_eq!(two.columns[1].grade(), Some(Grade::Good));
        assert_eq!(two.progress.unwrap().ratio(), 2.0 / 3.0);
        assert!(two.columns[2].score.is_none());

        // 未知列名不造行也不 panic（列清单可能刚变）
        assert_eq!(two.with_column_score("不存在", 10.0).columns.len(), 3);
    }

    #[test]
    fn evaluated_writes_the_summary_and_counts_problem_columns() {
        let base = TableProfileView::from_profile(&table_profile_fixture(), "orders");
        let quality = TableQuality {
            table_name: "orders".into(),
            overall_score: 68.4,
            level: "一般".into(),
            column_scores: vec![
                ColumnQualityEntry {
                    column_name: "payload".into(),
                    quality_score: 30.0,
                    level: "较差".into(),
                    null_rate: 0.2,
                },
                ColumnQualityEntry {
                    column_name: "amount".into(),
                    quality_score: 72.0,
                    level: "良好".into(),
                    null_rate: 0.05,
                },
            ],
            summary: "表质量一般 (68分)，2 列已评估 (1风险列)".into(),
            scored_count: 2,
            total_columns: 2,
        };

        let done = base.evaluating(2, 2).evaluated(&quality);
        assert!(done.progress.is_none(), "评估完进度行必须消失");
        let view = done.quality.expect("应有表级摘要");
        assert_eq!(view.overall, 68.4);
        assert_eq!(view.grade, Grade::Fair);
        assert_eq!(view.problem_columns, 1, "低于 50 分才算问题列");
        assert_eq!(view.scored_columns, 2);
        assert!(view.summary.contains("一般"));
    }

    #[test]
    fn row_count_formatting_switches_units() {
        assert_eq!(fmt_rows(0), "0");
        assert_eq!(fmt_rows(9_999), "9,999");
        assert_eq!(fmt_rows(10_000), "1.0 万");
        assert_eq!(fmt_rows(124_000), "12.4 万");
        assert_eq!(fmt_rows(99_999_999), "10000.0 万");
        assert_eq!(fmt_rows(100_000_000), "1.0 亿");
    }

    #[test]
    fn empty_progress_ratio_is_zero_not_nan() {
        assert_eq!(TableEvalProgress { done: 0, total: 0 }.ratio(), 0.0);
        assert_eq!(TableEvalProgress { done: 5, total: 3 }.ratio(), 1.0);
    }

    // ==================== 多列分析视图模型（Phase 3.2 / 3.3） ====================

    fn multi_rule_json() -> serde_json::Value {
        serde_json::json!({
            "id": "pearson-correlation",
            "name": "Pearson 相关系数",
            "description": "计算两个数值列的相关系数",
            "category": "multi",
            "applies_to": ["Numeric", "Numeric"],
            "parameters": ["table", "col1", "col2"],
            "result_type": serde_json::Value::Null,
            "scope": "内置",
        })
    }

    #[test]
    fn multi_rule_parses_and_drops_the_table_parameter() {
        let rule = MultiRuleView::from_rule_json(&multi_rule_json()).expect("应能解析");
        assert_eq!(rule.id, "pearson-correlation");
        assert_eq!(rule.name, "Pearson 相关系数");
        assert_eq!(
            rule.column_params,
            vec!["col1".to_string(), "col2".to_string()],
            "`table` 由界面自己填，不占用户的列位"
        );
        assert_eq!(rule.arity(), 2);
        assert_eq!(rule.types_hint(), "Numeric · Numeric");
        assert!(rule.result_type.is_none());

        // 缺 id / name 的行不可用（宁少不假）
        assert!(MultiRuleView::from_rule_json(&serde_json::json!({"name": "X"})).is_none());
        assert!(MultiRuleView::from_rule_json(&serde_json::json!({"id": "x"})).is_none());
    }

    #[test]
    fn multi_rule_accepts_only_matching_families_and_arity() {
        let rule = MultiRuleView::from_rule_json(&multi_rule_json()).unwrap();
        assert!(rule.accepts(&[ColumnKind::Numeric, ColumnKind::Numeric]));
        assert!(!rule.accepts(&[ColumnKind::Numeric, ColumnKind::Text]));
        assert!(!rule.accepts(&[ColumnKind::Numeric]), "列数不对就不该可选");
        assert!(!rule.accepts(&[]));

        // `Any` 放行；`applies_to` 缺位不限制
        let any = MultiRuleView {
            applies_to: vec!["Any".into(), "Any".into()],
            ..rule.clone()
        };
        assert!(any.accepts(&[ColumnKind::Text, ColumnKind::Unknown]));
        let no_hint = MultiRuleView {
            applies_to: Vec::new(),
            ..rule.clone()
        };
        assert!(no_hint.accepts(&[ColumnKind::Boolean, ColumnKind::DateTime]));
        assert_eq!(no_hint.types_hint(), "通用");
    }

    fn execution(data: serde_json::Value) -> ExecutionResult {
        ExecutionResult {
            data,
            quality: None,
        }
    }

    #[test]
    fn single_result_becomes_key_value_rows() {
        let view = MultiResultView::from_execution(&execution(serde_json::json!({
            "correlation": 0.8765,
            "sample_size": 120,
            "note": serde_json::Value::Null,
        })));
        let MultiResultView::Single(rows) = view else {
            panic!("对象形态应转成键值行");
        };
        let map: Vec<(&str, &str)> = rows.iter().map(|r| (r.label.as_str(), r.value.as_str())).collect();
        assert!(map.contains(&("correlation", "0.8765")));
        assert!(map.contains(&("sample_size", "120")), "整数不带小数点：{map:?}");
        assert!(map.contains(&("note", "—")), "NULL 显式显示，不留空");
    }

    #[test]
    fn list_result_becomes_a_table_with_a_union_of_headers() {
        let view = MultiResultView::from_execution(&execution(serde_json::json!([
            {"row_label": "paid", "col_label": "cn", "count": 12},
            // 第二行少一个键：表头取并集，缺的格子留「—」而不是错位
            {"row_label": "paid", "count": 3},
        ])));
        let MultiResultView::Table { headers, rows } = view else {
            panic!("数组形态应转成表格");
        };
        assert_eq!(headers, vec!["row_label", "col_label", "count"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][1], "—", "缺键的格子补「—」，不得错位");
        assert_eq!(rows[1][2], "3");

        // 空数组：空表格（视图说「没有数据」），不造假行
        let empty = MultiResultView::from_execution(&execution(serde_json::json!([])));
        assert!(empty.is_empty());
        assert_eq!(
            empty,
            MultiResultView::Table {
                headers: Vec::new(),
                rows: Vec::new()
            }
        );
    }

    #[test]
    fn scalar_result_still_renders() {
        let view = MultiResultView::from_execution(&execution(serde_json::json!(42)));
        assert_eq!(
            view,
            MultiResultView::Single(vec![KeyValueRow {
                label: "结果".into(),
                value: "42".into(),
            }])
        );
    }

    #[test]
    fn quality_notes_keep_only_failures() {
        let passed = QualityReport {
            passed: true,
            checks: vec![QualityCheck {
                field: "correlation".into(),
                passed: true,
                rule: "min".into(),
                actual: Some(0.9),
                severity: "warning".into(),
                message: "不该出现".into(),
            }],
        };
        assert!(quality_notes(Some(&passed)).is_empty(), "通过的检查不占提示行");

        let failed = QualityReport {
            passed: false,
            checks: vec![
                QualityCheck {
                    field: "correlation".into(),
                    passed: false,
                    rule: "min=0.5".into(),
                    actual: Some(0.21),
                    severity: "warning".into(),
                    message: "相关系数偏低".into(),
                },
                QualityCheck {
                    field: "sample_size".into(),
                    passed: false,
                    rule: "min=30".into(),
                    actual: Some(12.0),
                    severity: "error".into(),
                    message: String::new(),
                },
            ],
        };
        let notes = quality_notes(Some(&failed));
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].text, "相关系数偏低", "有文案就用规则自己的文案");
        assert_eq!(notes[1].text, "sample_size 未过 min=30（实际 12）", "没文案就拼一条可定位的");
        assert!(notes.iter().all(|n| n.level == NoteLevel::Warning));
        assert!(quality_notes(None).is_empty(), "规则没有门控时没有提示");
    }

    #[test]
    fn multi_view_keeps_columns_and_marks_the_result_in_place() {
        let rules = vec![MultiRuleView::from_rule_json(&multi_rule_json()).unwrap()];
        let view = MultiColumnView::from_profile(&table_profile_fixture(), "orders", rules);
        assert_eq!(view.table_name, "orders");
        assert_eq!(view.columns.len(), 3);
        assert_eq!(
            view.kinds_of(&["id".into(), "amount".into()]),
            vec![ColumnKind::Numeric, ColumnKind::Numeric]
        );
        assert!(
            view.rules[0].accepts(&view.kinds_of(&["id".into(), "amount".into()])),
            "两列数值应满足 Pearson 的要求"
        );
        assert!(!view.rules[0].accepts(&view.kinds_of(&["id".into()])), "只选一列不行");

        let done = view.with_result(
            MultiResultView::Single(vec![KeyValueRow {
                label: "correlation".into(),
                value: "0.9".into(),
            }]),
            Vec::new(),
        );
        assert_eq!(view.result, None, "原视图不被就地修改");
        assert!(done.result.is_some());
        assert_eq!(done.columns, view.columns, "写结果不动列清单");
    }

    #[test]
    fn number_formatting_drops_trailing_zeros() {
        assert_eq!(fmt_num(100.0), "100");
        assert_eq!(fmt_num(0.5), "0.5");
        assert_eq!(fmt_num(123.456789), "123.4568");
        assert_eq!(fmt_num(f64::NAN), "—");
        assert_eq!(fmt_int(0), "0");
        assert_eq!(fmt_int(999), "999");
        assert_eq!(fmt_int(1_234_567), "1,234,567");
        assert_eq!(fmt_pct(0.1234), "12.3%");
    }

    #[test]
    fn numeric_distribution_comes_from_histogram() {
        // 直方图挂在 `ColumnInsightFull::histogram`（不在 `NumericStats` 里），
        // 因此构建器必须在分派时把它交给数值分支，否则数值列永远看不到分布。
        let mut full = base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        full.histogram = Some(vec![
            DistributionBin {
                label: "0–10".into(),
                count: 5,
                ratio: 0.25,
            },
            DistributionBin {
                label: "10–20".into(),
                count: 15,
                ratio: 0.75,
            },
        ]);
        let view = ColumnProfileView::from_domain(&full);
        assert_eq!(view.distribution.len(), 2);
        assert_eq!(view.distribution[0].label, "0–10");
        assert_eq!(view.distribution[0].text, "25.0%");
        assert_eq!(view.distribution[1].ratio, 0.75);
    }

    #[test]
    fn empty_histogram_yields_no_bars() {
        // 引擎在样本行数低于 `HISTOGRAM_MIN_ROWS` 时返回空直方图，视图不造假分布
        let mut full = base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        full.histogram = Some(Vec::new());
        let view = ColumnProfileView::from_domain(&full);
        assert!(view.distribution.is_empty());
    }

    #[test]
    fn score_card_is_built_for_non_empty_columns() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(
            numeric(None, Vec::new()),
        )));
        let score = view.score.expect("有数据就应产出评分卡");
        assert_eq!(score.dimensions.len(), 4, "评分卡固定四维");
        let weights: f64 = score.dimensions.iter().map(|d| d.weight).sum();
        assert!((weights - 1.0).abs() < 1e-9, "权重合计应为 1.0");
        for dim in &score.dimensions {
            assert!(
                (0.0..=100.0).contains(&dim.score),
                "维度得分应在 0–100：{}={}",
                dim.name,
                dim.score
            );
            assert!(!dim.detail.is_empty(), "维度应带明细文案：{}", dim.name);
        }
        assert!(
            (0.0..=100.0).contains(&score.overall),
            "总分应在 0–100：{}",
            score.overall
        );
        // 等级按分数现算，与领域结果里的 `level` 字符串同源
        assert_eq!(score.grade, Grade::of(score.overall));
        assert!(!score.summary.is_empty());
    }

    #[test]
    fn empty_column_has_no_score() {
        // 「表为空或无数据不产假分数」：全空列若给 0 分，用户会误以为数据质量差，
        // 实际是根本没有数据——视图据此不渲染评分卡。
        let mut full = base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        full.stats.total_count = 0;
        full.stats.null_count = 0;
        full.stats.null_rate = 0.0;
        full.stats.unique_count = None;
        full.sample.clear();
        let view = ColumnProfileView::from_domain(&full);
        assert!(view.score.is_none());
        // 计数与空值率仍然照给（基础统计不依赖有无数据）
        assert_eq!(view.total_count, 0);
        let labels: Vec<&str> = view.basics.iter().map(|r| r.label).collect();
        assert_eq!(&labels[..4], ["总行数", "非空值", "空值", "唯一值"]);
    }
}
