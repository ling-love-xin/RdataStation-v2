/**
 * 会话管理模块
 *
 * 管理数据库会话的生命周期，包括：
 * - 会话创建和销毁
 * - 事务管理
 * - 上下文传递
 */
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::error::CoreError;

/// 会话模式
#[derive(Debug, Clone, PartialEq)]
pub enum SessionMode {
    /// 会话级：会话结束后数据消失
    Session,
    /// 持久化：数据保存到项目文件
    Persistent,
}

/// 会话配置
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// 会话模式
    pub mode: SessionMode,
    /// 是否启用 DuckDB 加速
    pub enable_duckdb_acceleration: bool,
    /// 最大结果集数量
    pub max_result_sets: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            mode: SessionMode::Session,
            enable_duckdb_acceleration: true,
            max_result_sets: 100,
        }
    }
}

/// 结果集元数据
#[derive(Debug, Clone)]
pub struct ResultSetMeta {
    /// 用户自定义名称
    pub name: String,
    /// 原始 SQL（持久化时保存）
    pub sql: Option<String>,
    /// 结果集模式
    pub mode: SessionMode,
    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 来源连接 ID
    pub source_connection: Option<String>,
}

/// 数据库会话
pub struct Session {
    /// 会话配置
    config: SessionConfig,
    /// 当前连接 ID
    current_connection_id: Arc<Mutex<Option<String>>>,
    /// 结果集注册表
    result_sets: Arc<Mutex<HashMap<String, ResultSetMeta>>>,
    /// 事务状态
    in_transaction: Arc<Mutex<bool>>,
}

impl Session {
    /// 创建新会话
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            current_connection_id: Arc::new(Mutex::new(None)),
            result_sets: Arc::new(Mutex::new(HashMap::new())),
            in_transaction: Arc::new(Mutex::new(false)),
        }
    }

    /// 设置当前连接
    pub async fn set_connection(&self, conn_id: String) {
        let mut current = self.current_connection_id.lock().await;
        *current = Some(conn_id);
    }

    /// 获取当前连接 ID
    pub fn current_connection_id(&self) -> Option<String> {
        // 注意：这里需要同步访问，实际实现可能需要异步
        None // 占位，实际需要从 Mutex 获取
    }

    /// 注册结果集
    pub async fn register_result_set(
        &self,
        name: String,
        sql: Option<String>,
        mode: SessionMode,
    ) -> Result<(), CoreError> {
        let mut sets = self.result_sets.lock().await;

        if sets.len() >= self.config.max_result_sets {
            return Err(CoreError::common(
                crate::core::error::CommonError::NotSupported(format!(
                    "Result set limit reached: {} sets",
                    self.config.max_result_sets
                )),
            ));
        }

        sets.insert(
            name.clone(),
            ResultSetMeta {
                name,
                sql,
                mode,
                created_at: chrono::Utc::now(),
                source_connection: self.current_connection_id().clone(),
            },
        );

        Ok(())
    }

    /// 获取结果集元数据
    pub async fn get_result_set(&self, name: &str) -> Option<ResultSetMeta> {
        let sets = self.result_sets.lock().await;
        sets.get(name).cloned()
    }

    /// 列出所有结果集
    pub async fn list_result_sets(&self) -> Vec<ResultSetMeta> {
        let sets = self.result_sets.lock().await;
        sets.values().cloned().collect()
    }

    /// 删除结果集
    pub async fn remove_result_set(&self, name: &str) -> bool {
        let mut sets = self.result_sets.lock().await;
        sets.remove(name).is_some()
    }

    /// 开始事务
    pub async fn begin_transaction(&self) -> Result<(), CoreError> {
        let mut in_tx = self.in_transaction.lock().await;
        if *in_tx {
            return Err(CoreError::common(crate::core::error::CommonError::General(
                "Already in transaction".to_string(),
            )));
        }
        *in_tx = true;
        Ok(())
    }

    /// 提交事务
    pub async fn commit_transaction(&self) -> Result<(), CoreError> {
        let mut in_tx = self.in_transaction.lock().await;
        if !*in_tx {
            return Err(CoreError::common(crate::core::error::CommonError::General(
                "Not in transaction".to_string(),
            )));
        }
        *in_tx = false;
        Ok(())
    }

    /// 回滚事务
    pub async fn rollback_transaction(&self) -> Result<(), CoreError> {
        let mut in_tx = self.in_transaction.lock().await;
        if !*in_tx {
            return Err(CoreError::common(crate::core::error::CommonError::General(
                "Not in transaction".to_string(),
            )));
        }
        *in_tx = false;
        Ok(())
    }

    /// 检查是否在事务中
    pub async fn is_in_transaction(&self) -> bool {
        *self.in_transaction.lock().await
    }

    /// 获取会话配置
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }
}
