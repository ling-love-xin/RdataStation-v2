//! 配置中心页：窗口级测试（GPUI headless）。
//!
//! 覆盖三件事（其余交给真实服务）：
//! 1. 页面首次打开才建（未打开时 `current_kind` 为 `None`）；
//! 2. `show_domain` 落域并渲染出对应导航项（全局库未初始化时列表为空但不得 panic）；
//! 3. 页面已建后切域只切域、不重建（`current_kind` 随之变化）。
//!
//! 与 `connection_dialog_ui.rs` 同规矩：不通配导入（避免 gpui 的 `test` 宏被带进作用域）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Context, ElementId, Entity, IntoElement, Render, SharedString,
    TestAppContext, VisualTestContext, Window,
};

use connection_dialog::ManagerKind;
use rds_workbench::panels::ConfigCenterPanel;

/// 元素是否**真可见**（在 kit 观测里 + 没被裁掉 / 隐藏 / 透明）。
fn visible(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.update(|window, _| {
        window
            .try_find(ElementId::Name(SharedString::from(id.to_string())))
            .is_some_and(|s| s.visible())
    })
}

struct PageHarness {
    panel: Entity<ConfigCenterPanel>,
}

impl Render for PageHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.panel.clone()
    }
}

fn open_harness(cx: &mut TestAppContext) -> (Entity<PageHarness>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<PageHarness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        if !cx.has_global::<settings::model::Settings>() {
            cx.set_global(settings::model::Settings::default());
        }
        let harness = cx.new(|cx| PageHarness {
            panel: cx.new(ConfigCenterPanel::new),
        });
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

/// 读当前域（测试上下文 → `App`）。
fn current_kind(
    panel: &Entity<ConfigCenterPanel>,
    cx: &mut VisualTestContext,
) -> Option<ManagerKind> {
    cx.update(|_, app| panel.read(app).current_kind())
}

#[gpui_kit::test]
fn page_is_created_lazily_then_switches_domain(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);

    let panel = cx.update(|_, app| harness.read(app).panel.clone());
    assert_eq!(
        current_kind(&panel, cx),
        None,
        "未打开任何域前不应建页（建页要查库）"
    );

    cx.update(|window, cx| {
        harness.update(cx, |h, cx| {
            h.panel
                .update(cx, |panel, cx| panel.show_domain(ManagerKind::Auth, None, window, cx));
        });
    });
    assert_eq!(current_kind(&panel, cx), Some(ManagerKind::Auth));

    cx.run_until_parked();
    assert!(visible(cx, "cc-nav-0"), "认证域导航项应渲染");
    assert!(visible(cx, "cc-nav-1"), "三域导航常驻，网络项应同时可见");

    cx.update(|window, cx| {
        harness.update(cx, |h, cx| {
            h.panel.update(cx, |panel, cx| {
                panel.show_domain(ManagerKind::Network, None, window, cx)
            });
        });
    });
    assert_eq!(current_kind(&panel, cx), Some(ManagerKind::Network));

    cx.run_until_parked();
    // 同域再点不重建：切回认证仍只切域。
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| {
            h.panel
                .update(cx, |panel, cx| panel.show_domain(ManagerKind::Auth, None, window, cx));
        });
    });
    assert_eq!(current_kind(&panel, cx), Some(ManagerKind::Auth));
}
