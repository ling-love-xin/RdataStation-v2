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
    BooleanStats, ColumnInsightFull, ColumnStatsDetail, DateTimeStats, DistributionBin,
    NumericStats, TableProfile, TextStats,
};

use crate::rule_types::{ExecutionResult, QualityReport};

// ==================== 子模块（拆分：视图模型按职责域下沉，纯位移） ====================
mod sets;
mod target;
mod panel;
mod columns;
mod profile;
mod multi;
mod history;
mod column_profile;
mod probe;

// 聚合 re-export：保持 `crate::model::*` 路径与 lib.rs 的 re-export 不变
pub use sets::*;
pub use target::*;
pub use panel::*;
pub use columns::*;
pub use profile::*;
pub use multi::*;
pub use history::*;
pub use column_profile::*;
pub use probe::*;
// ==================== 阈值（原型 §3.1） ====================

/// 空值率超过此值：数值行转 warning 强调
pub const NULL_RATE_WARN: f64 = 0.05;
/// 空值率超过此值：追加「高空值率」质量提示
pub const NULL_RATE_NOTE: f64 = 0.10;
/// |偏度| 超过此值：认为分布明显偏斜
pub const SKEW_NOTABLE: f64 = 1.0;
/// |超额峰度| 超过此值：认为峰态明显（DuckDB `KURTOSIS()` 以 0 为基准）
pub const KURT_NOTABLE: f64 = 1.0;
/// 布尔列某一侧占比超过此值：提示取值不平衡
pub const BOOLEAN_IMBALANCE: f64 = 0.95;
/// Top 值类数达到此值：提示字段更像类别型
pub const TOP_VALUE_CATEGORY_HINT: usize = 5;
/// 数据质量区里极端值最多逐条列出几个（其余折叠为计数）
pub const EXTREME_LIST_LIMIT: usize = 2;
// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::types::{DistributionBin, ExtremeValue, TableColumnMeta, TextFrequency};
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
        assert_eq!(labels, vec!["摘要", "列", "表", "多列", "结构", "历史"]);
        for (i, tab) in PanelTab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), i, "index() 必须与 ALL 顺序一致");
            assert_eq!(PanelTab::from_index(i), Some(*tab));
            assert!(
                tab.label().chars().count() <= 2,
                "标签一律 2 字，否则 280px 右栏内六项溢出"
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

    /// 源目标（导航树 / 结果集 / 存档 / 草稿箱）与临时表目标**同形**：
    /// Tab、标题、下钻都一致，差别只在「多一步取样」。
    #[test]
    fn source_targets_mirror_temp_table_targets() {
        let source = SampleSource::new("G_1", "SELECT * FROM `shop`.`orders`", "shop.orders");
        assert_eq!(source.label(), "shop.orders");
        assert_eq!(source.conn_id(), Some("G_1"));

        let col = InsightTarget::SourceColumn {
            source: source.clone(),
            column: "amount".into(),
            data_type: "DECIMAL".into(),
        };
        assert_eq!(col.default_tab(), PanelTab::Column);
        assert_eq!(col.title(), "amount");
        assert_eq!(
            col.detail().as_deref(),
            Some("DECIMAL · 来源 shop.orders"),
            "副标题要说清样本是从哪取的"
        );
        assert_eq!(col.temp_table(), "", "样本表要取样后才知道");
        assert!(col.source().is_some());

        let table = InsightTarget::SourceTable {
            source,
            table_name: "orders".into(),
        };
        assert_eq!(table.default_tab(), PanelTab::Table);
        assert_eq!(table.title(), "orders");
        assert_eq!(table.table_name(), "orders");
        assert_eq!(table.detail().as_deref(), Some("来源 shop.orders"));
    }

    /// 文件来源（CSV / Parquet / Excel / JSON）：**不走源库连接**，靠 DuckDB 扩展直接读。
    #[test]
    fn file_source_picks_the_duckdb_reader_by_extension() {
        let xlsx =
            SampleSource::duckdb_file("C:/data/2026 预算.xlsx", "预算表").expect("xlsx 应能识别");
        assert_eq!(xlsx.conn_id(), None, "文件不需要源库连接");
        assert_eq!(xlsx.label(), "预算表");
        assert!(
            xlsx.sql
                .contains("read_excel_auto('C:/data/2026 预算.xlsx')"),
            "Excel 靠 excel 扩展读: {}",
            xlsx.sql
        );

        let csv = SampleSource::duckdb_file("/tmp/a.csv", "a").expect("csv");
        assert!(csv.sql.contains("read_csv_auto"), "{}", csv.sql);
        let pq = SampleSource::duckdb_file("/tmp/a.parquet", "a").expect("parquet");
        assert!(pq.sql.contains("read_parquet"), "{}", pq.sql);

        // 路径里的单引号要转义（否则拼出来的 SQL 直接断在路径上）
        let tricky = SampleSource::duckdb_file("/tmp/it's.csv", "a").expect("带引号的路径");
        assert!(tricky.sql.contains("'/tmp/it''s.csv'"), "{}", tricky.sql);

        // 不认识的格式：报「不支持」，不猜一个读取器
        let err = SampleSource::duckdb_file("/tmp/a.docx", "a").expect_err("docx 不该被猜");
        assert!(err.to_string().contains("还不能分析"), "{err}");
    }

    /// 面板「当前该分析哪张临时表」：源目标读解析出的样本表，其余读目标自带的。
    #[test]
    fn sample_table_resolves_source_and_plain_targets() {
        let mut data = PanelData::default();

        let plain = InsightTarget::Column {
            temp_table: "tmp_q_x".into(),
            column: "amount".into(),
            data_type: "DOUBLE".into(),
        };
        assert_eq!(data.sample_table(&plain), Some("tmp_q_x"));
        assert_eq!(
            data.sample_table(&InsightTarget::Schema {
                conn_id: "G_1".into(),
                database: "shop".into(),
                schema: None,
            }),
            None,
            "结构目标没有临时表"
        );

        let source = InsightTarget::SourceColumn {
            source: SampleSource::new("G_1", "SELECT 1", "shop.orders"),
            column: "amount".into(),
            data_type: "DOUBLE".into(),
        };
        assert_eq!(
            data.sample_table(&source),
            None,
            "还没取样时没有可分析的临时表"
        );
        data.source_sample = Some("tmp_i_sample_1".into());
        assert_eq!(data.sample_table(&source), Some("tmp_i_sample_1"));
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
                "总行数",
                "非空值",
                "空值",
                "唯一值",
                "平均",
                "中位",
                "最小",
                "最大",
                "P25",
                "P75",
                "标准差",
                "偏度"
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
        let view =
            ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(numeric(
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
            ))));
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
        assert!(
            view.basics
                .iter()
                .any(|r| r.label == "跨度" && r.value == "90 天")
        );
        assert_eq!(view.distribution.len(), 1);
    }

    #[test]
    fn boolean_imbalance_is_flagged_both_ways() {
        let balanced =
            ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Boolean(BooleanStats {
                true_count: 500,
                false_count: 500,
                true_ratio: 0.5,
            })));
        assert!(balanced.notes.iter().all(|n| !n.text.contains("不平衡")));
        assert_eq!(balanced.distribution[0].label, "True 占比");

        let lopsided =
            ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Boolean(BooleanStats {
                true_count: 990,
                false_count: 10,
                true_ratio: 0.99,
            })));
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
        assert_eq!(
            ColumnKind::of_type_name("DECIMAL(12,2)"),
            ColumnKind::Numeric
        );
        assert_eq!(ColumnKind::of_type_name("BIGINT"), ColumnKind::Numeric);
        assert_eq!(
            ColumnKind::of_type_name("TIMESTAMP_NS"),
            ColumnKind::DateTime
        );
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
    fn table_view_lists_columns_with_families_and_no_fake_stats() {
        let view = TableProfileView::from_profile(&table_profile_fixture(), "orders");
        assert_eq!(view.table_name, "orders");
        assert_eq!(view.row_count, 124_000);
        assert_eq!(view.columns.len(), 3);
        assert_eq!(view.columns[0].kind, ColumnKind::Numeric);
        assert!(view.columns[0].primary_key);
        assert!(!view.columns[0].nullable);
        assert_eq!(
            view.columns[2].kind,
            ColumnKind::Unknown,
            "BLOB 不参与列统计"
        );
        // 只探查元数据：列统计 / 表级事实摘要 / 进度都不预先编造
        assert!(view.columns.iter().all(|c| c.null_rate.is_none()));
        assert!(view.quality.is_none());
        assert!(view.progress.is_none());
    }

    #[test]
    fn evaluation_progress_is_monotonic_and_column_details_persist() {
        let base = TableProfileView::from_profile(&table_profile_fixture(), "orders");
        let started = base.evaluating(0, 3);
        assert_eq!(
            started.progress,
            Some(TableEvalProgress { done: 0, total: 3 })
        );
        assert_eq!(started.progress.unwrap().ratio(), 0.0);

        let full = base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        let one = started.with_column_detail("id", &full);
        let two = one.with_column_detail("amount", &full).evaluating(2, 3);
        assert!(
            two.columns[0].stats_detail.is_some(),
            "先回填的明细不得被覆盖"
        );
        assert!(two.columns[1].stats_detail.is_some());
        assert!(two.columns[2].stats_detail.is_none(), "未回填列保持空");
        assert_eq!(two.progress.unwrap().ratio(), 2.0 / 3.0);

        // 未知列名不造行也不 panic（列清单可能刚变）
        assert_eq!(two.with_column_detail("不存在", &full).columns.len(), 3);
    }

    #[test]
    fn finish_evaluation_clears_progress_and_checker_fills_facts() {
        let base = TableProfileView::from_profile(&table_profile_fixture(), "orders");

        let done = base.evaluating(2, 2).finish_evaluation();
        assert!(done.progress.is_none(), "评估完进度行必须消失");
        let view = done.quality.as_ref().expect("应有表级事实摘要位");
        assert_eq!(view.checker.len(), 0, "检查器由 with_checker 单独填充");
        assert_eq!(view.checker_applicable, 0);
        assert_eq!(view.checker_hits, 0);

        let filled = done.with_checker(vec![
            ColumnChecker {
                column: "id".into(),
                name: "非空率".into(),
                state: ColumnCheckState::Hit,
                detail: "非空率 98.2% ≥ 90%".into(),
            },
            ColumnChecker {
                column: "amount".into(),
                name: "近似正态".into(),
                state: ColumnCheckState::Miss,
                detail: "偏度 2.1（高度右偏）".into(),
            },
            ColumnChecker {
                column: "payload".into(),
                name: "唯一率".into(),
                state: ColumnCheckState::Excluded,
                detail: "列被规则排除".into(),
            },
        ]);
        let view = filled.quality.expect("检查器回填后摘要仍在");
        assert_eq!(view.checker.len(), 3);
        assert_eq!(view.checker_applicable, 2, "不适用 / 排除不计入适用数");
        assert_eq!(view.checker_hits, 1, "命中只统计通过的条数");
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
        let map: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.label.as_str(), r.value.as_str()))
            .collect();
        assert!(
            map.contains(&("相关系数", "0.8765")),
            "已知字段名给中文标签：{map:?}"
        );
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
        assert!(
            quality_notes(Some(&passed)).is_empty(),
            "通过的检查不占提示行"
        );

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
        assert_eq!(
            notes[1].text, "sample_size 未过 min=30（实际 12）",
            "没文案就拼一条可定位的"
        );
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
        assert!(
            !view.rules[0].accepts(&view.kinds_of(&["id".into()])),
            "只选一列不行"
        );

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
    fn non_empty_column_fills_fact_basics_and_sample() {
        let view = ColumnProfileView::from_domain(&base_stats(ColumnStatsDetail::Numeric(
            numeric(None, Vec::new()),
        )));
        // 只承载事实统计：基础行非空、样本来自真实取值，不合成评分结论
        assert_eq!(view.total_count, 1000);
        assert!(!view.basics.is_empty());
        assert!(!view.sample.is_empty());
        let labels: Vec<&str> = view.basics.iter().map(|r| r.label).collect();
        assert!(labels.contains(&"总行数"));
        assert!(labels.contains(&"平均"));
    }

    #[test]
    fn empty_column_still_reports_zero_counts_without_fake_conclusion() {
        // 全空列：不编质量结论；计数与空值率仍然照给（基础统计不依赖有无数据）
        let mut full = base_stats(ColumnStatsDetail::Numeric(numeric(None, Vec::new())));
        full.stats.total_count = 0;
        full.stats.null_count = 0;
        full.stats.null_rate = 0.0;
        full.stats.unique_count = None;
        full.sample.clear();
        let view = ColumnProfileView::from_domain(&full);
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
        assert!(!view.entries[0].chain_truncated, "不是最旧一版，不标");
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

        // 对比只列事实行，不合成质量分；首行即总行数
        assert_eq!(diff.rows[0].label, "总行数");

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
        assert_eq!(
            (kind.old.as_str(), kind.new.as_str()),
            ("DECIMAL(12,2)", "VARCHAR")
        );

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
        assert_eq!(
            row_of(&diff, "非空值").delta,
            DeltaView::Up("+1,010".into())
        );
    }
}
