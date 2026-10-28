//! 标准 Dialog 布局模板。
//!
//! 所有弹窗应遵循统一的结构：标题区 / 内容区 / 底部按钮区，
//! 间距、对齐、样式由本模板统一管理，避免每个弹窗手搓布局。
//!
//! # 用法示例
//!
//! ```ignore
//! use rds_workbench::components::dialog_helpers::*;
//!
//! window.open_dialog(cx, move |dialog, window, cx| {
//!     dialog
//!         .title("缓存管理")
//!         .child(standard_dialog_body(
//!             div().v_flex().gap_2().child("内容..."),
//!         ))
//!         .footer(standard_dialog_footer(
//!             Button::new("confirm").label("确定"),
//!             Button::new("cancel").label("取消").on_click(|_, w, cx| w.close_dialog(cx)),
//!         ))
//! });
//! ```

use gpui_kit::{App, ClickEvent, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, px};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{
    DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
};
use gpui_kit::component::WindowExt as _;

/// 标准对话框内容区。
///
/// 统一的内容区容器：垂直布局、内容间距、最大宽度限制。
/// 替代手搓的 `div().v_flex().w_full().gap_2()...`。
pub fn standard_dialog_body(content: impl IntoElement) -> DialogContent {
    DialogContent::new().w_full().max_w(px(560.0)).child(content)
}

/// 标准对话框标题区。
///
/// 统一的标题 + 描述结构，替代手搓的标题文本。
pub fn standard_dialog_header(
    title: impl IntoElement,
    description: Option<impl IntoElement>,
) -> DialogHeader {
    let mut header = DialogHeader::new().child(DialogTitle::new().child(title));
    if let Some(desc) = description {
        header = header.child(DialogDescription::new().child(desc));
    }
    header
}

/// 标准对话框底部按钮区。
///
/// 右对齐的按钮组，主操作按钮在最右侧，取消按钮在左侧。
/// 替代手搓的 `DialogFooter::new().child(...)`。
pub fn standard_dialog_footer(
    primary_action: impl IntoElement,
    cancel_action: Option<impl IntoElement>,
) -> DialogFooter {
    let mut footer = DialogFooter::new();
    if let Some(cancel) = cancel_action {
        footer = footer.child(cancel);
    }
    footer = footer.child(primary_action);
    footer
}

/// 标准确认对话框（用于删除/确认等简单场景）。
///
/// 一行标题 + 一行描述 + 取消/确认按钮。
///
/// # 用法
///
/// ```ignore
/// window.open_dialog(cx, standard_confirm_dialog(
///     "删除连接",
///     "确定要删除这个连接吗？此操作不可撤销。",
///     |_, _, cx| { /* 删除逻辑 */ },
/// ));
/// ```
pub fn standard_confirm_dialog<F>(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    on_confirm: F,
) -> impl Fn(gpui_kit::component::dialog::Dialog, &mut Window, &mut App) -> gpui_kit::component::dialog::Dialog
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + Clone + 'static,
{
    let title: SharedString = title.into();
    let message: SharedString = message.into();
    move |dialog, _window, _cx| {
        let cancel = Button::new("confirm-cancel")
            .label("取消")
            .on_click(|_, w, cx| w.close_dialog(cx));

        let confirm = Button::new("confirm-ok")
            .with_variant(ButtonVariant::Danger)
            .label("确认")
            .on_click({
                let on_confirm = on_confirm.clone();
                move |e, w, cx| {
                    on_confirm(e, w, cx);
                    w.close_dialog(cx);
                }
            });

        dialog
            .title(title.clone())
            .child(standard_dialog_body(div().text_sm().child(message.clone())))
            .footer(standard_dialog_footer(confirm, Some(cancel)))
    }
}
