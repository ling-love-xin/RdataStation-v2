//! rds-settings — 命令（GPUI Action）。
//!
//! - `OpenSettings`：打开设置面板（workbench 活动栏底部 ⚙ 触发；面板开关状态
//!   由 workbench 持有，本命令仅作统一入口，处理逻辑在 workbench `on_action`）。
//! - `ToggleThemeMode`：明暗主题切换（app 层绑定快捷键，经 `SettingsService`
//!   即时生效并持久化）。

use gpui_kit::*;

actions!(settings, [OpenSettings, ToggleThemeMode]);
