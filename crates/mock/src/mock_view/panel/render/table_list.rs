//! 表清单：左面板的唯一管理入口（单表 / 本次生成 / 结果表三态共用一个渲染器）。

use super::*;

impl MockPanel {
    /// 表清单：**唯一的管理入口**（D38）——单表 / 本次生成 / 结果表三态共用一个渲染器。
    ///
    /// 行 = 状态点 + 表名 + 行数；点一行 = 切 / 开那张表的中央 tab（选中行就是当前表）。
    /// 关系挂在**子表**行下的 `↳` 子行（真身是列上的 `dependency`），不再单开一段。
    pub(crate) fn render_table_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let success = cx.theme().colors.success;
        let warning = cx.theme().colors.warning;
        let primary = cx.theme().colors.primary;
        let danger = cx.theme().colors.danger;
        let running = self.is_running();
        // 一次性取主题（clone）：循环里还要可变借用 `cx`（`cx.entity()` 等），
        // 不能一路把 `cx.theme()` 的借用攥在手里。
        // 选中 / 悬停的色与卡片形态都收在 `row_card*` 原语里（与 M4 导航同口径：
        // 选中走 `list_active` 族，不当场挑 `accent`）
        let theme = cx.theme().clone();

        let mut list = div().v_flex().gap_1().w_full();

        // 清单搜索框：按表名过滤行（与原型「搜索表名…」对齐；空搜索 = 全显示）。
        // 搜索词只影响显示，不影响下标语义（index 仍是 results 里的真实位置）。
        let search = self.search_input.clone();
        list = list.child(
            div().w_full().child(
                search
                    .map(|input| div().child(Input::new(&input)))
                    .unwrap_or_else(|| div()),
            ),
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
            list = list.child(section(
                &theme,
                format!("结果表（{count}）· 点一行切到它的 tab"),
            ));
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
                        // 不再 `flex_1`：名字取自然宽度，好让状态紧跟其后；
                        // 长表名靠 `min_w_0` + `text_ellipsis` 收缩，状态那格永远留得住
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
                // 清单卡片（v_flex）：上面一行是这张表的信息，下面挂它的关系与下缘进度线。
                // 卡片化之前这是贴在面板上的几行字，只有选中才临时上色——平时行与行之间没有边界
                let mut card = if selected {
                    row_card_selected(&theme)
                } else {
                    row_card_interactive(&theme)
                };
                card =
                    card.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            // 表名 + 状态同处一栏：状态紧跟表名，一眼读出「这张表现在什么态」。
                            // 行数仍在行尾——它是量纲，不是状态，不该夹在名字和状态中间。
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_1()
                                    .min_w_0()
                                    .child(open)
                                    .child(render_status_cell(icon, &status, color)),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(format!(
                                "{} 行",
                                with_thousands(u64::from(info.row_count))
                            ))),
                    );
                // 关系挂在**这这张卡的卡内**（`pl_4` 仍表达从属）：卡片化之后清单的视觉
                // 单位是「一张表 = 一张卡」，关系属于这张表，不该掉到卡片外面单独成行
                for relation in self.outgoing_relations(&name).iter() {
                    let range = self.relation_range(relation);
                    card = card.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .pl_4()
                            .text_xs()
                            .text_color(text_hint(&theme))
                            .child(div().flex_1().min_w_0().text_ellipsis().child(format!(
                                "↳ {} → {}.{}",
                                relation.child_column,
                                relation.parent_table,
                                relation.parent_column
                            )))
                            .children(range.map(|range| div().flex_none().child(range))),
                    );
                }
                // 下缘 2px 进度线：这张表走到哪了（切走中央 tab 也还看得见）
                if let Some(line) =
                    self.row_progress_overlay(&format!("result-{name}"), &name, index, cx)
                {
                    card = card.child(line);
                }
                list = list.child(card);
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
            list = list.child(section(&theme, heading));
            let relations = relations_of(&template);
            // `enumerate` 的序号是**集合里的真实位置**（搜索过滤只影响显示，不改它）：
            // 逐表推进的进度线按它判断「这张过了没有」
            for (index, table) in template.tables.iter().enumerate() {
                if !query.is_empty() && !table.name.to_lowercase().contains(&query) {
                    continue;
                }
                let name = table.name.clone();
                let edit = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    let mut btn = button(
                        SharedString::from(format!("mock-scenario-table-edit-{click_name}")),
                        "编辑",
                        ActionKind::Quiet,
                    )
                    .xsmall()
                    .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, window, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_table_dialog(&name, window, cx);
                            });
                        });
                    }
                    btn
                };
                let remove = {
                    let entity = cx.entity();
                    let click_name = name.clone();
                    let mut btn = icon_button(
                        SharedString::from(format!("mock-scenario-table-remove-{click_name}")),
                        IconName::Close,
                        "从本次生成移除这张表",
                    )
                    .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, _, app| {
                            let name = click_name.clone();
                            entity.update(app, |panel, cx| {
                                panel.remove_scenario_table(&name, cx);
                            });
                        });
                    }
                    btn
                };
                // 行焦点句柄：行内操作是悬停才显形的，键盘用户没有悬停，
                // 焦点落到行上时也要显形。按业务键（表名）缓存句柄——每帧新建会丢焦点。
                let row_handle = window
                    .use_keyed_state(
                        ElementId::Name(SharedString::from(format!("mock-scenario-row-{name}"))),
                        cx,
                        |_, cx| cx.focus_handle(),
                    )
                    .read(cx);
                let row_focused = row_handle.is_focused(window);
                let row_active =
                    self.hovered_scenario_table.as_deref() == Some(name.as_str()) || row_focused;
                let hover_border = theme.colors.ring;
                // 清单卡片（v_flex）：卡片化之后清单的视觉单位是「一张表 = 一张卡」
                let mut card = row_card(&theme)
                    // 元素 id 是挂 hover 事件的前提（无 id 的元素收不到 hover）
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-scenario-row-{name}"
                    ))))
                    // 行可被 Tab 到（行内操作悬停才显形，键盘得先落到行上）
                    .track_focus(&row_handle)
                    .tab_stop(true)
                    .on_hover({
                        let entity = cx.entity();
                        let hover_name = name.clone();
                        move |&hovering, _, app| {
                            entity.update(app, |panel, cx| {
                                let next = hovering.then(|| hover_name.clone());
                                if panel.hovered_scenario_table != next {
                                    panel.hovered_scenario_table = next;
                                    cx.notify();
                                }
                            });
                        }
                    });
                // 卡内第一行：表名 + 状态 + 行数 + 行操作
                card = card
                    // 悬停 / 键盘聚焦：卡片描边转 ring。卡片底已经是卡片表面色，
                    // 再拿 `list_hover` 换底等于没换（亮主题下它与卡片底同为 #f5f5f5）
                    .when(row_active, move |c| c.border_color(hover_border))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            // 表名 + 状态同栏（同上）：场景里的表还没生成，状态一律「待生成」
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_ellipsis()
                                            .child(name.clone()),
                                    )
                                    .child(render_status_cell(None, "待生成", muted)),
                            )
                            .child(div().flex_none().text_xs().text_color(muted).child(format!(
                                "{} 行",
                                with_thousands(u64::from(table.row_count))
                            )))
                            .child(if row_active {
                                // 显形：鼠标在这行上 / 键盘焦点在这行上
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_none()
                                    .child(edit)
                                    .child(remove)
                            } else {
                                // 不显形：占位容器保持宽度，行数那一格不会跟着按钮左右跳
                                div().flex_none().w(rems(ui::ROW_OPS_WIDTH))
                            }),
                    );
                // 关系收进这张卡的卡内（`pl_4` 仍表达从属；行尾 ✕ 删关系）
                for relation in relations.iter().filter(|r| r.child_table == name) {
                    let range = self.relation_range(relation);
                    let remove_relation = {
                        let entity = cx.entity();
                        let child_table = relation.child_table.clone();
                        let child_column = relation.child_column.clone();
                        icon_button(
                            SharedString::from(format!(
                                "mock-relation-remove-{child_table}-{child_column}"
                            )),
                            IconName::Close,
                            "删除这条关系",
                        )
                        .on_click(move |_, _, app| {
                            let child_table = child_table.clone();
                            let child_column = child_column.clone();
                            entity.update(app, |panel, cx| {
                                panel.remove_relation(&child_table, &child_column, cx);
                            });
                        })
                    };
                    card = card.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .pl_4()
                            .text_xs()
                            .text_color(text_hint(&theme))
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
                // 下缘 2px 进度线：场景生成是逐表推进的，这条线说「轮到 / 过了哪张」
                if let Some(line) =
                    self.row_progress_overlay(&format!("scenario-{name}"), &name, index, cx)
                {
                    card = card.child(line);
                }
                list = list.child(card);
            }

            // 自定义多表：把当前表配置加进来（导入结构 / 列编辑都在单表态里做完）
            let add_table = {
                let entity = cx.entity();
                // 场景清单的主行动：把调好的表配置收进本次生成
                let mut btn = button(
                    "mock-scenario-table-add",
                    "＋ 加表（当前表配置）",
                    ActionKind::Main,
                )
                .xsmall()
                .disabled(running);
                if !running {
                    btn = btn.on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.add_draft_to_scenario(cx);
                        });
                    });
                }
                btn
            };
            let add_relation = {
                let entity = cx.entity();
                let mut btn = button("mock-add-relation", "＋ 加关系", ActionKind::Soft)
                    .xsmall()
                    .disabled(running);
                if !running {
                    btn = btn.on_click(move |_, window, app| {
                        entity.update(app, |panel, cx| {
                            panel.open_relation_dialog(window, cx);
                        });
                    });
                }
                btn
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
                .child(hint_line(
                    &theme,
                    "关系只在本次多表生成内成立（不读已落数据、不改已生成的表 / 文件）；行尾是取值域：父列自增参数 × 父表行数，改父表行数它跟着变。",
                ))
                .child(hint_line(
                    &theme,
                    "加进来的表取表配置的表名 / 行数 / 列：先在中央那张表配置 tab 里调好，再加进本次生成。",
                ));
            return list;
        }

        // ── 单表态（还没有结果）：**每一份表配置各占一行** ──
        // 多配置之后右面板必须能看见、也能切回全部配置：只画活动那一份时，
        // 开过第二个「＋」就再也回不到前一份（用户 2026-11-05 的反馈）。
        let configs = self.config_rows();
        list = list.child(section(
            &theme,
            format!("表配置（{}）· 点一行切到它的 tab", configs.len()),
        ));
        for (id, name, columns, _rows) in configs {
            if !query.is_empty() && !name.to_lowercase().contains(&query) {
                continue;
            }
            let selected = id == self.config_id;
            // 状态按**表名**查：这份配置若已经生成过结果，名字就对得上。
            // 百分比只对活动那份有意义（面板级任务指针指向它）。
            let (icon, status, color) = match self.table_status(&name) {
                TableStatus::Generating if selected => (
                    Some(IconName::RefreshCw),
                    format!("生成中 {:.0}%", self.job_percent()),
                    primary,
                ),
                TableStatus::Generating => (None, "生成中".to_string(), primary),
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
            // 活动那份沿用稳定 id `mock-draft-open`（宿主用例与键盘焦点都指着它）；
            // 其余按配置 id 生成，避免多行重复 ElementId（同帧重复 id 会撞 a11y 校验）。
            let row_id = if selected {
                SharedString::from("mock-draft-open")
            } else {
                SharedString::from(format!("mock-config-open-{id}"))
            };
            let open_handle = window
                .use_keyed_state(ElementId::Name(row_id.clone()), cx, |_, cx| {
                    cx.focus_handle()
                })
                .read(cx)
                .clone();
            let entity = cx.entity();
            let entity_kb = entity.clone();
            let open = div()
                .id(row_id)
                .role(gpui::Role::Button)
                // 自绘可点行：test_support 让 headless 测试可观察（与导航行同口径）
                .test_support()
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
                        panel.open_detail_for(DetailTarget::Config(id), window, cx);
                    });
                })
                .on_key_down(move |event: &KeyDownEvent, window, app| {
                    if event.keystroke.key == "enter" || event.keystroke.key == "space" {
                        entity_kb.update(app, |panel, cx| {
                            panel.open_detail_for(DetailTarget::Config(id), window, cx);
                        });
                    }
                });
            let mut card = if selected {
                row_card_selected(&theme)
            } else {
                row_card_interactive(&theme)
            }
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(open)
                            .child(render_status_cell(icon, &status, color)),
                    )
                    // 列数是**这份配置**最能区分彼此的一件事（行数常常一样）
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{columns} 列")),
                    ),
            );
            // 下缘 2px 进度线：只有活动那份在跑任务
            if selected {
                if let Some(line) = self.row_progress_overlay("draft", &name, 0, cx) {
                    card = card.child(line);
                }
            }
            list = list.child(card);
        }
        list
        // 状态图例整行删掉：图例解释 6 种态，而实际态已经直接跟在表名旁（图标 + 文字），
        // 再单独列一遍是拿说明挤占 280px 面板的清单空间。状态文字本身就说清了含义。
    }
}
