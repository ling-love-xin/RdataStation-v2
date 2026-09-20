//! 资产库面板的窗口测试（GPUI headless）。
//!
//! 目的：把面板的**可交互行为**固化成回归——空态与只读提示可渲染、快照推送生效、
//! 选中可由宿主驱动、悬空选中被清掉、**工具栏条件真的改变可见行**、
//! **宿主选中镜像回列表时不重入**。这些是后续改动最容易悄悄弄坏的地方。
//!
//! 两条纪律（都是踩过的坑）：
//! 1. **不通配导入**：`use gpui_kit::*` 会把 gpui 的 `test` 属性宏带进作用域，而
//!    `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己（recursion limit）；
//! 2. **实体访问要包在 `cx.update(|window, cx| …)` 里**：`VisualTestContext` 不实现
//!    `AppContext`，直接 `entity.update(&mut cx, …)` 不能编译。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{
    App, Context, Focusable as _, IntoElement, ParentElement, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};

use rds_analytics_resource::commands::{
    ClearSearch, DeleteSelected, OpenSelected, RenameSelected, SelectAllRows,
};
use rds_analytics_resource::detail_view::{
    ArchiveDetail, ArchiveTagChip, DetailActions, render_detail,
};
use rds_analytics_resource::dnd::GroupDrop;
use rds_analytics_resource::filter::{SortField, SortOrder};
use rds_analytics_resource::model::{ArchiveKind, ArchiveStatus, ArchiveUndo, TagTarget};
use rds_analytics_resource::preview::Preview;
use rds_analytics_resource::resource_view::{
    ArchiveCounts, ArchiveRow, GroupOption, HeaderMenuAction, ResourcesHost, ResourcesPanel,
    ResourcesSnapshot, RowClick, TagOption, dispatch_header_action,
};

/// 宿主替身：只记录调用，不接真实服务（窗口测试不碰后端）。
#[derive(Default)]
struct RecordingHost {
    calls: RefCell<Vec<String>>,
}

impl RecordingHost {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl ResourcesHost for RecordingHost {
    fn request_archive_from_drafts(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("archive:drafts".to_string());
    }
    fn request_archive_from_file(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("archive:file".to_string());
    }
    fn request_open(&self, detail: &ArchiveDetail, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push(format!("open:{}", detail.id));
    }
    fn request_view_stats(&self, detail: &ArchiveDetail, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("view-stats:{}", detail.id));
    }
    fn request_reveal(&self, detail: &ArchiveDetail, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("reveal:{}", detail.id));
    }
    fn request_copy_path(&self, detail: &ArchiveDetail, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("copy-path:{}", detail.id));
    }
    fn request_checkout(&self, detail: &ArchiveDetail, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("checkout:{}", detail.id));
    }
    fn request_version_history(&self, resource_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("versions:{resource_id}"));
    }
    fn request_delete(&self, resource_ids: &[String], _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("delete:{}", resource_ids.join(",")));
    }
    fn request_edit_tags(&self, targets: &[TagTarget], _window: &mut Window, _cx: &mut App) {
        // 记录**整批目标**（批量打标签的验收靠它：多选时是不是真的把选集送下去了）。
        let ids: Vec<&str> = targets.iter().map(|t| t.id.as_str()).collect();
        self.calls
            .borrow_mut()
            .push(format!("edit-tags:{}", ids.join(",")));
    }
    fn request_remove_tag(
        &self,
        detail: &ArchiveDetail,
        tag_id: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        self.calls
            .borrow_mut()
            .push(format!("untag:{}:{tag_id}", detail.id));
    }
    fn request_move_to_group(
        &self,
        resource_ids: &[String],
        folder_id: Option<&str>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        self.calls.borrow_mut().push(format!(
            "move-to-group:{}:{}",
            resource_ids.join(","),
            folder_id.unwrap_or("__ungrouped__")
        ));
    }
    fn request_create_group(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("create-group".to_string());
    }
    fn request_rename(&self, resource_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("rename:{resource_id}"));
    }
    fn request_rename_group(&self, folder_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("rename-group:{folder_id}"));
    }
    fn request_delete_group(&self, folder_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("delete-group:{folder_id}"));
    }
    fn request_undo_archive(&self, undo: &ArchiveUndo, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("undo:{}", undo.resource_id));
    }
    fn request_index_repair(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("repair".to_string());
    }
    fn request_open_payload_dir(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("open-dir".to_string());
    }
    fn request_open_trash(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("trash".to_string());
    }
    fn request_refresh(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("refresh".to_string());
    }
    fn remember_sort(&self, field: SortField, order: SortOrder, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("sort:{}:{}", field.key(), order.arrow()));
    }
    fn remember_collapsed(&self, keys: &[String], _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("collapsed:{}", keys.join(",")));
    }
    fn remember_default_group(&self, folder_id: Option<&str>, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("default-group:{}", folder_id.unwrap_or("__none__")));
    }
}

fn row(id: &str, kind: ArchiveKind, status: ArchiveStatus, version: i32) -> ArchiveRow {
    ArchiveRow {
        id: id.to_string(),
        name: format!("{id}.sql"),
        alias: None,
        kind,
        version,
        status,
        tail: "1.2 KB · 3 天前".to_string(),
        source_table: None,
        tags: Vec::new(),
        folder_id: None,
        updated_epoch: 1_700_000_000,
        archived_epoch: Some(1_700_000_000),
        size_bytes: Some(1_228),
    }
}

fn snapshot(rows: Vec<ArchiveRow>, read_only: bool) -> ResourcesSnapshot {
    let mut counts = ArchiveCounts {
        total: rows.len(),
        ..ArchiveCounts::default()
    };
    for item in &rows {
        match (item.status, item.kind) {
            (ArchiveStatus::Missing, _) => counts.missing += 1,
            (_, ArchiveKind::Analysis) => counts.analysis += 1,
            (_, ArchiveKind::TableRef) => counts.table_ref += 1,
            _ => counts.archived += 1,
        }
    }
    // 详情与行同一次取数产出（生产入口就是 `build_snapshot`）：打开 / 取回都要靠它，
    // 窗口用例里也照这个形状造，否则“动作拿得到本体路径”这条链在测试里是断的。
    let details = rows
        .iter()
        .map(|row| (row.id.clone(), detail_for(row)))
        .collect();
    ResourcesSnapshot {
        rows,
        counts,
        read_only,
        details,
        tags: Vec::new(),
        groups: Vec::new(),
    }
}

/// 一行 → 详情（窗口用例只关心动作要用的那几个字段）。
fn detail_for(row: &ArchiveRow) -> ArchiveDetail {
    ArchiveDetail {
        id: row.id.clone(),
        name: row.name.clone(),
        alias: None,
        kind: row.kind,
        version: row.version,
        status: row.status,
        readonly: true,
        size_label: "1.2 KB".to_string(),
        modified_label: "2026-09-17 08:00".to_string(),
        archived_label: "2026-09-17 08:00".to_string(),
        promoted_from: None,
        source_connection_id: None,
        source_table: None,
        content_hash: Some("0123456789abcdef0123".to_string()),
        payload_rel_path: Some(format!("{}.sql", row.id)),
        history_label: String::new(),
        tags: Vec::new(),
        group: None,
        // 预览：窗口用例只关心“能不能摆”，给一段两行的文本就好。
        preview: Preview::Text {
            lines: vec!["select 1".to_string(), "from dual".to_string()],
            truncated: false,
        },
    }
}

#[gpui_kit::test]
fn renders_empty_state_and_read_only_notice(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    // 空态渲染一帧：不应 panic（无行、计数全零）。
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    // 只读提示 + 再次渲染（提示行与禁用按钮都不应 panic）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_notice(Some("项目以只读方式打开".to_string()), cx);
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    let rows = cx.update(|_window, cx| panel.read(cx).snapshot().rows.len());
    assert_eq!(rows, 0);
    assert!(host.calls().is_empty(), "仅渲染不应触发任何宿主动作");
}

#[gpui_kit::test]
fn loading_state_shows_skeleton_instead_of_empty_state(cx: &mut TestAppContext) {
    // 取数期间不能先摆空态：那会给用户看一眼"还没有任何存档"，而其实只是还没读到（原型 §5）。
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_loading(true, cx));
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(cx.debug_bounds("archive-loading").is_some(), "取数中给骨架");
    assert!(
        cx.debug_bounds("archive-empty-action").is_none(),
        "取数中不得摆空态按钮"
    );

    // 快照到达（哪怕是空库）：骨架退场、空态登场。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(snapshot(Vec::new(), false), cx);
            panel.set_loading(false, cx);
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(cx.debug_bounds("archive-loading").is_none());
    assert!(cx.debug_bounds("archive-empty-action").is_some());
}

#[gpui_kit::test]
fn undo_bar_appears_with_the_token_and_leaves_when_cleared(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    // 没凭据：撤销栏不存在（不给一个点不动的入口占地方）。
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(cx.debug_bounds("archive-undo").is_none());

    // 宿主推送凭据（生产入口：归档成功之后）→ 栏出现（含"撤销"入口）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_undo(
                Some(ArchiveUndo {
                    resource_id: "ar_1".to_string(),
                    name: "dau_report".to_string(),
                    source_path: std::path::PathBuf::from("D:\\work\\dau.sql"),
                }),
                cx,
            );
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-undo").is_some(),
        "有凭据时撤销栏要在（固定在状态行上方）"
    );
    assert!(host.calls().is_empty(), "仅是渲染栏子不该发起任何动作");

    // 撤销完成后宿主清掉凭据 → 栏退场。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_undo(None, cx));
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(cx.debug_bounds("archive-undo").is_none());
}

#[gpui_kit::test]
fn snapshot_push_drives_rows_selection_and_dangling_cleanup(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3),
                        row("ar_2", ArchiveKind::Analysis, ArchiveStatus::Normal, 1),
                        row("ar_3", ArchiveKind::TableRef, ArchiveStatus::Normal, 1),
                        row("ar_4", ArchiveKind::File, ArchiveStatus::Missing, 2),
                    ],
                    false,
                ),
                cx,
            );
            // 宿主驱动选中（生产入口）。
            panel.set_selected(Some("ar_2".to_string()), cx);
        });
    });

    // 行列表 + 选中行级操作按钮 + 异常态"修复…"入口一起渲染，不应 panic。
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    let (counts, selected) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (
            panel.snapshot().counts,
            panel.selected_id().map(str::to_string),
        )
    });
    assert_eq!(counts.total, 4);
    assert_eq!(counts.archived, 1);
    assert_eq!(counts.analysis, 1);
    assert_eq!(counts.table_ref, 1);
    assert_eq!(counts.missing, 1);
    assert!(counts.has_issues(), "缺失应给出修复入口");
    assert_eq!(selected, Some("ar_2".to_string()));

    // 该行被删除 / 被筛掉：选中必须清掉，否则详情面板会指向不存在的东西。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3)],
                    false,
                ),
                cx,
            );
        });
    });
    let selected = cx.update(|_window, cx| panel.read(cx).selected_id().map(str::to_string));
    assert_eq!(selected, None, "悬空选中应被清除");
}

#[gpui_kit::test]
fn toolbar_conditions_narrow_rows_and_clear_dangling_selection(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3),
                        row("ar_2", ArchiveKind::Analysis, ArchiveStatus::Normal, 1),
                        row("ar_3", ArchiveKind::File, ArchiveStatus::Missing, 2),
                    ],
                    false,
                ),
                cx,
            );
            panel.set_selected(Some("ar_2".to_string()), cx);
        });
    });

    // 搜索把选中的行挡在条件外：选中必须清掉（详情面板不能指向看不见的行）。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.set_query("ar_1", window, cx));
    });
    let (ids, selected, filter_is_empty) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (
            panel
                .view_rows()
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            panel.selected_id().map(str::to_string),
            panel.filter().is_empty(),
        )
    });
    assert_eq!(ids, vec!["ar_1".to_string()]);
    assert_eq!(selected, None, "被筛掉的选中应被清除");
    assert!(!filter_is_empty, "有关键字就是筛选态：空态文案据此分流");

    // 「只看需处理」：只剩缺失那行。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.clear_filter(window, cx);
            panel.toggle_only_issues(cx);
        });
    });
    let ids = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(ids, vec!["ar_3".to_string()]);

    // 种类筛选：只留分析表（先关掉上一个条件，避免两个条件互相解释不清）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.toggle_only_issues(cx);
            panel.toggle_kind(ArchiveKind::Analysis, cx);
        });
    });
    let ids = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(ids, vec!["ar_2".to_string()]);

    // 筛选是纯视图行为：不应触发任何宿主动作。
    assert!(host.calls().is_empty());
}

#[gpui_kit::test]
fn host_selection_mirrors_into_list_without_reentry(cx: &mut TestAppContext) {
    // 背景：面板在渲染期把选中镜像回列表，而 `ListState::set_selected_index` 会回调委托，
    // 委托再写回面板就是"更新正在被更新的实体"——这条用例就是钉住那个 panic。
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3),
                        row("ar_2", ArchiveKind::Analysis, ArchiveStatus::Normal, 1),
                        row("ar_3", ArchiveKind::File, ArchiveStatus::Missing, 2),
                    ],
                    false,
                ),
                cx,
            );
        });
    });
    // 建列表的那一帧。
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    // 宿主来回改选中（每次之后都渲染一帧：镜像发生在渲染期）。
    for id in [Some("ar_2"), Some("ar_1"), Some("ar_3"), None] {
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.set_selected(id.map(str::to_string), cx);
            });
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let selected = cx.update(|_window, cx| panel.read(cx).selected_id().map(str::to_string));
    assert_eq!(selected, None);
    assert!(host.calls().is_empty(), "仅渲染与选中不应触发任何宿主动作");
}

#[gpui_kit::test]
fn multi_selection_and_select_all_reach_the_host_on_delete(cx: &mut TestAppContext) {
    // 多选批的第一条**真链路**：手势 → 选择集 → `Ctrl+A` → `Delete` → 整批到宿主。
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1),
                        row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 2),
                        row("ar_3", ArchiveKind::File, ArchiveStatus::Normal, 3),
                    ],
                    false,
                ),
                cx,
            );
        });
    });

    // 单击 → 单选；Ctrl 点击 → 加一条（按可见行顺序）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.handle_row_click("ar_2", RowClick::Select, cx);
            panel.handle_row_click("ar_1", RowClick::Toggle, cx);
        });
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_ids()),
        vec!["ar_1", "ar_2"]
    );

    // `Ctrl+A` 全选（生产入口：app 层的键绑定就是派发它）。
    cx.update(|window, cx| {
        let handle = panel.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(SelectAllRows), cx);
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_ids()),
        vec!["ar_1", "ar_2", "ar_3"]
    );

    // `Delete`：整批交给宿主（真删除 = 回收站，见 P0.8）。
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(DeleteSelected), cx);
    });
    assert_eq!(host.calls(), vec!["delete:ar_1,ar_2,ar_3"]);
    assert!(
        !host.calls().iter().any(|call| call.starts_with("open:")),
        "选中不是打开：单击只改选中（打开在双击 / Enter）"
    );
}

#[gpui_kit::test]
fn open_selected_action_hands_the_host_the_row_detail(cx: &mut TestAppContext) {
    // 走生产入口：聚焦面板 → 派发 `OpenSelected`（右键菜单「打开（只读）」与详情面板按钮同一条道）。
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1),
                        row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 2),
                    ],
                    false,
                ),
                cx,
            );
            panel.set_selected(Some("ar_2".to_string()), cx);
        });
    });
    cx.update(|window, cx| {
        let handle = panel.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(OpenSelected), cx);
    });

    assert_eq!(
        host.calls(),
        vec!["open:ar_2"],
        "打开要拿的是**选中那条的详情**（宿主据此解析本体路径）"
    );
}

#[gpui_kit::test]
fn clear_search_action_clears_query_only(cx: &mut TestAppContext) {
    // 走生产入口：聚焦面板 → 派发 `ClearSearch`（app 层的 `Esc` 就是派发它）。
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3),
                        row("ar_2", ArchiveKind::Analysis, ArchiveStatus::Normal, 1),
                    ],
                    false,
                ),
                cx,
            );
            panel.toggle_only_issues(cx);
        });
    });
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.set_query("zzz", window, cx));
    });

    cx.update(|window, cx| {
        let handle = panel.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(ClearSearch), cx);
    });

    let (query, kinds_only_issues) = cx.update(|_window, cx| {
        let filter = panel.read(cx).filter().clone();
        (filter.query.clone(), filter.only_issues)
    });
    assert_eq!(query, "", "`Esc` 应清掉搜索词");
    assert!(kinds_only_issues, "菜单里的条件不应被 `Esc` 一并清掉");
}

/// `F2` → 宿主开改名对话框；多选与只读项目不发（与菜单里那项同一判据）。
#[gpui_kit::test]
fn f2_renames_the_focused_row_only(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let press_f2 = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let handle = panel.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.update(|window, cx| {
            window.dispatch_action(Box::new(RenameSelected), cx);
        });
    };

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1),
                        row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 1),
                    ],
                    false,
                ),
                cx,
            );
            panel.set_selected(Some("ar_2".to_string()), cx);
        });
    });

    // 单选：落到焦点行上。
    press_f2(cx);
    assert_eq!(host.calls(), vec!["rename:ar_2".to_string()]);

    // 多选：改了哪一条都是猜（与菜单项置灰同一判据）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.select_all(cx));
    });
    press_f2(cx);
    assert_eq!(host.calls().len(), 1, "多选时不给改");

    // 只读项目：写不进去，也就没必要开对话框。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1)],
                    true,
                ),
                cx,
            );
            panel.set_selected(Some("ar_1".to_string()), cx);
        });
    });
    press_f2(cx);
    assert_eq!(host.calls().len(), 1, "只读项目不给改");
}

#[gpui_kit::test]
fn sort_click_flips_direction_and_keeps_it_across_fields(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 2),
                        row("ar_1", ArchiveKind::Analysis, ArchiveStatus::Normal, 1),
                        row("ar_3", ArchiveKind::File, ArchiveStatus::Normal, 8),
                    ],
                    false,
                ),
                cx,
            );
        });
    });

    // 默认：名称升序。
    let order = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        order,
        vec!["ar_1".to_string(), "ar_2".to_string(), "ar_3".to_string()]
    );

    // 同一字段再点一次 = 翻转方向。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.choose_sort(SortField::Name, cx));
    });
    let (order, sort) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (
            panel
                .view_rows()
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            panel.sort(),
        )
    });
    assert_eq!(
        order,
        vec!["ar_3".to_string(), "ar_2".to_string(), "ar_1".to_string()]
    );
    assert_eq!(sort, (SortField::Name, SortOrder::Desc));

    // 换字段保持方向：版本降序，再点同字段翻成升序。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.choose_sort(SortField::Version, cx));
    });
    let versions = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.version)
            .collect::<Vec<_>>()
    });
    assert_eq!(versions, vec![8, 2, 1], "换字段沿用当前方向（降序）");

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.choose_sort(SortField::Version, cx));
    });
    let versions = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.version)
            .collect::<Vec<_>>()
    });
    assert_eq!(versions, vec![1, 2, 8]);
    // 排序是“记住上次”的入口：每次都把（字段 + 方向）告诉宿主（宿主写 `resources.default_sort`），
    // 但**不碰**取数（没有任何 refresh / 服务调用）。
    assert_eq!(
        host.calls(),
        vec![
            "sort:name:↓".to_string(),
            "sort:version:↓".to_string(),
            "sort:version:↑".to_string(),
        ]
    );
}

#[gpui_kit::test]
fn sort_by_time_and_size_uses_raw_values(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    // 两条行的名称与时间 / 体积故意反着排：排序若落到名称兜底上就会被看出来。
    let mut big = row("ar_big", ArchiveKind::File, ArchiveStatus::Normal, 1);
    big.name = "a_big.sql".to_string();
    big.updated_epoch = 300;
    big.archived_epoch = Some(30);
    big.size_bytes = Some(1_228_800);

    let mut small = row("ar_small", ArchiveKind::File, ArchiveStatus::Normal, 1);
    small.name = "z_small.sql".to_string();
    small.updated_epoch = 100;
    small.archived_epoch = Some(10);
    small.size_bytes = Some(900);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(snapshot(vec![big, small], false), cx);
        });
    });

    // 默认名称升序：a_big 在前（作为后面“确实换了口径”的对照）。
    let order = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["ar_big".to_string(), "ar_small".to_string()]);

    // 更新时间升序：时间早的在前（与名称顺序相反）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.choose_sort(SortField::UpdatedAt, cx));
    });
    let order = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["ar_small".to_string(), "ar_big".to_string()]);

    // 换到大小（沿用升序）：900 B 在 1.2 KB 之前——拿尾巴字符串比就会反过来。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.choose_sort(SortField::Size, cx));
    });
    let order = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["ar_small".to_string(), "ar_big".to_string()]);

    // 再点一次翻转：大的在前。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.choose_sort(SortField::Size, cx));
    });
    let order = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["ar_big".to_string(), "ar_small".to_string()]);
    assert_eq!(
        host.calls(),
        vec![
            "sort:updated_at:↑".to_string(),
            "sort:size:↑".to_string(),
            "sort:size:↓".to_string(),
        ],
        "排序只惊动“记住排序”这一个口，不触发取数"
    );
}

#[gpui_kit::test]
fn injected_default_sort_reorders_without_echoing_back(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let mut big = row("ar_big", ArchiveKind::File, ArchiveStatus::Normal, 1);
    big.size_bytes = Some(4_096);
    let mut small = row("ar_small", ArchiveKind::File, ArchiveStatus::Normal, 1);
    small.size_bytes = Some(900);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(snapshot(vec![big, small], false), cx);
        });
    });

    // 宿主注入设置项里的默认排序（大小降序）：行重排，但**不回写宿主**——
    // 否则注入的默认值会被当成用户动作原样写回去。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_sort(SortField::Size, SortOrder::Desc, cx)
        });
    });
    let order = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["ar_big".to_string(), "ar_small".to_string()]);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.sort()),
        (SortField::Size, SortOrder::Desc)
    );
    assert!(
        host.calls().is_empty(),
        "注入默认值是宿主的动作，不是用户动作：不写回、不取数"
    );
}

#[gpui_kit::test]
fn no_match_state_renders_and_clear_filter_restores_rows(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![
                        row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3),
                        row("ar_2", ArchiveKind::Analysis, ArchiveStatus::Normal, 1),
                    ],
                    false,
                ),
                cx,
            );
        });
    });

    // 条件命中 0 行：走「没有匹配」那一支（不是空库态），渲染一帧不应 panic。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.set_query("zzz", window, cx));
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let (rows, filter_is_empty) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (panel.view_rows().len(), panel.filter().is_empty())
    });
    assert_eq!(rows, 0);
    assert!(!filter_is_empty);

    // 清空筛选：行回来，且输入框里的字一并清掉（`set_value` 不发 Change，
    // 只能由 `clear_filter` 自己同步——两边不一致是最难排查的那种 bug）。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.clear_filter(window, cx));
    });
    let (rows, text, filter_is_empty) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (
            panel.view_rows().len(),
            panel
                .search_input()
                .expect("渲染一帧后搜索框应已创建")
                .read(cx)
                .value()
                .to_string(),
            panel.filter().is_empty(),
        )
    });
    assert_eq!(rows, 2);
    assert_eq!(text, "");
    assert!(filter_is_empty);

    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(host.calls().is_empty());
}

#[gpui_kit::test]
fn header_more_button_sits_in_the_header_and_dispatches_host_actions(cx: &mut TestAppContext) {
    // 原型 §2.1：面板头右侧是 `＋ ▾` 与 `⋯` 两个按钮。菜单里的项在窗口测试里点不到
    // （弹层），所以这里钉住两件事：**按钮真在**、**四个动作各自到得了宿主**。
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (_panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-more").is_some(),
        "`⋯` 按钮要在面板头里"
    );

    // 四个动作依次派发（顺序就是菜单顺序）：每个都要原样落到宿主端口。
    let expected = [
        (HeaderMenuAction::RebuildIndex, "repair"),
        (HeaderMenuAction::OpenPayloadDir, "open-dir"),
        (HeaderMenuAction::OpenTrash, "trash"),
        (HeaderMenuAction::Refresh, "refresh"),
    ];
    cx.update(|window, cx| {
        let dyn_host: Rc<dyn ResourcesHost> = host.clone();
        for (action, _) in expected {
            dispatch_header_action(&dyn_host, action, window, cx);
        }
    });
    let calls = host.calls();
    assert_eq!(
        calls,
        expected
            .iter()
            .map(|(_, call)| call.to_string())
            .collect::<Vec<_>>()
    );
}

/// 分组折叠区（原型 §2.4）：分区渲染 + 折叠只影响行的出场。
#[gpui_kit::test]
fn group_section_headers_render_and_collapsing_hides_rows(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let mut grouped = row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1);
    grouped.folder_id = Some("af_1".to_string());
    let loose = row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 1);
    let mut snapshot = snapshot(vec![grouped, loose], false);
    snapshot.groups = vec![GroupOption {
        id: "af_1".to_string(),
        name: "月报".to_string(),
    }];
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_snapshot(snapshot, cx));
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });

    assert!(cx.debug_bounds("archive-group-__all__").is_some());
    assert!(cx.debug_bounds("archive-group-__ungrouped__").is_some());
    assert!(cx.debug_bounds("archive-group-af_1").is_some());

    // 行距 = `ui::ROW_HEIGHT`（24px）：`List` 的行距是**单一值**（只量一个样本行），所以
    // 分组头与存档行必须同高；`ListItem` 自带的 `py_1` 已在 `list_row` 里压掉，
    // 否则每个条目都会高 8px（多选行的选中底也会上下露出 hover 光晕）。
    // 这三项之间隔了三个条目（头 / 头 / 一条行）。
    let first = cx.debug_bounds("archive-group-__all__").expect("顶部头在");
    let month = cx.debug_bounds("archive-group-af_1").expect("分组头在");
    assert_eq!(
        month.origin.y - first.origin.y,
        px(72.),
        "三个条目应正好 3 × 24px（行高口径：`ui::ROW_HEIGHT`）"
    );

    // 折叠「月报」：它的行从列表里消失，但行集合（`view_rows`）不变——
    // 折叠是呈现层的事，不能把行从筛选结果里删掉（否则选中 / 多选会被误清）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_group_collapse("af_1", cx));
    });
    assert_eq!(cx.update(|_window, cx| panel.read(cx).view_rows().len()), 2);
    let items = cx.update(|_window, cx| panel.read(cx).view_items().len());
    assert_eq!(items, 4, "三个头 + 未分组那一行（月报那一行被折掉）");
    assert!(
        cx.debug_bounds("archive-group-af_1").is_some(),
        "头还在（否则展不开）"
    );

    // 再展开回去。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_group_collapse("af_1", cx));
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).view_items().len()),
        5
    );

    // 折叠态不是纯视图状态：每次切换都把**当前全集**交给宿主（按项目分桶写设置；
    // 空集也要交一次——“一条都不折”同样是用户的现状，不交就等于永远恢复不了）。
    assert_eq!(
        host.calls(),
        vec!["collapsed:af_1".to_string(), "collapsed:".to_string()]
    );
}

/// 注入折叠态（设置里读回来的）：行当场就是折的，且**不回写宿主**（注入不是用户动作）。
#[gpui_kit::test]
fn injected_collapsed_state_folds_rows_without_echoing_back(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let mut grouped = row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1);
    grouped.folder_id = Some("af_1".to_string());
    let mut snapshot = snapshot(vec![grouped], false);
    snapshot.groups = vec![GroupOption {
        id: "af_1".to_string(),
        name: "月报".to_string(),
    }];
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_snapshot(snapshot, cx));
    });

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_collapsed(&["af_1".to_string()], cx)
        });
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).view_items().len()),
        3,
        "三个头，月报那一行被折掉"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).collapsed_keys()),
        vec!["af_1".to_string()]
    );
    assert!(
        host.calls().is_empty(),
        "注入折叠态是宿主的动作，不是用户动作：不写回"
    );
}

/// 搜索匹配面比展示面宽（原型 §2.2）：面板上搜「别名 / 标签名 / 来源表」也能录到行，
/// 哪怕这三个字段在行上根本不显示。
#[gpui_kit::test]
fn search_matches_alias_tag_name_and_source_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let mut aliased = row("ar_1", ArchiveKind::Analysis, ArchiveStatus::Normal, 1);
    aliased.alias = Some("月报".to_string());
    let mut tagged = row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 1);
    tagged.tags = vec![ArchiveTagChip {
        id: "at_1".to_string(),
        name: "财务报表".to_string(),
    }];
    let mut from_table = row("ar_3", ArchiveKind::TableRef, ArchiveStatus::Normal, 1);
    from_table.source_table = Some("dwd.dwd_orders".to_string());
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(snapshot(vec![aliased, tagged, from_table], false), cx);
        });
    });

    let hit = |cx: &mut VisualTestContext, query: &str| {
        cx.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.clear_filter(window, cx);
                panel.set_query(query, window, cx);
            });
        });
        cx.update(|_window, cx| {
            panel
                .read(cx)
                .view_rows()
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        })
    };

    assert_eq!(hit(cx, "月报"), vec!["ar_1".to_string()]);
    assert_eq!(hit(cx, "财务报表"), vec!["ar_2".to_string()]);
    assert_eq!(hit(cx, "dwd.dwd_orders"), vec!["ar_3".to_string()]);
    // 匹配面是宽出来的，不是换掉的：名称照旧命中。
    assert_eq!(hit(cx, "ar_3"), vec!["ar_3".to_string()]);
}

/// 默认分组：注入后头上出「默认」标记；右键切一次就把新值交给宿主（取消时给 `None`）。
#[gpui_kit::test]
fn default_group_marker_follows_the_setting_and_reports_changes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let mut initial = snapshot(
        vec![row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1)],
        false,
    );
    initial.groups = vec![
        GroupOption {
            id: "af_1".to_string(),
            name: "报表".to_string(),
        },
        GroupOption {
            id: "af_2".to_string(),
            name: "临时".to_string(),
        },
    ];
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(initial, cx);
            // 注入是宿主的动作（与默认排序 / 折叠态同一口径）：不回写设置。
            panel.set_default_group(Some("af_1".to_string()), cx);
        });
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-group-default-af_1").is_some(),
        "默认分组那一行要有标记"
    );
    assert!(
        cx.debug_bounds("archive-group-default-af_2").is_none(),
        "别的分组不该有标记"
    );
    assert!(host.calls().is_empty(), "注入不是用户动作：不写回");

    // 换一个：标记跟着走，宿主拿到新值。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_default_group("af_2", cx));
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert_eq!(host.calls(), vec!["default-group:af_2".to_string()]);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).default_group().map(str::to_string)),
        Some("af_2".to_string())
    );
    assert!(cx.debug_bounds("archive-group-default-af_1").is_none());
    assert!(cx.debug_bounds("archive-group-default-af_2").is_some());

    // 再点同一个：取消（设置里也不留空壳）。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_default_group("af_2", cx));
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert_eq!(
        host.calls(),
        vec![
            "default-group:af_2".to_string(),
            "default-group:__none__".to_string()
        ]
    );
    assert!(
        cx.debug_bounds("archive-group-default-af_2").is_none(),
        "取消后不再有标记"
    );
}

/// 拖到分组头 = 移动（原型 §3.2）：走的仍是宿主的 `request_move_to_group`，且**只发真的要改的行**。
#[gpui_kit::test]
fn drop_rows_onto_group_sends_only_rows_that_change(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let loose = row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1);
    let mut in_report = row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 1);
    in_report.folder_id = Some("af_1".to_string());
    let mut in_temp = row("ar_3", ArchiveKind::File, ArchiveStatus::Normal, 1);
    in_temp.folder_id = Some("af_2".to_string());

    let mut initial = snapshot(vec![loose.clone(), in_report, in_temp], false);
    initial.groups = vec![
        GroupOption {
            id: "af_1".to_string(),
            name: "报表".to_string(),
        },
        GroupOption {
            id: "af_2".to_string(),
            name: "临时".to_string(),
        },
    ];
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_snapshot(initial, cx));
    });

    let ids = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();

    // 拖到「报表」：ar_2 已在里面，只有 ar_1 要发。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.drop_rows_onto_group(
                &ids(&["ar_1", "ar_2"]),
                GroupDrop::Into("af_1".to_string()),
                window,
                cx,
            );
        });
    });
    assert_eq!(host.calls(), vec!["move-to-group:ar_1:af_1".to_string()]);

    // 拖到「未分组」：已在分组里的两条要发，ar_1 本就是未分组。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.drop_rows_onto_group(
                &ids(&["ar_1", "ar_2", "ar_3"]),
                GroupDrop::Ungroup,
                window,
                cx,
            );
        });
    });
    assert_eq!(
        host.calls(),
        vec![
            "move-to-group:ar_1:af_1".to_string(),
            "move-to-group:ar_2,ar_3:__ungrouped__".to_string(),
        ]
    );
}

/// 两个「不该发生」的落点：聚合头不接，只读项目不发（拖起来也是白拖）。
#[gpui_kit::test]
fn drop_on_aggregate_header_and_read_only_projects_do_nothing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let ids = vec!["ar_1".to_string()];
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1)],
                    false,
                ),
                cx,
            )
        });
    });
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.drop_rows_onto_group(&ids, GroupDrop::NotATarget, window, cx);
        });
    });
    assert!(host.calls().is_empty(), "「全部分组」不是落点");

    // 只读项目：同一落点、同一批行，仍不发。
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_snapshot(
                snapshot(
                    vec![row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1)],
                    true,
                ),
                cx,
            )
        });
    });
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.drop_rows_onto_group(&ids, GroupDrop::Ungroup, window, cx);
        });
    });
    assert!(host.calls().is_empty(), "只读项目不发写入请求");
}

/// 标签筛选维：勾上就窄，标签被删后条件自动抹掉（否则列表“什么都没匹配”而勾还在）。
#[gpui_kit::test]
fn tag_filter_narrows_rows_and_drops_stale_conditions(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let (panel, cx) = cx.add_window_view({
        let host = host.clone();
        move |_window, cx| ResourcesPanel::new(host.clone(), cx)
    });

    let mut tagged = row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1);
    tagged.tags = vec![ArchiveTagChip {
        id: "at_1".to_string(),
        name: "报表".to_string(),
    }];
    let other = row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 2);
    let mut initial = snapshot(vec![tagged, other.clone()], false);
    initial.tags = vec![TagOption {
        id: "at_1".to_string(),
        name: "报表".to_string(),
        count: 1,
    }];
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_snapshot(initial, cx));
    });
    assert_eq!(cx.update(|_window, cx| panel.read(cx).view_rows().len()), 2);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.toggle_tag("at_1", cx));
    });
    let visible = cx.update(|_window, cx| {
        panel
            .read(cx)
            .view_rows()
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(visible, vec!["ar_1"]);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).filter().menu_dims()),
        1
    );

    // 标签被删（词典里没了）：新快照一到，悬空条件自己消失，列表回到两行。
    let mut without_tag = snapshot(vec![other], false);
    without_tag.tags = Vec::new();
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_snapshot(without_tag, cx));
    });
    assert!(cx.update(|_window, cx| panel.read(cx).filter().is_empty()));
    assert_eq!(cx.update(|_window, cx| panel.read(cx).view_rows().len()), 1);
}

/// 详情面板的标签壳：详情视图不自持实体，窗口测试给它一个壳。
struct DetailHarness {
    detail: ArchiveDetail,
    actions: Option<DetailActions>,
}

impl Render for DetailHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(render_detail(&self.detail, self.actions.clone(), cx))
    }
}

/// 详情面板的标签区：chips 与「×」在写态下都出场；只读项目下只留 chips。
/// 内容预览（原型 §3.1）：文本型摆前几行的等宽块，仅元信息型只摆一句说明。
#[gpui_kit::test]
fn detail_preview_block_renders_text_or_says_why_not(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());

    // 文本型：等宽块出场（`archive-detail-preview` 是那块本身的 id）。
    let (with_text, cx) = cx.add_window_view({
        let host = host.clone();
        let mut detail = detail_for(&row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1));
        detail.preview = Preview::Text {
            lines: vec!["select 1".to_string(), "from dual".to_string()],
            truncated: true,
        };
        move |_window, _cx| DetailHarness {
            detail: detail.clone(),
            actions: Some(DetailActions {
                host: host.clone(),
                read_only: false,
            }),
        }
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-detail-preview").is_some(),
        "文本型应有预览块"
    );
    drop(with_text);

    // 仅元信息型（二进制 / 大文件 / 无预览）：不摆块，只给一句说明。
    let (meta_only, cx) = cx.add_window_view({
        let host = host.clone();
        let mut detail = detail_for(&row("ar_2", ArchiveKind::File, ArchiveStatus::Normal, 1));
        detail.preview = Preview::OnlyMeta("仅元信息（不预览内容）");
        move |_window, _cx| DetailHarness {
            detail: detail.clone(),
            actions: Some(DetailActions {
                host: host.clone(),
                read_only: false,
            }),
        }
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-detail-preview").is_none(),
        "仅元信息时不该摆空块"
    );
    drop(meta_only);
}

#[gpui_kit::test]
fn detail_tag_chips_render_with_actions_only_when_writable(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let host = Rc::new(RecordingHost::default());
    let mut detail = detail_for(&row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 1));
    detail.tags = vec![ArchiveTagChip {
        id: "at_1".to_string(),
        name: "报表".to_string(),
    }];
    let (writable, cx) = cx.add_window_view({
        let host = host.clone();
        let detail = detail.clone();
        move |_window, _cx| DetailHarness {
            detail: detail.clone(),
            actions: Some(DetailActions {
                host: host.clone(),
                read_only: false,
            }),
        }
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-detail-add-tag").is_some(),
        "有宿主时摆「＋ 标签」"
    );
    assert!(
        cx.debug_bounds("archive-tag-remove-at_1").is_some(),
        "chip 上的 × 要用 id 认出具体是哪个标签"
    );

    // 只读项目：× 不出场（＋ 标签 仍在，但置灰——置灰态靠 disabled 保证，这里只验“不摆 ×”）。
    let (read_only, cx) = cx.add_window_view({
        let host = host.clone();
        let detail = detail.clone();
        move |_window, _cx| DetailHarness {
            detail: detail.clone(),
            actions: Some(DetailActions {
                host: host.clone(),
                read_only: true,
            }),
        }
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    assert!(
        cx.debug_bounds("archive-tag-remove-at_1").is_none(),
        "只读项目下不能去标签"
    );
    assert!(cx.debug_bounds("archive-detail-add-tag").is_some());
    let _ = writable.read_with(cx, |_harness, _cx| ());
    let _ = read_only.read_with(cx, |_harness, _cx| ());
}
