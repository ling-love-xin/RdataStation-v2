//! 内存防护模块
//!
//! 提供统一的内存管理服务，包括：
//! - 内存压力监控
//! - 缓存自动淘汰
//! - 连接池内存限制
//! - 内存使用统计

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::{CacheManager, MemoryPressure};

/// 内存防护配置
#[derive(Debug, Clone)]
pub struct MemoryGuardConfig {
    /// 是否启用内存防护
    pub enabled: bool,
    /// 内存压力检查间隔
    pub check_interval: Duration,
    /// 缓存淘汰触发阈值（0.0 - 1.0）
    pub eviction_threshold: f64,
    /// 最大缓存条目数
    pub max_cache_entries: usize,
    /// 最大连接池大小
    pub max_pool_size: usize,
    /// 内存压力警告阈值（MB）
    pub warning_threshold_mb: u64,
    /// 内存压力临界阈值（MB）
    pub critical_threshold_mb: u64,
}

impl Default for MemoryGuardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: Duration::from_secs(30),
            eviction_threshold: 0.8,
            max_cache_entries: 10000,
            max_pool_size: 20,
            warning_threshold_mb: 2048,
            critical_threshold_mb: 4096,
        }
    }
}

/// 内存使用统计
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// 当前缓存条目数
    pub cache_entries: usize,
    /// 当前连接池大小
    pub pool_size: usize,
    /// 估算内存使用（MB）
    pub estimated_memory_mb: f64,
    /// 上次淘汰时间
    pub last_eviction: Option<Instant>,
    /// 淘汰总数
    pub total_evictions: u64,
    /// 内存压力级别
    pub pressure_level: String,
}

/// 内存防护管理器
///
/// 负责监控和管理整个应用的内存使用
#[allow(dead_code)]
pub struct MemoryGuard {
    /// 配置
    config: MemoryGuardConfig,
    /// 缓存管理器引用
    cache_manager: Arc<RwLock<CacheManager>>,
    /// 内存统计
    stats: RwLock<MemoryStats>,
    /// 上次检查时间
    last_check: RwLock<Instant>,
}

impl MemoryGuard {
    /// 创建新的内存防护管理器
    pub fn new(config: MemoryGuardConfig, cache_manager: Arc<RwLock<CacheManager>>) -> Self {
        Self {
            config,
            cache_manager,
            stats: RwLock::new(MemoryStats::default()),
            last_check: RwLock::new(Instant::now()),
        }
    }

    /// 创建默认配置的内存防护管理器
    pub fn default_with_cache_manager(cache_manager: Arc<RwLock<CacheManager>>) -> Self {
        Self::new(MemoryGuardConfig::default(), cache_manager)
    }

    /// 检查内存压力并执行淘汰
    pub async fn check_and_evict(&self) -> Result<usize, String> {
        if !self.config.enabled {
            return Ok(0);
        }

        let pressure = MemoryPressure::detect();
        let mut stats = self.stats.write().await;
        stats.pressure_level = format!("{:?}", pressure);

        let mut evicted = 0;

        match pressure {
            MemoryPressure::Normal => {
                // 正常状态，仅检查缓存大小
                let cache = self.cache_manager.read().await;
                let cache_stats = cache.stats();

                if cache_stats.metadata.entry_count > self.config.max_cache_entries {
                    warn!(
                        "Cache size {} exceeds limit {}, triggering eviction",
                        cache_stats.metadata.entry_count, self.config.max_cache_entries
                    );

                    drop(cache);
                    evicted = self.evict_cache_entries(0.2).await?;
                }
            }
            MemoryPressure::Moderate => {
                // 中等压力，淘汰 25% 缓存
                info!("Moderate memory pressure detected, evicting 25% cache");
                evicted = self.evict_cache_entries(0.25).await?;
                stats.last_eviction = Some(Instant::now());
            }
            MemoryPressure::Critical => {
                // 高压力，淘汰 50% 缓存
                warn!("Critical memory pressure detected, evicting 50% cache");
                evicted = self.evict_cache_entries(0.5).await?;
                stats.last_eviction = Some(Instant::now());
            }
        }

        stats.total_evictions += evicted as u64;

        Ok(evicted)
    }

    /// 淘汰指定比例的缓存条目
    async fn evict_cache_entries(&self, ratio: f64) -> Result<usize, String> {
        let cache = self.cache_manager.write().await;

        // 清理过期条目
        let expired_cleaned = cache.cleanup_expired();
        debug!("Cleaned {} expired cache entries", expired_cleaned);

        // 强制淘汰指定比例
        // 注意：这里需要通过 metadata_cache 的 lock 获取可变引用
        if let Ok(mut metadata_cache) = cache.metadata_cache().lock() {
            let evicted = metadata_cache.force_evict(ratio);
            debug!("Evicted {} cache entries (ratio: {})", evicted, ratio);
            Ok(evicted + expired_cleaned)
        } else {
            Err("Failed to acquire metadata cache lock".to_string())
        }
    }

    /// 获取当前内存统计
    pub async fn get_stats(&self) -> MemoryStats {
        let stats = self.stats.read().await.clone();
        stats
    }

    /// 更新配置
    pub fn update_config(&mut self, config: MemoryGuardConfig) {
        self.config = config;
    }

    /// 获取配置
    pub fn config(&self) -> &MemoryGuardConfig {
        &self.config
    }

    /// 启动定期内存检查任务
    pub fn start_periodic_check(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let interval = self.config.check_interval;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;

                match self.check_and_evict().await {
                    Ok(evicted) => {
                        if evicted > 0 {
                            info!("Periodic memory check: evicted {} entries", evicted);
                        }
                    }
                    Err(e) => {
                        warn!("Periodic memory check failed: {}", e);
                    }
                }
            }
        })
    }
}

/// 全局内存防护实例
use std::sync::OnceLock;

static MEMORY_GUARD: OnceLock<Arc<MemoryGuard>> = OnceLock::new();

/// 获取全局内存防护实例
pub fn get_memory_guard() -> Option<&'static Arc<MemoryGuard>> {
    MEMORY_GUARD.get()
}

/// 初始化全局内存防护实例
pub fn init_memory_guard(
    config: MemoryGuardConfig,
    cache_manager: Arc<RwLock<CacheManager>>,
) -> &'static Arc<MemoryGuard> {
    MEMORY_GUARD.get_or_init(|| Arc::new(MemoryGuard::new(config, cache_manager)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_guard_config() {
        let config = MemoryGuardConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_cache_entries, 10000);
        assert_eq!(config.max_pool_size, 20);
    }

    #[test]
    fn test_memory_stats() {
        let stats = MemoryStats::default();
        assert_eq!(stats.cache_entries, 0);
        assert_eq!(stats.pool_size, 0);
        assert_eq!(stats.total_evictions, 0);
    }
}
