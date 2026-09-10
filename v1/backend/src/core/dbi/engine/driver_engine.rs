/**
 * 原生驱动执行引擎
 *
 * 负责将查询下发到具体的数据库驱动执行
 */
use std::sync::Arc;

use crate::core::dbi::context::QueryContext;
use crate::core::dbi::engine::ExecutionEngine;
use crate::core::error::ConnectionError;
use crate::core::error::CoreError;
use crate::core::models::QueryResult;
use crate::core::services::get_connection_manager;

/// 原生驱动执行引擎
pub struct DriverEngine {
    /// 连接管理器（用于获取数据库连接）
    connection_manager: Arc<crate::core::services::ConnectionManager>,
}

impl DriverEngine {
    /// 创建新的驱动引擎
    pub fn new() -> Self {
        Self {
            connection_manager: get_connection_manager().clone(),
        }
    }
}

impl Default for DriverEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DriverEngine {
    /// 使用指定的连接管理器创建引擎
    pub fn with_connection_manager(
        connection_manager: Arc<crate::core::services::ConnectionManager>,
    ) -> Self {
        Self { connection_manager }
    }

    /// 获取数据库连接
    async fn get_database(
        &self,
        connection_id: &Option<String>,
    ) -> Result<Arc<dyn crate::core::driver::traits::Database + Send + Sync>, CoreError> {
        match connection_id {
            Some(id) => self
                .connection_manager
                .get_connection(id)
                .await
                .ok_or_else(|| CoreError::connection(ConnectionError::NotFound(id.clone()))),
            None => self
                .connection_manager
                .get_active_connection()
                .await
                .map(|(_, db)| db)
                .ok_or_else(|| CoreError::connection(ConnectionError::NoActiveConnection)),
        }
    }
}

#[async_trait::async_trait]
impl ExecutionEngine for DriverEngine {
    async fn execute(&self, sql: &str, context: &QueryContext) -> Result<QueryResult, CoreError> {
        let db = self.get_database(&context.connection_id).await?;

        // 根据上下文配置执行查询
        let result = if context.read_only {
            // 只读模式：使用普通查询
            db.query(sql).await?
        } else {
            // 写模式：使用普通查询（后续可扩展事务支持）
            db.query(sql).await?
        };

        Ok(result)
    }

    fn name(&self) -> &str {
        "driver"
    }

    fn supports(&self, _sql: &str) -> bool {
        // 原生驱动支持所有 SQL
        true
    }
}
