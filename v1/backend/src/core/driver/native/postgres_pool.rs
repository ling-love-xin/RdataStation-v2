use std::sync::Arc;

use sqlx::postgres::PgPool;

use crate::core::driver::native::postgres::PostgresDatabase;
use crate::core::driver::traits::{Database, DbPool, PoolStatus};
use crate::core::error::{ConnectionError, CoreError};

pub struct PostgresPoolWrapper {
    pool: PgPool,
    closed: Arc<std::sync::atomic::AtomicBool>,
    server_version: Option<String>,
    max_connections: usize,
    min_connections: usize,
}

impl PostgresPoolWrapper {
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        let pool = PgPool::connect(url).await.map_err(|e| {
            CoreError::connection(ConnectionError::Refused {
                conn_id: "postgres".to_string(),
                reason: e.to_string(),
            })
        })?;

        let server_version = sqlx::query_scalar::<_, String>("SELECT version()")
            .fetch_one(&pool)
            .await
            .ok();

        Ok(Self {
            pool,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            server_version,
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
        use sqlx::postgres::PgPoolOptions;
        let pool = PgPoolOptions::new()
            .max_connections(max_connections as u32)
            .min_connections(min_connections as u32)
            .connect(url)
            .await
            .map_err(|e| {
                CoreError::connection(ConnectionError::Refused {
                    conn_id: "postgres".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let server_version = sqlx::query_scalar::<_, String>("SELECT version()")
            .fetch_one(&pool)
            .await
            .ok();

        Ok(Self {
            pool,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            server_version,
            max_connections,
            min_connections,
        })
    }

    pub fn from_pool(pool: PgPool, server_version: Option<String>) -> Self {
        Self {
            pool,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            server_version,
            max_connections: 10,
            min_connections: 0,
        }
    }
}

#[async_trait::async_trait]
impl DbPool for PostgresPoolWrapper {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::connection(ConnectionError::PoolClosed));
        }

        let db = PostgresDatabase::from_pool_with_config(
            self.pool.clone(),
            self.server_version.clone(),
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
