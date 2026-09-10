/**
 * DBI (Database Interface) - 对外唯一接口
 *
 * 提供统一的查询和执行入口，内部自动路由到合适的执行引擎
 */
use std::sync::Arc;

use crate::core::dbi::{
    context::QueryContext,
    engine::{ExecutionMode, QueryRouter},
    session::Session,
};
use crate::core::error::CoreError;
use crate::core::models::QueryResult;

/// 统一数据访问接口
///
/// DBI 是 RdataStation 的核心数据访问层，所有数据操作都通过此接口进行
pub struct DBI {
    /// 查询路由器
    router: Arc<QueryRouter>,
    /// 当前会话
    session: Arc<Session>,
}

impl DBI {
    /// 创建新的 DBI 实例
    pub fn new(router: Arc<QueryRouter>, session: Arc<Session>) -> Self {
        Self { router, session }
    }

    /// 执行查询（只读）
    ///
    /// # 参数
    /// - `sql`: SQL 语句
    /// - `mode`: 执行模式（原生 / DuckDB 加速）
    ///
    /// # 返回
    /// 查询结果（Arrow 格式）
    pub async fn query(&self, sql: &str, mode: ExecutionMode) -> Result<QueryResult, CoreError> {
        let context = QueryContext::new(self.session.current_connection_id(), mode);

        self.router.execute(sql, &context).await
    }

    /// 执行更新（写操作）
    ///
    /// # 参数
    /// - `sql`: SQL 语句
    ///
    /// # 返回
    /// 执行结果（影响的行数）
    pub async fn execute(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let context = QueryContext::new(
            self.session.current_connection_id(),
            ExecutionMode::Native, // 写操作必须走原生驱动
        );

        self.router.execute(sql, &context).await
    }

    /// 获取当前会话
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }

    /// 获取查询路由器
    pub fn router(&self) -> Arc<QueryRouter> {
        self.router.clone()
    }
}
