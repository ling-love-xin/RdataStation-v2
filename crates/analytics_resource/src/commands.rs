//! 资产库 Action 与快捷键（M6）。
//!
//! Action 在 crate 内声明（面板用 `.on_action` 处理），**快捷键在 app 层绑定**
//! （`crates/app/src/main.rs` 的 `analytics-resource` context）——按 GPUI-kit 规范，
//! 声明与绑定分离，视图不自己注册全局键。
//!
//! 本批只声明**已在面板接上处理器的动作**——已声明的 Action 必须在面板里有 `.on_action`
//! 实现，否则"键绑上了但什么也不发生"比没有更难排查。

use gpui_kit::*;

actions!(
    analytics_resource,
    [
        /// 归档入口（等价于点面板头「归档」；草稿箱上下文另绑 `Ctrl+Shift+A`）。
        RequestArchive,
        /// 打开当前选中存档（只读）。
        OpenSelected,
        /// 取回（检出）当前选中存档为草稿工作副本。
        CheckoutSelected,
        /// 把当前选中存档移入回收站。
        DeleteSelected,
    ]
);
