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
    TextStats,
};

// 等级是 `quality_scorer` 的定义（阈值与文案的唯一来源），这里只借用类型
use crate::quality_scorer::Grade;

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
    Data(ColumnProfileView),
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

    /// 类型徽标文案
    pub fn label(self) -> &'static str {
        match self {
            ColumnKind::Numeric => "数值",
            ColumnKind::Text => "文本",
            ColumnKind::DateTime => "时间",
            ColumnKind::Boolean => "布尔",
            ColumnKind::Unknown => "未识别",
        }
    }
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
pub fn fmt_int(v: u32) -> String {
    let digits = v.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
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
    use super::types::{DistributionBin, ExtremeValue, TextFrequency};
    use super::*;

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
