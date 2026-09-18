use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use specta::Type;

use crate::cache::CacheManager;
use crate::driver::registry::DriverConnectionConfig;
use crate::driver::traits::{DynDatabase, Transaction};
use crate::driver::DriverRegistry;
use shared::error::{ConnectionError, CoreError, DatabaseError, TransactionState};
use shared::models::QueryResult;

/// 连接 ID 类型
pub type ConnId = String;

/// 事务类错误（操作名 + 状态 + 可读原因）
fn transaction_error(operation: &str, state: TransactionState, reason: &str) -> CoreError {
    CoreError::database(DatabaseError::Transaction {
        operation: operation.to_string(),
        state,
        reason: reason.to_string(),
    })
}

/// 连接类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Type)]
pub enum ConnectionType {
    /// 全局连接：归属软件，不随项目迁移
    Global,
    /// 项目连接：归属项目，随项目完整迁移
    Project,
}

impl std::fmt::Display for ConnectionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionType::Global => write!(f, "global"),
            ConnectionType::Project => write!(f, "project"),
        }
    }
}

impl ConnectionType {
    pub fn parse_type(s: &str) -> Option<Self> {
        match s {
            "global" => Some(ConnectionType::Global),
            "project" => Some(ConnectionType::Project),
            _ => None,
        }
    }
}

/// 连接配置（旧版，用于兼容）
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub db_type: String,
    pub url: String,
    pub name: Option<String>,
    pub connection_type: Option<ConnectionType>,
    pub project_id: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub description: Option<String>,
}

/// 连接信息
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: ConnId,
    pub name: String,
    pub db_type: String,
    pub url: String,
    pub server_version: Option<String>,
    pub connection_type: ConnectionType,
    pub project_id: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub description: Option<String>,
    /// 开了「DuckDB 本地加速」吗（连接对话框里的开关；联邦源清单据此组装）
    ///
    /// 语义：**这条连接可以被 DuckDB 直连**（凭据已注册为 Secret）。本地加速档用它做门控，
    /// 联邦档用它决定“哪些连接参与跨源查询”。
    pub use_duckdb_fed: bool,
    pub created_at: std::time::Instant,
}

/// 连接管理器
///
/// 负责管理所有数据库连接的生命周期，包括：
/// - 连接的创建和存储
/// - 连接的复用
/// - 活动连接的切换
/// - 连接的关闭和清理
/// - 空闲连接回收（idle timeout）
///
/// # 空闲回收策略
///
/// 默认空闲超时 30 分钟。超过此时间未被访问的连接将被自动关闭。
/// 可通过 `set_idle_timeout()` 调整。
pub struct ConnectionManager {
    /// 存储所有数据库连接
    connections: tokio::sync::RwLock<HashMap<ConnId, DynDatabase>>,
    /// 连接信息映射
    connection_info: tokio::sync::RwLock<HashMap<ConnId, ConnectionInfo>>,
    /// 连接配置缓存（用于断线重连）
    connection_configs: tokio::sync::RwLock<HashMap<ConnId, DriverConnectionConfig>>,
    /// 连接最后访问时间（用于空闲回收）
    last_access: tokio::sync::RwLock<HashMap<ConnId, Instant>>,
    /// 当前活动连接 ID
    active_conn_id: tokio::sync::RwLock<Option<ConnId>>,
    /// 隧道守卫注册表（协议链执行产物）：与连接管理器同生命周期。
    ///
    /// 必须挂在管理器上而不是 `ConnectionService` 实例字段上：生产连接入口是短生命周期的
    /// （每次调用新建服务），守卫若随实例释放，隧道刚建好就会被关掉——真机表现为
    /// 「配了跳板机 / 代理却直连或直接失败」。
    tunnels: connection::chain::TunnelRegistry,
    /// 取消令牌映射（每个连接一个正在执行的查询令牌）
    cancel_tokens: tokio::sync::RwLock<HashMap<ConnId, tokio_util::sync::CancellationToken>>,
    /// 【B4】活动事务：每个连接至多一个。
    ///
    /// 事务对象**持有那条物理连接**（驱动 `begin_transaction` 时拿的就是它），所以事务内的
    /// 语句必须走这里而不是重新取连接——P0.2c 实测：MySQL / PG 并发下会另开物理连接，
    /// 靠池的顺序巧合做会话亲和不可靠（架构 §12 #2）。
    transactions: tokio::sync::Mutex<HashMap<ConnId, ActiveTransaction>>,
    /// 空闲超时时间（默认 30 分钟）
    idle_timeout: tokio::sync::RwLock<Duration>,
}

/// 一个活动事务（事务对象 + 开始时刻）
///
/// 事务对象本身不能跨驱动统一成 SQL 文本：MySQL 的显式 `BEGIN` 会被 prepared 协议拒绍
/// （1295），必须走驱动的 `begin_transaction`（P0.2 的结论）。
pub struct ActiveTransaction {
    tx: Box<dyn Transaction>,
    /// 开始时刻（界面显示事务时长用；`Instant` 单调，不受系统时间调整影响）
    started_at: std::time::Instant,
}

impl ActiveTransaction {
    /// 事务已持续多久
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

impl ConnectionManager {
    /// 创建新的连接管理器
    pub fn new() -> Self {
        Self {
            connections: tokio::sync::RwLock::new(HashMap::new()),
            connection_info: tokio::sync::RwLock::new(HashMap::new()),
            connection_configs: tokio::sync::RwLock::new(HashMap::new()),
            last_access: tokio::sync::RwLock::new(HashMap::new()),
            active_conn_id: tokio::sync::RwLock::new(None),
            tunnels: connection::chain::TunnelRegistry::new(),
            cancel_tokens: tokio::sync::RwLock::new(HashMap::new()),
            transactions: tokio::sync::Mutex::new(HashMap::new()),
            idle_timeout: tokio::sync::RwLock::new(Duration::from_secs(30 * 60)),
        }
    }

    /// 使用 DriverRegistry 创建数据库连接
    ///
    /// 这是推荐的连接创建方式，通过 DriverRegistry 动态发现和创建连接
    ///
    /// # Arguments
    ///
    /// * `config` - 驱动连接配置（来自 driver/registry.rs）
    ///
    /// # Returns
    ///
    /// 返回 (连接ID, 动态数据库实例) 元组
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config = ConnectionConfig {
    ///     driver: "mysql".to_string(),
    ///     host: Some("localhost".to_string()),
    ///     port: Some(3306),
    ///     database: Some("test".to_string()),
    ///     username: Some("root".to_string()),
    ///     password: Some("password".to_string()),
    ///     ..Default::default()
    /// };
    /// let (conn_id, db) = manager.create_connection_with_registry(config).await?;
    /// ```ignore
    pub async fn create_connection_with_registry(
        &self,
        config: DriverConnectionConfig,
    ) -> Result<(ConnId, DynDatabase), CoreError> {
        let driver_id = &config.driver;

        // 从 DriverRegistry 获取驱动工厂
        let factory = DriverRegistry::get(driver_id).ok_or_else(|| {
            CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.clone(),
            })
        })?;

        // 使用工厂创建数据库连接
        let db = factory.create(config.clone()).await?;

        // 生成连接 ID
        let conn_id = create_connection_id_from_config(&config);

        // 创建连接信息
        let info = ConnectionInfo {
            id: conn_id.clone(),
            name: config.name.clone().unwrap_or_else(|| driver_id.clone()),
            db_type: driver_id.clone(),
            url: config.to_url().unwrap_or_else(|_| String::new()),
            server_version: None,
            connection_type: ConnectionType::Global,
            project_id: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
            description: None,
            // 这条路径（按驱动配置直接建连）不走连接对话框的开关：默认不参与联邦源清单
            use_duckdb_fed: false,
            created_at: std::time::Instant::now(),
        };

        // 添加到连接管理器
        self.add_connection(conn_id.clone(), db.clone(), info, config)
            .await?;

        Ok((conn_id, db))
    }

    /// 添加数据库连接
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接唯一标识
    /// * `db` - 数据库连接实例
    /// * `info` - 连接信息
    ///
    /// # Returns
    ///
    /// 如果添加成功返回 Ok(())，否则返回 CoreError
    pub async fn add_connection(
        &self,
        conn_id: ConnId,
        db: DynDatabase,
        info: ConnectionInfo,
        config: DriverConnectionConfig,
    ) -> Result<(), CoreError> {
        let mut connections = self.connections.write().await;
        let mut conn_info = self.connection_info.write().await;
        let mut conn_configs = self.connection_configs.write().await;
        let mut access = self.last_access.write().await;

        connections.insert(conn_id.clone(), db);
        conn_info.insert(conn_id.clone(), info);
        conn_configs.insert(conn_id.clone(), config);
        access.insert(conn_id.clone(), Instant::now());

        // 如果没有活动连接，将此连接设为活动连接
        let mut active_conn = self.active_conn_id.write().await;
        if active_conn.is_none() {
            *active_conn = Some(conn_id);
        }

        Ok(())
    }

    /// 记录连接访问时间（重置空闲计时器）
    async fn touch_connection(&self, conn_id: &ConnId) {
        let mut access = self.last_access.write().await;
        access.insert(conn_id.clone(), Instant::now());
    }

    /// 获取指定连接
    ///
    /// 同时更新最后访问时间，用于空闲回收判断。
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接唯一标识
    ///
    /// # Returns
    ///
    /// 如果连接存在返回 Some(DynDatabase)，否则返回 None
    pub async fn get_connection(&self, conn_id: &ConnId) -> Option<DynDatabase> {
        let connections = self.connections.read().await;
        let db = connections.get(conn_id).cloned();
        if db.is_some() {
            drop(connections);
            self.touch_connection(conn_id).await;
        }
        db
    }

    /// 获取当前活动连接
    ///
    /// # Returns
    ///
    /// 如果存在活动连接返回 Some((ConnId, DynDatabase))，否则返回 None
    pub async fn get_active_connection(&self) -> Option<(ConnId, DynDatabase)> {
        let active_conn_id = self.active_conn_id.read().await;
        if let Some(conn_id) = active_conn_id.as_ref() {
            self.get_connection(conn_id)
                .await
                .map(|db| (conn_id.clone(), db))
        } else {
            None
        }
    }

    /// 设置当前活动连接
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 要设为活动的连接 ID
    ///
    /// # Returns
    ///
    /// 如果连接存在并设置成功返回 true，否则返回 false
    pub async fn set_active_connection(&self, conn_id: ConnId) -> bool {
        let connections = self.connections.read().await;
        if connections.contains_key(&conn_id) {
            let mut active_conn = self.active_conn_id.write().await;
            *active_conn = Some(conn_id);
            true
        } else {
            false
        }
    }

    /// 获取当前活动连接的 ID
    pub async fn get_active_connection_id(&self) -> Option<ConnId> {
        let active_conn = self.active_conn_id.read().await;
        active_conn.clone()
    }

    /// 获取当前活动连接的 ID（别名）
    pub async fn get_active_conn_id(&self) -> Option<ConnId> {
        self.get_active_connection_id().await
    }

    /// 切换活动连接
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 要切换到的连接 ID
    ///
    /// # Returns
    ///
    /// 如果连接存在返回 Ok(())，否则返回 CoreError
    pub async fn switch_connection(&self, conn_id: &ConnId) -> Result<(), CoreError> {
        if !self.has_connection(conn_id).await {
            return Err(shared::error::CoreError::connection(
                shared::error::ConnectionError::NotFound(conn_id.clone()),
            ));
        }
        self.set_active_connection(conn_id.clone()).await;
        Ok(())
    }

    /// 移除连接
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 要移除的连接 ID
    pub async fn remove_connection(&self, conn_id: &ConnId) {
        let mut connections = self.connections.write().await;
        let mut conn_info = self.connection_info.write().await;
        let mut conn_configs = self.connection_configs.write().await;
        let mut access = self.last_access.write().await;

        connections.remove(conn_id);
        conn_info.remove(conn_id);
        conn_configs.remove(conn_id);
        access.remove(conn_id);

        // 如果移除的是活动连接，清空活动连接
        let mut active_conn = self.active_conn_id.write().await;
        if active_conn.as_ref() == Some(conn_id) {
            *active_conn = None;
        }

        // 清理与该连接相关的所有缓存
        let cache_manager = CacheManager::instance();
        let conn_id_str = conn_id.to_string();
        std::thread::spawn(move || {
            if let Ok(manager) = cache_manager.lock() {
                manager.invalidate_connection(&conn_id_str);
            }
        });
    }

    /// 获取所有连接 ID
    pub async fn get_all_connection_ids(&self) -> Vec<ConnId> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }

    /// 获取所有连接信息
    pub async fn get_all_connection_info(&self) -> Vec<ConnectionInfo> {
        let conn_info = self.connection_info.read().await;
        conn_info.values().cloned().collect()
    }

    /// 获取指定连接的信息
    pub async fn get_connection_info(&self, conn_id: &ConnId) -> Option<ConnectionInfo> {
        let conn_info = self.connection_info.read().await;
        conn_info.get(conn_id).cloned()
    }

    /// 更新连接信息（用于连接类型转换）
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID
    /// * `info` - 新的连接信息
    ///
    /// # Returns
    ///
    /// 如果连接存在并更新成功返回 Ok(())，否则返回 CoreError
    pub async fn update_connection_info(
        &self,
        conn_id: &ConnId,
        info: ConnectionInfo,
    ) -> Result<(), CoreError> {
        let mut conn_info = self.connection_info.write().await;
        if conn_info.contains_key(conn_id) {
            conn_info.insert(conn_id.clone(), info);
            Ok(())
        } else {
            Err(shared::error::CoreError::connection(
                shared::error::ConnectionError::NotFound(conn_id.clone()),
            ))
        }
    }

    /// 检查连接是否存在
    pub async fn has_connection(&self, conn_id: &ConnId) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(conn_id)
    }

    /// 获取连接数量
    pub async fn connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    /// 关闭所有连接
    pub async fn close_all_connections(&self) {
        let mut connections = self.connections.write().await;
        let mut conn_info = self.connection_info.write().await;
        let mut conn_configs = self.connection_configs.write().await;
        let mut access = self.last_access.write().await;

        connections.clear();
        conn_info.clear();
        conn_configs.clear();
        access.clear();

        let mut active_conn = self.active_conn_id.write().await;
        *active_conn = None;
    }

    /// 关闭指定连接
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 要关闭的连接 ID
    ///
    /// # Returns
    ///
    /// 如果连接存在并关闭成功返回 true，否则返回 false
    pub async fn close_connection(&self, conn_id: &ConnId) -> bool {
        if self.has_connection(conn_id).await {
            self.remove_connection(conn_id).await;
            true
        } else {
            false
        }
    }

    /// 为指定连接创建取消令牌
    ///
    /// 取消旧令牌并创建新令牌，用于后续查询取消
    pub async fn create_cancel_token(
        &self,
        conn_id: &ConnId,
    ) -> tokio_util::sync::CancellationToken {
        let token = tokio_util::sync::CancellationToken::new();
        let mut tokens = self.cancel_tokens.write().await;
        tokens.insert(conn_id.clone(), token.clone());
        token
    }

    /// 取消指定连接的正在执行的查询
    ///
    /// 返回 true 表示存在并已触发取消，false 表示没有正在执行的查询
    pub async fn cancel_query(&self, conn_id: &ConnId) -> bool {
        let tokens = self.cancel_tokens.read().await;
        if let Some(token) = tokens.get(conn_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /* ===== 事务（B4） ===== */

    /// 开始事务：拿**驱动**的事务对象存下来（不再是拼一句 `BEGIN`）
    ///
    /// 已有活动事务时拒绝并给可读原因：嵌套事务是 `SAVEPOINT` 的事，不在这里假装支持。
    /// 事务对象持有那条物理连接，后续语句必须走 [`Self::query_in_transaction`]。
    pub async fn begin_transaction(&self, conn_id: &str) -> Result<(), CoreError> {
        let db = self.get_or_reconnect(&conn_id.to_string()).await?;
        let mut transactions = self.transactions.lock().await;
        if transactions.contains_key(conn_id) {
            return Err(transaction_error(
                "begin",
                TransactionState::InProgress,
                "这个连接上已经有事务在跑（先提交或回滚）",
            ));
        }
        let tx = db.begin_transaction().await?;
        transactions.insert(
            conn_id.to_string(),
            ActiveTransaction {
                tx,
                started_at: Instant::now(),
            },
        );
        Ok(())
    }

    /// 提交并移除活动事务，返回它持续了多久（消息里用得上）
    pub async fn commit_transaction(&self, conn_id: &str) -> Result<Duration, CoreError> {
        let mut transactions = self.transactions.lock().await;
        let Some(mut active) = transactions.remove(conn_id) else {
            return Err(transaction_error(
                "commit",
                TransactionState::NotStarted,
                "这个连接上没有活动事务",
            ));
        };
        let elapsed = active.elapsed();
        active.tx.commit().await.map_err(|error| {
            // 提交失败就不再把它当成“还开着”：如实报错，比留一个说不清状态的句柄强
            transaction_error("commit", TransactionState::Failed(error.to_string()), &error.to_string())
        })?;
        Ok(elapsed)
    }

    /// 回滚并移除活动事务，返回它持续了多久
    pub async fn rollback_transaction(&self, conn_id: &str) -> Result<Duration, CoreError> {
        let mut transactions = self.transactions.lock().await;
        let Some(mut active) = transactions.remove(conn_id) else {
            return Err(transaction_error(
                "rollback",
                TransactionState::NotStarted,
                "这个连接上没有活动事务",
            ));
        };
        let elapsed = active.elapsed();
        active.tx.rollback().await.map_err(|error| {
            transaction_error(
                "rollback",
                TransactionState::Failed(error.to_string()),
                &error.to_string(),
            )
        })?;
        Ok(elapsed)
    }

    /// 事务已开了多久（`None` = 没有活动事务）——界面上的 TX 时长读的就是它
    pub async fn transaction_elapsed(&self, conn_id: &str) -> Option<Duration> {
        self.transactions
            .lock()
            .await
            .get(conn_id)
            .map(ActiveTransaction::elapsed)
    }

    /// 这个连接上有没有活动事务
    pub async fn has_transaction(&self, conn_id: &str) -> bool {
        self.transactions.lock().await.contains_key(conn_id)
    }

    /// **事务内**执行一条语句
    ///
    /// 返回 `None` = 这个连接上没有活动事务（调用方自己回退到普通执行路径）。
    /// 全程持有槽位：一个连接同时只跑一条事务内语句（事务对象不是可重入的）。
    ///
    /// 注意：这条路径**暂时不支持取消与超时**（驱动的事务对象只有 `query/commit/rollback`，
    /// 没有取消令牌入口）——要补得先在驱动 `Transaction` trait 上开取消口。
    pub async fn query_in_transaction(
        &self,
        conn_id: &str,
        sql: &str,
    ) -> Option<Result<QueryResult, CoreError>> {
        let mut transactions = self.transactions.lock().await;
        let active = transactions.get_mut(conn_id)?;
        Some(active.tx.query(sql).await)
    }

    /// 连接断开时丢掉它的事务（重连后原来的事务已经不在了，留着只会骗界面）
    pub async fn drop_transaction(&self, conn_id: &str) {
        self.transactions.lock().await.remove(conn_id);
    }

    /// 获取连接配置（用于重连）"
    pub async fn get_connection_config(&self, conn_id: &ConnId) -> Option<DriverConnectionConfig> {
        let configs = self.connection_configs.read().await;
        configs.get(conn_id).cloned()
    }

    /// 断线重连（exponential backoff）
    ///
    /// 尝试使用原始配置重新建立数据库连接。
    /// 重试策略：100ms → 200ms → 400ms（最多 3 次）
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 要重连的连接 ID
    ///
    /// # Returns
    ///
    /// 成功返回新的数据库连接实例，失败返回 CoreError
    pub async fn reconnect_connection(&self, conn_id: &ConnId) -> Result<DynDatabase, CoreError> {
        let config = self
            .get_connection_config(conn_id)
            .await
            .ok_or_else(|| CoreError::connection(ConnectionError::NotFound(conn_id.clone())))?;

        let driver_id = config.driver.clone();
        let factory = DriverRegistry::get(&driver_id).ok_or_else(|| {
            CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.clone(),
            })
        })?;

        let base_backoff_ms: u64 = 100;
        let max_retries: u32 = 3;

        for attempt in 0..=max_retries {
            match factory.create(config.clone()).await {
                Ok(db) => {
                    let mut connections = self.connections.write().await;
                    connections.insert(conn_id.clone(), db.clone());
                    info!(
                        conn_id = %conn_id,
                        driver = %driver_id,
                        attempt = attempt + 1,
                        "Connection re-established"
                    );
                    return Ok(db);
                }
                Err(e) if attempt < max_retries => {
                    let delay_ms = base_backoff_ms * 2u64.pow(attempt);
                    warn!(
                        conn_id = %conn_id,
                        driver = %driver_id,
                        attempt = attempt + 1,
                        delay_ms = delay_ms,
                        error = %e,
                        "Reconnect attempt failed, retrying"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                }
                Err(e) => {
                    warn!(
                        conn_id = %conn_id,
                        driver = %driver_id,
                        total_attempts = attempt + 1,
                        error = %e,
                        "All reconnect attempts exhausted"
                    );
                    return Err(e);
                }
            }
        }

        Err(CoreError::connection(ConnectionError::Other {
            conn_id: conn_id.clone(),
            reason: "Reconnect exhausted all retries".to_string(),
        }))
    }

    /// 检查连接健康状态并尝试重连
    ///
    /// 先 ping 检查连接，若失败则尝试重连。
    ///
    /// # Returns
    ///
    /// 返回健康的数据库连接（可能是重连后的新连接）
    pub async fn get_or_reconnect(&self, conn_id: &ConnId) -> Result<DynDatabase, CoreError> {
        if let Some(db) = self.get_connection(conn_id).await {
            match db.ping().await {
                Ok(()) => {
                    self.touch_connection(conn_id).await;
                    return Ok(db);
                }
                Err(e) => {
                    warn!(
                        conn_id = %conn_id,
                        error = %e,
                        "Connection ping failed, attempting reconnect"
                    );
                }
            }
        }

        self.reconnect_connection(conn_id).await
    }

    /// 设置空闲超时时间
    ///
    /// # Arguments
    ///
    /// * `timeout` - 新的空闲超时时间
    pub async fn set_idle_timeout(&self, timeout: Duration) {
        let mut current = self.idle_timeout.write().await;
        *current = timeout;
    }

    /// 获取当前空闲超时时间
    pub async fn get_idle_timeout(&self) -> Duration {
        let timeout = self.idle_timeout.read().await;
        *timeout
    }

    /// 隧道注册表（协议链执行产物；与连接管理器同生命周期）。
    ///
    /// 供 `ConnectionService` 取用：同一个管理器下的所有服务实例共用一张隧道表，
    /// 保证「建隧道的实例」与「断开时释放隧道的实例」可以是不同实例。
    pub fn tunnels(&self) -> &connection::chain::TunnelRegistry {
        &self.tunnels
    }

    /// 回收空闲连接
    ///
    /// 遍历所有连接，关闭超过 `idle_timeout` 未访问的连接。
    /// 返回被回收的连接 ID 列表。
    ///
    /// # Returns
    ///
    /// 被关闭的连接 ID 列表
    pub async fn reclaim_idle_connections(&self) -> Vec<ConnId> {
        let idle_timeout = self.get_idle_timeout().await;
        let now = Instant::now();

        let access = self.last_access.read().await;
        let idle_ids: Vec<ConnId> = access
            .iter()
            .filter(|(_, last)| now.duration_since(**last) > idle_timeout)
            .map(|(id, _)| id.clone())
            .collect();
        drop(access);

        for conn_id in &idle_ids {
            info!(conn_id = %conn_id, "Reclaiming idle connection");
            self.remove_connection(conn_id).await;
        }

        idle_ids
    }

    /// 启动后台空闲回收任务
    ///
    /// 每隔 `check_interval` 检查并回收空闲连接。
    /// 返回一个用于停止任务的 handle。
    ///
    /// # Arguments
    ///
    /// * `check_interval` - 检查间隔（推荐 5 分钟）
    ///
    /// # Returns
    ///
    /// 返回一个 `CancellationToken`，调用 `cancel()` 可停止后台任务
    pub fn start_idle_reclaimer(
        self: Arc<Self>,
        check_interval: Duration,
    ) -> tokio_util::sync::CancellationToken {
        let cancel = tokio_util::sync::CancellationToken::new();
        let cancel_clone = cancel.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(check_interval) => {
                        let reclaimed = self.reclaim_idle_connections().await;
                        if !reclaimed.is_empty() {
                            info!(
                                count = reclaimed.len(),
                                ids = ?reclaimed,
                                "Background reclaimer closed idle connections"
                            );
                        }
                    }
                    _ = cancel_clone.cancelled() => {
                        info!("Idle connection reclaimer stopped");
                        break;
                    }
                }
            }
        });

        cancel
    }
    /// 移除指定连接的取消令牌（查询完成后清理）
    pub async fn remove_cancel_token(&self, conn_id: &ConnId) {
        let mut tokens = self.cancel_tokens.write().await;
        tokens.remove(conn_id);
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局连接管理器实例
pub static CONNECTION_MANAGER: OnceLock<Arc<ConnectionManager>> = OnceLock::new();

/// 获取全局连接管理器实例
///
/// 使用 OnceLock 确保全局只有一个连接管理器实例
///
/// # Returns
///
/// 返回全局连接管理器的静态引用
pub fn get_connection_manager() -> &'static Arc<ConnectionManager> {
    CONNECTION_MANAGER.get_or_init(|| Arc::new(ConnectionManager::new()))
}

/// 创建连接 ID
///
/// 根据数据库类型和 URL 生成唯一的连接 ID
///
/// # Arguments
///
/// * `db_type` - 数据库类型（如 "mysql", "postgres"）
/// * `url` - 数据库连接 URL
///
/// # Returns
///
/// 返回生成的连接 ID
pub fn create_connection_id(db_type: &str, url: &str) -> ConnId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    db_type.hash(&mut hasher);
    url.hash(&mut hasher);
    let hash = hasher.finish();

    format!("{}-{:x}", db_type, hash)
}

/// 从 DriverConnectionConfig 创建连接 ID
///
/// 根据驱动配置生成唯一的连接 ID
///
/// # Arguments
///
/// * `config` - 驱动连接配置
///
/// # Returns
///
/// 返回生成的连接 ID
fn create_connection_id_from_config(config: &DriverConnectionConfig) -> ConnId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    config.driver.hash(&mut hasher);
    config.host.hash(&mut hasher);
    config.port.hash(&mut hasher);
    config.database.hash(&mut hasher);
    let hash = hasher.finish();

    format!("{}-{:x}", config.driver, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_manager_new() {
        let manager = ConnectionManager::new();
        assert_eq!(manager.connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_create_connection_id() {
        let id1 = create_connection_id("mysql", "mysql://localhost:3306/test");
        let id2 = create_connection_id("mysql", "mysql://localhost:3306/test");
        let id3 = create_connection_id("postgres", "postgres://localhost:5432/test");

        // 相同的输入应该生成相同的 ID
        assert_eq!(id1, id2);
        // 不同的输入应该生成不同的 ID
        assert_ne!(id1, id3);
    }
}
