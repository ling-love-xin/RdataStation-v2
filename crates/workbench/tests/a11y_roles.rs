//! a11y 角色断言（2026-10-16）——gpui-kit 观测。
//!
//! 主界面已补 `.id(...)` + `.role(...)` 的元素（本仓 a11y 修复的「有 id 无 role /
//! 无 id」两类告警消除项）：用 `TestWindowExt` 取快照断言 `role` 与可见性。
//!
//! 说明：`note_focus_without_node` 告警**只在真实 UIA 连接（a11y 激活）时打印**，
//! headless 测试窗口捕获不到；这里的断言验证「role 正确 + 元素在树里可见」——
//! 即告警的两个前提（有 id、有 role）在代码侧已满足，真机验证看日志归零。
//!
//! 宿主说明：面板元素渲染在 dock 面板内，headless 下 `WorkbenchView` 不会自动展开
//! 所有 dock 面板，所以**每个面板单独作为宿主渲染**（与 `connection_render_matrix.rs`
//! 同构），保证被断言元素真实出现在元素树里。
//!
//! 运行：`cargo test -p rds-workbench --test a11y_roles -j 1`

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, ElementId, Entity, SharedString, TestAppContext, VisualTestContext,
};

use rds_workbench::panels::{EditorPanel, RightSidebarPanel, Shared, SidebarPanel};
use rds_workbench::view::WorkbenchView;
use settings::model::Settings;

/// 打开主界面窗口（`WorkbenchView::new_for_test` 装配：跳过真机副作用，渲染同真机路径）。
fn open_workbench(cx: &mut TestAppContext) -> (Entity<WorkbenchView>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<WorkbenchView>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let wb = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            WorkbenchView::new_for_test(None, cx)
        });
        *slot_in.borrow_mut() = Some(wb.clone());
        Root::new(wb, window, cx)
    });
    let wb = slot.borrow().clone().expect("WorkbenchView 已创建");
    (wb, cx)
}

/// 打开 SidebarPanel 宿主（左侧面板；构造需要 editor shared 端口）。
fn open_sidebar(cx: &mut TestAppContext) -> (Entity<SidebarPanel>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<SidebarPanel>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let shared = Shared::new();
        let editor_service = editor::shared::EditorShared::new();
        let panel = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            SidebarPanel::new(shared.clone(), &editor_service, cx)
        });
        let _ = window;
        *slot_in.borrow_mut() = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = slot.borrow().clone().expect("SidebarPanel 已创建");
    (panel, cx)
}

/// 打开 RightSidebarPanel 宿主（右侧面板）。
fn open_right_sidebar(cx: &mut TestAppContext) -> (Entity<RightSidebarPanel>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<RightSidebarPanel>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let shared = Shared::new();
        let panel = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            RightSidebarPanel::new(shared.clone(), window, cx)
        });
        *slot_in.borrow_mut() = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = slot.borrow().clone().expect("RightSidebarPanel 已创建");
    (panel, cx)
}

/// 打开 EditorPanel 宿主（中央编辑面板）。
fn open_editor(cx: &mut TestAppContext) -> (Entity<EditorPanel>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<EditorPanel>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let shared = Shared::new();
        let panel = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            EditorPanel::new(shared.clone(), cx)
        });
        *slot_in.borrow_mut() = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = slot.borrow().clone().expect("EditorPanel 已创建");
    (panel, cx)
}

/// 按 id 取快照（不在观测注册表就 panic，带已登记路径）。
fn snap(cx: &mut VisualTestContext, id: &str) -> gpui_kit::test::ElementSnapshot {
    cx.update(|window, _| window.find(ElementId::Name(SharedString::from(id.to_string()))))
}

/// WorkbenchView 主界面：quick-open 入口（键盘唤起）应是 Button。
#[gpui_kit::test]
fn workbench_quick_open_exposes_button_role(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_wb, cx) = open_workbench(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 启动焦点探针：headless 下读窗口当前焦点 handle（与真机 `note_focus_without_node`
    // 的 FocusId 可对号；headless 不激活 a11y，仅记录焦点归属）。
    let focused = cx.update(|window, app| window.focused(app));
    match focused {
        Some(h) => println!("headless 启动焦点 handle={:?}", h),
        None => println!("headless 启动无焦点（窗口未聚焦任何元素）"),
    }

    let qo = snap(cx, "quick-open-entry");
    assert_eq!(
        qo.role(),
        Some(gpui_kit::Role::Button),
        "quick-open-entry 应为 Button（键盘唤起入口）"
    );
    assert!(qo.visible(), "quick-open-entry 应可见");
}

/// SidebarPanel：左侧面板宿主应有 Group role 且可见。
#[gpui_kit::test]
fn sidebar_panel_exposes_group_role(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_panel, cx) = open_sidebar(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let s = snap(cx, "sidebar-panel");
    assert_eq!(
        s.role(),
        Some(gpui_kit::Role::Group),
        "sidebar-panel 应有 Group role（告警前提之一已消除）"
    );
    assert!(s.visible(), "sidebar-panel 应可见");
}

/// RightSidebarPanel：右侧面板宿主应有 Group role 且可见。
#[gpui_kit::test]
fn right_sidebar_panel_exposes_group_role(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_panel, cx) = open_right_sidebar(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let s = snap(cx, "right-sidebar-panel");
    assert_eq!(
        s.role(),
        Some(gpui_kit::Role::Group),
        "right-sidebar-panel 应有 Group role"
    );
    assert!(s.visible(), "right-sidebar-panel 应可见");
}

/// EditorPanel：中央编辑面板宿主应有 Group role 且可见。
#[gpui_kit::test]
fn editor_panel_exposes_group_role(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_panel, cx) = open_editor(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let s = snap(cx, "editor-panel");
    assert_eq!(
        s.role(),
        Some(gpui_kit::Role::Group),
        "editor-panel 应有 Group role"
    );
    assert!(s.visible(), "editor-panel 应可见");
}
