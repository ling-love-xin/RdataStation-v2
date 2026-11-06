//! 版本对话框：打开入口与行/动作条的装配。

use super::*;
use super::render::{action_bar, header_line, version_line};

/// 打开版本历史对话框（宿主入口）。
///
/// `on_action` 在用户点动作栏时回调一次（带版本号）；**是否忙由宿主说了算**：
/// 宿主把 `state.set_busy(true)` 打开，动作完成、新行推回来后 `set_busy(false)`。
/// `on_close` 在对话框关闭后调用（宿主据此清掉"当前开着的那一个"的记账）。
pub fn open_version_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: VersionDialogSeed,
    state: VersionDialogState,
    on_action: impl Fn(VersionAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    open_version_dialog_with(window, cx, seed, state, on_action, on_close);
}

/// 同上，但用调用方建好的状态（宿主持它做后续更新；窗口测试从这条缝进来）。
pub fn open_version_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: VersionDialogSeed,
    state: VersionDialogState,
    on_action: impl Fn(VersionAction, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    // 种子行灌进状态：之后一切渲染与动作都只读这一个来源。
    if state.rows().is_empty() {
        state.set_rows(seed.rows.clone());
    }
    let on_action: Rc<dyn Fn(VersionAction, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    tracing::info!(
        name = %seed.name,
        current_version = seed.current_version,
        rows = seed.rows.len(),
        "【探针】版本历史对话框打开"
    );
    let name = seed.name.clone();
    let current_version = seed.current_version;

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let rows = state.rows();
        let selected = state.selected();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();
        let shown = rows.len().min(MAX_VERSION_ROWS);

        let mut list = div().v_flex().w_full();
        list = list.child(header_line(theme));
        for row in rows.iter().take(shown) {
            list = list.child(version_line(
                theme,
                row,
                selected == Some(row.version),
                state.clone(),
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
                        "还有 {} 个更早的版本未列出（一次最多列 {MAX_VERSION_ROWS} 个）",
                        rows.len() - shown
                    )),
            );
        }

        let mut body = div()
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
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("共 {} 个版本", rows.len())),
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
            )
            .child(
                // 行多时滚动（带滚动的区域返回的不是 `Div`）。
                list.max_h(rems(ui::VERSION_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            );

        if let Some(row) = state.selected_row() {
            body = body.child(action_bar(
                theme,
                &row,
                busy,
                read_only,
                state.clone(),
                on_action.clone(),
            ));
        }

        dialog
            .title(format!("版本历史 · {name}"))
            .w(cx.theme().font_size * ui::VERSION_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new().child(
                    Button::new("version-close")
                        .with_variant(ButtonVariant::Secondary)
                        .small()
                        .debug_selector(|| "version-close".to_string())
                        .label("关闭")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
            // 副标题位摆当前版本号：列表第一行也有，但这里回答"我现在在第几版"更快。
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("当前版本 v{current_version}")),
            )
            .on_close({
                // `Rc` 不是 `Copy`：`Fn` 闭包（每帧重建）里要 clone 一份再 move 进去。
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_cancel(|_, _, _| true)
    });
}

