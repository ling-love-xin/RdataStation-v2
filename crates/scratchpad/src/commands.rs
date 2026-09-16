//! rds-scratchpad — 草稿箱键盘动作（GPUI Action）。
//!
//! 绑定在 app 层（`crates/app/src/main.rs`，key context `scratchpad`），处理在草稿箱视图里
//! （`crate::scratchpad_view`）。定义随视图归本 crate；`workbench::commands` 重导，
//! app 侧路径不变。

use gpui_kit::*;

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
