//! 弹窗域（`panel/` 第五块）：SQL 预览弹出层（等效浮层 + kit Textarea）。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::ResultPanel;
use crate::ports::ResultRequest;
use crate::ui;

impl ResultPanel {
    pub(crate) fn render_sql_dialog(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            // 遮罩：走主题 `overlay` token（明暗各一套），不再写死半透明黑。
            // ⚠️ `overlay` 与 Quick Open 的遮罩是同一个 token（palette.rs:715），此处靠它表达
            // 「模态」强度；两者若要分档，须等 schema 支持独立 scrim 键（0.8.1 无，
            // colors 内写未知键会被 serde 忽略并回退透明）。
            // 无点击关闭（kit Div 无 on_click，交互走 ✕ 按钮 / Esc）
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-sql-dialog".to_string())
            .on_key_down(move |event, _window, app| {
                if event.keystroke.key == "escape" {
                    close_queue.request(ResultRequest::CloseSqlDialog, app);
                }
            })
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(680.))
                    .max_h(px(480.))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-sql-dialog-card".to_string())
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .border_b(ui::HAIRLINE)
                            .border_color(theme.colors.border)
                            .child(div().text_sm().child("原始 SQL"))
                            .child(
                                Button::new("editor-result-sql-dialog-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| "editor-result-sql-dialog-close".to_string())
                                    .tooltip("关闭".to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue2.request(ResultRequest::CloseSqlDialog, app);
                                    }),
                            ),
                    )
                    .child(
                        div().flex_1().min_h_0().p_3().child(
                            self.sql_textarea
                                .as_ref()
                                .map(|ta| {
                                    Textarea::new(ta)
                                        .readonly(true)
                                        .h(px(360.))
                                        .into_any_element()
                                })
                                .unwrap_or_else(|| {
                                    div()
                                        .p_4()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("（无 SQL）")
                                        .into_any_element()
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }
}
