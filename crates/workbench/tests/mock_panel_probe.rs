//! 临时排查测试（2026-10-16）：复现「点 Mock 面板后无新增表入口」的装配级验证。
//!
//! 链路：open_right_panel(Mock) → 右 Dock 渲染表配置行（mock-draft-open）→ 点击
//! → host.open_detail(Draft) → 中央创建 MockDetailView tab（mock_details["draft"]）
//! → 渲染空态起手卡片（mock-detail-scenario-empty）。
//!
//! 验证后按用户决定保留或删除。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, ElementId, Entity, SharedString, TestAppContext, VisualTestContext,
};

use project::ui::OpenProject;
use rds_workbench::panels::{RightSidebarPanel, Shared};
use rds_workbench::view::{RightPanel, WorkbenchView};
use settings::model::Settings;

/// 单独渲染右侧面板宿主（隔离 WorkbenchView 装配，a11y_roles 同构）。
fn open_right_sidebar(
    cx: &mut TestAppContext,
) -> (Entity<RightSidebarPanel>, Shared, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<(Entity<RightSidebarPanel>, Shared)>>> =
        Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let shared = Shared::new();
        let panel = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            RightSidebarPanel::new(shared.clone(), window, cx)
        });
        *slot_in.borrow_mut() = Some((panel.clone(), shared));
        Root::new(panel, window, cx)
    });
    let (panel, shared) = slot.borrow().clone().expect("RightSidebarPanel 已创建");
    (panel, shared, cx)
}

/// 打开主界面窗口（与 `a11y_roles.rs` 同构的测试装配）。
fn open_workbench(cx: &mut TestAppContext) -> (Entity<WorkbenchView>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<WorkbenchView>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let wb = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            // 有项目：中央才渲染 DockArea（无项目时 render 用项目选择器整块覆盖中央，
            // 左右 Dock 面板全部不渲染——见 view.rs render 的 no_project 分支）
            let project = Some(OpenProject::from_root(PathBuf::from("D:/probe-project")));
            WorkbenchView::new_for_test(project, cx)
        });
        *slot_in.borrow_mut() = Some(wb.clone());
        Root::new(wb, window, cx)
    });
    let wb = slot.borrow().clone().expect("WorkbenchView 已创建");
    (wb, cx)
}

fn snap(cx: &mut VisualTestContext, id: &str) -> gpui_kit::test::ElementSnapshot {
    cx.update(|window, _| window.find(ElementId::Name(SharedString::from(id.to_string()))))
}

/// 场景 0：无项目时点 Mock 图标——右 Dock 照常渲染（bug 修复验证）。
///
/// 原状：无项目时 render 用项目选择器覆盖中央、DockArea 不挂树，点 Mock 图标只
/// 点亮图标、看不到任何面板。修复后项目选择器迁入中央 tab 组（`ProjectPickerPanel`），
/// DockArea 恒挂树——Mock 面板「纯生成不受影响」（mock-architecture I8「未打开
/// 项目也能跑」），无项目时点 Mock 图标必须能打开右 Dock。
#[gpui_kit::test]
fn no_project_mock_panel_still_renders(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let slot: Rc<RefCell<Option<Entity<WorkbenchView>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let wb = cx.new(|cx| {
            if !cx.has_global::<Settings>() {
                cx.set_global(Settings::default());
            }
            // 无项目：与真机启动默认一致
            WorkbenchView::new_for_test(None, cx)
        });
        *slot_in.borrow_mut() = Some(wb.clone());
        Root::new(wb, window, cx)
    });
    let wb = slot.borrow().clone().expect("WorkbenchView 已创建");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 无项目：中央应是「项目」选择器 tab（打开项目入口，面板化后仍可见）
    let picker = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from("project-picker-panel")))
    });
    assert!(
        picker.is_some(),
        "无项目时中央应有项目选择器面板（project-picker-panel）"
    );

    // 点 Mock 图标 → 右 Dock 打开 → 表配置行可见（原 bug：这里什么都看不到）
    let shared = cx.update(|_, app| wb.read(app).shared_for_test());
    cx.update(|_, app| shared.open_right_panel(RightPanel::Mock, app));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 活动栏项已迁移为 `SidebarMenuItem`（0.8.1 组件无业务 id，旧 `right-activity-*`
    // id 断言失效）：高亮由 `active(selected)` 承担，此处断言右 Dock 打开后的实际渲染。
    let dock =
        cx.update(|window, _| window.try_find(ElementId::Name(SharedString::from("dock-area"))));
    assert!(dock.is_some(), "无项目时 DockArea 也应在树里");

    let draft = snap(cx, "mock-draft-open");
    assert!(
        draft.visible(),
        "无项目时右 Dock 应显示表配置行（mock-draft-open）"
    );
}

/// 场景 1：打开 Mock 面板后，右 Dock 应渲染表配置行（`mock-draft-open`，○ 未生成）。
#[gpui_kit::test]
fn mock_panel_open_renders_draft_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (wb, cx) = open_workbench(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let shared = cx.update(|_, app| wb.read(app).shared_for_test());
    cx.update(|_, app| shared.open_right_panel(RightPanel::Mock, app));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 诊断：面板状态（results / scenario / draft 默认值）
    cx.update(|_, app| {
        if let Some(panel) = shared
            .mock_panel
            .borrow()
            .clone()
            .and_then(|weak| weak.upgrade())
        {
            let p = panel.read(app);
            println!(
                "panel state: results={} scenario={:?} draft_name={:?} draft_rows={} draft_cols={}",
                p.results().len(),
                p.scenario().map(|s| s.name.clone()),
                Some(p.draft().table_name.clone()),
                p.draft().options.rows,
                p.draft().columns.len(),
            );
        } else {
            println!("panel state: mock_panel weak is DEAD");
        }
    });

    let draft = snap(cx, "mock-draft-open");
    assert!(draft.visible(), "右 Dock 应显示表配置行（mock-draft-open）");
}

/// 场景 2：点击表配置行 → 中央应创建 draft tab 且空态起手卡片渲染出来。
#[gpui_kit::test]
fn click_draft_row_opens_central_empty_state(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (wb, cx) = open_workbench(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let shared = cx.update(|_, app| wb.read(app).shared_for_test());
    cx.update(|_, app| shared.open_right_panel(RightPanel::Mock, app));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 点击表配置行（TestWindowExt 自己 render_frame + 命中测试）
    cx.update(|window, app| {
        window.click(ElementId::Name(SharedString::from("mock-draft-open")), app)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 断言 1：中央已创建表配置 tab 实体（键是 `config:{id}`，每份配置一个 tab）
    let alive = cx.update(|_, _| has_config_tab(&shared));
    assert!(
        alive,
        "点击表配置行后 shared.mock_details 应有 config: 开头的表配置 tab"
    );

    // 断言 2：空态起手卡片（选场景模板按钮）已渲染
    let card = snap(cx, "mock-detail-scenario-empty");
    assert!(
        card.visible(),
        "中央空态卡片应渲染（mock-detail-scenario-empty）"
    );
}

/// 场景 4：Mock 面板**工具栏**的「+」按钮（2026-10-18 起 ＋ 从标题栏迁到面板内工具栏）——
/// 可见，且点击打开中央表配置 tab。
///
/// 2026-11-01 起「＋」新表**自带一列**（`new_table_config` 加 `column_1` + Integer 默认生成器），
/// 所以这里落点是**非空态**的列段（`mock-detail-scenario-columns`）；空态卡片
/// （`mock-detail-scenario-empty`）只在一列都没有时才出现，见 `click_draft_row_opens_central_empty_state`。
#[gpui_kit::test]
fn title_suffix_plus_opens_draft_tab(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (wb, cx) = open_workbench(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let shared = cx.update(|_, app| wb.read(app).shared_for_test());
    cx.update(|_, app| shared.open_right_panel(RightPanel::Mock, app));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 工具栏「+」按钮存在且可见（标题栏只留标题，按钮在面板内 toolbar 一行）
    let plus = snap(cx, "mock-toolbar-new-table");
    assert!(
        plus.visible(),
        "Mock 工具栏应有「+」按钮（mock-toolbar-new-table）"
    );

    // 点击「+」→ 打开 / 聚焦中央表配置 tab（与清单表配置行同一目标）
    cx.update(|window, app| {
        window.click(
            ElementId::Name(SharedString::from("mock-toolbar-new-table")),
            app,
        )
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let alive = cx.update(|_, _| has_config_tab(&shared));
    assert!(
        alive,
        "点「+」后 shared.mock_details 应有 config: 开头的表配置 tab"
    );

    // 新表自带一列 → 渲染的是列段（非空态）的起手卡片
    let card = snap(cx, "mock-detail-scenario-columns");
    assert!(
        card.visible(),
        "中央列段应渲染（mock-detail-scenario-columns）"
    );
}

/// 场景 3：单独渲染右侧面板（隔离 WorkbenchView 装配），MockPanel 本身是否渲染表配置行。
#[gpui_kit::test]
fn right_sidebar_alone_renders_draft_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_panel, shared, cx) = open_right_sidebar(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|_, app| shared.open_right_panel(RightPanel::Mock, app));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let draft = snap(cx, "mock-draft-open");
    assert!(
        draft.visible(),
        "单独渲染右侧面板时也应显示表配置行（mock-draft-open）"
    );
}

/// `shared.mock_details` 里是否已有**任意一份**表配置 tab（键形如 `config:{id}`）。
///
/// 多配置改造后每个 tab 的键带配置 id，不再共用一个 `draft` 键；
/// 首次打开的 id 由 `MockPanel` 的初始 `config_id`/`draft_seq` 决定，
/// 所以这里按前缀扫描，不写死具体 id。
fn has_config_tab(shared: &Shared) -> bool {
    let map = shared.mock_details.borrow();
    map.iter()
        .any(|(key, weak)| key.starts_with("config:") && weak.upgrade().is_some())
}
