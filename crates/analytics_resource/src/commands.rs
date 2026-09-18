//! 资产库 Action 与快捷键（M6）。
//!
//! Action 在 crate 内声明（面板用 `.on_action` 处理），**快捷键在 app 层绑定**
//! （`crates/app/src/main.rs` 的 `analytics-resource` context）——按 GPUI-kit 规范，
//! 声明与绑定分离，视图不自己注册全局键。
//!
//! 本批只声明**已在面板接上处理器的动作**——已声明的 Action 必须在面板里有 `.on_action`
//! 实现，否则"键绑上了但什么也不发生"比没有更难排查。
//!
//! 不在本文件的键：行漫游（`↑↓`）由列表组件自己的选中通道处理，打开（`Enter`）走列表的
//! 确认通道（`ArchiveListDelegate::confirm`）——行的**鼠标**点击（选中 / 多选 / 双击打开）
//! 由行自己接管（组件默认把单击也接到确认上，与原型不符，见 `classify_row_click`）；
//! `F2` 重命名待重命名入口（Phase 2）。

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
        /// 把当前选中存档移入回收站（多选时对整个选择集生效）。
        DeleteSelected,
        /// 全选当前可见行（`Ctrl+A`；分组未落，即全部可见行）。
        SelectAllRows,
        /// 聚焦工具栏搜索框（`Ctrl+F`）。
        FocusSearch,
        /// 清空搜索词（`Esc`）——只清搜索，不连同种类 / 只看需处理一起清
        /// （那两个条件在菜单里看得见，误清会让人以为筛选坏了）。
        ClearSearch,
    ]
);
