//! 【B8 余项】把工具栏「⟲ 历史」接到右 Dock 历史面板（端口注入）。
//!
//! 分工：编辑器只说「打开历史面板」（[`crate::shared::OpenHistoryPort`]），
//! 打开哪一侧的哪个面板是宿主的事——editor crate **不依赖工作台的 Dock**。
//!
//! 历史面板本体（`HistoryView` + `HistoryPort` + 重放）在 workbench 装配；
//! 这里只补工具栏那个缺失的入口（原型 §2.2 的 `⟲ 历史`）。
//!
//! P2-a：经 [`crate::host::EditorHost::open_history`] 转发，不再依赖
//! `workbench::panels::Shared` 的具体类型。

use std::rc::Rc;

use crate::host::EditorHost;
use crate::shared::EditorShared;

/// 注入「打开历史面板」端口（`WorkbenchView::assemble` 装配期调用一次）
pub fn attach(editor: &EditorShared, host: Rc<dyn EditorHost>) {
    editor.attach_open_history(Rc::new(move |cx| {
        host.open_history(cx);
    }));
}
