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
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::component::{ActiveTheme as _, Root, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    ParentElement, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point, px,
};

use super::{
    ConnectionOption, OpenProject, PendingAction, PickerTab, ProjectEditorBridge, ProjectInputs,
    ProjectMenuEntry, ProjectSort, ProjectUiHost, ProjectUiNotifier, ProjectUiState,
    SettingsSnapshot, StatusFilter, advance_pending, build_project_menu, card_description,
    card_menu_entries, confirm_delete, create_version_action, cycle_sort, default_connection_label,
    load_settings_snapshot, meta_tree_rows, more_menu, open_create_dialog, open_delete_dialog,
    open_folder_dialog, open_lock_busy_dialog, parse_search, pick_directory, prepare_unsaved,
    project_card, project_menu_entries, render_picker, render_settings, request_close,
    request_create_project, request_open, request_open_folder, save_project_info,
    set_default_connection, snapshot_description, submit_create, submit_open_folder,
    toggle_archive, visible_items,
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
        // 标题栏项目菜单现在由 `Button::dropdown_menu` 承载（`build_project_menu` 只产出
        // `PopupMenu`），这里照样挂一个：菜单构造路径在窗口里跑一遍。
        let menu_host = self.host.clone();
        let menu_inputs = self.inputs.clone();
        body = body.child(
            Button::new("harness-project-menu")
                .ghost()
                .small()
                .label("项目 ▾")
                .dropdown_menu(move |menu, _, _| {
                    build_project_menu(menu, &menu_host, &menu_inputs)
                }),
        );
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
        assert!(
            prepare_unsaved(&host, false, window, cx),
            "放弃分支应可推进"
        );
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

#[test]
fn visible_items_filters_by_status_and_needle() {
    let mut offline = summary("b", "离线项目", "offline");
    offline.path_exists = false;
    let items = vec![
        summary("a", "风控看板", "active"),
        offline,
        summary("c", "归档项目", "archived"),
    ];

    // 状态筛选：All 命中全部，具体状态只命中对应项，无匹配返回空。
    assert_eq!(visible_items(&items, "", StatusFilter::All).len(), 3);
    assert_eq!(visible_items(&items, "", StatusFilter::Offline).len(), 1);
    assert_eq!(visible_items(&items, "", StatusFilter::Syncing).len(), 0);

    // 搜索：名称子串 + 路径子串（大小写不敏感）。
    assert_eq!(visible_items(&items, "风控", StatusFilter::All).len(), 1);
    assert_eq!(
        visible_items(&items, "RDS-DEMO", StatusFilter::All).len(),
        3
    );
    assert_eq!(
        visible_items(&items, "不存在的名字", StatusFilter::All).len(),
        0
    );

    // 组合：搜索与状态取交集。
    assert_eq!(
        visible_items(&items, "项目", StatusFilter::Offline).len(),
        1
    );
    assert_eq!(visible_items(&items, "项目", StatusFilter::Active).len(), 0);
}

#[gpui_kit::test]
fn empty_dir_prompts_create_in_place(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, inputs, cx) = open_harness(cx, host);

    // 准备一个真正的空目录（原型 C2：空目录应询问「是否在此创建项目」）。
    let empty = temp_root("empty-dir");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).expect("重建空目录");
    let value = empty.to_string_lossy().to_string();

    cx.update(|window, cx| open_folder_dialog(&host, &inputs, window, cx));
    cx.update(|window, cx| {
        inputs
            .create_location
            .update(cx, |s, cx| s.set_value(value.clone(), window, cx));
    });

    let ok = cx.update(|window, cx| submit_open_folder(&host, &inputs, window, cx));
    assert!(!ok, "空目录不应当作项目直接打开");
    assert!(host.current().is_none(), "确认前不应改动会话");
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "应弹出「在空目录创建项目？」确认对话框"
    );
}

#[gpui_kit::test]
fn save_project_info_rejects_invalid_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    host.set_current(Some(OpenProject::new(
        temp_root("rename-guard"),
        "校验项目",
    )));
    let (host, inputs, cx) = open_harness(cx, host);

    // 名称非法（含分隔符）→ 写入提示、不触碰名册。
    cx.update(|window, cx| {
        inputs
            .rename
            .update(cx, |s, cx| s.set_value("非法/名称", window, cx));
    });
    cx.update(|_, cx| save_project_info(&host, &inputs, cx));
    let notice = host.state.borrow().notice.clone();
    assert!(
        notice.as_deref().is_some_and(|n| n.contains("不能包含")),
        "非法名称应给出校验提示，实际：{notice:?}"
    );

    // 空名同样被拦。
    cx.update(|window, cx| {
        inputs
            .rename
            .update(cx, |s, cx| s.set_value("", window, cx));
    });
    cx.update(|_, cx| save_project_info(&host, &inputs, cx));
    assert_eq!(
        host.state.borrow().notice.as_deref(),
        Some("项目名称不能为空")
    );
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

// ==================== C1：只读禁用态 ====================

/// 写命令即便被触发也要被拦截：禁用态只是「点不动」，拦截是「点了也没用」，两道都要有。
#[gpui_kit::test]
fn read_only_blocks_project_info_save(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    host.set_current(Some(OpenProject::new(temp_root("read-only"), "只读项目")));
    {
        let mut state = host.state.borrow_mut();
        state.read_only = true;
        state.settings_open = true;
    }
    let (host, inputs, cx) = open_harness(cx, host);

    cx.update(|window, cx| {
        inputs
            .rename
            .update(cx, |s, cx| s.set_value("改名尝试", window, cx));
    });
    let notifies_before = rec.notifies.get();
    cx.update(|_, cx| save_project_info(&host, &inputs, cx));

    let notice = host.state.borrow().notice.clone();
    assert!(
        notice.as_deref().is_some_and(|n| n.contains("只读模式")),
        "只读应被拦截并给出提示，实际：{notice:?}"
    );
    assert_eq!(
        host.current().map(|p| p.name),
        Some("只读项目".to_string()),
        "拦截时不应改会话名"
    );
    assert!(rec.notifies.get() > notifies_before, "拦截也要给出可见反馈");

    // 只读是「不许写」，不是「不许看」：设置面板照样开得起来（写控件已置灰）。
    assert!(cx.update(|_, cx| render_settings(&host, &inputs, cx).is_some()));
}

// ==================== B2/B3：描述展示与 `.RSmeta` 结构树 ====================

/// B3：描述展示的两个口（卡片行 / 概览文案）都只认「非空描述」。
#[test]
fn description_shows_only_when_present() {
    let mut item = summary("p-desc", "带描述的项目", "active");
    assert_eq!(card_description(&item), None, "没写描述 → 不占卡片行");
    item.description = Some("   ".to_string());
    assert_eq!(card_description(&item), None, "纯空白视作没写");
    item.description = Some("  风控看板  ".to_string());
    assert_eq!(
        card_description(&item),
        Some("风控看板".to_string()),
        "两端空白去掉"
    );

    let mut snapshot = SettingsSnapshot::default();
    assert_eq!(snapshot_description(&snapshot), "—", "未取快照不当成有描述");
    snapshot.description = Some("  ".to_string());
    assert_eq!(snapshot_description(&snapshot), "—");
    snapshot.description = Some("风控看板".to_string());
    assert_eq!(snapshot_description(&snapshot), "风控看板");
}

/// B2：结构树 = 目录在前 + 同级按名 + 深度优先；目录不给大小。
#[test]
fn meta_tree_rows_lists_dirs_first_then_files() {
    let root = temp_root("meta-tree");
    let meta = root.join(crate::service::RS_META_DIR_NAME);
    let _ = std::fs::remove_dir_all(&meta);
    std::fs::create_dir_all(meta.join("config")).expect("建 config");
    std::fs::create_dir_all(meta.join("queries")).expect("建 queries");
    std::fs::write(meta.join("project.db"), b"0123456789").expect("写 project.db");
    std::fs::write(meta.join("config").join("settings.json"), b"{}").expect("写 settings.json");

    let rows = meta_tree_rows(&meta);
    let shape: Vec<(usize, &str, bool)> = rows
        .iter()
        .map(|r| (r.depth, r.name.as_str(), r.is_dir))
        .collect();
    assert_eq!(
        shape,
        vec![
            (0, "config", true),
            (1, "settings.json", false),
            (0, "queries", true),
            (0, "project.db", false),
        ],
        "目录在前（同级按名）、子项紧跟父目录"
    );
    let project_db = rows
        .iter()
        .find(|r| r.name == "project.db")
        .expect("project.db");
    assert_eq!(project_db.size, Some(10), "文件给字节数");
    assert_eq!(
        project_db.path,
        meta.join("project.db"),
        "行带完整路径（复制路径用）"
    );
    assert!(
        rows.iter().filter(|r| r.is_dir).all(|r| r.size.is_none()),
        "目录不给大小（不递归求和，免得掩盖哪个文件大）"
    );

    // 目录不存在 / 为空：返回空表，不 panic（排障视图要能照常画）
    assert!(meta_tree_rows(&root.join("不存在的目录")).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// B2：设置面板的存储段读**快照**（事件路径填充），窗口里画得出来。
#[gpui_kit::test]
fn settings_meta_tree_comes_from_snapshot(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let root = temp_root("settings-meta");
    let meta = root.join(crate::service::RS_META_DIR_NAME);
    let _ = std::fs::remove_dir_all(&meta);
    std::fs::create_dir_all(meta.join("config")).expect("建 .RSmeta");
    std::fs::write(meta.join("project.db"), b"meta").expect("写 project.db");
    host.set_current(Some(OpenProject::new(root.clone(), "结构树项目")));
    let (host, inputs, cx) = open_harness(cx, host);

    // 与菜单「项目设置…」同一条路：事件路径取快照
    cx.update(|_, cx| load_settings_snapshot(&host, cx));
    {
        let state = host.state.borrow();
        assert!(state.settings.loaded, "快照应标为已取");
        let names: Vec<&str> = state
            .settings
            .meta_rows
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert!(
            names.contains(&"config") && names.contains(&"project.db"),
            "结构树应来自磁盘，实际：{names:?}"
        );
    }

    host.state.borrow_mut().settings_open = true;
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|_, cx| render_settings(&host, &inputs, cx).is_some()));
    let _ = std::fs::remove_dir_all(&root);
}

// ==================== A1：默认连接（U3） ====================

/// U3：按钮文案三种状态都如实显示（记录值指向已删除的连接时不能静默变「未设置」）。
#[test]
fn default_connection_label_reports_missing_option() {
    let mut snapshot = SettingsSnapshot::default();
    assert_eq!(default_connection_label(&snapshot), "默认连接：未设置");

    snapshot.connection_options = vec![
        ConnectionOption {
            id: "G_a".to_string(),
            name: "分析库".to_string(),
        },
        ConnectionOption {
            id: "G_b".to_string(),
            name: "业务库".to_string(),
        },
    ];
    snapshot.default_connection = Some("G_b".to_string());
    assert_eq!(default_connection_label(&snapshot), "默认连接：业务库");

    snapshot.default_connection = Some("G_gone".to_string());
    assert_eq!(
        default_connection_label(&snapshot),
        "默认连接：G_gone（已不可用）"
    );
}

/// U3：写默认连接走服务落盘，只读模式被拦（置灰是第一道，拦截是第二道）。
#[gpui_kit::test]
fn default_connection_write_respects_read_only(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    // 真项目目录：写默认连接要落到 `.RSmeta/config/settings.json`
    let root = temp_root("default-conn");
    let _ = std::fs::remove_dir_all(&root);
    crate::ProjectStore::create("默认连接项目", &root).expect("建项目");
    host.set_current(Some(OpenProject::new(root.clone(), "默认连接项目")));
    let (host, _inputs, cx) = open_harness(cx, host);

    // 快照：新项目 = 不设默认
    cx.update(|_, cx| load_settings_snapshot(&host, cx));
    {
        let state = host.state.borrow();
        assert!(state.settings.loaded);
        assert_eq!(state.settings.default_connection, None);
    }
    // 候选由宿主注入（测试里直接给）
    host.state.borrow_mut().settings.connection_options = vec![ConnectionOption {
        id: "G_a".to_string(),
        name: "分析库".to_string(),
    }];

    cx.update(|_, cx| set_default_connection(&host, Some("G_a".to_string()), cx));
    assert_eq!(
        crate::service::load_default_connection(&root).expect("读磁盘"),
        Some("G_a".to_string()),
        "默认连接应已落盘"
    );
    {
        let state = host.state.borrow();
        assert_eq!(state.settings.default_connection.as_deref(), Some("G_a"));
        assert_eq!(
            default_connection_label(&state.settings),
            "默认连接：分析库"
        );
        assert!(
            state
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("默认连接")),
            "实际：{:?}",
            state.notice
        );
    }

    // 面板（含选择器）在窗口里画得出来
    host.state.borrow_mut().settings_open = true;
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 只读：拦下、给提示、不动磁盘
    host.state.borrow_mut().read_only = true;
    cx.update(|_, cx| set_default_connection(&host, Some("G_b".to_string()), cx));
    assert!(
        host.state
            .borrow()
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("只读模式")),
        "只读应被拦截"
    );
    assert_eq!(
        crate::service::load_default_connection(&root).expect("读磁盘"),
        Some("G_a".to_string()),
        "只读不该改磁盘"
    );

    let _ = std::fs::remove_dir_all(&root);
}

// ==================== C5：搜索 facet 语法 ====================

/// C5：facet 解析——未知键 / 空值 / 半截值一律回落到自由文本（不失字、不清列表）。
#[test]
fn search_facets_parse_known_keys_only() {
    let query = parse_search("风控 状态:offline 固定:是 锁:否");
    assert_eq!(query.free, "风控");
    assert_eq!(query.status.as_deref(), Some("offline"));
    assert_eq!(query.pinned, Some(true));
    assert_eq!(query.locked, Some(false));
    assert!(query.has_facet());

    // 「全部」= 解除状态约束；其余键不认就当日志文本
    assert_eq!(parse_search("状态:全部").status, None);
    assert_eq!(
        parse_search("状态:离").free,
        "状态:离",
        "半截值不该清空列表"
    );
    assert_eq!(parse_search("状态:").free, "状态:", "只有键没有值也回落");
    assert_eq!(
        parse_search("描述:报表").free,
        "描述:报表",
        "未知键不装 facet"
    );

    // 盘符 / URL 里的冒号不会被误当 facet
    assert_eq!(parse_search("C:\\data").free, "C:\\data");
    assert!(!parse_search("C:\\data").has_facet());
    assert!(!parse_search("").has_facet());

    // 布尔值多写法
    assert_eq!(parse_search("固定:true").pinned, Some(true));
    assert_eq!(parse_search("固定:0").pinned, Some(false));
    assert_eq!(parse_search("固定:maybe").free, "固定:maybe");
}

/// C5：facet 与自由文本 / 状态筛选按钮**叠加（AND）**。
#[test]
fn search_facets_narrow_the_list_with_and_semantics() {
    let mut offline = summary("p-off", "风控离线库", "offline");
    offline.description = Some("月末对账用的".to_string());
    let mut pinned = summary("p-pin", "置顶的报表库", "active");
    pinned.is_pinned = true;
    let mut locked = summary("p-lock", "被占用的库", "active");
    locked.lock = Some(crate::LockInfo {
        pid: 4242,
        acquired_at: chrono::Utc::now(),
    });
    let items = vec![
        offline,
        pinned,
        locked,
        summary("p-plain", "普通库", "active"),
    ];

    // facet 单独生效
    assert_eq!(
        visible_items(&items, "状态:offline", StatusFilter::All).len(),
        1
    );
    assert_eq!(visible_items(&items, "固定:是", StatusFilter::All).len(), 1);
    assert_eq!(visible_items(&items, "锁:是", StatusFilter::All).len(), 1);
    assert_eq!(visible_items(&items, "锁:否", StatusFilter::All).len(), 3);

    // facet ∩ 自由文本（描述也参与匹配）
    assert_eq!(
        visible_items(&items, "状态:offline 对账", StatusFilter::All).len(),
        1
    );
    assert_eq!(
        visible_items(&items, "状态:offline 报表", StatusFilter::All).len(),
        0,
        "自由文本仍是约束"
    );

    // facet ∩ 状态筛选按钮（两边都要过）
    assert_eq!(
        visible_items(&items, "固定:是", StatusFilter::Active).len(),
        1
    );
    assert_eq!(
        visible_items(&items, "固定:是", StatusFilter::Offline).len(),
        0,
        "筛选按钮与 facet 取交集"
    );
}

// ==================== C3：卡片菜单（`⋯` 下拉与右键菜单同源） ====================

/// C3：卡片命令集按**视图 / 路径状态**分支，只读时写命令全置灰、读命令保留。
#[test]
fn card_menu_spec_branches_and_gates_read_only() {
    let shape = |entries: Vec<ProjectMenuEntry>| -> Vec<(&'static str, bool)> {
        entries.into_iter().map(|e| (e.label, e.enabled)).collect()
    };
    let active = summary("p-card", "活跃项目", "active");

    // 可写 · 路径正常：打开 / 固定 / 显示位置 / 移出（软删）/ 删数据
    assert_eq!(
        shape(card_menu_entries(&active, PickerTab::Recent, false)),
        vec![
            ("打开", true),
            ("固定", true),
            ("在资源管理器中显示", true),
            ("移出列表", true),
            ("删除数据…", true),
        ]
    );

    // 已固定：文案翻转（同一个 id，动作相反）
    let mut pinned = active.clone();
    pinned.is_pinned = true;
    assert_eq!(
        card_menu_entries(&pinned, PickerTab::Recent, false)[1].label,
        "取消固定"
    );

    // 只读：写命令全灰，读命令（打开 / 显示位置）保留
    assert_eq!(
        shape(card_menu_entries(&active, PickerTab::Recent, true)),
        vec![
            ("打开", true),
            ("固定", false),
            ("在资源管理器中显示", true),
            ("移出列表", false),
            ("删除数据…", false),
        ],
        "只读只挡写命令"
    );

    // 路径失效：重新定位 + 移出（无显示位置 / 删数据）；不能去资源管理器里显示一个不存在的目录
    let mut invalid = active.clone();
    invalid.path_exists = false;
    assert_eq!(
        shape(card_menu_entries(&invalid, PickerTab::Recent, false)),
        vec![
            ("打开", true),
            ("固定", true),
            ("重新定位…", true),
            ("移出列表", true),
        ]
    );

    // 已移除视图：只剩恢复
    assert_eq!(
        shape(card_menu_entries(&active, PickerTab::Removed, false)),
        vec![("打开", true), ("固定", true), ("恢复", true)]
    );
}

// ==================== E1：只读拦截的其他路径 ====================

/// E1：只读不止挡「改项目信息」——归档与创建版本快照同样被拦（各自给提示）。
#[gpui_kit::test]
fn read_only_blocks_archive_and_version_snapshot(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let root = temp_root("read-only-more");
    let _ = std::fs::remove_dir_all(&root);
    crate::ProjectStore::create("只读项目", &root).expect("建项目");
    host.set_current(Some(OpenProject::new(root.clone(), "只读项目")));
    host.state.borrow_mut().read_only = true;
    let (host, inputs, cx) = open_harness(cx, host);

    // 归档
    cx.update(|_, cx| toggle_archive(&host, cx));
    assert!(
        host.state
            .borrow()
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("只读模式")),
        "归档应被只读拦下，实际：{:?}",
        host.state.borrow().notice
    );

    // 创建版本快照
    cx.update(|window, cx| {
        inputs
            .version_msg
            .update(cx, |s, cx| s.set_value("只读下的快照尝试", window, cx));
    });
    cx.update(|_, cx| create_version_action(&host, &inputs, cx));
    assert!(
        host.state
            .borrow()
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("只读模式")),
        "创建版本应被只读拦下"
    );

    // 磁盘上不该多出版本记录（名册/本体都没被碰）
    let versions = crate::service::list_versions(&root).expect("列版本");
    assert!(versions.is_empty(), "只读不该写版本台账");

    let _ = std::fs::remove_dir_all(&root);
}

// ==================== B1/C4：菜单规格与键盘可达 ====================

/// 菜单规格是「菜单长什么样」的唯一权威来源（渲染只按 id 挂事件）。
#[test]
fn menu_spec_greys_out_write_commands_in_read_only() {
    let writable: Vec<(&str, bool)> = project_menu_entries(false)
        .iter()
        .map(|e| (e.label, e.enabled))
        .collect();
    assert_eq!(
        writable,
        vec![
            ("切换项目…", true),
            ("项目设置…", true),
            ("重命名…", true),
            ("在资源管理器中显示", true),
            ("归档 / 取消归档", true),
            ("关闭项目", true),
        ],
        "可写模式：全部可用，且没有只读提示项"
    );

    let read_only: Vec<(&str, bool)> = project_menu_entries(true)
        .iter()
        .map(|e| (e.label, e.enabled))
        .collect();
    assert_eq!(
        read_only,
        vec![
            ("只读模式：写操作不可用", false),
            ("切换项目…", true),
            ("项目设置…", true),
            ("重命名…", false),
            ("在资源管理器中显示", true),
            ("归档 / 取消归档", false),
            ("关闭项目", true),
        ],
        "只读模式：写命令置灰，读命令与出口保持可用"
    );
}

/// 分隔线位置：只读提示与「切换项目」之间、列表尾部两项之前。
#[test]
fn menu_spec_keeps_escape_hatches_without_leading_separator() {
    for read_only in [false, true] {
        let entries = project_menu_entries(read_only);
        assert!(
            !entries[0].separator_before,
            "首项上方不该有分隔线（read_only={read_only}）"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.id == "close" && e.separator_before),
            "「关闭项目」应与上方命令分开"
        );
        // 只读提示与后续命令之间要有分隔线（提示项当时是首项，分隔线落在下一项上）。
        let switch = entries
            .iter()
            .position(|e| e.id == "switch")
            .expect("切换项目项");
        assert_eq!(entries[switch].separator_before, read_only);
    }
}

/// 两种模式的菜单都构造得出来（禁用项 / 分隔线 / 事件闭包类型全部跑一遍）。
#[gpui_kit::test]
fn title_bar_menu_builds_in_both_modes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    host.set_current(Some(OpenProject::new(temp_root("menu"), "菜单项目")));
    let (host, inputs, cx) = open_harness(cx, host);

    for read_only in [false, true] {
        host.state.borrow_mut().read_only = read_only;
        let menu = cx.update(|window, cx| {
            PopupMenu::build(window, cx, {
                let host = host.clone();
                let inputs = inputs.clone();
                move |menu, _, _| build_project_menu(menu, &host, &inputs)
            })
        });
        assert!(
            !cx.update(|_, cx| menu.read(cx).is_empty()),
            "菜单不应为空（read_only={read_only}）"
        );
    }
}

/// C4：Tab 停到控件上按 Enter 能真的改状态——即「键盘可达」不是靠自绘 div 装出来的。
#[gpui_kit::test]
fn picker_controls_activate_from_the_keyboard(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = Rc::new(Recorder::default());
    let host = test_host(&rec);
    let (host, _inputs, cx) = open_harness(cx, host);

    // 先画一帧（Tab 顺序来自上一帧布局），再点一下窗口把键盘事件交给它。
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_click(point(px(2.), px(2.)), Modifiers::default());

    let signature = |host: &ProjectUiHost| {
        let ui = host.state.borrow();
        (ui.picker.tab, ui.picker.sort, ui.picker.status_filter)
    };
    let start = signature(&host);
    let mut focused = 0usize;
    let mut activated = false;
    for _ in 0..12 {
        cx.update(|window, cx| {
            window.focus_next(cx);
            window.draw(cx).clear(cx);
        });
        if cx.update(|window, cx| window.focused(cx)).is_some() {
            focused += 1;
        }
        // 激活发生在 KeyUp：gpui 的元素在 key up 上派发 `ClickEvent::Keyboard`。
        cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("enter").expect("enter"),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("enter"),
        });
        if signature(&host) != start {
            activated = true;
            break;
        }
    }

    assert!(focused > 0, "Tab 应当能停到控件上（焦点句柄非空）");
    assert!(
        activated,
        "Tab 停到控件上按 Enter 应当能激活它（Tab / 状态 / 排序），实际状态一直是 {start:?}"
    );
}
