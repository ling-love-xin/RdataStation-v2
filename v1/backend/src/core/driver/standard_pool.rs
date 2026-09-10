use serde::{Deserialize, Serialize};
use specta::Type;
/**
 * 标准连接池模块 (Standard Pool)
 *
 * 面向用户数据源的标准连接池实现，属于双池架构中的 StandardPool 层。
 *
 * ## 架构归属
 *
 * RdataStation 连接池采用双层架构：
 *
 * ```
 * SmartPool（守护系统内置库）                 StandardPool（用户数据源）← 本模块
 * ├── 应用级 SQLite（global.db）              ├── 用户连接的 MySQL
 * ├── 项目级 SQLite（project.db）             ├── 用户连接的 PostgreSQL
 * ├── 连接元数据 SQLite（conn_{id}.sqlite）   ├── 用户连接的 SQLite
 * ├── 应用级 DuckDB（analytics.duckdb）       └── 用户连接的 DuckDB
 * └── 项目级 DuckDB（analytics.duckdb）
 * ```
 *
 * ## 与 SmartPool 的区别
 *
 * | 维度         | SmartPool                        | StandardPool                    |
 * |-------------|----------------------------------|---------------------------------|
 * | 管理对象     | 系统内置库（RdataStation 自身依赖） | 用户数据源（用户外部数据库）      |
 * | 生命周期     | 应用/项目级别（启动→关闭）          | 连接级别（连接→断开）            |
 * | 配置方式     | 应用开发者硬编码                    | 用户在连接页面手动设置            |
 * | 失败处理     | 系统级故障（需要立即告警）           | 用户级故障（提示并允许重试）       |
 * | 动态扩缩容   | ✅ 延迟感知 + 内存压力感知           | ❌ 固定大小（用户配置）            |
 *
 * ## 用户可配置参数
 *
 * | 参数               | 默认值 | 说明                       |
 * |-------------------|--------|---------------------------|
 * | `min_connections` | 2      | 最小保持连接数               |
 * | `max_connections` | 20     | 最大连接数上限               |
 * | `idle_timeout_secs`| 600   | 空闲连接回收时限（秒）         |
 * | `max_lifetime_secs`| 1800  | 连接最大生命周期（秒）         |
 * | `acquire_timeout_secs`| 30 | 获取连接超时（秒）            |
 * | `health_check_enabled`| true| 是否启用健康检查              |
 *
 * ## 前端连接页面配置入口
 *
 * 前端连接配置表单中增加「连接池」配置区域，用户可为每个连接独立设置参数。
 * 配置通过 `StandardPoolConfig` 结构体传递到后端。
 */
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::core::driver::traits::{Database, DbPool, PoolStatus};
use crate::core::error::{ConnectionError, CoreError};

/// 标准连接池用户可配置参数
///
/// 通过前端连接页面表单传递，支持 JSON 序列化
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StandardPoolConfig {
    /// 最小连接数
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    /// 最大连接数
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    /// 空闲超时（秒）
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u32,
    /// 连接最大生命周期（秒）
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime_secs: u32,
    /// 获取连接超时（秒）
    #[serde(default = "default_acquire_timeout")]
    pub acquire_timeout_secs: u32,
    /// 是否启用健康检查
    #[serde(default = "default_health_check")]
    pub health_check_enabled: bool,
}

fn default_min_connections() -> u32 {
    2
}
fn default_max_connections() -> u32 {
    20
}
fn default_idle_timeout() -> u32 {
    600
}
fn default_max_lifetime() -> u32 {
    1800
}
fn default_acquire_timeout() -> u32 {
    30
}
fn default_health_check() -> bool {
    true
}

impl Default for StandardPoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 2,
            max_connections: 20,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
            acquire_timeout_secs: 30,
            health_check_enabled: true,
        }
    }
}

impl StandardPoolConfig {
    /// 创建针对 SQLite 的推荐配置（本地文件，连接开销低）
    pub fn for_sqlite() -> Self {
        Self {
            min_connections: 1,
            max_connections: 5,
            idle_timeout_secs: 300,
            max_lifetime_secs: 3600,
            acquire_timeout_secs: 10,
            health_check_enabled: true,
        }
    }

    /// 创建针对 DuckDB 的推荐配置（单写入者模型，连接数应保持为 1）
    pub fn for_duckdb() -> Self {
        Self {
            min_connections: 1,
            max_connections: 1,
            idle_timeout_secs: 1800,
            max_lifetime_secs: 7200,
            acquire_timeout_secs: 30,
            health_check_enabled: true,
        }
    }

    /// 创建针对网络数据库的推荐配置（MySQL/PostgreSQL）
    pub fn for_network() -> Self {
        Self {
            min_connections: 2,
            max_connections: 20,
            idle_timeout_secs: 600,
            max_lifetime_secs: 1800,
            acquire_timeout_secs: 30,
            health_check_enabled: true,
        }
    }
}

/// 标准连接池统计信息
#[derive(Debug, Clone, Default, Serialize)]
pub struct StandardPoolStats {
    /// 池大小
    pub size: usize,
    /// 空闲连接数
    pub idle: usize,
    /// 活跃连接数
    pub active: usize,
    /// 等待获取的请求数
    pub waiting: usize,
    /// 累计获取次数
    pub total_acquires: u64,
    /// 累计释放次数
    pub total_releases: u64,
    /// 健康检查失败次数
    pub health_check_failures: u64,
}

/// 标准连接池包装器
///
/// 包装底层 DbPool 实现，提供用户可配置的参数管理和统计。
/// 底层连接复用由具体驱动（sqlx::Pool / rusqlite / duckdb-rs）负责。
pub struct StandardPool {
    /// 底层连接池
    inner: Arc<dyn DbPool>,
    /// 用户配置
    config: StandardPoolConfig,
    /// 池统计
    stats: RwLock<StandardPoolStats>,
    /// 池名称（用于日志）
    name: String,
    /// 是否已关闭
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl StandardPool {
    /// 创建新的标准连接池
    ///
    /// # Arguments
    /// * `name` - 池名称（通常为 conn_id）
    /// * `config` - 用户配置的连接池参数
    /// * `inner` - 底层 DbPool 实现
    pub fn new(
        name: impl Into<String>,
        config: StandardPoolConfig,
        inner: Arc<dyn DbPool>,
    ) -> Self {
        let name = name.into();
        info!(
            pool_name = %name,
            min_conn = config.min_connections,
            max_conn = config.max_connections,
            idle_timeout = config.idle_timeout_secs,
            "StandardPool created"
        );

        Self {
            inner,
            config,
            stats: RwLock::new(StandardPoolStats::default()),
            name,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 创建带默认配置的标准连接池（SQLite 本地文件优化）
    pub fn for_sqlite(name: impl Into<String>, inner: Arc<dyn DbPool>) -> Self {
        Self::new(name, StandardPoolConfig::for_sqlite(), inner)
    }

    /// 创建带默认配置的标准连接池（DuckDB 本地文件优化）
    pub fn for_duckdb(name: impl Into<String>, inner: Arc<dyn DbPool>) -> Self {
        Self::new(name, StandardPoolConfig::for_duckdb(), inner)
    }

    /// 创建带默认配置的标准连接池（网络数据库）
    pub fn for_network(name: impl Into<String>, inner: Arc<dyn DbPool>) -> Self {
        Self::new(name, StandardPoolConfig::for_network(), inner)
    }

    /// 获取池配置
    pub fn config(&self) -> &StandardPoolConfig {
        &self.config
    }

    /// 获取池名称
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 获取池统计信息
    pub async fn stats(&self) -> StandardPoolStats {
        self.stats.read().await.clone()
    }

    /// 获取底层 DbPool（用于传递给需要 DbPool 的接口）
    pub fn inner_pool(&self) -> Arc<dyn DbPool> {
        self.inner.clone()
    }
}

#[async_trait::async_trait]
impl DbPool for StandardPool {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::connection(ConnectionError::PoolClosed));
        }

        let mut stats = self.stats.write().await;
        stats.total_acquires += 1;
        drop(stats);

        debug!(pool_name = %self.name, "StandardPool acquiring connection");

        self.inner.acquire().await
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        info!(pool_name = %self.name, "StandardPool closing");
        self.inner.close().await
    }

    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn status(&self) -> PoolStatus {
        let inner_status = self.inner.status();
        let rt = tokio::runtime::Handle::try_current();

        if let Ok(handle) = rt {
            handle.block_on(async {
                let mut stats = self.stats.write().await;
                stats.size = inner_status.size;
                stats.idle = inner_status.idle;
                stats.active = inner_status.active;
                stats.waiting = inner_status.waiting;
            });
        }

        inner_status
    }
}

/// 标准连接池构建器
///
/// 提供流式 Builder API 用于创建 StandardPool
pub struct StandardPoolBuilder {
    name: String,
    config: StandardPoolConfig,
}

impl StandardPoolBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            config: StandardPoolConfig::default(),
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

    pub fn idle_timeout_secs(mut self, secs: u32) -> Self {
        self.config.idle_timeout_secs = secs;
        self
    }

    pub fn max_lifetime_secs(mut self, secs: u32) -> Self {
        self.config.max_lifetime_secs = secs;
        self
    }

    pub fn acquire_timeout_secs(mut self, secs: u32) -> Self {
        self.config.acquire_timeout_secs = secs;
        self
    }

    pub fn health_check(mut self, enabled: bool) -> Self {
        self.config.health_check_enabled = enabled;
        self
    }

    /// 使用构建器配置创建 StandardPool
    pub fn build(self, inner: Arc<dyn DbPool>) -> StandardPool {
        StandardPool::new(self.name, self.config, inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct MockDbPool {
        closed: std::sync::atomic::AtomicBool,
    }

    impl MockDbPool {
        fn new() -> Self {
            Self {
                closed: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl DbPool for MockDbPool {
        async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
            Err(CoreError::connection(ConnectionError::PoolClosed))
        }

        async fn close(&self) -> Result<(), CoreError> {
            Ok(())
        }

        fn is_closed(&self) -> bool {
            self.closed.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn status(&self) -> PoolStatus {
            PoolStatus {
                size: 5,
                idle: 3,
                active: 2,
                waiting: 0,
                max_connections: 10,
                min_connections: 2,
            }
        }
    }

    #[test]
    fn test_standard_pool_config_defaults() {
        let config = StandardPoolConfig::default();
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.idle_timeout_secs, 600);
    }

    #[test]
    fn test_sqlite_config() {
        let config = StandardPoolConfig::for_sqlite();
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.min_connections, 1);
    }

    #[test]
    fn test_duckdb_config_single_connection() {
        let config = StandardPoolConfig::for_duckdb();
        assert_eq!(config.max_connections, 1);
        assert_eq!(config.min_connections, 1);
    }

    #[test]
    fn test_builder() {
        let config = StandardPoolBuilder::new("test")
            .min_connections(5)
            .max_connections(50)
            .idle_timeout_secs(300)
            .build(Arc::new(MockDbPool::new()))
            .config()
            .clone();

        assert_eq!(config.min_connections, 5);
        assert_eq!(config.max_connections, 50);
        assert_eq!(config.idle_timeout_secs, 300);
    }

    #[tokio::test]
    async fn test_standard_pool_creation() {
        let pool = StandardPool::for_network("test_pool", Arc::new(MockDbPool::new()));
        assert_eq!(pool.name(), "test_pool");
        assert!(!pool.is_closed());
        assert!(pool.acquire().await.is_err()); // Mock returns PoolClosed
    }
}
