//! 启动设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

/// 与 `Shared::new` 的构造默认保持一致（设置未写入时行为不变）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Startup {
    /// 启动时左侧栏默认显示的模块（`database` / `draft` / `resources` / `plugin`）。
    #[serde(default = "default_startup_left_panel")]
    pub default_left_panel: String,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            default_left_panel: default_startup_left_panel(),
        }
    }
}

fn default_startup_left_panel() -> String {
    "database".to_string()
}
