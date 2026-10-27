//! 打标签对话框（对象级勾选）——占位骨架。
//!
//! 右键连接行 →「打标签…」→ 弹出 sheet。
//! TODO: 后续完善 checkbox 列表 + 新标签输入。

use gpui_kit::base::StyledExt;
use gpui_kit::component::sheet::Sheet;
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::panels::Shared;

pub fn open_tag_assign_dialog(
    window: &mut Window,
    cx: &mut App,
    _shared: &Shared,
    conn_id: &str,
) {
    let conn_id = conn_id.to_string();
    window.open_sheet(cx, move |sheet, _window, cx| {
        let theme = cx.theme();
        let body = div()
            .v_flex()
            .p_4()
            .child(div().text_sm().child(format!("打标签 · {conn_id}")))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("（待完善：勾选已有标签 + 输入新标签名）"),
            );
        sheet.title("打标签").size(px(320.)).child(body)
    });
}
