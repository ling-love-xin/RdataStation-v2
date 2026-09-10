/**
 * 执行引擎模块
 *
 * 包含多种执行引擎：
 * - DriverEngine: 原生数据库驱动执行
 * - DuckDBEngine: DuckDB 本地加速/联邦查询
 * - StreamEngine: 流式处理、合并、后处理
 */
pub mod driver_engine;
pub mod duckdb_engine;
pub mod stream_engine;

use std::sync::Arc;

use crate::core::dbi::context::QueryContext;
use crate::core::dbi::performance::PerformanceCollector;
use crate::core::error::CoreError;
use crate::core::models::QueryResult;

/// SQL 特征
#[derive(Debug, Clone, PartialEq)]
pub struct SqlFeatures {
    /// 是否是只读查询
    pub is_read_only: bool,
    /// 是否包含聚合
    pub has_aggregation: bool,
    /// 是否包含 JOIN
    pub has_join: bool,
    /// 是否包含子查询
    pub has_subquery: bool,
    /// 是否包含窗口函数
    pub has_window_function: bool,
    /// 是否包含 CTE
    pub has_cte: bool,
    /// 是否包含 ORDER BY
    pub has_order_by: bool,
    /// 是否包含 GROUP BY
    pub has_group_by: bool,
    /// 是否包含 HAVING
    pub has_having: bool,
    /// 是否包含 UNION
    pub has_union: bool,
    /// 是否包含 LIMIT
    pub has_limit: bool,
    /// 是否包含 DISTINCT
    pub has_distinct: bool,
    /// 查询复杂度评分
    pub complexity_score: u32,
}

impl SqlFeatures {
    /// 分析 SQL 特征
    pub fn analyze(sql: &str) -> Self {
        let sql_upper = sql.trim_start().to_uppercase();

        let is_read_only = !(sql_upper.starts_with("INSERT")
            || sql_upper.starts_with("UPDATE")
            || sql_upper.starts_with("DELETE")
            || sql_upper.starts_with("CREATE")
            || sql_upper.starts_with("DROP")
            || sql_upper.starts_with("ALTER")
            || sql_upper.starts_with("TRUNCATE"));

        let has_aggregation = sql_upper.contains("COUNT(")
            || sql_upper.contains("SUM(")
            || sql_upper.contains("AVG(")
            || sql_upper.contains("MAX(")
            || sql_upper.contains("MIN(");

        let has_join = sql_upper.contains("JOIN")
            || sql_upper.contains("CROSS JOIN")
            || sql_upper.contains("INNER JOIN")
            || sql_upper.contains("LEFT JOIN")
            || sql_upper.contains("RIGHT JOIN")
            || sql_upper.contains("FULL JOIN");

        let has_subquery = {
            let select_count = sql_upper.matches("SELECT").count();
            select_count > 1
        };

        let has_window_function = sql_upper.contains("OVER(")
            || sql_upper.contains("ROW_NUMBER()")
            || sql_upper.contains("RANK()")
            || sql_upper.contains("DENSE_RANK()")
            || sql_upper.contains("LAG(")
            || sql_upper.contains("LEAD(");

        let has_cte = sql_upper.starts_with("WITH");

        let has_order_by = sql_upper.contains("ORDER BY");
        let has_group_by = sql_upper.contains("GROUP BY");
        let has_having = sql_upper.contains("HAVING");
        let has_union = sql_upper.contains("UNION");
        let has_limit = sql_upper.contains("LIMIT");
        let has_distinct = sql_upper.contains("DISTINCT");

        let mut complexity_score = 0;
        if has_aggregation {
            complexity_score += 1;
        }
        if has_join {
            complexity_score += 2;
        }
        if has_subquery {
            complexity_score += 2;
        }
        if has_window_function {
            complexity_score += 3;
        }
        if has_cte {
            complexity_score += 1;
        }
        if has_order_by {
            complexity_score += 1;
        }
        if has_group_by {
            complexity_score += 1;
        }
        if has_having {
            complexity_score += 1;
        }
        if has_union {
            complexity_score += 2;
        }

        Self {
            is_read_only,
            has_aggregation,
            has_join,
            has_subquery,
            has_window_function,
            has_cte,
            has_order_by,
            has_group_by,
            has_having,
            has_union,
            has_limit,
            has_distinct,
            complexity_score,
        }
    }

    /// 是否需要 DuckDB 加速
    pub fn needs_duckdb_acceleration(&self) -> bool {
        self.complexity_score >= 3
            || self.has_join
            || self.has_aggregation
            || self.has_window_function
    }
}

/// 执行模式
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// 原生驱动执行
    Native,
    /// DuckDB 加速执行
    DuckDB,
    /// 流式执行
    Stream,
    /// 用户选择（未指定）
    UserChoice,
}

/// 执行引擎 trait
#[async_trait::async_trait]
pub trait ExecutionEngine: Send + Sync {
    /// 执行查询
    async fn execute(&self, sql: &str, context: &QueryContext) -> Result<QueryResult, CoreError>;

    /// 获取引擎名称
    fn name(&self) -> &str;

    /// 检查是否支持该 SQL
    fn supports(&self, sql: &str) -> bool;
}

/// 查询路由器
///
/// 根据查询上下文选择合适的执行引擎
pub struct QueryRouter {
    /// 原生驱动引擎
    driver_engine: Arc<driver_engine::DriverEngine>,
    /// DuckDB 引擎
    duckdb_engine: Arc<duckdb_engine::DuckDBEngine>,
    /// 流式引擎
    stream_engine: Arc<stream_engine::StreamEngine>,
    /// 性能统计收集器
    performance_collector: Arc<PerformanceCollector>,
}

impl QueryRouter {
    /// 创建新的查询路由器
    pub fn new(
        driver_engine: Arc<driver_engine::DriverEngine>,
        duckdb_engine: Arc<duckdb_engine::DuckDBEngine>,
        stream_engine: Arc<stream_engine::StreamEngine>,
        performance_collector: Arc<PerformanceCollector>,
    ) -> Self {
        Self {
            driver_engine,
            duckdb_engine,
            stream_engine,
            performance_collector,
        }
    }

    /// 执行查询
    pub async fn execute(
        &self,
        sql: &str,
        context: &QueryContext,
    ) -> Result<QueryResult, CoreError> {
        let start_time = std::time::Instant::now();
        let features = SqlFeatures::analyze(sql);
        let mode_name = match context.mode {
            ExecutionMode::Native => "Native",
            ExecutionMode::DuckDB => "DuckDB",
            ExecutionMode::Stream => "Stream",
            ExecutionMode::UserChoice => "UserChoice",
        };

        let result = match context.mode {
            ExecutionMode::Native => self.driver_engine.execute(sql, context).await,
            ExecutionMode::DuckDB => self.duckdb_engine.execute(sql, context).await,
            ExecutionMode::Stream => self.stream_engine.execute(sql, context).await,
            ExecutionMode::UserChoice => {
                // 智能推荐执行模式
                let recommended = self.recommend_mode(sql);
                match recommended {
                    ExecutionMode::DuckDB => self.duckdb_engine.execute(sql, context).await,
                    _ => self.driver_engine.execute(sql, context).await,
                }
            }
        };

        let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
        let row_count = result
            .as_ref()
            .map_or(0, |r| r.batches.iter().map(|b| b.num_rows()).sum());
        let success = result.is_ok();
        let error = result.as_ref().err().map(|e| e.to_string());

        self.performance_collector
            .record_query(
                sql, features, mode_name, elapsed_ms, row_count, success, error,
            )
            .await;

        result
    }

    /// 智能推荐执行模式
    pub fn recommend_mode(&self, sql: &str) -> ExecutionMode {
        let sql_upper = sql.trim_start().to_uppercase();

        // 写操作必须走原生驱动
        if sql_upper.starts_with("INSERT")
            || sql_upper.starts_with("UPDATE")
            || sql_upper.starts_with("DELETE")
            || sql_upper.starts_with("CREATE")
            || sql_upper.starts_with("DROP")
            || sql_upper.starts_with("ALTER")
        {
            return ExecutionMode::Native;
        }

        // 复杂查询推荐 DuckDB
        if sql_upper.contains("GROUP BY")
            || sql_upper.contains("JOIN")
            || sql_upper.contains("ORDER BY")
            || sql_upper.contains("HAVING")
            || sql_upper.contains("UNION")
        {
            return ExecutionMode::DuckDB;
        }

        // 默认用户选择
        ExecutionMode::UserChoice
    }

    /// 获取性能统计收集器
    pub fn performance_collector(&self) -> Arc<PerformanceCollector> {
        self.performance_collector.clone()
    }
}
