//! 洞察模型 · 探查配置（确认窗口产物）。
//!
//! 探查配置贯穿整条链：**取数建表**（数据量形态 + 采样方式 + 过滤 + 种子）→
//! **自动评估**（探查列集合）→ **取样档**（查询级过滤的行/字段子集）。
//! 语义对齐 `docs/architecture/insight/insight-probe-config-decision.md`（C1–C7 / R 系列 / Q1–Q3）。

/// 采样方式：行数 vs 百分比（UI 必须说明差异，R2）。
///
/// - [`Head`](ProbeSampleMode::Head)：前 N 行（`LIMIT`），确定、可预期；
/// - [`Reservoir`](ProbeSampleMode::Reservoir)：随机采样（DuckDB `TABLESAMPLE` reservoir），
///   均匀、有代表性，配 [`ProbeConfig::seed`] 可复现。
///
/// 源库连接（MySQL / PG）没有 `TABLESAMPLE reservoir`：百分比在那里退化为
/// 「`COUNT(*)` 估算行数 → `LIMIT N'`」的近似比例取样（见
/// `service::persistence::build_source_sample_sql`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum ProbeSampleMode {
    Head,
    Reservoir,
}

/// 数据量形态（确认窗口「数据量」块，C1）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum ProbeAmount {
    /// 全量：不带 `LIMIT`（建表即评估基数 = 源数据全集）
    Full,
    /// 前 N 行
    RowCount(u32),
    /// 百分比（0.0..=100.0，reservoir 采样）
    Percent(f64),
}

/// 探查列集合（确认窗口「列」块，C1 / C4）。
///
/// [`Subset`](ProbeColumns::Subset) 之外的列在 EDA 上**不存在**（不渲染，非灰态）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum ProbeColumns {
    /// 全部列
    All,
    /// 子集：只探查这些列
    Subset(Vec<String>),
}

/// 探查配置：一次确认窗口的完整产物。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProbeConfig {
    /// 数据量形态
    pub amount: ProbeAmount,
    /// 采样方式
    pub sample_mode: ProbeSampleMode,
    /// 探查列集合
    pub columns: ProbeColumns,
    /// 可选过滤条件（`WHERE` 子句主体，不带 `WHERE` 关键字；`None`/空串 = 不过滤，R1）
    pub filter: Option<String>,
    /// reservoir 采样种子（`Percent` + `Reservoir` 时生效，可复现）
    pub seed: Option<u64>,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self::sample_default()
    }
}

impl ProbeConfig {
    /// 默认取样：前 500 行、全部列、不过滤（R8 默认值）。
    pub fn sample_default() -> Self {
        Self {
            amount: ProbeAmount::RowCount(500),
            sample_mode: ProbeSampleMode::Head,
            columns: ProbeColumns::All,
            filter: None,
            seed: None,
        }
    }

    /// 全量口径：不带 `LIMIT`、全部列（对应原 `full = true` 语义）。
    pub fn full() -> Self {
        Self {
            amount: ProbeAmount::Full,
            sample_mode: ProbeSampleMode::Head,
            columns: ProbeColumns::All,
            filter: None,
            seed: None,
        }
    }

    /// 是否全量（不带 `LIMIT`）。
    pub fn is_full(&self) -> bool {
        matches!(self.amount, ProbeAmount::Full)
    }

    /// 探查列集合里的列名（`All` 时为 `None`，表示「全部列」）。
    pub fn column_subset(&self) -> Option<&[String]> {
        match &self.columns {
            ProbeColumns::All => None,
            ProbeColumns::Subset(cols) => Some(cols),
        }
    }

    /// 过滤条件是否有效（非空）。
    pub fn has_filter(&self) -> bool {
        self.filter.as_deref().is_some_and(|f| !f.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_head_500_rows_all_columns() {
        let cfg = ProbeConfig::sample_default();
        assert_eq!(cfg.amount, ProbeAmount::RowCount(500));
        assert_eq!(cfg.sample_mode, ProbeSampleMode::Head);
        assert_eq!(cfg.columns, ProbeColumns::All);
        assert!(!cfg.is_full());
        assert_eq!(ProbeConfig::default(), cfg, "Default 与默认取样同义");
    }

    #[test]
    fn full_has_no_limit() {
        assert!(ProbeConfig::full().is_full());
        assert_eq!(ProbeConfig::full().amount, ProbeAmount::Full);
    }

    #[test]
    fn column_subset_and_filter_helpers() {
        let all = ProbeConfig::sample_default();
        assert!(all.column_subset().is_none());
        assert!(!all.has_filter());

        let subset = ProbeConfig {
            columns: ProbeColumns::Subset(vec!["a".into(), "b".into()]),
            ..all.clone()
        };
        assert_eq!(subset.column_subset(), Some(&["a".to_string(), "b".to_string()][..]));

        let filtered = ProbeConfig {
            filter: Some("  amount > 10  ".into()),
            ..all
        };
        assert!(filtered.has_filter());
        let blank = ProbeConfig {
            filter: Some("   ".into()),
            ..ProbeConfig::sample_default()
        };
        assert!(!blank.has_filter(), "空白过滤条件视为不过滤");
    }
}
