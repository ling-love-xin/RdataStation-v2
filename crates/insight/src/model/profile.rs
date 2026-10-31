//! 洞察模型 · 表探查视图模型（自 model.rs 拆分，纯位移）

use super::*;
use crate::model::types::TextFrequency;
// ==================== 表探查视图模型（Phase 3.1） ====================

/// 表探查（「表」Tab）：列元数据表 + 行数 + 表级质量（评估后才有）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableProfileView {
    /// 逻辑表名（目标里的 `table_name`，用于展示与快照归属）
    pub table_name: String,
    /// 数据源标签（当前恒为「DuckDB 临时表」：面板只认临时表，见 D20）
    pub source_label: &'static str,
    pub row_count: u64,
    /// 整行重复产生的冗余行数（`SUM(组内行数 - 1)`；画像时引擎 `GROUP BY ALL`
    /// 轻查询回填；未查 / 失败为 `None`，不编假值）。概览五卡「重复行」口径 = 可去重行。
    pub dup_rows: Option<u64>,
    pub columns: Vec<TableColumnView>,
    /// 表级质量：`None` = 还没「评估全表」（不是 0 分）
    pub quality: Option<TableQualityView>,
    /// 评估进度：进行中才有
    pub progress: Option<TableEvalProgress>,
    /// EDA 样本（前 5 行）：画像时引擎顺带取的轻查询；失败为 `None`（不编假值）
    pub sample: Option<TableSample>,
}

/// EDA 样本区数据（原型 .sample-section 表格）：列名 + 至多 5 行文本值。
#[derive(Debug, Clone, PartialEq)]
pub struct TableSample {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
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
    /// EDA 列统计摘要：空值率（0–1）。表画像时由引擎一次查询回填；
    /// 查询失败 / 未跑为 `None`（不编假值）。
    pub null_rate: Option<f64>,
    /// EDA 列统计摘要：唯一率（唯一值数 / 非空行数，0–1）。
    pub unique_rate: Option<f64>,
    /// 列级统计明细（评估全表后回填；未评估为 `None`）：
    /// 列卡「基础统计 / 分布」区直接消费它，不再单独发查询。
    pub stats_detail: Option<ColumnStatsDetail>,
    /// 数值列直方图分箱（评估后回填；样本行数低于引擎下限时为 `None`）。
    pub histogram: Option<Vec<DistributionBin>>,
    /// 该列样本值（评估后从全量画像的样本列取前 5 条；未评估为空）。
    pub sample_values: Vec<String>,
    /// 取样档迷你统计（C7：A 方案——迷你统计并入取样链路）。
    ///
    /// 取样档对**评估全表**（确认窗口生成的临时表）做查询级过滤现算（Q2：行 LIMIT +
    /// 列投影，不建子表），逐列产出轻量分布供列卡折叠态迷你图。与评估产物
    /// （`stats_detail` / `histogram`）互不覆盖：全量档读评估明细，取样档读本字段；
    /// 取样档未算 / 查询失败 / 列无有效样本为 `None`（不编假值）。
    pub mini_stats: Option<ColumnMiniStats>,
    /// 评估中断标记（C5）：评估全表被取消 / 中断时，未完成列置 `true`——
    /// 列卡灰态显示「未完成，可补算」，点补算单列续跑后清除。
    pub interrupted: bool,
}

/// 取样档迷你统计（列卡折叠态迷你图的数据源，与 [`TableColumnView::mini_stats`] 配套）。
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnMiniStats {
    /// 数值列：8 柱等宽直方图（与评估直方图同族，label 仅展示、迷你图只读 count）
    Numeric(Vec<DistributionBin>),
    /// 文本 / 时间列：Top 3 值频次
    Frequencies(Vec<TextFrequency>),
    /// 布尔列：真值占比（0–1；无布尔样本时不渲染）
    Boolean { true_ratio: f64 },
}

/// EDA 列统计摘要：引擎对临时表所有列一次查询得到的最小统计集。
///
/// 用于概览五卡（缺失率 / 唯一率）、缺失矩阵与值得关注区——这些指标
/// 从列级统计现算，不依赖逐列跑规则（规则是「评估全表」的贵工序）。
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnStatBrief {
    pub name: String,
    /// 空值率（0–1）；列全空 / 查询失败为 `None`
    pub null_rate: Option<f64>,
    /// 唯一率（唯一值数 / 非空行数，0–1）
    pub unique_rate: Option<f64>,
}

/// 列检查器状态（原型「检查器」四态）：命中 / 未命中 / 不适用-类型 / 不适用-列排除。
///
/// 「列排除」是用户显式对某列关闭某规则的结果——规则按列指定执行功能未接入前
/// 无数据来源，数据侧暂不产生该态（枚举位保留，视图口径先行对齐原型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnCheckState {
    /// 检查通过（命中）
    Hit,
    /// 检查未通过（未命中）
    Miss,
    /// 该检查器不适用于此列类型（如唯一率对未知类型列）
    NotApplicableType,
    /// 该检查器被显式排除（待「规则按列指定」接入）
    Excluded,
}

/// 一条列检查器（原型「检查器」区的数据行）：列名 + 检查名 + 四态 + 判定依据。
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnChecker {
    pub column: String,
    pub name: String,
    pub state: ColumnCheckState,
    /// 命中 = 判定依据；未命中 = 实际值；不适用 = 原因
    pub detail: String,
}

/// 表级事实摘要：只挂检查器命中事实，**不含总分 / 等级 / 加权合成**。
///
/// 定位口径：洞察是会话级 EDA 探索，只回答「这份数据长什么样」；
/// 质量评级、归档打分归 M6 资产库，本视图模型不承载评级结论。
#[derive(Debug, Clone, PartialEq)]
pub struct TableQualityView {
    /// 列检查器（评估后逐列内置判定聚合）
    pub checker: Vec<ColumnChecker>,
    /// 适用检查器数（排除不适用-类型 / 列排除）
    pub checker_applicable: usize,
    /// 命中检查器数（适用且通过的条数）
    pub checker_hits: usize,
}

/// 「评估全表」的进度（串行逐列统计，避免撞后端并发上限）
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
                null_rate: None,
                unique_rate: None,
                stats_detail: None,
                histogram: None,
                sample_values: Vec::new(),
                mini_stats: None,
                interrupted: false,
            })
            .collect();
        Self {
            table_name: table_name.to_string(),
            source_label: "DuckDB 临时表",
            row_count: profile.row_count.unwrap_or(0).max(0) as u64,
            dup_rows: None,
            columns,
            quality: None,
            progress: None,
            sample: None,
        }
    }

    /// EDA 列统计摘要回填：按列名把引擎一次查询的结果写回视图模型。
    ///
    /// 摘要查询失败时调用方保留 `None`（不编假值），本方法只在拿到结果后调用。
    pub fn fill_column_stats(&mut self, stats: &[ColumnStatBrief]) {
        for column in &mut self.columns {
            if let Some(brief) = stats.iter().find(|b| b.name == column.name) {
                column.null_rate = brief.null_rate;
                column.unique_rate = brief.unique_rate;
            }
        }
    }

    /// 评估进行中的中间态（不丢已有分数：用户看着分数一列列长出来）
    pub fn evaluating(&self, done: usize, total: usize) -> Self {
        let mut next = self.clone();
        next.progress = Some(TableEvalProgress { done, total });
        next
    }

    /// 写入一列的级联明细（评估循环里与进度同批回填）。
    ///
    /// `stats_detail` / `histogram` 直接来自该列的 `ColumnInsightFull`（同一次查询，
    /// 不重复取数）；`sample_values` 从全量画像的样本列截前 5 条文本值。
    pub fn with_column_detail(&self, column: &str, full: &ColumnInsightFull) -> Self {
        let mut next = self.clone();
        if let Some(row) = next.columns.iter_mut().find(|row| row.name == column) {
            row.stats_detail = Some(full.stats.stats_detail.clone());
            row.histogram = full.histogram.clone();
            row.sample_values = full
                .sample
                .iter()
                .take(5)
                .map(|v| match v {
                    serde_json::Value::Null => "NULL".to_string(),
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            // 补算成功：清除中断标记（C5）
            row.interrupted = false;
        }
        next
    }

    /// 评估中断（C5）：未完成（无列级明细）的列置中断标记，供视图灰态 + 补算。
    ///
    /// 已完成列不动（明细保留）；`progress` 保留中断时进度（视图据此维持
    /// 「评估中」的取消按钮语境，补算走单列入口）。
    pub fn mark_interrupted(&self) -> Self {
        let mut next = self.clone();
        for col in next.columns.iter_mut() {
            if col.stats_detail.is_none() {
                col.interrupted = true;
            }
        }
        next
    }

    /// 评估完成：清进度、挂一个空的表级事实摘要位。
    ///
    /// 表级摘要只承载检查器事实计数，由 [`with_checker`](Self::with_checker) 填充；
    /// 本模块不合成总分、不出等级——评级结论归归档后的资产库（M6）。
    pub fn finish_evaluation(&self) -> Self {
        let mut next = self.clone();
        next.progress = None;
        next.quality = Some(TableQualityView {
            checker: Vec::new(),
            checker_applicable: 0,
            checker_hits: 0,
        });
        next
    }

    /// 评估完成：写入列检查器聚合（四态 + 适用/命中计数）。
    ///
    /// 检查器由 [`compute_column_checks`](crate::quality_scorer::compute_column_checks)
    /// 在评估循环逐列现算，这里只做聚合；`applicable` 剔除不适用-类型 / 列排除，
    /// 命中计数**无评级语义**（洞察侧不审计，评级在归档后的资产库）。
    pub fn with_checker(&self, checks: Vec<ColumnChecker>) -> Self {
        let mut next = self.clone();
        if let Some(quality) = next.quality.as_mut() {
            let applicable = checks
                .iter()
                .filter(|c| c.state == ColumnCheckState::Hit || c.state == ColumnCheckState::Miss)
                .count();
            let hits = checks
                .iter()
                .filter(|c| c.state == ColumnCheckState::Hit)
                .count();
            quality.checker = checks;
            quality.checker_applicable = applicable;
            quality.checker_hits = hits;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C5：中断标记只落在未完成（无列级明细）列，已完成列不动。
    #[test]
    fn mark_interrupted_flags_only_pending_columns() {
        let profile = TableProfileView {
            table_name: "t".into(),
            source_label: "DuckDB 临时表",
            row_count: 3,
            dup_rows: None,
            columns: vec![
                TableColumnView {
                    index: 1,
                    name: "done".into(),
                    data_type: "INTEGER".into(),
                    kind: ColumnKind::Numeric,
                    nullable: false,
                    primary_key: false,
                    null_rate: None,
                    unique_rate: None,
                    stats_detail: Some(crate::model::types::ColumnStatsDetail::Unknown),
                    histogram: None,
                    sample_values: Vec::new(),
                    mini_stats: None,
                    interrupted: false,
                },
                TableColumnView {
                    index: 2,
                    name: "pending".into(),
                    data_type: "VARCHAR".into(),
                    kind: ColumnKind::Text,
                    nullable: true,
                    primary_key: false,
                    null_rate: None,
                    unique_rate: None,
                    stats_detail: None,
                    histogram: None,
                    sample_values: Vec::new(),
                    mini_stats: None,
                    interrupted: false,
                },
            ],
            quality: None,
            progress: None,
            sample: None,
        };

        let interrupted = profile.mark_interrupted();
        let done = interrupted.columns.iter().find(|c| c.name == "done").unwrap();
        let pending = interrupted.columns.iter().find(|c| c.name == "pending").unwrap();
        assert!(!done.interrupted, "已完成列不得标中断");
        assert!(pending.interrupted, "未完成列应标中断");
    }

    /// C5：补算成功（`with_column_detail`）清除该列中断标记。
    #[test]
    fn with_column_detail_clears_interrupted_flag() {
        let mut profile = TableProfileView {
            table_name: "t".into(),
            source_label: "DuckDB 临时表",
            row_count: 1,
            dup_rows: None,
            columns: vec![TableColumnView {
                index: 1,
                name: "c".into(),
                data_type: "VARCHAR".into(),
                kind: ColumnKind::Text,
                nullable: true,
                primary_key: false,
                null_rate: None,
                unique_rate: None,
                stats_detail: None,
                histogram: None,
                sample_values: Vec::new(),
                mini_stats: None,
                interrupted: true,
            }],
            quality: None,
            progress: None,
            sample: None,
        };
        // 构造最小补算产物（stats_detail 仅需非空 + 空 sample）
        let full = crate::model::types::ColumnInsightFull {
            stats: crate::model::types::ColumnStats {
                column_name: "c".into(),
                data_type: "VARCHAR".into(),
                total_count: 0,
                null_count: 0,
                null_rate: 0.0,
                unique_count: None,
                stats_detail: crate::model::types::ColumnStatsDetail::Unknown,
            },
            histogram: None,
            sample: Vec::new(),
        };
        profile = profile.with_column_detail("c", &full);
        let col = profile.columns.first().unwrap();
        assert!(col.stats_detail.is_some(), "补算应回填列明细");
        assert!(!col.interrupted, "补算成功应清除中断标记");
    }
}