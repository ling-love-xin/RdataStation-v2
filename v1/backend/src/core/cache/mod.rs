//! 缓存模块
//!
//! 提供多级缓存机制，用于元数据、查询结果等数据的缓存

pub mod cache_manager;
pub mod lru_cache;
pub mod memory_guard;
pub mod metadata_cache;
pub mod minicatalogs;
pub mod query_cache;

pub use cache_manager::{CacheConfig, CacheLevel, CacheManager, CacheManagerStats};
pub use lru_cache::{LruCache, MemoryEstimate, MemoryPressure};
pub use memory_guard::{get_memory_guard, init_memory_guard, MemoryGuard, MemoryGuardConfig};
pub use metadata_cache::{
    MetadataCache, MetadataCacheConfig, MetadataCacheKey, MetadataCacheValue,
};
pub use query_cache::{get_query_cache, QueryCache, QueryCacheConfig, QueryCacheStats};

use std::hash::Hash;
use std::time::{Duration, Instant};

/// 缓存条目
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    /// 缓存值
    pub value: V,
    /// 创建时间
    pub created_at: Instant,
    /// 过期时间
    pub expires_at: Option<Instant>,
    /// 访问次数
    pub access_count: u64,
}

impl<V> CacheEntry<V> {
    /// 创建新的缓存条目
    pub fn new(value: V, ttl: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            expires_at: ttl.map(|d| now + d),
            access_count: 0,
        }
    }

    /// 检查是否过期
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| Instant::now() > exp)
    }

    /// 记录访问
    pub fn record_access(&mut self) {
        self.access_count += 1;
    }
}

/// 缓存统计信息
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// 命中次数
    pub hits: u64,
    /// 未命中次数
    pub misses: u64,
    /// 驱逐次数
    pub evictions: u64,
    /// 当前条目数
    pub entry_count: usize,
    /// 当前大小（字节）
    pub size_bytes: usize,
}

impl CacheStats {
    /// 获取命中率
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// 获取总访问次数
    pub fn total_accesses(&self) -> u64 {
        self.hits + self.misses
    }
}

/// 缓存键 trait
pub trait CacheKey: Hash + Eq + Clone + Send + Sync + 'static {}
impl<T: Hash + Eq + Clone + Send + Sync + 'static> CacheKey for T {}

/// 缓存值 trait
pub trait CacheValue: Clone + Send + Sync + 'static {}
impl<T: Clone + Send + Sync + 'static> CacheValue for T {}

/// 缓存策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// 永不过期
    NeverExpire,
    /// 固定过期时间
    FixedTTL(Duration),
    /// 滑动过期时间（每次访问后重置）
    SlidingTTL(Duration),
    /// 基于访问频率（LRU）
    LRU(usize),
    /// 基于访问频率（LFU）
    LFU(usize),
}

impl Default for CachePolicy {
    fn default() -> Self {
        CachePolicy::FixedTTL(Duration::from_secs(300)) // 默认 5 分钟
    }
}
