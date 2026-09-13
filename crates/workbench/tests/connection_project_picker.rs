//! 连接对话框「项目」下拉契约测试（项目名 + 路径；末项「＋ 新增项目」）。
//!
//! 覆盖（对应 dev-plan C17）：
//! - 打开时选项 = 当前会话项目 + 最近项目（本测试无全局库 → 仅会话项目）+ 末尾「＋ 新增项目」；
//! - 项目栏默认选中当前会话项目，并把项目根写回路径输入（保存 / 作用域预检读同一份数据）；
//! - 选中「＋ 新增项目」→ 置位 `Shared::project_new_request` 且清空选中（由宿主开新建入口）；
//! - 选中普通项目 → 项目路径写回；空确认（清空按钮）不产生副作用。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的 `test`
//! 属性宏带入作用域，导致 `#[gpui_kit::test]` 展开出的裸 `#[test]` 解析到它自己，
//! 造成无限递归。所有依赖显式列举。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::searchable_list::SearchableListItem as _;
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};

use project::ui::OpenProject;
use rds_workbench::components::connection_dialog::{
    ConnectionDialogState, PROJECT_NEW_LABEL, PROJECT_NONE_LABEL, PROJECT_OPEN_LABEL, ProjectItem,
    ProjectItemKind,
};
use rds_workbench::panels::{EditorPanel, ProjectActionRequest, Shared};

/// 测试宿主：持有对话框状态与 `Entity<EditorPanel>`（`open` 的宿主参数），
/// 并在渲染时挂上对话框层（`open_dialog` 依赖窗口根是 `Root`）。
struct PickerHarness {
    shared: Shared,
    editor: Entity<EditorPanel>,
}

impl PickerHarness {
    /// 带项目会话的宿主（项目作用域 / 项目下拉的来源）。
    fn new(window: &mut Window, cx: &mut Context<Self>, root: PathBuf) -> Self {
        let _ = window;
        let shared = Shared::new();
        *shared.project.borrow_mut() = Some(OpenProject::new(root, "演示项目"));
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        Self { shared, editor }
    }

    fn open(&self, window: &mut Window, cx: &mut gpui_kit::App) {
        // 走生产入口（面板 request_new_connection）：订阅建立等副作用与真实路径一致。
        self.editor
            .update(cx, |editor, cx| editor.request_new_connection(window, cx));
    }

    /// 面板持有的对话框状态（首次 `open` 后才有）。
    fn dialog(&self, cx: &gpui_kit::App) -> Rc<ConnectionDialogState> {
        self.editor
            .read(cx)
            .dialog_state()
            .expect("对话框状态已创建")
    }
}

impl Render for PickerHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

/// 打开测试窗口（窗口根 `Root`）并取回宿主实体。
fn open_harness(
    cx: &mut TestAppContext,
    root: PathBuf,
) -> (Entity<PickerHarness>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<PickerHarness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| PickerHarness::new(window, cx, root));
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

/// 会话项目根（不触盘：仅作为路径字符串参与下拉映射）。
fn demo_root() -> PathBuf {
    std::env::temp_dir()
        .join("rds_project_picker")
        .join("演示项目")
}

#[gpui_kit::test]
fn dropdown_offers_session_project_and_new_entry(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = demo_root();
    let (harness, cx) = open_harness(cx, root.clone());

    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(window, cx));
    });
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "连接对话框应已打开"
    );
    // 渲染一帧（含对话框层）不 panic。
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let root_str = root.to_string_lossy().to_string();
    let options = cx.update(|_, cx| harness.read(cx).dialog(cx).project_options.borrow().clone());
    assert_eq!(
        options.first(),
        Some(&("演示项目".to_string(), root_str.clone())),
        "当前会话项目应置顶"
    );

    // 选项存在性用「选中回读」证明（`position` 命中才能选中）：
    // 一）当前会话项目在列表中；二）末项「＋ 新增项目」在列表中；三）不存在的项目选不中。
    let mut pick = |label: &str| {
        let v = SharedString::from(label);
        let sel = cx.update(|_, cx| harness.read(cx).dialog(cx).project_sel.clone());
        cx.update(|window, cx| {
            sel.update(cx, |s, cx| s.set_selected_value(&v, window, cx));
        });
        cx.update(|_, cx| {
            harness
                .read(cx)
                .dialog(cx)
                .project_sel
                .read(cx)
                .selected_value()
                .map(|v| v.to_string())
        })
    };
    assert_eq!(
        pick("演示项目").as_deref(),
        Some("演示项目"),
        "当前会话项目应在选项列表中"
    );
    assert_eq!(
        pick(PROJECT_NEW_LABEL).as_deref(),
        Some(PROJECT_NEW_LABEL),
        "「＋ 新增项目」应在选项列表中"
    );
    assert_eq!(
        pick(PROJECT_OPEN_LABEL).as_deref(),
        Some(PROJECT_OPEN_LABEL),
        "「打开现有目录…」应在选项列表中"
    );
    assert_eq!(
        pick(PROJECT_NONE_LABEL).as_deref(),
        Some(PROJECT_NONE_LABEL),
        "「不需要项目（仅全局）」应在选项列表中"
    );
    assert_eq!(pick("不存在的项目"), None, "不存在的项目不应可选中");

    let path_value =
        cx.update(|_, cx| harness.read(cx).dialog(cx).project_path.read(cx).value().to_string());
    assert_eq!(path_value, root_str, "项目根应写回路径输入");
}

#[gpui_kit::test]
fn confirm_new_entry_requests_creation_and_clears_selection(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = demo_root();
    let (harness, cx) = open_harness(cx, root.clone());
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(window, cx));
    });

    // 空确认（下拉「清除」按钮路径）不产生副作用。
    let cleared = cx.update(|window, cx| {
        let (dialog, shared) = {
            let h = harness.read(cx);
            (h.dialog(cx), h.shared.clone())
        };
        dialog.handle_project_confirm(None, &shared, window, cx)
    });
    assert!(!cleared, "空确认不应请求新增项目");
    assert!(
        !cx.update(|_, cx| harness.read(cx).shared.project_new_request.get()),
        "空确认不应置位新增项目请求"
    );

    // 选中「＋ 新增项目」：置位请求 + 清空选中（宿主随后打开新建入口）。
    let new_label = SharedString::from(PROJECT_NEW_LABEL);
    let requested = cx.update(|window, cx| {
        let (dialog, shared) = {
            let h = harness.read(cx);
            (h.dialog(cx), h.shared.clone())
        };
        dialog.handle_project_confirm(Some(&new_label), &shared, window, cx)
    });
    assert!(requested, "应识别为「＋ 新增项目」");
    assert!(
        cx.update(|_, cx| harness.read(cx).shared.project_new_request.get()),
        "应置位宿主新增项目请求"
    );
    let selected = cx.update(|_, cx| {
        harness
            .read(cx)
            .dialog(cx)
            .project_sel
            .read(cx)
            .selected_value()
            .cloned()
    });
    assert!(selected.is_none(), "新增项目后本下拉应恢复未选中");
}

#[gpui_kit::test]
fn confirm_project_writes_back_path(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = demo_root();
    let root_str = root.to_string_lossy().to_string();
    let (harness, cx) = open_harness(cx, root.clone());
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(window, cx));
    });

    // 先把路径输入改成陈旧值（模拟手动改过 / 回读别的项目），确认当前会话项目 → 应写回它的根。
    cx.update(|window, cx| {
        let input = harness.read(cx).dialog(cx).project_path.clone();
        input.update(cx, |s, cx| s.set_value("D:/stale-ish".to_string(), window, cx));
    });
    let label = SharedString::from("演示项目");
    let requested = cx.update(|window, cx| {
        let (dialog, shared) = {
            let h = harness.read(cx);
            (h.dialog(cx), h.shared.clone())
        };
        dialog.handle_project_confirm(Some(&label), &shared, window, cx)
    });
    assert!(!requested, "普通项目确认不应请求新增项目");
    let path_value =
        cx.update(|_, cx| harness.read(cx).dialog(cx).project_path.read(cx).value().to_string());
    assert_eq!(path_value, root_str, "选中项目应把项目根写回路径输入");
    assert!(
        !cx.update(|_, cx| harness.read(cx).shared.project_new_request.get()),
        "普通项目确认不应置位新增项目请求"
    );
}

#[gpui_kit::test]
fn confirm_open_folder_requests_folder_dialog(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = demo_root();
    let (harness, cx) = open_harness(cx, root);
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(window, cx));
    });

    let label = SharedString::from(PROJECT_OPEN_LABEL);
    let requested = cx.update(|window, cx| {
        let (dialog, shared) = {
            let h = harness.read(cx);
            (h.dialog(cx), h.shared.clone())
        };
        dialog.handle_project_confirm(Some(&label), &shared, window, cx)
    });
    assert!(requested, "应识别为动作项（打开现有目录…）");
    assert!(
        cx.update(|_, cx| harness.read(cx).shared.project_open_request.get()),
        "应置位宿主「打开现有目录」请求"
    );
    assert!(
        !cx.update(|_, cx| harness.read(cx).shared.project_new_request.get()),
        "不应误置位新增项目请求"
    );
    let selected = cx.update(|_, cx| {
        harness
            .read(cx)
            .dialog(cx)
            .project_sel
            .read(cx)
            .selected_value()
            .cloned()
    });
    assert!(selected.is_none(), "动作项确认后本下拉应恢复未选中");
}

#[gpui_kit::test]
fn confirm_no_project_switches_scope_to_global(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let root = demo_root();
    let (harness, cx) = open_harness(cx, root.clone());
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(window, cx));
    });

    // 先切到「仅项目」（项目作用域下才需要项目）→ 再选「不需要项目」。
    let scope_label = SharedString::from("仅项目");
    let scope_entity = cx.update(|_, cx| harness.read(cx).dialog(cx).scope.clone());
    cx.update(|window, cx| {
        scope_entity.update(cx, |s, cx| s.set_selected_value(&scope_label, window, cx));
    });

    let label = SharedString::from(PROJECT_NONE_LABEL);
    let requested = cx.update(|window, cx| {
        let (dialog, shared) = {
            let h = harness.read(cx);
            (h.dialog(cx), h.shared.clone())
        };
        dialog.handle_project_confirm(Some(&label), &shared, window, cx)
    });
    assert!(requested, "应识别为动作项（不需要项目）");
    // 作用域回到「仅全局」，项目路径清空、下拉选中清空。
    let scope_now = cx.update(|_, cx| {
        harness
            .read(cx)
            .dialog(cx)
            .scope
            .read(cx)
            .selected_value()
            .map(|v| v.to_string())
    });
    assert_eq!(scope_now.as_deref(), Some("仅全局"));
    let path_now =
        cx.update(|_, cx| harness.read(cx).dialog(cx).project_path.read(cx).value().to_string());
    assert!(path_now.is_empty(), "项目路径应清空：{path_now}");
    assert!(
        !cx.update(|_, cx| harness.read(cx).shared.project_new_request.get())
            && !cx.update(|_, cx| harness.read(cx).shared.project_open_request.get()),
        "不应误置位新增 / 打开目录请求"
    );
}

/// 下拉项数据契约（纯数据，无需窗口）：显示名 / 路径 / 动作项标记 / 搜索匹配。
#[test]
fn project_action_request_is_consumed_exactly_once() {
    // 回归点（#9）：宿主消费分支以前直接 `replace(false)` 两个标记 + if/else，
    // 无法在无窗口环境下验证“标记 → 动作”的映射与“不会重复开窗”。
    let shared = Shared::new();

    // 无请求（绝大多数帧）：不产生动作。
    assert_eq!(shared.take_project_action_request(), None);

    // 「＋ 新增项目」→ 取回一次即消。
    shared.project_new_request.set(true);
    assert_eq!(
        shared.take_project_action_request(),
        Some(ProjectActionRequest::CreateProject)
    );
    assert!(
        !shared.project_new_request.get(),
        "取出后应清标记（否则后续帧会重复开窗）"
    );
    assert_eq!(shared.take_project_action_request(), None, "不得重复消费");

    // 「打开现有目录…」→ 同理。
    shared.project_open_request.set(true);
    assert_eq!(
        shared.take_project_action_request(),
        Some(ProjectActionRequest::OpenFolder)
    );
    assert_eq!(shared.take_project_action_request(), None);

    // 两个标记同帧置位（竞态）：「新建」优先，且两个标记都要清掉。
    shared.project_new_request.set(true);
    shared.project_open_request.set(true);
    assert_eq!(
        shared.take_project_action_request(),
        Some(ProjectActionRequest::CreateProject)
    );
    assert!(
        !shared.project_new_request.get() && !shared.project_open_request.get(),
        "同帧两个请求都要清掉（否则被丢弃的那一个会在下一帧补开一个窗）"
    );
    assert_eq!(shared.take_project_action_request(), None);
}

#[test]
fn project_item_contract() {
    let item = ProjectItem::project("演示项目", "/tmp/rds_project_picker/演示项目");
    assert_eq!(item.title().as_ref(), "演示项目");
    assert_eq!(item.path().as_ref(), "/tmp/rds_project_picker/演示项目");
    assert!(!item.is_new(), "普通项目项不应标记为新增入口");
    assert!(item.matches("演示"), "应按项目名匹配");
    assert!(item.matches("rds_project_picker"), "应按路径匹配");
    assert!(!item.matches("postgres"), "无关查询不应命中");

    let new_item = ProjectItem::new_project();
    assert!(new_item.is_new(), "「＋ 新增项目」应是新增入口项");
    assert!(new_item.is_action(), "新增项目是动作项（不带路径）");
    assert_eq!(new_item.path().as_ref(), "", "动作项不带路径");
    assert_eq!(new_item.value().as_ref(), PROJECT_NEW_LABEL);

    let open_item = ProjectItem::open_folder();
    assert!(open_item.is_action(), "「打开现有目录…」是动作项");
    assert!(!open_item.is_new(), "它不是「新增项目」项");
    assert_eq!(open_item.path().as_ref(), "", "动作项不带路径");
    assert_eq!(open_item.value().as_ref(), PROJECT_OPEN_LABEL);
    assert_eq!(open_item.kind(), ProjectItemKind::OpenFolder);

    let none_item = ProjectItem::no_project();
    assert!(none_item.is_action(), "「不需要项目」是动作项");
    assert_eq!(none_item.path().as_ref(), "", "动作项不带路径");
    assert_eq!(none_item.value().as_ref(), PROJECT_NONE_LABEL);
}
