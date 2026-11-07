//! 表单行辅助函数：标签 + 输入。

use super::*;

/// 表单行（标签 + 输入）。
pub(crate) fn form_line(
    theme: &gpui_kit::component::Theme,
    label: &str,
    input: &Entity<InputState>,
) -> Div {
    // 走 `kv`（标签-值行）这一支原语：它与卡片里的「生成器名 / 产出类型」是同一结构，
    // 此前各写一份的后果是标签色分了两档（这里 muted_foreground、那里再淡一档）
    kv(
        theme,
        label,
        div()
            .w(rems(ui::PARAM_INPUT_WIDTH))
            .flex_none()
            .child(Input::new(input)),
    )
}

/// 复杂参数的一行：标签 + 多行文本（全宽）+ 格式提示。
///
/// 多行不能用 `form_line` 的「标签 | 定宽输入」横排：值集合一行一个，横排窄框放不下。
pub(crate) fn complex_form_line(
    theme: &gpui_kit::component::Theme,
    label: &str,
    hint: &str,
    state: &Entity<TextareaState>,
) -> Div {
    div()
        .v_flex()
        .gap_1()
        .w_full()
        .child(
            div()
                .text_xs()
                .text_color(text_hint(theme))
                .child(label.to_string()),
        )
        .child(Textarea::new(state).h(rems(ui::COMPLEX_INPUT_HEIGHT)))
        // 格式提示是「填错了才会回来读」的那一行：比标签还淡一档，不抢输入区的视线
        .child(
            div()
                .text_xs()
                .text_color(text_hint(theme))
                .child(hint.to_string()),
        )
}
