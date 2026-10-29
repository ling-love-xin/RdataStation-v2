//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::dock::{BasePanel as _, DockArea, DockPlacement, DockSkin};
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use crate::channel::ExecChannel;
use crate::connection::{ConnectionOption, ConnectionsPort};
use crate::execution::{self, QueryData, QueryRunner};
use crate::mode::CellGranularity;
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::{
    EditorHostPanel, request_close_document, request_save_as,
    resolve_close_choice,
};

use super::{
    bind_editor_keys, open_panel,
    shared_with_document, shared_with_runner, temp_dir,
    wait_for_result,
};

// ═══════════════════════════════════════════════════════════════════════
//
// 对话框要真弹得出来，窗口的根视图必须是组件库的 `Root`（`Root::update` 找不到 Root 会
// 直接 panic），且宿主 render 里要挂 `Root::render_dialog_layer`——与生产同构。
// 下面这组测试因此不复用上面那个 Harness，而是把 `Root` 包在外面（“走生产入口”）。

/// 带对话框层的宿主（与 `WorkbenchView` 同构：DockArea + 对话框层）
pub(crate) struct DialogHarness {
    area: Entity<DockArea>,
    pub(crate) panels: Vec<Entity<EditorHostPanel>>,
}

impl Render for DialogHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.area.clone())
    }
}

/// 起一个带对话框层的窗口，里面装一个文档的面板
pub(crate) fn dialog_harness<'a>(
    cx: &'a mut TestAppContext,
    shared: &EditorShared,
    id: &DocumentId,
    tag: &'static str,
) -> (Entity<DialogHarness>, &'a mut VisualTestContext) {
    let shared = shared.clone();
    let id = id.clone();
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None::<Entity<DialogHarness>>));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let (area, _skin) = DockSkin::dock_area(tag, Some(1), window, cx);
        let panel = cx.new(|cx| EditorHostPanel::new(shared.clone(), id, window, cx));
        area.update(cx, |area, cx| {
            area.add_panel(panel.clone(), DockPlacement::Center, None, window, cx);
        });
        let harness = cx.new(|_cx| DialogHarness {
            area,
            panels: vec![panel],
        });
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

/// 真点对话框上的某个按钮：headless 下 `debug_bounds` 的坐标与鼠标命中测试对不上
/// （试过 `simulate_click` + `run_until_parked`：单跑能中、全套跑就中不了），
/// 因此对话框只断言“层真渲染 + 按钮真在”，点击分发不在这里赌。
pub(crate) fn dialog_button_rendered(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.debug_bounds(selector).is_some()
}

/// 【遗留 #7】值查看器：右键「查看完整值」→ 模态弹层真的弹出（列/行/值 + 复制按钮）
///
/// 钩子接线（网格右键 → `open_value_preview`）另有单测覆盖；这里验收的是**弹层行为**。
/// 弹层要 `&mut Window`，而生产钩子只给 `&mut App`——宿主经 `self.window.update` 转手，
/// 但 headless 测试里窗口不在 `App::windows`（测试上下文持有），转手会报
/// `window not found` 而弹不出。所以窗口级验收走**同一弹层实现 + 真实窗口**的
/// [`EditorHostPanel::open_value_preview_with_window`]（生产方法），钩子路径只验
/// 「目标被记录 + 转手失败留可读原因」。
#[gpui_kit::test]
fn value_preview_opens_as_a_dialog_from_the_host(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\report.sql", "select 1");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-value-preview");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    // 带窗口的生产方法：同一弹层实现 + 真实窗口
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_value_preview_with_window("第一行\n第二行", "notes", 3, window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "对话框层要真渲染（不是只有状态）"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-value-preview-copy"),
        "复制按钮要在值查看器里"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-value-preview-body"),
        "值内容要真渲染在弹层里"
    );

    // 关闭（Esc / 点遮罩 / 关闭按钮走同一条路）：弹层收起
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "关闭后弹层收起"
    );

    // 钩子路径（无 window）：headless 下转手必然失败（测试环境限制），
    // 但目标要记录、失败要留可读原因——真实窗口下正常弹。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_value_preview("钩子值", "notes", 1, cx);
        });
    });
    let (recorded, message) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (panel.value_preview_for_test(), panel.message.clone())
    });
    assert!(recorded, "目标要记录（弹层内容在 with_window 路径已验证）");
    assert!(
        message
            .as_deref()
            .is_some_and(|m| m.contains("弹不出")),
        "headless 转手失败要留可读原因：{message:?}"
    );
}

/// 需要付出代价的切换：先问，不问就不切（禁止静默切换，原型 §1.3）
#[gpui_kit::test]
fn switching_to_text_asks_before_hiding_results(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\ask.sql", "select 1;");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-ask");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.request_mode_switch(EditorMode::General, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "SQL → 文本要先确认"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "对话框层要真渲染（不是只有状态）"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql),
        "未确认前模式不得变（静默切换是禁止项）"
    );

    // 取消（Esc / 点遮罩 / 取消按钮走的是同一条路：对话框关掉、状态不变）
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-switch-cancel"),
        "取消按钮要在对话框里"
    );
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql),
        "取消就是什么都不做"
    );
}

/// 确认后真切：SQL → 分析要带粒度，按钮上的粒度直接决定单元怎么分
#[gpui_kit::test]
fn confirming_the_switch_applies_the_chosen_granularity(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\cells.sql", "select 1; select 2;");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-confirm");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.request_mode_switch(EditorMode::Analysis, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    // 粒度二选一：两个动作按钮就是选择本身（不是单选 + 确定）
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-switch-per-statement")
            && dialog_button_rendered(cx, "editor-dialog-editor-switch-single"),
        "粒度两个选项都该在对话框里"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql),
        "未确认前模式不得变"
    );

    // 确认（真路径是按钮回调，这里直接走同一个入口）
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.confirm_mode_switch(
                EditorMode::Analysis,
                CellGranularity::PerStatement,
                window,
                cx,
            )
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Analysis),
        "模式落到文档上"
    );
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        text.contains("-- %%"),
        "按语句拆分后文本层要有单元分隔标记：{text}"
    );
}

/// 免确认的切换（文本 → SQL）不弹窗：不是所有切换都要打断用户
#[gpui_kit::test]
fn switching_from_text_to_sql_needs_no_dialog(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\sql\notes.txt",
            "select 1;",
            EditorMode::General,
        ))
        .id()
        .clone();
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-free-switch");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.request_mode_switch(EditorMode::Sql, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "文本 → SQL 没有代价，不必打断用户"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql)
    );
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, "select 1;", "免确认的切换不动内容");
}

/// 确认回调与粒度要能独立驱动（宿主也能走这条路；粒度不参与内容变换时不该改内容）
#[gpui_kit::test]
fn confirmed_switch_recomputes_the_plan_with_the_chosen_granularity(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\single.sql", "select 1; select 2;");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-single-cell");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.confirm_mode_switch(EditorMode::Analysis, CellGranularity::Single, window, cx)
        });
    });

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        !text.contains("-- %%"),
        "整篇一个单元不该出现分隔标记：{text}"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Analysis)
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 对话框流程（A9）：关闭三态 / 另存为
// ═══════════════════════════════════════════════════════════════════════

/// 假“系统文件对话框”：把用户选的路径固定下来（真写盘，不碰宿主与 rfd）
fn attach_picker(shared: &EditorShared, picked: Option<PathBuf>) {
    shared.attach_save_path_picker(Rc::new(move |_current, _default_name| picked.clone()));
}

/// 脏文档关闭：先弹三态确认，**确认之前一张纸也不动**
#[gpui_kit::test]
fn closing_a_dirty_document_asks_before_touching_anything(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\dirty.sql", "select 1;");
    shared.update(|service| service.set_content(&id, "select 2;".to_string()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-close-ask");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });
    cx.update(|window, cx| request_close_document(&area, panel.clone(), window, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "脏文档关闭要先问"
    );
    assert!(shared.service().find(&id).is_some(), "未确认前文档不能关");
    assert!(!cx.update(|_window, cx| panel.read(cx).is_closed()));

    // 取消：文档与面板都留着
    let (area, panel) = (area.clone(), panel.clone());
    cx.update(|window, cx| {
        window.close_dialog(cx);
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Cancel,
            window,
            cx,
        );
    });
    assert!(shared.service().find(&id).is_some(), "取消后文档还在");
    assert!(!cx.update(|_window, cx| panel.read(cx).is_closed()));
}

/// “不保存”分支：直接丢改动关掉（用户明确选了不保存）
#[gpui_kit::test]
fn discarding_unsaved_changes_closes_without_writing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("discard");
    let path = dir.join("keep.sql");
    std::fs::write(&path, "select 1;").expect("写盘");

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(&path, "select 1;", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_content(&id, "select 999;".to_string()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-discard");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Discard,
            window,
            cx,
        )
    });

    assert!(shared.service().find(&id).is_none(), "不保存 = 文档关掉");
    assert!(cx.update(|_window, cx| panel.read(cx).is_closed()));
    assert_eq!(
        std::fs::read_to_string(&path).expect("读盘"),
        "select 1;",
        "磁盘上的内容不得被未保存的改动覆盖"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// “保存”分支 + 未命名文档：走另存为（路径选择端口）→ 写盘成功才关
#[gpui_kit::test]
fn saving_an_untitled_document_writes_it_before_closing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("save-then-close");
    let target = dir.join("new.sql");

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 7;", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_content(&id, "select 7;".to_string()));
    attach_picker(&shared, Some(target.clone()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-save-close");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Save,
            window,
            cx,
        )
    });

    assert_eq!(
        std::fs::read_to_string(&target).expect("另存为应真的写盘"),
        "select 7;"
    );
    assert!(shared.service().find(&id).is_none(), "写完才能关");
    assert!(cx.update(|_window, cx| panel.read(cx).is_closed()));
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "顺利写盘不该弹二次确认"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// 用户在另存为里取消：文档**不关**，而且不能当已经存过
#[gpui_kit::test]
fn cancelling_save_as_keeps_the_document_open(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 7;", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_content(&id, "select 8;".to_string()));
    attach_picker(&shared, None);

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-save-cancel");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Save,
            window,
            cx,
        )
    });

    assert!(shared.service().find(&id).is_some(), "取消另存为就不能关");
    assert!(!cx.update(|_window, cx| panel.read(cx).is_closed()));
    assert!(
        shared.service().find(&id).expect("文档").is_dirty(),
        "没写盘就仍是脏的"
    );
}

/// 未接入系统文件对话框时，“保存”分支不能静默吞掉：状态栏要说原因
#[gpui_kit::test]
fn saving_without_a_path_picker_reports_why(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 7;", EditorMode::Sql))
        .id()
        .clone();
    // 不注入 picker：模拟宿主未接系统文件对话框

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-no-picker");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Save,
            window,
            cx,
        )
    });

    assert!(shared.service().find(&id).is_some(), "没存成就不许关");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("要留原因").contains("系统文件对话框"),
        "原因要说清是宿主未接入"
    );
}

/// `Ctrl+Shift+S`（另存为，不关文档）：写盘、标题跟随、清脏；取消则什么都不变
#[gpui_kit::test]
fn save_as_rewrites_the_path_and_keeps_the_document_open(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("save-as");
    let target = dir.join("renamed.sql");

    let (shared, id) = shared_with_document(r"D:\sql\original.sql", "select 1;");
    shared.update(|service| service.set_content(&id, "select 2;".to_string()));
    attach_picker(&shared, Some(target.clone()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-save-as");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    let saved = cx.update(|_window, cx| request_save_as(&panel, cx));
    assert!(saved, "另存为应当写盘成功");

    assert_eq!(std::fs::read_to_string(&target).expect("读盘"), "select 2;");
    let (path, title, dirty) = {
        let service = shared.service();
        let doc = service.find(&id).expect("文档");
        (
            doc.path().map(|path| path.to_path_buf()),
            doc.title().to_string(),
            doc.is_dirty(),
        )
    };
    assert_eq!(path.as_deref(), Some(target.as_path()), "路径改为另存目标");
    assert_eq!(title, "renamed.sql", "标签标题跟随文件名");
    assert!(!dirty, "另存为即已保存");
    assert!(shared.service().find(&id).is_some(), "另存为不关文档");
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════════════
// B1：连接绑定
// ══════════════════════════════════════════════════════════════════════

/// 假连接端口：固定选项表 + 可注入的建连失败；建连成功后运行态变真
pub(crate) struct FakeConnections {
    options: Vec<ConnectionOption>,
    fail_with: Option<String>,
    connected: std::cell::RefCell<Vec<String>>,
}

impl FakeConnections {
    pub(crate) fn new(options: Vec<ConnectionOption>) -> Self {
        Self {
            options,
            fail_with: None,
            connected: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl ConnectionsPort for FakeConnections {
    fn options(&self) -> Vec<ConnectionOption> {
        let connected = self.connected.borrow();
        self.options
            .iter()
            .map(|option| ConnectionOption {
                connected: option.connected || connected.contains(&option.id),
                ..option.clone()
            })
            .collect()
    }

    fn ensure_connected(&self, conn_id: &str) -> Result<(), String> {
        if let Some(reason) = &self.fail_with {
            return Err(reason.clone());
        }
        self.connected.borrow_mut().push(conn_id.to_string());
        Ok(())
    }
}

pub(crate) fn option(id: &str, short: &str, name: &str) -> ConnectionOption {
    ConnectionOption {
        id: id.to_string(),
        short: short.to_string(),
        name: name.to_string(),
        connected: false,
        // B10：方言来自驱动；大多数用例用 MySQL，需要别的方言的用例自己改这一项
        db_type: "mysql_native".to_string(),
    }
}

/// 绑定一个连接：状态栏跟着变，且**绑定是文档属性**（换文档不影响）
#[gpui_kit::test]
fn binding_a_connection_shows_up_in_the_status_bar(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\bound.sql", "select 1;");
    let port = Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )]));
    shared.attach_connections(port);

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 初始：未绑定（会跟随当前连接，状态栏要如实说明）
    assert_eq!(
        shared.connection_status_text(None),
        "○ 未绑定连接",
        "未绑定要看得懂"
    );

    // 绑定：先自动建连（假端口成功）→ 写回文档 → 状态栏带运行态点
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });
    assert_eq!(
        shared.service().connection_for(&id).as_deref(),
        Some("P_orders"),
        "绑定要落到文档上"
    );
    assert_eq!(shared.connection_status_text(Some("P_orders")), "●P·orders");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("绑定要有可见反馈").contains("已绑定"),
        "成功也要留痕（否则用户不知道自自动建连了没）"
    );

    // 解绑：回到“跟随当前连接”
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.bind_connection(None, cx));
    });
    assert_eq!(shared.service().connection_for(&id), None);
}

/// 建连失败**不绑定**：半绑定比不绑定更难排查
#[gpui_kit::test]
fn a_failed_connection_leaves_the_document_unbound(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\unbound.sql", "select 1;");
    let mut port = FakeConnections::new(vec![option("P_down", "P", "down")]);
    port.fail_with = Some("端口不可达".to_string());
    shared.attach_connections(Rc::new(port));

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_down".to_string()), cx)
        });
    });

    assert_eq!(
        shared.service().connection_for(&id),
        None,
        "建连失败不得绑定"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    let message = message.expect("失败必须留原因");
    assert!(message.contains("连接不可用"), "{message}");
    assert!(
        message.contains("端口不可达"),
        "原因要原样带上来：{message}"
    );
}

// ══════════════════════════════════════════════════════════════════════
// B11：宿主发起的执行（导航「查看数据」打开后自动跑一次）
// ══════════════════════════════════════════════════════════════════════

/// 宿主用 `run_all` 触发的执行与 `Ctrl+Shift+Enter` 同一条路（同一套目标解析与回填）
#[gpui_kit::test]
fn the_host_can_trigger_a_full_run(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) =
        shared_with_runner("select 1;\nselect 2;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 不按键：宿主直接调公开入口（导航「查看数据」路径就是这一条）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.run_all(cx)));
    wait_for_result(cx, &panel);

    assert_eq!(
        seen.lock().expect("锁").as_slice(),
        ["select 1;\nselect 2;".to_string()],
        "宿主触发的是整篇脚本"
    );

    // 空文档：不打扰执行器（与快捷键路径同一判据）
    let shared_empty = EditorShared::new();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    shared_empty.attach_runner(std::sync::Arc::new(CountingRunner {
        calls: calls.clone(),
    }));
    let id_empty = shared_empty
        .open(OpenRequest::untitled("-- 只有注释\n", EditorMode::Sql))
        .id()
        .clone();
    let (panel_empty, cx) = open_panel(cx, &shared_empty, &id_empty);
    cx.update(|_window, cx| panel_empty.update(cx, |panel, cx| panel.run_all(cx)));
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "没有可执行内容就该被拒，不该打扰执行器"
    );
}

/// 只数调用次数的执行器
struct CountingRunner {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl QueryRunner for CountingRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(QueryData::default())
    }
}

/// 绑定的连接要**真的**传到执行器（架构 §12 #26：以前只能走“当前活动连接”）
#[gpui_kit::test]
fn the_bound_connection_reaches_the_execution_port(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, _seen, seen_connections) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders", "P", "orders",
    )])));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    assert_eq!(
        seen_connections.lock().expect("锁").as_slice(),
        [Some("P_orders".to_string())],
        "执行器必须收到文档绑定的连接"
    );

    // 【B5】结果也要记住它在哪个连接上跑出来的（结果工具栏显示来源）
    let entry_connection = cx.update(|_window, _cx| {
        shared
            .results()
            .active(&id)
            .and_then(|entry| entry.connection.clone())
    });
    assert_eq!(
        entry_connection.as_deref(),
        Some("P_orders"),
        "结果记录要带上来源连接"
    );
    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test(cx))
        .expect("有结果就有工具栏");
    assert_eq!(
        toolbar.connection.as_deref(),
        Some("●P·orders"),
        "工具栏拿到的是可读的连接段"
    );
}

/// 执行通道在 SQL 模式给出工具栏右区连接定位（连接名称 + schema@db 固定槽；
/// 2026-10-27 自状态栏迁出；没有端口也说清楚，不假装有得选）
#[gpui_kit::test]
fn the_toolbar_shows_the_connection_slots_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\picker.sql", "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        let panel_slot = Rc::new(std::cell::RefCell::new(None::<Entity<EditorHostPanel>>));
        let slot = panel_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let shell = cx.new(|cx| EditorHostPanel::new(shared, id, window, cx));
            *slot.borrow_mut() = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let panel = panel_slot.borrow().clone().expect("EditorHostPanel 已创建");
        (panel, cx)
    };

    cx.update(|window, cx| window.draw(cx).clear(cx));
    // 连接定位在工具栏右区：连接名称槽（editor-conn-name；2026-10-27 拆为名称+▾ 两段）
    assert!(
        cx.debug_bounds("editor-conn-name").is_some(),
        "SQL 模式工具栏应当有连接名称槽（执行通道）"
    );
    assert!(
        cx.debug_bounds("editor-status-connection").is_none(),
        "状态栏不再显示连接路径（已迁工具栏三固定区）"
    );
    // 未绑定连接（跟随当前连接态）：schema 概念还没出现 → 无 schema@库 槽（不假装）
    assert!(
        cx.debug_bounds("editor-schema-name").is_none(),
        "未绑定连接时没有 schema 槽"
    );
    // 【B13】执行位置指示器仍在工具栏（通道 pill）
    assert!(
        cx.debug_bounds("editor-channel").is_some(),
        "SQL 模式应当有「执行位置」指示器"
    );

    // 未接端口：选项表为空（菜单会直说“未接入连接列表”）
    assert!(shared.connection_options().is_empty());
    assert!(!shared.has_connections());

    // 文本模式：不与数据库通信 → 连不上问题都不该问，三个指示器都不出现
    shared.update(|service| {
        service.set_mode(&id, EditorMode::General);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-conn-name").is_none());
    assert!(cx.debug_bounds("editor-schema-name").is_none());
    assert!(
        cx.debug_bounds("editor-channel").is_none(),
        "文本模式没有“在哪儿执行”这回事"
    );
}

// ══════════════════════════════════════════════════════════════════════
// B16：关闭口径 + 工具栏分层
// ══════════════════════════════════════════════════════════════════════

/// 脏文档不给标签 ✕：Dock 没有“关闭前否决”钩子，能问用户的入口只留 `Ctrl+W`
#[gpui_kit::test]
fn a_dirty_document_has_no_close_glyph(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\glyph.sql", "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 刚打开：干净 → 有 ✕（`closable` 是 Dock 显示 ✕ 的判据）
    assert!(
        cx.update(|_window, cx| panel.read(cx).closable(cx)),
        "干净文档应当可关（标签上有 ✕）"
    );

    // 改动 → 脏 → 收起 ✕（否则 Dock 会直接移除面板，静默丢掉改动）
    shared.update(|service| service.set_content(&id, "select 2;".to_string()));
    assert!(
        !cx.update(|_window, cx| panel.read(cx).closable(cx)),
        "脏文档不得给 ✕（Dock 无否决钩子，只能靠收起入口）"
    );
    // 标签脏点仍然在（用户看得出“有未保存改动”）
    assert!(cx.update(|_window, cx| panel.read(cx).is_dirty()));

    // 保存（清脏）→ ✕ 回来
    shared.update(|service| {
        service.mark_saved(&id);
    });
    assert!(
        cx.update(|_window, cx| panel.read(cx).closable(cx)),
        "保存后 ✕ 应当回来"
    );
}

/// 工具栏按模式分层：文本 / 分析模式不放“执行”（那是 SQL 模式的能力，不在 UI 上假装）
#[gpui_kit::test]
fn the_toolbar_only_offers_execution_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\layers.sql", "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    let exec_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-exec-run").is_some()
    };

    // SQL 模式：模式指示器 + 执行
    assert!(exec_present(cx), "SQL 模式应当有执行按钮");
    assert!(
        cx.debug_bounds("editor-mode-indicator").is_some(),
        "模式指示器在所有模式下都在"
    );

    // 文本模式：只剩模式指示器（不与数据库通信）
    shared.update(|service| {
        service.set_mode(&id, EditorMode::General);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!exec_present(cx), "文本模式不得出现执行按钮");
    assert!(cx.debug_bounds("editor-mode-indicator").is_some());

    // 分析模式：执行属笔记级动作（1c），现在也不放
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Analysis);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!exec_present(cx), "分析模式的执行随单元落地（1c）");
}

// ══════════════════════════════════════════════════════════════════════
// 书签：说明输入框 + 管理器浮层（工具栏入口 → 弹层）
// ══════════════════════════════════════════════════════════════════════

/// 右键「添加书签/说明…」：当前行弹说明输入框；保存路径（apply_bookmark_label）
/// 改 label 并落盘 sidecar——"写了说明才被记录"（无说明折叠不落盘）。
#[gpui_kit::test]
fn bookmark_note_dialog_applies_a_label_and_persists(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("bookmark-note");
    let path = dir.join("note.sql");
    std::fs::write(&path, "select 1\nselect 2\nselect 3").expect("写盘");
    let sidecar = dir.join(".note.sql.meta.json");

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-bookmark-note");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    // 当前行（默认第 1 行）→ 说明输入框弹出（生产入口，带真实窗口）
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.on_add_bookmark_note(&crate::commands::AddBookmarkNote, window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "说明输入框要弹出"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-note-input"),
        "输入框要在弹层里"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-note-save"),
        "保存按钮要在弹层里"
    );

    // 保存路径：apply_bookmark_label（输入框真实键入在 headless 难驱动，走同一提交回调）
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.apply_bookmark_label(1, "折叠说明：CTE 迁移遗留".to_string(), window, cx);
        });
    });
    assert_eq!(shared.bookmarks(&id).len(), 1, "说明后内存应有一枚书签");
    assert_eq!(shared.bookmarks(&id)[0].label, "折叠说明：CTE 迁移遗留");
    assert!(sidecar.exists(), "说明要落盘 sidecar");
    let meta = file_meta::read_meta(&path).expect("读回 meta");
    assert_eq!(meta.bookmarks[0].label, "折叠说明：CTE 迁移遗留");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 管理器浮层：列书签（行号 + 徽标 + 说明）+ 三操作；无书签给空态
#[gpui_kit::test]
fn bookmark_manager_lists_and_offers_actions(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\mgr.sql", "select 1\nselect 2\nselect 3");
    // 预置：一枚默认书签（行 1，label=行号）+ 一枚说明书签（行 3）
    shared.set_bookmarks(
        &id,
        vec![
            file_meta::Bookmark {
                line: 1,
                label: "1".to_string(),
            },
            file_meta::Bookmark {
                line: 3,
                label: "折叠说明：权限模板".to_string(),
            },
        ],
    );
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-bookmark-mgr");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_bookmark_manager(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "管理器要弹出"
    );
    eprintln!(
        "DIAG line1={} jump3={} edit3={} delete1={} empty={}",
        cx.debug_bounds("editor-dialog-editor-bookmark-line-1").is_some(),
        cx.debug_bounds("editor-dialog-editor-bookmark-jump-3").is_some(),
        cx.debug_bounds("editor-dialog-editor-bookmark-edit-3").is_some(),
        cx.debug_bounds("editor-dialog-editor-bookmark-delete-1").is_some(),
        cx.debug_bounds("editor-dialog-editor-bookmark-empty").is_some()
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-line-1"),
        "行 1 书签在列表"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-jump-3"),
        "说明书签行 3 有跳转"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-edit-3"),
        "说明书签行 3 有编辑"
    );
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-delete-1"),
        "行 1 有删除"
    );
}

/// 管理器空态：没书签时给引导文案，不是"加载失败"
#[gpui_kit::test]
fn bookmark_manager_shows_empty_state_when_no_bookmarks(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\empty.sql", "select 1");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-bookmark-empty");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_bookmark_manager(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-bookmark-empty"),
        "空态文案要在"
    );
}

