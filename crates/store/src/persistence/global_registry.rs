//! 全局数据库管理器注册表（单例）
//!
//! 自 engine `migration::global_init` 下沉（2026-10-29，engine 瘦身第二刀）：
//! 注册表是持久化域的全局单例（logging 落库 / 启动编排 / 测试注入都要取它），
//! 与启动编排（`global_init`，依赖驱动声明同步，留在 engine）解耦后放本 crate。

use std::sync::OnceLock;

use shared::error::{CommonError, CoreError};

use crate::persistence::GlobalDatabaseManager;

/// 全局系统数据库管理器实例
///
/// 使用 OnceLock 确保只初始化一次
/// 应用启动时创建，应用关闭时销毁
static GLOBAL_DB_MANAGER: OnceLock<GlobalDatabaseManager> = OnceLock::new();

/// 注入全局库管理器（测试 / 嵌入场景；只能设置一次）。
///
/// 与 `initialize_global_system` 互斥：后到者返回错误。
/// 集成测试可用临时目录构造管理器后注入，从而覆盖
/// `DataSourceService::global()` 等生产单例路径。
pub fn install_global_db_manager(manager: GlobalDatabaseManager) -> Result<(), CoreError> {
    GLOBAL_DB_MANAGER.set(manager).map_err(|_| {
        CoreError::common(CommonError::General(
            "Global database manager already initialized".to_string(),
        ))
    })
}

/// 获取全局系统数据库管理器实例
///
/// 如果尚未初始化，将返回 None
pub fn get_global_db_manager() -> Option<&'static GlobalDatabaseManager> {
    GLOBAL_DB_MANAGER.get()
}
