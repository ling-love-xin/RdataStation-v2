use super::{VersionAction, VersionDialogState, VersionRow};

fn row(version: i32, is_current: bool) -> VersionRow {
    VersionRow {
        version,
        is_current,
        time_label: "2026-09-17 08:00".to_string(),
        size_label: "1.2 KB".to_string(),
        hash_short: "0123456789ab".to_string(),
        has_copy: true,
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
    assert_eq!(VersionAction::DeleteCopy { version: 7 }.version(), 7);
}
