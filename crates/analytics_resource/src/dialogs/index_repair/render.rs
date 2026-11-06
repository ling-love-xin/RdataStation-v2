//! 索引修复对话框的渲染件：问题条目行。

use super::*;

/// 一行待修复项：标题 / 副文案 / （指纹对照）+ 行内动作。
pub(super) fn repair_line(
    theme: &Theme,
    row: &RepairRow,
    index: usize,
    busy: bool,
    read_only: bool,
    state: RepairDialogState,
    on_action: Rc<dyn Fn(RepairAction, &mut Window, &mut App)>,
) -> impl IntoElement {
    let muted = theme.colors.muted_foreground;
    let can_write = !busy && !read_only;
    let mut line = div()
        .h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(theme.colors.list_hover)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .v_flex()
                .gap_0p5()
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(theme.colors.foreground)
                        .child(row.title.clone()),
                )
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(muted)
                        .child(row.detail.clone()),
                )
                .when(!row.hash_detail.is_empty(), |cell| {
                    cell.child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(muted)
                            .child(row.hash_detail.clone()),
                    )
                }),
        );

    match row.group {
        RepairGroup::Untracked => {
            let rel_path = row.rel_path.clone();
            let dispatch = {
                let state = state.clone();
                let on_action = on_action.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    state.set_busy(true);
                    state.set_note(Some("正在补登…".to_string()));
                    on_action(
                        RepairAction::Adopt {
                            rel_path: rel_path.clone(),
                        },
                        window,
                        cx,
                    );
                }
            };
            line = line.child(
                Button::new(("repair-adopt", index))
                    .small()
                    .debug_selector(move || format!("repair-adopt-{index}"))
                    .label("补登为存档")
                    .disabled(!can_write)
                    .on_click(dispatch),
            );
        }
        RepairGroup::Missing => {
            let resource_id = row.resource_id.clone().unwrap_or_default();
            // 「从回收站还原」：本体当初进了项目级回收站的话，这就是正规的路（不用手搬文件）。
            // 还原的成败与“为什么不行”都在服务层说（找不到条目会直说只能删记录）。
            let restore = {
                let state = state.clone();
                let on_action = on_action.clone();
                let rel_path = row.rel_path.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    state.set_busy(true);
                    state.set_note(Some("正在从回收站还原…".to_string()));
                    on_action(
                        RepairAction::RestoreFromTrash {
                            rel_path: rel_path.clone(),
                        },
                        window,
                        cx,
                    );
                }
            };
            line = line.child(
                Button::new(("repair-restore", index))
                    .ghost()
                    .small()
                    .debug_selector(move || format!("repair-restore-{index}"))
                    .label("从回收站还原")
                    .disabled(!can_write)
                    .on_click(restore),
            );
            let dispatch = {
                let state = state.clone();
                let on_action = on_action.clone();
                let resource_id = resource_id.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    // 删记录不可恢复（本体都没了，没有可还原的东西）：先过确认框。
                    let state = state.clone();
                    let on_action = on_action.clone();
                    let resource_id = resource_id.clone();
                    window.open_alert_dialog(cx, move |alert, _window, _cx| {
                        let state = state.clone();
                        let on_action = on_action.clone();
                        let resource_id = resource_id.clone();
                        alert
                            .title("删除这条登记记录？")
                            .description(
                                "本体已经不存在，删掉记录后这个存档就彻底没了（不含回收站）。",
                            )
                            .show_cancel(true)
                            .on_ok(move |_, window, cx| {
                                state.set_busy(true);
                                state.set_note(Some("正在删除记录…".to_string()));
                                on_action(
                                    RepairAction::DeleteRecord {
                                        resource_id: resource_id.clone(),
                                    },
                                    window,
                                    cx,
                                );
                                true
                            })
                    });
                }
            };
            line = line.child(
                Button::new(("repair-delete", index))
                    .ghost()
                    .small()
                    .debug_selector(move || format!("repair-delete-{index}"))
                    .label("删除记录")
                    .disabled(!can_write)
                    .on_click(dispatch),
            );
        }
        RepairGroup::Changed => {
            let resource_id = row.resource_id.clone().unwrap_or_default();
            let accept = {
                let state = state.clone();
                let on_action = on_action.clone();
                let resource_id = resource_id.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    state.set_busy(true);
                    state.set_note(Some("正在接受当前内容…".to_string()));
                    on_action(
                        RepairAction::AcceptContent {
                            resource_id: resource_id.clone(),
                        },
                        window,
                        cx,
                    );
                }
            };
            let open_versions = {
                let resource_id = resource_id.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    on_action(
                        RepairAction::OpenVersions {
                            resource_id: resource_id.clone(),
                        },
                        window,
                        cx,
                    );
                }
            };
            line = line
                .child(
                    Button::new(("repair-accept", index))
                        .small()
                        .debug_selector(move || format!("repair-accept-{index}"))
                        .label("接受当前内容")
                        .disabled(!can_write)
                        .on_click(accept),
                )
                // 跳转不受只读与忙态影响（它不改任何状态）。
                .child(
                    Button::new(("repair-versions", index))
                        .ghost()
                        .small()
                        .debug_selector(move || format!("repair-versions-{index}"))
                        .label("打开版本历史…")
                        .on_click(open_versions),
                );
        }
    }

    line
}

