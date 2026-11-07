//! 列编辑对话框：列名 / 类型 / 参数 / 空值率 / 唯一 / 恢复智能默认。

use super::*;

impl MockDetailView {
    /// 打开「列编辑」对话框（列名 / 类型 / 参数 / 空值率 / 唯一 / 恢复智能默认）。
    pub fn open_column_dialog(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        // 对话框的「应用」要落回这张 tab 的配置：先把配置 id 抓住，闭包里切活动指针
        let config_id = self.config_id();
        let Some(column) = self
            .panel
            .read(cx)
            .config_for(config_id.unwrap_or_default())
            .columns
            .iter()
            .find(|c| c.id == id)
            .cloned()
        else {
            return;
        };

        let null_initial = format!("{}", (column.def.nullable_ratio * 100.0).round() as i64);
        // 第三行左栏宽度沿用上次拖拽的结果（面板字段，仍受 220–520 约束）
        let list_width = px(self.panel.read(cx).column_list_px.clamp(ui::COLUMN_LIST_MIN, ui::COLUMN_LIST_MAX));
        let null_input = cx.new(|cx| InputState::new(window, cx).placeholder("0~100"));
        null_input.update(cx, |s, cx| s.set_value(null_initial.clone(), window, cx));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("列名"));
        name_input.update(cx, |s, cx| s.set_value(column.def.name.clone(), window, cx));
        // 打开时参数序列化快照（dirty 判定：输入框值逐一与初值比较）
        let original_params = generator_catalog::spec_of(&column.def.generator)
            .params
            .iter()
            .copied()
            .map(|field| match field.kind {
                ParamKind::Complex => complex_param_text(&column.def.generator, field.key),
                _ => param_text(&column.def.generator, field.key),
            })
            .collect::<Vec<String>>();

        self.draft = Some(ColumnDraft {
            id,
            name_input,
            type_label: column_type_label(&column.def.data_type),
            def: column.def.clone(),
            original: column.def.clone(),
            original_params,
            original_null: null_initial,
            generator_edited: false,
            recommendation: None,
            _name_sub: None,
            confidence: column.confidence.clone(),
            sample_value: column.sample_value.clone(),
            params: Vec::new(),
            null_input,
            param_error: None,
            sample_preview: String::new(),
            sample_round: 0,
            param_generator: None,
            // 打开时继承这一列原来选的自定义项（内建列 / 历史重放 → None）
            picked_user: column.user_generator.clone(),
        });
        // 列名失焦 → 智能重推（#4）：若用户未主动选过生成器，且新推荐与当前生成器不同，
        // 在属性区提示「建议改用 XX（可采纳）」，供一键重推
        let name_sub = self.draft.as_ref().unwrap().name_input.clone();
        let _name_sub = cx.subscribe_in(
            &name_sub,
            window,
            move |view, emitter, ev: &InputEvent, _window, cx| {
                if !matches!(ev, InputEvent::Blur) {
                    return;
                }
                let name = emitter.read(cx).value().trim().to_string();
                let Some(draft) = view.draft.as_mut() else {
                    return;
                };
                draft.recommendation = if draft.generator_edited || name.is_empty() {
                    None
                } else {
                    let mapped = ColumnMapper::infer(&name, &draft.def.data_type);
                    if mapped.generator == draft.def.generator {
                        None
                    } else {
                        Some(mapped.generator)
                    }
                };
                cx.notify();
            },
        );
        if let Some(draft) = self.draft.as_mut() {
            draft._name_sub = Some(_name_sub);
        }
        // 参数输入行按当前生成器建立（生成器身份变化后由 `rebuild_params` 重建）
        self.rebuild_params(window, cx);
        // 初始样例预览（按当前配置生成，事件路径，不在 render 里做）
        self.refresh_sample_preview(cx);
        // 生成器库浏览区内嵌（同面板）：分组/最近/搜索/精调直接开在列编辑对话框里，
        // 选中写回表配置（`LibraryWriteback::ColumnDraft`），点「应用」才落地
        let view = cx.entity();
        let initial_generator = self.draft.as_ref().map(|draft| draft.def.generator.clone());
        self.generator_browser = Some(crate::mock_view::generator_library::create_library_state(
            self.panel.clone(),
            crate::mock_view::generator_library::LibraryWriteback {
                detail: view.downgrade(),
                column_id: id,
            },
            true,
            initial_generator,
            window,
            cx,
        ));

        let view = cx.entity();
        let panel = self.panel.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let theme = cx.theme().clone();
            let Some(draft) = view.read(cx).draft.as_ref() else {
                return dialog.title("列编辑");
            };
            let name_state = draft.name_input.clone();
            let null_state = draft.null_input.clone();
            let type_label = draft.type_label.clone();
            let unique = draft.def.unique;
            // 参数行：标量单行 / 集合多行，标签与提示都从目录派生（不手写清单）
            let param_rows: Vec<ParamRowView> = draft
                .params
                .iter()
                .map(|p| {
                    let label = generator_catalog::spec_of(&draft.def.generator)
                        .params
                        .iter()
                        .find(|f| f.key == p.key)
                        .map(|f| f.label.to_string())
                        .unwrap_or_else(|| p.key.clone());
                    match &p.widget {
                        ParamWidget::Scalar(state) => ParamRowView::Scalar {
                            label,
                            state: state.clone(),
                        },
                        ParamWidget::Complex(state) => ParamRowView::Complex {
                            label,
                            hint: complex_param_hint(&p.key),
                            state: state.clone(),
                        },
                    }
                })
                .collect();
            let param_error = draft.param_error.clone();
            // 属性区只读展示（右栏「上属性」）：当前生成器名 + 其业务组（随左栏选中即时更新）。
            // 用户点的是**自定义项**时显示自定义名（`def.generator` 只有解包后的 base，
            // 直接显示 base 名会让人以为「点的没生效」）。业务组同理按自定义项自身挂的组。
            let (generator_label, category_label) = {
                let picked_ug = draft.picked_user().and_then(|id| {
                    let panel = view.read(cx).panel.clone();
                    let panel_ref = panel.read(cx);
                    panel_ref
                        .user_generators()
                        .iter()
                        .find(|ug| ug.id == id)
                        .cloned()
                });
                let spec = generator_catalog::spec_of(&draft.def.generator);
                match picked_ug {
                    Some(ug) => (
                        format!("{}（自定义）", ug.name),
                        ug.category
                            .map(|category| category.label().to_string())
                            .unwrap_or_else(|| spec.category.label().to_string()),
                    ),
                    None => (spec.label.to_string(), spec.category.label().to_string()),
                }
            };
            // 右栏滚动区 id：随选中生成器变化，换生成器时滚动位置自动回顶部
            let right_scroll_id = SharedString::from(format!("mock-right-scroll::{generator_label}"));
            // 样例预览克隆成值：draft 借用要在左右分栏渲染（需要 `&mut cx`）前结束
            let sample_preview = draft.sample_preview.clone();
            // 左栏生成器库实体（顶部搜索 + 业务/数据双 Tab 列表；选中写回表配置，
            // 见 `apply_picked_generator` 的 `ColumnDraft` 分支）
            let browser = view.read(cx).generator_browser.clone();

            // —— 1-7 优化：按当前表配置计算的提示态（纯读，不写状态）——
            // #2 产出类型不匹配：生成器实际产出 ≠ 所选类型（改类型不自动换生成器）→ 参数区顶部警示
            let actual_type = crate::generators::data_type_for_generator(&draft.def.generator);
            let type_mismatch = actual_type != draft.def.data_type;
            let actual_type_label = column_type_label(&actual_type);
            // 产出类型下拉闭包会 move `type_label`，警示行这里先克隆一份
            let type_label_for_warning = type_label.clone();
            // #4 列名失焦推荐（失焦时 `_name_sub` 订阅回填）：采纳按钮 = 按当前名 + 类型重推
            //（与「恢复智能默认」同口径），采纳后视为用户主动选定生成器，不再弹推荐
            let recommendation = draft.recommendation.clone();
            let recommendation_label = recommendation.as_ref().and_then(|cfg| {
                generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .find(|s| generator_catalog::default_of(s.name) == Some(cfg.clone()))
                    .map(|s| s.label.to_string())
            });
            // #5 零参生成器（如 UUID v4 / 布尔 / 时长）：参数区标题下显示空态文案
            let zero_param = generator_catalog::spec_of(&draft.def.generator).params.is_empty();

            // 右栏（用户定稿「上属性、下参数」）：属性区 = 生成器名 / 产出类型 / 业务组
            //（生成器身份只读展示；产出类型可改——改类型不自动换生成器，按新类型重推要点
            // 上方行 2 的「智能推荐」）；
            // 参数区 = 参数行 / 样例 / 空值率 / 唯一值。列名已移到窗口顶部。
            let mut right = div().v_flex().gap_2().w_full();
            // —— 属性 ——（分区标题与右 Dock 的三处段标题同一支原语：`section` 自带下缘线）
            right = right.child(section(&theme, "属性"));
            // 生成器名（只读）：点左栏列表换生成器（选中即写回表配置，属性 / 参数随之重建）
            right = right.child(kv(
                &theme,
                "生成器名",
                div()
                    .id("mock-col-generator")
                    .test_support()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(generator_label),
            ));
            // #4 列名失焦智能重推：提示已上移到行 2 的「智能推荐」块（与「历史生成器」并排），
            // 右栏属性区只留只读的生成器身份——同一提示不两处出现。
            // 产出类型（可改，下拉）
            right = right.child(
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
                            .child("产出类型"),
                    )
                    .child({
                        let view = view.clone();
                        dropdown_button("mock-col-type", type_label.clone())
                            .dropdown_menu(move |menu, _window, _cx| {
                                let mut menu = menu;
                                for candidate in COLUMN_TYPES {
                                    let label = column_type_label(&candidate);
                                    let is_current = label == type_label;
                                    let view = view.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(label).checked(is_current).on_click(
                                            move |_, _, app| {
                                                let chosen = candidate.clone();
                                                view.update(app, |view, cx| {
                                                    if let Some(draft) = view.draft.as_mut() {
                                                        draft.type_label =
                                                            column_type_label(&chosen);
                                                        draft.def.data_type = chosen;
                                                        draft.confidence = "manual".to_string();
                                                    }
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                menu
                            })
                    }),
            );
            // 业务组（只读）：生成器所属业务分类（左栏业务视角的组名）
            right = right.child(kv(
                &theme,
                "业务组",
                div()
                    .id("mock-col-category")
                    .test_support()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(category_label),
            ));
            // 注释（后期占位）：字段说明，先只读展示，下版加编辑 + 持久化
            right = right.child(kv(
                &theme,
                "注释",
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("（后期支持）"),
            ));
            // 类型与生成器解耦的说明：改类型不自动换生成器（避免参数行与生成器不一致）；
            // 想按新类型重推生成器 → 点上方「智能推荐」（行 2 左块）
            right = right.child(hint_line(
                &theme,
                "改类型不会自动换生成器；按新类型重推请点上方「智能推荐」。",
            ));
            // #4 列名失焦重推：提示摆在属性区（行 2 只有一个「智能推荐」按钮，提示放那里
            // 会把行 2 撑成两行）。点了即采纳，采纳后不再弹。
            if let (Some(cfg), Some(label)) = (&recommendation, &recommendation_label) {
                let view_adopt = view.clone();
                let cfg_for_click = cfg.clone();
                let label = label.clone();
                right = right.child(
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
                                .text_color(theme.colors.warning)
                                .child(format!("建议改用「{label}」")),
                        )
                        .child(
                            button("mock-col-adopt", "采纳", ActionKind::Quiet)
                                .xsmall()
                                .on_click(move |_, window, app| {
                                    view_adopt.update(app, |view, cx| {
                                        let Some(draft) = view.draft.as_mut() else {
                                            return;
                                        };
                                        draft.generator_edited = true;
                                        draft.recommendation = None;
                                        // 写回表配置：采纳推荐配置（列名已失焦重推，
                                        // 配置按当前名 + 类型）
                                        let mapped = ColumnMapper::infer(
                                            &draft.name_input.read(cx).value(),
                                            &draft.def.data_type,
                                        );
                                        draft.def.generator = cfg_for_click.clone();
                                        draft.confidence = mapped.confidence;
                                        draft.sample_value = mapped.sample_value;
                                        cx.defer_in(window, |view, window, cx| {
                                            view.rebuild_params(window, cx);
                                        });
                                        cx.notify();
                                    });
                                }),
                        ),
                );
            }
            // —— 参数 ——
            right = right.child(section(&theme, "参数"));
            // #2 产出类型不匹配警示：生成器实际产出与所选类型不一致时，参数区顶部提示
            //（改类型不自动换生成器是设计，这里把「不一致」讲明，指向更换生成器或恢复默认）
            if type_mismatch {
                right = right.child(
                    warning_line(
                        &theme,
                        format!(
                            "当前生成器产出 {actual_type_label}，与所选 {type_label_for_warning} 不匹配——请更换生成器或点上方「智能推荐」。"
                        ),
                    )
                    .id("mock-col-type-mismatch")
                    .test_support(),
                );
            }
            // #5 零参生成器：参数区没有表单行时显示空态文案（样例/空值率/唯一仍可用）
            if param_rows.is_empty() && zero_param {
                right = right.child(
                    hint_line(&theme, "该生成器无需参数。")
                        .id("mock-col-params-empty")
                        .test_support(),
                );
            }
            for row in param_rows {
                right = right.child(match row {
                    ParamRowView::Scalar { label, state } => form_line(&theme, &label, &state),
                    ParamRowView::Complex { label, hint, state } => {
                        complex_form_line(&theme, &label, hint, &state)
                    }
                });
            }
            if let Some((_, message)) = param_error {
                right = right.child(error_line(&theme, message));
            }
            // 参数实时样例预览（2 条示意值）：输入参数即改即看，固定种子可复现；
            // 也可以点「重新生成」手动重抽（用户要求样例自带生成按钮）
            right = right.child(
                div()
                    .v_flex()
                    .gap_1()
                    .w_full()
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
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("样例"),
                            )
                            .child({
                                let view = view.clone();
                                button(
                                    "mock-col-sample-refresh",
                                    "重新生成",
                                    ActionKind::Quiet,
                                )
                                .xsmall()
                                .tooltip("按当前参数重新抽样（固定种子，结果可复现）")
                                .tooltip_show_delay(TOOLTIP_DELAY)
                                .on_click(move |_, _window, app| {
                                    view.update(app, |view, cx| {
                                        // 批次 +1：换一批示意值（同一批次内仍然可复现）
                                        if let Some(draft) = view.draft.as_mut() {
                                            draft.bump_sample_round();
                                        }
                                        view.refresh_sample_preview(cx);
                                        cx.notify();
                                    });
                                })
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground)
                            .child(sample_preview),
                    ),
            );
            // 公共属性沉底一行：空值率 + 唯一值并排（两个全列通用的开关，不占太多垂直空间）
            right = right.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .child(
                        div().flex_1().min_w_0().child(form_line(&theme, "空值率（%）", &null_state))
                    )
                    .child(
                        div().flex_none().child(kv(
                            &theme,
                            "唯一值",
                            Switch::new("mock-col-unique").checked(unique).on_click({
                                let view = view.clone();
                                move |checked, _window, app| {
                                    let checked = *checked;
                                    view.update(app, |view, cx| {
                                        if let Some(draft) = view.draft.as_mut() {
                                            draft.def.unique = checked;
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                        ))
                    ),
            );

            // 窗口主体：三横行（用户定稿）——行 1「列名 + 搜索定位」、行 2「智能推荐 | 历史
            // 生成器」、行 3「左：业务分类/数据类型 + 右：属性·参数·样例」。整块定高
            // （`COLUMN_DIALOG_HEIGHT`），内容超高在各块内部滚动，不再由内容把对话框撑高；
            // 行 3 左右可拖（左列宽可调），右列永远跟左列选中的生成器走——左选右变，
            // 点「应用」才落地。
            // 分栏拖拽回写用的面板句柄：必须在每帧这里 clone（`open_dialog` 的 builder 是
            // `Fn`，外面捕获的 Entity 不能再 move 进内层 `'static` 回调）
            let col_split_panel = panel.clone();
            let body = div()
                .v_flex()
                .gap_2()
                .w_full()
                .h(ui::COLUMN_DIALOG_HEIGHT)
                .min_h_0()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .w_full()
                        // 列名（定宽）：本列名字，与搜索定位框同排
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .flex_none()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(text_hint(&theme))
                                        .child("列名"),
                                )
                                .child(
                                    div()
                                        .id("mock-col-name")
                                        .test_support()
                                        .w(ui::COLUMN_NAME_INPUT_WIDTH)
                                        .child(Input::new(&name_state)),
                                ),                        )
                        // 搜索定位框（跨剩余宽度，过滤左栏分组项）
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .when_some(browser.clone(), |col, b| {
                                    col.child(
                                        crate::mock_view::generator_library::render_search_box(
                                            &b, window, cx,
                                        ),
                                    )
                                }),
                        ),
                )
                // 顶部行与主体之间一条更轻的线：上半是「这一列叫什么」，
                // 下半是「它用什么造」，两块不是一回事
                .child(card_divider(&theme))
                // —— 行 2：智能推荐（左，一个按钮） | 历史生成器（右，一行 chips） ——
                // 左边是「按这一列的名字和类型该用什么」（推导），右边是「这个会话里已经用过
                // 什么」（回访）。两块都压成**单行**：行 2 高度必须可预测，否则会把行 3 的
                // 库挤矮；chips 不换行（见 `render_history_chips`）。
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .flex_none()
                        .child(if let Some(b) = browser.as_ref() {
                            crate::mock_view::generator_library::render_smart_button(
                                b, &theme, window, cx,
                            )
                        } else {
                            div().into_any_element()
                        })
                        .child(Separator::vertical().color(theme.colors.border))
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("历史"),
                        )
                        .child(if let Some(b) = browser.as_ref() {
                            crate::mock_view::generator_library::render_history_chips(
                                b, &theme, window, cx,
                            )
                        } else {
                            div().into_any_element()
                        }),
                )
                .child(card_divider(&theme))
                // —— 行 3：左 = 库（业务分类下的生成器 / 数据类型下的方法），
                //           右 = 这一列的属性 · 参数 · 样例（随左列选中即时重建） ——
                // 左右可拖（左列宽 220~520，右列吃剩余），两栏各自内部滚动：展开分组、
                // 切 Tab 只让库那一栏滚，不会把对话框拉高
                .child(
                    div().flex_1().min_h_0().w_full().child(
                        h_resizable("mock-col-split")
                            .child(
                                resizable_panel()
                                    .size(list_width)
                                    .size_range(px(ui::COLUMN_LIST_MIN)..px(ui::COLUMN_LIST_MAX))
                                    .child(
                                        div()
                                            .size_full()
                                            .min_h_0()
                                            .p_2()
                                            .overflow_y_scrollbar()
                                            .when_some(browser, |col, b| {
                                                col.child(
                                                    crate::mock_view::generator_library::render_browse_pane(
                                                        &b, &theme, window, cx,
                                                    ),
                                                )
                                            }),
                                    ),
                            )
                            .child(
                                resizable_panel().flex_basis(ui::FLEX_BASIS_ZERO).child(
                                    div()
                                        .size_full()
                                        .min_h_0()
                                        .p_2()
                                        .overflow_y_scrollbar()
                                        .id(right_scroll_id)
                                        .child(right),
                                ),
                            )
                            // 拖到哪记到哪：左栏宽度存面板字段，重开对话框沿用（不是「关掉就回 280」）
                            .on_resize(move |state, _window, app| {
                                if let Some(width) = state.read(app).sizes().first().copied() {
                                    col_split_panel.update(app, |panel, cx| {
                                        panel.column_list_px = f32::from(width).clamp(220., 520.);
                                        cx.notify();
                                    });
                                }
                            })
                    )
                );

            let view_apply = view.clone();
            let view_cancel = view.clone();
            let view_cancel_esc = view.clone();
            let view_next = view.clone();
            let panel_apply = panel.clone();
            dialog
                .title("列编辑")
                .width(ui::COLUMN_DIALOG_WIDTH)
                .child(body)
                // #6 ESC / 遮罩关闭拦截：有未应用修改 → 弹确认框并阻止本次关闭
                .on_cancel(move |_event, window, app| {
                    let dirty = view_cancel_esc
                        .read(app)
                        .draft
                        .as_ref()
                        .map(|d| Self::draft_is_dirty(d, app))
                        .unwrap_or(false);
                    if dirty {
                        Self::confirm_discard(&view_cancel_esc, window, app);
                        false
                    } else {
                        true
                    }
                })
                .footer(
                DialogFooter::new()
                    .child(
                        button("mock-col-cancel", "取消", ActionKind::Soft)
                            .on_click(move |_, window, app| {
                                let dirty = view_cancel
                                    .read(app)
                                    .draft
                                    .as_ref()
                                    .map(|d| Self::draft_is_dirty(d, app))
                                    .unwrap_or(false);
                                if dirty {
                                    // 有未应用修改：弹确认框，确认后才真正清表配置并关闭
                                    Self::confirm_discard(&view_cancel, window, app);
                                } else {
                                    view_cancel.update(app, |view, _cx| view.draft = None);
                                    window.close_dialog(app);
                                }
                            }),
                    )
                    .child(
                        button("mock-col-apply", "应用", ActionKind::Main)
                            .on_click(move |_, window, app| {
                                let edited =
                                    view_apply.update(app, |view, cx| view.collect_column_edit(cx));
                                if let Some(edited) = edited {
                                    panel_apply.update(app, |panel, cx| {
                                        if let Some(id) = config_id {
                                            panel.focus_config(id);
                                        }
                                        panel.apply_column(edited, cx)
                                    });
                                }
                                window.close_dialog(app);
                            }),
                    )
                    // #7 应用并新增下一列：应用当前表配置 → 关对话框 → 按当前列名 +2 去重新建列
                    //（沿用 §8.3 字段预设命名策略）→ 立即打开新列的列编辑对话框（连续录入）
                    .child(
                        button(
                            "mock-col-apply-next",
                            "应用并新增下一列",
                            ActionKind::Soft,
                        )
                            .on_click(move |_, window, app| {
                                window.close_dialog(app);
                                view_next.update(app, |view, cx| view.apply_and_next(window, cx));
                            }),
                    ),
            )
        });
    }

    /// 按**当前列名**（输入框里的值）+ 类型重推这一列的生成器配置（左栏「智能推荐」）。
    ///
    /// 与右栏的「采纳推荐」（只换生成器、保留参数）不同：这里整套重来
    /// （生成器 + 参数 + 空值率 + 唯一值都回到推荐值）——它是「让这一列回到推断结果」。
    pub(crate) fn smart_reset(
        &mut self,
        column_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        if draft.id != column_id {
            return;
        }
        // 按输入框当前值（用户可能刚改过列名还没应用），不按打开对话框时的快照
        let name = draft.name_input.read(cx).value().to_string();
        let mapped = ColumnMapper::infer(&name, &draft.def.data_type);
        draft.def.generator = mapped.generator;
        draft.def.nullable_ratio = 0.0;
        draft.def.unique = false;
        draft.confidence = mapped.confidence;
        draft.sample_value = mapped.sample_value;
        // 智能推荐 = 用户主动选定生成器，之后列名失焦不再弹推荐；
        // 它给的是内建推断结果，所以自定义项身份一并清掉（否则左栏还亮着原来那一项）
        draft.generator_edited = true;
        draft.picked_user = None;
        draft.recommendation = None;
        draft.param_error = None;
        // 生成器身份可能变了：参数行按新生成器重建（事件路径，不能在 render 做）
        cx.defer_in(window, |view, window, cx| {
            view.rebuild_params(window, cx);
        });
        cx.notify();
    }

    /// 按当前生成器重建参数输入行（保留同名参数的输入框与订阅）。
    /// 列编辑对话框内选了生成器（库对话框写回表配置，`LibraryWriteback::ColumnDraft`）：
    /// 写 draft 的生成器配置并重建参数区 / 样例预览。列还没应用，面板列不动——
    /// 与应用按钮同口径（改动留在表配置，点「应用」才落地）。
    pub(crate) fn apply_generator_config_to_draft(
        &mut self,
        column_id: u64,
        config: GeneratorConfig,
        picked_user: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        if draft.id != column_id {
            return;
        }
        draft.def.generator = config;
        // 选择新生成器后，自动同步产出类型为该生成器的产出类型
        draft.def.data_type = crate::generators::data_type_for_generator(&draft.def.generator);
        // 「点的是哪一项」：内建 = None（按 base 名认领即可），自定义 = 那一项的 id
        // （否则同一 base 的多个自定义项在左栏会一起高亮、右栏只能显示 base 名）
        draft.picked_user = picked_user;
        draft.confidence = "manual".to_string();
        // #4：左栏选中 = 用户主动选定生成器，之后列名失焦不再弹推荐
        draft.generator_edited = true;
        draft.recommendation = None;
        draft.param_error = None;
        self.rebuild_params(window, cx);
        self.refresh_sample_preview(cx);
        cx.notify();
    }
}