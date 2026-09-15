//! rds-workbench — commands。
//!
//! 工作台全局动作（GPUI Action）：
//! - `ToggleQuickOpen`：唤起 / 关闭 Quick Open（搜索 + 命令融合，Ctrl+P 绑定于 app 层）；
//! - `HideSidebars` / `RestoreSidebars`：三模式之"完全隐藏 / 恢复"；
//! - `FocusNavSearch`：聚焦数据源导航搜索框（Ctrl+F，先切到数据源面板）；
//! - `NavUp` / `NavDown` / `NavExpand` / `NavCollapse` / `NavOpenProperties`：
//!   数据源导航树键盘导航（仅 `database-nav` key context 内生效）；
//! - `NavReorderUp` / `NavReorderDown`：把选中连接在所属容器内上移 / 下移一位（`Alt+↑/↓`）。
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
        CloseProject,
        FocusNavSearch
    ]
);

// 数据源导航（M4）局部动作：绑定在导航面板的 `key_context("database-nav")` 上。
// `NavUp`/`NavDown` 移动**光标**，`NavReorderUp`/`NavReorderDown` 移动**条目**（换顺序）。
actions!(
    database_nav,
    [
        NavUp,
        NavDown,
        NavExpand,
        NavCollapse,
        NavOpenProperties,
        NavReorderUp,
        NavReorderDown
    ]
);

// 草稿箱（M5）局部动作：绑定在草稿箱面板的 `key_context("scratchpad")` 上。
actions!(
    scratchpad,
    [
        ScratchpadSelectAll,
        ScratchpadRename,
        ScratchpadDelete,
        ScratchpadCancelEdit,
        ScratchpadUp,
        ScratchpadDown,
        ScratchpadOpen,
        ScratchpadNewFile
    ]
);

// 连接对话框（M3）局部动作：仅绑定在为对话框容器声明的 key context 上。
//
// - `SaveConnection`：Ctrl+Enter 保存（焦点在输入框内时由 Enter action 冒泡兜底）；
// - `TestConnection`：Ctrl+T 测试连接；
// - `DraftPrev` / `DraftNext`：↑↓ 切换暂存列表条目（输入框内 ↑↓ 仍由 Input 处理）。
actions!(
    connection_dialog,
    [SaveConnection, TestConnection, DraftPrev, DraftNext]
);
