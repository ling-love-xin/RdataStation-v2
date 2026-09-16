//! 资产库面板的窗口测试（GPUI headless）。
//!
//! 目的：把面板的**可交互行为**固化成回归——空态与只读提示可渲染、快照推送生效、
//! 选中可由宿主驱动、悬空选中被清掉、**工具栏条件真的改变可见行**。
//! 这些是后续改动最容易悄悄弄坏的地方。
//!
//! 两条纪律（都是踩过的坑）：
//! 1. **不通配导入**：`use gpui_kit::*` 会把 gpui 的 `test` 属性宏带进作用域，而
//!    `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己（recursion limit）；
//! 2. **实体访问要包在 `cx.update(|window, cx| …)` 里**：`VisualTestContext` 不实现
//!    `AppContext`，直接 `entity.update(&mut cx, …)` 不能编译。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{App, TestAppContext, Window};

use rds_analytics_resource::filter::{SortField, SortOrder};
use rds_analytics_resource::model::{ArchiveKind, ArchiveStatus};
use rds_analytics_resource::resource_view::{
    ArchiveCounts, ArchiveRow, ResourcesHost, ResourcesPanel, ResourcesSnapshot,
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
    fn request_archive(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("archive".to_string());
    }
    fn request_open(&self, resource_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push(format!("open:{resource_id}"));
    }
    fn request_checkout(&self, resource_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls
            .borrow_mut()
            .push(format!("checkout:{resource_id}"));
    }
    fn request_delete(&self, resource_id: &str, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push(format!("delete:{resource_id}"));
    }
    fn request_index_repair(&self, _window: &mut Window, _cx: &mut App) {
        self.calls.borrow_mut().push("repair".to_string());
    }
}

fn row(id: &str, kind: ArchiveKind, status: ArchiveStatus, version: i32) -> ArchiveRow {
    ArchiveRow {
        id: id.to_string(),
        name: format!("{id}.sql"),
        kind,
        version,
        status,
        tail: "1.2 KB · 3 天前".to_string(),
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
    ResourcesSnapshot {
        rows,
        counts,
        read_only,
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
        (panel.snapshot().counts, panel.selected_id().map(str::to_string))
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
                snapshot(vec![row("ar_1", ArchiveKind::File, ArchiveStatus::Normal, 3)], false),
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
    assert!(host.calls().is_empty());
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
