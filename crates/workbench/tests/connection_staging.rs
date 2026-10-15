//! 暂存列表（多连接连续编辑）行为测试 —— 对应原型设计 §2.2 与开发计划 C5。
//!
//! 覆盖：切换条目保留各自字段、删除至最后一条自动补位、**保存语义（2026-10-02 用户决策）**：
//! 「保存」仅暂存草稿箱（不落库），「保存并退出」批量提交后 `staging_clear` 清空草稿箱、
//! 部分失败时 `remove_drafts_at` 只移除已成功条目（失败保留供修正）；暂存区只保留未保存草稿
//! （历史遗留的已保存条目会被清理）。不接真实服务（`DataSourceService::global()` 不可用时安全降级）。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::components::connection_dialog::{ConnectionDialogState, ConnectionDraft};
use rds_workbench::panels::{EditorPanel, Shared};

/// 简化宿主：与 `WorkbenchView` 同构（挂对话框层 + 注入宿主重绘桥）。
struct Harness {
    _shared: Shared,
    editor: Entity<EditorPanel>,
    dialog: Rc<ConnectionDialogState<Harness>>,
}

impl Harness {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = Shared::new();
        // 面板构造会读设置 global（`SettingsService::*`）：注入默认值，不读用户磁盘配置。
        if !cx.has_global::<settings::model::Settings>() {
            cx.set_global(settings::model::Settings::default());
        }
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let dialog = Rc::new(ConnectionDialogState::<Harness>::new(_window, cx));
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.editor.clone())
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

/// 「保存并退出」批量提交**全部成功**后：`staging_clear` 清空草稿箱（用户决策：提交后不留草稿）。
#[gpui_kit::test]
fn staging_clear_empties_draft_box(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 暂存栏有两条约定已提交的非空草稿（模拟「保存并退出」批量提交前的状态）。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("生产PG", window, cx));
    });
    cx.update(|window, cx| dialog.staging_add(window, cx));
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("测试MySQL", window, cx));
    });
    assert_eq!(cx.update(|_, _cx| dialog.drafts.borrow().len()), 2);

    // 批量提交全部成功 → 清空草稿箱。
    cx.update(|_, cx| dialog.staging_clear(cx));

    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert_eq!(
        drafts.len(),
        0,
        "全成功提交后草稿箱必须清空（不留草稿）"
    );
    assert_eq!(
        cx.update(|_, _cx| dialog.draft_cursor.get()),
        0,
        "清空后光标回到 0"
    );
}

/// 「保存并退出」批量提交**部分失败**后：`remove_drafts_at` 只移除已成功落库的条目，
/// 失败条目保留供修正重试；删空时补一条空草稿（列表恒非空）。
#[gpui_kit::test]
fn remove_drafts_at_keeps_failed_entries(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 三条草稿：0、1、2；模拟 0、2 已成功落库、1 失败 → 只移除 0、2，保留 1。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("草稿A", window, cx));
    });
    cx.update(|window, cx| dialog.staging_add(window, cx));
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("草稿B", window, cx));
    });
    cx.update(|window, cx| dialog.staging_add(window, cx));
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("草稿C", window, cx));
    });
    assert_eq!(cx.update(|_, _cx| dialog.drafts.borrow().len()), 3);

    cx.update(|window, cx| dialog.remove_drafts_at(vec![0, 2], window, cx));

    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert_eq!(drafts.len(), 1, "失败条目（草稿B）应保留");
    assert_eq!(drafts[0].name, "草稿B", "保留的应是失败条目本身");
    assert!(drafts[0].saved_id.is_none());

    // 再移除最后一条 → 自动补空草稿（列表恒非空，状态机不越界）。
    cx.update(|window, cx| dialog.remove_drafts_at(vec![0], window, cx));
    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert_eq!(drafts.len(), 1, "删空后应补一条空草稿");
    assert!(drafts[0].name.is_empty() && drafts[0].saved_id.is_none());
    assert!(cx.update(|_, _cx| dialog.draft_cursor.get()) < drafts.len());
}

#[gpui_kit::test]
fn draft_carries_tags_and_groups(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 标签（逗号分隔）与分组勾选 → 快照应逐条携带。
    cx.update(|window, cx| {
        dialog
            .tags_input
            .update(cx, |s, cx| s.set_value("prod, core", window, cx));
        *dialog.group_checks.borrow_mut() = vec![
            ("g1".to_string(), "alpha".to_string(), true),
            ("g2".to_string(), "beta".to_string(), false),
        ];
    });
    cx.update(|window, cx| dialog.staging_add(window, cx));

    let first = cx.update(|_, _cx| dialog.drafts.borrow()[0].clone());
    assert_eq!(first.tags, "prod, core");
    assert_eq!(first.groups, vec!["g1".to_string()], "仅勾选的分组进入快照");

    // 切换条目：表单应恢复该条目的标签与勾选（当前草稿的输入已写回）。
    cx.update(|window, cx| {
        dialog
            .tags_input
            .update(cx, |s, cx| s.set_value("临时", window, cx));
    });
    cx.update(|window, cx| dialog.staging_select(0, window, cx));
    let tags_now = cx.update(|_, cx| dialog.tags_input.read(cx).value().to_string());
    assert_eq!(tags_now, "prod, core");
    let checked = cx.update(|_, _cx| {
        dialog
            .group_checks
            .borrow()
            .iter()
            .filter(|(_, _, c)| *c)
            .map(|(id, _, _)| id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(checked, vec!["g1".to_string()]);
}

#[gpui_kit::test]
fn staging_prune_saved_drops_legacy_saved_entries(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 历史草稿表可能残留已保存条目（旧版合并行为）：清理入口必须把它们移除。
    cx.update(|_, _cx| {
        let mut drafts = dialog.drafts.borrow_mut();
        drafts.push(ConnectionDraft {
            name: "历史残留".to_string(),
            saved_id: Some("G_conn_legacy".to_string()),
            ..ConnectionDraft::empty()
        });
    });
    cx.update(|_, _cx| dialog.staging_prune_saved());

    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    assert!(
        drafts.iter().all(|d| d.saved_id.is_none()),
        "已保存条目必须被清理（暂存区只放未保存草稿）：{:?}",
        drafts.iter().map(|d| d.saved_id.clone()).collect::<Vec<_>>()
    );
    assert!(!drafts.is_empty(), "列表恒非空（补空草稿）");
}

/// 脏比对等价性（§6 决策 #73）：`form_matches_draft`（逐字段、无分配）必须与
/// `snapshot_form() == draft`（整份构造）给出相同结论；字段集合漂移会在这里暴露。
#[gpui_kit::test]
fn form_matches_draft_agrees_with_snapshot(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog.clone());

    // 基准：以初始表单快照当作「已写回的草稿」；两套比对结论必须始终一致。
    let check = |cx: &mut VisualTestContext,
                 baseline: &rds_workbench::components::connection_dialog::ConnectionDraft,
                 expect_dirty: bool,
                 why: &str| {
        cx.update(|_, cx| {
            let by_fields = dialog.form_matches_draft(baseline, cx);
            let by_snapshot = dialog.snapshot_form(cx) == *baseline;
            assert_eq!(by_fields, by_snapshot, "{why}：逐字段与快照比对必须等价");
            assert_eq!(by_fields, !expect_dirty, "{why}：脏标记期望不符");
        });
    };

    // ① 初始空表单 → 不脏。
    let mut baseline = cx.update(|_, cx| dialog.snapshot_form(cx));
    check(cx, &baseline, false, "初始状态");

    // ② 改名称 → 脏。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("改名", window, cx));
    });
    check(cx, &baseline, true, "改名称后");

    // ③ 把当前表单写回草稿（并重置基准）→ 不脏。
    baseline = cx.update(|_, cx| {
        let d = dialog.snapshot_form(cx);
        dialog.drafts.borrow_mut()[0] = d.clone();
        d
    });
    check(cx, &baseline, false, "写回后");

    // ④ 切 Tab（旧行为也把它算作表单修改）→ 脏；同时验证 live_entry_view 与之一致。
    dialog.active_tab.set(1);
    check(cx, &baseline, true, "切 Tab 后");
    let view = cx.update(|_, cx| dialog.live_entry_view(0, cx));
    let view = view.expect("光标位条目存在");
    assert!(view.dirty, "live_entry_view 应报告脏标记");
    assert_eq!(view.name, "改名");

    // ⑤ 光标越界 → None（与旧行为一致）。
    assert!(cx.update(|_, cx| dialog.live_entry_view(99, cx)).is_none());
}
