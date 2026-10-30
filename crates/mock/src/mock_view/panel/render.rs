// 继承父模块（panel）与 mock_view 的全部 use 绑定
use super::*;

impl MockPanel {
    /// 表头第一行：表名 / 行数 / 种子 / 语言（草稿可改）。
    fn render_target_rows(&self, cx: &mut Context<Self>) -> Div {
        let mut row = div().h_flex().items_center().gap_2().w_full();
        if let Some(input) = self.table_input.clone() {
            row = row.child(div().flex_1().min_w_0().child(Input::new(&input)));
        }
        let mut nums = div().h_flex().items_center().gap_2().w_full();
        if let Some(input) = self.rows_input.clone() {
            nums = nums.child(
                div()
                    .w(rems(ui::NUM_INPUT_WIDTH))
                    .flex_none()
                    .child(Input::new(&input)),
            );
        }
        if let Some(input) = self.seed_input.clone() {
            nums = nums.child(
                div()
                    .w(rems(ui::NUM_INPUT_WIDTH))
                    .flex_none()
                    .child(Input::new(&input)),
            );
        }
        let locale = self.draft.options.locale.clone();
        nums = nums.child({
            let entity = cx.entity();
            Button::new("mock-locale")
                .ghost()
                .label(locale_label(&locale))
                // 组件自带下拉 caret（不再手拼「▾」字形）；`dropdown_caret` 与 `dropdown_menu`
                // 是两个独立字段——开 caret 只画箭头，菜单照旧由 `dropdown_menu` 挂
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for candidate in LOCALES {
                        let chosen = candidate.clone();
                        let is_current = chosen == locale;
                        let entity = entity.clone();
                        menu = menu.item(
                            PopupMenuItem::new(locale_label(&chosen))
                                .checked(is_current)
                                .on_click(move |_, _, app| {
                                    let chosen = chosen.clone();
                                    entity.update(app, |panel, cx| {
                                        panel.draft.options.locale = chosen;
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                })
        });
        div().v_flex().gap_1().w_full().child(row).child(nums)
    }

    /// 表头第二行：单表生成（三态）+ 场景模板菜单；有结果时尾部带一行结果摘要。
    ///
    /// 三态是同一个动作的不同阶段：无结果 → 主按钮「生成」；有结果 → 收成小按钮「重新生成」
    /// （避免“要不要重跑”的分歧）；进行中 → 禁用「生成中…」。
    fn render_generate_row(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let running = self.is_running();
        let generating = self.is_generating();
        let has_result = self.has_result();

        let generate = {
            let entity = cx.entity();
            let mut button = if has_result {
                Button::new("mock-detail-generate")
                    .secondary()
                    .xsmall()
                    .label("重新生成")
            } else {
                Button::new("mock-detail-generate")
                    .primary()
                    .label(if generating { "生成中…" } else { "生成" })
            };
            if !running {
                button = button.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.run_generate(cx));
                });
            }
            button
        };

        // 场景模板（内置 6 套多表）：同样是生成类任务，进行中一并禁用
        let scenario = self.render_scenario_button("mock-detail-scenario", cx);

        let summary = self.current_info().map(|info| {
            format!(
                "临时表 {} · {} 行 · {} ms",
                info.temp_table_name,
                with_thousands(u64::from(info.row_count)),
                info.elapsed_ms
            )
        });

        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(generate)
            .child(scenario)
            .children(summary.map(|text| {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(text)
            }))
    }

    /// 这张表的**设计与生成状态**（画在中央 tab 的表头，决策 D38）。
    ///
    /// 为何在中央：表名 / 行数 / 种子 / 语言与列定义同属「这张表怎么造」，进度 / 取消 / 失败
    /// 与重试从属于同一次生成——它们属于这张表，不属于「有哪些表」那份清单（那是右 Dock 的事）。
    pub(crate) fn render_design_head(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.ensure_inputs(window, cx);
        let muted = cx.theme().colors.muted_foreground;
        let danger = cx.theme().colors.danger;
        let running = self.is_running();
        let has_result = self.has_result();

        let mut head = div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(self.render_target_rows(cx));

        if self.scenario.is_some() {
            // 场景态：单表生成让位给「生成 N 张表」（在右 Dock），这里只说明草稿的用途
            head =
                head.child(div().text_xs().text_color(muted).child(
                    "这是草稿：把表名 / 行数 / 列调好，再去右侧「＋ 加表」把它加进本次生成",
                ));
        } else {
            head = head.child(self.render_generate_row(cx));
        }

        // 单表任务的进度与取消（集合任务的在右 Dock 场景清单下，见 D38）
        head = head.child(self.render_job_row(JobRowScope::Single, cx));

        // 失败原因就地给 + 重试：只认“没有结果”的失败（生成类）——出口类失败有结果，
        // 属于出口按钮那段（反馈也就地给在那边）
        if let Some(err) = self.error.clone().filter(|_| !has_result) {
            let retry = {
                let entity = cx.entity();
                let mut button = Button::new("mock-detail-retry")
                    .secondary()
                    .xsmall()
                    .label("重试");
                if !running {
                    button = button.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| panel.run_generate(cx));
                    });
                }
                button
            };
            head = head.child(
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
                            .text_color(danger)
                            .text_ellipsis()
                            .child(format!("生成失败：{err}")),
                    )
                    .child(retry),
            );
        }
        head
    }

    /// 任务进行中的进度行：`Progress` 组件 + 阶段文案 + 取消按钮。
    ///
    /// - 生成类有批次粒度回调 → 定量进度条 + 取消（引擎在批次边界响应）；
    /// - 出口类（写入分析库 / 写文件）跑在 DuckDB 与文件系统内部，拿不到中间进度 →
    ///   不定量动画；也不给取消（强中断会留下半张表 / 半个文件）。
    ///
    /// 空闲时返回一个空占位（保持后续元素的排列稳定）。
    fn render_job_row(&mut self, scope: JobRowScope, cx: &mut Context<Self>) -> Div {
        if self.job_row_scope() != Some(scope) {
            return div();
        }
        let Some(job) = self.job.as_ref() else {
            return div();
        };
        // 量纲即归属（D38）：集合任务（场景 / 批量落库）画在右 Dock 场景清单下，
        // 单表任务（生成 / 追加 / 三个出口）画在中央这张表的表头。
        let progress = job.progress;
        let cancellable = job.kind.generates();
        let cancel_requested = job.cancel_requested;
        let by_table_detail = job.kind.by_table();

        let muted = cx.theme().colors.muted_foreground;
        let detail = match progress.phase {
            MockJobPhase::Generating if progress.batches_total == 0 => "准备中…".to_string(),
            MockJobPhase::Generating if by_table_detail => format!(
                "{} / {} 张表（每张表逐个处理）",
                progress.batches_done, progress.batches_total
            ),
            MockJobPhase::Generating => format!(
                "{} / {} 批（≈{} / {} 行）",
                progress.batches_done,
                progress.batches_total,
                with_thousands(progress.rows_done() as u64),
                with_thousands(progress.rows_total as u64)
            ),
            phase if by_table_detail => format!(
                "{}…（{} / {} 张表）",
                phase.label(),
                progress.batches_done,
                progress.batches_total
            ),
            phase => format!(
                "{}…（{} 行）",
                phase.label(),
                with_thousands(progress.rows_total as u64)
            ),
        };
        let cancel =
            cancellable.then(|| {
                let entity = cx.entity();
                let mut button = Button::new("mock-cancel-job").secondary().xsmall().label(
                    if cancel_requested {
                        "正在取消…"
                    } else {
                        "取消"
                    },
                );
                if !cancel_requested {
                    button = button.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| panel.cancel_job(cx));
                    });
                }
                button
            });

        div()
            .v_flex()
            .gap_1()
            .w_full()
            .child(
                Progress::new("mock-job-progress")
                    .w_full()
                    .loading(!progress.phase.is_quantified())
                    .value(progress.percent()),
            )
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
                            .text_color(muted)
                            .text_ellipsis()
                            .child(detail),
                    )
                    .children(cancel),
            )
    }

    /// 场景模板菜单按钮（中央表头的生成行与空态卡片共用；`id` 分开是避免同帧撞 id）。
    pub(crate) fn render_scenario_button(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = self.is_running();
        let templates = self.scenario_templates.clone();
        let entity = cx.entity();
        Button::new(id)
            .secondary()
            .xsmall()
            .label("场景模板")
            .dropdown_caret(true)
            .disabled(running)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if templates.is_empty() {
                    return menu.item(PopupMenuItem::new("（没有可用的场景模板）").disabled(true));
                }
                for choice in templates.iter() {
                    let id = choice.id.clone();
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(choice.menu_label()).on_click(
                        move |_, _, app| {
                            let id = id.clone();
                            entity.update(app, |panel, cx| panel.open_scenario(&id, cx));
                        },
                    ));
                }
                menu
            })
            .into_any_element()
    }

    /// 预览标题行右侧的「行数」菜单（预览要显示多少行）。
    ///
    /// 为何是固定档而不是输入框：行数是**重查代价**的旋钮（`LIMIT N`），档位够用就不必解析输入、
    /// 也不必管非法值；同时把「预览多少行」放到看得见的地方（生成时只读 [`PREVIEW_ROWS`] 行，
    /// 想要更多是**重查**临时表，不是就地多显示几行）。
    pub(crate) fn render_preview_limit_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let current = self.preview_limit;
        Button::new("mock-preview-limit")
            .ghost()
            .xsmall()
            .label(format!("行数 {current}"))
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for choice in PREVIEW_LIMIT_CHOICES {
                    let entity = entity.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format!("{choice} 行"))
                            .checked(choice == current)
                            .on_click(move |_, _, app| {
                                entity.update(app, |panel, cx| panel.set_preview_limit(choice, cx));
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// 列来源按钮（导入源库结构 / 手工加列）：它们属于「这张表的列」→ 画在中央 tab 的「列」段。
    pub(crate) fn render_column_source_buttons(&self, cx: &mut Context<Self>) -> Div {
        let import = {
            let entity = cx.entity();
            Button::new("mock-import-schema")
                .secondary()
                .xsmall()
                .label("导入结构")
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.open_import_dialog(window, cx);
                    });
                })
        };
        let add = {
            let entity = cx.entity();
            Button::new("mock-add-column")
                .secondary()
                .xsmall()
                // 「＋」走组件图标（与工具栏「新增 Mock 表」同一 `Plus`），不手拼字符
                .icon(IconName::Plus)
                .label("加列")
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| {
                        let index = panel.draft.columns.len() + 1;
                        panel.add_column(format!("column_{index}"), ColumnDataType::Integer, cx);
                    });
                })
        };
        // 字段预设：常用字段（电话/地址/姓名等）一行打包，从分组菜单一键加列。
        // 与「导入结构」（源库批量）「加列」（手工起手）三分入口；应用后是普通列可再编辑
        let presets = {
            let entity = cx.entity();
            Button::new("mock-preset-column")
                .secondary()
                .xsmall()
                .icon(IconName::Folder)
                .label("预设")
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for category in PresetCategory::ALL {
                        let items: Vec<FieldPreset> = builtin_presets()
                            .into_iter()
                            .filter(|p| p.category == category)
                            .collect();
                        if items.is_empty() {
                            continue;
                        }
                        // 组标题：disabled 只读行，不参与点击
                        menu = menu.item(PopupMenuItem::new(category.label()).disabled(true));
                        for preset in items {
                            let entity = entity.clone();
                            let id = preset.id;
                            menu = menu.item(
                                PopupMenuItem::new(preset.label)
                                    .on_click(move |_, _, app| {
                                        let Some(preset) = preset_by_id(id) else {
                                            return;
                                        };
                                        entity.update(app, |panel, cx| {
                                            panel.add_preset_column(&preset, cx);
                                        });
                                    }),
                            );
                        }
                    }
                    menu
                })
        };
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(import)
            .child(presets)
            .child(add)
    }

    /// 表清单：**唯一的管理入口**（D38）——单表 / 本次生成 / 结果表三态共用一个渲染器。
    ///
    /// 行 = 状态点 + 表名 + 行数；点一行 = 切 / 开那张表的中央 tab（选中行就是当前表）。
    /// 关系挂在**子表**行下的 `↳` 子行（真身是列上的 `dependency`），不再单开一段。
    fn render_table_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let warning = cx.theme().colors.warning;
        let primary = cx.theme().colors.primary;
        let danger = cx.theme().colors.danger;
        // 选中 / 悬停走列表角色（与 M4 导航、M5 草稿箱同一口径）：`accent` 是主题强调色，
        // 拿它当选中底会让「当前表」与各種强调元素混作一谈
        let selected_bg = cx.theme().colors.list_active;
        let selected_border = cx.theme().colors.list_active_border;
        let hover = cx.theme().colors.list_hover;
        let running = self.is_running();
        let radius = cx.theme().radius;

        let mut list = div().v_flex().gap_1().w_full();

        // 清单搜索框：按表名过滤行（与原型「搜索表名…」对齐；空搜索 = 全显示）。
        // 搜索词只影响显示，不影响下标语义（index 仍是 results 里的真实位置）。
        let search = self.search_input.clone();
        list = list.child(
            div()
                .w_full()
                .child(search.map(|input| div().child(Input::new(&input))).unwrap_or_else(|| div())),
        );
        let query = self
            .search_input
            .clone()
            .map(|input| input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();

        // ── 结果表：每张结果表一个中央 tab；状态点说「落了没 / 缺不缺父表」──
        if !self.results.is_empty() {
            let count = self.results.len();
            let current_index = self.current.min(count.saturating_sub(1));
            list = list.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("结果表（{count}）· 点一行切到它的 tab")),
            );
            for (index, info) in self.results.iter().enumerate() {
                if !query.is_empty() && !info.table_name.to_lowercase().contains(&query) {
                    continue;
                }
                let selected = index == current_index;
                let name = info.table_name.clone();
                let (icon, status, color) = match self.table_status(&name) {
                    TableStatus::Persisted => {
                        (Some(IconName::CircleCheck), "已落库".to_string(), success)
                    }
                    TableStatus::NotPersisted => {
                        (Some(IconName::Check), "已生成".to_string(), success)
                    }
                    TableStatus::DanglingParent => {
                        (Some(IconName::Info), "引用的表未落库".to_string(), warning)
                    }
                    // 生成中的那一行把百分比说出口（D38 的 `◐ orders 40%`）：
                    // 集合任务不敢保证看到的是哪一张，但单表任务停在哪个百分位一眼就能读
                    TableStatus::Generating => (
                        Some(IconName::RefreshCw),
                        format!("生成中 {:.0}%", self.job_percent()),
                        primary,
                    ),
                    TableStatus::Failed => {
                        (Some(IconName::CircleX), "生成失败".to_string(), danger)
                    }
                    TableStatus::Idle => (None, "未生成".to_string(), muted),
                };
                let open = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    // 键盘可达：行内左对齐文本不宜换 `Button`（其内容容器固定 `justify_center`，
                    // 换上去会破坏左对齐与选中加粗），按 skill 的自绘下限补焦点——
                    // `use_keyed_state` 每行一个独立焦点句柄（与 Button 同款），Enter/Space 激活。
                    // id 必须是稳定业务键（表名），不能随选中态变，否则一换选中就丢焦点。
                    let open_handle = window
                        .use_keyed_state(
                            ElementId::Name(SharedString::from(format!(
                                "mock-result-open-{click_name}"
                            ))),
                            cx,
                            |_, cx| cx.focus_handle(),
                        )
                        .read(cx)
                        .clone();
                    let entity_kb = entity.clone();
                    let click_kb = name.clone();
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "mock-result-open-{click_name}"
                        ))))
                        .role(gpui::Role::Button)
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .font_weight(if selected {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_ellipsis()
                        .cursor_pointer()
                        .track_focus(&open_handle)
                        .tab_stop(true)
                        .child(name.clone())
                        .on_click(move |_, window, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.select_result(index, cx);
                                panel.open_table_detail(&name, window, cx);
                            });
                        })
                        .on_key_down(move |event: &KeyDownEvent, window, app| {
                            // Enter / Space 与点击同一动作（gpui 可点元素在 KeyUp 激活，
                            // 这里在 KeyDown 直接触发同一入口，行为与 Button 的键盘点击等价）
                            if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                                let name = click_kb.clone();
                                entity_kb.update(app, |panel, cx| {
                                    panel.select_result(index, cx);
                                    panel.open_table_detail(&name, window, cx);
                                });
                            }
                        })
                };
                let mut row = div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .h(rems(ui::ROW_HEIGHT))
                    .px_1()
                    .rounded(radius)
                    .relative()
                    .child(open)
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} 行", with_thousands(u64::from(info.row_count)))),
                    )
                    .child(render_status_cell(icon, &status, color));
                // 选中行：`list_active` 底 + 左缘 2px 标识条（浮在行左缘，不挤动名称列）；
                // 悬停只给未选中行——不得覆盖激活态（与导航树同一口径）
                row = if selected {
                    row.bg(selected_bg).child(tree::active_bar(selected_border))
                } else {
                    row.hover(move |s| s.bg(hover))
                };
                list = list.child(row);

                // 关系挂在**子表**行下：`↳ user_id → users.id · 1..1,000`
                for relation in self.outgoing_relations(&name).iter() {
                    let range = self.relation_range(relation);
                    list = list.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .pl_4()
                            .text_xs()
                            .text_color(muted)
                            .child(div().flex_1().min_w_0().text_ellipsis().child(format!(
                                "↳ {} → {}.{}",
                                relation.child_column,
                                relation.parent_table,
                                relation.parent_column
                            )))
                            .children(range.map(|range| div().flex_none().child(range))),
                    );
                }
            }
            return list;
        }

        // ── 本次 Mock 表：场景工作副本（还没有结果）——行尾「编辑 / ✕」，关系挂子表行下 ──
        if let Some(template) = self.scenario.clone() {
            // 空白场景（0 张表）用原型专门的引导文案，别让「本次 Mock 表（0）」空着不知所云
            let heading = if template.tables.is_empty() {
                "空白场景 · 0 张表——从「＋ 加表」开始".to_string()
            } else {
                format!(
                    "本次 Mock 表（{}）· {}",
                    template.tables.len(),
                    template.name
                )
            };
            list = list.child(
                div().text_xs().text_color(muted).text_ellipsis().child(heading),
            );
            let relations = relations_of(&template);
            for table in template.tables.iter() {
                if !query.is_empty() && !table.name.to_lowercase().contains(&query) {
                    continue;
                }
                let name = table.name.clone();
                let edit = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    let mut button = Button::new(ElementId::Name(SharedString::from(format!(
                        "mock-scenario-table-edit-{click_name}"
                    ))))
                    .ghost()
                    .xsmall()
                    .label("编辑")
                    .disabled(running);
                    if !running {
                        button = button.on_click(move |_, window, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_table_dialog(&name, window, cx);
                            });
                        });
                    }
                    button
                };
                let remove = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    let mut button = Button::new(ElementId::Name(SharedString::from(format!(
                        "mock-scenario-table-remove-{click_name}"
                    ))))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("从本次生成移除这张表")
                    .disabled(running);
                    if !running {
                        button = button.on_click(move |_, _, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.remove_scenario_table(&name, cx);
                            });
                        });
                    }
                    button
                };
                list =
                    list.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .h(rems(ui::ROW_HEIGHT))
                            .px_1()
                            .rounded(radius)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_ellipsis()
                                    .child(name.clone()),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(format!(
                                "{} 行",
                                with_thousands(u64::from(table.row_count))
                            )))
                            .child(render_status_cell(None, "待生成", muted))
                            .child(edit)
                            .child(remove),
                    );

                // 关系挂在**子表**行下（行尾 ✕ 删关系）
                for relation in relations.iter().filter(|r| r.child_table == name) {
                    let range = self.relation_range(relation);
                    let remove_relation = {
                        let entity = cx.entity();
                        let child_table = relation.child_table.clone();
                        let child_column = relation.child_column.clone();
                        Button::new(ElementId::Name(SharedString::from(format!(
                            "mock-relation-remove-{}-{}",
                            child_table, child_column
                        ))))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("删除这条关系")
                        .on_click(move |_, _, app| {
                            let child_table = child_table.clone();
                            let child_column = child_column.clone();
                            entity.update(app, |panel, cx| {
                                panel.remove_relation(&child_table, &child_column, cx);
                            });
                        })
                    };
                    list = list.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .pl_4()
                            .text_xs()
                            .text_color(muted)
                            .child(div().flex_1().min_w_0().text_ellipsis().child(format!(
                                "↳ {} → {}.{}",
                                relation.child_column,
                                relation.parent_table,
                                relation.parent_column
                            )))
                            .children(range.map(|range| div().flex_none().child(range)))
                            .child(remove_relation),
                    );
                }
            }

            // 自定义多表：把当前草稿加进来（导入结构 / 列编辑都在单表态里做完）
            let add_table = {
                let entity = cx.entity();
                let mut button = Button::new("mock-scenario-table-add")
                    .secondary()
                    .xsmall()
                    .label("＋ 加表（当前草稿）")
                    .disabled(running);
                if !running {
                    button = button.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.add_draft_to_scenario(cx);
                        });
                    });
                }
                button
            };
            let add_relation = {
                let entity = cx.entity();
                let mut button = Button::new("mock-add-relation")
                    .secondary()
                    .xsmall()
                    .label("＋ 加关系")
                    .disabled(running);
                if !running {
                    button = button.on_click(move |_, window, app| {
                        entity.update(app, |panel, cx| {
                            panel.open_relation_dialog(window, cx);
                        });
                    });
                }
                button
            };
            list = list
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(add_table)
                        .child(add_relation),
                )
                .child(div().text_xs().text_color(muted).child(
                    "关系只在本次多表生成内成立（不读已落数据、不改已生成的表 / 文件）；行尾是取值域：父列自增参数 × 父表行数，改父表行数它跟着变。",
                ))
                .child(div().text_xs().text_color(muted).child(
                    "加进来的表取草稿的表名 / 行数 / 列：先在中央那张草稿 tab 里调好，再加进本次生成。",
                ));
            return list;
        }

        // ── 单表态：草稿（与它的结果）就这一行 ──
        // 标题行对齐原型清单头「本次 Mock 表（1）」（结果态走上面的「结果表（N）」分支）
        list = list.child(
            div()
                .text_xs()
                .text_color(muted)
                .text_ellipsis()
                .child("本次 Mock 表（1）"),
        );
        let name = self.draft.table_name.clone();
        let rows = with_thousands(self.draft.options.rows as u64);
        let (icon, status, color) = match self.table_status(&name) {
            TableStatus::Generating => (
                Some(IconName::RefreshCw),
                format!("生成中 {:.0}%", self.job_percent()),
                primary,
            ),
            TableStatus::Persisted => {
                (Some(IconName::CircleCheck), "已落库".to_string(), success)
            }
            TableStatus::NotPersisted => (Some(IconName::Check), "已生成".to_string(), success),
            TableStatus::DanglingParent => {
                (Some(IconName::Info), "引用的表未落库".to_string(), warning)
            }
            TableStatus::Failed => (Some(IconName::CircleX), "生成失败".to_string(), danger),
            TableStatus::Idle => (None, "未生成".to_string(), muted),
        };
        let open = {
            let entity = cx.entity();
            // 键盘可达：与结果行同一口径（keyed focus + Enter/Space 激活；`mock-draft-open` 固定 id）
            let open_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from("mock-draft-open")),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            let entity_kb = entity.clone();
            div()
                .id("mock-draft-open")
                .role(gpui::Role::Button)
                // 自绘可点行：test_support 让 headless 测试可观察（与导航行同口径）；
                // 无 test_support 的元素不进 observation 注册表，会被误判为「未渲染」
                .test_support()
                .flex_1()
                .min_w_0()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_ellipsis()
                .cursor_pointer()
                .track_focus(&open_handle)
                .tab_stop(true)
                .child(name.clone())
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.open_detail_for(DetailTarget::Draft, window, cx);
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, window, app| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        entity_kb.update(app, |panel, cx| {
                            panel.open_detail_for(DetailTarget::Draft, window, cx);
                        });
                    }
                })
        };
        list.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .h(rems(ui::ROW_HEIGHT))
                .px_1()
                .rounded(radius)
                .relative()
                // 单表态只有这一行：它就是当前表，选中态常驻（不挂 hover）
                .bg(selected_bg)
                .child(tree::active_bar(selected_border))
                .child(open)
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{rows} 行")),
                )
                .child(render_status_cell(icon, &status, color)),
        )
        // 状态点分两套口径（生成的 / 落库的）：图例不常驻——280px 面板里 6 行解释
        // 会把出口区挤出可视区（2026-10-30 布局调整）；收到单行「状态图例 ⓘ」，
        // 悬停展开 6 项说明（与清单行右端状态点同一套 `render_status_cell`，不另写旧字形）
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .child("状态图例")
                .child(
                    HoverCard::new("mock-status-legend")
                        .trigger(
                            Button::new("mock-status-legend-trigger")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Info)
                                .tooltip("悬停查看状态含义"),
                        )
                        .content(|_, _, cx| {
                            let theme = cx.theme();
                            div()
                                .v_flex()
                                .gap_1()
                                .p_2()
                                .child(render_status_cell(
                                    None,
                                    "未生成",
                                    theme.colors.muted_foreground,
                                ))
                                .child(render_status_cell(
                                    Some(IconName::RefreshCw),
                                    "生成中",
                                    theme.colors.primary,
                                ))
                                .child(render_status_cell(
                                    Some(IconName::Check),
                                    "已生成",
                                    theme.colors.success,
                                ))
                                .child(render_status_cell(
                                    Some(IconName::CircleCheck),
                                    "已落库",
                                    theme.colors.success,
                                ))
                                .child(render_status_cell(
                                    Some(IconName::Info),
                                    "引用表未落库（多表）",
                                    theme.colors.warning,
                                ))
                                .child(render_status_cell(
                                    Some(IconName::CircleX),
                                    "生成失败",
                                    theme.colors.danger,
                                ))
                        }),
                ),
        )
    }

    /// 这张表引用的父表里，还有没落库的吗（`⚠ 引用的表未落库` 的数据来源）。
    ///
    /// 只看**本会话的**落库记录（`landed_tables`）：项目库里本来就有同名表不算它落过。

    /// 右 Dock 的面板主体：表清单 + 集合动作 + 出口 + 出口反馈（D38）。
    fn render_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let danger = cx.theme().colors.danger;
        let info = cx.theme().colors.info;

        let running = self.is_running();
        let generating = self.is_generating();

        // 集合级动作：生成 N 张表 / 退出场景（它属于这一批，不属于某张表 → 留在管理侧，D38）
        let scenario_actions = {
            let mut row = div().h_flex().items_center().gap_2().w_full();
            if let Some(template) = self.scenario.clone() {
                let start = {
                    let entity = cx.entity();
                    let mut button =
                        Button::new("mock-scenario-generate")
                            .primary()
                            .label(if generating {
                                "生成中…".to_string()
                            } else {
                                format!("生成 {} 张表", template.tables.len())
                            });
                    if !running {
                        button = button.on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                // 点生成 = 这一轮数据重新验收：先清掉上一轮残留的搜索过滤，
                                // 免得新清单被旧关键词挡住（曾误判「只生成了一张表」）
                                if let Some(search) = panel.search_input.clone() {
                                    search
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                }
                                panel.run_scenario(cx);
                            });
                        });
                    }
                    button
                };
                // S5：场景种子（留空 = 随机；填了 = 整组可复现，历史行标注 seed）
                let seed_box = self
                    .scenario_seed_input
                    .clone()
                    .map(|input| div().w(rems(ui::NUM_INPUT_WIDTH)).flex_none().child(Input::new(&input)))
                    .unwrap_or_else(|| div().flex_none());
                let exit = {
                    let entity = cx.entity();
                    let mut button = Button::new("mock-scenario-exit")
                        .secondary()
                        .label("退出场景")
                        .disabled(running);
                    if !running {
                        button = button.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.close_scenario(cx));
                        });
                    }
                    button
                };
                row = row
                    .child(div().flex_1().min_w_0().child(start))
                    .child(seed_box)
                    .child(div().flex_none().child(exit));
            }
            row
        };

        // ① 表清单：唯一的管理入口（单表 / 本次生成 / 结果表三态共用一个渲染器）
        let table_list = self.render_table_list(window, cx);

        // 关系闭包里还没落库的：摆出来 + 一键依次落（免得漏落，留下悬空引用）
        let pending_row = {
            let pending = self.pending_relation_tables();
            if pending.is_empty() {
                div()
            } else {
                let button = {
                    let entity = cx.entity();
                    let mut button = Button::new("mock-persist-related")
                        .secondary()
                        .xsmall()
                        .label(format!("落库这 {} 张", pending.len()))
                        .disabled(running);
                    if !running {
                        button = button.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.persist_related(cx));
                        });
                    }
                    button
                };
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
                            .text_color(cx.theme().colors.warning)
                            .text_ellipsis()
                            .child(format!(
                                "还有 {} 张没落库：{}",
                                pending.len(),
                                pending.join("、")
                            )),
                    )
                    .child(button)
            }
        };

        // 多表结果（场景生成）：与「落库这 N 张」对称的批量草稿箱出口。
        // 单表结果（1 张）不显示——单表走出口区「保存到草稿箱」即可。
        // 没项目根不显示：草稿箱只写项目目录（与单表出口同口径）。
        let batch_row = {
            let count = self.results.len();
            let has_root = self.host.project_root().is_some();
            if count > 1 && has_root && !running {
                let entity = cx.entity();
                let mut button = Button::new("mock-scratchpad-all")
                    .secondary()
                    .xsmall()
                    .label(format!("全部存草稿箱（{} 张）", count));
                button = button.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.scratchpad_all(cx));
                });
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
                            .text_color(cx.theme().colors.muted_foreground)
                            .text_ellipsis()
                            .child("场景结果一键导出（CSV）："),
                    )
                    .child(button)
            } else {
                div()
            }
        };

        // 任务进行中：进度条（组件，不手搓）+ 量纲文案 + 取消
        // 集合任务的进度与取消（单表任务的在中央表头，见 D38）
        let job_row = self.render_job_row(JobRowScope::Collection, cx);

        // 出口：落库 + 追加 + 草稿箱 + 另存为（任务进行中全部禁用：一次只能跑一个）
        //
        // 不再有「查看详情」按钮：清单点一行就是入口（D38），少一个按钮就少一份要同步的文案。
        // 没有结果时也禁用（出口是「把结果落地」，没结果就没什么可落）：点了报错不如直接不可点。
        let has_result = self.has_result();
        // 项目级出口（落库 / 追加 / 草稿箱）都要项目根：没打开项目时直接不可点，
        // 而不是点了才由任务层拒绝（「点了报错不如直接不可点」，与生成前禁用同一口径）。
        // `project_root()` 只读内存里的会话，不是 I/O（`read_only()` 同理）。
        let project_open = self.host.project_root().is_some();
        let sinks_ready = self.sinks_ready();
        let persist = {
            let entity = cx.entity();
            // 主出口（把结果写进项目分析库）：与其它三个出口区分层级——
            // 其余都是「换个去向」，这个是「落到项目里」，视觉上给 primary
            Button::new("mock-persist")
                .primary()
                .label("持久化到项目分析库")
                .w_full()
                .disabled(running || !sinks_ready)
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.persist_table(cx));
                })
        };
        let append = {
            let entity = cx.entity();
            let tables = self.existing_tables.clone();
            Button::new("mock-append")
                .secondary()
                .label("追加到既有表")
                .w_full()
                .dropdown_caret(true)
                .disabled(running || !sinks_ready)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    if tables.is_empty() {
                        return menu
                            .item(PopupMenuItem::new("（项目分析库暂无表）").disabled(true));
                    }
                    for name in tables.iter() {
                        let table = name.clone();
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                            move |_, window, app| {
                                let table = table.clone();
                                entity.update(app, |panel, cx| {
                                    panel.open_append_dialog(table, window, cx);
                                });
                            },
                        ));
                    }
                    menu
                })
        };
        let scratchpad = {
            let entity = cx.entity();
            Button::new("mock-save-scratchpad")
                .secondary()
                .label("保存到草稿箱")
                .w_full()
                .dropdown_caret(true)
                .disabled(running || !sinks_ready)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (label, format) in FILE_FORMATS {
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.save_scratchpad(&format, cx));
                        }));
                    }
                    menu
                })
        };
        let export = {
            let entity = cx.entity();
            Button::new("mock-export")
                .secondary()
                .label("另存为")
                .w_full()
                .dropdown_caret(true)
                .disabled(running || !has_result)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (label, format) in FILE_FORMATS {
                        let entity = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, app| {
                                let format = format.clone();
                                let name = entity.read(app).export_file_name(&format);
                                let dir = entity.read(app).host.export_dir();
                                let dir = if dir.trim().is_empty() {
                                    PathBuf::from(".")
                                } else {
                                    PathBuf::from(dir)
                                };
                                // 系统保存对话框（异步回传）：取消则不动
                                let receiver = app.prompt_for_new_path(&dir, Some(&name));
                                let entity = entity.clone();
                                window
                                    .spawn(app, async move |cx| {
                                        let Ok(Ok(Some(path))) = receiver.await else {
                                            return;
                                        };
                                        let path = path.to_string_lossy().to_string();
                                        cx.update(|_, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.export_file(&format, path, cx)
                                            });
                                        })
                                        .ok();
                                    })
                                    .detach();
                            }));
                    }
                    menu
                })
        };

        // 当前结果表（出口作用于它）；多张时上面给一个「当前表」选择器
        let outcome = self.outcome.clone();
        let error = self.error.clone();
        let landed = self.landed.clone();
        let read_only = self.host.read_only();
        let fg = cx.theme().colors.foreground;

        div()
            .v_flex()
            .gap_2()
            .w_full()
            .p_2()
            .child(table_list)
            .child(pending_row)
            .child(batch_row)
            // ② 集合级动作与它的进度：单表任务的在中央表头（D38）
            .child(scenario_actions)
            .child(job_row)
            // ③ 出口：作用于当前 tab 那张表
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("出口（作用于当前 tab 那张表；生成后可用）"),
            )
            // 项目级出口（落库 / 追加 / 草稿箱）与文件级出口（另存为）分两组：
            // 组件 `Separator` 分隔，不手搓 w_px 线条（容器层规范）
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .child(persist)
                    .child(append)
                    .child(scratchpad)
                    .child(Separator::horizontal())
                    .child(export),
            )
            // ④ 出口反馈就地给（成功 / 失败 / 落库目标 / 当前状态）
            .children(outcome.map(|text| div().text_xs().text_color(success).child(text)))
            .children(error.map(|err| div().text_xs().text_color(danger).child(err)))
            .children(landed.map(|table| {
                div()
                    .text_xs()
                    .text_color(fg)
                    .child(format!("落库目标：{table}"))
            }))
            // 未打开项目 / 只读：这两个状态决定出口能不能用，各一句话说清（也是禁用按钮的理由）
            .children((!project_open).then(|| {
                div()
                    .text_xs()
                    .text_color(info)
                    .child("未打开项目：落库 / 追加 / 草稿箱不可用（纯生成、预览与另存为不受影响）")
            }))
            .children(read_only.then(|| {
                div()
                    .text_xs()
                    .text_color(info)
                    .child("只读模式：不允许落库与写文件（仍可生成预览）")
            }))
            // 常驻的边界一句话（M7）。完整指引（要进全局分析库的两条路）不常驻：
            // 它在**真正需要时**由被拒的出口给出（`services::mock_generator` 的未打开项目 / 只读原因），
            // 而面板只有 280px 宽，天天摆着三行长说明就是天天占掉半屏清单
            .child(div().text_xs().text_color(muted).child(if project_open {
                "数据只写入项目分析库（{项目}/.RSmeta/analytics.duckdb）与文件，不回传源库（M7）"
            } else {
                "数据只写入项目分析库与文件，不回传源库（M7）"
            }))
    }

    /// 打开「导入源库结构」对话框。
    /// 标题栏下方的工具栏（2026-10-18 对齐原型）：新增 Mock 表（`Plus`）/ 引用结构（`HardDrive`）/
    /// 新增场景（`LayoutDashboard`）/ 刷新（`RefreshCw`）。
    ///
    /// 按钮语义全部是 `Button`（自带 hover / 焦点 / 键盘）；标题栏只留标题，
    /// 「＋」从标题栏工具栏迁到这里（原型同构：mock-toolbar 在面板头下方一行）。
    /// 行高走 `CONTROL_HEIGHT_SM`（与数据导航筛选行同档），底部分隔线用 `HAIRLINE`。
    fn render_toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let running = self.is_running();
        let entity = cx.entity();
        let muted = cx.theme().colors.muted_foreground;

        let new_table = {
            let entity = entity.clone();
            Button::new("mock-toolbar-new-table")
                .icon(IconName::Plus)
                .ghost()
                .tooltip("新增 Mock 表：开一个中央 tab 配字段与名称")
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| panel.new_draft_table(window, cx));
                })
        };
        // ⇄ 引用结构 = 原型 conn-pop：先弹连接列表选来源，再进对话框（库 / schema 按连接预填）。
        // 来源在 `open_import_dialog` 里填充，render 只读内存里的 `sources`（不触发 I/O）；
        // 还没有来源时菜单给一句说明 + 「在对话框里选择连接…」兜底（保持同一弹出组件形态）。
        let import = {
            let entity = entity.clone();
            let sources = self.sources.clone();
            Button::new("mock-toolbar-import")
                .icon(IconName::HardDrive)
                .ghost()
                // 显式 `small`：`dropdown_menu` 返回 `DropdownMenuPopover`（无 Sizable，
                // 进不了 Toolbar 的 size 托管槽），与 Toolbar 托管的按钮保持同档
                .small()
                .tooltip("引用数据库结构：选连接读表结构")
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    if sources.is_empty() {
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                        return menu.item(
                            PopupMenuItem::new("在对话框里选择连接…").on_click(
                                move |_, window, app| {
                                    let entity = entity.clone();
                                    entity.update(app, |panel, cx| {
                                        panel.open_import_dialog(window, cx);
                                    });
                                },
                            ),
                        );
                    }
                    for source in sources.iter() {
                        let entity = entity.clone();
                        let conn_id = source.conn_id.clone();
                        menu = menu.item(
                            PopupMenuItem::new(source.label.clone()).on_click(
                                move |_, window, app| {
                                    let entity = entity.clone();
                                    let conn_id = conn_id.clone();
                                    entity.update(app, |panel, cx| {
                                        panel.open_import_for(&conn_id, window, cx);
                                    });
                                },
                            ),
                        );
                    }
                    menu
                })
        };
        let new_scene = {
            let entity = entity.clone();
            Button::new("mock-toolbar-new-scene")
                .icon(IconName::LayoutDashboard)
                .ghost()
                .tooltip("新增场景：从 0 张表开始的工作副本")
                .disabled(running)
                .on_click(move |_, _window, app| {
                    entity.update(app, |panel, cx| panel.new_blank_scene(cx));
                })
        };
        let refresh = {
            let entity = entity.clone();
            Button::new("mock-toolbar-refresh")
                .icon(IconName::RefreshCw)
                .ghost()
                .tooltip("刷新：重读临时表与生成状态")
                .disabled(self.results.is_empty())
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.refresh_tables(cx));
                })
        };
        // 工具栏右侧的清单状态提示（对齐原型 mockToolHint：单表态「N 张草稿」；
        // 场景 / 结果态显示本批表数——比原型残留的「1 张草稿」更贴合当前清单）。
        let hint = if self.results.is_empty() {
            match &self.scenario {
                Some(template) => format!("{} 张表", template.tables.len()),
                None => "1 张草稿".to_string(),
            }
        } else {
            format!("{} 张结果", self.results.len())
        };

        // 工具栏走 `toolbar::Toolbar`（roving 方向键焦点 + toolbar a11y 语义 + 统一 size 托管）；
        // 外层只画高度 / 内距 / 底部分隔线（规范：背景边框高度外层自己画）。
        div()
            .w_full()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                Toolbar::new("mock-toolbar")
                    .children([new_table, new_scene, refresh])
                    // import 是 `DropdownMenuPopover` 包装（无 Sizable），走非托管 content 槽
                    .content(import)
                    .content(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(hint),
                    ),
            )
    }

    /// 底部折叠段：生成历史 + 用户模板（默认收起）。
    ///
    /// 为何收起来：面板天天用的是上面那份「有哪些表」清单，历史与模板是偶尔翻一次的落盘记录；
    /// 展开状态存实体（不是每帧派生的），展开体走 `Collapsible`（不手搮揭示动画）。
    fn render_folds(&mut self, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().colors.border;
        let open = self.fold_open;
        let toggle = {
            let entity = cx.entity();
            let label = if self.history_loaded {
                format!(
                    "生成历史（{}）· 用户模板（{}）",
                    self.history.len(),
                    self.templates.len()
                )
            } else {
                "生成历史 · 用户模板".to_string()
            };
            // 展开 / 收起箭头走图标（`ChevronDown` / `ChevronRight`），不用字形字符
            Button::new("mock-fold-toggle")
                .ghost()
                .xsmall()
                .icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .label(label)
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| {
                        panel.fold_open = !panel.fold_open;
                        cx.notify();
                    });
                })
        };

        let body = div()
            .v_flex()
            .gap_1()
            .w_full()
            .p_2()
            .border_t_1()
            .border_color(border)
            // 段序与折叠标题一致（「生成历史 · 用户模板」，见原型 §2 / 使用手册 §3）：
            // 展开后第一眼读到的就是标题先说的那一段
            .child(self.render_history(cx))
            // S5 起场景运行也记入历史（标注：scene_id + seed + 场景名，不落单表列行），
            // 所以这里没有「场景不进历史」的说明行——那条口径已随 S5 反转
            .child(self.render_templates(cx));

        div()
            .v_flex()
            .w_full()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(border)
            // `Collapsible` 是「揭盖」容器：触发行是普通子元素，内容走 `content`（收起时不渲染）；
            // `motion_id` 给「展开 / 收起」一段 spring 揭示动效（0.7.1 `collapsible.rs` 的 `reveal`），
            // 不给则收起 / 展开直接切，观感上是「啪」一下
            .child(toggle)
            .child(
                Collapsible::new()
                    .open(open)
                    .motion_id(ElementId::Name("mock-fold-reveal".into()))
                    .content(body),
            )
    }

    /// 用户模板段：把当前配置存下来 / 套用已有模板 / 删除。
    ///
    /// 与历史段共用同一次后台读（`HistorySnapshot`）；「保存」走对话框问名字，
    /// 空配置与空名都在事件路径上拦住（不给静默失败）。
    fn render_templates(&mut self, cx: &mut Context<Self>) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;

        let header = {
            let entity = cx.entity();
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
                        .text_color(muted)
                        .text_ellipsis()
                        .child(format!("用户模板（{}）", self.templates.len())),
                )
                .child(
                    Button::new("mock-template-save")
                        .ghost()
                        .xsmall()
                        .label("保存为模板…")
                        .on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.open_save_template_dialog(window, cx);
                            });
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        // 首次读取中：历史段已经说「读取中…」，这里不重复
        if self.history_loading && !self.history_loaded {
            return section;
        }
        if self.history_loaded && self.templates.is_empty() {
            // 空态走 `empty::Empty`（自带居中布局 + 虚线框 + 图标变体），不手搓居中文字块
            return section.child(
                Empty::new().header(
                    EmptyHeader::new()
                        // 模板空态给语义图标（`EmptyMedia` Icon 变体 = 圆角底框 + 图标），
                        // 不再用空框默认位
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(Icon::new(IconName::Folder).size(rems(ui::PANEL_ICON_SIZE))),
                        )
                        .title(EmptyTitle::new().child("还没有保存的模板"))
                        .description(EmptyDescription::new().child("把当前列配置存下来，下次一键套用")),
                ),
            );
        }

        for template in self.templates.clone() {
            let rows = template.row_count.max(0) as u64;
            let summary = template.description.clone().unwrap_or_default();
            let meta = if summary.is_empty() {
                format!("{} 行", with_thousands(rows))
            } else {
                format!("{} 行 · {summary}", with_thousands(rows))
            };
            let apply = {
                let entity = cx.entity();
                let id = template.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-template-apply-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("应用")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.apply_template(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = template.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-template-delete-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("删除")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_template(id, cx));
                })
            };

            section = section.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-template-{}",
                        template.id
                    ))))
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
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
                                    .text_sm()
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(template.name.clone()),
                            )
                            .child(apply)
                            .child(delete),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(meta),
                    ),
            );
        }
        section
    }

    /// 生成历史段：最近若干次运行（重放 / 删除都在事件路径上交给后台）。
    ///
    /// 列表与错误分开呈现：读失败时**保留旧列表**（能看到的旧数据比一片空白有用）。
    /// 行按 `created_at` 倒序（存储层已排），每行的 id 用任务 id——用得下标会在
    /// 「最新在前」的插入下把 hover 等按 id 记的状态串行。
    fn render_history(&mut self, cx: &mut Context<Self>) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let danger = cx.theme().colors.danger;
        let success = cx.theme().colors.success;

        let header = {
            let entity = cx.entity();
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
                        .text_color(muted)
                        .text_ellipsis()
                        .child(format!("生成历史（最近 {} 次）", history::HISTORY_LIMIT)),
                )
                .child(
                    Button::new("mock-history-refresh")
                        .ghost()
                        .xsmall()
                        .label("刷新")
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.refresh_history(cx));
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        if let Some(error) = self.history_error.clone() {
            section = section.child(div().text_xs().text_color(danger).child(error));
        }
        if self.history_loading && !self.history_loaded {
            // 不确定加载给一个 `Spinner`（小号转圈），文字保留便于弱网可读与测试断言
            return section.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(Spinner::new().small())
                    .child(div().text_xs().text_color(muted).child("读取中…")),
            );
        }
        if self.history_loaded && self.history.is_empty() {
            if self.history_error.is_none() {
                section = section.child(
                    Empty::new().header(
                        EmptyHeader::new()
                            // 历史空态给语义图标（收件箱 = 记录的去处）
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(Icon::new(IconName::Inbox).size(rems(ui::PANEL_ICON_SIZE))),
                            )
                            .title(EmptyTitle::new().child("还没有生成记录"))
                            .description(EmptyDescription::new().child("跑一次生成后，记录会出现在这里")),
                    ),
                );
            }
            return section;
        }

        for task in self.history.clone() {
            let rows = task.generated_rows.unwrap_or(task.row_count).max(0);
            let rows = with_thousands(rows as u64);
            let stamp = task
                .created_at
                .as_deref()
                .map(history::time_label)
                .unwrap_or_default();
            let stamp = (!stamp.is_empty())
                .then(|| div().flex_none().text_xs().text_color(muted).child(stamp));
            // 状态文案按语义取色：失败的原因原文展出来（截断由布局管）
            let (status_text, status_color) = match task.status.as_str() {
                // S5：场景历史行标注「场景 X · seed Y」（模板名已作为行标题展示）
                "success" if task.scene_id.is_some() => (
                    format!(
                        "成功 · seed {}",
                        task.seed
                            .map(|seed| seed.to_string())
                            .unwrap_or_else(|| "随机".to_string())
                    ),
                    success,
                ),
                "success" if task.save_format.as_deref() == Some(history::SAVE_FORMAT_TABLE) => {
                    (format!("成功 · {rows} 行 · 已追加"), success)
                }
                "success" => (format!("成功 · {rows} 行"), success),
                _ => (
                    task.error_message
                        .clone()
                        .unwrap_or_else(|| "失败".to_string()),
                    danger,
                ),
            };

            let replay = {
                let entity = cx.entity();
                let id = task.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-history-replay-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("重放")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.replay_history(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = task.id.clone();
                Button::new(ElementId::Name(SharedString::from(format!(
                    "mock-history-delete-{id}"
                ))))
                .ghost()
                .xsmall()
                .label("删除")
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_history(id, cx));
                })
            };

            section = section.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-history-{}",
                        task.id
                    ))))
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded(cx.theme().radius)
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
                                    .text_sm()
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(task.table_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{rows} 行")),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .children(stamp)
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(status_color)
                                            .text_ellipsis()
                                            .child(status_text),
                                    ),
                            )
                            .child(replay)
                            .child(delete),
                    ),
            );
        }
        section
    }

    /// 「保存为模板」对话框：只问名字——行数 / 种子 / 语言 / 列 / 生成器参数都取当前配置。
    ///
    /// 校验本身在 [`MockPanel::save_template`]；这里提前拒空配置，只是为了别弹一个
    /// 必然失败的框。名字默认填「{目标表名} 配置」。
    pub fn open_save_template_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        let name = self
            .template_name
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("模板名")))
            .clone();
        let suggestion = format!("{} 配置", self.draft.table_name.trim());
        name.update(cx, |state, cx| state.set_value(suggestion, window, cx));

        let panel = cx.entity();
        let columns = self.draft.columns.len();
        let rows = with_thousands(self.draft.options.rows as u64);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let body = div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!(
                            "存下当前配置：{rows} 行 · {columns} 列（含生成器与参数）。\
                             目标表名不存——套用时按当时的输入走。"
                        )),
                )
                .child(form_line(theme, "模板名", &name));
            let panel_save = panel.clone();
            let input = name.clone();
            dialog.title("保存为模板").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-template-dialog-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-template-dialog-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("保存")
                            .on_click(move |_, window, app| {
                                let value = input.read(app).value().to_string();
                                panel_save.update(app, |panel, cx| panel.save_template(value, cx));
                                window.close_dialog(app);
                            }),
                    ),
            )
        });
    }

    /// ⇄ 连接菜单选中：记下连接后直接进导入对话框（库 / schema 由对话框按连接记录预填，
    /// 见 [`Self::open_import_dialog`] 的 `panel_match` 分支）——对应原型 conn-pop 的
    /// 「选一个连接读它的库表结构」。
    pub fn open_import_for(
        &mut self,
        conn_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        *self.import_conn.borrow_mut() = Some(conn_id.to_string());
        self.open_import_dialog(window, cx);
    }

    pub fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sources.is_empty() {
            self.sources = self.host.schema_sources();
        }
        let panel_match = self
            .import_conn
            .borrow()
            .clone()
            .and_then(|id| self.sources.iter().find(|s| s.conn_id == id).cloned());
        let catalog = self
            .import_catalog
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("数据库")))
            .clone();
        let schema = self
            .import_schema
            .get_or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).placeholder("schema（可留空）"))
            })
            .clone();
        let table = self
            .import_table
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        if let Some(source) = panel_match.as_ref() {
            let (catalog_value, schema_value, table_value) = (
                source.catalog.clone(),
                source.schema.clone(),
                self.draft.table_name.clone(),
            );
            catalog.update(cx, |s, cx| s.set_value(catalog_value, window, cx));
            schema.update(cx, |s, cx| s.set_value(schema_value, window, cx));
            table.update(cx, |s, cx| s.set_value(table_value, window, cx));
        }

        let panel = cx.entity();
        // `Rc` 包裹：dialog builder 每帧重建，内层菜单闭包逐个 clone 只增计数
        let sources = Rc::new(self.sources.clone());
        let selected = self.import_conn.clone();
        let catalog_input = catalog.clone();
        let schema_input = schema.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let current = selected.borrow().clone();
            let mut body = div().v_flex().gap_2().child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("从源库读取表结构（列名 / 类型 / 可空 / 主键）并自动推断生成器；不读取数据本身。"),
            );
            // 连接：菜单选择（可见触发按钮）
            let mut conn_row = div().h_flex().items_center().gap_2().w_full();
            conn_row = conn_row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("连接"),
            );
            conn_row = conn_row.child({
                let selected = selected.clone();
                let catalog_input = catalog_input.clone();
                let schema_input = schema_input.clone();
                let sources = sources.clone();
                let current_label = current
                    .as_deref()
                    .and_then(|id| sources.iter().find(|s| s.conn_id == id))
                    .map(|s| s.label.clone());
                Button::new("mock-import-conn")
                    .ghost()
                    .dropdown_caret(true)
                    .label(match current_label {
                        Some(label) => label,
                        None => "选择连接".to_string(),
                    })
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        if sources.is_empty() {
                            return menu
                                .item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                        }
                        for candidate in sources.iter() {
                            let source = candidate.clone();
                            let selected = selected.clone();
                            let catalog_input = catalog_input.clone();
                            let schema_input = schema_input.clone();
                            menu = menu.item(
                                PopupMenuItem::new(candidate.label.clone()).on_click(
                                    move |_, window, app| {
                                        // 选中连接：记下 conn_id，并把连接记录的库 / schema 预填进输入框
                                        *selected.borrow_mut() = Some(source.conn_id.clone());
                                        let catalog = source.catalog.clone();
                                        let schema = source.schema.clone();
                                        catalog_input.update(app, |s, cx| {
                                            s.set_value(catalog, window, cx)
                                        });
                                        schema_input.update(app, |s, cx| {
                                            s.set_value(schema, window, cx)
                                        });
                                    },
                                ),
                            );
                        }
                        menu
                    })
            });
            body = body.child(conn_row);
            body = body
                .child(form_line(theme, "数据库 / catalog", &catalog))
                .child(form_line(theme, "schema", &schema))
                .child(form_line(theme, "表名", &table));

            let panel_ok = panel.clone();
            let selected_ok = selected.clone();
            dialog.title("导入源库结构").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("mock-import-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(move |_, window, app| {
                                window.close_dialog(app);
                            }),
                    )
                    .child(
                        Button::new("mock-import-ok")
                            .with_variant(ButtonVariant::Primary)
                            .label("导入")
                            .on_click(move |_, window, app| {
                                panel_ok.update(app, |panel, cx| {
                                    let request = SchemaRequest {
                                        conn_id: selected_ok
                                            .borrow()
                                            .clone()
                                            .unwrap_or_default(),
                                        catalog: panel
                                            .import_catalog
                                            .as_ref()
                                            .map(|i| i.read(cx).value().to_string())
                                            .unwrap_or_default(),
                                        schema: panel
                                            .import_schema
                                            .as_ref()
                                            .map(|i| i.read(cx).value().to_string())
                                            .unwrap_or_default(),
                                        table: panel
                                            .import_table
                                            .as_ref()
                                            .map(|i| i.read(cx).value().to_string())
                                            .unwrap_or_default(),
                                    };
                                    if request.conn_id.is_empty() {
                                        panel.fail("请选择连接", cx);
                                        return;
                                    }
                                    if request.table.trim().is_empty() {
                                        panel.fail("请填写表名", cx);
                                        return;
                                    }
                                    panel.preset_from_source(request, cx);
                                });
                                window.close_dialog(app);
                            }),
                    ),
            )
        });
    }
}

/// 表单行（标签 + 输入）。
pub(crate) fn form_line(theme: &gpui_kit::component::Theme, label: &str, input: &Entity<InputState>) -> Div {
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
                .child(label.to_string()),
        )
        .child(
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
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(Textarea::new(state).h(rems(ui::COMPLEX_INPUT_HEIGHT)))
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(hint.to_string()),
        )
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
                            // Dock 的面板头已经写着「Mock 生成」：面板内不再重复一个标题，
                            // 工具栏（＋⇄▦⟳）直接接在标题栏下面（D38 + 2026-10-18 对齐原型）
                            .child(self.render_toolbar(window, cx))
                            .child(actions)
                            .child(self.render_folds(cx)),
                    ),
            )
    }
}

