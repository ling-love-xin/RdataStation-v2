//! rds-settings — 命令（GPUI Action）。
//!
//! - `OpenSettings`：打开设置页（workbench 活动栏底部 ⚙ / `Ctrl+,` / Quick Open；
//!   面板开关状态由 workbench 持有，本命令仅作统一入口，处理逻辑在 workbench `on_action`）。
//! - `CloseSettings`：关闭设置页（`Esc`，仅在 `key_context("settings")` 内生效）。
//! - `FocusSettingsSearch`：把焦点交给搜索框（`Ctrl+F`，仅在设置页内生效）——
//!   注意焦点已在输入框内时，按键会先被内核的 `input::Search` 拿走（同编辑器口径）。
//! - `ToggleThemeMode`：明暗主题切换（`Ctrl+Shift+L`，键位在装配层 `crates/app/src/main.rs` 绑定；
//!   显式模式两态切换，跟随系统时一次按键反切到对侧并脱离跟随）。

use gpui_kit::*;

actions!(
    settings,
    [
        OpenSettings,
        CloseSettings,
        FocusSettingsSearch,
        ToggleThemeMode
    ]
);
