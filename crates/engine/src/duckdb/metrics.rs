use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

/// DuckDB 性能指标
///
/// 记录连接池统计、查询耗时、内存使用等关键指标。
pub struct DuckDBMetrics {
    /// 总查询次数
    total_queries: AtomicU64,
    /// 总写入次数
    total_writes: AtomicU64,
    /// 总查询耗时（微秒）
    total_query_duration_us: AtomicU64,
    /// 最长查询耗时（微秒）
    max_query_duration_us: AtomicU64,
    /// 当前活跃连接数
    active_connections: AtomicUsize,
    /// 总连接创建次数
    total_connections_created: AtomicU64,
    /// 总连接关闭次数
    total_connections_closed: AtomicU64,
    /// 当前临时表数量
    current_temp_tables: AtomicUsize,
    /// 总错误次数
    total_errors: AtomicU64,
}

impl DuckDBMetrics {
    /// 创建新的性能指标实例。
    pub fn new() -> Self {
        DuckDBMetrics {
            total_queries: AtomicU64::new(0),
            total_writes: AtomicU64::new(0),
            total_query_duration_us: AtomicU64::new(0),
            max_query_duration_us: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            total_connections_created: AtomicU64::new(0),
            total_connections_closed: AtomicU64::new(0),
            current_temp_tables: AtomicUsize::new(0),
            total_errors: AtomicU64::new(0),
        }
    }

    /// 记录查询执行。
    ///
    /// # 参数
    /// - `start_time`: 查询开始时间
    pub fn record_query(&self, start_time: Instant) {
        let duration_us = start_time.elapsed().as_micros() as u64;

        self.total_queries.fetch_add(1, Ordering::Relaxed);
        self.total_query_duration_us
            .fetch_add(duration_us, Ordering::Relaxed);

        // 更新最大耗时
        let mut current_max = self.max_query_duration_us.load(Ordering::Relaxed);
        while duration_us > current_max {
            match self.max_query_duration_us.compare_exchange_weak(
                current_max,
                duration_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(new_max) => current_max = new_max,
            }
        }
    }

    /// 记录写入操作。
    pub fn record_write(&self, start_time: Instant) {
        let duration_us = start_time.elapsed().as_micros() as u64;

        self.total_writes.fetch_add(1, Ordering::Relaxed);
        self.total_query_duration_us
            .fetch_add(duration_us, Ordering::Relaxed);
    }

    /// 记录错误。
    pub fn record_error(&self) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录连接创建。
    pub fn record_connection_created(&self) {
        self.total_connections_created
            .fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录连接关闭。
    pub fn record_connection_closed(&self) {
        self.total_connections_closed
            .fetch_add(1, Ordering::Relaxed);
        let current = self.active_connections.fetch_sub(1, Ordering::Relaxed);
        if current == 0 {
            tracing::warn!("[DuckDBMetrics] 活跃连接数变为 0");
        }
    }

    /// 更新临时表数量。
    pub fn set_temp_table_count(&self, count: usize) {
        self.current_temp_tables.store(count, Ordering::Relaxed);
    }

    /// 获取总查询次数。
    pub fn total_queries(&self) -> u64 {
        self.total_queries.load(Ordering::Relaxed)
    }

    /// 获取总写入次数。
    pub fn total_writes(&self) -> u64 {
        self.total_writes.load(Ordering::Relaxed)
    }

    /// 获取总查询耗时（微秒）。
    pub fn total_query_duration_us(&self) -> u64 {
        self.total_query_duration_us.load(Ordering::Relaxed)
    }

    /// 获取最长查询耗时（微秒）。
    pub fn max_query_duration_us(&self) -> u64 {
        self.max_query_duration_us.load(Ordering::Relaxed)
    }

    /// 获取平均查询耗时（微秒）。
    pub fn avg_query_duration_us(&self) -> f64 {
        let total = self.total_queries.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        self.total_query_duration_us.load(Ordering::Relaxed) as f64 / total as f64
    }

    /// 获取当前活跃连接数。
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// 获取总连接创建次数。
    pub fn total_connections_created(&self) -> u64 {
        self.total_connections_created.load(Ordering::Relaxed)
    }

    /// 获取总连接关闭次数。
    pub fn total_connections_closed(&self) -> u64 {
        self.total_connections_closed.load(Ordering::Relaxed)
    }

    /// 获取当前临时表数量。
    pub fn current_temp_tables(&self) -> usize {
        self.current_temp_tables.load(Ordering::Relaxed)
    }

    /// 获取总错误次数。
    pub fn total_errors(&self) -> u64 {
        self.total_errors.load(Ordering::Relaxed)
    }

    /// 重置所有指标。
    pub fn reset(&self) {
        self.total_queries.store(0, Ordering::Relaxed);
        self.total_writes.store(0, Ordering::Relaxed);
        self.total_query_duration_us.store(0, Ordering::Relaxed);
        self.max_query_duration_us.store(0, Ordering::Relaxed);
        self.active_connections.store(0, Ordering::Relaxed);
        self.total_connections_created.store(0, Ordering::Relaxed);
        self.total_connections_closed.store(0, Ordering::Relaxed);
        self.current_temp_tables.store(0, Ordering::Relaxed);
        self.total_errors.store(0, Ordering::Relaxed);
    }

    /// 生成指标快照（用于导出/日志）。
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_queries: self.total_queries(),
            total_writes: self.total_writes(),
            total_query_duration_us: self.total_query_duration_us(),
            max_query_duration_us: self.max_query_duration_us(),
            avg_query_duration_us: self.avg_query_duration_us(),
            active_connections: self.active_connections(),
            total_connections_created: self.total_connections_created(),
            total_connections_closed: self.total_connections_closed(),
            current_temp_tables: self.current_temp_tables(),
            total_errors: self.total_errors(),
        }
    }
}

impl Default for DuckDBMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 指标快照（不可变副本）
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub total_queries: u64,
    pub total_writes: u64,
    pub total_query_duration_us: u64,
    pub max_query_duration_us: u64,
    pub avg_query_duration_us: f64,
    pub active_connections: usize,
    pub total_connections_created: u64,
    pub total_connections_closed: u64,
    pub current_temp_tables: usize,
    pub total_errors: u64,
}

impl std::fmt::Display for MetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== DuckDB 性能指标快照 ===")?;
        writeln!(f, "总查询次数: {}", self.total_queries)?;
        writeln!(f, "总写入次数: {}", self.total_writes)?;
        writeln!(
            f,
            "总查询耗时: {:.2} ms",
            self.total_query_duration_us as f64 / 1000.0
        )?;
        writeln!(
            f,
            "最长查询耗时: {:.2} ms",
            self.max_query_duration_us as f64 / 1000.0
        )?;
        writeln!(
            f,
            "平均查询耗时: {:.2} ms",
            self.avg_query_duration_us / 1000.0
        )?;
        writeln!(f, "活跃连接数: {}", self.active_connections)?;
        writeln!(
            f,
            "连接创建/关闭: {}/{}",
            self.total_connections_created, self.total_connections_closed
        )?;
        writeln!(f, "临时表数量: {}", self.current_temp_tables)?;
        write!(f, "错误次数: {}", self.total_errors)
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_query_and_stats() {
        let metrics = DuckDBMetrics::new();

        assert_eq!(metrics.total_queries(), 0);
        assert_eq!(metrics.avg_query_duration_us(), 0.0);

        let start = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        metrics.record_query(start);

        assert_eq!(metrics.total_queries(), 1);
        assert!(metrics.max_query_duration_us() > 0);
        assert!(metrics.avg_query_duration_us() > 0.0);
    }

    #[test]
    fn test_record_write() {
        let metrics = DuckDBMetrics::new();
        let start = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        metrics.record_write(start);

        assert_eq!(metrics.total_writes(), 1);
    }

    #[test]
    fn test_record_error() {
        let metrics = DuckDBMetrics::new();
        metrics.record_error();
        metrics.record_error();

        assert_eq!(metrics.total_errors(), 2);
    }

    #[test]
    fn test_connection_tracking() {
        let metrics = DuckDBMetrics::new();

        metrics.record_connection_created();
        metrics.record_connection_created();
        assert_eq!(metrics.active_connections(), 2);
        assert_eq!(metrics.total_connections_created(), 2);

        metrics.record_connection_closed();
        assert_eq!(metrics.active_connections(), 1);
        assert_eq!(metrics.total_connections_closed(), 1);
    }

    #[test]
    fn test_temp_table_count() {
        let metrics = DuckDBMetrics::new();
        metrics.set_temp_table_count(15);
        assert_eq!(metrics.current_temp_tables(), 15);
    }

    #[test]
    fn test_reset() {
        let metrics = DuckDBMetrics::new();
        metrics.record_query(Instant::now());
        metrics.record_error();
        metrics.set_temp_table_count(5);

        metrics.reset();

        assert_eq!(metrics.total_queries(), 0);
        assert_eq!(metrics.total_errors(), 0);
        assert_eq!(metrics.current_temp_tables(), 0);
    }

    #[test]
    fn test_snapshot_display() {
        let metrics = DuckDBMetrics::new();
        metrics.record_query(Instant::now());

        let snapshot = metrics.snapshot();
        let display = format!("{}", snapshot);

        assert!(display.contains("DuckDB 性能指标快照"));
        assert!(display.contains("总查询次数: 1"));
    }

    #[test]
    fn test_multiple_queries_avg() {
        let metrics = DuckDBMetrics::new();

        let start1 = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        metrics.record_query(start1);

        let start2 = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        metrics.record_query(start2);

        assert_eq!(metrics.total_queries(), 2);
        assert!(metrics.avg_query_duration_us() > 10000.0); // 平均 > 10ms
    }

    #[test]
    fn test_max_query_duration() {
        let metrics = DuckDBMetrics::new();

        // 短查询
        let start = Instant::now();
        metrics.record_query(start);
        let short_max = metrics.max_query_duration_us();

        // 长查询
        let start = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(50));
        metrics.record_query(start);
        let long_max = metrics.max_query_duration_us();

        assert!(long_max > short_max);
    }
}
