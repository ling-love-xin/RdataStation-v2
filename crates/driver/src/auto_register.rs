//! 驱动自动注册模块
//!
//! 提供内置驱动的统一注册入口。
//! 驱动列表的唯一真相源在 [BuiltinDriverDiscovery::builtin_factories()]。
//!
//! 新增内置驱动流程：
//! 1. 在 driver/factory.rs 实现 DriverFactory
//! 2. 在 driver/native/<db>.rs 实现 Database trait
//! 3. 在 loader.rs BuiltinDriverDiscovery::builtin_factories() 添加一行 → 唯一修改点
//!    （auto_register.rs 和 manager.rs 自动同步，无需修改）

use crate::DriverRegistry;
use crate::loader::BuiltinDriverDiscovery;

/// 驱动自动注册器
///
/// 通过 [BuiltinDriverDiscovery::builtin_factories()] 获取内置驱动工厂列表，
/// 统一注册到全局 [DriverRegistry]。
pub struct AutoDriverRegistrar;

impl AutoDriverRegistrar {
    /// 注册所有内置驱动（委托 BuiltinDriverDiscovery）
    pub fn register_builtin_drivers() {
        for factory in BuiltinDriverDiscovery::builtin_factories() {
            let id = factory.descriptor().id.clone();
            DriverRegistry::register_by_factory(id, factory);
        }

        #[cfg(debug_assertions)]
        tracing::debug!(
            "Builtin drivers registered: {:?}",
            DriverRegistry::all_driver_ids()
        );
    }

    /// 自动注册所有驱动
    pub fn auto_register() {
        Self::register_builtin_drivers();

        #[cfg(debug_assertions)]
        tracing::debug!(
            "Auto registration complete. Registered drivers: {:?}",
            DriverRegistry::all_driver_ids()
        );
    }
}

/// 驱动注册宏
///
/// 简化驱动注册代码
#[macro_export]
macro_rules! register_driver {
    ($factory:ty) => {
        $crate::DriverRegistry::register($factory);
    };
}

/// 批量注册驱动宏
#[macro_export]
macro_rules! register_drivers {
    ($($factory:ty),+ $(,)?) => {
        $(
            $crate::DriverRegistry::register($factory);
        )+
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_register() {
        // 确保测试时不会 panic
        AutoDriverRegistrar::register_builtin_drivers();

        let drivers = DriverRegistry::all_driver_ids();
        assert!(!drivers.is_empty());
        // 内核只注册**每族一条官方实现**（D3 之后）。种子 `drivers.id`（global/008 + 013）
        // 里另有 `mysql` / `postgres` 两行，它们的实现在 sidecar 插件里（`rds.driver.sqlx`），
        // 由插件加载时贡献，不走这里的注册表 —— 那两行的处置（含未安装插件时的文案）见
        // `sqlx-driver-extraction-plan.md` D4。
        for id in ["mysql_native", "postgres_native", "sqlite", "duckdb"] {
            assert!(
                drivers.iter().any(|d| d == id),
                "内置驱动未注册：{id}（已注册：{drivers:?}）"
            );
        }
        for id in ["mysql", "postgres"] {
            assert!(
                !drivers.iter().any(|d| d == id),
                "内核不应再注册 {id}：实现已剥离到插件（注册了说明剥离没做干净）：{drivers:?}"
            );
        }
    }
}
