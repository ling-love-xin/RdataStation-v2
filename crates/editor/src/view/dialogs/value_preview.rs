//! 值查看器：看全一个值（等宽、保留换行、可滚动可复制）。

use super::*;

/// 【遗留 #7】值查看器：右键「查看完整值」弹的模态层
///
/// 与右键菜单里「按值筛选「预览」」（24 字符截断）的分工：那是菜单文案，不该把菜单
/// 撑开；这里是**看全一个值**的地方——等宽字体、保留原始换行、可滚动、可复制。
/// 复制**不关弹层**（用户复制完可能还要接着看），关闭 / Esc / 点遮罩才收。
pub fn open_value_preview(
    window: &mut Window,
    cx: &mut App,
    value: impl Into<SharedString>,
    column: impl Into<SharedString>,
    row: usize,
    on_copy: impl Fn(&mut App) + 'static,
) {
    let value = value.into();
    let column = column.into();
    let on_copy = Rc::new(on_copy);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("列「{column}」 · 行 {row}")),
            )
            .child(
                div()
                    .w_full()
                    .max_h(rems(ui::VALUE_PREVIEW_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .bg(theme.colors.background)
                    .border_1()
                    .border_color(theme.colors.border)
                    .rounded_md()
                    .p_3()
                    .font_family("monospace")
                    .text_sm()
                    // 测试可观测：值内容区（窗口级验收断言「值真的渲染在弹层里」）
                    .debug_selector(|| "editor-dialog-editor-value-preview-body".to_string())
                    // 多行值（JSON / 文本）按 `\n` 自然换行；连续空格折叠可接受——
                    // 看全的是「换行结构」，不是空格字节
                    .child(value.clone()),
            );

        // 复制不关弹层（复制完可能还要接着看），所以不复用 action_button 的“点完即关”
        let copy = Button::new("editor-value-preview-copy")
            .with_variant(ButtonVariant::Primary)
            .small()
            .debug_selector(|| "editor-dialog-editor-value-preview-copy".to_string())
            .label("复制")
            .on_click({
                let on_copy = on_copy.clone();
                move |_, _window, cx| on_copy(cx)
            });
        let close = action_button(
            "editor-value-preview-close",
            "关闭",
            ButtonVariant::Ghost,
            move |_window, _cx| {},
        );

        dialog
            .title("查看值")
            .child(body)
            .footer(DialogFooter::new().child(copy).child(close))
    });
}
