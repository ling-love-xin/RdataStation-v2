//! 布局设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

/// 布局设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    /// 是否锁定布局（用户不能通过拖动调整面板大小或位置）。
    #[serde(default = "default_layout_locked")]
    pub locked: bool,
    /// 左侧侧边栏宽度（px）。
    #[serde(default = "default_layout_left_sidebar_width")]
    pub left_sidebar_width: i64,
    /// 右侧侧边栏宽度（px）。
    #[serde(default = "default_layout_right_sidebar_width")]
    pub right_sidebar_width: i64,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            locked: default_layout_locked(),
            left_sidebar_width: default_layout_left_sidebar_width(),
            right_sidebar_width: default_layout_right_sidebar_width(),
        }
    }
}

/// 布局锁定：默认关闭。
fn default_layout_locked() -> bool {
    false
}

/// 左侧侧边栏宽度：默认 250px。
fn default_layout_left_sidebar_width() -> i64 {
    250
}

/// 右侧侧边栏宽度：默认 300px。
fn default_layout_right_sidebar_width() -> i64 {
    300
}
