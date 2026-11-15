use super::{RepairAction, RepairDialogState, RepairGroup, RepairRow};

fn row(group: RepairGroup, title: &str) -> RepairRow {
    RepairRow {
        group,
        title: title.to_string(),
        detail: "resources/x.sql".to_string(),
        hash_detail: String::new(),
        rel_path: "x.sql".to_string(),
        resource_id: Some("ar_1".to_string()),
    }
}

#[test]
fn groups_follow_the_prototype_order() {
    let titles: Vec<&str> = RepairGroup::ALL.iter().map(|group| group.title()).collect();
    assert_eq!(
        titles,
        vec!["有文件、无记录", "有记录、无本体", "指纹不匹配"]
    );
    for group in RepairGroup::ALL {
        assert!(!group.hint().is_empty(), "每组都要有一句说明");
    }
}

/// 「有记录、无本体」的三条出路都要写在说明里：回收站 / 把文件放回去重扫 / 删记录。
/// 少说第三条，用户手上明明有那份文件也会以为只剩「删掉」。
#[test]
fn missing_group_hint_names_all_three_ways_out() {
    let hint = RepairGroup::Missing.hint();
    for word in ["回收站", "放回", "删记录"] {
        assert!(hint.contains(word), "说明里缺「{word}」：{hint}");
    }
}

#[test]
fn set_rows_replaces_wholesale_and_busy_is_independent() {
    let state = RepairDialogState::new();
    assert!(state.rows().is_empty());
    state.set_rows(vec![row(RepairGroup::Untracked, "a.sql")]);
    assert_eq!(state.rows().len(), 1);

    // 修完一项后宿主换一批（可能变空）：行集合整体替换，没有选中态要清理。
    state.set_rows(Vec::new());
    assert!(state.rows().is_empty());

    state.set_busy(true);
    assert!(state.busy());
    state.set_busy(false);
    assert!(!state.busy());
}

#[test]
fn actions_carry_what_the_host_needs() {
    // 三类动作各带走一个字段：补登带相对路径，另两个带存档 id。
    let adopt = RepairAction::Adopt {
        rel_path: "reports/a.sql".to_string(),
    };
    let RepairAction::Adopt { rel_path } = adopt else {
        unreachable!("刚构造的就是 Adopt")
    };
    assert_eq!(rel_path, "reports/a.sql");

    let accept = RepairAction::AcceptContent {
        resource_id: "ar_9".to_string(),
    };
    let RepairAction::AcceptContent { resource_id } = accept else {
        unreachable!("刚构造的就是 AcceptContent")
    };
    assert_eq!(resource_id, "ar_9");
}
