//! 异常提示条与「源已变更」条（都不进索引状态统计）。

use super::*;

pub(super) fn render_banners(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    cx: &App,
) -> Vec<Div> {
    let tone_color = cx.theme().colors;
    let alert = alert_line(detail);
    let mut out: Vec<Div> = Vec::new();
    // 异常提示条
    if let Some(alert) = alert {
        out.push(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .border_1()
                .border_color(tone_color.warning)
                .text_color(tone_color.warning)
                .child(alert),
        );
    }

    // 效力条：与「物理完整性」分开呈现。文件被外部改过（ContentChanged）不等于结论
    // 已被废弃，两种状态混成一条会让用户误判资产还能不能引用。
    let (effect_label, effect_color) = match detail.lifecycle {
        crate::model::Lifecycle::Draft => ("", tone_color.muted_foreground),
        crate::model::Lifecycle::Certified => (
            "正式资产：已通过确认，可作为依据引用",
            tone_color.success,
        ),
        crate::model::Lifecycle::Deprecated => (
            "已废弃：结论不再适用，仅保留追溯",
            tone_color.danger,
        ),
        crate::model::Lifecycle::Superseded => (
            "已被取代：请改用更新版本",
            tone_color.warning,
        ),
    };
    if !effect_label.is_empty() {
        out.push(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .border_1()
                .border_color(effect_color)
                .text_color(effect_color)
                .child(effect_label),
        );
    }

    // reference 失效条（§12.1.1 ②：打开时懒校验发现源已变更；不进索引状态统计，
    // 也不属于 `ArchiveStatus`——槽位由宿主按 `resource_id` 维护，切换项目 / 接受当前内容
    // 后清除）。处置只有两条：接受当前内容（源生成新版本）、从历史还原（复用版本历史对话框）。
    let stale = actions
        .as_ref()
        .map(|a| a.host.is_reference_stale(&detail.id))
        .unwrap_or(false);
    if stale {
        let Some(actions) = actions else {
            unreachable!("无宿主时 stale 必然为 false（槽位由宿主维护）");
        };
        let detail_for_accept = detail.clone();
        let detail_for_history = detail.clone();
        let host_for_accept = actions.host.clone();
        let host_for_history = actions.host.clone();
        out.push(
            div()
                .v_flex()
                .w_full()
                .gap_1()
                .px_2()
                .py_1()
                .border_1()
                .border_color(tone_color.warning)
                .text_color(tone_color.warning)
                .child(
                    div()
                        .text_xs()
                        .child("源已变更，快照失效：当前源文件与归档时的指纹不一致。"),
                )
                .child(
                    div().h_flex().w_full().gap_2().child(
                        Button::new("archive-stale-accept")
                            .ghost()
                            .xsmall()
                            .label("接受当前内容")
                            .on_click(move |_, window, cx| {
                                host_for_accept.accept_reference_current(
                                    &detail_for_accept,
                                    window,
                                    cx,
                                )
                            }),
                    ),
                )
                .child(
                    Button::new("archive-stale-history")
                        .ghost()
                        .xsmall()
                        .label("从历史还原")
                        .on_click(move |_, window, cx| {
                            host_for_history.request_version_history(
                                &detail_for_history.id,
                                window,
                                cx,
                            )
                        }),
                ),
        );
    }
    out
}
