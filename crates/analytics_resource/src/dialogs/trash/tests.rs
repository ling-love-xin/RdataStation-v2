// 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀，`resource_view` 踩过）。
use super::{ForeignTrash, TrashDialogState, TrashRow};
use crate::model::PayloadMode;

fn row(id: &str) -> TrashRow {
    TrashRow {
        trash_id: id.to_string(),
        name: format!("dau_{id}.sql"),
        original_label: format!("resources/dau_{id}.sql"),
        kind_label: "文件".to_string(),
        time_label: "2026-09-18 10:00".to_string(),
        size_label: "1.2 KB".to_string(),
        payload_mode: PayloadMode::Copy,
        resource_id: String::new(),
        has_payload: true,
    }
}

/// 行数据可被宿主整体替换（动作完成后重取）：换行不保留旧的 id（那是指向已不在列表里的东西）。
#[test]
fn set_rows_replaces_the_previous_snapshot() {
    let state = TrashDialogState::new();
    state.set_rows(vec![row("a"), row("b")]);
    assert_eq!(state.rows().len(), 2);

    state.set_rows(vec![row("c")]);
    assert_eq!(state.rows().len(), 1);
    assert_eq!(state.rows()[0].trash_id, "c");
}

/// 别人的条目只挂说明，不混进行列表（混进来等于摆一个必然被拒的「还原」）。
#[test]
fn foreign_entries_are_kept_out_of_the_row_list() {
    let state = TrashDialogState::new();
    state.set_rows(vec![row("a")]);
    state.set_foreign(vec![ForeignTrash {
        module_label: "草稿箱".to_string(),
        count: 2,
    }]);
    assert_eq!(state.rows().len(), 1);
    assert_eq!(state.foreign()[0].count, 2);
    assert_eq!(state.foreign()[0].module_label, "草稿箱");
}

#[test]
fn busy_and_read_only_gate_actions() {
    let state = TrashDialogState::new();
    assert!(!state.busy() && !state.read_only());
    state.set_busy(true);
    state.set_read_only(true);
    assert!(state.busy() && state.read_only());
}
