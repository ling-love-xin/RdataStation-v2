//! 写操作确认（原型 ⑨ 执行语义矩阵 · DML 需确认）
//!
//! 口径：环境配了「DML 需确认」时，UPDATE / DELETE 在入队前先说清**哪个环境、要改什么**；
//! 用户点头才跑。确认框不给「少改几行」这类选项——收窄影响面是用户改 SQL 的事。
//! 事实（环境名 / 语句种类 / 摘要）来自引擎 `policy_guard::DmlConfirm`，视图不自己解析 SQL。

use shared::i18n::tr;
use super::*;

/// 确认框：环境名 + 语句种类 + SQL 摘要 + 「取消」/「执行」
///
/// 与 [`open_federated_confirm`] 同款：关窗**先于**回调（回调会重新进提交链）。
pub fn open_dml_confirm(
    window: &mut Window,
    cx: &mut App,
    detail: engine::services::policy_guard::DmlConfirm,
    on_run: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut App) + 'static,
) {
    let on_run = Rc::new(on_run);
    let on_cancel = Rc::new(on_cancel);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let environment = if detail.environment.is_empty() {
            "（未命名环境）".to_string()
        } else {
            detail.environment.clone()
        };

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .child(format!(
                        "环境「{environment}」的 {} 会改动数据。",
                        detail.statement
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(tr("确认后本次提交放行；取消则不入队，数据库不会被改动。")),
            )
            .child(
                div()
                    .p_2()
                    .rounded_sm()
                    .bg(theme.colors.muted)
                    .text_xs()
                    .text_color(muted)
                    .child(detail.preview.clone()),
            );

        let cancel = Button::new("editor-dml-cancel")
            .with_variant(ButtonVariant::Ghost)
            .small()
            .label(tr("取消"))
            .debug_selector(|| "editor-dml-cancel".to_string())
            .on_click({
                let on_cancel = on_cancel.clone();
                move |_, window, cx| {
                    window.close_dialog(cx);
                    on_cancel(cx);
                }
            });
        let run = Button::new("editor-dml-run")
            .with_variant(ButtonVariant::Danger)
            .small()
            .label(tr("执行"))
            .debug_selector(|| "editor-dml-run".to_string())
            .on_click({
                let on_run = on_run.clone();
                move |_, window, cx| {
                    window.close_dialog(cx);
                    on_run(window, cx);
                }
            });

        dialog
            .title(format!("确认执行 {}？", detail.statement))
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(run))
    });
}
