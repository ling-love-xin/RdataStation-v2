//! 洞察分析引擎（薄包装，实际实现已拆分到 `insight_engine_internal/` 子模块）
//!
//! 此文件保留为兼容层，所有公开 API 从 `crate::insight_engine_internal` 模块重新导出。
//! 新代码应直接从 `crate::insight_engine_internal::*` 引用。

// 列统计
pub use crate::insight_engine_internal::column::{
    ColumnBasics, get_column_insight_full, get_column_insight_full_on,
    get_column_insight_full_on_with_basics, get_column_insight_full_with_basics,
    get_column_insights, get_column_extremes_internal, get_column_sample_internal,
    get_column_stats_internal, get_column_stats_with_basics,
};
// 表画像
pub use crate::insight_engine_internal::profile::{
    compute_dup_rows, compute_dup_rows_on, compute_table_column_summary,
    compute_table_column_summary_on, get_temp_table_profile, get_temp_table_profile_on,
};
// 取样与迷你统计
pub use crate::insight_engine_internal::sample::{
    TableEdaBundle, compute_sample_mini_stats, compute_sample_mini_stats_on,
    get_table_eda_bundle, get_temp_table_sample, get_temp_table_sample_on,
};
// 极值与直方图
pub use crate::insight_engine_internal::extremes::detect_extremes;
pub use crate::insight_engine_internal::histogram::{get_column_histogram_internal, get_column_histogram_with_non_null};
// 列重叠
pub use crate::insight_engine_internal::overlap::compute_column_overlap;
// 规则执行
pub use crate::insight_engine_internal::rules::{execute_insight_rule, list_insight_rules, list_rules_for_column};
// 公共函数
pub use crate::insight_engine_internal::get_or_create_duckdb;

// 重新导出公共常量
pub use crate::insight_engine_internal::{DEFAULT_SAMPLE_SIZE, ERR_TOO_MANY_CONCURRENT, HISTOGRAM_MIN_ROWS, SAMPLE_MINI_LIMIT};
