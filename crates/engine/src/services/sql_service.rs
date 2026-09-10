use std::sync::Arc;

use crate::cache::get_query_cache;
use crate::driver::traits::DynDatabase;
use shared::error::{CoreError, DatabaseError};
use shared::models::QueryResult;
use crate::persistence::history_store;
use crate::connection_manager::ConnectionManager;
use crate::sql::SqlEngine;
use crate::sql::{SqlDialect, SqlStatementType};

/// 事务状态结果
#[derive(Debug)]
pub struct TransactionStatusResult {
    pub conn_id: String,
    pub is_in_transaction: bool,
    pub transaction_start_time_ms: Option<u64>,
    pub transaction_duration_ms: Option<u64>,
}

/// SQL 执行服务
///
/// 负责 SQL 查询的执行和管理，包括：
/// - 执行查询
/// - 执行事务
/// - 取消查询
/// - SQL 历史记录
pub struct SqlService {
    manager: Arc<ConnectionManager>,
}

/// SQL 执行结果
#[derive(Debug)]
pub struct SqlExecuteResult {
    /// 查询结果
    pub result: QueryResult,
    /// 执行耗时（毫秒）
    pub elapsed_ms: u64,
    /// 结果是否被截断（超出最大行数限制）
    pub truncated: bool,
}

/// 单次查询最大返回行数
pub const MAX_QUERY_ROWS: usize = 10_000;

/// 默认连接键名（无显式连接 ID 时的回退值）
const DEFAULT_CONN_KEY: &str = "active";

/// SQL 执行选项
#[derive(Debug, Default)]
pub struct SqlExecuteOptions {
    /// 是否记录到历史
    pub record_history: bool,
    /// 是否使用事务
    pub use_transaction: bool,
    /// 查询超时时间（毫秒）
    pub timeout_ms: Option<u64>,
    /// 是否使用查询缓存
    pub use_cache: bool,
}

impl SqlService {
    /// 创建新的 SQL 服务
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        Self { manager }
    }

    /// 执行 SQL 查询
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID（使用活动连接如果为 None）
    /// * `sql` - SQL 语句
    /// * `options` - 执行选项
    ///
    /// # Returns
    ///
    /// 返回执行结果
    pub async fn execute(
        &self,
        conn_id: Option<String>,
        sql: &str,
        options: SqlExecuteOptions,
    ) -> Result<SqlExecuteResult, CoreError> {
        let start_time = std::time::Instant::now();

        // 参数校验
        if sql.trim().is_empty() {
            return Err(CoreError::database(DatabaseError::query(
                sql,
                "SQL statement cannot be empty".to_string(),
            )));
        }

        // SQL 语句分类（智能路由）
        let (stmt_type, _normalized) = SqlEngine::parse_and_route(sql, SqlDialect::Ansi);
        let (_is_ddl, _is_dml, is_dql) = {
            let is_ddl = matches!(stmt_type, SqlStatementType::Ddl);
            let is_dml = matches!(
                stmt_type,
                SqlStatementType::Insert | SqlStatementType::Update | SqlStatementType::Delete
            );
            let is_dql = matches!(stmt_type, SqlStatementType::Select);
            (is_ddl, is_dml, is_dql)
        };

        // 获取查询缓存
        let query_cache = get_query_cache();

        // 检查缓存（仅当启用缓存、非事务操作、非 DDL/DML 时）
        if options.use_cache && !options.use_transaction && is_dql {
            let connection_id = match conn_id.as_deref() {
                Some(id) => id,
                None => DEFAULT_CONN_KEY,
            };
            if let Some(cached_result) = query_cache.get(connection_id, sql).await {
                let elapsed_ms = start_time.elapsed().as_millis() as u64;

                return Ok(SqlExecuteResult {
                    result: cached_result,
                    elapsed_ms,
                    truncated: false,
                });
            }
        }

        // 获取数据库连接
        let db = self.get_database(conn_id.clone()).await?;

        // 创建取消令牌（支持前端 cancel_query）
        let conn_key = match conn_id.clone() {
            Some(id) => id,
            None => DEFAULT_CONN_KEY.to_string(),
        };
        let cancel_token = self.manager.create_cancel_token(&conn_key).await;

        // 执行查询（支持取消和超时）
        let result = if let Some(timeout_ms) = options.timeout_ms {
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(timeout_ms),
                db.query_with_cancel(sql, cancel_token.clone()),
            )
            .await
            {
                Ok(inner_result) => inner_result?,
                Err(_elapsed) => {
                    cancel_token.cancel();
                    return Err(CoreError::database(DatabaseError::Query {
                        sql: sql.to_string(),
                        reason: format!("Query timed out after {}ms", timeout_ms),
                        position: None,
                    }));
                }
            }
        } else {
            db.query_with_cancel(sql, cancel_token.clone()).await?
        };

        // 清理取消令牌
        self.manager.remove_cancel_token(&conn_key).await;

        // 应用行数限制，防止内存溢出
        let mut result = result;
        let truncated = result.truncate(MAX_QUERY_ROWS) > 0;

        // 将结果存入缓存（仅当启用缓存、非事务操作、且为 SELECT 查询时）
        if options.use_cache && !options.use_transaction && is_dql {
            let connection_id = match conn_id.as_deref() {
                Some(id) => id,
                None => DEFAULT_CONN_KEY,
            };
            let _ = query_cache
                .set(connection_id, sql, result.clone(), None)
                .await;
        }

        let elapsed_ms = start_time.elapsed().as_millis() as u64;

        // 记录到历史
        if options.record_history {
            tracing::info!(
                sql = %sql,
                elapsed_ms,
                stmt_type = ?stmt_type,
                "SQL executed"
            );
            if let Err(e) = history_store::save_sql_history(sql, conn_id.as_deref()) {
                tracing::error!(error = %e, "Failed to save SQL history");
            }
        }

        Ok(SqlExecuteResult {
            result,
            elapsed_ms,
            truncated,
        })
    }

    /// 在事务中执行多个 SQL
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID
    /// * `sqls` - SQL 语句列表
    ///
    /// # Returns
    ///
    /// 返回每个 SQL 的执行结果
    pub async fn execute_in_transaction(
        &self,
        conn_id: Option<String>,
        sqls: Vec<String>,
    ) -> Result<Vec<SqlExecuteResult>, CoreError> {
        if sqls.is_empty() {
            return Ok(Vec::new());
        }

        // 获取数据库连接
        let db = self.get_database(conn_id).await?;

        // 开始事务
        let mut tx = db.begin_transaction().await?;

        let mut results = Vec::with_capacity(sqls.len());
        let mut failed = false;
        let mut error = None;

        for sql in sqls {
            let start_time = std::time::Instant::now();

            match tx.query(&sql).await {
                Ok(result) => {
                    let elapsed_ms = start_time.elapsed().as_millis() as u64;
                    results.push(SqlExecuteResult {
                        result,
                        elapsed_ms,
                        truncated: false,
                    });
                }
                Err(e) => {
                    failed = true;
                    error = Some(e);
                    break;
                }
            }
        }

        if failed {
            let _ = tx.rollback().await;
            return Err(match error {
                Some(e) => e,
                None => CoreError::database(DatabaseError::Query {
                    sql: "unknown".to_string(),
                    reason: "Transaction failed with unknown error".to_string(),
                    position: None,
                }),
            });
        }

        // 提交事务
        tx.commit().await?;

        Ok(results)
    }

    /// 执行查询（简化版，不带选项）
    pub async fn query(
        &self,
        conn_id: Option<String>,
        sql: &str,
    ) -> Result<QueryResult, CoreError> {
        let result = self
            .execute(
                conn_id,
                sql,
                SqlExecuteOptions {
                    record_history: true,
                    use_cache: true, // 默认启用缓存
                    ..Default::default()
                },
            )
            .await?;
        Ok(result.result)
    }

    /// 获取数据库连接（含自动重连）
    ///
    /// 先从连接管理器获取连接，若连接不存在或 ping 失败则尝试重连。
    async fn get_database(&self, conn_id: Option<String>) -> Result<DynDatabase, CoreError> {
        match conn_id {
            Some(ref id) => self.manager.get_or_reconnect(id).await,
            None => {
                let conn_id = self
                    .manager
                    .get_active_connection_id()
                    .await
                    .ok_or_else(|| {
                        CoreError::connection(
                            shared::error::ConnectionError::NoActiveConnection,
                        )
                    })?;
                self.manager.get_or_reconnect(&conn_id).await
            }
        }
    }

    /// 获取 SQL 执行历史
    pub fn get_sql_history(
        &self,
        limit: usize,
    ) -> Result<Vec<history_store::SqlHistoryRecord>, CoreError> {
        history_store::get_sql_history(limit).map_err(|e| {
            CoreError::storage(shared::error::StorageError::read(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 搜索 SQL 历史
    pub fn search_sql_history(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<history_store::SqlHistoryRecord>, CoreError> {
        history_store::search_sql_history(keyword, limit).map_err(|e| {
            CoreError::storage(shared::error::StorageError::read(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 清空 SQL 历史
    pub fn clear_sql_history(&self) -> Result<(), CoreError> {
        history_store::clear_sql_history().map_err(|e| {
            CoreError::storage(shared::error::StorageError::write(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 删除单条 SQL 历史
    pub fn remove_sql_history(&self, id: &str) -> Result<(), CoreError> {
        history_store::remove_sql_history(id).map_err(|e| {
            CoreError::storage(shared::error::StorageError::write(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 注册外部数据库连接
    pub async fn register_external_database(
        &self,
        conn_id: Option<String>,
        name: &str,
        driver: &str,
        connection_string: &str,
    ) -> Result<(), CoreError> {
        // 获取数据库连接
        let db = self.get_database(conn_id).await?;

        // 检查是否支持联邦查询
        if !db.meta().supports_federated {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "generic".to_string(),
                operation: "register_external_database".to_string(),
                source: "Federated queries not supported".to_string(),
            }));
        }

        // 注册外部数据库
        db.register_external_database(name, driver, connection_string)
            .await
    }

    /// 创建外部表
    pub async fn create_external_table(
        &self,
        conn_id: Option<String>,
        external_db_name: &str,
        schema_name: &str,
        table_name: &str,
        external_table_name: &str,
    ) -> Result<(), CoreError> {
        // 获取数据库连接
        let db = self.get_database(conn_id).await?;

        // 检查是否支持联邦查询
        if !db.meta().supports_federated {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "generic".to_string(),
                operation: "create_external_table".to_string(),
                source: "Federated queries not supported".to_string(),
            }));
        }

        // 创建外部表
        db.create_external_table(
            external_db_name,
            schema_name,
            table_name,
            external_table_name,
        )
        .await
    }

    /// 开始事务
    pub async fn begin_transaction(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let db = self.get_database(conn_id.clone()).await?;
        let conn_id_str = match conn_id {
            Some(ref id) => id.clone(),
            None => DEFAULT_CONN_KEY.to_string(),
        };

        // 执行 BEGIN TRANSACTION
        db.query("BEGIN TRANSACTION").await?;

        // 返回事务状态
        Ok(TransactionStatusResult {
            conn_id: conn_id_str,
            is_in_transaction: true,
            transaction_start_time_ms: Some(
                match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => d.as_millis() as u64,
                    Err(_) => 0u64,
                },
            ),
            transaction_duration_ms: Some(0),
        })
    }

    /// 提交事务
    pub async fn commit_transaction(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let db = self.get_database(conn_id.clone()).await?;
        let conn_id_str = match conn_id {
            Some(ref id) => id.clone(),
            None => DEFAULT_CONN_KEY.to_string(),
        };

        // 执行 COMMIT
        db.query("COMMIT").await?;

        // 返回事务状态
        Ok(TransactionStatusResult {
            conn_id: conn_id_str,
            is_in_transaction: false,
            transaction_start_time_ms: None,
            transaction_duration_ms: None,
        })
    }

    /// 回滚事务
    pub async fn rollback_transaction(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let db = self.get_database(conn_id.clone()).await?;
        let conn_id_str = match conn_id {
            Some(ref id) => id.clone(),
            None => DEFAULT_CONN_KEY.to_string(),
        };

        // 执行 ROLLBACK
        db.query("ROLLBACK").await?;

        // 返回事务状态
        Ok(TransactionStatusResult {
            conn_id: conn_id_str,
            is_in_transaction: false,
            transaction_start_time_ms: None,
            transaction_duration_ms: None,
        })
    }

    /// 获取事务状态
    pub async fn get_transaction_status(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let conn_id_str = match conn_id {
            Some(ref id) => id.clone(),
            None => DEFAULT_CONN_KEY.to_string(),
        };

        // 检查事务状态（简化实现，实际应从 session 获取）
        Ok(TransactionStatusResult {
            conn_id: conn_id_str,
            is_in_transaction: false, // 实际应从 session 查询
            transaction_start_time_ms: None,
            transaction_duration_ms: None,
        })
    }

    /// 取消指定连接正在执行的查询
    pub async fn cancel_query(&self, conn_id: Option<String>) -> Result<bool, CoreError> {
        let conn_id_str = match conn_id {
            Some(ref id) => id.clone(),
            None => DEFAULT_CONN_KEY.to_string(),
        };
        if conn_id_str.is_empty() {
            return Err(CoreError::connection(
                shared::error::ConnectionError::NoActiveConnection,
            ));
        }
        Ok(self.manager.cancel_query(&conn_id_str).await)
    }
}

pub fn value_to_sql(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        serde_json::Value::Array(arr) => {
            format!("'{}'", serde_json::to_string(arr).unwrap_or_default())
        }
        serde_json::Value::Object(obj) => {
            format!("'{}'", serde_json::to_string(obj).unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_service() -> SqlService {
        SqlService::new(Arc::new(ConnectionManager::new()))
    }

    #[tokio::test]
    async fn test_execute_empty_sql() {
        let service = new_service();
        let result = service.execute(None, "", Default::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_whitespace_sql() {
        let service = new_service();
        let result = service.execute(None, "   \n\t  ", Default::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_no_active_connection() {
        let service = new_service();
        let result = service.execute(None, "SELECT 1", Default::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_specific_connection_not_found() {
        let service = new_service();
        let result = service
            .execute(Some("nonexistent".into()), "SELECT 1", Default::default())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_with_record_history_option() {
        let service = new_service();
        let options = SqlExecuteOptions {
            record_history: true,
            ..Default::default()
        };
        let result = service.execute(None, "SELECT 1", options).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_in_transaction_empty() {
        let service = new_service();
        let result = service.execute_in_transaction(None, vec![]).await;
        assert!(result.is_ok());
        let results = match result {
            Ok(r) => r,
            Err(_) => {
                panic!("expected Ok");
            }
        };
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_in_transaction_no_connection() {
        let service = new_service();
        let result = service
            .execute_in_transaction(None, vec!["SELECT 1".into()])
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_query_no_connection() {
        let service = new_service();
        let result = service.query(None, "SELECT 1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_sql_history() {
        let service = new_service();
        let result = service.get_sql_history(10);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_search_sql_history() {
        let service = new_service();
        let result = service.search_sql_history("test", 10);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_clear_sql_history() {
        let service = new_service();
        let result = service.clear_sql_history();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_sql_history_nonexistent() {
        let service = new_service();
        let result = service.remove_sql_history("nonexistent");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_begin_transaction_no_connection() {
        let service = new_service();
        let result = service.begin_transaction(None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_commit_transaction_no_connection() {
        let service = new_service();
        let result = service.commit_transaction(None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rollback_transaction_no_connection() {
        let service = new_service();
        let result = service.rollback_transaction(None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_transaction_status_no_connection() {
        let service = new_service();
        let result = service.get_transaction_status(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_query_no_connection() {
        let service = new_service();
        let result = service.cancel_query(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_query_specific_not_found() {
        let service = new_service();
        let result = service.cancel_query(Some("nonexistent".into())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_register_external_database_no_connection() {
        let service = new_service();
        let result = service
            .register_external_database(None, "ext", "mysql", "mysql://localhost")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_external_table_no_connection() {
        let service = new_service();
        let result = service
            .create_external_table(None, "ext", "public", "users", "ext_users")
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_sql_execute_options_default() {
        let options = SqlExecuteOptions::default();
        assert!(!options.record_history);
        assert!(!options.use_transaction);
        assert!(options.timeout_ms.is_none());
        assert!(!options.use_cache);
    }

    #[test]
    fn test_sql_execute_result_construction() {
        let result = SqlExecuteResult {
            result: QueryResult::empty(),
            elapsed_ms: 42,
            truncated: false,
        };
        assert_eq!(result.elapsed_ms, 42);
        assert!(!result.truncated);
        assert!(result.result.is_empty());
    }
}
