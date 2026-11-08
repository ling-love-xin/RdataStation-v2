//! 布局设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

use workbench_shell::ui;

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

/// 左侧侧边栏宽度：默认 288px。
///
/// **必须与 Dock 起步宽同源**：`ui::LEFT_DOCK_WIDTH`（18rem）× `ui::BASE_FONT_SIZE`（16px）= 288px。
/// 为什么（2026-11-08 取证）：启动时 `apply_left_mode` 先按 `font_size × LEFT_DOCK_WIDTH` 设宽
/// （`crates/workbench/src/view.rs:1526-1528`）；首帧 render 的「设置值 → Dock」同步
/// （`view.rs:3120-3136`）因为 `prev_left_sidebar_width` 初值是 `0.0`（`panels/shared.rs:420`）
/// 必然触发一次，把宽度覆盖成 `left_width × (font_size / 16)`。两值不一致时面板会在首帧静默缩水：
/// 旧默认 250 在 13px 字号下只剩 203px（比启动的 234px 窄 31px，名称可见字符少约 2 个字）。
fn default_layout_left_sidebar_width() -> i64 {
    (ui::LEFT_DOCK_WIDTH * ui::BASE_FONT_SIZE) as i64
}

/// 右侧侧边栏宽度：默认 320px。
///
/// 与左栏同一口径：`ui::RIGHT_DOCK_WIDTH`（20rem）× `ui::BASE_FONT_SIZE`（16px）= 320px，
/// 与启动时 `apply_right_mode` 的 `font_size × RIGHT_DOCK_WIDTH` 同源
/// （`crates/workbench/src/view.rs:1642` / `:1667`）。旧默认 300 在 13px 字号下会被首帧同步
/// 覆盖成 244px（启动本身是 260px），与左栏旧默认 250→203 是同一类静默缩水。
fn default_layout_right_sidebar_width() -> i64 {
    (ui::RIGHT_DOCK_WIDTH * ui::BASE_FONT_SIZE) as i64
}
