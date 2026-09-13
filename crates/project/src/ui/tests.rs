//! `ui` 模块的窗口级测试（GPUI headless 窗口）。
//!
//! 用真实窗口验证视图行为：选择器 / 设置 / 菜单内容渲染不 panic、语义对话框开得起来、
//! 未保存拦截与校验走对分支、宿主回调（重绘 / 排序持久化）被调用。
//! 宿主依赖全部用测试桥，不接 workbench 与真实设置服务；全局库未初始化，
//! 因此列表查询走 `Err` 分支（视图需降级显示而不是崩溃）。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的
//! `test` 属性宏带入作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会因此解析到
//! 它自己，造成无限递归（recursion limit reached）。所有依赖显式列举。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme as _, Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, IntoElement, ParentElement, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};

use super::{
    OpenProject, PendingAction, PickerTab, ProjectEditorBridge, ProjectInputs, ProjectSort,
    ProjectUiHost, ProjectUiNotifier, ProjectUiState, advance_pending, confirm_delete, cycle_sort,
    more_menu, open_create_dialog, open_delete_dialog, open_lock_busy_dialog, pick_directory,
    prepare_unsaved, project_card, render_menu_content, render_picker, render_settings,
    request_close, request_create_project, request_open, request_open_folder, submit_create,
};
use crate::service::ProjectSummary;

/// 记录宿主桥被调用的次数与参数。
#[derive(Default)]
struct Recorder {
    notifies: Cell<usize>,
    saved_sort: RefCell<Option<ProjectSort>>,
    dirty: Cell<bool>,
    cleared: Cell<usize>,
    marked_clean: Cell<usize>,
}

struct TestNotifier(Rc<Recorder>);

impl ProjectUiNotifier for TestNotifier {
    fn notify(&self, _cx: &mut App) {
        self.0.notifies.set(self.0.notifies.get() + 1);
    }
}

struct TestEditor(Rc<Recorder>);

impl ProjectEditorBridge for TestEditor {
    fn is_dirty(&self) -> bool {
        self.0.dirty.get()
    }

    fn sql(&self) -> String {
        String::new()
    }

    fn clear(&self, _window: &mut Window, _cx: &mut App) {
        self.0.cleared.set(self.0.cleared.get() + 1);
    }

    fn mark_clean(&self) {
        self.0.dirty.set(false);
        self.0.marked_clean.set(self.0.marked_clean.get() + 1);
    }
}

/// 组装测试宿主；state / session 与视图共享同一份 `Rc`。
fn test_host(rec: &Rc<Recorder>) -> ProjectUiHost {
    let state = Rc::new(RefCell::new(ProjectUiState::default()));
    let session = Rc::new(RefCell::new(None));
    let sort_rec = rec.clone();
    ProjectUiHost::new(state, session, Rc::new(TestNotifier(rec.clone())))
        .with_editor(Rc::new(TestEditor(rec.clone())))
        .with_sort_saver(Rc::new(move |sort, _cx| {
            *sort_rec.saved_sort.borrow_mut() = Some(sort);
        }))
}

/// 视图宿主：选择器 / 设置面板 / 菜单内容按状态渲染，并挂上对话框层。
struct Harness {
    host: ProjectUiHost,
    inputs: ProjectInputs,
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().v_flex().gap_2();
        if self.host.current().is_none() {
            body = body.child(render_picker(&self.host, &self.inputs, cx));
        }
        if let Some(settings) = render_settings(&self.host, &self.inputs, cx) {
            body = body.child(settings);
        }
        // 菜单内容平时由标题栏 `Popover` 承载，这里直接渲染以覆盖其构造路径。
        body = body.child(render_menu_content(&self.host, &self.inputs, cx));
        div()
            .size_full()
            .child(body)
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

/// 打开测试窗口（窗口根是 `Root`，`open_dialog` 依赖它）。
fn open_harness(
    cx: &mut TestAppContext,
    host: ProjectUiHost,
) -> (ProjectUiHost, ProjectInputs, &mut VisualTestContext) {
    let inputs_slot: Rc<RefCell<Option<ProjectInputs>>> = Rc::new(RefCell::new(None));
    let view_host = host.clone();
    let slot = inputs_slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| {
            let inputs = ProjectInputs::new(window, cx);
            *slot.borrow_mut() = Some(inputs.clone());
            Harness {
                host: view_host,
                inputs,
            }
        });
        Root::new(harness, window, cx)
    });
    let inputs = inputs_slot.borrow().clone().expect("inputs 已创建");
    (host, inputs, cx)
}

/// 临时目录（视图只读访问，避免触碰真实项目）。
fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_ui_it_{name}_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 名册条目（crate 内可字面量构造：`#[non_exhaustive]` 只限制外部 crate）。
fn summary(id: &str, name: &str, status: &str) -> ProjectSummary {
    ProjectSummary {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        path: PathBuf::from("C:/tmp/rds-demo"),
        status: status.to_string(),
        is_pinned: false,
        created_at: "2026-09-11 10:00".to_string(),
        updated_at: "2026-09-11 10:00".to_string(),
        last_opened_at: None,
        path_exists: true,
        lock: None,
        missing_drivers: Vec::new(),
    }
}

#[gpui_kit::test]
fn picker_renders_without_project(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, _inputs, cx) = open_harness(cx, host);

    assert!(host.current().is_none());
    // 无项目 → 渲染选择器（空态 + 加载态），不 panic。
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let state = host.state.borrow();
    assert!(state.picker.items.is_empty());
}

#[gpui_kit::test]
fn settings_and_menu_render_for_open_project(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let root = temp_root("settings");
    host.set_current(Some(OpenProject::new(root.clone(), "测试项目")));
    {
        let mut state = host.state.borrow_mut();
        state.settings_open = true;
        state.read_only = true;
    }
    let (host, inputs, cx) = open_harness(cx, host);

    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 设置面板在窗口外也可构造，且只读状态来自宿主注入的 state。
    let has_settings = cx.update(|_, cx| render_settings(&host, &inputs, cx).is_some());
    assert!(has_settings);
    assert!(host.state.borrow().read_only);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn create_dialog_opens_and_rejects_empty_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, inputs, cx) = open_harness(cx, host);

    cx.update(|window, cx| open_create_dialog(&host, &inputs, window, cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "新建项目对话框应已打开"
    );

    // 名称为空 → 校验失败：对话框保持打开，错误写入 state。
    let ok = cx.update(|window, cx| submit_create(&host, &inputs, window, cx));
    assert!(!ok, "空名称不应通过校验");
    assert_eq!(
        host.state.borrow().dialog_error.as_deref(),
        Some("项目名称不能为空")
    );
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx), "校验失败后对话框保持打开");
    });
}

#[gpui_kit::test]
fn delete_dialog_requires_exact_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, inputs, cx) = open_harness(cx, host);
    let item = summary("p-del", "演示项目", "active");

    cx.update(|window, cx| open_delete_dialog(&host, &inputs, &item, window, cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));

    // 未输入名称 → 不删除、保留对话框、错误可见。
    let ok =
        cx.update(|_, cx| confirm_delete(&host, &inputs, &item.id, &item.name, &item.path, cx));
    assert!(!ok);
    assert_eq!(
        host.state.borrow().dialog_error.as_deref(),
        Some("输入的项目名不匹配")
    );
}

#[gpui_kit::test]
fn request_close_intercepts_dirty_editor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    rec.dirty.set(true);
    let host = test_host(&rec);
    host.set_current(Some(OpenProject::new(temp_root("close"), "有草稿的项目")));
    let (host, _inputs, cx) = open_harness(cx, host);

    cx.update(|window, cx| request_close(&host, window, cx));

    assert!(host.current().is_some(), "有未保存草稿时不应直接关闭项目");
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui_kit::test]
fn project_action_continues_after_unsaved_confirm(cx: &mut TestAppContext) {
    // 回归点（#4）：脏草稿下从连接对话框项目栏选「＋ 新增项目 / 打开现有目录…」
    // 以前只弹未保存确认，确认后请求就没了（用户得回选择器再点一次）。
    // 现在确认后**直接推进**到目标对话框（`advance_pending` 的两个新分支）。
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    rec.dirty.set(true);
    let host = test_host(&rec);
    host.set_current(Some(OpenProject::new(temp_root("pa"), "有草稿的项目")));
    let (host, inputs, cx) = open_harness(cx, host);

    // 哨兵值：用来证明“新建项目对话框真的开了”（它会把表单重置）。
    cx.update(|window, cx| {
        inputs
            .create_name
            .update(cx, |s, cx| s.set_value("sentinel", window, cx));
    });

    // 1) 脏草稿：请求「＋ 新增项目」→ 先出未保存确认，不动编辑区、不提前开表单。
    cx.update(|window, cx| request_create_project(&host, &inputs, window, cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "脏草稿应先出未保存确认"
    );
    assert_eq!(rec.cleared.get(), 0, "确认前不应清空编辑区");
    assert_eq!(
        cx.update(|_, cx| inputs.create_name.read(cx).value().to_string()),
        "sentinel",
        "确认前不应开新建对话框"
    );

    // 2) 复现「放弃更改并继续」按钮的调用序列：准备 → 关确认 → 推进。
    cx.update(|window, cx| {
        assert!(prepare_unsaved(&host, false, window, cx), "放弃分支应可推进");
        window.close_dialog(cx);
        advance_pending(
            &host,
            &PendingAction::CreateProject(inputs.clone()),
            window,
            cx,
        );
    });
    assert_eq!(rec.cleared.get(), 1, "确认后应清空编辑区");
    assert!(!rec.dirty.get(), "编辑区应标记为已保存");
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "确认后应直接打开目标对话框"
    );
    assert_eq!(
        cx.update(|_, cx| inputs.create_name.read(cx).value().to_string()),
        "",
        "新建项目对话框已打开（表单被重置）"
    );

    // 3) 「打开现有目录…」：干净编辑器直接开目录对话框（不重置名称框）。
    cx.update(|window, cx| request_open_folder(&host, &inputs, window, cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui_kit::test]
fn request_open_intercepts_dirty_editor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    rec.dirty.set(true);
    let host = test_host(&rec);
    let target = temp_root("open-target");
    let (host, _inputs, cx) = open_harness(cx, host);

    cx.update(|window, cx| request_open(&host, &target, window, cx));

    assert!(host.current().is_none(), "拦截期间不应切换项目");
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui_kit::test]
fn cycle_sort_persists_through_host_callback(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, _inputs, cx) = open_harness(cx, host);

    cx.update(|_, cx| cycle_sort(&host, cx));

    assert_eq!(host.state.borrow().picker.sort, ProjectSort::Name);
    assert_eq!(rec.saved_sort.borrow().as_ref(), Some(&ProjectSort::Name));
    assert!(rec.notifies.get() > 0, "刷新应请求宿主重绘");
}

#[gpui_kit::test]
fn lock_busy_dialog_offers_escape_hatches(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, _inputs, cx) = open_harness(cx, host);

    cx.update(|window, cx| {
        open_lock_busy_dialog(
            &host,
            "占用项目".to_string(),
            temp_root("lock"),
            4242,
            window,
            cx,
        )
    });

    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    // 逃生口对话框不改会话状态，只给出选择。
    assert!(host.current().is_none());
}

#[gpui_kit::test]
fn cards_and_menus_construct_for_all_states(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, inputs, cx) = open_harness(cx, host);

    // 三种卡片分支：活跃 / 路径失效（重新定位）/ 已移除（恢复）+ 固定态。
    let mut invalid = summary("p-invalid", "失效项目", "offline");
    invalid.path_exists = false;
    let mut removed = summary("p-removed", "已移除项目", "archived");
    removed.is_pinned = true;
    let cases = [
        (PickerTab::Recent, summary("p-active", "活跃项目", "active")),
        (PickerTab::All, invalid),
        (PickerTab::Removed, removed),
    ];

    cx.update(|_, cx| {
        let theme = cx.theme().clone();
        for (tab, item) in cases {
            let card = project_card(&host, &inputs, &item, tab, &theme);
            let menu = more_menu(&host, &inputs, &item, tab);
            let _ = (card, menu);
        }
    });
}

#[gpui_kit::test]
fn browse_fills_location_from_system_picker(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let picked = temp_root("browse-pick");
    let expected = picked.to_string_lossy().to_string();

    let (host, inputs, cx) = open_harness(cx, host);
    cx.update(|window, cx| open_create_dialog(&host, &inputs, window, cx));

    // 「浏览」先向平台发起目录选择请求，再模拟用户响应（只允许选目录、单选）。
    cx.update(|window, cx| pick_directory(inputs.create_location.clone(), window, cx));
    assert!(cx.did_prompt_for_paths(), "应已打开系统目录选择器");
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files && !options.multiple);
        Some(vec![picked.clone()])
    });
    cx.run_until_parked();

    let value = cx.update(|_, cx| inputs.create_location.read(cx).value().to_string());
    assert_eq!(value, expected, "选中的目录应回填到位置输入框");
}

#[gpui_kit::test]
fn browse_cancel_keeps_location(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);

    let (host, inputs, cx) = open_harness(cx, host);
    cx.update(|window, cx| open_create_dialog(&host, &inputs, window, cx));
    let before = cx.update(|_, cx| inputs.create_location.read(cx).value().to_string());

    // 取消（返回 `None`）时不改输入框。
    cx.update(|window, cx| pick_directory(inputs.create_location.clone(), window, cx));
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    let after = cx.update(|_, cx| inputs.create_location.read(cx).value().to_string());

    assert_eq!(before, after);
}
