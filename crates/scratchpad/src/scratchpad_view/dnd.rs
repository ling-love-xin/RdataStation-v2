//! 拖拽幽灵：把草稿行拖进编辑器时跟着鼠标的胶囊（载荷类型在 `shared::drag`）。

use super::*;

/// 拖拽幽灵：一个跟着鼠标的胶囊，写着文件名（与用户抓住的东西一致）。
pub(super) struct ScratchpadDragGhost {
    pub(super) label: String,
}

impl gpui_kit::Render for ScratchpadDragGhost {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_0p5()
            .rounded_sm()
            .border_1()
            .border_color(theme.colors.border)
            .bg(theme.colors.popover)
            .text_xs()
            .text_color(theme.colors.foreground)
            .child(self.label.clone())
    }
}
