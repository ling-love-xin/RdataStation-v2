//! 归档 / 取回对话框的窗口回归：开得出、按钮真在、校验与提交走的是同一条路。
//!
//! 为什么不断言"点按钮"：headless 下 `debug_bounds` 的坐标与鼠标命中测试对不上
//! （编辑器踩过：`simulate_click` 单跑能中、全套跑就中不了），所以这里断言
//! 「对话框层真渲染 + 两个按钮真在」，提交路径直接调 `submit_*`——**与按钮回调、
//! Enter 确认用的是同一个函数**（项目 crate 的对话框用例同一口径）。
//!
//! 两条纪律（与 `panel_window.rs` 同）：
//! 1. **不通配导入**（`use gpui_kit::*` 会把 `test` 属性宏带进作用域）；
//! 2. 实体访问包在 `cx.update(|window, cx| …)` 里。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::{
    AppContext as _, Context, IntoElement, ParentElement, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};

use rds_analytics_resource::dialogs::archive::{
    ArchiveConflict, ArchiveDialogResult, ArchiveDialogSeed, build_inputs,
    open_archive_dialog_with, submit_archive,
};
use rds_analytics_resource::dialogs::checkout::{
    CheckoutDialogResult, CheckoutDialogSeed, build_inputs as build_checkout_inputs,
    open_checkout_dialog_with, suggest_work_copy_name, submit_checkout,
};

/// 窗口根：组件库的 `Root`（`open_dialog` / `render_dialog_layer` 依赖它）。
struct DialogHarness;

impl Render for DialogHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div().size_full();
        if let Some(layer) = Root::render_dialog_layer(window, cx) {
            root = root.child(layer);
        }
        root
    }
}

/// 建一个以 `Root` 为根的窗口。
fn harness(cx: &mut TestAppContext) -> &mut VisualTestContext {
    let (_, cx) =
        cx.add_window_view(|window, cx| Root::new(cx.new(|_cx| DialogHarness), window, cx));
    cx
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn archive_seed(conflict: Option<ArchiveConflict>) -> ArchiveDialogSeed {
    ArchiveDialogSeed {
        source_label: "本地文件：D:\\work\\月报.sql".to_string(),
        rel_path: "resources/月报.sql".to_string(),
        name: "月报".to_string(),
        source_connection: Some("conn_demo".to_string()),
        conflict,
    }
}

fn checkout_seed() -> CheckoutDialogSeed {
    CheckoutDialogSeed {
        resource_name: "月报".to_string(),
        file_name: suggest_work_copy_name("月报", Some("sql")),
        target_dir_label: "D:\\proj\\scratchpad".to_string(),
        version: 3,
    }
}

#[gpui_kit::test]
fn archive_dialog_opens_renders_and_validates(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let submitted: Rc<RefCell<Vec<ArchiveDialogResult>>> = Rc::new(RefCell::new(Vec::new()));

    let seed = archive_seed(None);
    let inputs = cx.update(|window, cx| build_inputs(window, cx, &seed));
    let inputs_for_submit = inputs.clone();
    {
        let submitted = submitted.clone();
        cx.update(|window, cx| {
            open_archive_dialog_with(window, cx, seed, inputs, move |result, _cx| {
                submitted.borrow_mut().push(result);
            });
        });
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "归档对话框应打开"
    );
    assert!(
        cx.debug_bounds("archive-dialog-ok").is_some(),
        "确认按钮要在对话框里（层真渲染，不是只有状态）"
    );
    assert!(cx.debug_bounds("archive-dialog-cancel").is_some());

    // 预填生效：默认值直接可提交（显示名来自文件名，标签与保留份数留空 = 跟随设置）。
    let prefilled = cx
        .update(|_window, cx| submit_archive(&inputs_for_submit, cx))
        .expect("预填后的默认值应合法");
    assert_eq!(prefilled.name, "月报");
    assert!(prefilled.tags.is_empty());
    assert_eq!(prefilled.keep_versions, None, "留空 = 跟随设置默认");

    // 清空显示名：不通过（对话框的按钮回调走的就是这个函数），宿主也收不到回调。
    cx.update(|window, cx| {
        inputs_for_submit
            .name
            .update(cx, |state, cx| state.set_value("  ", window, cx));
    });
    assert!(
        cx.update(|_window, cx| submit_archive(&inputs_for_submit, cx)).is_none(),
        "空显示名不该通过校验"
    );
    assert!(submitted.borrow().is_empty(), "校验不过时宿主不该被通知");

    // 填好：拿到解析后的结果（标签去空去重、保留份数解析成数字）。
    cx.update(|window, cx| {
        inputs_for_submit
            .name
            .update(cx, |state, cx| state.set_value("月报 2026", window, cx));
        inputs_for_submit
            .tags
            .update(cx, |state, cx| state.set_value(" 报表, 月度 ，报表 ", window, cx));
        inputs_for_submit
            .keep_versions
            .update(cx, |state, cx| state.set_value("3", window, cx));
    });
    let result = cx
        .update(|_window, cx| submit_archive(&inputs_for_submit, cx))
        .expect("填好后应通过校验");
    assert_eq!(result.name, "月报 2026");
    assert_eq!(result.tags, vec!["报表", "月度"], "去空、去重、保序");
    assert_eq!(result.keep_versions, Some(3));

    // 非法保留份数：挡住提交（提示由渲染期从同一个解析函数推出，不会两处不一致）。
    cx.update(|window, cx| {
        inputs_for_submit
            .keep_versions
            .update(cx, |state, cx| state.set_value("很多", window, cx));
    });
    assert!(
        cx.update(|_window, cx| submit_archive(&inputs_for_submit, cx)).is_none(),
        "非数字不该通过"
    );
}

#[gpui_kit::test]
fn archive_dialog_with_target_conflict_renders(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);

    // 目标被占用：按钮文案与提示都换成"改名归档"，且照样开得出来（渲染期 no-op 分支不能 panic）。
    let seed = archive_seed(Some(ArchiveConflict {
        taken_rel_path: "resources/月报.sql".to_string(),
        resolved_rel_path: "resources/月报-2.sql".to_string(),
    }));
    let inputs = cx.update(|window, cx| build_inputs(window, cx, &seed));
    cx.update(|window, cx| {
        open_archive_dialog_with(window, cx, seed, inputs, |_, _cx| {});
    });
    draw(cx);

    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    assert!(cx.debug_bounds("archive-dialog-ok").is_some());
}

#[gpui_kit::test]
fn checkout_dialog_opens_and_validates_file_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let submitted: Rc<RefCell<Vec<CheckoutDialogResult>>> = Rc::new(RefCell::new(Vec::new()));

    let seed = checkout_seed();
    let inputs = cx.update(|window, cx| build_checkout_inputs(window, cx, &seed));
    let inputs_for_submit = inputs.clone();
    {
        let submitted = submitted.clone();
        cx.update(|window, cx| {
            open_checkout_dialog_with(window, cx, seed, inputs, move |result, _cx| {
                submitted.borrow_mut().push(result);
            });
        });
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "取回对话框应打开"
    );
    assert!(cx.debug_bounds("checkout-dialog-ok").is_some());
    assert!(cx.debug_bounds("checkout-dialog-cancel").is_some());

    // 默认值合法：文件名按原型拼、且默认勾上"取回后打开"（取回的目的通常是接着改）。
    let result = cx
        .update(|_window, cx| submit_checkout(&inputs_for_submit, cx))
        .expect("默认值应通过校验");
    assert_eq!(result.file_name, "月报（工作副本）.sql");
    assert!(result.open_after);

    // 路径分隔符进不了文件名（挡在对话框，不让它变成一次注定失败的复制）。
    cx.update(|window, cx| {
        inputs_for_submit
            .file_name
            .update(cx, |state, cx| state.set_value("子目录/月报.sql", window, cx));
    });
    assert!(
        cx.update(|_window, cx| submit_checkout(&inputs_for_submit, cx)).is_none(),
        "含路径分隔符的文件名不该通过"
    );
    assert!(submitted.borrow().is_empty());
}
