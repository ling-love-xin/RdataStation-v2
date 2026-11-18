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
    TestAppContext, VisualTestContext, Window, point,
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

/// 形态（配置中心页原型）：进域先看列表，点「新建」才进表单，「返回列表」回得去。
///
/// 钉住两件事：默认态是列表（此前列表与表单同屏，条目一多表单被顶出可视区）；
/// 两态互斥（表单开着时列表的「新建」不该还挂在那儿）。
#[gpui_kit::test]
fn manager_panel_is_list_first_then_toggles_form(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);

    cx.update(|window, cx| {
        harness.update(cx, |h, cx| {
            h.panel
                .update(cx, |panel, cx| panel.show_domain(ManagerKind::Auth, None, window, cx));
        });
    });
    cx.run_until_parked();

    assert!(visible(cx, "mgr-new"), "列表态应看到「新建」");
    assert!(!visible(cx, "mgr-back"), "列表态不该有返回栏");

    // 点「新建」→ 表单态。
    let new_btn = cx.update(|window, _| {
        window.find(ElementId::Name(SharedString::from("mgr-new")))
    });
    let center = point(
        new_btn.bounds().size.width / 2.,
        new_btn.bounds().size.height / 2.,
    );
    cx.update(|window, cx| {
        window.click_at(
            ElementId::Name(SharedString::from("mgr-new")),
            center,
            cx,
        )
    });
    cx.run_until_parked();

    assert!(visible(cx, "mgr-back"), "表单态应出现「‹ 返回列表」");
    assert!(!visible(cx, "mgr-new"), "表单态不该还挂着「新建」");

    // 点「返回列表」→ 回到列表态。
    let back_btn = cx.update(|window, _| {
        window.find(ElementId::Name(SharedString::from("mgr-back")))
    });
    let center = point(
        back_btn.bounds().size.width / 2.,
        back_btn.bounds().size.height / 2.,
    );
    cx.update(|window, cx| {
        window.click_at(
            ElementId::Name(SharedString::from("mgr-back")),
            center,
            cx,
        )
    });
    cx.run_until_parked();

    assert!(visible(cx, "mgr-new"), "返回列表后「新建」应回来");
    assert!(!visible(cx, "mgr-back"), "返回列表后表单应关掉");
}
