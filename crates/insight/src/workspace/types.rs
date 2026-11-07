//! 工作区数据类型：形态枚举 / 已打开目标 / 画像摘要 / 报告 Tab / 列卡区 / 关联 Tab。

use crate::model::{ColumnCheckState, InsightSetEntry, InsightSetKind, TableProfileView};
use crate::InsightTarget;

/// 中间工作页的两种形态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    /// 单表画像（原型「洞察工作页」：工具栏 + 画像版式）
    Insight,
    /// 多表对比（D64 阶段②，不重新取数；三态：候选 → 运行 → 结果）
    Compare,
}

/// 会话内已进入洞察的目标（对比候选）。
///
/// 「只有进入过洞察的表才能选择对比」——候选源就是本列表：导航树 / 结果列表 /
/// SQL 编辑器等入口**在打开洞察时**都会经 [`super::InsightWorkspacePanel::open_target`] 登记。
/// 历史快照候选留待 `InsightTableReportStore` 落库后接入（D64 阶段②后半）。
#[derive(Debug, Clone)]
pub struct OpenedTarget {
    /// 稳定业务键（跨目标种类去重用，见 [`super::target_key`]）
    pub key: String,
    pub target: InsightTarget,
    /// 该目标最近一次完成的表画像摘要；未完成 / 非表目标为 `None`（不编假值）
    pub summary: Option<TargetSummary>,
}

impl OpenedTarget {
    /// 转成右栏「已洞察表」集合条目。
    ///
    /// 只收**表级**目标（列 / 多列 / Schema 没有独立的集合身份）；
    /// 来源种类判定：源表目标看有没有连接（有 = 源库表，`None` = 文件），
    /// 临时表目标（结果集 / 落库样本）一律按结果集呈现。
    pub(crate) fn to_set_entry(&self, is_current: bool) -> Option<InsightSetEntry> {
        let (kind, source, temp_table) = match &self.target {
            InsightTarget::SourceTable { source, .. } => {
                let kind = if source.conn_id().is_some() {
                    InsightSetKind::SourceTable
                } else {
                    InsightSetKind::File
                };
                (kind, source.label().to_string(), None)
            }
            InsightTarget::Table { temp_table, .. } => (
                InsightSetKind::ResultSet,
                temp_table.clone(),
                Some(temp_table.clone()),
            ),
            _ => return None,
        };
        let (row_count, column_count, checker_hits, checker_applicable) = match self.summary {
            Some(summary) => (
                summary.row_count,
                summary.column_count,
                summary.checker_hits,
                summary.checker_applicable,
            ),
            None => (None, None, None, None),
        };
        Some(InsightSetEntry {
            key: self.key.clone(),
            label: self.target.table_name(),
            kind,
            source,
            temp_table,
            row_count,
            column_count,
            checker_hits,
            checker_applicable,
            // 集合行「切换目标」的还原依据（宿主经 `request_insight_switch` 使用）
            target: self.target.clone(),
            is_current,
        })
    }
}

/// 画像摘要：对比聚合的最小事实集（行数 / 列数 / 检查器命中 / 缺失率 / 唯一率 /
/// 值得关注列数），**不重新取数**；不含质量总分或等级（评级归归档后的资产库）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSummary {
    pub row_count: Option<u64>,
    pub column_count: Option<usize>,
    /// 检查器命中计数（适用且通过；无评级语义）
    pub checker_hits: Option<usize>,
    pub checker_applicable: Option<usize>,
    /// 表级缺失率均值（0–1；列统计未跑为 `None`）
    pub null_rate: Option<f64>,
    /// 表级唯一率均值（0–1；列统计未跑为 `None`）
    pub unique_rate: Option<f64>,
    /// 值得关注列数（缺失率高于阈值 / 检查器未命中的列，评估后才有）
    pub attention_columns: Option<usize>,
}

impl TargetSummary {
    pub(crate) fn of_table(profile: &TableProfileView) -> Self {
        let (null_rate, unique_rate) = {
            let (mut null_sum, mut null_n, mut uniq_sum, mut uniq_n) = (0.0, 0usize, 0.0, 0usize);
            for c in &profile.columns {
                if let Some(n) = c.null_rate {
                    null_sum += n;
                    null_n += 1;
                }
                if let Some(u) = c.unique_rate {
                    uniq_sum += u;
                    uniq_n += 1;
                }
            }
            (
                (null_n > 0).then_some(null_sum / null_n as f64),
                (uniq_n > 0).then_some(uniq_sum / uniq_n as f64),
            )
        };
        // 值得关注列：缺失率 > 20% 或检查器未命中（事实口径，不依赖旧评分等级）
        let attention_columns = profile.quality.as_ref().map(|q| {
            let miss: std::collections::HashSet<&str> = q
                .checker
                .iter()
                .filter(|c| c.state == ColumnCheckState::Miss)
                .map(|c| c.column.as_str())
                .collect();
            profile
                .columns
                .iter()
                .filter(|c| {
                    miss.contains(c.name.as_str()) || c.null_rate.map(|r| r > 0.2).unwrap_or(false)
                })
                .count()
        });
        Self {
            row_count: Some(profile.row_count),
            column_count: Some(profile.columns.len()),
            checker_hits: profile.quality.as_ref().map(|q| q.checker_hits),
            checker_applicable: profile.quality.as_ref().map(|q| q.checker_applicable),
            null_rate,
            unique_rate,
            attention_columns,
        }
    }
}

/// 报告主 Tab（原型 .report-tabs：列分布 / 缺失 / 关联性 / 样本）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdaTab {
    #[default]
    Cols,
    Missing,
    Rel,
    Sample,
}

impl EdaTab {
    pub const ALL: [Self; 4] = [Self::Cols, Self::Missing, Self::Rel, Self::Sample];
    pub fn label(self) -> &'static str {
        match self {
            Self::Cols => "列分布",
            Self::Missing => "缺失",
            Self::Rel => "关联性",
            Self::Sample => "样本",
        }
    }
    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap()
    }
    pub(crate) fn of_index(i: usize) -> Self {
        Self::ALL[i.min(3)]
    }
}

/// 列卡展开四区（原型 .cc-zones：基础统计 / 分布 / 检查器 / 样本）。
/// 区状态按列存（`col_detail_zone`），不共用全局区，避免切列串台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColZone {
    #[default]
    Stats,
    Dist,
    Quality,
    Sample,
    /// 自定义规则 Tab（2026-11-06 YData 密度对齐）：适用该列的规则摘要 + SQL 入口
    Rules,
}

impl ColZone {
    pub const ALL: [Self; 5] = [
        Self::Stats,
        Self::Dist,
        Self::Quality,
        Self::Sample,
        Self::Rules,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Stats => "基础统计",
            Self::Dist => "分布",
            Self::Quality => "检查器",
            Self::Sample => "样本",
            Self::Rules => "规则",
        }
    }
    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|z| *z == self).unwrap()
    }
    pub(crate) fn of_index(i: usize) -> Self {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }
}

/// 关联性子 Tab（原型 .rel-tabs：数值相关 / 分类关联 / 值重叠）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RelTab {
    #[default]
    Corr,
    Assoc,
    Overlap,
}

impl RelTab {
    pub const ALL: [Self; 3] = [Self::Corr, Self::Assoc, Self::Overlap];
    pub fn label(self) -> &'static str {
        match self {
            Self::Corr => "数值相关",
            Self::Assoc => "分类关联",
            Self::Overlap => "值重叠",
        }
    }
    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap()
    }
    pub(crate) fn of_index(i: usize) -> Self {
        Self::ALL[i.min(2)]
    }
}
