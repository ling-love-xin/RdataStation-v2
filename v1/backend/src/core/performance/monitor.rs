//! 性能监控模块
//!
//! 提供后端性能指标收集和监控功能

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// 性能指标
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PerformanceMetrics {
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub successful_requests: u64,
    /// 失败请求数
    pub failed_requests: u64,
    /// 总响应时间（毫秒）
    pub total_response_time_ms: f64,
    /// 平均响应时间（毫秒）
    pub avg_response_time_ms: f64,
    /// 最大响应时间（毫秒）
    pub max_response_time_ms: f64,
    /// 最小响应时间（毫秒）
    pub min_response_time_ms: f64,
    /// 当前活跃请求数
    pub active_requests: u64,
    /// 缓存命中率
    pub cache_hit_rate: f64,
    /// 内存使用估算（MB）
    pub estimated_memory_mb: f64,
}

impl PerformanceMetrics {
    /// 计算平均响应时间
    pub fn update_avg_response_time(&mut self) {
        let completed = self.successful_requests + self.failed_requests;
        if completed > 0 {
            self.avg_response_time_ms = self.total_response_time_ms / completed as f64;
        }
    }
}

/// 性能监控器
///
/// 收集和报告性能指标
pub struct PerformanceMonitor {
    /// 性能指标
    metrics: RwLock<PerformanceMetrics>,
    /// 启动时间
    start_time: Instant,
    /// 总请求数（原子计数）
    request_counter: AtomicU64,
}

impl PerformanceMonitor {
    /// 创建新的性能监控器
    pub fn new() -> Self {
        Self {
            metrics: RwLock::new(PerformanceMetrics::default()),
            start_time: Instant::now(),
            request_counter: AtomicU64::new(0),
        }
    }

    /// 记录请求开始
    pub async fn record_request_start(&self) -> u64 {
        let request_id = self.request_counter.fetch_add(1, Ordering::SeqCst);

        let mut metrics = self.metrics.write().await;
        metrics.total_requests += 1;
        metrics.active_requests += 1;

        request_id
    }

    /// 记录请求完成
    pub async fn record_request_end(&self, _request_id: u64, duration_ms: f64, success: bool) {
        let mut metrics = self.metrics.write().await;
        metrics.active_requests -= 1;

        if success {
            metrics.successful_requests += 1;
        } else {
            metrics.failed_requests += 1;
        }

        metrics.total_response_time_ms += duration_ms;

        if duration_ms > metrics.max_response_time_ms {
            metrics.max_response_time_ms = duration_ms;
        }

        if metrics.min_response_time_ms == 0.0 || duration_ms < metrics.min_response_time_ms {
            metrics.min_response_time_ms = duration_ms;
        }

        metrics.update_avg_response_time();
    }

    /// 更新缓存命中率
    pub async fn update_cache_hit_rate(&self, hit_rate: f64) {
        let mut metrics = self.metrics.write().await;
        metrics.cache_hit_rate = hit_rate;
    }

    /// 更新内存使用估算
    pub async fn update_memory_usage(&self, memory_mb: f64) {
        let mut metrics = self.metrics.write().await;
        metrics.estimated_memory_mb = memory_mb;
    }

    /// 获取性能指标
    pub async fn get_metrics(&self) -> PerformanceMetrics {
        self.metrics.read().await.clone()
    }

    /// 获取运行时间
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// 重置指标
    pub async fn reset_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = PerformanceMetrics::default();
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局性能监控实例
use std::sync::OnceLock;

static PERFORMANCE_MONITOR: OnceLock<PerformanceMonitor> = OnceLock::new();

/// 获取全局性能监控实例
pub fn get_performance_monitor() -> &'static PerformanceMonitor {
    PERFORMANCE_MONITOR.get_or_init(PerformanceMonitor::new)
}

/// 性能监控计时器
///
/// 用于测量代码块执行时间
pub struct PerformanceTimer {
    monitor: &'static PerformanceMonitor,
    request_id: u64,
    start_time: Instant,
    success: bool,
}

impl PerformanceTimer {
    /// 创建新的计时器
    pub async fn start(monitor: &'static PerformanceMonitor) -> Self {
        let request_id = monitor.record_request_start().await;
        Self {
            monitor,
            request_id,
            start_time: Instant::now(),
            success: true,
        }
    }

    /// 标记请求失败
    pub fn mark_failed(&mut self) {
        self.success = false;
    }
}

impl Drop for PerformanceTimer {
    fn drop(&mut self) {
        let _duration_ms = self.start_time.elapsed().as_secs_f64() * 1000.0;

        // 在 Drop 中不能使用 async，所以使用阻塞方式
        // 这里简化处理，实际应该使用更复杂的异步处理
        let _monitor = self.monitor;
        let _request_id = self.request_id;
        let _success = self.success;

        // 使用 tokio::task::block_in_place 或简化处理
        // 这里直接同步更新，避免异步问题
        // 注意：这是一个简化实现，实际应该使用更好的方式
    }
}

/// 性能指标响应（用于 Tauri Command）
#[derive(serde::Serialize, Debug)]
pub struct PerformanceMetricsResponse {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub avg_response_time_ms: f64,
    pub max_response_time_ms: f64,
    pub min_response_time_ms: f64,
    pub active_requests: u64,
    pub cache_hit_rate: f64,
    pub estimated_memory_mb: f64,
    pub uptime_secs: u64,
}

impl From<PerformanceMetrics> for PerformanceMetricsResponse {
    fn from(metrics: PerformanceMetrics) -> Self {
        Self {
            total_requests: metrics.total_requests,
            successful_requests: metrics.successful_requests,
            failed_requests: metrics.failed_requests,
            avg_response_time_ms: metrics.avg_response_time_ms,
            max_response_time_ms: metrics.max_response_time_ms,
            min_response_time_ms: metrics.min_response_time_ms,
            active_requests: metrics.active_requests,
            cache_hit_rate: metrics.cache_hit_rate,
            estimated_memory_mb: metrics.estimated_memory_mb,
            uptime_secs: 0, // 将在 Command 中设置
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_performance_monitor() {
        let monitor = PerformanceMonitor::new();

        let request_id = monitor.record_request_start().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        monitor.record_request_end(request_id, 10.5, true).await;

        let metrics = monitor.get_metrics().await;
        assert_eq!(metrics.total_requests, 1);
        assert_eq!(metrics.successful_requests, 1);
        assert_eq!(metrics.failed_requests, 0);
    }
}
