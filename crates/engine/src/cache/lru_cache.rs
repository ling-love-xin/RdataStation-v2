//! LRU 缓存实现
//!
//! 基于访问时间的最近最少使用缓存，支持内存压力感知淘汰

use std::collections::HashMap;
use std::time::Duration;

use super::{CacheEntry, CacheKey, CachePolicy, CacheStats, CacheValue};

/// 内存估算 trait
pub trait MemoryEstimate {
    /// 估算内存使用量（字节）
    fn estimate_memory_bytes(&self) -> usize;
}

/// 内存压力级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// 正常状态
    Normal,
    /// 中等压力（开始淘汰缓存）
    Moderate,
    /// 高压力（激进淘汰）
    Critical,
}

impl MemoryPressure {
    /// 根据系统内存使用情况评估压力级别
    #[cfg(target_os = "linux")]
    pub fn detect() -> Self {
        if let Ok(mem_info) = std::fs::read_to_string("/proc/meminfo") {
            for line in mem_info.lines() {
                if line.starts_with("MemAvailable:") {
                    if let Some(available_kb) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        let available_mb = available_kb / 1024;
                        return if available_mb < 512 {
                            MemoryPressure::Critical
                        } else if available_mb < 1024 {
                            MemoryPressure::Moderate
                        } else {
                            MemoryPressure::Normal
                        };
                    }
                }
            }
        }
        MemoryPressure::Normal
    }

    #[cfg(target_os = "macos")]
    pub fn detect() -> Self {
        use std::process::Command;
        let output = Command::new("vm_stat").output().ok();

        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("Pages free") {
                    if let Some(pages) = line
                        .split_whitespace()
                        .last()
                        .and_then(|s| s.trim_end_matches('.').parse::<u64>().ok())
                    {
                        let free_mb = (pages * 4096) / (1024 * 1024);
                        return if free_mb < 512 {
                            MemoryPressure::Critical
                        } else if free_mb < 1024 {
                            MemoryPressure::Moderate
                        } else {
                            MemoryPressure::Normal
                        };
                    }
                }
            }
        }
        MemoryPressure::Normal
    }

    #[cfg(target_os = "windows")]
    pub fn detect() -> Self {
        // Windows 平台使用简单启发式，默认正常状态
        // 后续可通过 sysinfo crate 获取更准确的内存信息
        MemoryPressure::Normal
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    pub fn detect() -> Self {
        MemoryPressure::Normal
    }

    /// 获取淘汰比例（根据压力级别）
    pub fn eviction_ratio(&self) -> f64 {
        match self {
            MemoryPressure::Normal => 0.0,
            MemoryPressure::Moderate => 0.25,
            MemoryPressure::Critical => 0.5,
        }
    }
}

/// LRU 缓存
///
/// 基于 HashMap + 访问顺序列表实现
#[allow(dead_code)]
pub struct LruCache<K, V> {
    /// 容量
    capacity: usize,
    /// 存储
    map: HashMap<K, CacheEntry<V>>,
    /// 访问顺序列表（最近访问的在前面）
    access_order: Vec<K>,
    /// 统计信息
    stats: CacheStats,
    /// 缓存策略
    policy: CachePolicy,
}

impl<K: CacheKey, V: CacheValue> LruCache<K, V> {
    /// 创建新的 LRU 缓存
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            map: HashMap::with_capacity(capacity),
            access_order: Vec::with_capacity(capacity),
            stats: CacheStats::default(),
            policy: CachePolicy::LRU(capacity),
        }
    }

    /// 创建带策略的缓存
    pub fn with_policy(capacity: usize, policy: CachePolicy) -> Self {
        Self {
            capacity,
            map: HashMap::with_capacity(capacity),
            access_order: Vec::with_capacity(capacity),
            stats: CacheStats::default(),
            policy,
        }
    }

    /// 获取所有键
    pub fn keys(&self) -> Vec<K> {
        self.map.keys().cloned().collect()
    }

    /// 获取值（简化版，避免借用冲突）
    pub fn get(&mut self, key: &K) -> Option<V> {
        // 检查是否存在且未过期
        let is_valid = self
            .map
            .get(key)
            .map(|entry| !entry.is_expired())
            .unwrap_or(false);

        if is_valid {
            // 更新访问顺序
            self.update_access_order(key.clone());

            // 获取值并增加访问计数
            if let Some(entry) = self.map.get_mut(key) {
                entry.record_access();
                self.stats.hits += 1;
                return Some(entry.value.clone());
            }
        } else if self.map.contains_key(key) {
            // 已过期，移除
            self.remove(key);
        }

        self.stats.misses += 1;
        None
    }

    /// 插入值
    pub fn put(&mut self, key: K, value: V) {
        self.put_with_ttl(key, value, None);
    }

    /// 插入值并指定 TTL
    pub fn put_with_ttl(&mut self, key: K, value: V, ttl: Option<Duration>) {
        // 如果已存在，更新值
        if self.map.contains_key(&key) {
            let entry = CacheEntry::new(value, ttl);
            self.map.insert(key.clone(), entry);
            self.update_access_order(key);
            return;
        }

        // 如果容量已满，驱逐最久未使用的
        if self.map.len() >= self.capacity {
            self.evict_lru();
        }

        // 插入新值
        let entry = CacheEntry::new(value, ttl);
        self.map.insert(key.clone(), entry);
        self.access_order.push(key);

        self.stats.entry_count = self.map.len();
    }

    /// 移除值
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(entry) = self.map.remove(key) {
            self.access_order.retain(|k| k != key);
            self.stats.entry_count = self.map.len();
            Some(entry.value)
        } else {
            None
        }
    }

    /// 检查是否包含键
    pub fn contains_key(&self, key: &K) -> bool {
        if let Some(entry) = self.map.get(key) {
            !entry.is_expired()
        } else {
            false
        }
    }

    /// 获取当前大小
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 清空缓存
    pub fn clear(&mut self) {
        self.map.clear();
        self.access_order.clear();
        self.stats = CacheStats::default();
    }

    /// 获取统计信息
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// 获取容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 设置容量
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        // 如果新容量小于当前大小，驱逐多余条目
        while self.map.len() > self.capacity {
            self.evict_lru();
        }
    }

    /// 清理过期条目
    pub fn cleanup_expired(&mut self) -> usize {
        let expired_keys: Vec<K> = self
            .map
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        let count = expired_keys.len();
        for key in expired_keys {
            self.remove(&key);
        }

        count
    }

    /// 内存压力感知淘汰
    ///
    /// 根据系统内存压力自动淘汰缓存条目
    pub fn memory_pressure_eviction(&mut self) -> usize {
        let pressure = MemoryPressure::detect();
        let ratio = pressure.eviction_ratio();

        if ratio == 0.0 {
            return 0;
        }

        let to_evict = (self.map.len() as f64 * ratio).ceil() as usize;
        let mut evicted = 0;

        for _ in 0..to_evict {
            if self.access_order.is_empty() {
                break;
            }

            if let Some(key) = self.access_order.first().cloned() {
                self.remove(&key);
                evicted += 1;
            }
        }

        evicted
    }

    /// 强制淘汰指定比例的缓存
    ///
    /// # 参数
    /// * `ratio` - 淘汰比例 (0.0 - 1.0)
    pub fn force_evict(&mut self, ratio: f64) -> usize {
        let ratio = ratio.clamp(0.0, 1.0);
        let to_evict = (self.map.len() as f64 * ratio).ceil() as usize;
        let mut evicted = 0;

        for _ in 0..to_evict {
            if self.access_order.is_empty() {
                break;
            }

            if let Some(key) = self.access_order.first().cloned() {
                self.remove(&key);
                evicted += 1;
            }
        }

        evicted
    }

    /// 获取缓存内存使用估算（字节）
    ///
    /// 注意：这是粗略估算，不包含内部结构开销
    pub fn estimated_memory_usage(&self) -> usize
    where
        V: MemoryEstimate,
    {
        let entry_overhead = std::mem::size_of::<CacheEntry<V>>();
        let map_overhead = self.map.len() * (std::mem::size_of::<K>() + entry_overhead);
        let value_size = self
            .map
            .values()
            .map(|entry| entry.value.estimate_memory_bytes())
            .sum::<usize>();

        map_overhead + value_size
    }

    /// 更新访问顺序
    fn update_access_order(&mut self, key: K) {
        // 移除旧位置
        self.access_order.retain(|k| k != &key);
        // 添加到最前面（最近访问）
        self.access_order.push(key);
    }

    /// 驱逐最久未使用的条目
    fn evict_lru(&mut self) {
        if let Some(key) = self.access_order.first().cloned() {
            self.remove(&key);
            self.stats.evictions += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_basic() {
        let mut cache = LruCache::new(3);

        cache.put("a", 1);
        cache.put("b", 2);
        cache.put("c", 3);

        assert_eq!(cache.get(&"a"), Some(1));
        assert_eq!(cache.get(&"b"), Some(2));
        assert_eq!(cache.get(&"c"), Some(3));

        // 插入第四个，应该驱逐 "a"
        cache.put("d", 4);

        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"b"), Some(2));
        assert_eq!(cache.get(&"c"), Some(3));
        assert_eq!(cache.get(&"d"), Some(4));
    }

    #[test]
    fn test_lru_cache_access_order() {
        let mut cache = LruCache::new(3);

        cache.put("a", 1);
        cache.put("b", 2);
        cache.put("c", 3);

        // 访问 "a"，使其变为最近使用
        cache.get(&"a");

        // 插入 "d"，应该驱逐 "b" 而不是 "a"
        cache.put("d", 4);

        assert_eq!(cache.get(&"a"), Some(1));
        assert_eq!(cache.get(&"b"), None);
        assert_eq!(cache.get(&"c"), Some(3));
        assert_eq!(cache.get(&"d"), Some(4));
    }

    #[test]
    fn test_lru_cache_ttl() {
        let mut cache = LruCache::new(3);

        // 插入带 TTL 的值
        cache.put_with_ttl("a", 1, Some(Duration::from_millis(50)));

        assert_eq!(cache.get(&"a"), Some(1));

        // 等待过期
        std::thread::sleep(Duration::from_millis(100));

        assert_eq!(cache.get(&"a"), None);
    }
}
