//! 启动设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

/// 与 `Shared::new` 的构造默认保持一致（设置未写入时行为不变）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Startup {
    /// 启动时左侧栏默认显示的模块（`database` / `draft` / `resources` / `plugin`）。
    #[serde(default = "default_startup_left_panel")]
    pub default_left_panel: String,
    /// 启动时自动恢复上次打开的项目（`workbench::services::project_session::resolve`）。
    ///
    /// **默认关**：启动先进空态（欢迎页），由用户自己点项目进来；需要「打开即回到上次」
    /// 的用户可以在设置页打开它。关掉不影响「最近项目」列表本身。
    #[serde(default = "default_restore_last_workspace")]
    pub restore_last_workspace: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            default_left_panel: default_startup_left_panel(),
            restore_last_workspace: default_restore_last_workspace(),
        }
    }
}

fn default_startup_left_panel() -> String {
    "database".to_string()
}

/// 启动恢复上次项目：默认关（2026-11-27 用户口径：先空态、再自己点项目）。
fn default_restore_last_workspace() -> bool {
    false
}
