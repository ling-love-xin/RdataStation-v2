//! 标签选择状态（三态勾选 / 差集）的单元用例。

// 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀）。
use super::{TagChoice, TagDialogState};

fn choice(id: &str) -> TagChoice {
    TagChoice {
        id: id.to_string(),
        name: format!("标签{id}"),
        count: 1,
    }
}

#[test]
fn diff_is_relative_to_the_baseline() {
    let state = TagDialogState::new(vec!["at_1".to_string()]);
    state.set_options(vec![choice("at_1"), choice("at_2"), choice("at_3")]);

    // 勾上一个新的、去掉原来的 → (add, remove) 各一个。
    state.toggle("at_2");
    state.toggle("at_1");
    assert_eq!(
        state.diff(),
        (vec!["at_2".to_string()], vec!["at_1".to_string()])
    );

    // 勾回去「at_1」：只剩 at_2 是新增。
    state.toggle("at_1");
    assert_eq!(state.diff(), (vec!["at_2".to_string()], Vec::new()));

    // 再取消 at_2：回到开窗时的状态，没有改动可提交。
    state.toggle("at_2");
    assert_eq!(state.diff(), (Vec::new(), Vec::new()));
}

/// 行内勾选框按**值**写（不是翻转）：重复写同一值不改变勾选顺序，也不会重复入列。
#[test]
fn set_checked_writes_by_value_and_is_idempotent() {
    let state = TagDialogState::new(Vec::new());
    state.set_options(vec![choice("at_1"), choice("at_2")]);

    state.set_checked("at_1", true);
    state.set_checked("at_2", true);
    assert_eq!(
        state.selected(),
        vec!["at_1".to_string(), "at_2".to_string()]
    );
    assert_eq!(state.diff().0.len(), 2, "两枚都是新增");

    // 重复写：不重复入列、不重置顺序。
    state.set_checked("at_1", true);
    assert_eq!(
        state.selected(),
        vec!["at_1".to_string(), "at_2".to_string()]
    );

    // 写 false 只摘掉这一枚；已经没勾的再写 false 不报错。
    state.set_checked("at_1", false);
    assert_eq!(state.selected(), vec!["at_2".to_string()]);
    state.set_checked("at_1", false);
    assert_eq!(state.selected(), vec!["at_2".to_string()]);
}

/// 多选三态：部分 → 全有（补打）→ 无（摘掉）；**部分集必须算进差集**。
///
/// 为何单独铉：把「部分有」当布尔翻转，会让用户想“让全部都有”的那一下
/// 反而把已挂的那些摘掉（一次没被要求的批量删除）。
#[test]
fn batch_toggle_promotes_partial_then_clears_and_the_diff_is_batch_correct() {
    let state = TagDialogState::new_batch(
        vec!["at_1".to_string()], // 两项都挂着
        vec!["at_2".to_string()], // 只有一项挂着
    );
    state.set_options(vec![choice("at_1"), choice("at_2"), choice("at_3")]);
    assert!(state.is_selected("at_1"));
    assert!(state.is_partial("at_2"));
    assert!(!state.is_partial("at_1"), "全有的不是部分");
    assert_eq!(state.diff(), (Vec::new(), Vec::new()), "开窗时无改动");

    // 点一下「部分」：提升为全有 → 差集里出现在 add（宿主会给缺的补上）
    state.promote_partial("at_2");
    assert!(!state.is_partial("at_2"));
    assert!(state.is_selected("at_2"));
    assert_eq!(state.diff(), (vec!["at_2".to_string()], Vec::new()));

    // 再点一下（现在是全有）：变成无 → 出现在 remove（宿主会从所有目标上摘掉）
    state.toggle("at_2");
    assert!(!state.is_selected("at_2"));
    let (add, remove) = state.diff();
    assert!(add.is_empty(), "取消不是新增：{add:?}");
    assert_eq!(
        remove,
        vec!["at_2".to_string()],
        "“部分 → 取消”也要摘掉（只算 baseline 会漏掉它）"
    );

    // 取消一个本来就是全有的 → 也在 remove 里（顺序：baseline 在前，baseline_partial 在后）
    state.toggle("at_1");
    assert_eq!(state.diff().1, vec!["at_1".to_string(), "at_2".to_string()]);

    // 勾一个全新的 → 只进 add
    state.toggle("at_3");
    assert_eq!(
        state.diff(),
        (
            vec!["at_3".to_string()],
            vec!["at_1".to_string(), "at_2".to_string()]
        )
    );
}

/// 行内勾选框的三态版：勾上 = 全有（含从部分提升）；取消 = 全无（部分也一并清）。
#[test]
fn three_way_checkbox_writes_full_have_or_none() {
    let state = TagDialogState::new_batch(Vec::new(), vec!["at_1".to_string()]);
    state.set_options(vec![choice("at_1")]);

    state.set_checked_three_way("at_1", true);
    assert!(state.is_selected("at_1") && !state.is_partial("at_1"));
    assert_eq!(state.diff().0, vec!["at_1".to_string()]);

    state.set_checked_three_way("at_1", false);
    assert!(!state.is_selected("at_1") && !state.is_partial("at_1"));
    assert_eq!(
        state.diff().1,
        vec!["at_1".to_string()],
        "取消后不能还原成“部分”（否则用户会看到它又自己回来了）"
    );
}

/// 多选重置：新基准含「部分」集，不会重复提交同一份改动。
#[test]
fn reset_batch_moves_both_baselines() {
    let state = TagDialogState::new_batch(Vec::new(), vec!["at_1".to_string()]);
    state.set_options(vec![choice("at_1"), choice("at_2")]);
    state.promote_partial("at_1");
    assert_eq!(state.diff().0, vec!["at_1".to_string()]);

    // 宿主动作完成后回推新状态：at_1 现在是全有，at_2 变成部分
    state.reset_batch(vec!["at_1".to_string()], vec!["at_2".to_string()]);
    assert!(state.is_selected("at_1") && state.is_partial("at_2"));
    assert_eq!(state.diff(), (Vec::new(), Vec::new()));
}

/// 标签被删（词典里没了）：勾选跟着清掉，否则会提交一个指向不存在标签的 add。
#[test]
fn set_options_drops_selections_that_no_longer_exist() {
    let state = TagDialogState::new(vec!["at_1".to_string()]);
    state.set_options(vec![choice("at_2")]);
    assert_eq!(state.selected(), Vec::<String>::new());
    assert_eq!(state.diff(), (Vec::new(), Vec::new()));
}

/// 动作成功后宿主重置选择：新状态即新基准，不会重复提交同一份改动。
#[test]
fn reset_selection_moves_the_baseline() {
    let state = TagDialogState::new(Vec::new());
    state.set_options(vec![choice("at_1")]);
    state.toggle("at_1");
    assert_eq!(state.diff().0, vec!["at_1".to_string()]);

    state.reset_selection(vec!["at_1".to_string()]);
    assert!(state.is_selected("at_1"));
    assert_eq!(state.diff(), (Vec::new(), Vec::new()));
}
