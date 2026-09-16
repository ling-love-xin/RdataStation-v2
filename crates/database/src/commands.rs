//! rds-database — 导航树的键盘动作（GPUI Action）。
//!
//! 这些动作的**绑定**在 app 层（`crates/app/src/main.rs`，key context `database-nav`），
//! **处理**在导航视图里（`database::nav_view`，`.on_action(...)`）。定义随视图归本 crate；
//! `workbench::commands` 重导，app 侧路径不变。
//!
//! `NavUp` / `NavDown` 移动**光标**，`NavReorderUp` / `NavReorderDown` 移动**条目**（换顺序）。

use gpui_kit::*;

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
