use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tracing::{info, warn};

use specta::Type;

use crate::core::cache::CacheManager;
use crate::core::driver::registry::DriverConnectionConfig;
use crate::core::driver::traits::DynDatabase;
use crate::core::driver::DriverRegistry;
use crate::core::error::{ConnectionError, CoreError};

/// 连接 ID 类型
pub type ConnId = String;

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
    /// 取消令牌映射（每个连接一个正在执行的查询令牌）
    cancel_tokens: tokio::sync::RwLock<HashMap<ConnId, tokio_util::sync::CancellationToken>>,
    /// 空闲超时时间（默认 30 分钟）
    idle_timeout: tokio::sync::RwLock<Duration>,
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
            cancel_tokens: tokio::sync::RwLock::new(HashMap::new()),
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
    /// ```rust
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
    /// ```
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
            return Err(crate::core::error::CoreError::connection(
                crate::core::error::ConnectionError::NotFound(conn_id.clone()),
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
            Err(crate::core::error::CoreError::connection(
                crate::core::error::ConnectionError::NotFound(conn_id.clone()),
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
