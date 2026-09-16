//! 资产库 Action 与快捷键（M6）。
//!
//! Action 在 crate 内声明（面板用 `.on_action` 处理），**快捷键在 app 层绑定**
//! （`crates/app/src/main.rs` 的 `analytics-resource` context）——按 GPUI-kit 规范，
//! 声明与绑定分离，视图不自己注册全局键。
//!
//! 本批只声明**已在面板接上处理器的动作**——已声明的 Action 必须在面板里有 `.on_action`
//! 实现，否则"键绑上了但什么也不发生"比没有更难排查。
//!
//! 不在本文件的键：行漫游（`↑↓`）与打开（`Enter`）由列表组件自己的选中/确认通道处理
//! （面板把选中回传与 `confirm` 接在 `ArchiveListDelegate` 上）；`F2` 重命名与
//! `Ctrl+A` 全选待各自的对话框 / 多选批。

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
        /// 聚焦工具栏搜索框（`Ctrl+F`）。
        FocusSearch,
        /// 清空搜索词（`Esc`）——只清搜索，不连同种类 / 只看需处理一起清
        /// （那两个条件在菜单里看得见，误清会让人以为筛选坏了）。
        ClearSearch,
    ]
);
