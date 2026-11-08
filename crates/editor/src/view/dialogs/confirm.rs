//! 关闭与保存的确认对话框：关闭三态、保存失败、模式切换代价。

use super::*;

/// 关闭脏文档时用户的选择（三态）
///
/// `Cancel` 与关闭对话框本身是两件事：Esc / 点遮罩也会让对话框消失，但那是“取消”，
/// 不会有回调；这里的 `Cancel` 是**用户显式点了“不关”**，调用方据此清掉状态栏提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    /// 先保存，保存成功再关
    Save,
    /// 丢弃未保存的改动直接关
    Discard,
    /// 什么都不做
    Cancel,
}

/// 保存失败后的选择（“保存失败二次确认”）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFailureChoice {
    /// 重试保存（可能是文件被占用 / 短暂无权限）
    Retry,
    /// 换个路径再试
    SaveAs,
    /// 放弃保存（**文档仍是脏的**，不关也不清脏）
    Cancel,
}

/// 关闭脏文档前的三态确认（“保存 / 不保存 / 取消”）
///
/// 调用方在 `on_choice` 里执行真正的动作：`Discard` 后才移除面板，`Save` 后再移除。
pub fn open_close_confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    on_choice: impl Fn(CloseChoice, &mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let on_choice = Rc::new(on_choice);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(SharedString::from(format!("「{title}」有未保存的改动。")))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("保存后关闭，或不保存直接丢弃这次改动。"),
            );

        let cancel = action_button("editor-close-cancel", "取消", ButtonVariant::Ghost, {
            let on_choice = on_choice.clone();
            move |window, cx| {
                // 显式取消：留痕清掉（“按了没反应”的提示不该继续挂着）
                on_choice(CloseChoice::Cancel, window, cx)
            }
        });
        let discard = action_button("editor-close-discard", "不保存", ButtonVariant::Danger, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(CloseChoice::Discard, window, cx)
        });
        let save = action_button("editor-close-save", "保存", ButtonVariant::Primary, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(CloseChoice::Save, window, cx)
        });

        dialog
            .title("关闭前确认")
            .child(body)
            // 自绘 footer 会关掉组件库的默认“确定 / 取消”按钮（footer 与 button_props 互斥）
            .footer(DialogFooter::new().child(cancel).child(discard).child(save))
    });
}

/// 保存失败后的二次确认（原因必须显示出来：用户要能据此决定重试还是换路径）
pub fn open_save_failure_confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    reason: impl Into<SharedString>,
    on_choice: impl Fn(SaveFailureChoice, &mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let reason = reason.into();
    let on_choice = Rc::new(on_choice);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let danger = cx.theme().colors.danger;
        let muted = cx.theme().colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(SharedString::from(format!("「{title}」保存失败。")))
            .child(div().text_xs().text_color(danger).child(reason.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("文档仍处于未保存状态（改动不会丢失）。"),
            );

        let cancel = action_button("editor-save-cancel", "取消保存", ButtonVariant::Ghost, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(SaveFailureChoice::Cancel, window, cx)
        });
        let save_as = action_button("editor-save-as", "另存为…", ButtonVariant::Secondary, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(SaveFailureChoice::SaveAs, window, cx)
        });
        let retry = action_button("editor-save-retry", "重试", ButtonVariant::Primary, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(SaveFailureChoice::Retry, window, cx)
        });

        dialog.title("保存失败").child(body).footer(
            DialogFooter::new()
                .child(cancel)
                .child(save_as)
                .child(retry),
        )
    });
}

/// 模式切换确认（原型 §1.3：禁止静默切换）
///
/// `kind.picks_granularity()` 为真时，footer 直接给两个动作按钮——按钮文案就是选择本身
/// （“整篇一个单元”/“按语句拆分”），不再额外摆一组单选框。
/// 其它确认分支只有“确认 + 取消”，回调收到的粒度用 `CellGranularity::default()`。
pub fn open_switch_confirm(
    window: &mut Window,
    cx: &mut App,
    kind: ConfirmKind,
    from: EditorMode,
    to: EditorMode,
    note: Option<&'static str>,
    on_confirm: impl Fn(CellGranularity, &mut Window, &mut App) + 'static,
) {
    let on_confirm = Rc::new(on_confirm);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().colors.muted_foreground;

        let mut body = div().v_flex().w_full().gap_2().text_sm().child(kind.body());
        // 状态栏同款说明（例如“结果仍保留但置灰”）：与切换后的提示保持一致口径
        if let Some(note) = note {
            body = body.child(div().text_xs().text_color(muted).child(note));
        }

        let cancel = action_button("editor-switch-cancel", "取消", ButtonVariant::Ghost, {
            // 取消：对话框消失即结束，不需要回调（不改任何状态）
            move |_window, _cx| {}
        });

        let footer = if kind.picks_granularity() {
            let single = action_button(
                "editor-switch-single",
                CellGranularity::Single.label(),
                ButtonVariant::Secondary,
                {
                    let on_confirm = on_confirm.clone();
                    move |window, cx| on_confirm(CellGranularity::Single, window, cx)
                },
            );
            let per_statement = action_button(
                "editor-switch-per-statement",
                CellGranularity::PerStatement.label(),
                ButtonVariant::Primary,
                {
                    let on_confirm = on_confirm.clone();
                    move |window, cx| on_confirm(CellGranularity::PerStatement, window, cx)
                },
            );
            DialogFooter::new()
                .child(cancel)
                .child(single)
                .child(per_statement)
        } else {
            let confirm = action_button(
                "editor-switch-confirm",
                kind.confirm_label(),
                ButtonVariant::Primary,
                {
                    let on_confirm = on_confirm.clone();
                    move |window, cx| on_confirm(CellGranularity::default(), window, cx)
                },
            );
            DialogFooter::new().child(cancel).child(confirm)
        };

        dialog
            .title(kind.title())
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(SharedString::from(format!(
                                "{} → {}",
                                from.label(),
                                to.label()
                            ))),
                    )
                    .child(body),
            )
            .footer(footer)
    });
}
