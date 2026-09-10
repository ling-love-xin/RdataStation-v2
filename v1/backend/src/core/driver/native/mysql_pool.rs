//! MySQL 连接池实现
//!
//! 实现 DbPool trait，包装 sqlx::MySqlPool

use std::sync::Arc;

use sqlx::mysql::MySqlPool;

use crate::core::driver::native::mysql::MySqlDatabase;
use crate::core::driver::traits::{Database, DbPool, PoolStatus};
use crate::core::error::{ConnectionError, CoreError};

/// MySQL 连接池
pub struct MySqlPoolWrapper {
    pool: MySqlPool,
    closed: Arc<std::sync::atomic::AtomicBool>,
    max_connections: usize,
    min_connections: usize,
}

impl MySqlPoolWrapper {
    /// 从 URL 创建连接池
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        let pool = MySqlPool::connect(url).await.map_err(|e| {
            CoreError::connection(ConnectionError::Refused {
                conn_id: "mysql".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(Self {
            pool,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            max_connections: 10,
            min_connections: 0,
        })
    }

    /// 从 URL 创建连接池（可配置连接数）
    pub async fn new_with_options(
        url: &str,
        max_connections: usize,
        min_connections: usize,
    ) -> Result<Self, CoreError> {
        use sqlx::mysql::MySqlPoolOptions;
        let pool = MySqlPoolOptions::new()
            .max_connections(max_connections as u32)
            .min_connections(min_connections as u32)
            .connect(url)
            .await
            .map_err(|e| {
                CoreError::connection(ConnectionError::Refused {
                    conn_id: "mysql".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(Self {
            pool,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            max_connections,
            min_connections,
        })
    }

    /// 从现有 Pool 创建
    pub fn from_pool(pool: MySqlPool) -> Self {
        Self {
            pool,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            max_connections: 10,
            min_connections: 0,
        }
    }
}

#[async_trait::async_trait]
impl DbPool for MySqlPoolWrapper {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::connection(ConnectionError::PoolClosed));
        }

        let db = MySqlDatabase::from_pool_with_config(
            self.pool.clone(),
            self.max_connections,
            self.min_connections,
        );
        Ok(Box::new(db))
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.pool.close().await;
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn status(&self) -> PoolStatus {
        let size = self.pool.size() as usize;
        let idle = self.pool.num_idle();
        let active = size.saturating_sub(idle);

        PoolStatus {
            size,
            idle,
            active,
            waiting: 0,
            max_connections: self.max_connections,
            min_connections: self.min_connections,
        }
    }
}
