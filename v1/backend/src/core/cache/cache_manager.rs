//! 缓存管理器
//!
//! 统一管理所有缓存实例，提供多级缓存支持

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{CacheStats, MetadataCache, QueryCache, QueryCacheConfig, QueryCacheStats};

/// 缓存级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheLevel {
    /// L1: 内存缓存（进程内）
    L1,
    /// L2: 共享内存缓存（进程间）
    L2,
    /// L3: 磁盘缓存
    L3,
}

/// 缓存配置
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// 是否启用 L1 缓存
    pub l1_enabled: bool,
    /// L1 缓存容量
    pub l1_capacity: usize,
    /// 是否启用 L2 缓存
    pub l2_enabled: bool,
    /// L2 缓存容量
    pub l2_capacity: usize,
    /// 是否启用 L3 缓存
    pub l3_enabled: bool,
    /// L3 缓存路径
    pub l3_path: String,
    /// 默认 TTL
    pub default_ttl: Duration,
    /// 是否启用查询缓存
    pub query_cache_enabled: bool,
    /// 查询缓存最大条目数
    pub query_cache_max_entries: usize,
    /// 查询缓存默认 TTL
    pub query_cache_default_ttl: Duration,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_enabled: true,
            l1_capacity: 1000,
            l2_enabled: false, // 默认不启用 L2
            l2_capacity: 10000,
            l3_enabled: false, // 默认不启用 L3
            l3_path: "./cache".to_string(),
            default_ttl: Duration::from_secs(600),
            query_cache_enabled: true,
            query_cache_max_entries: 1000,
            query_cache_default_ttl: Duration::from_secs(600),
        }
    }
}

/// 缓存管理器
///
/// 管理所有缓存实例，提供统一的缓存访问接口
pub struct CacheManager {
    /// L1 元数据缓存
    l1_metadata: Arc<Mutex<MetadataCache>>,
    /// 查询缓存
    query_cache: Arc<QueryCache>,
    /// 配置
    config: CacheConfig,
}

impl CacheManager {
    /// 创建新的缓存管理器
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// 创建带配置的缓存管理器
    pub fn with_config(config: CacheConfig) -> Self {
        let l1_metadata = Arc::new(Mutex::new(MetadataCache::with_ttl(
            config.l1_capacity,
            config.default_ttl,
        )));

        let query_cache_config = QueryCacheConfig {
            max_entries: config.query_cache_max_entries,
            default_ttl: config.query_cache_default_ttl,
            enable_stats: true,
        };

        let query_cache = Arc::new(QueryCache::new(Some(query_cache_config)));

        Self {
            l1_metadata,
            query_cache,
            config,
        }
    }

    /// 获取单例实例
    pub fn instance() -> Arc<Mutex<Self>> {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<Arc<Mutex<CacheManager>>> = OnceLock::new();

        INSTANCE
            .get_or_init(|| Arc::new(Mutex::new(CacheManager::new())))
            .clone()
    }

    // ==================== 元数据缓存接口 ====================

    /// 获取 L1 元数据缓存
    pub fn metadata_cache(&self) -> Arc<Mutex<MetadataCache>> {
        self.l1_metadata.clone()
    }

    // ==================== 查询缓存接口 ====================

    /// 获取查询缓存
    pub fn query_cache(&self) -> Arc<QueryCache> {
        self.query_cache.clone()
    }

    /// 清除指定连接的所有缓存
    pub fn invalidate_connection(&self, conn_id: &str) {
        // 清除元数据缓存
        if let Ok(mut cache) = self.l1_metadata.lock() {
            cache.invalidate_connection(conn_id);
        }

        // 清除查询缓存（仅在 Tokio runtime 可用时异步执行）
        if tokio::runtime::Handle::try_current().is_ok() {
            let query_cache = self.query_cache.clone();
            let conn_id = conn_id.to_string();
            tokio::spawn(async move {
                let _ = query_cache.clear_by_connection(&conn_id).await;
            });
        }
    }

    /// 清除所有缓存
    pub fn clear_all(&self) {
        // 清除元数据缓存
        if let Ok(mut cache) = self.l1_metadata.lock() {
            cache.clear();
        }

        // 清除查询缓存（仅在 Tokio runtime 可用时异步执行）
        if tokio::runtime::Handle::try_current().is_ok() {
            let query_cache = self.query_cache.clone();
            tokio::spawn(async move {
                let _ = query_cache.clear().await;
            });
        }
    }

    /// 获取所有缓存统计信息
    pub fn stats(&self) -> CacheManagerStats {
        let metadata_stats = self
            .l1_metadata
            .lock()
            .map(|cache| cache.stats().clone())
            .unwrap_or_default();

        // 获取查询缓存统计信息（使用默认值，避免在同步上下文中创建运行时）
        let query_stats = QueryCacheStats::default();

        CacheManagerStats {
            metadata: metadata_stats,
            query: query_stats,
        }
    }

    /// 清理过期缓存
    pub fn cleanup_expired(&self) -> usize {
        // 清理元数据缓存
        let metadata_cleaned = self
            .l1_metadata
            .lock()
            .map(|mut cache| cache.cleanup_expired())
            .unwrap_or(0);

        // 查询缓存清理需要异步上下文，这里返回元数据清理数量
        metadata_cleaned
    }

    /// 获取配置
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// 更新配置
    pub fn update_config(&mut self, config: CacheConfig) {
        self.config = config;
        // 重新创建元数据缓存
        self.l1_metadata = Arc::new(Mutex::new(MetadataCache::with_ttl(
            self.config.l1_capacity,
            self.config.default_ttl,
        )));

        // 重新创建查询缓存
        let query_cache_config = QueryCacheConfig {
            max_entries: self.config.query_cache_max_entries,
            default_ttl: self.config.query_cache_default_ttl,
            enable_stats: true,
        };
        self.query_cache = Arc::new(QueryCache::new(Some(query_cache_config)));
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 缓存管理器统计信息
#[derive(Debug, Clone, Default)]
pub struct CacheManagerStats {
    /// 元数据缓存统计
    pub metadata: CacheStats,
    /// 查询缓存统计
    pub query: QueryCacheStats,
}

impl CacheManagerStats {
    /// 获取总命中率
    pub fn overall_hit_rate(&self) -> f64 {
        let total_hits = self.metadata.hits + self.query.hits;
        let total_accesses = self.total_accesses();

        if total_accesses == 0 {
            0.0
        } else {
            total_hits as f64 / total_accesses as f64
        }
    }

    /// 获取总访问次数
    pub fn total_accesses(&self) -> u64 {
        self.metadata.total_accesses() + self.query.hits + self.query.misses
    }

    /// 获取元数据缓存命中率
    pub fn metadata_hit_rate(&self) -> f64 {
        self.metadata.hit_rate()
    }

    /// 获取查询缓存命中率
    pub fn query_hit_rate(&self) -> f64 {
        self.query.hit_rate()
    }
}

/// 缓存管理器构建器
pub struct CacheManagerBuilder {
    config: CacheConfig,
}

impl CacheManagerBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            config: CacheConfig::default(),
        }
    }

    /// 设置 L1 容量
    pub fn l1_capacity(mut self, capacity: usize) -> Self {
        self.config.l1_capacity = capacity;
        self
    }

    /// 启用 L2 缓存
    pub fn enable_l2(mut self, capacity: usize) -> Self {
        self.config.l2_enabled = true;
        self.config.l2_capacity = capacity;
        self
    }

    /// 启用 L3 缓存
    pub fn enable_l3(mut self, path: impl Into<String>) -> Self {
        self.config.l3_enabled = true;
        self.config.l3_path = path.into();
        self
    }

    /// 设置默认 TTL
    pub fn default_ttl(mut self, ttl: Duration) -> Self {
        self.config.default_ttl = ttl;
        self
    }

    /// 启用或禁用查询缓存
    pub fn query_cache_enabled(mut self, enabled: bool) -> Self {
        self.config.query_cache_enabled = enabled;
        self
    }

    /// 设置查询缓存最大条目数
    pub fn query_cache_max_entries(mut self, max_entries: usize) -> Self {
        self.config.query_cache_max_entries = max_entries;
        self
    }

    /// 设置查询缓存默认 TTL
    pub fn query_cache_default_ttl(mut self, ttl: Duration) -> Self {
        self.config.query_cache_default_ttl = ttl;
        self
    }

    /// 构建缓存管理器
    pub fn build(self) -> CacheManager {
        CacheManager::with_config(self.config)
    }
}

impl Default for CacheManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_manager() {
        let manager = CacheManagerBuilder::new()
            .l1_capacity(100)
            .default_ttl(Duration::from_secs(60))
            .build();

        assert_eq!(manager.config().l1_capacity, 100);
        assert_eq!(manager.config().default_ttl, Duration::from_secs(60));
    }

    #[test]
    fn test_cache_manager_stats() {
        let manager = CacheManager::new();
        let stats = manager.stats();

        assert_eq!(stats.overall_hit_rate(), 0.0);
        assert_eq!(stats.total_accesses(), 0);
    }
}
