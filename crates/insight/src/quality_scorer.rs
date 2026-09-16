use crate::model::types::{
    ColumnInsightFull, ColumnQualityEntry, ColumnStatsDetail, QualityDimension, QualityScore,
    TableQuality,
};

/// Dimension weights for quality scoring (must sum to 1.0).
pub const WEIGHT_COMPLETENESS: f64 = 0.35;
pub const WEIGHT_UNIQUENESS: f64 = 0.25;
pub const WEIGHT_TYPE_CONSISTENCY: f64 = 0.20;
pub const WEIGHT_DISTRIBUTION: f64 = 0.20;

/// Quality grade thresholds (overall_score >= threshold → level).
pub const GRADE_EXCELLENT: f64 = 85.0;
pub const GRADE_GOOD: f64 = 70.0;
pub const GRADE_FAIR: f64 = 50.0;
pub const GRADE_POOR: f64 = 30.0;

/// 质量等级（总分 → 等级）。
///
/// 阈值与文案收在这一处：先前列级与表级各写了一份判断（表级还直接写死 85/70/50/30），
/// 视图再各写一份取色映射就会三处漂移。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    /// ≥ 85
    Excellent,
    /// ≥ 70
    Good,
    /// ≥ 50
    Fair,
    /// ≥ 30
    Poor,
    /// < 30
    Bad,
}

impl Grade {
    pub fn of(score: f64) -> Self {
        if score >= GRADE_EXCELLENT {
            Grade::Excellent
        } else if score >= GRADE_GOOD {
            Grade::Good
        } else if score >= GRADE_FAIR {
            Grade::Fair
        } else if score >= GRADE_POOR {
            Grade::Poor
        } else {
            Grade::Bad
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Grade::Excellent => "优秀",
            Grade::Good => "良好",
            Grade::Fair => "一般",
            Grade::Poor => "较差",
            Grade::Bad => "差",
        }
    }
}

/// Computes a quality score for a single column based on four dimensions:
/// completeness (null-rate), uniqueness (distinct-ratio), type consistency,
/// and value distribution (histogram uniformity).
///
/// Returns a [QualityScore] with overall score (0–100), grade level, and
/// per-dimension breakdown with detail strings.
pub fn compute_column_quality(stats: &ColumnInsightFull) -> QualityScore {
    let null_rate = stats.stats.null_rate;
    let total = stats.stats.total_count as f64;
    let unique = stats.stats.unique_count.unwrap_or(0) as f64;
    let non_null = total * (1.0 - null_rate);

    let completeness = if total > 0.0 {
        (1.0 - null_rate) * 100.0
    } else {
        0.0
    };

    let uniqueness = if non_null > 0.0 {
        let ratio = unique / non_null;
        if ratio > 0.9 {
            100.0
        } else if ratio > 0.5 {
            80.0
        } else if ratio > 0.2 {
            60.0
        } else if ratio > 0.05 {
            40.0
        } else if ratio > 0.01 {
            20.0
        } else {
            10.0
        }
    } else {
        0.0
    };

    let type_consistency = match stats.stats.stats_detail {
        ColumnStatsDetail::Numeric(_) => {
            if null_rate > 0.5 {
                40.0
            } else {
                90.0
            }
        }
        ColumnStatsDetail::Text(_) => {
            if unique < 2.0 {
                30.0
            } else if null_rate > 0.6 {
                40.0
            } else {
                75.0
            }
        }
        ColumnStatsDetail::DateTime(_) => {
            let has_range = stats.histogram.as_ref().is_some_and(|h| h.len() > 1);
            if has_range {
                85.0
            } else {
                60.0
            }
        }
        ColumnStatsDetail::Boolean(_) => 95.0,
        ColumnStatsDetail::Unknown => 50.0,
    };

    fn detail_variant_name(detail: &ColumnStatsDetail) -> &str {
        match detail {
            ColumnStatsDetail::Numeric(_) => "Numeric",
            ColumnStatsDetail::Text(_) => "Text",
            ColumnStatsDetail::DateTime(_) => "DateTime",
            ColumnStatsDetail::Boolean(_) => "Boolean",
            ColumnStatsDetail::Unknown => "Unknown",
        }
    }

    let distribution = if let Some(ref hist) = stats.histogram {
        let bins = hist.len() as f64;
        if bins > 0.0 {
            let values: Vec<f64> = hist.iter().map(|b| b.count as f64).collect();
            let sum: f64 = values.iter().sum();
            if sum > 0.0 {
                let avg = sum / bins;
                let variance: f64 = values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / bins;
                let cv = variance.sqrt() / avg.max(1.0);
                if cv < 0.3 {
                    90.0
                } else if cv < 0.7 {
                    75.0
                } else if cv < 1.5 {
                    50.0
                } else {
                    30.0
                }
            } else {
                50.0
            }
        } else {
            50.0
        }
    } else {
        50.0
    };

    let unique_display = stats.stats.unique_count.unwrap_or(0);

    let dimensions = vec![
        QualityDimension {
            name: "完整性".into(),
            score: completeness,
            weight: WEIGHT_COMPLETENESS,
            detail: format!("空值率 {:.1}%", null_rate * 100.0),
        },
        QualityDimension {
            name: "唯一性".into(),
            score: uniqueness,
            weight: WEIGHT_UNIQUENESS,
            detail: format!("去重 {}/{}", unique_display, stats.stats.total_count),
        },
        QualityDimension {
            name: "类型一致".into(),
            score: type_consistency,
            weight: WEIGHT_TYPE_CONSISTENCY,
            detail: detail_variant_name(&stats.stats.stats_detail).into(),
        },
        QualityDimension {
            name: "分布均匀".into(),
            score: distribution,
            weight: WEIGHT_DISTRIBUTION,
            detail: "直方图分布评估".into(),
        },
    ];

    let overall: f64 = dimensions.iter().map(|d| d.score * d.weight).sum();

    let grade = Grade::of(overall);

    let summary = match grade {
        Grade::Excellent => format!("数据质量优秀 ({:.0}分)，可直接用于分析", overall),
        Grade::Good => format!("数据质量良好 ({:.0}分)，建议关注空值", overall),
        Grade::Fair => format!("数据质量一般 ({:.0}分)，存在明显质量问题", overall),
        Grade::Poor | Grade::Bad => {
            format!("数据质量较差 ({:.0}分)，建议清洗后使用", overall)
        }
    };

    QualityScore {
        column_name: stats.stats.column_name.clone(),
        overall_score: overall,
        level: grade.label().into(),
        dimensions,
        summary,
    }
}

/// Computes aggregate quality for a table by scoring each column and
/// producing a weighted average. Columns are sorted worst→best for
/// quick identification of data problems. Unscoreable columns are skipped.
pub fn compute_table_quality(
    table_name: &str,
    stats_list: &[ColumnInsightFull],
) -> TableQuality {
    let mut entries: Vec<ColumnQualityEntry> = stats_list
        .iter()
        .map(|s| {
            let qs = compute_column_quality(s);
            ColumnQualityEntry {
                column_name: s.stats.column_name.clone(),
                quality_score: qs.overall_score,
                level: qs.level,
                null_rate: s.stats.null_rate,
            }
        })
        .collect();

    entries.sort_by(|a, b| {
        a.quality_score
            .partial_cmp(&b.quality_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let scored_count = entries.len();
    let total_columns = scored_count;
    let overall = if scored_count > 0 {
        entries.iter().map(|e| e.quality_score).sum::<f64>() / scored_count as f64
    } else {
        0.0
    };

    // 表级与列级共用同一套阈值与文案（先前此处写死了 85/70/50/30）
    let grade = Grade::of(overall);
    let level = if scored_count == 0 {
        "无数据"
    } else {
        grade.label()
    };

    let problem_columns = entries.iter().filter(|e| e.quality_score < 50.0).count();
    let summary = if scored_count == 0 {
        "无数据".into()
    } else if overall >= 85.0 {
        format!("表质量优秀 ({:.0}分)，{} 列均健康", overall, scored_count)
    } else if problem_columns > 0 {
        format!(
            "表质量{} ({:.0}分)，{} 列需关注 ({}风险列)",
            level, overall, scored_count, problem_columns
        )
    } else {
        format!(
            "表质量{} ({:.0}分)，{} 列已评估",
            level, overall, scored_count
        )
    };

    TableQuality {
        table_name: table_name.into(),
        overall_score: overall,
        level: level.into(),
        column_scores: entries,
        summary,
        scored_count: scored_count as u32,
        total_columns: total_columns as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{
        BooleanStats, ColumnInsightFull, ColumnStats, ColumnStatsDetail, DistributionBin,
        NumericStats, TextStats,
    };

    fn make_insight(null_rate: f64, unique_ratio: f64) -> ColumnInsightFull {
        let total = 100.0;
        ColumnInsightFull {
            stats: ColumnStats {
                column_name: "test_col".into(),
                data_type: "DOUBLE".into(),
                total_count: total as u32,
                null_count: (total * null_rate) as u32,
                null_rate,
                unique_count: Some((total * unique_ratio) as u32),
                stats_detail: ColumnStatsDetail::Numeric(NumericStats {
                    min: 1.0,
                    max: 100.0,
                    avg: 50.0,
                    median: 50.5,
                    p25: 25.0,
                    p75: 75.0,
                    sum: 5000.0,
                    stddev: Some(28.0),
                    skewness: None,
                    kurtosis: None,
                    is_extreme: vec![],
                }),
            },
            sample: vec![],
            histogram: Some(vec![
                DistributionBin {
                    label: "a".into(),
                    count: 50,
                    ratio: 0.5,
                },
                DistributionBin {
                    label: "b".into(),
                    count: 50,
                    ratio: 0.5,
                },
            ]),
        }
    }

    #[test]
    fn test_column_quality_perfect() {
        let qi = make_insight(0.0, 1.0);
        let qs = compute_column_quality(&qi);
        assert!(qs.overall_score > 85.0, "perfect data = excellent score");
        assert_eq!(qs.level, "优秀");
    }

    #[test]
    fn test_column_quality_abysmal() {
        let qi = make_insight(0.9, 0.01);
        let qs = compute_column_quality(&qi);
        assert!(
            qs.overall_score < 40.0,
            "90% null + 1% unique = very poor, got {}",
            qs.overall_score
        );
        assert_eq!(qs.dimensions.len(), 4);
    }

    #[test]
    fn test_column_quality_text_type() {
        let qi = ColumnInsightFull {
            stats: ColumnStats {
                column_name: "name".into(),
                data_type: "VARCHAR".into(),
                total_count: 100,
                null_count: 5,
                null_rate: 0.05,
                unique_count: Some(80),
                stats_detail: ColumnStatsDetail::Text(TextStats {
                    min_length: 1,
                    max_length: 20,
                    top_values: vec![],
                }),
            },
            sample: vec![],
            histogram: None,
        };
        let qs = compute_column_quality(&qi);
        assert!(qs.overall_score > 50.0);
    }

    #[test]
    fn test_column_quality_boolean_type() {
        let qi = ColumnInsightFull {
            stats: ColumnStats {
                column_name: "active".into(),
                data_type: "BOOLEAN".into(),
                total_count: 100,
                null_count: 0,
                null_rate: 0.0,
                unique_count: Some(2),
                stats_detail: ColumnStatsDetail::Boolean(BooleanStats {
                    true_count: 80,
                    false_count: 20,
                    true_ratio: 0.8,
                }),
            },
            sample: vec![],
            histogram: None,
        };
        let qs = compute_column_quality(&qi);
        assert!(
            qs.overall_score > 65.0,
            "boolean quality got {}",
            qs.overall_score
        );
    }

    #[test]
    fn test_table_quality_sorted_worst_first() {
        let stats_list = vec![
            make_insight(0.01, 0.98),
            make_insight(0.5, 0.1),
            make_insight(0.1, 0.8),
        ];
        let tq = compute_table_quality("sorted_table", &stats_list);
        assert_eq!(tq.column_scores.len(), 3);
        assert!(
            tq.column_scores[0].quality_score <= tq.column_scores[1].quality_score,
            "column scores must be sorted ascending (worst first)"
        );
        assert!(tq.column_scores[1].quality_score <= tq.column_scores[2].quality_score);
    }

    #[test]
    fn test_table_quality_empty() {
        let tq = compute_table_quality("empty", &[]);
        assert_eq!(tq.overall_score, 0.0);
        assert_eq!(tq.level, "无数据");
        assert_eq!(tq.scored_count, 0);
    }

    /// 等级边界：阈值端点**含在下档**（`>=`），差一个 epsilon 立刻降级。
    ///
    /// 边界写死在这里而不是引用常量，是为了让「改阈值」必须来这里同步——
    /// 否则测试跟着实现一起漂移就失去意义。
    #[test]
    fn grade_thresholds_are_inclusive_on_the_boundary() {
        assert_eq!(Grade::of(100.0), Grade::Excellent);
        assert_eq!(Grade::of(85.0), Grade::Excellent);
        assert_eq!(Grade::of(84.99), Grade::Good);
        assert_eq!(Grade::of(70.0), Grade::Good);
        assert_eq!(Grade::of(69.99), Grade::Fair);
        assert_eq!(Grade::of(50.0), Grade::Fair);
        assert_eq!(Grade::of(49.99), Grade::Poor);
        assert_eq!(Grade::of(30.0), Grade::Poor);
        assert_eq!(Grade::of(29.99), Grade::Bad);
        assert_eq!(Grade::of(0.0), Grade::Bad);
    }

    #[test]
    fn grade_labels_are_distinct() {
        let labels: Vec<&str> = [
            Grade::Excellent,
            Grade::Good,
            Grade::Fair,
            Grade::Poor,
            Grade::Bad,
        ]
        .iter()
        .map(|g| g.label())
        .collect();
        assert_eq!(labels, vec!["优秀", "良好", "一般", "较差", "差"]);
    }

    /// 领域结果里的 `level` 字符串必须等于 `Grade::of(总分).label()`：
    /// 视图按分数现算等级（不去解析字符串），两者一旦漂移就会出现「88 分 · 良好」。
    #[test]
    fn level_string_agrees_with_grade_of_score() {
        for (null_rate, unique_ratio) in [(0.0, 1.0), (0.05, 0.6), (0.4, 0.3), (0.9, 0.01)] {
            let qs = compute_column_quality(&make_insight(null_rate, unique_ratio));
            assert_eq!(qs.level, Grade::of(qs.overall_score).label());
        }
        let tq = compute_table_quality("t", &[make_insight(0.1, 0.5), make_insight(0.6, 0.05)]);
        assert_eq!(tq.level, Grade::of(tq.overall_score).label());
    }

    /// 四维权重合计 1.0：否则总分不再是 0–100 标度，等级阈值也跟着失去意义。
    #[test]
    fn dimension_weights_sum_to_one() {
        let sum = WEIGHT_COMPLETENESS
            + WEIGHT_UNIQUENESS
            + WEIGHT_TYPE_CONSISTENCY
            + WEIGHT_DISTRIBUTION;
        assert!((sum - 1.0).abs() < 1e-9, "权重合计应为 1.0，实际 {sum}");
    }

    #[test]
    fn test_dimensions_includes_four() {
        let qi = make_insight(0.02, 0.95);
        let qs = compute_column_quality(&qi);
        assert_eq!(qs.dimensions.len(), 4);
        let names: Vec<&str> = qs.dimensions.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"完整性"));
        assert!(names.contains(&"唯一性"));
        assert!(names.contains(&"类型一致"));
        assert!(names.contains(&"分布均匀"));
    }
}
