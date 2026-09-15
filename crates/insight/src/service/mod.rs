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
use crate::model::ColumnProfileView;
use crate::{with_rules, ExecutionResult};

pub use persistence::{
    batch_evaluate_columns, cleanup_old_insight_snapshots, get_column_insight_history,
    get_insight_storage_stats, get_insight_version_detail, profile_column_from_table,
    save_column_insight_snapshot,
};

/// 错误 → 面板可展示的语义。
///
/// 面板的「错误态」只需要两件事：给人看的文案、要不要给「重试」。因此不直接展示
/// [`CoreError`] 的 `Display`（它带 `[code]` 内部错误码前缀）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightErrorInfo {
    pub message: String,
    /// 值得重试吗？瞬时的（并发配额 / 连接抖动）为真；结果集没了重试也没用，为假。
    pub retryable: bool,
}

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

    /// 列画像 → **面板视图模型**：一次调用拿齐「取当前项目的规则集 + 算 + 映射」。
    ///
    /// 阻塞：底层要抢 DuckDB 全局锁与并发配额（D12 / D13）。因此调用方负责把它放到
    /// 后台线程，算完再把结果回填面板（渲染路径零 I/O）。
    ///
    /// 直方图在 `ColumnInsightFull::histogram` 上（不在 `NumericStats` 里），
    /// 由 [`ColumnProfileView::from_domain`] 一并消费——所以面板不必分两次取数。
    pub fn profile_column_view(
        project_root: Option<&Path>,
        temp_table: &str,
        column_name: &str,
    ) -> Result<ColumnProfileView, CoreError> {
        let full = Self::get_column_insight_full(project_root, temp_table, column_name)?;
        Ok(ColumnProfileView::from_domain(&full))
    }

    /// 错误 → 面板可展示的语义（文案 + 是否可重试）。
    ///
    /// 识别方式是**按消息内容**匹配：引擎侧的 DuckDB 错误还没有结构化分类，
    /// 这是权宜。一旦 engine 给出 typed error（如 `TableNotFound` / `ConnectionLost`），
    /// 这里应改为匹配类型而不是字符串。
    pub fn describe_error(err: &CoreError) -> InsightErrorInfo {
        let raw = strip_error_code(&err.to_string());
        let lower = raw.to_lowercase();

        // 并发配额（D12）：瞬时，值得重试。文案直接用引擎侧那一份，
        // 避免「引擎一套、面板又一套」的两份文案。
        if raw == crate::insight_engine::ERR_TOO_MANY_CONCURRENT {
            return InsightErrorInfo {
                message: raw,
                retryable: true,
            };
        }

        // 结果集没了（临时表被重建 / 会话结束 / 超时清理）：重试无意义——
        // 用户必须重新执行那条查询。
        if hits_any(
            &lower,
            &[
                "does not exist",
                "no such table",
                "catalog error",
                "不存在",
                "binder error",
            ],
        ) {
            return InsightErrorInfo {
                message: "结果集已失效或已过期，请重新执行查询".into(),
                retryable: false,
            };
        }

        // 连接抖动：多为瞬时，值得重试。
        if hits_any(
            &lower,
            &[
                "connection",
                "socket",
                "network",
                "timeout",
                "timed out",
                "连接",
                "超时",
            ],
        ) {
            return InsightErrorInfo {
                message: "连接不可用，请检查数据源后重试".into(),
                retryable: true,
            };
        }

        // 兵底：原文照给（往往直接可定位），但不主动承诺重试。
        InsightErrorInfo {
            message: raw,
            retryable: false,
        }
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
    /// 常规改动由目录监听（[`watcher`]）自动生效；本方法是兵底与排障入口。
    pub fn reload_insight_rules(project_root: Option<&Path>) -> usize {
        crate::reload_insight_rules(project_root)
    }
}

/// 去掉 `Display` 的 `[code] ` 前缀（`CoreError` 的 Display 形如 `[C001] 文案`）。
/// 面板展示的是给人的文案，不展示内部错误码。
fn strip_error_code(text: &str) -> String {
    if let (Some(0), Some(end)) = (text.find('['), text.find(']')) {
        if end + 1 < text.len() {
            return text[end + 1..].trim_start().to_string();
        }
    }
    text.to_string()
}

fn hits_any(haystack_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack_lower.contains(n))
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::{strip_error_code, InsightService};
    use crate::insight_engine::ERR_TOO_MANY_CONCURRENT;
    use shared::error::{CommonError, ConnectionError, CoreError, DatabaseError};

    #[test]
    fn error_code_prefix_is_not_shown_to_users() {
        assert_eq!(strip_error_code("[C001] 出错了"), "出错了");
        assert_eq!(strip_error_code("无前缀"), "无前缀");
        // 中括号不在开头时不动它（文案里可能有引用）
        assert_eq!(strip_error_code("列 [amount] 不存在"), "列 [amount] 不存在");
        assert_eq!(strip_error_code("[C001]"), "[C001]");
    }

    #[test]
    fn concurrency_error_reuses_engine_text_and_is_retryable() {
        let err = CoreError::Common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()));
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, ERR_TOO_MANY_CONCURRENT);
        assert!(info.retryable, "并发配额是瞬时的，应给重试入口");
        assert!(
            !info.message.contains('['),
            "不得把内部错误码展示给用户：{}",
            info.message
        );
    }

    #[test]
    fn missing_result_set_is_not_retryable() {
        let err = CoreError::Database(DatabaseError::Query {
            sql: "SELECT * FROM t_result_7".into(),
            reason: "Catalog Error: Table with name t_result_7 does not exist".into(),
            position: None,
        });
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, "结果集已失效或已过期，请重新执行查询");
        assert!(!info.retryable, "结果集没了，重试不会变好——要重新执行查询");
    }

    #[test]
    fn connection_trouble_is_retryable() {
        let err = CoreError::Connection(ConnectionError::Refused {
            conn_id: "G_1".into(),
            reason: "connection refused".into(),
        });
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, "连接不可用，请检查数据源后重试");
        assert!(info.retryable);
    }

    #[test]
    fn unknown_error_keeps_wording_without_promising_retry() {
        let err = CoreError::Common(CommonError::General("规则 numeric-stats 执行失败".into()));
        let info = InsightService::describe_error(&err);
        assert_eq!(info.message, "规则 numeric-stats 执行失败", "原文往往自己就能定位");
        assert!(!info.retryable);
    }
}
