//! 精调层表单：自定义生成器的创建/编辑对话框渲染。

use super::*;

/// 精调层表单：名称 + 类型（锁定/可选）+ 业务组（锁定/可选）+ 基础生成器 + 参数区 + 样例。
pub(crate) fn render_create(
    state: &Entity<GeneratorLibraryState>,
    theme: &Theme,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let s = state.read(cx);
    let Some(draft) = s.create.as_ref() else {
        return div().into_any_element();
    };
    let theme = theme.clone();
    let name_state = draft.name_input.clone();
    let mut body = div().v_flex().gap_2().w_full().h(ui::GENERATOR_BODY_HEIGHT);
    body = body.child(form_line(&theme, "生成器名", &name_state));
    // 类型行：锁定 → 只读文本；未锁定 → Select（用户定稿：字段名「产出类型」）
    body = match &draft.locked_type {
        Some(t) => body.child(
            div().h_flex().items_center().gap_2().w_full().child(kv(
                &theme,
                "产出类型",
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(column_type_label(t)),
            )),
        ),
        None => {
            if let Some(type_state) = &draft.type_state {
                body.child(select_line(
                    &theme,
                    "产出类型",
                    Select::new(type_state)
                        .menu_width(ui::GENERATOR_MENU_WIDTH)
                        .menu_max_h(ui::GENERATOR_MENU_MAX_HEIGHT)
                        .placeholder("选择产出类型…")
                        .accessibility_label("产出类型"),
                ))
            } else {
                body
            }
        }
    };
    // 业务组行：锁定 → 只读文本；未锁定 → Select（含「（不分组）」）
    body = match &draft.locked_category {
        Some(c) => body.child(
            div().h_flex().items_center().gap_2().w_full().child(kv(
                &theme,
                "业务组",
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(c.label()),
            )),
        ),
        None => {
            if let Some(category_state) = &draft.category_state {
                body.child(select_line(
                    &theme,
                    "业务组",
                    Select::new(category_state)
                        .menu_width(ui::GENERATOR_MENU_WIDTH)
                        .menu_max_h(ui::GENERATOR_MENU_MAX_HEIGHT)
                        .placeholder("选择业务组（可不选）")
                        .accessibility_label("业务组"),
                ))
            } else {
                body
            }
        }
    };
    // 基础生成器：编辑模式锁定为只读文本（换 base = 换生成器，删了重建更直接）；
    // 新建模式走 Select（按锁定类型过滤）
    body = match &draft.locked_base {
        Some(spec) => body.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("基础生成器"),
                )
                .child(kv(
                    &theme,
                    "基础生成器",
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(spec.label),
                )),
        ),
        None => body.child(select_line(
            &theme,
            "基础生成器",
            Select::new(&draft.base_state)
                .menu_width(ui::GENERATOR_MENU_WIDTH)
                .menu_max_h(ui::GENERATOR_MENU_MAX_HEIGHT)
                .placeholder("选择基础生成器…")
                .accessibility_label("基础生成器"),
        )),
    };
    // 参数区（按 base spec 重建；Int/Float/Text→Input、Bool→Switch、Complex→Textarea）
    let has_params = !draft.params.is_empty();
    // 基础生成器是否已定（新建选完 / 编辑锁定）：已定但零参数（自增 / UUID）如实说明，别误导成「还没选」
    let base_ready = draft.effective_config(cx).is_some();
    body = body.child(
        div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(section(
                &theme,
                if has_params {
                    "参数（来自基础生成器）"
                } else if base_ready {
                    "参数（该生成器没有参数）"
                } else {
                    "参数（选择基础生成器后出现）"
                },
            ))
            .when(has_params, |el| {
                el.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .w_full()
                        .children(draft.params.iter().map(|param| {
                            let key = param_key(param).to_string();
                            let label = {
                                let fields = draft
                                    .effective_config(cx)
                                    .map(|config| {
                                        generator_catalog::spec_of(&config)
                                            .params
                                            .iter()
                                            .copied()
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                fields
                                    .iter()
                                    .find(|f| f.key == param_key(param))
                                    .map(|f| f.label.to_string())
                                    .unwrap_or_else(|| param_key(param).to_string())
                            };
                            let state = state.clone();
                            let theme2 = theme.clone();
                            match param {
                                DraftParam::Scalar {
                                    state: input_state, ..
                                } => form_line(&theme2, &label, input_state).into_any_element(),
                                DraftParam::Complex {
                                    state: text_state, ..
                                } => {
                                    let hint = complex_param_hint(&key);
                                    complex_form_line(&theme2, &label, hint, text_state)
                                        .into_any_element()
                                }
                                DraftParam::Bool { value, .. } => {
                                    let value = *value;
                                    let key2 = key.clone();
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_2()
                                        .w_full()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_xs()
                                                .text_color(theme2.colors.muted_foreground)
                                                .child(label),
                                        )
                                        .child(
                                            Switch::new(format!("lib-param-{key2}"))
                                                .checked(value)
                                                .on_click(move |checked, _window, app| {
                                                    let checked = *checked;
                                                    state.update(app, |s, cx| {
                                                        if let Some(d) = s.create.as_mut() {
                                                            for p in &mut d.params {
                                                                if let DraftParam::Bool {
                                                                    key: pk,
                                                                    value: v,
                                                                } = p
                                                                {
                                                                    if *pk == key2 {
                                                                        *v = checked;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        s.refresh_preview(cx);
                                                    });
                                                }),
                                        )
                                        .into_any_element()
                                }
                            }
                        })),
                )
            }),
    );
    // 样例预览
    body = body.child(kv(
        &theme,
        "样例预览",
        div()
            .text_xs()
            .text_color(theme.colors.foreground)
            .child(draft.preview.clone()),
    ));
    // 错误行
    if let Some(error) = &draft.error {
        body = body.child(error_line(&theme, error.clone()));
    }
    body.into_any_element()
}