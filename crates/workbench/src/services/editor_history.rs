//! 【B8 余项】把工具栏「⟲ 历史」接到右 Dock 历史面板（端口注入）。
//!
//! 分工：编辑器只说「打开历史面板」（[`editor::shared::OpenHistoryPort`]），
//! 打开哪一侧的哪个面板是宿主的事——editor crate **不依赖工作台的 Dock**。
//!
//! 历史面板本体（`HistoryView` + `HistoryPort` + 重放）已在 B8 完成并装配在
//! `panels/right.rs`；这里只补工具栏那个缺失的入口（原型 §2.2 的 `⟲ 历史`）。

use std::rc::Rc;

use editor::shared::EditorShared;

use crate::panels::Shared;
use crate::view::RightPanel;

/// 注入「打开历史面板」端口（`WorkbenchView::assemble` 装配期调用一次）
pub fn attach(editor: &EditorShared, workbench: &Shared) {
    let workbench = workbench.clone();
    editor.attach_open_history(Rc::new(move |cx| {
        workbench.open_right_panel(RightPanel::History, cx);
    }));
}
