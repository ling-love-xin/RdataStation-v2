//! 洞察服务层（M8）：服务门面与编排，不实现统计算法。
//!
//! # 分层
//!
//! | 内容 | 位置 | 说明 |
//! | --- | --- | --- |
//! | 统计算法 | `crate::insight_engine` / `quality_scorer` / `schema_analyzer` / `table_profile_service` | 纯计算，只吃显式连接与规则集 |
//! | **服务门面** | 本文件 [`InsightService`] | 把「取规则集 + 调用算法」合成一步，供视图层直接使用 |
//! | 规则索引同步 | [`indexer`] | 扫描磁盘 → 合并 → 写库 → 应用启停 |
//! | 快照持久化编排 | [`persistence`] | 快照保存 / 历史 / 清理 / 存储统计 / 表级评估 |
//! | 目录监听 | [`watcher`] | 内容哈希轮询触发重载 |
//!
//! # 门面为什么存在
//!
//! 基础统计本身由 TOML 规则驱动，因此每次分析都要先拿到**当前项目的规则集**
//! （[`crate::with_rules`]）。若把这步留给调用方，每个视图入口都要重复
//! 「取缓存 → 加读锁 → 传引用」三行样板，且容易漏传 `project_root` 而用错项目的规则。
//! 门面把这一步收在一处。
//!
//! 归属变更（M8 Phase 0 / 0.2）：本门面的洞察方法自
//! `workbench/src/services/result_service.rs` 迁入；结果集相关方法（执行 / 导出 /
//! 单元格回写）留在 workbench——它们不属于洞察。

pub mod indexer;
pub mod persistence;
pub mod watcher;

use std::collections::HashMap;
use std::path::Path;

use shared::error::CoreError;

use crate::model::types::{ColumnInsightFull, ColumnStats, QualityScore, TableProfile, TableQuality};
use crate::{with_rules, ExecutionResult};

pub use persistence::{
    batch_evaluate_columns, cleanup_old_insight_snapshots, get_column_insight_history,
    get_insight_storage_stats, get_insight_version_detail, profile_column_from_table,
    save_column_insight_snapshot,
};

/// 洞察服务门面。
pub struct InsightService;

impl InsightService {
    // ==================== 列画像 ====================

    /// 列画像全量结果（统计 + 样本 + 数值列直方图）。`project_root` 决定使用哪一层规则。
    pub fn get_column_insight_full(
        project_root: Option<&Path>,
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnInsightFull, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::get_column_insight_full(registry, temp_table, column_name)
        })
    }

    /// 列基础统计（不含样本与直方图），更轻。
    pub fn get_column_insights(
        project_root: Option<&Path>,
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnStats, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::get_column_insights(registry, temp_table, column_name)
        })
    }

    // ==================== 质量评分 ====================

    pub fn compute_column_quality(stats: &ColumnInsightFull) -> QualityScore {
        crate::quality_scorer::compute_column_quality(stats)
    }

    pub fn compute_table_quality(table_name: &str, stats_list: &[ColumnInsightFull]) -> TableQuality {
        crate::quality_scorer::compute_table_quality(table_name, stats_list)
    }

    // ==================== 表探查 ====================

    /// 表画像（列元数据 + 行数）。走源库内省，不依赖规则集。
    pub async fn get_table_profile(
        conn_id: String,
        db_type: String,
        database: &str,
        schema: &str,
        table: &str,
    ) -> Result<TableProfile, CoreError> {
        crate::table_profile_service::get_table_profile(conn_id, db_type, database, schema, table)
            .await
    }

    // ==================== 规则 ====================

    /// 执行指定 id 的规则（含 QualityRule 质量门控）。调用方需已持有 DuckDB 锁。
    pub fn execute_insight_rule(
        project_root: Option<&Path>,
        rule_id: &str,
        conn: &duckdb::Connection,
        params: &HashMap<String, String>,
    ) -> Result<ExecutionResult, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::execute_insight_rule(registry, rule_id, conn, params)
        })
    }

    /// 列出规则（可按分类过滤）。返回项含 `scope` / `scope_path`，供界面按作用域分组。
    pub fn list_insight_rules(
        project_root: Option<&Path>,
        category: Option<&str>,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::list_insight_rules(registry, category)
        })
    }

    /// 列出适用于某列类型的规则。
    pub fn list_rules_for_column(
        project_root: Option<&Path>,
        column_type: &str,
    ) -> Result<Vec<serde_json::Value>, CoreError> {
        with_rules(project_root, |registry| {
            crate::insight_engine::list_rules_for_column(registry, column_type)
        })
    }

    /// 重新加载规则（丢弃缓存并按三层重建），返回生效规则总数。
    ///
    /// 常规改动由目录监听（[`watcher`]）自动生效；本方法是兜底与排障入口。
    pub fn reload_insight_rules(project_root: Option<&Path>) -> usize {
        crate::reload_insight_rules(project_root)
    }
}
