//! rds-workbench — commands。
//!
//! 工作台全局动作（GPUI Action）：
//! - `ToggleQuickOpen`：唤起 / 关闭 Quick Open 默认档（`Ctrl+P`，搜索 + 命令融合）；
//! - `ToggleQuickGoto`：唤起 / 关闭「跳转到对象」档（`Ctrl+Shift+O`，仅元数据、主动作回树定位）；
//! - `HideSidebars` / `RestoreSidebars`：三模式之"完全隐藏 / 恢复"；
//! - `FocusNavSearch`：聚焦数据源导航搜索框（Ctrl+F，先切到数据源面板）；
//! - `Nav*`（`NavUp` / `NavDown` / `NavExpand` / `NavCollapse` / `NavOpenProperties` /
//!   `NavReorderUp` / `NavReorderDown`）：数据源导航树键盘导航——**定义在
//!   `database::commands`**（随视图归位），此处重导保持 app 侧 `workbench::commands::Nav*` 路径不变；
//! - `GenerateMock`：Mock 面板的「生成」——**定义在 `mock::commands`**，同样在此重导；
//! - 各 panel 切换、打开设置等命令由 Quick Open 与活动栏直接驱动（见 `view.rs`）。
//!
//! 按编码规范「事件、Action 与焦点」：动作只更新共享状态并 notify，Dock 同步在
//! `WorkbenchView::render` 统一执行（render 是模式权威同步点）。

use gpui_kit::*;

actions!(
    workbench,
    [
        ToggleQuickOpen,
        ToggleQuickGoto,
        QuickOpenLocate,
        HideSidebars,
        RestoreSidebars,
        SwitchProject,
        CloseProject,
        FocusNavSearch
    ]
);

// 数据源导航（M4）局部动作：定义在 `database::commands`（随导航视图归位），此处重导。
pub use database::commands::{
    NavClearSearch, NavCollapse, NavCommitStaged, NavDeleteToStaged, NavDown, NavEnd, NavExpand,
    NavHome, NavOpenProperties, NavPageDown, NavPageUp, NavRename, NavReorderDown, NavReorderUp,
    NavRevertAllStaged, NavUp,
};

// 草稿箱（M5）局部动作：定义在 `scratchpad::commands`（随草稿箱视图归位），此处重导。
pub use scratchpad::commands::{
    ScratchpadCancelEdit, ScratchpadDelete, ScratchpadDown, ScratchpadNewFile, ScratchpadOpen,
    ScratchpadRename, ScratchpadSelectAll, ScratchpadUp,
};

// Mock 生成（M7）局部动作：定义在 `mock::commands`（随 Mock 视图归位），此处重导。
pub use mock::commands::GenerateMock;

// 连接对话框（M3）局部动作：定义在 `connection-dialog::commands`（随对话框下沉，2026-10-15），此处重导。
pub use connection_dialog::commands::{DraftNext, DraftPrev, SaveConnection, TestConnection};
