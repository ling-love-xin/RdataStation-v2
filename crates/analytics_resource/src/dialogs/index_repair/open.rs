//! 索引修复对话框：打开入口与分组列表装配。

use super::*;
use super::render::repair_line;

/// 打开索引修复对话框（宿主入口）。
///
/// `on_action` 在用户点行内动作时回调一次；**是否忙由宿主说了算**（动作完成、新行推回来后
/// `set_busy(false)`）。`on_close` 在对话框关闭后调用（宿主据此清掉会话记账）。
pub fn open_index_repair_dialog(
    window: &mut Window,
    cx: &mut App,
    state: RepairDialogState,
    on_action: impl Fn(RepairAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    open_index_repair_dialog_with(window, cx, state, on_action, on_close);
}

/// 同上（窗口测试从这条缝进来）。
pub fn open_index_repair_dialog_with(
    window: &mut Window,
    cx: &mut App,
    state: RepairDialogState,
    on_action: impl Fn(RepairAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    let on_action: Rc<dyn Fn(RepairAction, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let rows = state.rows();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();

        let mut body = div().v_flex().w_full().gap_2().child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("共 {} 项需要处理", rows.len())),
                )
                .when_some(note, |bar, note| {
                    bar.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.colors.warning)
                            .child(note),
                    )
                }),
        );

        if rows.is_empty() {
            // 干净 = 好消息，用 success 而不是警告色：对话框开着时用户可能刚修完最后一项。
            body = body.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.success)
                    .child("索引与本体一致：没有需要处理的问题"),
            );
        } else {
            let mut groups = div().v_flex().w_full().gap_2();
            for group in RepairGroup::ALL {
                let in_group: Vec<&RepairRow> =
                    rows.iter().filter(|row| row.group == group).collect();
                if in_group.is_empty() {
                    continue;
                }
                let shown = in_group.len().min(MAX_REPAIR_ROWS_PER_GROUP);
                let mut section = div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.colors.foreground)
                                    .child(group.title()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(format!("{}", in_group.len())),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(group.hint()),
                    );
                for (index, row) in in_group.iter().take(shown).enumerate() {
                    section = section.child(repair_line(
                        theme,
                        row,
                        index,
                        busy,
                        read_only,
                        state.clone(),
                        on_action.clone(),
                    ));
                }
                if in_group.len() > shown {
                    section = section.child(
                        div()
                            .w_full()
                            .py_1()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!(
                                "还有 {} 项未列出（每组一次最多列 {MAX_REPAIR_ROWS_PER_GROUP} 项）",
                                in_group.len() - shown
                            )),
                    );
                }
                groups = groups.child(section);
            }
            body = body.child(
                groups
                    .max_h(rems(ui::REPAIR_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            );
        }

        dialog
            .title("索引修复")
            .w(cx.theme().font_size * ui::REPAIR_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new().child(
                    Button::new("repair-close")
                        .with_variant(ButtonVariant::Secondary)
                        .small()
                        .debug_selector(|| "repair-close".to_string())
                        .label("关闭")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
            .on_close({
                // `Rc` 不是 `Copy`：`Fn` 闭包（每帧重建）里要 clone 一份再 move 进去。
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_cancel(|_, _, _| true)
    });
}

