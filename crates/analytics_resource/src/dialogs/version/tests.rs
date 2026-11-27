use super::{VersionAction, VersionDialogState, VersionRow};

fn row(version: i32, is_current: bool) -> VersionRow {
    VersionRow {
        version,
        is_current,
        time_label: "2026-09-17 08:00".to_string(),
        size_label: "1.2 KB".to_string(),
        hash_short: "0123456789ab".to_string(),
        has_copy: true,
        is_table: false,
        is_global_ref: false,
        delta_label: String::new(),
    }
}

#[test]
fn set_rows_clears_selection_that_no_longer_exists() {
    let state = VersionDialogState::new();
    state.set_rows(vec![row(2, true), row(1, false)]);
    state.set_selected(Some(1));

    // 动作后新行里没有 v1 了（被裁 / 列表变了）：选中必须清掉。
    state.set_rows(vec![row(3, true), row(2, false)]);
    assert_eq!(state.selected(), None);

    state.set_selected(Some(2));
    state.set_rows(vec![row(3, true), row(2, false)]);
    assert_eq!(state.selected(), Some(2), "还在的行保留选中");
}

#[test]
fn selected_row_follows_the_selection() {
    let state = VersionDialogState::new();
    state.set_rows(vec![row(2, true), row(1, false)]);
    assert!(state.selected_row().is_none(), "没选中就没有行");

    state.set_selected(Some(1));
    let selected = state.selected_row().expect("selected row");
    assert_eq!(selected.version, 1);
    assert!(!selected.is_current);
}

#[test]
fn action_carries_its_version() {
    assert_eq!(VersionAction::Restore { version: 3 }.version(), 3);
    assert_eq!(VersionAction::CheckoutDraft { version: 1 }.version(), 1);
    assert_eq!(
        VersionAction::Diff {
            version: 2,
            against: None
        }
        .version(),
        2
    );
    assert_eq!(VersionAction::Analyze { version: 4 }.version(), 4);
}

#[test]
fn global_scope_restore_ignores_project_read_only_but_project_scope_does_not() {
    use super::render::can_apply_version;

    let historical = row(1, false);
    assert!(
        can_apply_version(&historical, false, true, true),
        "全局实例的还原写全局库，项目只读不该挡"
    );
    assert!(
        !can_apply_version(&historical, false, true, false),
        "项目 scope 的还原仍是写操作，只读必须挡"
    );
    assert!(
        !can_apply_version(&row(2, true), false, false, true),
        "当前版本没有可还原的历史内容"
    );
    let mut pruned = row(1, false);
    pruned.has_copy = false;
    assert!(
        !can_apply_version(&pruned, false, false, true),
        "内容已裁除的版本不能还原"
    );
    assert!(
        !can_apply_version(&historical, true, false, true),
        "动作进行中要置灰，避免连点生成两个版本"
    );
}

#[test]
fn diff_base_toggles_and_clears_with_rows() {
    let state = VersionDialogState::new();
    state.set_rows(vec![row(3, true), row(2, false), row(1, false)]);

    assert_eq!(state.diff_base(), None);
    state.toggle_diff_base(2);
    assert_eq!(state.diff_base(), Some(2));
    assert_eq!(
        state.diff_base_row().expect("base row").version,
        2,
        "基准行要能拿到，动作栏才知道它是否可读"
    );
    state.toggle_diff_base(2);
    assert_eq!(state.diff_base(), None, "再 Shift+点同一行 = 取消");

    // 动作后换行：基准不在新列表里也要清掉，不能拿一个不存在的版本去 diff。
    state.toggle_diff_base(1);
    state.set_rows(vec![row(4, true), row(3, false)]);
    assert_eq!(state.diff_base(), None);
}
