//! SQLite 连接池实现
//!
//! ## 架构归属
//!
//! 属于 **StandardPool**（用户数据源）层。管理用户通过驱动连接的外部 `.db` 文件。
//!
//! 不同于 SmartPool 管理的系统内置 SQLite（global.db、project.db），
//! 本池的配置由用户在连接页面手动设置。
//!
//! ## 设计
//!
//! 预建 N 个 `SqliteDatabase` 实例，每次 acquire 从池中取出复用。
//! DbPool trait 的语义限制（返回 Box<dyn Database>，调用方管理生命周期），
//! 因此采用「预填充→取出→补充」策略而非连接归还。
//!

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::driver::native::sqlite::SqliteDatabase;
use crate::core::driver::traits::{Database, DbPool, PoolStatus};
use crate::core::error::{ConnectionError, CoreError};

const DEFAULT_POOL_SIZE: usize = 5;

/// SQLite 用户连接池
///
/// 预建 N 个 SqliteDatabase 实例。每次 acquire 从池中取出一个，
/// 异步后台任务自动补充池直到达到目标大小。
pub struct SqlitePoolWrapper {
    path: String,
    /// 预建数据库实例队列
    pool: Arc<Mutex<Vec<SqliteDatabase>>>,
    pool_size: usize,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl SqlitePoolWrapper {
    /// 创建 SQLite 连接池（默认大小 5）
    pub fn new(path: &str) -> Result<Self, CoreError> {
        Self::with_size(path, DEFAULT_POOL_SIZE)
    }

    /// 创建指定大小的 SQLite 连接池
    pub fn with_size(path: &str, pool_size: usize) -> Result<Self, CoreError> {
        let actual_size = pool_size.clamp(1, 10);

        let instances = (0..actual_size)
            .map(|_| SqliteDatabase::new(path))
            .collect::<Result<Vec<_>, _>>()?;

        tracing::info!(
            pool_size = actual_size,
            path = %path,
            "SQLite user pool created (StandardPool)"
        );

        Ok(Self {
            path: path.to_string(),
            pool: Arc::new(Mutex::new(instances)),
            pool_size: actual_size,
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    async fn replenish(&self) {
        let pool = self.pool.clone();
        let path = self.path.clone();
        let target = self.pool_size;

        tokio::spawn(async move {
            let current = pool.lock().await.len();
            let needed = target.saturating_sub(current);
            for _ in 0..needed {
                match SqliteDatabase::new(&path) {
                    Ok(db) => pool.lock().await.push(db),
                    Err(e) => {
                        tracing::warn!(
                            path = %path,
                            error = %e,
                            "SQLite user pool: failed to replenish connection"
                        );
                        break;
                    }
                }
            }
        });
    }
}

#[async_trait::async_trait]
impl DbPool for SqlitePoolWrapper {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::connection(ConnectionError::PoolClosed));
        }

        let db = {
            let mut pool = self.pool.lock().await;
            match pool.pop() {
                Some(db) => db,
                None => SqliteDatabase::new(&self.path)?,
            }
        };

        // 异步补充连接
        self.replenish().await;

        Ok(Box::new(db))
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut pool = self.pool.lock().await;
        pool.clear();
        tracing::info!(path = %self.path, "SQLite user pool closed");
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn status(&self) -> PoolStatus {
        let rt = tokio::runtime::Handle::try_current();
        let idle = rt
            .ok()
            .map(|h| h.block_on(async { self.pool.lock().await.len() }))
            .unwrap_or(0);

        let active = self.pool_size.saturating_sub(idle);

        PoolStatus {
            size: self.pool_size,
            idle,
            active,
            waiting: 0,
            max_connections: self.pool_size,
            min_connections: 1,
        }
    }
}
