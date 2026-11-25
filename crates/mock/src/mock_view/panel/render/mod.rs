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
        let actions = self.render_actions(window, cx);
        // 右 Dock 内容区自身可滚动（Dock 的 `#tab-content` 不产生滚动，见布局规格）
        // 工具栏**移出滚动体与 `p_2`**（2026-11-25 反馈②）：它属于面板头那一档，
        // 必须紧贴 dock 标签条、与正文同左缘（`PanelToolbar` 自带 8 内距，正文也是 8）。
        // 此前工具栏是 `p_2` 的第一个孩子 → 上方多 8px 缝、左右各多 8。
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().colors.background)
            .child(self.render_toolbar(window, cx))
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
                            .px_2()
                            .pt_2()
                            .pb_2()
                            // Dock 的面板头已经写着「Mock 生成」：面板内不再重复一个标题，
                            // 工具栏（＋⇄▦⟳）直接接在标题栏下面（D38 + 2026-10-18 对齐原型）
                            .child(actions)
                            .child(self.render_folds(window, cx)),
                    ),
            )
    }
}
