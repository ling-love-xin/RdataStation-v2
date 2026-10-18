//! 编辑器工具栏「⟲ 历史」→ 右 Dock 历史面板的**宿主接线**（B8 余项）。
//!
//! 链路：工具栏「历史」按钮 → `EditorShared` 上宿主注入的端口
//! （[`workbench::services::editor_history::attach`]）→ `Shared::open_right_panel(RightPanel::History)`
//! （展开右 Dock + 切到历史面板）。
//!
//! 本用例从**端口**那一环往下验（按钮渲染 / 模式分层由 editor 侧用例覆盖）：
//! 调端口，断言右 Dock 展开 + 目标面板确实是历史。历史面板本体（`HistoryView` +
//! 重放）在 B8 已装配，这里不再重测其内部。

use gpui_kit::{AppContext as _, TestAppContext};

use editor::shared::EditorShared;
use rds_workbench::panels::Shared;
use workbench_shell::model::{RightPanel, SidebarMode};

#[gpui_kit::test]
fn the_editor_history_port_opens_the_right_dock_history_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = Shared::with_connections(Vec::new(), None);
    let editor_shared = EditorShared::new();
    // 未接端口时按钮应置灰（render 的判据就是“端口在不在”）
    assert!(
        editor_shared.open_history_port().is_none(),
        "新编辑器实例还没接宿主端口"
    );

    rds_workbench::services::editor_history::attach(&editor_shared, &shared);

    let port = editor_shared
        .open_history_port()
        .expect("attach 之后应有端口");
    cx.update(|cx| port(cx));

    assert_eq!(
        shared.right_mode.get(),
        SidebarMode::Expanded,
        "入口要展开右 Dock，否则用户看不到历史面板"
    );
    assert_eq!(shared.active_right.get(), RightPanel::History);
}
