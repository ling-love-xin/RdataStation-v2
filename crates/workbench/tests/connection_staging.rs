//! 暂存列表（多连接连续编辑）行为测试 —— 对应原型设计 §2.2 与开发计划 C5。
//!
//! 覆盖：切换条目保留各自字段、删除至最后一条自动补位、保存后草稿转正式并补空草稿、
//! 已保存条目不参与删除。不接真实服务（`DataSourceService::global()` 不可用时安全降级）。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::components::connection_dialog::ConnectionDialogState;
use rds_workbench::panels::{EditorPanel, Shared};

/// 简化宿主：与 `WorkbenchView` 同构（挂对话框层 + 注入宿主重绘桥）。
struct Harness {
    _shared: Shared,
    editor: Entity<EditorPanel>,
    dialog: Rc<ConnectionDialogState>,
}

impl Harness {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = Shared::new();
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let dialog = Rc::new(ConnectionDialogState::new(_window, cx));
        // 宿主重绘桥（生产由 `WorkbenchView::new` 注入）。
        let weak = cx.entity().downgrade();
        let bridge: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        *shared.host_redraw.borrow_mut() = Some(bridge);
        Self {
            _shared: shared,
            editor,
            dialog,
        }
    }
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.editor.clone())
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

fn open_harness(cx: &mut TestAppContext) -> (Entity<Harness>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<Harness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| Harness::new(window, cx));
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

#[gpui_kit::test]
fn staging_add_and_switch_keeps_fields(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 草稿 0：填写名称 A
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("连接A", window, cx));
    });
    // 「+ 添加」：新增草稿并选中，填写名称 B
    cx.update(|window, cx| dialog.staging_add(window, cx));
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("连接B", window, cx));
    });
    assert_eq!(cx.update(|_, _cx| dialog.drafts.borrow().len()), 2);
    assert_eq!(cx.update(|_, _cx| dialog.draft_cursor.get()), 1);

    // 切回草稿 0：字段应恢复（且草稿 1 的输入已写回）
    cx.update(|window, cx| dialog.staging_select(0, window, cx));
    let name_now = cx.update(|_, cx| dialog.name.read(cx).value().to_string());
    assert_eq!(name_now, "连接A", "切换回草稿 0 应恢复其名称");
    assert_eq!(
        cx.update(|_, _cx| dialog.drafts.borrow()[1].name.clone()),
        "连接B",
        "切换前应把当前表单写回草稿 1"
    );
}

#[gpui_kit::test]
fn staging_remove_last_keeps_one_empty_draft(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 初始 1 个空草稿：删除后自动补位（仍为 1 个空草稿）
    cx.update(|window, cx| dialog.staging_remove(0, window, cx));
    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert_eq!(drafts.len(), 1, "删至最后一条应自动补一个空草稿");
    assert!(drafts[0].name.is_empty() && drafts[0].saved_id.is_none());

    // 增到 3 个再删中间条目：数量与光标收敛正确
    cx.update(|window, cx| dialog.staging_add(window, cx));
    cx.update(|window, cx| dialog.staging_add(window, cx));
    assert_eq!(cx.update(|_, _cx| dialog.drafts.borrow().len()), 3);
    cx.update(|window, cx| dialog.staging_remove(1, window, cx));
    assert_eq!(cx.update(|_, _cx| dialog.drafts.borrow().len()), 2);
    assert!(cx.update(|_, _cx| dialog.draft_cursor.get()) < 2);
}

#[gpui_kit::test]
fn staging_after_save_marks_saved_and_appends(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("生产PG", window, cx));
    });
    cx.update(|window, cx| dialog.staging_after_save("G_abc", "生产PG", window, cx));

    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert_eq!(drafts.len(), 2, "保存成功后应追加空草稿以保持连续新建");
    assert_eq!(drafts[0].saved_id.as_deref(), Some("G_abc"));
    assert_eq!(drafts[0].name, "生产PG");
    assert!(drafts[1].saved_id.is_none(), "新草稿应为未保存状态");
    assert_eq!(
        cx.update(|_, _cx| dialog.draft_cursor.get()),
        1,
        "保存后应选中新草稿"
    );
}

#[gpui_kit::test]
fn staging_remove_ignores_saved_entries(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    cx.update(|window, cx| dialog.staging_after_save("G_def", "已保存连接", window, cx));
    // 已保存条目不在暂存列表删除（删除入口归导航栏）
    cx.update(|window, cx| dialog.staging_remove(0, window, cx));

    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert_eq!(drafts.len(), 2);
    assert_eq!(drafts[0].saved_id.as_deref(), Some("G_def"));
}
