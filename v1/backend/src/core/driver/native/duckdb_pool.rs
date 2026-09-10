//! DuckDB 连接池实现
//!
//! ## 架构归属
//!
//! 属于 **StandardPool**（用户数据源）层。管理用户通过驱动连接的外部 `.duckdb` 文件。
//!
//! 不同于 SmartPool 管理的系统内置 DuckDB（analytics.duckdb），
//! 本池的配置由用户在连接页面手动设置。
//!
//! ## 设计
//!
//! DuckDB 是单写入者模型，最大连接数恒为 1。
//! 预建单个 DuckDbDatabase 实例，每次 acquire 返回同一实例。
//! DbPool trait 语义限制（返回 Box<dyn Database>），采用 mem::replace 策略：
//! acquire 时取出实例，后台异步补充新实例。
//!

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::driver::native::duckdb::DuckDbDatabase;
use crate::core::driver::traits::{Database, DbPool, PoolStatus};
use crate::core::error::{ConnectionError, CoreError};

/// DuckDB 用户连接池
///
/// 单连接模式（DuckDB 为单写入者）。
/// 首次 connect 时创建连接，后续 acquire 复用。
pub struct DuckDbPoolWrapper {
    path: String,
    db: Arc<Mutex<Option<DuckDbDatabase>>>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl DuckDbPoolWrapper {
    /// 创建 DuckDB 连接池（单连接模式）
    pub fn new(path: &str) -> Result<Self, CoreError> {
        let db = DuckDbDatabase::new(path)?;

        tracing::info!(
            path = %path,
            "DuckDB user pool created (StandardPool, single-connection mode)"
        );

        Ok(Self {
            path: path.to_string(),
            db: Arc::new(Mutex::new(Some(db))),
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
}

#[async_trait::async_trait]
impl DbPool for DuckDbPoolWrapper {
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError> {
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::connection(ConnectionError::PoolClosed));
        }

        let db = {
            let mut guard = self.db.lock().await;
            match guard.take() {
                Some(db) => db,
                None => DuckDbDatabase::new(&self.path)?,
            }
        };

        // 异步补充连接
        let db_ref = self.db.clone();
        let path = self.path.clone();
        tokio::spawn(async move {
            match DuckDbDatabase::new(&path) {
                Ok(new_db) => {
                    *db_ref.lock().await = Some(new_db);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path,
                        error = %e,
                        "DuckDB user pool: failed to replenish connection"
                    );
                }
            }
        });

        Ok(Box::new(db))
    }

    async fn close(&self) -> Result<(), CoreError> {
        self.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut guard = self.db.lock().await;
        *guard = None;
        tracing::info!(path = %self.path, "DuckDB user pool closed");
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn status(&self) -> PoolStatus {
        let rt = tokio::runtime::Handle::try_current();
        let has_instance = rt
            .ok()
            .map(|h| h.block_on(async { self.db.lock().await.is_some() }))
            .unwrap_or(false);

        PoolStatus {
            size: 1,
            idle: if has_instance { 1 } else { 0 },
            active: if has_instance { 0 } else { 1 },
            waiting: 0,
            max_connections: 1,
            min_connections: 1,
        }
    }
}
