//! rds-settings — 命令（GPUI Action）。
//!
//! - `OpenSettings`：打开设置页（workbench 活动栏底部 ⚙ / `Ctrl+,` / Quick Open；
//!   面板开关状态由 workbench 持有，本命令仅作统一入口，处理逻辑在 workbench `on_action`）。
//! - `OpenResultPanelSettings`：打开设置页并**定位到「结果集」节**——对齐 DBeaver 结果集
//!   状态栏的 `ConfigAction`（`ResultSetViewer.java:5402-5418`：齿轮 →
//!   `UIUtils.showPreferencesFor(…, PrefPageResultSetMain.PAGE_ID)`）。结果面板自己不认识
//!   settings crate（依赖只向下），只推请求；由宿主派发本动作。
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
        OpenResultPanelSettings,
        CloseSettings,
        FocusSettingsSearch,
        ToggleThemeMode
    ]
);
