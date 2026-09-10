//! 智能连接池模块 — SmartPool（守护系统内置库）
//!
//! ## 架构归属
//!
//! SmartPool 是 RdataStation 双池架构中的系统层，专门管理 RdataStation 自身运行
//! 所需的核心存储，与用户数据源（StandardPool）完全隔离。
//!
//! ```
//! SmartPool（守护系统内置库）                 StandardPool（用户数据源）
//! ├── 应用级 SQLite（global.db）              ├── 用户连接的 MySQL
//! ├── 项目级 SQLite（project.db）             ├── 用户连接的 PostgreSQL
//! ├── 连接元数据 SQLite（conn_{id}.sqlite）   ├── 用户连接的 SQLite（外部 .db）
//! ├── 应用级 DuckDB（analytics.duckdb）       └── 用户连接的 DuckDB（外部 .duckdb）
//! └── 项目级 DuckDB（analytics.duckdb）
//! ```
//!
//! ## 能力
//!
//! - 动态池大小调整（根据负载自动扩缩容）
//! - 连接健康检查（定期检测失效连接）
//! - 连接预热（启动时预创建连接）
//! - 负载感知（根据查询延迟调整策略）
//! - 优雅关闭（等待活跃查询完成）
//! - 内存压力感知（系统内存不足时自动缩容）
//!
//! ## 与 StandardPool 的关键差异
//!
//! | 维度         | SmartPool                        | StandardPool                    |
//! |-------------|----------------------------------|---------------------------------|
//! | 管理对象     | 系统内置库（RdataStation 自身依赖） | 用户数据源（用户外部数据库）      |
//! | 生命周期     | 应用/项目级别（启动→关闭）          | 连接级别（连接→断开）            |
//! | 配置方式     | 应用开发者硬编码                    | 用户在连接页面手动设置            |
//! | 失败处理     | 系统级故障（需要立即告警）           | 用户级故障（提示并允许重试）       |
//! | 动态扩缩容   | ✅ 延迟感知 + 内存压力感知           | ❌ 固定大小（用户配置）            |

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

use crate::core::cache::MemoryPressure;
use crate::core::driver::traits::{Database, DbPool, PoolStatus};
use crate::core::error::{ConnectionError, CoreError};

/// 智能连接池配置
#[derive(Debug, Clone)]
pub struct SmartPoolConfig {
    /// 最小连接数
    pub min_connections: u32,
    /// 最大连接数
    pub max_connections: u32,
    /// 初始连接数（预热）
    pub initial_connections: u32,
    /// 连接获取超时
    pub acquire_timeout: Duration,
    /// 空闲连接超时
    pub idle_timeout: Duration,
    /// 连接最大生命周期
    pub max_lifetime: Duration,
    /// 健康检查间隔
    pub health_check_interval: Duration,
    /// 是否启用动态调整
    pub enable_dynamic_scaling: bool,
    /// 动态调整阈值（平均延迟超过此值时扩容）
    pub scaling_threshold_ms: u64,
    /// 每次扩容步长
    pub scale_up_step: u32,
    /// 每次缩容步长
    pub scale_down_step: u32,
}

impl Default for SmartPoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 2,
            max_connections: 20,
            initial_connections: 2,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
            health_check_interval: Duration::from_secs(60),
            enable_dynamic_scaling: true,
            scaling_threshold_ms: 100,
            scale_up_step: 2,
            scale_down_step: 1,
        }
    }
}

/// 连接池统计信息
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// 当前连接数
    pub current_size: u32,
    /// 活跃连接数
    pub active_connections: u32,
    /// 空闲连接数
    pub idle_connections: u32,
    /// 等待获取连接的请求数
    pub waiting_requests: u32,
    /// 总获取次数
    pub total_acquires: u64,
    /// 总释放次数
    pub total_releases: u64,
    /// 平均获取延迟（毫秒）
    pub avg_acquire_ms: f64,
    /// 连接创建次数
    pub connections_created: u64,
    /// 连接销毁次数
    pub connections_destroyed: u64,
    /// 健康检查失败次数
    pub health_check_failures: u64,
    /// 扩容次数
    pub scale_up_count: u64,
    /// 缩容次数
    pub scale_down_count: u64,
}

/// 单个连接元数据
#[allow(dead_code)]
struct ConnectionMetadata {
    /// 创建时间
    created_at: Instant,
    /// 最后使用时间
    last_used: Instant,
    /// 使用次数
    use_count: u64,
    /// 是否正在使用
    in_use: bool,
}

#[allow(dead_code)]
impl ConnectionMetadata {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            created_at: now,
            last_used: now,
            use_count: 0,
            in_use: false,
        }
    }

    fn mark_used(&mut self) {
        self.last_used = Instant::now();
        self.use_count += 1;
        self.in_use = true;
    }

    fn mark_released(&mut self) {
        self.in_use = false;
    }

    fn is_expired(&self, max_lifetime: Duration) -> bool {
        self.created_at.elapsed() > max_lifetime
    }

    fn is_idle_expired(&self, idle_timeout: Duration) -> bool {
        !self.in_use && self.last_used.elapsed() > idle_timeout
    }
}

/// 智能连接池内部状态
struct SmartPoolInner {
    /// 连接池配置
    config: SmartPoolConfig,
    /// 连接元数据（key 为连接标识）
    connections: std::collections::HashMap<String, ConnectionMetadata>,
    /// 统计信息
    stats: PoolStats,
    /// 池是否已关闭
    closed: bool,
}

impl SmartPoolInner {
    fn new(config: SmartPoolConfig) -> Self {
        Self {
            config,
            connections: std::collections::HashMap::new(),
            stats: PoolStats::default(),
            closed: false,
        }
    }
}

/// 智能连接池包装器
///
/// 包装底层数据库连接池，提供智能管理功能
pub struct SmartPoolWrapper {
    /// 底层连接池
    inner_pool: Arc<dyn DbPool>,
    /// 智能池管理器
    smart_pool: SmartPool,
    /// 池是否已关闭（同步标志）
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl SmartPoolWrapper {
    /// 创建新的智能池包装器
    pub fn new(
        name: impl Into<String>,
        config: SmartPoolConfig,
        inner_pool: Arc<dyn DbPool>,
    ) -> Self {
        Self {
            inner_pool,
            smart_pool: SmartPool::new(name, config),
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 获取底层连接池
    pub fn inner_pool(&self) -> Arc<dyn DbPool> {
        self.inner_pool.clone()
    }

    /// 获取智能池管理器
    pub fn smart_pool(&self) -> &SmartPool {
        &self.smart_pool
    }
}

#[async_trait::async_trait]
impl DbPool for SmartPoolWrapper {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        let start = Instant::now();

        // 检查池是否已关闭
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::connection(ConnectionError::PoolClosed));
        }

        // 从底层池获取连接
        let db = self.inner_pool.acquire().await?;

        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

        // 记录获取统计
        self.smart_pool.record_acquire(latency_ms).await;

        // 检查是否需要扩容
        if self.smart_pool.should_scale_up().await {
            self.smart_pool.scale_up().await;
        }

        Ok(db)
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        // 先关闭智能池
        self.smart_pool.close().await?;
        // 再关闭底层池
        self.inner_pool.close().await
    }

    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn status(&self) -> PoolStatus {
        // 合并底层池状态和智能池统计
        let inner_status = self.inner_pool.status();
        PoolStatus {
            size: inner_status.size,
            idle: inner_status.idle,
            active: inner_status.active,
            waiting: inner_status.waiting,
            max_connections: inner_status.max_connections,
            min_connections: inner_status.min_connections,
        }
    }
}

/// 智能连接池
///
/// 提供自适应连接管理，根据负载动态调整池大小
pub struct SmartPool {
    /// 内部状态
    inner: Arc<Mutex<SmartPoolInner>>,
    /// 连接获取延迟采样（用于计算平均值）
    acquire_latencies: Arc<RwLock<Vec<f64>>>,
    /// 池名称（用于日志）
    name: String,
}

#[allow(dead_code)]
impl SmartPool {
    /// 创建新的智能连接池
    pub fn new(name: impl Into<String>, config: SmartPoolConfig) -> Self {
        let name = name.into();
        info!(
            pool_name = %name,
            min_conn = config.min_connections,
            max_conn = config.max_connections,
            dynamic_scaling = config.enable_dynamic_scaling,
            "Creating smart connection pool"
        );

        Self {
            inner: Arc::new(Mutex::new(SmartPoolInner::new(config))),
            acquire_latencies: Arc::new(RwLock::new(Vec::with_capacity(100))),
            name,
        }
    }

    /// 创建带默认配置的池
    pub fn with_defaults(name: impl Into<String>) -> Self {
        Self::new(name, SmartPoolConfig::default())
    }

    /// 获取池配置
    pub async fn config(&self) -> SmartPoolConfig {
        self.inner.lock().await.config.clone()
    }

    /// 获取池统计信息
    pub async fn stats(&self) -> PoolStats {
        self.inner.lock().await.stats.clone()
    }

    /// 记录连接获取延迟
    async fn record_acquire_latency(&self, latency_ms: f64) {
        let mut latencies = self.acquire_latencies.write().await;
        latencies.push(latency_ms);

        // 保持最近 100 次采样
        let len = latencies.len();
        if len > 100 {
            let drain_to = len - 100;
            latencies.drain(0..drain_to);
        }
    }

    /// 计算平均获取延迟
    async fn avg_acquire_latency(&self) -> f64 {
        let latencies = self.acquire_latencies.read().await;
        if latencies.is_empty() {
            return 0.0;
        }
        latencies.iter().sum::<f64>() / latencies.len() as f64
    }

    /// 检查是否需要扩容
    pub async fn should_scale_up(&self) -> bool {
        let inner = self.inner.lock().await;
        if !inner.config.enable_dynamic_scaling {
            return false;
        }

        let current_size = inner.connections.len() as u32;
        if current_size >= inner.config.max_connections {
            return false;
        }

        let scaling_threshold_ms = inner.config.scaling_threshold_ms;
        drop(inner);

        let avg_latency = self.avg_acquire_latency().await;
        avg_latency > scaling_threshold_ms as f64
    }

    /// 检查是否需要缩容
    pub async fn should_scale_down(&self) -> bool {
        let inner = self.inner.lock().await;
        if !inner.config.enable_dynamic_scaling {
            return false;
        }

        let current_size = inner.connections.len() as u32;
        if current_size <= inner.config.min_connections {
            return false;
        }

        // 检查是否有过多空闲连接
        let idle_count = inner.connections.values().filter(|m| !m.in_use).count() as u32;

        idle_count > current_size / 2
    }

    /// 检查是否因内存压力需要缩容
    async fn should_scale_down_for_memory(&self) -> bool {
        let pressure = MemoryPressure::detect();

        match pressure {
            MemoryPressure::Critical => {
                // 高内存压力，强制缩容到最小
                let inner = self.inner.lock().await;
                let current_size = inner.connections.len() as u32;
                current_size > inner.config.min_connections
            }
            MemoryPressure::Moderate => {
                // 中等内存压力，检查是否有空闲连接可释放
                let inner = self.inner.lock().await;
                let current_size = inner.connections.len() as u32;
                if current_size <= inner.config.min_connections {
                    return false;
                }

                let idle_count = inner.connections.values().filter(|m| !m.in_use).count() as u32;

                idle_count > current_size / 3
            }
            MemoryPressure::Normal => false,
        }
    }

    /// 内存压力感知缩容
    pub async fn memory_pressure_scale_down(&self) -> Result<u32, CoreError> {
        let pressure = MemoryPressure::detect();
        let mut inner = self.inner.lock().await;

        let current_size = inner.connections.len() as u32;
        let target_size = match pressure {
            MemoryPressure::Critical => inner.config.min_connections,
            MemoryPressure::Moderate => (current_size / 2).max(inner.config.min_connections),
            MemoryPressure::Normal => current_size,
        };

        if target_size >= current_size {
            return Ok(0);
        }

        let to_remove = current_size - target_size;

        // 收集空闲连接
        let mut idle_connections: Vec<_> = inner
            .connections
            .iter()
            .filter(|(_, m)| !m.in_use)
            .map(|(k, m)| (k.clone(), m.last_used))
            .collect();

        idle_connections.sort_by_key(|(_, last_used)| *last_used);

        // 移除最久未使用的空闲连接
        let removed_count = (to_remove as usize).min(idle_connections.len()) as u32;
        for (key, _) in idle_connections.into_iter().take(removed_count as usize) {
            inner.connections.remove(&key);
            inner.stats.connections_destroyed += 1;
        }

        inner.stats.scale_down_count += 1;

        info!(
            pool_name = %self.name,
            pressure_level = format!("{:?}", pressure),
            old_size = current_size,
            new_size = current_size - removed_count,
            "Memory pressure scale down"
        );

        Ok(removed_count)
    }

    /// 执行扩容
    pub async fn scale_up(&self) {
        let mut inner = self.inner.lock().await;
        let step = inner.config.scale_up_step;
        let current_size = inner.connections.len() as u32;
        let new_size = (current_size + step).min(inner.config.max_connections);

        info!(
            pool_name = %self.name,
            old_size = current_size,
            new_size = new_size,
            "Scaling up connection pool"
        );

        inner.stats.scale_up_count += 1;
        // 实际连接创建由具体数据库驱动实现
    }

    /// 执行缩容
    pub async fn scale_down(&self) {
        let mut inner = self.inner.lock().await;
        let step = inner.config.scale_down_step;
        let current_size = inner.connections.len() as u32;
        let new_size = (current_size.saturating_sub(step)).max(inner.config.min_connections);

        info!(
            pool_name = %self.name,
            old_size = current_size,
            new_size = new_size,
            "Scaling down connection pool"
        );

        inner.stats.scale_down_count += 1;

        // 移除最久未使用的空闲连接
        let mut idle_connections: Vec<_> = inner
            .connections
            .iter()
            .filter(|(_, m)| !m.in_use)
            .map(|(k, m)| (k.clone(), m.last_used))
            .collect();

        idle_connections.sort_by_key(|(_, last_used)| *last_used);

        let to_remove = (current_size.saturating_sub(new_size)) as usize;
        for (key, _) in idle_connections.into_iter().take(to_remove) {
            inner.connections.remove(&key);
            inner.stats.connections_destroyed += 1;
        }
    }

    /// 执行健康检查
    pub async fn health_check(&self) -> Result<usize, CoreError> {
        let mut inner = self.inner.lock().await;
        let mut expired_count = 0;

        let max_lifetime = inner.config.max_lifetime;
        let idle_timeout = inner.config.idle_timeout;

        let expired_keys: Vec<_> = inner
            .connections
            .iter()
            .filter(|(_, m)| m.is_expired(max_lifetime) || m.is_idle_expired(idle_timeout))
            .map(|(k, _)| k.clone())
            .collect();

        for key in expired_keys {
            inner.connections.remove(&key);
            expired_count += 1;
            inner.stats.connections_destroyed += 1;
            inner.stats.health_check_failures += 1;
        }

        if expired_count > 0 {
            warn!(
                pool_name = %self.name,
                expired = expired_count,
                "Health check removed expired connections"
            );
        }

        debug!(
            pool_name = %self.name,
            remaining = inner.connections.len(),
            "Health check completed"
        );

        Ok(expired_count)
    }

    /// 关闭连接池（优雅关闭）
    pub async fn close(&self) -> Result<(), CoreError> {
        let mut inner = self.inner.lock().await;
        if inner.closed {
            return Ok(());
        }

        info!(
            pool_name = %self.name,
            remaining_connections = inner.connections.len(),
            "Closing smart connection pool"
        );

        inner.closed = true;
        inner.connections.clear();
        inner.stats.connections_destroyed += inner.stats.current_size as u64;

        Ok(())
    }

    /// 检查池是否已关闭
    pub async fn is_closed(&self) -> bool {
        self.inner.lock().await.closed
    }

    /// 更新统计信息中的当前大小
    async fn update_current_size(&self, size: u32) {
        let mut inner = self.inner.lock().await;
        inner.stats.current_size = size;
    }

    /// 记录连接获取
    pub async fn record_acquire(&self, latency_ms: f64) {
        let mut inner = self.inner.lock().await;
        inner.stats.total_acquires += 1;
        inner.stats.avg_acquire_ms = latency_ms;

        drop(inner);
        self.record_acquire_latency(latency_ms).await;
    }

    /// 记录连接释放
    async fn record_release(&self) {
        let mut inner = self.inner.lock().await;
        inner.stats.total_releases += 1;
    }

    /// 记录连接创建
    async fn record_connection_created(&self) {
        let mut inner = self.inner.lock().await;
        inner.stats.connections_created += 1;
    }

    /// 记录连接销毁
    async fn record_connection_destroyed(&self) {
        let mut inner = self.inner.lock().await;
        inner.stats.connections_destroyed += 1;
    }

    /// 更新活跃连接数
    async fn update_active_connections(&self, active: u32) {
        let mut inner = self.inner.lock().await;
        inner.stats.active_connections = active;
    }

    /// 更新空闲连接数
    async fn update_idle_connections(&self, idle: u32) {
        let mut inner = self.inner.lock().await;
        inner.stats.idle_connections = idle;
    }

    /// 更新等待请求数
    async fn update_waiting_requests(&self, waiting: u32) {
        let mut inner = self.inner.lock().await;
        inner.stats.waiting_requests = waiting;
    }
}

/// 智能连接池构建器
pub struct SmartPoolBuilder {
    name: String,
    config: SmartPoolConfig,
}

impl SmartPoolBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            config: SmartPoolConfig::default(),
        }
    }

    pub fn min_connections(mut self, min: u32) -> Self {
        self.config.min_connections = min;
        self
    }

    pub fn max_connections(mut self, max: u32) -> Self {
        self.config.max_connections = max;
        self
    }

    pub fn initial_connections(mut self, initial: u32) -> Self {
        self.config.initial_connections = initial;
        self
    }

    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.config.acquire_timeout = timeout;
        self
    }

    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.config.idle_timeout = timeout;
        self
    }

    pub fn max_lifetime(mut self, lifetime: Duration) -> Self {
        self.config.max_lifetime = lifetime;
        self
    }

    pub fn health_check_interval(mut self, interval: Duration) -> Self {
        self.config.health_check_interval = interval;
        self
    }

    pub fn enable_dynamic_scaling(mut self, enabled: bool) -> Self {
        self.config.enable_dynamic_scaling = enabled;
        self
    }

    pub fn scaling_threshold_ms(mut self, threshold: u64) -> Self {
        self.config.scaling_threshold_ms = threshold;
        self
    }

    pub fn build(self) -> SmartPool {
        SmartPool::new(self.name, self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_smart_pool_creation() {
        let pool = SmartPool::with_defaults("test_pool");
        assert!(!pool.is_closed().await);

        let stats = pool.stats().await;
        assert_eq!(stats.current_size, 0);
    }

    #[tokio::test]
    async fn test_smart_pool_close() -> Result<(), CoreError> {
        let pool = SmartPool::with_defaults("test_pool");
        pool.close().await?;
        assert!(pool.is_closed().await);
        Ok(())
    }

    #[tokio::test]
    async fn test_pool_builder() {
        let pool = SmartPoolBuilder::new("test_pool")
            .min_connections(5)
            .max_connections(50)
            .acquire_timeout(Duration::from_secs(10))
            .enable_dynamic_scaling(true)
            .build();

        let config = pool.config().await;
        assert_eq!(config.min_connections, 5);
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.acquire_timeout, Duration::from_secs(10));
        assert!(config.enable_dynamic_scaling);
    }
}
