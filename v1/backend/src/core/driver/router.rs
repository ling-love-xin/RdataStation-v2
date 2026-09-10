//! 数据源路由层
//!
//! 职责：数据源注册、路由、能力发现
//! 不存放具体驱动实现，实现统一在 native/ 中
//!
//! # 架构设计
//!
//! ```
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
use crate::core::error::{ConnectionError, CoreError};

/// 数据源路由器
///
/// 根据驱动 ID 路由到对应的驱动工厂
pub struct DataSourceRouter;

impl DataSourceRouter {
    /// 根据驱动配置创建数据库连接
    pub async fn route(config: DriverConnectionConfig) -> Result<DynDatabase, CoreError> {
        let driver_id = &config.driver;

        let factory = DriverRegistry::get(driver_id).ok_or_else(|| {
            CoreError::connection(ConnectionError::DriverNotFound {
                driver: driver_id.clone(),
            })
        })?;

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
