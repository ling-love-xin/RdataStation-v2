//! 归档表单的字段件：只读块、分组选择、模式选择、标签与提示。

use super::*;

/// 只读信息区：来源 / 落点或源位置 / 来源连接。
///
/// 与表单区的形状刻意不同（标签在上是"要你填"，标签在左是"告诉你"），
/// 用户扫一眼就知道哪几行动不了。第二行随归档模式切换（§12.1.1 ①）：
/// 复制 = `resources/` 目标相对路径；引用 = 源文件绝对路径（本体留原位）。
pub(super) fn readonly_block(
    theme: &Theme,
    seed: &ArchiveDialogSeed,
    mode: &Rc<std::cell::RefCell<PayloadMode>>,
) -> Div {
    let body_mode = *mode.borrow();
    let (second_label, second_value) = match body_mode {
        PayloadMode::Copy => ("目标位置", seed.rel_path.clone()),
        PayloadMode::Reference => (
            "源位置",
            seed.source_abs_path
                .clone()
                .unwrap_or_else(|| seed.source_label.clone()),
        ),
    };
    let rows: Vec<(&str, String)> = vec![
        ("来源", seed.source_label.clone()),
        (second_label, second_value),
        (
            "来源连接",
            seed.source_connection
                .clone()
                .unwrap_or_else(|| "未记录".to_string()),
        ),
    ];
    let mut out = div().v_flex().w_full().gap_2();
    for (label, value) in rows {
        out = out.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .child(
                    div()
                        .w(rems(ui::DETAIL_LABEL_WIDTH))
                        .flex_none()
                        .text_color(theme.colors.muted_foreground)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().text_ellipsis().child(value)),
        );
    }
    out
}

/// 分组选择：一个下拉按钮（未分组 / 各分组），当前项打勾。
///
/// 为什么选择结果住在 `Rc<RefCell<_>>` 而不是实体：下拉菜单不是输入框，没有现成的实体可挂；
/// 而开窗 builder 是 `Fn`（每帧重建），状态不能放在闭包里——选项点击后调 `window.refresh()`
/// 让对话框重画一帧，按钮文案就跟着变。
pub(super) fn group_picker(group_id: &Rc<std::cell::RefCell<Option<String>>>, groups: &[GroupOption]) -> Div {
    let current = group_id.borrow().clone();
    // 名单里找不到的 id（分组刚被别处删了）按“未分组”显示：不把悬空 id 当选项摆出来。
    let label = current
        .as_ref()
        .and_then(|id| groups.iter().find(|group| &group.id == id))
        .map(|group| group.name.clone())
        .unwrap_or_else(|| "未分组".to_string());

    let items = groups.to_vec();
    let is_none = current.is_none();
    let button = Button::new("archive-group")
        .ghost()
        .small()
        .label(format!("{label} ▾"))
        .dropdown_menu({
            let target = group_id.clone();
            move |menu, _window, _cx| {
                let mut menu = menu.item(PopupMenuItem::label("分组"));
                let target_none = target.clone();
                menu = menu.item(PopupMenuItem::new("未分组").checked(is_none).on_click(
                    move |_, window, _cx| {
                        *target_none.borrow_mut() = None;
                        window.refresh();
                    },
                ));
                for group in &items {
                    let id = group.id.clone();
                    let checked = current.as_deref() == Some(id.as_str());
                    let target = target.clone();
                    let target_id = id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(group.name.clone())
                            .checked(checked)
                            .on_click(move |_, window, _cx| {
                                *target.borrow_mut() = Some(target_id.clone());
                                window.refresh();
                            }),
                    );
                }
                menu
            }
        });

    div().w_full().child(button)
}

/// 归档方式单选（复制 / 引用，§12.1.1 ①）。
///
/// 与 `group_picker` 同一纪律：选择结果住在共享单元格（builder 是 `Fn` 每帧重建，状态不能
/// 放闭包）；点选项后 `window.refresh()` 重画一帧，选中态跟着单元格走。仅 File 档渲染
/// （`allow_reference = true`）；分析快照固定复制，不摆单选。
pub(super) fn mode_picker(theme: &Theme, mode: &Rc<std::cell::RefCell<PayloadMode>>) -> Div {
    let current = *mode.borrow();
    let mut list = div().v_flex().w_full().gap_1();
    for candidate in [PayloadMode::Copy, PayloadMode::Reference] {
        let checked = current == candidate;
        let target = mode.clone();
        let picked = candidate;
        list = list.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .items_start()
                .py_1()
                .child(
                    Radio::new(ElementId::Name(
                        format!("archive-mode-{}", candidate.as_db_str()).into(),
                    ))
                    .checked(checked)
                    .on_click(move |_, window, _cx| {
                        *target.borrow_mut() = picked;
                        window.refresh();
                    }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.colors.foreground)
                                .child(candidate.label().to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(candidate.hint().to_string()),
                        ),
                ),
        );
    }
    list
}

/// 字段名（弱化色，与它下面的输入框一组）。
pub(super) fn field_label(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 一行提示（校验用 `danger`，结果告知用 `warning`）。
pub(super) fn hint_line(text: &str, color: gpui_kit::Hsla) -> Div {
    div()
        .w_full()
        .text_xs()
        .text_color(color)
        .child(text.to_string())
}

