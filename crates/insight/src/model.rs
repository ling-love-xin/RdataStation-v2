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
    /// 结构目标（Schema 健康报告）。`database` 是必需的：分析器的表清单要按
    /// `table_catalog` 过滤（`information_schema` 里同名表可能属于不同库）。
    Schema {
        conn_id: String,
        database: String,
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

    /// 数据来源的临时表名（结构目标没有临时表 → 空串）。
    ///
    /// 取数请求只认它：三个目标种类都指向临时表，视图因此不必逐个 `match`。
    pub fn temp_table(&self) -> &str {
        match self {
            InsightTarget::Column { temp_table, .. }
            | InsightTarget::Table { temp_table, .. }
            | InsightTarget::MultiColumn { temp_table, .. } => temp_table,
            InsightTarget::Schema { .. } => "",
        }
    }

    /// 展示用的表名（列 / 多列目标没有独立表名，退回临时表名）
    pub fn table_name(&self) -> String {
        match self {
            InsightTarget::Table { table_name, .. } => table_name.clone(),
            _ => self.temp_table().to_string(),
        }
    }
}

/// 面板内容状态（四态：空 / 加载 / 错误 / 数据）
///
/// `Data` 不带载荷：**载荷在 [`PanelData`] 里按 Tab 分开存**。
/// 两者分开的理由：Tab 条是「同一个目标的多个视角」，切 Tab 不该把别的视角取到的
/// 数据丢掉（单格子设计下，切到「表」再切回「多列」就会丢表单与结果）。
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
    /// 至少有一份可取的内容（具体是哪个 Tab 的，看 [`PanelData`]）
    Data,
}

/// 数据态里装的是什么：按 Tab 分开存（各 Tab 的取数时机不同，互不覆盖）。
///
/// 四种目标（列 / 表 / 多列 / 结构）各有自己的视图模型；`None` = 这个 Tab 还没取过数。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PanelData {
    pub column: Option<ColumnProfileView>,
    pub table: Option<TableProfileView>,
    pub multi: Option<MultiColumnView>,
    pub schema: Option<crate::schema_view::SchemaReportView>,
    pub history: Option<HistoryView>,
}

impl PanelData {
    pub fn as_column(&self) -> Option<&ColumnProfileView> {
        self.column.as_ref()
    }

    pub fn as_table(&self) -> Option<&TableProfileView> {
        self.table.as_ref()
    }

    pub fn as_multi(&self) -> Option<&MultiColumnView> {
        self.multi.as_ref()
    }

    pub fn as_schema(&self) -> Option<&crate::schema_view::SchemaReportView> {
        self.schema.as_ref()
    }

    pub fn as_history(&self) -> Option<&HistoryView> {
        self.history.as_ref()
    }

    /// 三个 Tab 都还没取过数
    pub fn is_empty(&self) -> bool {
        self.column.is_none() && self.table.is_none() && self.multi.is_none() && self.schema.is_none()
    }

    // 写入用链式构造：新数据只换它自己那一格，别的 Tab 的载荷不动
    pub fn with_column(mut self, profile: ColumnProfileView) -> Self {
        self.column = Some(profile);
        self
    }

    pub fn with_table(mut self, profile: TableProfileView) -> Self {
        self.table = Some(profile);
        self
    }

    pub fn with_multi(mut self, view: MultiColumnView) -> Self {
        self.multi = Some(view);
        self
    }

    pub fn with_schema(mut self, view: crate::schema_view::SchemaReportView) -> Self {
        self.schema = Some(view);
        self
    }

    pub fn with_history(mut self, view: HistoryView) -> Self {
        self.history = Some(view);
        self
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
        matches!(self, InsightPanelState::Data)
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
                        label: field_label(key).to_string(),
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
        Self::Table {
            headers: headers.iter().map(|h| field_label(h).to_string()).collect(),
            rows,
        }
    }

    /// 空结果（没跑过 / 规则未返回行）
    pub fn is_empty(&self) -> bool {
        match self {
            MultiResultView::Single(rows) => rows.is_empty(),
            MultiResultView::Table { rows, .. } => rows.is_empty(),
        }
    }
}

/// 输出字段名 → 中文标签（内置规则用到的那些）。
///
/// 只映射**已知的**字段名：规则的 `json_name` 是对外契约，认不出来的就原样展示——
/// 猜一个中文名比露个英文名更坏（用户对着不确定的词更没法搜）。
/// 将来若把中文名写进规则 schema（`[[output]] label`），这里就该退休。
pub fn field_label(json_name: &str) -> &str {
    match json_name {
        "correlation" => "相关系数",
        "covariance" => "协方差",
        "regression_slope" => "回归斜率",
        "regression_intercept" => "回归截距",
        "sample_size" => "样本量",
        "row_value" => "行",
        "col_value" => "列",
        "count" => "计数",
        other => other,
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
    /// 临时表名（**数据来源**：面板据此判断这份数据是否属于当前目标）
    pub temp_table: String,
    /// 逻辑表名（展示用）
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
    /// 空视图（面板初始态：没取过数）
    pub fn empty() -> Self {
        Self {
            temp_table: String::new(),
            table_name: String::new(),
            columns: Vec::new(),
            rules: Vec::new(),
            result: None,
            notes: Vec::new(),
        }
    }

    /// 这份数据是不是当前这个临时表的（是就不必重新取数）
    pub fn is_for(&self, temp_table: &str) -> bool {
        !self.temp_table.is_empty() && self.temp_table == temp_table
    }

    pub fn from_profile(profile: &TableProfile, table_name: &str, rules: Vec<MultiRuleView>) -> Self {
        let table = TableProfileView::from_profile(profile, table_name);
        Self {
            temp_table: profile.table_name.clone(),
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

/// 历史列表一页条数。
///
/// 查询上限与界面提示共用这一处：两处各写一个数字，提示迟早与事实不符。
/// 上限本身是**可摘信息**（更老的版本还在库里，只是没列出来），所以界面必须明示
/// （与采样提示 D15 同一立场：别让人把截断的列表当成全部）。
pub const HISTORY_PAGE_SIZE: usize = 10;

/// 清理旧快照时默认保留的天数。
///
/// **界面文案与实际切档共用这一处**（对话框写「N 天前」、接缝按同一个 N 删），
/// 否则会出现「说好清 30 天却按 7 天删」这类最坏的不一致。
///
/// 取值待拍板（架构 §12 **Q4**）：先按原型的 30 天落地——它只影响「多久算旧」，
/// 与「每列最多留 N 版」（`store::body::MAX_VERSIONS_PER_COLUMN`）是两道独立的闸。
pub const SNAPSHOT_RETENTION_DAYS: i64 = 30;

/// 快照历史（「历史」Tab，Phase 5.1 / 5.2）
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryView {
    /// 实体名（当前只做列：就是列名）
    pub entity: String,
    /// 版本列表（最新在前）
    pub entries: Vec<HistoryEntryView>,
    /// 存储用量（取不到就不显示数字，而不是显示 0）
    pub stats: Option<StorageStatsView>,
    /// 列表是否被分页截断（列表已满一页；不代表库里一定还有更多）
    pub truncated: bool,
    /// 版本对比（Phase 5.2）：`None` = 没在对比（面板不出现）。
    ///
    /// 对比结果也在**载荷**里而不在视图字段里：面板「有没有对比"只认载荷，
    /// 视图只管「用户点了哪一版」（与 D35「渲染以载荷为准」同一口径）。
    pub diff: Option<VersionDiffView>,
    /// 上次清理的回执（Phase 5.3）：同样是载荷的一部分——下次刷新自然消失，
    /// 正是「一次性动作结果」该有的寿命。
    pub cleanup: Option<CleanupOutcome>,
}

/// 一次清理的结果（面板给一行回执）。
///
/// 两侧条数分开报：正文与元数据是**成对写入**的（D16），删的时候也必须成对——
/// 两边对不上就是半写的信号，不能被一句「清理成功」盖过去。
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupOutcome {
    /// 切档用的天数（界面写的就是这个数）
    pub days: i64,
    /// 正文删掉的条数（项目 DuckDB）
    pub body_removed: usize,
    /// 元数据删掉的条数（项目 SQLite）
    pub meta_removed: usize,
}

impl CleanupOutcome {
    /// 两侧对上了（没有半写残留）
    pub fn is_balanced(&self) -> bool {
        self.body_removed == self.meta_removed
    }

    /// 面板那行回执
    pub fn summary(&self) -> String {
        if self.body_removed == 0 && self.meta_removed == 0 {
            return format!("没有 {} 天前的快照", self.days);
        }
        if !self.is_balanced() {
            return format!(
                "正文删了 {} 条、版本链删了 {} 条，两侧对不上（可能有一次半写）",
                self.body_removed, self.meta_removed
            );
        }
        format!("清理了 {} 条快照（{} 天前）", self.body_removed, self.days)
    }
}

/// 一个版本（列表里的一行）
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntryView {
    pub version_id: String,
    /// 短版本号（前 8 位；完整值太长，列表里放不下）
    pub short_version: String,
    pub created_at: String,
    pub data_type: String,
    /// 父版本（首版为 `None`）：版本链靠它串起来
    pub has_parent: bool,
    /// 【K14】有父版本、但父版**不在**这份列表里——链的起段被清理剪掉了。
    ///
    /// 只在列表**没被分页截断**时才判：截断时最后一页的最旧一版本来就看不到父版，
    /// 那不是说「被清理了」。
    pub chain_truncated: bool,
    /// 是否最新一版（列表里打「当前」标记）
    pub is_latest: bool,
}

/// 存储用量（面板底部的统计行）
#[derive(Debug, Clone, PartialEq)]
pub struct StorageStatsView {
    pub total_snapshots: usize,
    pub unique_columns: usize,
    /// 后端给的展示串（带单位），面板不自己换算
    pub size_display: String,
}

/// 版本对比（`HistoryView::diff`，Phase 5.2）。
///
/// 方向固定为**「选中的那一版 → 最新一版」**：这个 Tab 问的是「和上次比变了什么」，
/// 两个方向都能选只是多一个状态（还要多一套「谁是基准」的文案）。
#[derive(Debug, Clone, PartialEq)]
pub struct VersionDiffView {
    /// 基准版本（对比面板标题里的那个时间）
    pub baseline_version: String,
    pub baseline_label: String,
    pub latest_label: String,
    pub rows: Vec<DiffRowView>,
    /// 有变化的行数（面板头据此给结论，不数第二遍）
    pub changed: usize,
}

/// 对比面板的一行：`旧 → 新` + 差值
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRowView {
    pub label: String,
    /// 基准版本的展示值（新类型不再产这行时是「—」，不假装还有值）
    pub old: String,
    pub new: String,
    pub delta: DeltaView,
}

/// 变化方向。
///
/// **方向 ≠ 好坏**：空值率升是坏，唯一值升不一定是好，所以这里只给方向，
/// 由视图映射到中性的主题角色（不用 `success` / `danger` 预设评价）。
#[derive(Debug, Clone, PartialEq)]
pub enum DeltaView {
    /// 数值增加（文案已带符号与单位）
    Up(String),
    /// 数值减少
    Down(String),
    /// 两侧展示值完全一致
    Same,
    /// 变了，但算不出数值差（类型 / 区间 / 带单位的文本值）
    Changed,
}

impl DeltaView {
    /// 面板头数「几项有变化」用
    pub fn is_changed(&self) -> bool {
        !matches!(self, DeltaView::Same)
    }

    /// 差值文案（不变 / 已变这两档没有数字）
    pub fn text(&self) -> &str {
        match self {
            DeltaView::Up(text) | DeltaView::Down(text) => text,
            DeltaView::Same => "不变",
            DeltaView::Changed => "已变",
        }
    }
}

impl VersionDiffView {
    /// 两份领域画像 → 对比视图（纯函数）。
    ///
    /// 行集合与「列」Tab 的基础统计**同源**（同一个 [`ColumnProfileView`]）：
    /// 对比里出现的数字就是用户已经读过的那几个，不另立一套口径。
    pub fn between(
        baseline: &ColumnInsightFull,
        baseline_label: &str,
        latest: &ColumnInsightFull,
        latest_label: &str,
    ) -> Self {
        let old = ColumnProfileView::from_domain(baseline);
        let new = ColumnProfileView::from_domain(latest);
        let s = &baseline.stats;
        let n = &latest.stats;

        let mut rows = Vec::new();

        // 头号结论先给：质量分（两侧都算得出才有——全空列不产分，D2 口径）
        if let (Some(old_score), Some(new_score)) = (&old.score, &new.score) {
            rows.push(DiffRowView::scored("质量分", old_score.overall, new_score.overall));
        }

        rows.push(DiffRowView::count(
            "总行数",
            s.total_count as f64,
            n.total_count as f64,
        ));
        rows.push(DiffRowView::count(
            "非空值",
            s.total_count.saturating_sub(s.null_count) as f64,
            n.total_count.saturating_sub(n.null_count) as f64,
        ));
        // 「空值」与「空值率」拆成两行：「列」Tab 把它俩写成了一行
        // （`12（14.3%）`），而对比要的是能对齐的两个数
        rows.push(DiffRowView::count(
            "空值",
            s.null_count as f64,
            n.null_count as f64,
        ));
        rows.push(DiffRowView::rate("空值率", s.null_rate, n.null_rate));
        rows.push(match (s.unique_count, n.unique_count) {
            (Some(a), Some(b)) => DiffRowView::count("唯一值", a as f64, b as f64),
            // 至少一侧算不出唯一值（未识别类型）就不给差值
            _ => DiffRowView::text("唯一值", "—", "—"),
        });
        rows.push(DiffRowView::text("类型", &s.data_type, &n.data_type));

        // 类型专属行（平均 / 长度范围 / 跨度 / True 占比 …）逐行对齐：
        // 能解成数字就给数值差，否则只说「已变」（区间 / 类别串算不出差值）
        for row in &new.basics {
            if CANONICAL_LABELS.contains(&row.label) {
                continue;
            }
            let paired = old.basics.iter().find(|r| r.label == row.label);
            rows.push(DiffRowView::from_display(
                row.label,
                paired.map(|r| r.value.as_str()),
                &row.value,
            ));
        }
        // 旧版有、新版没有的行（类型变了）：如实列出来，新版侧写「—」
        for row in &old.basics {
            if CANONICAL_LABELS.contains(&row.label)
                || new.basics.iter().any(|r| r.label == row.label)
            {
                continue;
            }
            rows.push(DiffRowView::from_display(row.label, Some(&row.value), "—"));
        }

        let changed = rows.iter().filter(|r| r.delta.is_changed()).count();
        Self {
            baseline_version: String::new(),
            baseline_label: baseline_label.to_string(),
            latest_label: latest_label.to_string(),
            rows,
            changed,
        }
    }

    /// 挂上基准版本号（服务知道对的是哪一版；`between` 只算差值）
    pub fn with_baseline_version(mut self, version_id: &str) -> Self {
        self.baseline_version = version_id.to_string();
        self
    }

    /// 面板头的一句结论
    pub fn summary(&self) -> String {
        if self.changed == 0 {
            return format!("与 {latest} 完全一致", latest = self.latest_label);
        }
        format!("{} 项有变化", self.changed)
    }
}

/// 对比里我亲自给出的行（不参与「类型专属行」的二次扫描）
const CANONICAL_LABELS: [&str; 5] = ["总行数", "非空值", "空值", "唯一值", "类型"];

impl DiffRowView {
    /// 计数行（整数展示 + 整数差）
    fn count(label: &str, old: f64, new: f64) -> Self {
        Self {
            label: label.to_string(),
            old: fmt_int(old.max(0.0) as u32),
            new: fmt_int(new.max(0.0) as u32),
            delta: delta_of(old, new, |v| fmt_int(v as u32), ""),
        }
    }

    /// 占比行（1 位小数的百分数）
    fn rate(label: &str, old: f64, new: f64) -> Self {
        // 先按**展示精度**取整再算差：否则会出现「显示 8.2% → 8.2% 却 +0.01」的自相矛盾
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        let (a, b) = (round(old), round(new));
        Self {
            label: label.to_string(),
            old: fmt_pct(a),
            new: fmt_pct(b),
            delta: delta_of(a, b, fmt_pct, ""),
        }
    }

    /// 评分行（与评分卡同一精度：整数）
    fn scored(label: &str, old: f64, new: f64) -> Self {
        let (a, b) = (old.round(), new.round());
        Self {
            label: label.to_string(),
            old: format!("{a:.0}"),
            new: format!("{b:.0}"),
            delta: delta_of(a, b, |v| format!("{v:.0}"), ""),
        }
    }

    /// 纯文本行（类型 / 唯一值这类两侧都是字符串的）
    fn text(label: &str, old: &str, new: &str) -> Self {
        Self::from_display(label, Some(old), new)
    }

    /// 文本行：能解成数字就给数值差，否则只说变没变
    fn from_display(label: &str, old: Option<&str>, new: &str) -> Self {
        let old_text = old.unwrap_or("—").to_string();
        let delta = match (old.and_then(display_number), display_number(new)) {
            (Some((a, unit_a)), Some((b, unit_b))) if unit_a == unit_b => {
                let suffix = if unit_a { "%" } else { "" };
                delta_of(a, b, fmt_delta_number, suffix)
            }
            _ if old_text == new => DeltaView::Same,
            _ => DeltaView::Changed,
        };
        Self {
            label: label.to_string(),
            old: old_text,
            new: new.to_string(),
            delta,
        }
    }
}

/// 方向 + 差值文案（差为零就是「不变」；`show` 拿到的是**绝对值**，符号由这里加）
fn delta_of(old: f64, new: f64, show: impl Fn(f64) -> String, suffix: &str) -> DeltaView {
    let diff = new - old;
    if diff.abs() < f64::EPSILON {
        return DeltaView::Same;
    }
    let sign = if diff > 0.0 { "+" } else { "-" };
    let text = format!("{sign}{}{suffix}", show(diff.abs()));
    if diff > 0.0 {
        DeltaView::Up(text)
    } else {
        DeltaView::Down(text)
    }
}

/// 差值里的数值文案：整数加千分位（与计数行同一口径），小数去尾零
fn fmt_delta_number(v: f64) -> String {
    if v.fract() == 0.0 && v < u32::MAX as f64 {
        return fmt_int(v as u32);
    }
    fmt_num(v)
}

/// 展示串 → 数值（只认「纯数值」：数字 + 千分位 + 可有 `%` 后缀）。
///
/// 带单位 / 区间 / 类别串（`1.2 万` / `3–8` / `Top 3 类` / `2.5（右偏）`）一律不认——
/// 宁可不给差值，也不给一个错的差值。返回值第二项表示「这是百分数」。
fn display_number(text: &str) -> Option<(f64, bool)> {
    let trimmed = text.trim();
    let (body, percent) = match trimmed.strip_suffix('%') {
        Some(rest) => (rest, true),
        None => (trimmed, false),
    };
    let parsed = body.replace(',', "").parse::<f64>().ok()?;
    parsed.is_finite().then_some((parsed, percent))
}

impl HistoryView {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 领域版本列表 + 存储统计 → 视图模型（纯函数）。
    ///
    /// `entries` 由存储层按「最新在前」返回（`ORDER BY created_at DESC, rowid DESC`），
    /// 这里不再排序：**显示顺序与写入顺序只能有一处权威**。
    pub fn from_entries(
        entity: &str,
        entries: &[crate::store::InsightVersionEntry],
        stats: Option<&crate::store::InsightStorageStats>,
    ) -> Self {
        Self {
            entity: entity.to_string(),
            entries: entries
                .iter()
                .enumerate()
                .map(|(index, entry)| HistoryEntryView {
                    version_id: entry.version_id.clone(),
                    short_version: entry.version_id.chars().take(8).collect(),
                    created_at: entry.created_at.clone(),
                    data_type: entry.data_type.clone().unwrap_or_else(|| "—".to_string()),
                    has_parent: entry.parent_version_id.is_some(),
                    chain_truncated: {
                        // 三条件同时成立才能说链被剪过：「列表完整（未分页截断）」+
                        // 「最旧一版」+「父版不在此列」——否则就是分页造成的假阳性（K14）
                        let is_oldest = index + 1 == entries.len();
                        let page_complete = entries.len() < HISTORY_PAGE_SIZE;
                        is_oldest
                            && page_complete
                            && entry
                                .parent_version_id
                                .as_deref()
                                .is_some_and(|parent| {
                                    !entries.iter().any(|other| other.version_id == parent)
                                })
                    },
                    is_latest: index == 0,
                })
                .collect(),
            truncated: entries.len() >= HISTORY_PAGE_SIZE,
            diff: None,
            cleanup: None,
            stats: stats.map(|stats| StorageStatsView {
                total_snapshots: stats.total_snapshots,
                unique_columns: stats.unique_columns,
                size_display: stats.total_size_display.clone(),
            }),
        }
    }

    /// 存储统计行文案（拿不到统计返回 `None`，不编一个 0 出来）
    pub fn stats_line(&self) -> Option<String> {
        let stats = self.stats.as_ref()?;
        Some(format!(
            "存储 {} · {} 个快照 · {} 列",
            stats.size_display, stats.total_snapshots, stats.unique_columns
        ))
    }

    /// 挂上对比结果（服务读两版正文后调用）
    pub fn with_diff(mut self, diff: VersionDiffView) -> Self {
        self.diff = Some(diff);
        self
    }

    /// 放下对比结果（用户点 ✕，或基准版已不在列表里）
    pub fn without_diff(mut self) -> Self {
        self.diff = None;
        self
    }

    /// 挂上清理回执（服务删完再读回列表时调用）
    pub fn with_cleanup(mut self, outcome: CleanupOutcome) -> Self {
        self.cleanup = Some(outcome);
        self
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
    use super::types::{
        ColumnQualityEntry, DistributionBin, ExtremeValue, TableColumnMeta, TextFrequency,
    };
    use super::*;
    use crate::rule_types::{ExecutionResult, QualityCheck, QualityReport};
    use crate::store::{InsightStorageStats, InsightVersionEntry};

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
            database: "shop".into(),
            schema: None,
        };
        assert_eq!(schema.default_tab(), PanelTab::Schema);
        assert_eq!(schema.title(), "全部结构");
        assert_eq!(schema.detail().as_deref(), Some("G_1"));
        assert_eq!(schema.temp_table(), "", "结构目标没有临时表");
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
        assert!(map.contains(&("相关系数", "0.8765")), "已知字段名给中文标签：{map:?}");
        assert!(map.contains(&("样本量", "120")), "整数不带小数点：{map:?}");
        assert!(
            map.contains(&("note", "—")),
            "认不出的字段名原样展示（不猜中文），NULL 显式显示"
        );
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
        assert_eq!(
            headers,
            vec!["row_label", "col_label", "计数"],
            "认不出的字段名原样保留（这里是构造的假名）：{headers:?}"
        );
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

    // ==================== 快照历史（Phase 5.1） ====================

    fn version(
        id: &str,
        parent: Option<&str>,
        created_at: &str,
        data_type: &str,
    ) -> InsightVersionEntry {
        InsightVersionEntry {
            snapshot_id: format!("snap-{id}"),
            column_name: "amount".into(),
            data_type: Some(data_type.into()),
            stats_json: "{}".into(),
            version_id: id.into(),
            parent_version_id: parent.map(str::to_string),
            checksum: "sum".into(),
            created_at: created_at.into(),
        }
    }

    fn storage_stats() -> InsightStorageStats {
        InsightStorageStats {
            total_snapshots: 3,
            unique_columns: 1,
            total_size_bytes: 2048.0,
            total_size_display: "2.0 KB".into(),
        }
    }

    #[test]
    fn history_marks_latest_and_root_without_reordering() {
        // 存储层已按「最新在前」返回（D18）：视图**不再排序**，只贴标记
        let entries = vec![
            version(
                "aaaaaaaa-1111",
                Some("bbbbbbbb-2222"),
                "2026-09-15 14:22",
                "DOUBLE",
            ),
            version("bbbbbbbb-2222", None, "2026-09-14 09:10", "DOUBLE"),
        ];
        let view = HistoryView::from_entries("amount", &entries, None);

        assert_eq!(view.entity, "amount");
        assert_eq!(
            view.entries
                .iter()
                .map(|e| e.version_id.as_str())
                .collect::<Vec<_>>(),
            vec!["aaaaaaaa-1111", "bbbbbbbb-2222"],
            "顺序取存储层给的，视图不得重排"
        );
        assert!(view.entries[0].is_latest);
        assert!(!view.entries[1].is_latest, "只有首行是「当前」");
        assert!(view.entries[0].has_parent);
        assert!(
            !view.entries[1].has_parent,
            "首版无父版本，界面据此打「首版」"
        );
        // 短版本号是前 8 位（列表里放不下完整 uuid）
        assert_eq!(view.entries[0].short_version, "aaaaaaaa");
        assert_eq!(view.entries[1].short_version, "bbbbbbbb");
        assert_eq!(view.entries[0].created_at, "2026-09-15 14:22");
    }

    /// K14：链的起段被清理剪掉时，最旧一版**不谎称首版**，而是标「更早的版本已清理」。
    ///
    /// 三个条件必须同时成立（否则就是假阳性）：最旧一版 / 有父版 / 父版不在此列表，
    /// 且列表**未被分页截断**——截断时最旧一版的父版本来就看不到。
    #[test]
    fn history_flags_a_truncated_chain_only_on_a_complete_page() {
        // 父版被清理（不在此列表） → 标上
        let cleaned = vec![
            version(
                "aaaaaaaa-1111",
                Some("deadbeef-9999"),
                "2026-09-15 14:22",
                "DOUBLE",
            ),
            version(
                "bbbbbbbb-2222",
                Some("deadbeef-9999"),
                "2026-09-14 09:10",
                "DOUBLE",
            ),
        ];
        let view = HistoryView::from_entries("amount", &cleaned, None);
        assert!(
            !view.entries[0].chain_truncated,
            "不是最旧一版，不标"
        );
        assert!(view.entries[1].chain_truncated, "最旧一版的父版不在列表里");

        // 父版就在列表里（正常的链） → 不标
        let intact = vec![
            version(
                "aaaaaaaa-1111",
                Some("bbbbbbbb-2222"),
                "2026-09-15 14:22",
                "DOUBLE",
            ),
            version("bbbbbbbb-2222", None, "2026-09-14 09:10", "DOUBLE"),
        ];
        let view = HistoryView::from_entries("amount", &intact, None);
        assert!(view.entries.iter().all(|e| !e.chain_truncated));

        // 列表被分页截断（满页） → 不能断言「被清理」（父版可能在下一页）
        let full_page: Vec<_> = (0..HISTORY_PAGE_SIZE)
            .map(|i| {
                version(
                    &format!("{i:08}-page"),
                    Some(&format!("{:08}-page", i + 1)),
                    "2026-09-15 14:22",
                    "DOUBLE",
                )
            })
            .collect();
        let view = HistoryView::from_entries("amount", &full_page, None);
        assert!(
            view.entries.iter().all(|e| !e.chain_truncated),
            "满页时最旧一版的父版可能就在下一页，不得误报"
        );
    }

    #[test]
    fn history_stats_line_needs_real_stats() {
        let entries = vec![version("aaaaaaaa-1111", None, "2026-09-15 14:22", "DOUBLE")];
        // 取不到统计就不显示数字（编一个 0 会让人以为历史被清了）
        assert!(
            HistoryView::from_entries("amount", &entries, None)
                .stats_line()
                .is_none()
        );

        let stats = storage_stats();
        let view = HistoryView::from_entries("amount", &entries, Some(&stats));
        let line = view.stats_line().expect("有统计就该有一行");
        assert!(line.contains("2.0 KB"), "用量取后端给的展示串：{line}");
        assert!(
            line.contains('3') && line.contains('1'),
            "带上快照数与列数：{line}"
        );
    }

    #[test]
    fn history_flags_a_full_page_and_missing_types() {
        // 缺类型的快照（列在那时还不带类型）不能显示成空白格
        let single = vec![version("aaaaaaaa-1111", None, "2026-09-15 14:22", "DOUBLE")];
        let mut no_type = single.clone();
        no_type[0].data_type = None;
        assert_eq!(
            HistoryView::from_entries("amount", &no_type, None).entries[0].data_type,
            "—"
        );

        // 空历史：不是错误，列表区显示引导
        let empty = HistoryView::from_entries("amount", &[], None);
        assert!(empty.is_empty());
        assert!(!empty.truncated);

        // 满一页就明示「只列出最近 N 条」（与采样提示 D15 同一立场）
        let full: Vec<_> = (0..HISTORY_PAGE_SIZE)
            .map(|i| version(&format!("v{i:07}"), None, "2026-09-15 14:22", "DOUBLE"))
            .collect();
        assert!(HistoryView::from_entries("amount", &full, None).truncated);
        let short = &full[..HISTORY_PAGE_SIZE - 1];
        assert!(!HistoryView::from_entries("amount", short, None).truncated);
    }

    #[test]
    fn cleanup_receipt_states_what_it_actually_deleted() {
        let none = CleanupOutcome {
            days: SNAPSHOT_RETENTION_DAYS,
            body_removed: 0,
            meta_removed: 0,
        };
        assert!(none.is_balanced());
        assert!(
            none.summary().contains("没有 30 天前"),
            "没东西可清时说清楚：{}",
            none.summary()
        );

        let done = CleanupOutcome {
            days: 30,
            body_removed: 3,
            meta_removed: 3,
        };
        assert_eq!(done.summary(), "清理了 3 条快照（30 天前）");

        // 两侧对不上 = 半写信号（D16）：不能被一句「清理完成」盖过去
        let uneven = CleanupOutcome {
            days: 30,
            body_removed: 3,
            meta_removed: 2,
        };
        assert!(!uneven.is_balanced());
        let text = uneven.summary();
        assert!(
            text.contains('3') && text.contains('2') && text.contains("对不上"),
            "两侧条数都要写出来：{text}"
        );
    }

    // ==================== 版本对比（Phase 5.2） ====================

    /// 基准 / 最新一对画像：行数翻倍、空值大幅减少、唯一值占比升高
    fn compare_pair() -> (ColumnInsightFull, ColumnInsightFull) {
        let numeric = || base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        let old = numeric();
        let mut new = numeric();
        new.stats.total_count = 2000;
        new.stats.null_count = 10;
        new.stats.null_rate = 0.005;
        new.stats.unique_count = Some(1900);
        (old, new)
    }

    fn row_of<'a>(diff: &'a VersionDiffView, label: &str) -> &'a DiffRowView {
        diff.rows
            .iter()
            .find(|r| r.label == label)
            .unwrap_or_else(|| panic!("应有「{label}」这一行：{:?}", diff.rows))
    }

    #[test]
    fn diff_reports_direction_for_counts_and_rates() {
        let (old, new) = compare_pair();
        let diff = VersionDiffView::between(&old, "2026-09-14 09:10", &new, "2026-09-15 14:22")
            .with_baseline_version("v-1");

        assert_eq!(diff.baseline_version, "v-1");
        assert_eq!(diff.baseline_label, "2026-09-14 09:10");
        // 总行数 1000 → 2000：千分位与「列」Tab 同一口径
        let total = row_of(&diff, "总行数");
        assert_eq!((total.old.as_str(), total.new.as_str()), ("1,000", "2,000"));
        assert_eq!(total.delta, DeltaView::Up("+1,000".into()));
        // 空值率 2.0% → 0.5%：（差值按**展示精度**算，不出现「显示没变却 +0.01」）
        let rate = row_of(&diff, "空值率");
        assert_eq!((rate.old.as_str(), rate.new.as_str()), ("2.0%", "0.5%"));
        assert_eq!(rate.delta, DeltaView::Down("-1.5%".into()));
        // 唯一值 480 → 1900
        assert_eq!(
            row_of(&diff, "唯一值").delta,
            DeltaView::Up("+1,420".into())
        );
        assert_eq!(row_of(&diff, "类型").delta, DeltaView::Same);

        // 质量分排在最前（它才是「到底变好了没」的结论），展示精度与评分卡一致
        let scored = ColumnProfileView::from_domain(&old)
            .score
            .expect("有数据就有分")
            .overall;
        assert_eq!(diff.rows[0].label, "质量分");
        assert_eq!(diff.rows[0].old, format!("{:.0}", scored.round()));

        // 变化行数与摘要是同一口径，不数第二遍
        assert_eq!(
            diff.changed,
            diff.rows.iter().filter(|r| r.delta.is_changed()).count()
        );
        assert_eq!(diff.summary(), format!("{} 项有变化", diff.changed));
    }

    #[test]
    fn diff_says_same_when_nothing_moved() {
        let full = base_stats(ColumnStatsDetail::Numeric(numeric(Some(0.2), Vec::new())));
        let diff = VersionDiffView::between(&full, "2026-09-14 09:10", &full, "2026-09-15 14:22");
        assert!(
            diff.rows.iter().all(|r| !r.delta.is_changed()),
            "同一份数据不该报出变化：{:?}",
            diff.rows
        );
        assert_eq!(diff.changed, 0);
        assert!(
            diff.summary().contains("完全一致"),
            "摘要应直接给结论：{}",
            diff.summary()
        );
    }

    #[test]
    fn diff_keeps_text_rows_and_dropped_rows_honest() {
        // 数值 → 文本：数值专属行在新版没有对应行，如实写「—」而不是编一个
        let old = base_stats(ColumnStatsDetail::Numeric(numeric(Some(2.5), Vec::new())));
        let mut new = base_stats(ColumnStatsDetail::Text(TextStats {
            min_length: 3,
            max_length: 8,
            top_values: vec![TextFrequency {
                value: "a".into(),
                count: 5,
                ratio: 0.5,
            }],
        }));
        new.stats.data_type = "VARCHAR".into();
        let diff = VersionDiffView::between(&old, "2026-09-14 09:10", &new, "2026-09-15 14:22");

        let kind = row_of(&diff, "类型");
        assert_eq!(kind.delta, DeltaView::Changed, "类型变了就是变了");
        assert_eq!((kind.old.as_str(), kind.new.as_str()), ("DECIMAL(12,2)", "VARCHAR"));

        // 旧版独有的行保留，新版侧写「—」（不假装还有值）
        let avg = row_of(&diff, "平均");
        assert_eq!(avg.old, fmt_num(123.4567));
        assert_eq!(avg.new, "—");
        assert_eq!(avg.delta, DeltaView::Changed);
        // 带方向的展示串（`2.5（右偏）`）解不出数字：两侧一致时说「不变」
        // （那一支在 `diff_says_same_when_nothing_moved` 里盖住），这里只说它不编差值
        let skew = row_of(&diff, "偏度");
        assert_eq!(skew.old, "2.5（右偏）");
        assert_eq!(skew.new, "—");
        assert_eq!(skew.delta, DeltaView::Changed);
        // 新版独有的行照旧出现
        assert_eq!(row_of(&diff, "长度范围").new, "3 ~ 8");
    }

    #[test]
    fn diff_splits_the_null_row_into_count_and_rate() {
        // 「列」Tab 把空值写成一行（`20（2.0%）`）便于阅读，而对比要的是**能对齐的两个数**
        let (old, new) = compare_pair();
        let diff = VersionDiffView::between(&old, "09-14", &new, "09-15");
        assert_eq!(row_of(&diff, "空值").delta, DeltaView::Down("-10".into()));
        assert_eq!(row_of(&diff, "空值率").new, "0.5%");
        assert_eq!(row_of(&diff, "非空值").delta, DeltaView::Up("+1,010".into()));
    }
}
