//! 洞察模型 · 列画像视图模型（自 model.rs 拆分，纯位移）

use super::*;
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
}

impl ColumnProfileView {
    /// 从领域结果构建视图模型。
    ///
    /// 纯函数：不读 I/O、不碰主题、不依赖窗口——阈值与文案因此可单测。
    /// 本视图只承载事实统计（基础 / 分布 / 检查器提示 / 样本），
    /// 不合成质量总分或等级（评级归归档后的资产库）。
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
            value: format!(
                "{}（{}）",
                fmt_int(stats.null_count),
                fmt_pct(stats.null_rate)
            ),
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

        view
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
        // 偏度明显就提示方向（峰度缺失也要能看出偏斜，不丢信息）
        if skew.abs() > SKEW_NOTABLE {
            view.notes.push(QualityNote {
                level: NoteLevel::Info,
                text: format!("分布{}（偏度 {}）", skew_direction(skew), fmt_num(skew)),
            });
        }
    }
    // 峰度 + 偏峰联合判定（科研级描述，只报 DuckDB 原生统计量——DuckDB
    // `KURTOSIS()` 返回**超额峰度**（正态分布为 0），判定阈值是经验法则，
    // 不做分布检验 p 值：p 值需要推断库（statrs），本轮明确不做）。
    if let Some(kurt) = n.kurtosis {
        view.basics.push(StatRow {
            label: "峰度",
            value: format!("{}（{}）", fmt_num(kurt), kurt_direction(kurt)),
            emphasis: Emphasis::Normal,
        });
    }
    if let (Some(skew), Some(kurt)) = (n.skewness, n.kurtosis) {
        if let Some(judge) = normality_judge(skew, kurt) {
            view.notes.push(QualityNote {
                level: NoteLevel::Info,
                text: format!(
                    "分布{}（偏度 {} · 峰度 {}）",
                    judge,
                    fmt_num(skew),
                    fmt_num(kurt)
                ),
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

/// 超额峰度方向（DuckDB `KURTOSIS()` 基准：正态分布为 0；阈值是经验法则）
pub fn kurt_direction(kurt: f64) -> &'static str {
    if kurt > KURT_NOTABLE {
        "尖峰"
    } else if kurt < -KURT_NOTABLE {
        "平峰"
    } else {
        "近似正态峰度"
    }
}

/// 偏峰联合判定 → 探索价值提示文案（`None` = 无需额外提示）。
///
/// 只报 DuckDB 原生统计量（偏度 / 超额峰度），阈值是经验法则而非分布检验：
/// 分布检验 p 值需要推断库，本轮明确不做（用户口径：只处理 DuckDB 可提供的统计量）。
/// 偏度明显时 [`fill_numeric`] 已按方向单独提示，这里**只补偏度正常但峰度明显**
/// 的缺失信息，避免同一条分布出两条重复提示。
pub fn normality_judge(skew: f64, kurt: f64) -> Option<&'static str> {
    if skew.abs() <= SKEW_NOTABLE && kurt.abs() > KURT_NOTABLE {
        if kurt > 0.0 {
            Some("偏离正态（峰度尖峰）")
        } else {
            Some("偏离正态（峰度平缓）")
        }
    } else {
        None
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