//! rds-workbench — commands。
//!
//! 工作台全局动作（GPUI Action）：
//! - `ToggleQuickOpen`：唤起 / 关闭 Quick Open（搜索 + 命令融合，Ctrl+P 绑定于 app 层）；
//! - `HideSidebars` / `RestoreSidebars`：三模式之"完全隐藏 / 恢复"；
//! - 各 panel 切换、打开设置等命令由 Quick Open 与活动栏直接驱动（见 `view.rs`）。
//!
//! 按编码规范「事件、Action 与焦点」：动作只更新共享状态并 notify，Dock 同步在
//! `WorkbenchView::render` 统一执行（render 是模式权威同步点）。

use gpui_kit::*;

actions!(
    workbench,
    [
        ToggleQuickOpen,
        HideSidebars,
        RestoreSidebars,
        SwitchProject,
        CloseProject
    ]
);
