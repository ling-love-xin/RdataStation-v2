/**
 * 连接元数据缓存连接池 (Metadata Cache Connection Pool)
 *
 * 属于 SmartPool 体系中的「连接元数据 SQLite」—— 每个数据库连接独享一个缓存 SQLite 文件。
 *
 * 设计理由：
 * - 高频读写：元数据浏览时树节点展开、DDL 事件刷新都会触发密集读写
 * - 单文件多连接：WAL 模式下 SQLite 支持并发读，池化后可显著降低文件打开开销
 * - 生命周期跟随用户连接：连接建立时创建池，连接关闭时销毁
 *
 * 架构归属：SmartPool（守护系统内置库）
 *   ├── 应用级 SQLite（global.db）        → GlobalSqlitePool
 *   ├── 项目级 SQLite（project.meta.sqlite）→ ProjectSqlitePool
 *   ├── 连接元数据 SQLite（每个连接一个）    → MetadataCachePool  ← 本模块
 *   ├── 应用级 DuckDB（analytics.duckdb）   → GlobalDuckdbConnection
 *   └── 项目级 DuckDB（project.analytics.duckdb）→ ProjectDuckdbConnection
 */
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use rusqlite::Connection;
use tokio::sync::{Mutex, Semaphore};

use crate::core::error::{CommonError, CoreError, StorageError};

static METADATA_POOL_REGISTRY: OnceLock<Mutex<HashMap<String, Arc<MetadataCachePool>>>> =
    OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<MetadataCachePool>>> {
    METADATA_POOL_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 连接元数据缓存连接池
///
/// 为单个数据库连接的元数据缓存 SQLite 文件提供连接池化。
/// 每个 conn_id 对应一个独立的池实例。
pub struct MetadataCachePool {
    conn_id: String,
    db_path: PathBuf,
    pool: Arc<Mutex<Vec<Connection>>>,
    semaphore: Arc<Semaphore>,
    pool_size: usize,
}

impl MetadataCachePool {
    /// 获取或创建指定连接的元数据缓存池
    ///
    /// 如果池已存在则返回现有实例，否则创建新池
    pub async fn get_or_create(
        conn_id: &str,
        db_path: PathBuf,
        pool_size: usize,
    ) -> Result<Arc<Self>, CoreError> {
        let mut registry = registry().lock().await;

        if let Some(existing) = registry.get(conn_id) {
            return Ok(Arc::clone(existing));
        }

        let pool = Self::create(conn_id, db_path, pool_size).await?;
        let arc = Arc::new(pool);
        registry.insert(conn_id.to_string(), Arc::clone(&arc));
        Ok(arc)
    }

    async fn create(conn_id: &str, db_path: PathBuf, pool_size: usize) -> Result<Self, CoreError> {
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "metadata_cache".to_string(),
                    operation: "create_dir".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }

        let mut connections = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let conn = Self::open_connection(&db_path)?;
            connections.push(conn);
        }

        Ok(Self {
            conn_id: conn_id.to_string(),
            db_path,
            pool: Arc::new(Mutex::new(connections)),
            semaphore: Arc::new(Semaphore::new(pool_size)),
            pool_size,
        })
    }

    fn open_connection(path: &PathBuf) -> Result<Connection, CoreError> {
        let conn = Connection::open(path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "metadata_cache".to_string(),
                operation: "open".to_string(),
                reason: e.to_string(),
            })
        })?;

        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "metadata_cache".to_string(),
                    operation: "set_wal_mode".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let _ = conn.execute("PRAGMA mmap_size=268435456", []).map_err(|e| {
            tracing::warn!("Failed to set mmap_size on metadata cache pool: {}", e);
        });

        conn.execute("PRAGMA cache_size=-2000", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "metadata_cache".to_string(),
                operation: "set_cache_size".to_string(),
                reason: e.to_string(),
            })
        })?;

        conn.execute("PRAGMA foreign_keys=ON", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "metadata_cache".to_string(),
                operation: "set_foreign_keys".to_string(),
                reason: e.to_string(),
            })
        })?;

        conn.execute("PRAGMA synchronous=NORMAL", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "metadata_cache".to_string(),
                operation: "set_synchronous".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(conn)
    }

    /// 从池中获取一个连接
    ///
    /// 通过信号量控制并发数，池为空时等待其他连接归还
    pub async fn acquire(self: &Arc<Self>) -> Result<PooledMetadataConnection, CoreError> {
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .map_err(|_| {
                CoreError::common(CommonError::General(
                    "Metadata cache pool semaphore closed".to_string(),
                ))
            })?;

        let conn = {
            let mut pool = self.pool.lock().await;
            pool.pop().ok_or_else(|| {
                CoreError::common(CommonError::General(
                    "Metadata cache pool exhausted".to_string(),
                ))
            })?
        };

        Ok(PooledMetadataConnection {
            conn: Some(conn),
            pool: Arc::clone(&self.pool),
            _permit: permit,
        })
    }

    /// 同步获取连接（用于同步上下文）
    pub fn acquire_sync(self: &Arc<Self>) -> Result<PooledMetadataConnection, CoreError> {
        let rt = tokio::runtime::Handle::current();
        let permit = rt
            .block_on(Arc::clone(&self.semaphore).acquire_owned())
            .map_err(|_| {
                CoreError::common(CommonError::General(
                    "Metadata cache pool semaphore closed".to_string(),
                ))
            })?;

        let conn = {
            let mut pool = rt.block_on(self.pool.lock());
            pool.pop().ok_or_else(|| {
                CoreError::common(CommonError::General(
                    "Metadata cache pool exhausted".to_string(),
                ))
            })?
        };

        Ok(PooledMetadataConnection {
            conn: Some(conn),
            pool: Arc::clone(&self.pool),
            _permit: permit,
        })
    }

    pub fn conn_id(&self) -> &str {
        &self.conn_id
    }

    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// 关闭池并移除注册
    pub async fn close_and_remove(conn_id: &str) -> Result<(), CoreError> {
        let pool = {
            let mut registry = registry().lock().await;
            registry.remove(conn_id)
        };

        if let Some(pool) = pool {
            let mut pool_guard = pool.pool.lock().await;
            pool_guard.clear();
        }

        Ok(())
    }

    /// 获取池统计信息
    pub async fn stats(&self) -> PoolStats {
        let pool = self.pool.lock().await;
        let available = pool.len();
        let in_use = self.pool_size.saturating_sub(available);

        PoolStats {
            pool_size: self.pool_size,
            available,
            in_use,
        }
    }
}

/// 池化连接 RAII 包装器
///
/// Drop 时自动归还连接到池中
pub struct PooledMetadataConnection {
    conn: Option<Connection>,
    pool: Arc<Mutex<Vec<Connection>>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl PooledMetadataConnection {
    /// 获取内部连接的引用
    pub fn inner(&self) -> Result<&Connection, CoreError> {
        self.conn.as_ref().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "Connection already returned to pool".to_string(),
            ))
        })
    }

    /// 获取内部连接的可变引用
    pub fn inner_mut(&mut self) -> Result<&mut Connection, CoreError> {
        self.conn.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "Connection already returned to pool".to_string(),
            ))
        })
    }
}

impl Drop for PooledMetadataConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let pool = Arc::clone(&self.pool);
            let rt = tokio::runtime::Handle::try_current();

            match rt {
                Ok(handle) => {
                    handle.spawn(async move {
                        let mut pool_guard = pool.lock().await;
                        pool_guard.push(conn);
                    });
                }
                Err(_) => match pool.try_lock() {
                    Ok(mut pool_guard) => {
                        pool_guard.push(conn);
                    }
                    Err(_) => {
                        tracing::warn!("Failed to return metadata cache connection to pool");
                    }
                },
            }
        }
    }
}

/// 池统计信息
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub pool_size: usize,
    pub available: usize,
    pub in_use: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdata_test_mcp_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("test_cache.sqlite")
    }

    #[tokio::test]
    async fn test_pool_create_and_acquire() {
        let path = test_temp_path("acquire");
        let pool = Arc::new(
            MetadataCachePool::create("test_conn", path, 3)
                .await
                .expect("创建池失败"),
        );

        let stats = pool.stats().await;
        assert_eq!(stats.pool_size, 3);
        assert_eq!(stats.available, 3);
        assert_eq!(stats.in_use, 0);

        let conn = pool.acquire().await.expect("获取连接失败");
        let stats = pool.stats().await;
        assert_eq!(stats.available, 2);
        assert_eq!(stats.in_use, 1);

        drop(conn);
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let stats = pool.stats().await;
        assert_eq!(stats.available, 3);
        assert_eq!(stats.in_use, 0);
    }

    #[tokio::test]
    async fn test_pool_registry_get_or_create() -> Result<(), CoreError> {
        let path = test_temp_path("registry");
        let pool1 = MetadataCachePool::get_or_create("reg_conn", path.clone(), 2).await?;
        let pool2 = MetadataCachePool::get_or_create("reg_conn", path.clone(), 2).await?;
        assert!(Arc::ptr_eq(&pool1, &pool2));
        Ok(())
    }

    #[tokio::test]
    async fn test_pooled_connection_inner() -> Result<(), CoreError> {
        let path = test_temp_path("inner");
        let pool = Arc::new(MetadataCachePool::create("inner_conn", path, 1).await?);
        let conn = pool.acquire().await?;
        assert!(conn.inner().is_ok());
        Ok(())
    }
}
