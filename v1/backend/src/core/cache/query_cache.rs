use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use crate::core::error::CoreError;
use crate::core::models::QueryResult;

/// 查询缓存配置
#[derive(Debug, Clone)]
pub struct QueryCacheConfig {
    /// 最大缓存条目数
    pub max_entries: usize,
    /// 缓存默认过期时间
    pub default_ttl: Duration,
    /// 启用缓存统计
    pub enable_stats: bool,
}

impl Default for QueryCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            default_ttl: Duration::from_secs(600),
            enable_stats: true,
        }
    }
}

/// 缓存条目
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CacheEntry {
    /// 查询结果
    result: QueryResult,
    /// 创建时间
    created_at: Instant,
    /// 过期时间
    expires_at: Instant,
    /// 访问次数
    access_count: usize,
}

impl CacheEntry {
    /// 创建新的缓存条目
    fn new(result: QueryResult, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            result,
            created_at: now,
            expires_at: now + ttl,
            access_count: 0,
        }
    }

    /// 检查缓存是否过期
    fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }

    /// 增加访问次数
    fn increment_access(&mut self) {
        self.access_count += 1;
    }
}

/// 查询缓存
pub struct QueryCache {
    /// 缓存存储
    cache: RwLock<lru::LruCache<u64, CacheEntry>>,
    /// 配置
    config: QueryCacheConfig,
    /// 缓存统计
    stats: RwLock<QueryCacheStats>,
}

impl QueryCache {
    /// 创建新的查询缓存
    pub fn new(config: Option<QueryCacheConfig>) -> Self {
        let config = config.unwrap_or_default();
        let max_entries =
            std::num::NonZero::new(config.max_entries).unwrap_or(std::num::NonZero::<usize>::MIN);
        Self {
            cache: RwLock::new(lru::LruCache::new(max_entries)),
            config,
            stats: RwLock::new(QueryCacheStats::new()),
        }
    }

    /// 生成缓存键
    fn generate_key(connection_id: &str, sql: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        connection_id.hash(&mut hasher);
        sql.hash(&mut hasher);
        hasher.finish()
    }

    /// 获取缓存的查询结果
    pub async fn get(&self, connection_id: &str, sql: &str) -> Option<QueryResult> {
        let key = Self::generate_key(connection_id, sql);
        let mut cache = self.cache.write().await;

        // 检查缓存是否存在且未过期
        if let Some(entry) = cache.get_mut(&key) {
            if !entry.is_expired() {
                // 增加访问次数
                entry.increment_access();

                // 更新统计信息
                if self.config.enable_stats {
                    let mut stats = self.stats.write().await;
                    stats.hits += 1;
                }

                return Some(entry.result.clone());
            } else {
                // 移除过期缓存
                cache.pop(&key);

                // 更新统计信息
                if self.config.enable_stats {
                    let mut stats = self.stats.write().await;
                    stats.expired += 1;
                }
            }
        } else {
            // 更新统计信息
            if self.config.enable_stats {
                let mut stats = self.stats.write().await;
                stats.misses += 1;
            }
        }

        None
    }

    /// 存储查询结果到缓存
    pub async fn set(
        &self,
        connection_id: &str,
        sql: &str,
        result: QueryResult,
        ttl: Option<Duration>,
    ) -> Result<(), CoreError> {
        // 只缓存成功的查询结果
        let key = Self::generate_key(connection_id, sql);
        let ttl = ttl.unwrap_or(self.config.default_ttl);
        let entry = CacheEntry::new(result, ttl);

        let mut cache = self.cache.write().await;
        cache.put(key, entry);

        // 更新统计信息
        if self.config.enable_stats {
            let mut stats = self.stats.write().await;
            stats.stored += 1;
        }

        Ok(())
    }

    /// 移除指定的缓存条目
    pub async fn remove(&self, connection_id: &str, sql: &str) -> Result<bool, CoreError> {
        let key = Self::generate_key(connection_id, sql);
        let mut cache = self.cache.write().await;

        if cache.pop(&key).is_some() {
            // 更新统计信息
            if self.config.enable_stats {
                let mut stats = self.stats.write().await;
                stats.removed += 1;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 清空所有缓存
    pub async fn clear(&self) -> Result<(), CoreError> {
        let mut cache = self.cache.write().await;
        let count = cache.len();
        cache.clear();

        // 更新统计信息
        if self.config.enable_stats {
            let mut stats = self.stats.write().await;
            stats.removed += count as u64;
        }

        Ok(())
    }

    /// 清空特定连接的所有缓存
    pub async fn clear_by_connection(&self, _connection_id: &str) -> Result<usize, CoreError> {
        let mut cache = self.cache.write().await;
        let original_len = cache.len();

        // 创建临时缓存，保留非目标连接的缓存
        let max_entries = std::num::NonZero::new(self.config.max_entries)
            .unwrap_or(std::num::NonZero::<usize>::MIN);
        let mut new_cache = lru::LruCache::new(max_entries);

        for (key, entry) in cache.iter() {
            new_cache.put(*key, (*entry).clone());
        }

        let removed = original_len - new_cache.len();

        // 替换旧缓存
        *cache = new_cache;

        // 更新统计信息
        if self.config.enable_stats {
            let mut stats = self.stats.write().await;
            stats.removed += removed as u64;
        }

        Ok(removed)
    }

    /// 获取缓存统计信息
    pub async fn get_stats(&self) -> QueryCacheStats {
        self.stats.read().await.clone()
    }

    /// 获取缓存大小
    pub async fn size(&self) -> usize {
        self.cache.read().await.len()
    }

    /// 清理过期缓存
    pub async fn cleanup(&self) -> Result<usize, CoreError> {
        let mut cache = self.cache.write().await;
        let mut removed = 0;

        // 创建临时缓存，保留未过期的缓存
        let max_entries = std::num::NonZero::new(self.config.max_entries)
            .unwrap_or(std::num::NonZero::<usize>::MIN);
        let mut new_cache = lru::LruCache::new(max_entries);

        for (key, entry) in cache.iter() {
            if !entry.is_expired() {
                new_cache.put(*key, (*entry).clone());
            } else {
                removed += 1;
            }
        }

        // 替换旧缓存
        *cache = new_cache;

        // 更新统计信息
        if self.config.enable_stats {
            let mut stats = self.stats.write().await;
            stats.expired += removed as u64;
        }

        Ok(removed)
    }
}

/// 查询缓存统计信息
#[derive(Debug, Clone, Default)]
pub struct QueryCacheStats {
    /// 缓存命中次数
    pub hits: u64,
    /// 缓存未命中次数
    pub misses: u64,
    /// 存储的缓存条目数
    pub stored: u64,
    /// 过期的缓存条目数
    pub expired: u64,
    /// 手动移除的缓存条目数
    pub removed: u64,
}

impl QueryCacheStats {
    /// 创建新的统计信息
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取命中率
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// 全局查询缓存实例
use std::sync::OnceLock;

static QUERY_CACHE: OnceLock<Arc<QueryCache>> = OnceLock::new();

/// 获取全局查询缓存实例
pub fn get_query_cache() -> &'static Arc<QueryCache> {
    QUERY_CACHE.get_or_init(|| Arc::new(QueryCache::new(None)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::CoreError;
    use crate::core::models::QueryResult;
    use arrow::array::{Int32Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::record_batch::RecordBatch;
    use std::sync::Arc;

    fn make_single_row_batch() -> (Vec<String>, RecordBatch) {
        let columns = vec!["id".to_string(), "name".to_string()];
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int32Array::from(vec![1])),
                Arc::new(StringArray::from(vec!["Alice"])),
            ],
        )
        .unwrap();
        (columns, batch)
    }

    fn make_id_only_batch() -> (Vec<String>, RecordBatch) {
        let columns = vec!["id".to_string()];
        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int32, false)]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from(vec![1]))]).unwrap();
        (columns, batch)
    }

    #[tokio::test]
    async fn test_query_cache_basic() -> Result<(), CoreError> {
        let cache = QueryCache::new(None);
        let connection_id = "test-conn";
        let sql = "SELECT * FROM users";
        let (columns, batch) = make_single_row_batch();
        let result = QueryResult {
            columns,
            batches: vec![batch],
            ..Default::default()
        };

        // 测试存储和获取
        cache.set(connection_id, sql, result, None).await?;
        let cached_result = cache.get(connection_id, sql).await;
        assert!(cached_result.is_some());

        // 测试缓存大小
        assert_eq!(cache.size().await, 1);

        // 测试移除
        cache.remove(connection_id, sql).await?;
        assert_eq!(cache.size().await, 0);

        // 测试清空
        let (columns2, batch2) = make_single_row_batch();
        let result2 = QueryResult {
            columns: columns2,
            batches: vec![batch2],
            ..Default::default()
        };
        cache.set(connection_id, sql, result2, None).await?;
        assert_eq!(cache.size().await, 1);
        cache.clear().await?;
        assert_eq!(cache.size().await, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_query_cache_expiration() -> Result<(), CoreError> {
        let config = QueryCacheConfig {
            max_entries: 1000,
            default_ttl: Duration::from_millis(10),
            enable_stats: true,
        };
        let cache = QueryCache::new(Some(config));
        let connection_id = "test-conn";
        let sql = "SELECT * FROM users";
        let (columns, batch) = make_id_only_batch();
        let result = QueryResult {
            columns,
            batches: vec![batch],
            ..Default::default()
        };

        // 存储缓存
        cache.set(connection_id, sql, result, None).await?;
        assert_eq!(cache.size().await, 1);

        // 等待缓存过期
        tokio::time::sleep(Duration::from_millis(20)).await;

        // 测试缓存过期
        assert!(cache.get(connection_id, sql).await.is_none());
        assert_eq!(cache.size().await, 0);
        Ok(())
    }

    #[tokio::test]
    async fn test_query_cache_stats() -> Result<(), CoreError> {
        let cache = QueryCache::new(None);
        let connection_id = "test-conn";
        let sql = "SELECT * FROM users";
        let (columns, batch) = make_id_only_batch();
        let result = QueryResult {
            columns,
            batches: vec![batch],
            ..Default::default()
        };

        // 测试未命中
        assert!(cache.get(connection_id, sql).await.is_none());

        // 测试存储
        cache.set(connection_id, sql, result, None).await?;

        // 测试命中
        assert!(cache.get(connection_id, sql).await.is_some());

        // 测试统计信息
        let stats = cache.get_stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.stored, 1);
        assert_eq!(stats.expired, 0);
        assert_eq!(stats.removed, 0);
        assert_eq!(stats.hit_rate(), 0.5);
        Ok(())
    }
}
