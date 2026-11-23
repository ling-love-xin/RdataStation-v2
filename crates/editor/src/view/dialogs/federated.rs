//! 执行联邦前的确认（09 原型「执行联邦前的确认」卡落地）
//!
//! 口径（2026-11-16 拍板）：**联邦不许丢数据**。所以这里不是「要不要采样」的选择题，
//! 而是「这次要扫多少」的交代——用户唯一的干预是改 SQL / 收窄源清单，不是让我们替他砍行。
//! 正文由纯函数 [`crate::support::sources::federated_confirm_note`] 生成（视图只负责画）。

use shared::i18n::tr;
use super::*;

/// 确认框：正文 + 「取消」/「执行」
///
/// 与 [`open_bind_dialog`] 同款：关窗**先于**回调（回调里要重新进提交链，别让两层叠着）。
/// 取消回调必须清宿主挂起状态——Esc / 点遮罩也会关窗，但只有显式取消才知道「这次不跑了」。
pub fn open_federated_confirm(
    window: &mut Window,
    cx: &mut App,
    note: String,
    on_run: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut App) + 'static,
) {
    let on_run = Rc::new(on_run);
    let on_cancel = Rc::new(on_cancel);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(div().text_sm().child(note.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(tr("取消只是不执行，不会改动任何一个源。")),
            );

        let cancel = Button::new("editor-federated-cancel")
            .with_variant(ButtonVariant::Ghost)
            .small()
            .label(tr("取消"))
            .debug_selector(|| "editor-federated-cancel".to_string())
            .on_click({
                let on_cancel = on_cancel.clone();
                move |_, window, cx| {
                    window.close_dialog(cx);
                    on_cancel(cx);
                }
            });
        let run = Button::new("editor-federated-run")
            .with_variant(ButtonVariant::Primary)
            .small()
            .label(tr("执行"))
            .debug_selector(|| "editor-federated-run".to_string())
            .on_click({
                let on_run = on_run.clone();
                move |_, window, cx| {
                    window.close_dialog(cx);
                    on_run(window, cx);
                }
            });

        dialog
            .title(tr("执行联邦查询？"))
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(run))
    });
}
