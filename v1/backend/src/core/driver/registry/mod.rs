//! Driver 注册表
//!
//! 提供类似 DBeaver 的驱动管理能力，包括：
//! - 驱动工厂 trait（DriverFactory）
//! - 全局注册表（DriverRegistry）
//! - 连接配置模型（config 子模块）
//! - 驱动描述符定义（descriptors 子模块）

pub mod config;
pub mod descriptors;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, OnceLock, RwLock};

use crate::core::driver::traits::Database;
use crate::core::error::CoreError;

pub use config::DriverConnectionConfig;
pub use descriptors::{
    duckdb_driver, get_all_drivers, get_driver, mysql_driver, mysql_native_driver, postgres_driver,
    postgres_native_driver, sqlite_driver, DriverDescriptor, DriverField, DriverFieldType,
    DriverKind, DriverOption, DriverOptionType,
};

/// 动态数据库类型
pub type DynDatabase = Arc<dyn Database + Send + Sync>;

/// Driver 工厂 Trait
///
/// 每个数据库驱动需要实现此 trait，并在启动时注册到 DriverRegistry
///
/// # 示例
///
/// ```rust
/// pub struct MySqlDriverFactory;
///
/// impl DriverFactory for MySqlDriverFactory {
///     fn descriptor(&self) -> DriverDescriptor {
///         mysql_driver()
///     }
///
///     fn create(&self, config: DriverConnectionConfig) -> Pin<Box<dyn Future<Output = Result<DynDatabase, CoreError>> + Send>> {
///         Box::pin(async move {
///             let url = config.to_url()?;
///             let db = MySqlDatabase::new(&url).await?;
///             Ok(Arc::new(db))
///         })
///     }
/// }
/// ```
pub trait DriverFactory: Send + Sync {
    /// 获取驱动描述符（用于前端渲染表单）
    fn descriptor(&self) -> DriverDescriptor;

    /// 创建数据库连接
    ///
    /// # Arguments
    ///
    /// * `config` - 连接配置
    ///
    /// # Returns
    ///
    /// 返回动态数据库实例的 Future
    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<DynDatabase, CoreError>> + Send>>;
}

/// 全局 Driver Registry 存储
static DRIVER_REGISTRY: OnceLock<RwLock<HashMap<String, Arc<dyn DriverFactory>>>> = OnceLock::new();

/// Driver Registry
///
/// 管理所有已注册的驱动工厂，提供驱动发现和创建功能
///
/// # 使用示例
///
/// ```rust
/// // 注册驱动（在 lib.rs 或 main.rs 中）
/// DriverRegistry::register(MySqlDriverFactory);
/// DriverRegistry::register(PostgresDriverFactory);
///
/// // 获取驱动
/// if let Some(factory) = DriverRegistry::get("mysql") {
///     let db = factory.create(config).await?;
/// }
/// ```
pub struct DriverRegistry;

impl DriverRegistry {
    /// 注册驱动工厂
    ///
    /// # Arguments
    ///
    /// * `factory` - 实现 DriverFactory 的驱动工厂实例
    ///
    /// # Example
    ///
    /// ```rust
    /// DriverRegistry::register(MySqlDriverFactory);
    /// ```
    pub fn register<F>(factory: F)
    where
        F: DriverFactory + 'static,
    {
        let registry = DRIVER_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));
        let descriptor = factory.descriptor();
        let id = descriptor.id.clone();

        if let Ok(mut reg) = registry.write() {
            reg.insert(id, Arc::new(factory));
        }
    }

    /// 注册已构建的驱动工厂 Arc（配合 BuiltinDriverDiscovery 使用）
    ///
    /// 与 register() 不同，此方法接受已构建的 Arc<dyn DriverFactory>，
    /// 避免二次包装。主要用于批量注册场景。
    pub fn register_by_factory(id: String, factory: Arc<dyn DriverFactory>) {
        let registry = DRIVER_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));
        if let Ok(mut reg) = registry.write() {
            reg.insert(id, factory);
        }
    }

    /// 批量注册驱动工厂
    ///
    /// 一次性注册多个驱动工厂，减少锁竞争。
    pub fn register_batch(factories: Vec<(String, Arc<dyn DriverFactory>)>) {
        if factories.is_empty() {
            return;
        }
        let registry = DRIVER_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));
        if let Ok(mut reg) = registry.write() {
            for (id, factory) in factories {
                reg.insert(id, factory);
            }
        }
    }

    /// 根据 ID 获取驱动工厂
    ///
    /// # Arguments
    ///
    /// * `id` - 驱动 ID，如 "mysql", "postgres"
    ///
    /// # Returns
    ///
    /// 返回驱动工厂的 Arc 引用，如果不存在返回 None
    pub fn get(id: &str) -> Option<Arc<dyn DriverFactory>> {
        DRIVER_REGISTRY
            .get()
            .and_then(|registry| registry.read().ok().and_then(|reg| reg.get(id).cloned()))
    }

    /// 获取所有已注册驱动的描述符
    ///
    /// # Returns
    ///
    /// 返回所有驱动描述符列表
    pub fn all_descriptors() -> Vec<DriverDescriptor> {
        DRIVER_REGISTRY
            .get()
            .map(|registry| {
                registry
                    .read()
                    .ok()
                    .map(|reg| reg.values().map(|f| f.descriptor()).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// 获取所有已注册的驱动 ID
    ///
    /// # Returns
    ///
    /// 返回驱动 ID 列表
    pub fn all_driver_ids() -> Vec<String> {
        DRIVER_REGISTRY
            .get()
            .map(|registry| {
                registry
                    .read()
                    .ok()
                    .map(|reg| reg.keys().cloned().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// 检查驱动是否已注册
    ///
    /// # Arguments
    ///
    /// * `id` - 驱动 ID
    ///
    /// # Returns
    ///
    /// 如果驱动已注册返回 true
    pub fn is_registered(id: &str) -> bool {
        DRIVER_REGISTRY
            .get()
            .map(|registry| {
                registry
                    .read()
                    .ok()
                    .map(|reg| reg.contains_key(id))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// 注销驱动
    ///
    /// # Arguments
    ///
    /// * `id` - 驱动 ID
    ///
    /// # Returns
    ///
    /// 如果成功注销返回 true
    pub fn unregister(id: &str) -> bool {
        DRIVER_REGISTRY
            .get()
            .and_then(|registry| {
                registry
                    .write()
                    .ok()
                    .map(|mut reg| reg.remove(id).is_some())
            })
            .unwrap_or(false)
    }

    /// 清空所有注册的驱动
    pub fn clear() {
        if let Some(registry) = DRIVER_REGISTRY.get() {
            if let Ok(mut reg) = registry.write() {
                reg.clear();
            }
        }
    }
}
