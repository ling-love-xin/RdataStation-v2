//! 回收站对话框的渲染件：表头与条目行。

use super::*;

/// 表头（列名与下面各行的列宽一一对应）。
pub(super) fn header_line(theme: &Theme) -> Div {
    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .border_b(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(div().w(rems(ui::TRASH_COL_NAME)).flex_none().child("名称"))
        .child(div().flex_1().min_w_0().child("原位置"))
        .child(
            div()
                .w(rems(ui::TRASH_COL_TIME))
                .flex_none()
                .child("删除时间"),
        )
        .child(div().w(rems(ui::TRASH_COL_SIZE)).flex_none().child("大小"))
        .child(div().w(rems(ui::TRASH_COL_ACTION)).flex_none().child(""))
}

/// 一行回收站条目：名称（+ 类型）/ 原位置 / 时间 / 大小 + 行内动作。
///
/// 动作摆在行里而不是"选中后出现的动作栏"：这里一行最多两个动作、行内容短，
/// 装得下；再说"选中"在回收站里没有别的用处。
pub(super) fn trash_line(
    theme: &Theme,
    row: &TrashRow,
    index: usize,
    can_write: bool,
    state: TrashDialogState,
    on_action: Rc<dyn Fn(TrashAction, &mut Window, &mut App)>,
) -> impl IntoElement {
    let muted = theme.colors.muted_foreground;

    let restore = {
        let state = state.clone();
        let on_action = on_action.clone();
        let trash_id = row.trash_id.clone();
        let name = row.name.clone();
        // 还原口径按粒度区分（§12.1.1 ③）：copy 把副本从回收站取回原位；
        // reference 本体一直留在源位置，还原只是复活登记行。
        let restore_note = match row.payload_mode {
            PayloadMode::Reference => "正在还原登记记录…".to_string(),
            PayloadMode::Copy => "正在还原…".to_string(),
        };
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            tracing::info!(
                trash_id = %trash_id,
                name = %name,
                "【探针】回收站对话框「还原」触发"
            );
            state.set_busy(true);
            state.set_note(Some(restore_note.clone()));
            on_action(
                TrashAction::Restore {
                    trash_id: trash_id.clone(),
                },
                window,
                cx,
            );
        }
    };
    // 永久删除口径按粒度区分：copy 连本体一起删；reference 本体留原位，只删登记行与版本元数据。
    let purge_description = match row.payload_mode {
        PayloadMode::Reference => {
            "本体留在源位置不受影响；登记记录与版本元数据将彻底删除，不可恢复。".to_string()
        }
        PayloadMode::Copy => "本体将从回收站里彻底删除，不可恢复；登记记录一并删除。".to_string(),
    };
    let purge = {
        let state = state.clone();
        let on_action = on_action.clone();
        let trash_id = row.trash_id.clone();
        let name = row.name.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            // 永久删除不可恢复（本体不留在任何地方）：先过确认框。
            let state = state.clone();
            let on_action = on_action.clone();
            let trash_id = trash_id.clone();
            // 外层闭包是 `Fn`（按钮可能被点多次）：要移进内层的东西每帧各拿一份。
            let name = name.clone();
            let purge_description = purge_description.clone();
            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                let state = state.clone();
                let on_action = on_action.clone();
                let trash_id = trash_id.clone();
                // 外层是 `Fn` 闭包（可被多次调用）：探针用的 name 在此克隆一份，
                // 不能让内层 on_ok 把外层捕获的 name move 走。
                let name_for_log = name.clone();
                alert
                    .confirm()
                    .title(format!("永久删除「{name}」？"))
                    .description(purge_description.clone())
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("永久删除")
                            .ok_variant(ButtonVariant::Danger)
                            .show_cancel(true),
                    )
                    .on_ok(move |_, window, cx| {
                        tracing::info!(
                            trash_id = %trash_id,
                            name = %name_for_log,
                            "【探针】回收站对话框「永久删除」确认"
                        );
                        state.set_busy(true);
                        state.set_note(Some("正在永久删除…".to_string()));
                        on_action(
                            TrashAction::Purge {
                                trash_id: trash_id.clone(),
                            },
                            window,
                            cx,
                        );
                        true
                    })
            });
        }
    };

    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_sm()
        // 这一行不是可点行（动作全在行内的两个按钮上），所以**不给 hover 反馈**：
        // 悬停变色是"这里能点"的承诺，而这条行本身点不动（草稿箱的引用行 / 回收站行同口径）。
        // 原先那层 hover 用的是 `accent.opacity(0.3)`（品牌色淡底）——既与"可点"无关，
        // 也与 `list_hover` 这套行态 token 不是一回事。
        .child(
            div()
                .w(rems(ui::TRASH_COL_NAME))
                .flex_none()
                .min_w_0()
                .h_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(theme.colors.foreground)
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(row.kind_label.clone()),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .text_color(muted)
                .child(row.original_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::TRASH_COL_TIME))
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(row.time_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::TRASH_COL_SIZE))
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(row.size_label.clone()),
        )
        .child(
            div()
                .w(rems(ui::TRASH_COL_ACTION))
                .flex_none()
                .h_flex()
                .items_center()
                .gap_1()
                .child(
                    Button::new(("trash-restore", index))
                        .ghost()
                        .small()
                        .debug_selector(move || format!("trash-restore-{index}"))
                        .label("还原")
                        .disabled(!can_write)
                        .on_click(restore),
                )
                .child(
                    Button::new(("trash-purge", index))
                        .ghost()
                        .small()
                        .debug_selector(move || format!("trash-purge-{index}"))
                        .label("永久删除")
                        .disabled(!can_write)
                        .on_click(purge),
                ),
        )
}

