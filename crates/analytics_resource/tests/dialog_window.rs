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

use std::cell::{Cell, RefCell};
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
    open_checkout_dialog_with, submit_checkout, suggest_work_copy_name,
};
use rds_analytics_resource::dialogs::index_repair::{
    RepairDialogState, RepairGroup, RepairRow, open_index_repair_dialog_with,
};
use rds_analytics_resource::dialogs::pick::{
    DraftCandidate, PickDialogSeed, PickDialogState, open_draft_pick_dialog_with, submit_pick,
};
use rds_analytics_resource::dialogs::tag::{
    TagChoice, TagDialogSeed, TagDialogState, open_tag_dialog,
};
use rds_analytics_resource::dialogs::trash::{
    ForeignTrash, TrashDialogSeed, TrashDialogState, TrashRow, open_trash_dialog_with,
};
use rds_analytics_resource::dialogs::version::{
    VersionDialogSeed, VersionDialogState, VersionRow, open_version_dialog_with,
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

fn draft(rel: &str, connection: Option<&str>) -> DraftCandidate {
    DraftCandidate {
        abs_path: std::path::PathBuf::from(format!("D:/p/scratchpad/{rel}")),
        rel_path: rel.to_string(),
        display_name: rel
            .rsplit_once('/')
            .map(|(_, name)| name)
            .unwrap_or(rel)
            .trim_end_matches(".sql")
            .to_string(),
        connection_id: connection.map(str::to_string),
    }
}

#[gpui_kit::test]
fn draft_pick_dialog_opens_and_hands_back_the_checked_rows(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let picked_log: Rc<RefCell<Vec<Vec<DraftCandidate>>>> = Rc::new(RefCell::new(Vec::new()));

    let candidates = vec![
        draft("reports/a.sql", Some("conn_1")),
        draft("b.sql", None),
        draft("reports/c.sql", None),
    ];
    let seed = PickDialogSeed {
        candidates: candidates.clone(),
    };
    let state = PickDialogState::new(seed.candidates.len());
    {
        let picked_log = picked_log.clone();
        cx.update(|window, cx| {
            open_draft_pick_dialog_with(
                window,
                cx,
                seed,
                state.clone(),
                move |picked, _window, _cx| {
                    picked_log.borrow_mut().push(picked);
                },
            );
        });
        // 未选任何一条：不提交（按钮本就置灰，这条是给今后改动留的绳）。
        assert!(submit_pick(&candidates, &state).is_none());
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "草稿选择对话框应打开"
    );
    assert!(cx.debug_bounds("draft-pick-ok").is_some());
    assert!(cx.debug_bounds("draft-pick-cancel").is_some());
    assert!(picked_log.borrow().is_empty(), "没选就不该回调宿主");

    // 勾两条（含一条带来源连接的）：提交收的就是它们，顺序与列表一致。
    state.set_selected(0, true);
    state.set_selected(2, true);
    let picked = submit_pick(&candidates, &state).expect("选了两条就该通过");
    assert_eq!(
        picked
            .iter()
            .map(|candidate| candidate.rel_path.as_str())
            .collect::<Vec<_>>(),
        vec!["reports/a.sql", "reports/c.sql"]
    );
    assert_eq!(
        picked[0].connection_id.as_deref(),
        Some("conn_1"),
        "来源连接随行走"
    );
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
        cx.update(|_window, cx| submit_archive(&inputs_for_submit, cx))
            .is_none(),
        "空显示名不该通过校验"
    );
    assert!(submitted.borrow().is_empty(), "校验不过时宿主不该被通知");

    // 填好：拿到解析后的结果（标签去空去重、保留份数解析成数字）。
    cx.update(|window, cx| {
        inputs_for_submit
            .name
            .update(cx, |state, cx| state.set_value("月报 2026", window, cx));
        inputs_for_submit.tags.update(cx, |state, cx| {
            state.set_value(" 报表, 月度 ，报表 ", window, cx)
        });
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
        cx.update(|_window, cx| submit_archive(&inputs_for_submit, cx))
            .is_none(),
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
        inputs_for_submit.file_name.update(cx, |state, cx| {
            state.set_value("子目录/月报.sql", window, cx)
        });
    });
    assert!(
        cx.update(|_window, cx| submit_checkout(&inputs_for_submit, cx))
            .is_none(),
        "含路径分隔符的文件名不该通过"
    );
    assert!(submitted.borrow().is_empty());
}

fn version_row(version: i32, is_current: bool, has_copy: bool) -> VersionRow {
    VersionRow {
        version,
        is_current,
        time_label: format!("2026-09-17 0{version}:00"),
        size_label: "1.2 KB".to_string(),
        hash_short: "0123456789ab".to_string(),
        has_copy,
        delta_label: if version > 1 {
            format!("较 v{} 大小+0.2 KB · 指纹已变", version - 1)
        } else {
            String::new()
        },
    }
}

fn version_seed() -> VersionDialogSeed {
    VersionDialogSeed {
        name: "月报".to_string(),
        current_version: 3,
        rows: vec![
            version_row(3, true, true),
            version_row(2, false, true),
            // v1 的副本已被保留策略裁掉：它仍要列出来（行上标"副本缺失"）。
            version_row(1, false, false),
        ],
    }
}

#[gpui_kit::test]
fn version_dialog_lists_rows_and_shows_actions_after_selection(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let closed: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let state = VersionDialogState::new();
    {
        let closed = closed.clone();
        cx.update(|window, cx| {
            open_version_dialog_with(
                window,
                cx,
                version_seed(),
                state.clone(),
                move |_action, _window, _cx| {},
                move |_cx| closed.set(closed.get() + 1),
            );
        });
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "版本历史对话框应打开"
    );
    assert!(cx.debug_bounds("version-close").is_some());
    assert!(
        cx.debug_bounds("version-row-3").is_some() && cx.debug_bounds("version-row-1").is_some(),
        "当前版本与历史行都要在（行数少的存档就是两行）"
    );
    assert!(
        cx.debug_bounds("version-restore").is_none(),
        "没选中就不摆动作栏（768px 里每行三个按钮装不下）"
    );

    // 选中历史行 → 动作栏登场。
    state.set_selected(Some(2));
    draw(cx);
    assert!(cx.debug_bounds("version-restore").is_some());
    assert!(cx.debug_bounds("version-checkout").is_some());
    assert!(cx.debug_bounds("version-delete-copy").is_some());

    // 动作完成 → 宿主换一批行（还原把 v4 顶上来了，选中的 v2 不在新行里）：
    // 选中与动作栏一起退场（不能指向一个已经不在列表里的版本）。
    state.set_rows(vec![version_row(4, true, true), version_row(3, false, true)]);
    draw(cx);
    assert!(state.selected().is_none(), "新行里没有它 → 选中清掉");
    assert!(cx.debug_bounds("version-restore").is_none());
    assert!(
        cx.debug_bounds("version-row-4").is_some(),
        "换过的行要真渲染出来（还原后的新版本就在列表里）"
    );
    assert_eq!(closed.get(), 0, "对话框还开着：动作不该把它关掉");
}

fn repair_row(group: RepairGroup, title: &str) -> RepairRow {
    RepairRow {
        group,
        title: title.to_string(),
        detail: format!("本体 resources/{title}.sql"),
        hash_detail: if group == RepairGroup::Changed {
            "登记 111111111111 · 实际 222222222222".to_string()
        } else {
            String::new()
        },
        rel_path: format!("{title}.sql"),
        resource_id: (group != RepairGroup::Untracked).then(|| "ar_1".to_string()),
    }
}

#[gpui_kit::test]
fn index_repair_dialog_groups_rows_and_shows_inline_actions(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let closed: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let state = RepairDialogState::new();
    {
        let closed = closed.clone();
        cx.update(|window, cx| {
            open_index_repair_dialog_with(
                window,
                cx,
                state.clone(),
                move |_action, _window, _cx| {},
                move |_cx| closed.set(closed.get() + 1),
            );
        });
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "索引修复对话框应打开"
    );
    assert!(cx.debug_bounds("repair-close").is_some());
    assert!(
        cx.debug_bounds("repair-adopt-0").is_none(),
        "干净时只是“没有需要处理的问题”，不给动作按钮"
    );

    // 三组各一行：每组第一个动作的选择器都是“…-0”（序号按组内算）。
    state.set_rows(vec![
        repair_row(RepairGroup::Untracked, "a.sql"),
        repair_row(RepairGroup::Missing, "周报"),
        repair_row(RepairGroup::Changed, "月报"),
    ]);
    draw(cx);
    assert!(cx.debug_bounds("repair-adopt-0").is_some(), "未登记行给补登");
    assert!(
        cx.debug_bounds("repair-delete-0").is_some(),
        "缺本体行给删记录"
    );
    assert!(
        cx.debug_bounds("repair-accept-0").is_some(),
        "指纹不匹配行给接受当前内容"
    );
    assert!(
        cx.debug_bounds("repair-versions-0").is_some(),
        "并可跳去版本历史挑一版还原"
    );

    // 修完最后一项 → 宿主换空行：空态回来（对话框没被关掉）。
    state.set_rows(Vec::new());
    draw(cx);
    assert!(cx.debug_bounds("repair-adopt-0").is_none());
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "修复不该把对话框关掉"
    );
    assert_eq!(closed.get(), 0);
}

fn trash_row(id: &str) -> TrashRow {
    TrashRow {
        trash_id: id.to_string(),
        name: format!("dau_{id}.sql"),
        original_label: format!("resources/dau_{id}.sql"),
        kind_label: "文件".to_string(),
        time_label: "2026-09-18 10:00".to_string(),
        size_label: "1.2 KB".to_string(),
    }
}

#[gpui_kit::test]
fn trash_dialog_lists_own_rows_and_keeps_the_shared_store_visible(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let state = TrashDialogState::new();
    let seed = TrashDialogSeed {
        rows: vec![trash_row("t1")],
        // 别人的条目：只出一句说明，不进行列表（所以没有 trash-restore-1 这种行）。
        foreign: vec![ForeignTrash {
            module_label: "草稿箱".to_string(),
            count: 2,
        }],
    };
    {
        cx.update(|window, cx| {
            open_trash_dialog_with(
                window,
                cx,
                seed,
                state.clone(),
                move |_action, _window, _cx| {},
                move |_cx| {},
            );
        });
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "回收站对话框应打开"
    );
    assert!(cx.debug_bounds("trash-close").is_some());
    assert!(cx.debug_bounds("trash-empty").is_some(), "清空是常态入口");
    assert!(
        cx.debug_bounds("trash-restore-0").is_some(),
        "自己的条目给还原"
    );
    assert!(cx.debug_bounds("trash-purge-0").is_some());
    assert!(
        cx.debug_bounds("trash-restore-1").is_none(),
        "别人的条目不能出现在行列表里"
    );

    // 宿主换空行（刚清空 / 刚全还原）：行消失，对话框不关掉。
    state.set_rows(Vec::new());
    draw(cx);
    assert!(cx.debug_bounds("trash-restore-0").is_none());
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "换行不该把对话框关掉"
    );
}

#[gpui_kit::test]
fn tag_dialog_lists_choices_and_keeps_the_create_path_gated(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let cx = harness(cx);
    let state = TagDialogState::new(vec!["at_1".to_string()]);
    let seed = TagDialogSeed {
        resource_name: "月报".to_string(),
        options: vec![
            TagChoice {
                id: "at_1".to_string(),
                name: "重要".to_string(),
                count: 3,
            },
            TagChoice {
                id: "at_2".to_string(),
                name: "待办".to_string(),
                count: 0,
            },
        ],
        selected: vec!["at_1".to_string()],
    };
    {
        let cx: &mut VisualTestContext = cx;
        cx.update(|window, cx| {
            let input = cx.new(|cx| {
                gpui_kit::component::input::InputState::new(window, cx).placeholder("新建标签")
            });
            open_tag_dialog(
                window,
                cx,
                seed,
                state.clone(),
                input,
                move |_event, _window, _cx| {},
                move |_cx| {},
            );
        });
    }
    draw(cx);

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "标签对话框应打开"
    );
    assert!(cx.debug_bounds("tag-close").is_some());
    assert!(cx.debug_bounds("tag-apply").is_some());
    assert!(
        cx.debug_bounds("tag-create").is_some(),
        "新建路径恒在（名字为空时按钮置灰）"
    );
    assert!(cx.debug_bounds("tag-choice-at_1").is_some());
    assert!(cx.debug_bounds("tag-choice-at_2").is_some());

    // 词典被宿主换掉（标签被删）：行跟着换，对话框不关。
    state.set_options(vec![TagChoice {
        id: "at_2".to_string(),
        name: "待办".to_string(),
        count: 0,
    }]);
    draw(cx);
    assert!(cx.debug_bounds("tag-choice-at_1").is_none());
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "换行不该把对话框关掉"
    );
}
