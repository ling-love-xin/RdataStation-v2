//! 通知中心 UI 组件
//!
//! 状态栏铃铛图标 + 点击弹出历史面板。

use gpui_kit::base::StyledExt;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::service::{clear_history, history, mark_all_read, unread_count};

/// 通知铃铛按钮（状态栏用，带 Popover 通知中心）
pub fn notification_bell(_cx: &mut App) -> AnyElement {
    let unread = unread_count();
    let label = if unread > 0 {
        format!("🔔 {unread}")
    } else {
        "🔔".to_string()
    };

    Popover::new("notification-bell-popover")
        .trigger(
            Button::new("notification-bell")
                .ghost()
                .xsmall()
                .label(label),
        )
        .content(move |_, window, cx| render_notification_center(window, cx))
        .into_any_element()
}

/// 渲染通知中心内容
fn render_notification_center(_window: &mut Window, cx: &mut App) -> Div {
    let records = history();
    let theme = cx.theme().clone();

    div()
        .w(px(360.))
        .h_flex()
        .flex_col()
        .p_2()
        .gap_2()
        .bg(theme.colors.popover)
        .rounded_md()
        .border_1()
        .border_color(theme.colors.border)
        // 标题栏
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .child(text!("通知中心"))
                .child(
                    div()
                        .h_flex()
                        .gap_1()
                        .child(
                            Button::new("mark-all-read")
                                .ghost()
                                .xsmall()
                                .label("全部已读")
                                .on_click(|_, _, _cx| {
                                    mark_all_read();
                                }),
                        )
                        .child(
                            Button::new("clear-history")
                                .ghost()
                                .xsmall()
                                .label("清除")
                                .on_click(|_, _, _cx| {
                                    clear_history();
                                }),
                        ),
                ),
        )
        // 分隔线
        .child(div().h_px().bg(theme.colors.border))
        // 通知列表
        .child(
            div()
                .v_flex()
                .gap_1()
                .max_h(px(400.))
                .overflow_y_scrollbar()
                .children(if records.is_empty() {
                    vec![div().child(text!("暂无通知"))]
                } else {
                    records
                        .iter()
                        .take(10)
                        .map(|r| {
                            let type_color = match r.type_ {
                                crate::NotificationType::Info => theme.colors.info,
                                crate::NotificationType::Success => theme.colors.success,
                                crate::NotificationType::Warning => theme.colors.warning,
                                crate::NotificationType::Error => theme.colors.danger,
                            };

                            div()
                                .h_flex()
                                .gap_2()
                                .p_1()
                                .rounded_sm()
                                .when(!r.read, |el| el.bg(type_color.opacity(0.1)))
                                .child(div().w(px(4.)).h_full().bg(type_color))
                                        .child(
                                            div()
                                                .v_flex()
                                                .gap_0p5()
                                                .flex_1()
                                                .child(text!(&r.title))
                                                .when_some(r.message.as_ref(), |el, msg| {
                                                    el.child(div().text_sm().text_color(theme.colors.muted_foreground).child(text!(msg)))
                                                })
                                                .child(div().text_xs().text_color(theme.colors.muted_foreground).child(text!(&r.timestamp))),
                                        )
                        })
                        .collect()
                }),
        )
}
