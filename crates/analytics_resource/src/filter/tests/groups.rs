//! 分组折叠区的可见条目构造用例。

use crate::filter::{GROUP_ALL, GROUP_UNGROUPED, VisibleItem, build_visible_items};
use crate::model::{ArchiveKind, ArchiveStatus};
use crate::resource_view::{ArchiveRow, GroupOption};

use super::row;

fn row_in(id: &str, folder: Option<&str>) -> ArchiveRow {
    ArchiveRow {
        folder_id: folder.map(str::to_string),
        ..row(id, id, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
    }
}

fn group(id: &str, name: &str, count: usize) -> GroupOption {
    let _ = count;
    GroupOption {
        id: id.to_string(),
        name: name.to_string(),
    }
}

fn keys(items: &[VisibleItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            VisibleItem::GroupHeader { key, .. } => key.clone(),
            VisibleItem::Row(row) => row.id.clone(),
        })
        .collect()
}

/// 没有分组就不出头：空库与“全部未分组”不该多两行噪声。
#[test]
fn no_groups_means_no_headers() {
    let rows = vec![row_in("ar_1", None), row_in("ar_2", None)];
    let items = build_visible_items(&rows, &[], &Default::default());
    assert_eq!(keys(&items), vec!["ar_1", "ar_2"]);
}

/// 三层结构：全部分组 → 未分组 → 各分组；计数来自行集合，不另查库。
#[test]
fn groups_partition_rows_with_counts() {
    let rows = vec![
        row_in("ar_1", Some("af_1")),
        row_in("ar_2", None),
        row_in("ar_3", Some("af_1")),
        row_in("ar_4", Some("af_2")),
    ];
    let groups = vec![group("af_1", "月报", 0), group("af_2", "周报", 0)];
    let items = build_visible_items(&rows, &groups, &Default::default());
    assert_eq!(
        keys(&items),
        vec![
            GROUP_ALL,
            GROUP_UNGROUPED,
            "ar_2",
            "af_1",
            "ar_1",
            "ar_3",
            "af_2",
            "ar_4",
        ]
    );
    let total = items
        .iter()
        .find_map(|item| match item {
            VisibleItem::GroupHeader { key, count, .. } if key == GROUP_ALL => Some(*count),
            _ => None,
        })
        .expect("全部分组头");
    assert_eq!(total, 4, "“全部分组”数的是可见行总数");
}

/// 折叠：头还在（否则展不开），行不出场；折“全部分组”就只剩那一行。
#[test]
fn collapsing_hides_rows_but_keeps_headers() {
    let rows = vec![row_in("ar_1", Some("af_1")), row_in("ar_2", None)];
    let groups = vec![group("af_1", "月报", 1)];

    let collapsed: std::collections::HashSet<String> = ["af_1".to_string()].into_iter().collect();
    let items = build_visible_items(&rows, &groups, &collapsed);
    assert_eq!(
        keys(&items),
        vec![GROUP_ALL, GROUP_UNGROUPED, "ar_2", "af_1"],
        "折叠的分组只出头不出行"
    );

    let all_collapsed: std::collections::HashSet<String> =
        [GROUP_ALL.to_string()].into_iter().collect();
    let items = build_visible_items(&rows, &groups, &all_collapsed);
    assert_eq!(keys(&items), vec![GROUP_ALL], "折全部 = 只剩一行");
}

/// 计数按**当前可见行**现算：筛选后头里的数就是眼前的行数。
#[test]
fn header_counts_follow_the_visible_rows() {
    let groups = vec![group("af_1", "月报", 99)];
    let rows = vec![row_in("ar_1", Some("af_1")), row_in("ar_2", Some("af_1"))];
    let items = build_visible_items(&rows, &groups, &Default::default());
    let counts: Vec<usize> = items
        .iter()
        .filter_map(|item| match item {
            VisibleItem::GroupHeader { count, .. } => Some(*count),
            _ => None,
        })
        .collect();
    assert_eq!(counts, vec![2, 0, 2], "全部分组 / 未分组 / 月报");
}

/// 脏数据（分组被删而关联还在）：归属认不出就算未分组，不能把行吞掉。
#[test]
fn unknown_folders_fall_back_to_ungrouped() {
    let groups = vec![group("af_1", "月报", 0)];
    let rows = vec![
        row_in("ar_1", Some("af_gone")),
        row_in("ar_2", Some("af_1")),
    ];
    let items = build_visible_items(&rows, &groups, &Default::default());
    assert_eq!(
        keys(&items),
        vec![GROUP_ALL, GROUP_UNGROUPED, "ar_1", "af_1", "ar_2"],
        "认不出的分组归属算未分组（行不丢）"
    );
}
