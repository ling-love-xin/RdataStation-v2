use super::*;

impl PropertyHostPanel {
    pub(crate) fn render_property_cell(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        row: &PropertyRow,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        // label 文案共享捕获：label_fn 约束是 `Fn`（Field 内部 Rc 存储、可多次调用），
        // 各分支 Rc::clone 一份进闭包，内部借用渲染（避免跨分支 move 冲突）。
        let label_text = Rc::new(row.label.clone());

        if !row.editable {
            // 只读：文本（DBeaver 只读属性）。
            let lt = Rc::clone(&label_text);
            return Field::new()
                .items_start()
                .label_fn(move |_: &mut Window, _: &mut App| {
                    div().text_xs().text_color(muted).child(lt.to_string())
                })
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(fg)
                        .child(row.value.clone()),
                );
        }

        match row.value_kind {
            PropertyValueKind::Boolean => {
                // 布尔：Switch（DBeaver：切换写 dirty 草稿，Apply 才提交）。
                // dirty 覆盖显示值：草稿态显示用户刚切的值，而非服务端原值。
                let dirty_val = self
                    .dirty
                    .borrow()
                    .iter()
                    .find(|(rid, _)| *rid == row.id)
                    .map(|(_, v)| v.clone());
                let checked = dirty_val.as_deref().unwrap_or(&row.value) == "true";
                let entity = entity.clone();
                let rid = row.id.clone();
                let lt = Rc::clone(&label_text);
                Field::new()
                    .items_start()
                    .label_fn(move |_: &mut Window, _: &mut App| {
                        div().text_xs().text_color(muted).child(lt.to_string())
                    })
                    .child(
                        div().w_full().child(
                            Switch::new(format!("prop-bool-{}", rid))
                                .checked(checked)
                                .on_click(move |new_val, _, cx| {
                                    entity.update(cx, |panel, cx| {
                                        panel.toggle_property_bool(rid.clone(), *new_val, cx);
                                    });
                                }),
                        ),
                    )
            }
            PropertyValueKind::Select if row.select_options.is_empty() => {
                // 空候选回退文本展示（provider 未给候选时不做空下拉）。
                let lt = Rc::clone(&label_text);
                Field::new()
                    .items_start()
                    .label_fn(move |_: &mut Window, _: &mut App| {
                        div().text_xs().text_color(muted).child(lt.to_string())
                    })
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_color(fg)
                            .child(row.value.clone()),
                    )
            }
            PropertyValueKind::Select => {
                // 常驻下拉（不经编辑按钮，选中即提交）：`use_keyed_state` 在渲染管线内
                // 惰性创建 SelectState（元素消失自动回收，无需手工清理）；Confirm 走公共
                // `run_property_edit` 后台 ALTER 路径（与 Boolean / 文本同一提交链）。
                let row_id = row.id.clone();
                let options = row.select_options.clone();
                let state = window.use_keyed_state(
                    format!("prop-select-{row_id}"),
                    cx,
                    move |window, cx| {
                        SelectState::new(SearchableVec::new(options), None, window, cx)
                    },
                );
                // 首次渲染该行挂订阅（按业务键查重，切对象时在 show_properties 注销）。
                if !self.select_sub_keys.borrow().contains(&row_id) {
                    self.select_sub_keys.borrow_mut().insert(row_id.clone());
                    let sub = cx.subscribe_in(
                        &state,
                        window,
                        move |panel,
                              _state,
                              event: &SelectEvent<SearchableVec<String>>,
                              _window,
                              cx| {
                            if let SelectEvent::Confirm(Some(value)) = event {
                                if let Some(target) = panel.property_target.borrow().clone() {
                                    panel.run_property_edit(
                                        target,
                                        row_id.clone(),
                                        value.to_string(),
                                        cx,
                                    );
                                }
                            }
                        },
                    );
                    self.select_subs.borrow_mut().push(sub);
                }
                // 渲染同步：模型值（编辑重载后变化）→ 控件选中；`set_selected_value` 不触发
                // Confirm（程序化写入不发事件），不会造成重复提交。
                if state.read(cx).selected_value().map(|v| v.as_str()) != Some(row.value.as_str()) {
                    let value = row.value.clone();
                    state.update(cx, |s, cx| {
                        s.set_selected_value(&value, window, cx);
                    });
                }
                let lt = Rc::clone(&label_text);
                Field::new()
                    .items_start()
                    .label_fn(move |_: &mut Window, _: &mut App| {
                        div().text_xs().text_color(muted).child(lt.to_string())
                    })
                    .child(
                        div()
                            .w_full()
                            .child(Select::new(&state).w_full().placeholder("选择…")),
                    )
            }
            _ => {
                let editing = self
                    .property_edit
                    .borrow()
                    .as_ref()
                    .map(|e| e.row_id == row.id)
                    .unwrap_or(false);
                if editing {
                    // 编辑中：输入框 + 保存 / 取消。
                    let edit = self.property_edit.borrow().clone().expect("编辑态已检查");
                    let save_entity = entity.clone();
                    let cancel_entity = entity.clone();
                    let input = Input::new(&edit.input)
                        .id(format!("prop-edit-{}", row.id))
                        .w_full();
                    let lt = Rc::clone(&label_text);
                    Field::new()
                        .items_start()
                        .label_fn(move |_: &mut Window, _: &mut App| {
                            div().text_xs().text_color(muted).child(lt.to_string())
                        })
                        .child(
                            div().w_full().v_flex().gap_1().child(input).child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Button::new(format!("prop-save-{}", row.id))
                                            .label("保存")
                                            .with_size(Size::XSmall)
                                            .on_click(move |_, _, app| {
                                                save_entity.update(app, |panel, cx| {
                                                    panel.submit_property_edit(cx);
                                                });
                                            }),
                                    )
                                    .child(
                                        Button::new(format!("prop-cancel-{}", row.id))
                                            .label("取消")
                                            .with_size(Size::XSmall)
                                            .on_click(move |_, _, app| {
                                                cancel_entity.update(app, |panel, cx| {
                                                    panel.cancel_property_edit(cx);
                                                });
                                            }),
                                    ),
                            ),
                        )
                } else {
                    // 未编辑：值 + 编辑按钮（DBeaver 点击可编辑值进入编辑）。
                    let entity = entity.clone();
                    let rid = row.id.clone();
                    let initial = row.value.clone();
                    let lt = Rc::clone(&label_text);
                    Field::new()
                        .items_start()
                        .label_fn(move |_: &mut Window, _: &mut App| {
                            div().text_xs().text_color(muted).child(lt.to_string())
                        })
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_color(fg)
                                        .child(row.value.clone()),
                                )
                                .child(
                                    Button::new(format!("prop-edit-btn-{}", row.id))
                                        .label("编辑")
                                        .with_size(Size::XSmall)
                                        .on_click(move |_, window, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.begin_property_edit(
                                                    rid.clone(),
                                                    initial.clone(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }),
                                ),
                        )
                }
            }
        }
    }
}
