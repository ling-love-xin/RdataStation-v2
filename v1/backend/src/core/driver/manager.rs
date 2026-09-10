use std::sync::Arc;
use tokio::sync::RwLock;

use super::loader::BuiltinDriverDiscovery;
use super::registry::DriverConnectionConfig;
use crate::core::driver::{DriverDescriptor, DriverFactory, DynDatabase};
use crate::core::error::{ConnectionError, CoreError};

/// 驱动状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatus {
    /// 已加载
    Loaded,
    /// 加载中
    Loading,
    /// 加载失败
    Failed(String),
    /// 未加载
    Unloaded,
}

/// 驱动信息
#[derive(Debug, Clone)]
pub struct DriverInfo {
    /// 驱动描述符
    pub descriptor: DriverDescriptor,
    /// 驱动状态
    pub status: DriverStatus,
    /// 加载时间
    pub loaded_at: Option<std::time::Instant>,
}

/// 驱动管理器
///
/// 负责管理驱动的加载、卸载和状态
pub struct DriverManager {
    /// 驱动信息映射
    drivers: RwLock<std::collections::HashMap<String, DriverInfo>>,
    /// 驱动工厂映射
    factories: RwLock<std::collections::HashMap<String, Arc<dyn DriverFactory>>>,
}

impl DriverManager {
    /// 创建新的驱动管理器
    pub fn new() -> Self {
        Self {
            drivers: RwLock::new(std::collections::HashMap::new()),
            factories: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// 注册驱动工厂
    pub async fn register_driver(&self, driver_id: &str, factory: Arc<dyn DriverFactory>) {
        let descriptor = factory.descriptor();

        let mut drivers = self.drivers.write().await;
        drivers.insert(
            driver_id.to_string(),
            DriverInfo {
                descriptor: descriptor.clone(),
                status: DriverStatus::Unloaded,
                loaded_at: None,
            },
        );

        let mut factories = self.factories.write().await;
        factories.insert(driver_id.to_string(), factory);
    }

    /// 加载驱动
    pub async fn load_driver(&self, driver_id: &str) -> Result<(), CoreError> {
        let mut drivers = self.drivers.write().await;

        if let Some(info) = drivers.get_mut(driver_id) {
            if info.status == DriverStatus::Loaded {
                return Ok(());
            }

            info.status = DriverStatus::Loading;
        } else {
            return Err(CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.to_string(),
            }));
        }

        // 这里可以添加驱动加载的逻辑
        // 例如：加载WASM插件、初始化JVM等

        let mut drivers = self.drivers.write().await;
        if let Some(info) = drivers.get_mut(driver_id) {
            info.status = DriverStatus::Loaded;
            info.loaded_at = Some(std::time::Instant::now());
        }

        Ok(())
    }

    /// 卸载驱动
    pub async fn unload_driver(&self, driver_id: &str) -> Result<(), CoreError> {
        let mut drivers = self.drivers.write().await;

        if let Some(info) = drivers.get_mut(driver_id) {
            if info.status == DriverStatus::Unloaded {
                return Ok(());
            }

            // 这里可以添加驱动卸载的逻辑
            // 例如：卸载WASM插件、关闭JVM等

            info.status = DriverStatus::Unloaded;
            info.loaded_at = None;
        } else {
            return Err(CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.to_string(),
            }));
        }

        Ok(())
    }

    /// 获取驱动状态
    pub async fn get_driver_status(&self, driver_id: &str) -> Option<DriverStatus> {
        let drivers = self.drivers.read().await;
        drivers.get(driver_id).map(|info| info.status.clone())
    }

    /// 获取所有驱动信息
    pub async fn get_all_drivers(&self) -> Vec<DriverInfo> {
        let drivers = self.drivers.read().await;
        drivers.values().cloned().collect()
    }

    /// 创建数据库连接
    pub async fn create_connection(
        &self,
        config: DriverConnectionConfig,
    ) -> Result<DynDatabase, CoreError> {
        let driver_id = &config.driver;

        // 确保驱动已加载
        self.load_driver(driver_id).await?;

        let factories = self.factories.read().await;
        let factory = factories.get(driver_id).ok_or_else(|| {
            CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.clone(),
            })
        })?;

        factory.create(config).await
    }
}

impl Default for DriverManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局驱动管理器实例
use std::sync::OnceLock;
pub static DRIVER_MANAGER: OnceLock<DriverManager> = OnceLock::new();

/// 获取全局驱动管理器实例
///
/// 如果尚未初始化，将返回 None。必须先调用 init_driver_manager() 进行初始化。
pub fn get_driver_manager() -> Option<&'static DriverManager> {
    DRIVER_MANAGER.get()
}

/// 初始化全局驱动管理器
///
/// 必须在应用启动时调用此函数，且只能调用一次。
/// 内置驱动列表来自 [BuiltinDriverDiscovery::builtin_factories()] 唯一真相源。
pub async fn init_driver_manager() -> Result<(), CoreError> {
    let manager = DriverManager::new();

    // 从唯一真相源批量注册所有内置驱动
    for factory in BuiltinDriverDiscovery::builtin_factories() {
        let id = factory.descriptor().id.clone();
        manager.register_driver(&id, factory).await;
    }

    DRIVER_MANAGER.set(manager).map_err(|_| {
        CoreError::common(crate::core::error::CommonError::General(
            "Driver manager already initialized".to_string(),
        ))
    })?;

    Ok(())
}
