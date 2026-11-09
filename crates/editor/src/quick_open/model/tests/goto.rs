//! Goto 档（`Ctrl+Shift+O`）：锁档与去列 / 排序 / 上限。

use crate::quick_open::model::{
    Action, MetaObject, Mode, Query, RowKind, Sources, build_groups, meta_object,
};

use super::hit;

/// Goto 档：只出元数据一组、列不上屏；主动作 = 回树定位，次级动作 = 属性面板。
#[test]
fn goto_mode_lists_only_metadata_and_drops_columns() {
    let objects: Vec<MetaObject> = [
        hit("table", "orders", None),
        hit("column", "order_id", Some("orders")),
        hit("view", "v_report", None),
        hit("routine", "fn_x", None),
    ]
    .iter()
    .filter_map(meta_object)
    .collect();
    let groups = build_groups(
        &Query {
            mode: Mode::Goto,
            needle: String::new(),
        },
        &Sources {
            meta: &objects,
            ..Default::default()
        },
    );
    assert_eq!(groups.len(), 1, "Goto 只出元数据一组");
    assert_eq!(
        groups[0].title,
        "跳转到对象（表 · 视图 · 例程 · 序列 · 触发器 · 模式）"
    );
    assert!(
        groups[0].rows.iter().all(|r| r.kind != RowKind::Column),
        "列是从属细节，不进 Goto 档"
    );
    for row in &groups[0].rows {
        match &row.action {
            Action::RevealMetadata(_) => {}
            other => panic!("Goto 主动作应是回树定位，实际 {other:?}"),
        }
        match &row.secondary_action {
            Some(Action::ShowProperties(_)) => {}
            other => panic!("Goto 次级应是属性面板，实际 {other:?}"),
        }
    }
}

/// Goto 排序：限定名 ignoreCase 字典序；折叠后并列按类别序兜底（表 → 视图 → 例程 → 序列）。
#[test]
fn goto_sort_ignorecase_dictionary_with_category_tiebreak() {
    let objects: Vec<MetaObject> = [
        hit("table", "zebra", None),
        hit("view", "Apple", None),
        hit("table", "apple", None),
        hit("sequence", "seq_x", None),
    ]
    .iter()
    .filter_map(meta_object)
    .collect();
    let groups = build_groups(
        &Query {
            mode: Mode::Goto,
            needle: String::new(),
        },
        &Sources {
            meta: &objects,
            ..Default::default()
        },
    );
    let titles: Vec<String> = groups[0].rows.iter().map(|r| r.title.clone()).collect();
    assert_eq!(
        titles,
        vec![
            "public.apple",
            "public.Apple",
            "public.seq_x",
            "public.zebra"
        ],
        "折叠后 apple 与 Apple 并列，类别兜底表先于视图；其余按字典序"
    );
}

/// Goto 档渲染上限 1000：超出条数记在组尾 `hidden`，不静默丢。
#[test]
fn goto_caps_at_one_thousand_and_reports_hidden() {
    let objects: Vec<MetaObject> = (0..1001)
        .map(|i| meta_object(&hit("table", &format!("t_{i:04}"), None)))
        .flatten()
        .collect();
    let groups = build_groups(
        &Query {
            mode: Mode::Goto,
            needle: String::new(),
        },
        &Sources {
            meta: &objects,
            ..Default::default()
        },
    );
    assert_eq!(groups[0].rows.len(), 1000, "Goto 单组上限 1000 行");
    assert_eq!(groups[0].hidden, 1, "被截的 1 条要记下来（组尾提示）");
}
