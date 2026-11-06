//! 标签对话框：列表、三态勾选、新建与批量应用。

use super::*;

pub fn open_tag_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: TagDialogSeed,
    state: TagDialogState,
    name_input: Entity<InputState>,
    on_action: impl Fn(TagDialogEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    // 种子灌进状态：之后一切渲染与提交都只读这一个来源。
    if state.options().is_empty() {
        state.set_options(seed.options.clone());
        state.reset_batch(seed.selected.clone(), seed.partial.clone());
    }
    let on_action: Rc<dyn Fn(TagDialogEvent, &mut Window, &mut App)> = Rc::new(on_action);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let resource_name = seed.resource_name.clone();
    let target_count = seed.target_count;
    let batch = target_count > 1;
    // 标题：单选报名字（与其它对话框同一形态）；多选报**项数**（那是本次编辑真正的作用域）。
    let title = if batch {
        format!("标签 · {target_count} 项")
    } else {
        format!("标签 · {resource_name}")
    };

    // 新建（按钮或回车走同一条）：建完直接打上，对话框不关（宿主会把新词典推回来）。
    let dispatch_create = {
        let state = state.clone();
        let on_action = on_action.clone();
        let input = name_input.clone();
        let resource_name = resource_name.clone();
        move |window: &mut Window, cx: &mut App| {
            let name = input.read(cx).value().trim().to_string();
            if name.is_empty() || state.busy() || state.read_only() {
                return;
            }
            tracing::info!(
                tag = %name,
                target_count,
                resource = %resource_name,
                "【探针】标签对话框「新建并打标」提交"
            );
            state.set_busy(true);
            state.set_note(Some("正在新建…".to_string()));
            input.update(cx, |input, cx| input.set_value("", window, cx));
            on_action(TagDialogEvent::CreateAndTag { name }, window, cx);
        }
    };
    // 应用（按钮与回车共用）：只提交差集，没改动就什么都不做。
    let dispatch_apply = {
        let state = state.clone();
        let on_action = on_action.clone();
        let resource_name = resource_name.clone();
        move |window: &mut Window, cx: &mut App| {
            let (add, remove) = state.diff();
            if add.is_empty() && remove.is_empty() {
                return;
            }
            tracing::info!(
                add = add.len(),
                remove = remove.len(),
                target_count,
                resource = %resource_name,
                "【探针】标签对话框「应用」提交打标差集"
            );
            state.set_busy(true);
            state.set_note(Some("正在打标签…".to_string()));
            on_action(TagDialogEvent::Apply { add, remove }, window, cx);
        }
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let options = state.options();
        let busy = state.busy();
        let read_only = state.read_only();
        let note = state.note();
        let can_edit = !busy && !read_only;
        let selected_count = state.selected().len();
        let name_value = name_input.read(cx).value().trim().to_string();
        let can_create = can_edit && !name_value.is_empty();

        let mut list = div().v_flex().w_full();
        if options.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("还没有任何标签——在下面输入一个名字即可新建"),
            );
        }
        for (index, option) in options.iter().enumerate() {
            let checked = state.is_selected(&option.id);
            let partial = state.is_partial(&option.id);
            // 行点击与勾选框都走**三态**：部分 → 全有 → 无（多选下布尔翻转会误删，见 `promote_partial`）。
            let dispatch = {
                let state = state.clone();
                let tag_id = option.id.clone();
                move |_: &ClickEvent, window: &mut Window, _cx: &mut App| {
                    if state.is_partial(&tag_id) {
                        state.promote_partial(&tag_id);
                    } else {
                        state.toggle(&tag_id);
                    }
                    window.refresh();
                }
            };
            // 行内勾选框走真 `Checkbox`（与草稿选择对话框同一写法）：
            // 自绘 `✓` 字符既没有键盘焦点，也拿不到"勾上/未勾"的语义（a11y 只剩一个文本节点）。
            let check_dispatch = {
                let state = state.clone();
                let tag_id = option.id.clone();
                move |on: &bool, window: &mut Window, cx: &mut App| {
                    // 勾选框在行内：不让点击冒泡到行的 `on_click`（否则同一击翻转两次 = 没反应）。
                    cx.stop_propagation();
                    state.set_checked_three_way(&tag_id, *on);
                    window.refresh();
                }
            };
            // 行尾的「⋯」：改名 / 删除（v1 缺的两项，入口就在它们的词典里）。
            // 行尾的「⋯」：改名 / 删除（实现见 `menu.rs`）。
            let menu = menu::tag_menu(
                state.clone(),
                on_action.clone(),
                option.id.clone(),
                option.name.clone(),
                can_edit,
            );
            list = list.child(
                div()
                    .id(SharedString::from(format!("tag-choice-{}", option.id)))
                    .debug_selector({
                        let id = option.id.clone();
                        move || format!("tag-choice-{id}")
                    })
                    .when(can_edit, |row| row.cursor_pointer())
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    // 勾选底统一到 `list_active`（与面板 / 其它对话框的"选中底"同一个角色）：
                    // 对话框内不用 `accent.opacity(_)` 这类品牌淡色当状态底。
                    .when(checked, |row| row.bg(theme.colors.list_active))
                    // 悬停不覆盖勾选（导航侧 V11 口径）：勾上的行保持勾选底。
                    .when(can_edit && !checked, |row| {
                        row.hover(|s| s.bg(theme.colors.list_hover))
                    })
                    .when(can_edit, |row| row.on_click(dispatch))
                    .child(
                        Checkbox::new(SharedString::from(format!("tag-check-{}", option.id)))
                            .debug_selector({
                                let id = option.id.clone();
                                move || format!("tag-check-{id}")
                            })
                            .checked(checked)
                            .disabled(!can_edit)
                            .on_click(check_dispatch),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.foreground)
                            .child(option.name.clone()),
                    )
                    // 「部分」：只有一部分目标挂着（多选才有这个态）。`Checkbox` 没有半选态，
                    // 所以用一枚小字说清楚——它**不是**勾选，点一下才是「让所有目标都挂上」。
                    .when(partial, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child("部分"),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("{}", option.count)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("tag-more-{}", option.id)))
                            .ghost()
                            .xsmall()
                            .debug_selector({
                                let id = option.id.clone();
                                move || format!("tag-more-{id}")
                            })
                            .label("⋯")
                            .disabled(!can_edit)
                            .dropdown_menu(menu),
                    ),
            );
            let _ = index;
        }

        let mut body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("共 {} 个标签 · 已选 {selected_count} 个", options.len())),
                    )
                    .when_some(note, |bar, note| {
                        bar.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child(note),
                        )
                    }),
            )
            .child(
                list.max_h(rems(ui::TAG_LIST_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .into_any_element(),
            )
            // 新建行：输入 + 「新建并打上」（回车同一条路）。
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(Input::new(&name_input)))
                    .child(
                        Button::new("tag-create")
                            .small()
                            .debug_selector(|| "tag-create".to_string())
                            .label("新建并打上")
                            .disabled(!can_create)
                            .on_click({
                                let dispatch_create = dispatch_create.clone();
                                move |_, window, cx| dispatch_create(window, cx)
                            }),
                    ),
            );

        // 应用：只提交差集（回车与按钮同一条路）。
        let apply_dispatch = {
            let dispatch_apply = dispatch_apply.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                dispatch_apply(window, cx);
                window.close_dialog(cx);
            }
        };
        let (add_count, remove_count) = {
            let (add, remove) = state.diff();
            (add.len(), remove.len())
        };
        let has_changes = add_count + remove_count > 0;

        body = body.child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(if has_changes {
                    format!("本次改动：加 {add_count} 个 · 去 {remove_count} 个")
                } else {
                    "勾选标签后「应用」——改动在提交前不落库".to_string()
                }),
        );
        // 多选时把作用域与三态语义**写清楚**（不靠用户猜「部分」是什么意思）。
        if batch {
            body = body.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!(
                        "作用于选中的 {target_count} 项：勾选 = 全部打上，取消 = 全部摘掉；「部分」= 只有一部分项有它（点一下让全部都有）。"
                    )),
            );
        }

        dialog
            .title(title.clone())
            .w(cx.theme().font_size * ui::TAG_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("tag-close")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "tag-close".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("tag-apply")
                            .small()
                            .debug_selector(|| "tag-apply".to_string())
                            .label(if has_changes { "应用" } else { "关闭" })
                            .disabled(busy)
                            .on_click(apply_dispatch),
                    ),
            )
            .on_close({
                // `Rc` 不是 `Copy`：`Fn` 闭包（每帧重建）里要 clone 一份再 move 进去。
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_ok({
                // Enter = 应用（与主按钮同一条路）；新建走它自己的按钮。
                let dispatch_apply = dispatch_apply.clone();
                move |_, window, cx| {
                    dispatch_apply(window, cx);
                    true
                }
            })
            .on_cancel(|_, _, _| true)
    });
}
