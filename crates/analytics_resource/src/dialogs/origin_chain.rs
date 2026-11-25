//! 来源链对话框（刀 C）：只读列表，从当前资产一路往上追到源头。
//!
//! 打开入口在详情动作区（「查看来源链…」）；链路装配是纯函数的
//! [`crate::present::build_origin_chain`]，本文件只负责画。追不到的那一跳
//! **灰着画**并附一句解释——不跳过节点，也不假装它还存在。

use shared::i18n::tr;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _};
use gpui_kit::*;

use crate::present::OriginChain;
use crate::ui;

/// 打开来源链（只读，没有动作按钮，除关闭外不写任何东西）。
pub fn open_origin_chain(window: &mut Window, cx: &mut App, chain: OriginChain) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let mut body = div().v_flex().w_full().gap_1();

        for node in &chain.nodes {
            let color = if node.resolved {
                theme.colors.foreground
            } else {
                theme.colors.muted_foreground
            };
            let mut row = div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("•"),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(color)
                        .child(node.label.clone()),
                );
            if node.is_current {
                row = row.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.colors.primary)
                        .child(tr("当前")),
                );
            }
            body = body.child(row);
        }

        if let Some(note) = chain.note.as_deref() {
            body = body.child(
                div()
                    .w_full()
                    .pt_1()
                    .text_xs()
                    .text_color(theme.colors.warning)
                    .child(note.to_string()),
            );
        }

        dialog
            .title(tr("来源链"))
            .w(cx.theme().font_size * ui::TRASH_DIALOG_WIDTH)
            .child(
                div()
                    .w_full()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(tr("每一层只记直接上游；这里顺着它一路往上追")),
            )
            .child(body)
            .footer(
                DialogFooter::new().child(
                    Button::new("origin-chain-close")
                        .with_variant(ButtonVariant::Secondary)
                        .small()
                        .debug_selector(|| "origin-chain-close".to_string())
                        .label(tr("关闭"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
