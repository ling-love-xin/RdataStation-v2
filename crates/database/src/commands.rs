//! rds-database — 导航树的键盘动作（GPUI Action）。
//!
//! 这些动作的**绑定**在 app 层（`crates/app/src/main.rs`，key context `database-nav`），
//! **处理**在导航视图里（`database::nav_view`，`.on_action(...)`）。定义随视图归本 crate；
//! `workbench::commands` 重导，app 侧路径不变。
//!
//! `NavUp` / `NavDown` 移动**光标**，`NavReorderUp` / `NavReorderDown` 移动**条目**（换顺序）。
//! `NavClearSearch`（`Esc`）只清搜索框里的词，不动 facet 筛选。

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
        NavReorderDown,
        /// 清空搜索框（`Esc`）——只清自由文本，**不动**归属域 / 类型 / 驱动 / 标签这些 facet：
        /// 它们在 chips 与「筛选 ▾」里看得见，误清会让人以为筛选坏了。
        /// 语义与资产库的 `ClearSearch` 一致（原型设计 §6.3 的「`Esc` 清空」）。
        NavClearSearch
    ]
);
