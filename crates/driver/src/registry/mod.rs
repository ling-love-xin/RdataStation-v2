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

use crate::traits::Database;
use shared::error::CoreError;

pub use config::DriverConnectionConfig;
pub use descriptors::{
    DriverDescriptor, DriverField, DriverFieldType, DriverKind, DriverOption, DriverOptionType,
    duckdb_driver, get_all_drivers, get_driver, mysql_native_driver, postgres_native_driver,
    sqlite_driver,
};

/// 动态数据库类型
pub type DynDatabase = Arc<dyn Database + Send + Sync>;

/// Driver 工厂 Trait
///
/// 每个数据库驱动需要实现此 trait，并在启动时注册到 DriverRegistry
///
/// # 示例
///
/// ```ignore
/// pub struct MySqlNativeDriverFactory;
///
/// impl DriverFactory for MySqlNativeDriverFactory {
///     fn descriptor(&self) -> DriverDescriptor {
///         mysql_native_driver()
///     }
///
///     fn create(&self, config: DriverConnectionConfig) -> Pin<Box<dyn Future<Output = Result<DynDatabase, CoreError>> + Send>> {
///         Box::pin(async move {
///             let url = config.to_url()?;
///             let db = MySqlNativeDatabase::new(&url).await?;
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
/// ```ignore
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
    /// ```ignore
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

    /// 按「实现 id → 族」两级解析驱动工厂。
    ///
    /// 先按 id 直查（用户明确选中的那套实现）；查不到再把入参当作**族名**，
    /// 在同族已注册的实现里挑一个。
    ///
    /// 为什么要这一步：连接记录里的 `driver` / `driver_id` 可能指向一套已被停用或
    /// 卸载的插件驱动，而同族的内置实现仍在注册表里——那时应当回退，而不是直接报
    /// 「驱动不存在」，把一条本来能开的连接变成打不开。
    ///
    /// 返回 `(实际命中的 id, 工厂)`；命中 id 与入参不同即说明发生了回退，调用方应记日志。
    pub fn resolve_family(driver_id: &str) -> Option<(String, Arc<dyn DriverFactory>)> {
        if let Some(factory) = Self::get(driver_id) {
            return Some((driver_id.to_string(), factory));
        }
        let registry = DRIVER_REGISTRY.get()?;
        let reg = registry.read().ok()?;
        first_in_family(&reg, driver_id)
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

/// 在**同族**的已注册实现里挑一个。
///
/// 按 id 字典序取第一个：`HashMap` 的遍历顺序不定，不排序会让同一份数据每次挑到
/// 不同的实现（用户会看到「今天走 A 驱动、明天走 B 驱动」）。
///
/// 族的权威字段是 `DriverDescriptor::target_database`——内置 4 个驱动都显式写了，
/// 插件驱动的族由清单 `type_id` 喂进来；**不要按 id 字符串前缀猜**。
fn first_in_family(
    reg: &HashMap<String, Arc<dyn DriverFactory>>,
    family: &str,
) -> Option<(String, Arc<dyn DriverFactory>)> {
    reg.iter()
        .filter(|(_, factory)| factory.descriptor().target_database.as_deref() == Some(family))
        .min_by_key(|(id, _)| (*id).clone())
        .map(|(id, factory)| (id.clone(), Arc::clone(factory)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只放两个内置实现（`mysql_native` 族 `mysql`、`sqlite` 族 `sqlite`）。
    /// 用局部 map 而不是全局注册表，避免测试之间互相污染。
    fn sample_registry() -> HashMap<String, Arc<dyn DriverFactory>> {
        let mut map: HashMap<String, Arc<dyn DriverFactory>> = HashMap::new();
        map.insert(
            "mysql_native".to_string(),
            Arc::new(crate::factory::MySqlNativeDriverFactory),
        );
        map.insert(
            "sqlite".to_string(),
            Arc::new(crate::factory::SqliteDriverFactory),
        );
        map
    }

    #[test]
    fn family_picks_an_implementation_within_the_same_family() {
        let reg = sample_registry();
        // 只给族名 → 回退到同族的实现
        let (id, _) = first_in_family(&reg, "mysql").expect("mysql 族应当有实现");
        assert_eq!(id, "mysql_native");
        // 族名与实现同名时也不冲突（sqlite 既是族也是实现 id）
        let (id, _) = first_in_family(&reg, "sqlite").expect("sqlite 族");
        assert_eq!(id, "sqlite");
        // 不存在的族不凭空造工厂
        assert!(first_in_family(&reg, "no_such_family_at_all").is_none());
    }
}
