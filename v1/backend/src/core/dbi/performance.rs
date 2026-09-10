/**
 * 性能统计模块
 *
 * 收集和记录查询执行的性能数据，用于：
 * - 执行模式推荐优化
 * - 性能瓶颈分析
 * - 查询优化建议
 */
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::core::dbi::engine::SqlFeatures;

/// 单次查询的性能记录
#[derive(Debug, Clone)]
pub struct QueryPerformanceRecord {
    /// SQL 语句（截断）
    pub sql_preview: String,
    /// SQL 特征
    pub features: SqlFeatures,
    /// 执行模式（Native/DuckDB/Stream）
    pub execution_mode: String,
    /// 执行耗时（毫秒）
    pub elapsed_ms: f64,
    /// 返回行数
    pub row_count: usize,
    /// 是否成功
    pub success: bool,
    /// 错误信息（如果失败）
    pub error: Option<String>,
    /// 时间戳
    pub timestamp: std::time::SystemTime,
}

/// 聚合性能统计
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    /// 总查询次数
    pub total_queries: u64,
    /// 成功次数
    pub success_count: u64,
    /// 失败次数
    pub failure_count: u64,
    /// 平均耗时（毫秒）
    pub avg_elapsed_ms: f64,
    /// 最大耗时（毫秒）
    pub max_elapsed_ms: f64,
    /// 最小耗时（毫秒）
    pub min_elapsed_ms: f64,
    /// P50 耗时（毫秒）
    pub p50_elapsed_ms: f64,
    /// P95 耗时（毫秒）
    pub p95_elapsed_ms: f64,
    /// P99 耗时（毫秒）
    pub p99_elapsed_ms: f64,
    /// 总返回行数
    pub total_rows: u64,
    /// 平均返回行数
    pub avg_rows: f64,
}

/// 按执行模式分组的统计
#[derive(Debug, Clone)]
pub struct ModePerformanceStats {
    /// 模式名称
    pub mode: String,
    /// 该模式的查询次数
    pub query_count: u64,
    /// 平均耗时
    pub avg_elapsed_ms: f64,
    /// 成功率
    pub success_rate: f64,
}

/// 性能统计收集器
pub struct PerformanceCollector {
    /// 查询记录（最近 N 条）
    records: Arc<RwLock<Vec<QueryPerformanceRecord>>>,
    /// 最大记录数
    max_records: usize,
    /// 按 SQL 模式分类的统计
    mode_stats: Arc<RwLock<HashMap<String, Vec<f64>>>>,
}

impl PerformanceCollector {
    /// 创建新的性能收集器
    pub fn new(max_records: usize) -> Self {
        Self {
            records: Arc::new(RwLock::new(Vec::with_capacity(max_records))),
            max_records,
            mode_stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 记录一次查询
    #[allow(clippy::too_many_arguments)]
    pub async fn record_query(
        &self,
        sql: &str,
        features: SqlFeatures,
        execution_mode: &str,
        elapsed_ms: f64,
        row_count: usize,
        success: bool,
        error: Option<String>,
    ) {
        let record = QueryPerformanceRecord {
            sql_preview: if sql.len() > 100 {
                format!("{}...", &sql[..100])
            } else {
                sql.to_string()
            },
            features,
            execution_mode: execution_mode.to_string(),
            elapsed_ms,
            row_count,
            success,
            error,
            timestamp: std::time::SystemTime::now(),
        };

        let mut records = self.records.write().await;
        records.push(record);
        if records.len() > self.max_records {
            records.remove(0);
        }
        drop(records);

        let mut mode_stats = self.mode_stats.write().await;
        mode_stats
            .entry(execution_mode.to_string())
            .or_insert_with(Vec::new)
            .push(elapsed_ms);
    }

    /// 获取总体性能统计
    pub async fn get_overall_stats(&self) -> PerformanceStats {
        let records = self.records.read().await;
        if records.is_empty() {
            return PerformanceStats {
                total_queries: 0,
                success_count: 0,
                failure_count: 0,
                avg_elapsed_ms: 0.0,
                max_elapsed_ms: 0.0,
                min_elapsed_ms: 0.0,
                p50_elapsed_ms: 0.0,
                p95_elapsed_ms: 0.0,
                p99_elapsed_ms: 0.0,
                total_rows: 0,
                avg_rows: 0.0,
            };
        }

        let mut elapsed_times: Vec<f64> = records.iter().map(|r| r.elapsed_ms).collect();
        elapsed_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let total_queries = records.len() as u64;
        let success_count = records.iter().filter(|r| r.success).count() as u64;
        let failure_count = total_queries - success_count;
        let total_elapsed: f64 = elapsed_times.iter().sum();
        let total_rows: u64 = records.iter().map(|r| r.row_count as u64).sum();

        let avg_elapsed_ms = total_elapsed / total_queries as f64;
        let max_elapsed_ms = *elapsed_times.last().unwrap_or(&0.0);
        let min_elapsed_ms = *elapsed_times.first().unwrap_or(&0.0);

        let p50_idx = (total_queries as f64 * 0.5) as usize;
        let p95_idx = (total_queries as f64 * 0.95) as usize;
        let p99_idx = (total_queries as f64 * 0.99) as usize;

        let p50_elapsed_ms = elapsed_times
            .get(p50_idx.min(elapsed_times.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let p95_elapsed_ms = elapsed_times
            .get(p95_idx.min(elapsed_times.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let p99_elapsed_ms = elapsed_times
            .get(p99_idx.min(elapsed_times.len() - 1))
            .copied()
            .unwrap_or(0.0);

        PerformanceStats {
            total_queries,
            success_count,
            failure_count,
            avg_elapsed_ms,
            max_elapsed_ms,
            min_elapsed_ms,
            p50_elapsed_ms,
            p95_elapsed_ms,
            p99_elapsed_ms,
            total_rows,
            avg_rows: total_rows as f64 / total_queries as f64,
        }
    }

    /// 获取按执行模式分组的统计
    pub async fn get_mode_stats(&self) -> Vec<ModePerformanceStats> {
        let mode_stats = self.mode_stats.read().await;
        let records = self.records.read().await;

        mode_stats
            .iter()
            .map(|(mode, times)| {
                let query_count = times.len() as u64;
                let avg_elapsed_ms = if query_count > 0 {
                    times.iter().sum::<f64>() / query_count as f64
                } else {
                    0.0
                };

                let success_count = records
                    .iter()
                    .filter(|r| &r.execution_mode == mode && r.success)
                    .count() as u64;

                let success_rate = if query_count > 0 {
                    success_count as f64 / query_count as f64
                } else {
                    0.0
                };

                ModePerformanceStats {
                    mode: mode.clone(),
                    query_count,
                    avg_elapsed_ms,
                    success_rate,
                }
            })
            .collect()
    }

    /// 获取最近 N 条查询记录
    pub async fn get_recent_records(&self, limit: usize) -> Vec<QueryPerformanceRecord> {
        let records = self.records.read().await;
        let start = records.len().saturating_sub(limit);
        records[start..].to_vec()
    }

    /// 获取推荐执行模式的建议
    ///
    /// 基于历史性能数据，分析哪种执行模式更适合特定类型的查询
    pub async fn get_mode_recommendation(&self) -> HashMap<String, ModePerformanceStats> {
        let mode_stats = self.get_mode_stats().await;
        mode_stats
            .into_iter()
            .map(|s| (s.mode.clone(), s))
            .collect()
    }

    /// 清空所有统计数据
    pub async fn clear(&self) {
        let mut records = self.records.write().await;
        records.clear();
        drop(records);

        let mut mode_stats = self.mode_stats.write().await;
        mode_stats.clear();
    }
}

impl Default for PerformanceCollector {
    fn default() -> Self {
        Self::new(10000)
    }
}
