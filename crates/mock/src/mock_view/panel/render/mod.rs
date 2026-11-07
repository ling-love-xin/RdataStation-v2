// 继承父模块（panel）与 mock_view 的全部 use 绑定
use super::*;

mod toolbar;
mod columns;
mod actions;
mod sinks;
mod folds;
mod history;
mod header;
mod status;
mod dialogs;
mod form;
mod table_list;
pub(crate) use form::{complex_form_line, form_line};

impl MockPanel {
}


impl Render for MockPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let actions = self.render_actions(window, cx);
        // 右 Dock 内容区自身可滚动（Dock 的 `#tab-content` 不产生滚动，见布局规格）
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().colors.background)
            .child(
                div()
                    .id("mock-panel-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .p_2()
                            // Dock 的面板头已经写着「Mock 生成」：面板内不再重复一个标题，
                            // 工具栏（＋⇄▦⟳）直接接在标题栏下面（D38 + 2026-10-18 对齐原型）
                            .child(self.render_toolbar(window, cx))
                            .child(actions)
                            .child(self.render_folds(window, cx)),
                    ),
            )
    }
}
