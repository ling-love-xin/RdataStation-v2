//! rds-settings — 命令（GPUI Action）。
//!
//! - `OpenSettings`：打开设置页（workbench 活动栏底部 ⚙ / `Ctrl+,` / Quick Open；
//!   面板开关状态由 workbench 持有，本命令仅作统一入口，处理逻辑在 workbench `on_action`）。
//! - `CloseSettings`：关闭设置页（`Esc`，仅在 `key_context("settings")` 内生效）。
//! - `FocusSettingsSearch`：把焦点交给搜索框（`Ctrl+F`，仅在设置页内生效）——
//!   注意焦点已在输入框内时，按键会先被内核的 `input::Search` 拿走（同编辑器口径）。
//! - `ToggleThemeMode`：明暗主题切换（**尚未绑键位**：架构 §14 Q2 未拍板，接线还是删除）。

use gpui_kit::*;

actions!(
    settings,
    [OpenSettings, CloseSettings, FocusSettingsSearch, ToggleThemeMode]
);
