//! 回收站对话框：打开入口与列表装配。

use super::*;
use super::render::{header_line, trash_line};

/// 打开回收站对话框（宿主入口）。
///
/// `on_action` 在用户点行内动作或「清空回收站」时回调一次；**是否忙由宿主说了算**
/// （宿主 `set_busy(true)` 打开，动作完成、新行推回来后 `set_busy(false)`）。
/// `on_close` 在对话框关闭后调用（宿主据此清掉会话记账）。
pub fn open_trash_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: TrashDialogSeed,
    state: TrashDialogState,
    on_action: impl Fn(TrashAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    open_trash_dialog_with(window, cx, seed, state, on_action, on_close);
}

/// 同上，但用调用方建好的状态（宿主持它做后续更新；窗口测试从这条缝进来）。
pub fn open_trash_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: TrashDialogSeed,
    state: TrashDialogState,
    on_action: impl Fn(TrashAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    // 种子行灌进状态：之后一切渲染与动作都只读这一个来源（与版本历史同一个形态）。
    if state.rows().is_empty() {
        state.set_rows(seed.rows.clone());
        state.set_foreign(seed.foreign.clone());
    }
    let on_action: Rc<dyn Fn(TrashAction, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let rows = state.rows();
        let foreign = state.foreign();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();
        let can_write = !busy && !read_only;
        let shown = rows.len().min(MAX_TRASH_ROWS);

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
                        .child(format!("共 {} 项", rows.len())),
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
            body = body.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("回收站是空的"),
            );
        } else {
            let mut list = div().v_flex().w_full();
            list = list.child(header_line(&theme));
            for (index, row) in rows.iter().take(shown).enumerate() {
                list = list.child(trash_line(
                    &theme,
                    row,
                    index,
                    can_write,
                    state.clone(),
                    on_action.clone(),
                ));
            }
            if rows.len() > shown {
                list = list.child(
                    div()
                        .w_full()
                        .py_1()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!(
                            "还有 {} 项未列出（一次最多列 {MAX_TRASH_ROWS} 项）",
                            rows.len() - shown
                        )),
                );
            }
            body = body.child(
                list.max_h(rems(ui::TRASH_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            );
        }

        // 别人的条目：只解释，不给动作（还原归它自己的模块，删它更是越权）。
        for item in &foreign {
            body = body.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!(
                        "另有 {} 条「{}」的条目共用一个回收站——请在那个模块的界面里处理",
                        item.count, item.module_label
                    )),
            );
        }

        let empty_dispatch = {
            let state = state.clone();
            let on_action = on_action.clone();
            let count = rows.len();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let state = state.clone();
                let on_action = on_action.clone();
                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                    let state = state.clone();
                    let on_action = on_action.clone();
                    alert
                        .confirm()
                        .title("清空资产库的回收站？")
                        .description(format!(
                            "将彻底删除资产库的 {count} 项（本体不再保留，不可恢复）；\
                             其他模块共用的条目不会被删。"
                        ))
                        .button_props(
                            DialogButtonProps::default()
                                .ok_text("清空")
                                .ok_variant(ButtonVariant::Danger)
                                .show_cancel(true),
                        )
                        .on_ok(move |_, window, cx| {
                            tracing::info!(count, "【探针】回收站对话框「清空回收站」确认");
                            state.set_busy(true);
                            state.set_note(Some("正在清空…".to_string()));
                            on_action(TrashAction::Empty, window, cx);
                            true
                        })
                });
            }
        };

        dialog
            .title("回收站")
            .w(cx.theme().font_size * ui::TRASH_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("trash-empty")
                            .with_variant(ButtonVariant::Danger)
                            .small()
                            .debug_selector(|| "trash-empty".to_string())
                            .label("清空回收站")
                            .disabled(!can_write || rows.is_empty())
                            .on_click(empty_dispatch),
                    )
                    .child(
                        Button::new("trash-close")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "trash-close".to_string())
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

