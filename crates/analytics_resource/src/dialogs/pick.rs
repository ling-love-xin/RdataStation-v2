//! 「从草稿箱归档」的草稿选择对话框（原型 §2.1 的 `＋ ▾` 第一项）。
//!
//! 分工与另两个对话框一致：**候选列表由宿主备好**（M6 不认识草稿箱的内部结构，
//! 依赖方向是 `scratchpad → analytics_resource`），本对话框只做选中与确认。
//!
//! 单选与多选走两条收尾（这是刻意的）：
//!
//! - **选一个** → 交回给宿主展开完整的归档确认对话框：能改显示名、能看到"目标被占则改名到哪"，
//!   与「从本地文件归档…」是同一条路；
//! - **选多个** → 批量入队，显示名取文件名去扩展名、来源连接各自带出。
//!   列表里没有那么多位置让用户逐个改名——与其摆一个假的批量编辑，不如说清批量用的是默认名。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

/// 列表最多列多少条：再多只列前 N 并**明说**（不静默截断，也不为这个对话框手搓虚拟化）。
pub const MAX_DRAFT_ROWS: usize = 50;

/// 一条可归档的草稿（**宿主快照**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftCandidate {
    /// 草稿绝对路径（归档请求的 `source_path`）。
    pub abs_path: PathBuf,
    /// 草稿在 `scratchpad/` 下的相对路径（`ArchiveBinding::promoted_from` 的原料——归档凭证的"出处"）。
    pub rel_path: String,
    /// 默认显示名（文件名去扩展名）。
    pub display_name: String,
    /// 来源连接（草稿 `file_meta` 的首选连接：显式绑定优先，其次最近执行用过的那条）。
    pub connection_id: Option<String>,
}

/// 打开对话框所需的全部信息（宿主在事件路径备好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickDialogSeed {
    pub candidates: Vec<DraftCandidate>,
}

/// 选中态（builder 每帧重建，状态只能由外部持有）。
#[derive(Clone)]
pub struct PickDialogState {
    selected: Rc<RefCell<Vec<bool>>>,
}

impl PickDialogState {
    /// 按候选数量建全不选的初始态（用户点哪个才归档哪个，不预选）。
    pub fn new(count: usize) -> Self {
        Self {
            selected: Rc::new(RefCell::new(vec![false; count])),
        }
    }

    fn selected_count(&self) -> usize {
        self.selected.borrow().iter().filter(|on| **on).count()
    }

    fn all_selected(&self) -> bool {
        let selected = self.selected.borrow();
        !selected.is_empty() && selected.iter().all(|on| *on)
    }

    /// 勾选 / 取消某一条（行内 checkbox 与窗口测试走它：选中态只有这一处落笔）。
    pub fn set_selected(&self, index: usize, on: bool) {
        if let Some(slot) = self.selected.borrow_mut().get_mut(index) {
            *slot = on;
        }
    }

    /// 全选 / 全不选。
    pub fn set_all(&self, on: bool) {
        for slot in self.selected.borrow_mut().iter_mut() {
            *slot = on;
        }
    }
}

/// 校验 + 取值；`None` = 一个都没选（按钮本就置灰，这条是防御）。
pub fn submit_pick(
    candidates: &[DraftCandidate],
    state: &PickDialogState,
) -> Option<Vec<DraftCandidate>> {
    let picked: Vec<DraftCandidate> = candidates
        .iter()
        .zip(state.selected.borrow().iter())
        .filter(|(_, on)| **on)
        .map(|(candidate, _)| candidate.clone())
        .collect();
    (!picked.is_empty()).then_some(picked)
}

/// 打开草稿选择对话框（宿主入口）。
///
/// `on_submit` 只在**至少选了一个**时回调一次，收的是选中的候选（顺序与列表一致）。
pub fn open_draft_pick_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: PickDialogSeed,
    on_submit: impl Fn(Vec<DraftCandidate>, &mut Window, &mut App) + 'static,
) {
    let state = PickDialogState::new(seed.candidates.len());
    open_draft_pick_dialog_with(window, cx, seed, state, on_submit);
}

/// 同上，但用调用方建好的选中态（窗口测试从这条缝进来）。
pub fn open_draft_pick_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: PickDialogSeed,
    state: PickDialogState,
    on_submit: impl Fn(Vec<DraftCandidate>, &mut Window, &mut App) + 'static,
) {
    let candidates = seed.candidates;
    let on_submit = Rc::new(on_submit);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let selected_count = state.selected_count();
        let all_selected = state.all_selected();
        let shown = candidates.len().min(MAX_DRAFT_ROWS);

        // 全选：写全部；再点（已全选）就全清——一个开关比两个按钮省地方。
        let toggle_all = {
            let state = state.clone();
            move |_: &bool, _window: &mut Window, _cx: &mut App| {
                state.set_all(!all_selected);
            }
        };

        let mut rows = div().v_flex().w_full().gap_1();
        for (index, candidate) in candidates.iter().take(shown).enumerate() {
            let checked = state.selected.borrow().get(index).copied().unwrap_or(false);
            let toggle = {
                let state = state.clone();
                move |value: &bool, _window: &mut Window, _cx: &mut App| {
                    state.set_selected(index, *value);
                }
            };
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        Checkbox::new(("draft-pick-row", index))
                            .checked(checked)
                            .on_click(toggle),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .child(candidate.rel_path.clone()),
                    )
                    .when_some(candidate.connection_id.clone(), |row, connection| {
                        row.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(connection),
                        )
                    }),
            );
        }
        if candidates.len() > shown {
            rows = rows.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!(
                        "还有 {} 个未列出（一次最多选 {MAX_DRAFT_ROWS} 个）",
                        candidates.len() - shown
                    )),
            );
        }

        dialog
            .title("从草稿箱归档")
            .w(cx.theme().font_size * ui::PICK_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(format!(
                                        "已选 {selected_count} / 共 {} 个",
                                        candidates.len()
                                    )),
                            )
                            .child(
                                Checkbox::new("draft-pick-all")
                                    .label("全选")
                                    .checked(all_selected)
                                    .on_click(toggle_all),
                            ),
                    )
                    .child(
                        // 行多时滚动（带滚动的区域返回 `AnyElement`：`overflow_y_scrollbar`
                        // 返回的不是 `Div`）。
                        rows.max_h(rems(ui::PICK_LIST_MAX_HEIGHT))
                            .overflow_y_scrollbar()
                            .into_any_element(),
                    )
                    .child(hint_line(
                        theme,
                        "选一个 → 可改显示名；选多个 → 用默认名批量归档",
                    )),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("draft-pick-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "draft-pick-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("draft-pick-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "draft-pick-ok".to_string())
                            .label(if selected_count > 0 {
                                format!("归档 {selected_count} 个")
                            } else {
                                "归档".to_string()
                            })
                            .disabled(selected_count == 0)
                            .on_click({
                                let candidates = candidates.clone();
                                let state = state.clone();
                                let on_submit = on_submit.clone();
                                move |_, window, cx| {
                                    let Some(picked) = submit_pick(&candidates, &state) else {
                                        return;
                                    };
                                    // 关窗先于执行（回调里可能再开一个对话框：单选那条就是）。
                                    window.close_dialog(cx);
                                    on_submit(picked, window, cx);
                                }
                            }),
                    ),
            )
            .on_cancel(|_, _, _| true)
    });
}

/// 一行说明（弱化色）。
fn hint_line(theme: &Theme, text: &str) -> Div {
    div()
        .w_full()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::{DraftCandidate, PickDialogState, submit_pick};

    fn candidate(name: &str) -> DraftCandidate {
        DraftCandidate {
            abs_path: std::path::PathBuf::from(format!("D:/p/scratchpad/{name}")),
            rel_path: name.to_string(),
            display_name: name.trim_end_matches(".sql").to_string(),
            connection_id: None,
        }
    }

    #[test]
    fn pick_returns_selected_candidates_in_list_order() {
        let candidates = vec![candidate("a.sql"), candidate("b.sql"), candidate("c.sql")];
        let state = PickDialogState::new(candidates.len());

        assert!(
            submit_pick(&candidates, &state).is_none(),
            "全不选 = 不提交"
        );

        state.set_selected(2, true);
        state.set_selected(0, true);
        let picked = submit_pick(&candidates, &state).expect("选了就该提交");
        assert_eq!(
            picked
                .iter()
                .map(|candidate| candidate.rel_path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.sql", "c.sql"],
            "顺序跟列表一致（用户看到的就是这个顺序）"
        );
    }
}
