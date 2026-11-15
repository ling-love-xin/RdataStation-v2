//! 数据源路由层
//!
//! 职责：数据源注册、路由、能力发现
//! 不存放具体驱动实现，实现统一在 native/ 中
//!
//! # 架构设计
//!
//! ```text
//! 前端请求
//!     │
//!     ▼
//! DataSourceRouter (路由)
//!     │ 根据 driver_id 查找
//!     ▼
//! DriverRegistry (注册表)
//!     │ 返回 DriverFactory
//!     ▼
//! DriverFactory.create() (创建连接)
//!     │
//!     ▼
//! native/mysql.rs (具体实现)
//! ```

use super::registry::DriverConnectionConfig;
use super::{DriverRegistry, DynDatabase};
use shared::error::{ConnectionError, CoreError};

/// 数据源路由器
///
/// 根据驱动 ID 路由到对应的驱动工厂
pub struct DataSourceRouter;

impl DataSourceRouter {
    /// 根据驱动配置创建数据库连接
    pub async fn route(config: DriverConnectionConfig) -> Result<DynDatabase, CoreError> {
        let driver_id = &config.driver;

        // 先按实现 id 直查，再按族回退：连接记录指向的插件驱动可能已被停用 / 卸载，
        // 但同族的内置实现还在——那时应当接住这条连接，而不是报「驱动不存在」。
        let (resolved_id, factory) = DriverRegistry::resolve_family(driver_id).ok_or_else(|| {
            CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.clone(),
            })
        })?;
        if resolved_id != *driver_id {
            tracing::warn!(
                requested = %driver_id,
                resolved = %resolved_id,
                "驱动未注册，已按同族回退（插件可能已停用或卸载）"
            );
        }

        factory.create(config).await
    }

    /// 检查驱动是否已注册
    pub fn is_driver_registered(driver_id: &str) -> bool {
        DriverRegistry::get(driver_id).is_some()
    }

    /// 获取所有已注册的驱动 ID
    pub fn list_registered_drivers() -> Vec<String> {
        DriverRegistry::all_driver_ids()
    }
}
