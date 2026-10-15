//! 热重载薄封装（历史遗留的空壳模块，2026-10-15 落地）。
//!
//! 热插拔与重载的**真实实现**在 [`crate::manager::PluginManager`]：
//!
//! - 热插拔 = [`PluginManager::scan_and_diff_plugins`]（目录即插件，挂在运行时循环
//!   每个 tick 做 diff：新目录自动加载/激活、目录消失自动停用、清单变化置重载）
//! - 全量重载 = [`PluginManager::reload_plugins`]（VS Code「Reload Window」口径：
//!   先停全部已激活插件，再重扫磁盘重新加载激活）
//! - 待重载清单 = [`PluginManager::pending_reloads`]（插件面板 / 状态栏据此提示）
//!
//! 本模块只做一次薄转发，避免「重载入口」散落两处（既有代码已有引用本模块名的
//! 历史占位，摘掉会让引用点失效，故保留模块并让其成为 manager 的门面）。

use std::sync::Arc;

use crate::manager::{PluginDiff, PluginManager};

/// 热重载门面：持有 [`PluginManager`]，转发 reload / diff 查询。
#[derive(Clone)]
pub struct HotReloadManager {
    manager: Arc<PluginManager>,
}

impl HotReloadManager {
    /// 绑定管理器（`init_plugin_manager` 之后用 `get_plugin_manager()` 拿到的同一实例）。
    pub fn new(manager: Arc<PluginManager>) -> Self {
        Self { manager }
    }

    /// 全量重载（VS Code「Reload Window」口径）：停全部 → 清重载标记 → 重扫加载激活。
    pub async fn reload_plugins(&self) -> Result<PluginDiff, shared::error::CoreError> {
        self.manager.reload_plugins().await
    }

    /// 跑一次热插拔 diff 扫描（等价于运行时循环一个 tick 里的扫描动作）。
    pub async fn scan_and_diff_plugins(&self) -> Result<PluginDiff, shared::error::CoreError> {
        self.manager.scan_and_diff_plugins().await
    }

    /// 待重载插件列表（清单已变化、未热生效，需要用户触发重载）。
    pub fn pending_reloads(&self) -> Vec<String> {
        self.manager.pending_reloads()
    }
}
