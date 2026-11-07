//! 详情视图的渲染入口。

use super::*;

impl Render for MockDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;
        let is_draft = matches!(self.target, DetailTarget::Config(_));
        let config_id = self.config_id();
        // 本 tab 的稳定唯一键（`DetailTarget::key`）：tab 内所有会随 tab 挂树的元素
        // id 都拼它，保证同一窗口内 id 唯一（a11y 树重复节点会 panic，见
        // `multiple_detail_tabs_do_not_collide_a11y_ids`）。
        let tab_key = self.target.key();

        // 一次读齐渲染要用的状态：读租约不能跨到 `cx` 的独占使用处，而下面的
        // `panel.update`（表头要懒创建输入、按钮要进面板）必须独占面板。
        let (
            table,
            generated,
            scenario_source,
            relation_note,
            draft_columns,
            draft_columns_shown,
            result_columns,
            dropped_by_project_switch,
            preview_sort_label,
            preview_shown,
        ) = {
            let panel = self.panel.read(cx);
            // 本 tab 看哪张表：表配置 tab 看表配置目标表，结果表 tab 看它自己
            // （与面板的「当前表」无关——切 tab 就能对照两张表的预览）
            let table = match &self.target {
                DetailTarget::Config(id) => panel.config_for(*id).table_name.clone(),
                DetailTarget::Table(name) => name.clone(),
            };
            let generated = panel
                .results()
                .iter()
                .find(|info| info.table_name == table)
                .cloned();
            let relation_note = if is_draft {
                None
            } else {
                panel.relation_note_for(&table)
            };
            let result_columns = generated
                .as_ref()
                .map(|info| info.columns.len())
                .unwrap_or(0);
            // 预览当前显示的那一份（排序生效时是重查来的），标题里的行数与排序说明都从它来
            let shown = panel.preview_for(&table);
            (
                table,
                generated,
                panel.scenario_source().map(|s| s.to_string()),
                relation_note,
                config_id
                    .map(|id| panel.config_for(id).columns.len())
                    .unwrap_or(0),
                // 字段区的筛选计数与渲染读同一处（`visible_columns`），不另写一份口径
                config_id
                    .map(|id| self.visible_columns(panel, id, cx).len())
                    .unwrap_or(0),
                result_columns,
                panel.results_dropped_by_project_switch(),
                shown.and_then(|(_, sort)| sort).map(|sort| sort.label()),
                shown.map_or(0, |(preview, _)| preview.rows.len()),
            )
        };

        // ── 表头 ──
        // 表配置 tab：这张表的设计与生成状态（表名 / 行数 / 种子 / 语言 + 生成 + 进度 + 失败重试）。
        // 状态归面板（单一权威），画在中央——它属于这张表，不属于「有哪些表」那份清单。
        // 结果表 tab：只读事实 + 跨表后果；不给「生成」，免得像能只重跑这一张。
        let head = if let (true, Some(id)) = (is_draft, config_id) {
            self.panel.update(cx, |panel, cx| {
                // 传配置 id + tab 键：输入框按配置各自一份，出口按钮 id 按 tab 唯一
                panel.render_design_head(id, &tab_key, window, cx)
            })
        } else {
            // 事实行：主行只留语义事实（表名 / 行数 / 状态）；临时表名与耗时是技术细节，
            // 收进悬停（低频解释不常驻，与右面板状态条同一手法）。
            //
            // 没结果分两种：切项目把结果清掉了（旧 tab 留着，见 D35），或者这一轮没生成它。
            // 两种说法的行动建议一样（重新生成），但「为什么没结果」不一样——不能都说成「这一轮」。
            let facts_row = match generated.as_ref() {
                Some(info) => div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .text_ellipsis()
                            .child(format!(
                                "{} · {} 行 · 已生成",
                                table,
                                with_thousands(u64::from(info.row_count)),
                            )),
                    )
                    .child({
                        let temp_table = info.temp_table_name.clone();
                        let elapsed_ms = info.elapsed_ms;
                        HoverCard::new(ElementId::Name(SharedString::from(format!(
                            "mock-facts-tech-{tab_key}"
                        ))))
                        .trigger(info_hint_icon(&theme))
                        .content(move |_, _, _cx| {
                            div()
                                .p_2()
                                .text_xs()
                                .child(format!(
                                    "临时表 {temp_table} · 生成耗时 {elapsed_ms} ms"
                                ))
                        })
                    }),
                None if dropped_by_project_switch => div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .text_ellipsis()
                    .child(format!(
                        "{table} · 所属项目已切换：这张表的结果已作废（切项目会清空 mock 结果）——重新生成后再看"
                    )),
                None => div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .text_ellipsis()
                    .child(format!("{table} · 这一轮没有它的结果（重新生成后再看）")),
            };
            let mut head = div().v_flex().gap_1().w_full().child(facts_row);
            // 场景来源：常驻只说「从哪来」（出处是每次都要读的身份信息）；
            // 「怎么切到别的表」是一次性引导，收进悬停。
            if let Some(source) = scenario_source.clone() {
                head = head.child(
                    HoverCard::new(ElementId::Name(SharedString::from(format!(
                        "mock-scenario-source-{tab_key}"
                    ))))
                    .trigger(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(card_note(&theme, format!("来自场景模板「{source}」")))
                            .child(info_hint_icon(&theme)),
                    )
                    .content(move |_, _, _cx| {
                        div().p_2().text_xs().child(
                            "列与预览都属于它自己；看别的表切 tab（或在右 Dock 结果表清单点一行）",
                        )
                    }),
                );
            }
            if let Some(note) = relation_note.clone() {
                head = head.child(warning_line(&theme, note));
            }
            head
        };

        // 段 1：基本信息（表头）。内容可能超出一屏（进度行 / 失败重试 / 来源说明），
        // 进可拖分栏后有自己的滚动，不再把整页拖长。
        let head_block = div()
            .v_flex()
            .gap_1()
            .w_full()
            .pb_2()
            .border_b_1()
            .border_color(border)
            .child(head);

        let mut body = div()
            .id(format!("mock-detail-{tab_key}"))
            .role(gpui::Role::Group)
            .v_flex()
            .size_full()
            .min_h_0()
            .gap_2()
            .p_3();

        // ── 列（段 2）──
        // 三种态各自成块：无列一行提示 / 列操作行 + 单行列清单 / 结果表只读列。
        // **不画「列（N）」段标题**：那一行每份配置固定吃掉一段高度，换来的只是一个计数；
        // 计数并到操作行最左，跟着按钮一起读。
        let column_count = if draft_columns_shown == draft_columns {
            format!("{draft_columns} 列")
        } else {
            format!("{draft_columns_shown} / {draft_columns} 列 · 已筛选")
        };
        let mut columns_block = div().v_flex().gap_2().w_full();
        // 列区工具栏：**有列 / 无列同一套元素、同一顺序**（计数 → 场景 → 弹性 → 筛选 → 来源按钮）。
        // 此前无列态只画「还没有列 + 场景 + 来源按钮」，一加上第一列就多出筛选框、整排按钮右移，
        // 同一块区域两种排版——无列时改为「计数 0 列 + 筛选禁用」，位置一律不动。
        let scenario_id = if is_draft && draft_columns == 0 {
            "mock-detail-scenario-empty"
        } else {
            "mock-detail-scenario-columns"
        };
        // 工具栏条：自己的底色（list）+ 圆角 + 内距，与下面的列行明确分开。
        // 此前工具栏和列行在同一块 muted 上，按钮像是"浮"在内容里，没有"工具条"的边界。
        // 顺序：左组=批量起手（场景 / 预设 / 导入），右组=定位+新增（计数 badge / 筛选 / 加列）。
        let columns_toolbar = div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1()
            .rounded(theme.radius)
            .bg(theme.colors.list)
            .child(self.panel.update(cx, |panel, cx| {
                panel.render_scenario_button(scenario_id, cx)
            }))
            // 批量起手（预设 / 导入结构）紧跟场景模板：三者同属「把这张表的列起手」
            .child(self.panel.update(cx, |panel, cx| {
                panel.render_batch_source_buttons(config_id.unwrap_or_default(), cx)
            }))
            // 弹性空档：把右组（计数 / 筛选 / 加列）整体推到右端
            .child(div().flex_1().min_w_0())
            // 计数 badge：放在定位输入框左边，和筛选是一对（"多少列 / 过滤哪些列"）
            .child(
                div()
                    .flex_none()
                    .px_1()
                    .py(px(2.))
                    .rounded(px(3.))
                    .bg(theme.colors.popover)
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(column_count),
            )
            .child(self.panel.update(cx, |panel, cx| {
                panel.render_column_filter(
                    config_id.unwrap_or_default(),
                    is_draft && draft_columns > 0,
                    window,
                    cx,
                )
            }))
            .child(self.panel.update(cx, |panel, cx| {
                panel.render_add_column_button(config_id.unwrap_or_default(), cx)
            }));
        if is_draft {
            columns_block = columns_block.child(columns_toolbar);
            if draft_columns == 0 {
                // 空清单只留一句「下一步做什么」：按钮全在工具栏里（位置固定），
                // 这里不画卡、不塞按钮，免得空态把列区高度换成一块居中卡片
                columns_block = columns_block.child(card_note(
                    &theme,
                    "还没有列：用右侧「加列」新建，或走「预设 / 导入结构」批量起手",
                ));
            } else {
                columns_block = columns_block.child(self.render_fields(window, cx));
            }
        } else {
            // 结果 tab 也不给段标题：列数并进一行小字，和表配置 tab 的读法一致
            columns_block = columns_block
                .child(card_note(
                    &theme,
                    format!("{result_columns} 列 · 只读（来自这次生成）"),
                ))
                .child(self.render_result_columns(generated.as_ref(), cx));
        }

        // ── 预览 ──
        // 段 3：预览（标题行 + 表格 / 空态）。表格 `DataTable` 自带虚拟化与横向滚动，
        // 段内直接撑满——不再随整页滚。单表出口已上移到表头的动作行：这里只留预览。
        let mut preview_block = div().v_flex().gap_2().w_full().flex_1().min_h_0();

        // 有结果：标题行常驻的只有「现在看的是哪一段」（排序口径 + 前 N / 共 M 行）；
        // 临时表名 / 耗时 / 排序的重查语义是低频解释，一并收进 info 悬停
        // （表名与总行数已在表头事实行，标题行不重复）。
        if let Some(info) = generated.as_ref() {
            let title = match &preview_sort_label {
                // 排序生效时把口径说清：这是「按该列排序后的前 N 行」，不是生成顺序的前 N 行
                Some(label) => format!(
                    "预览（{label} · 前 {preview_shown} 行 · 共 {} 行）",
                    with_thousands(u64::from(info.row_count))
                ),
                None => format!(
                    "预览（前 {preview_shown} 行 · 共 {} 行）",
                    with_thousands(u64::from(info.row_count))
                ),
            };
            let temp_table = info.temp_table_name.clone();
            let elapsed_ms = info.elapsed_ms;
            // 排序生效时才解释重查语义（没排序时「前 N 行」不言自明）
            let sort_note = preview_sort_label.as_ref().map(|label| {
                format!(
                    "「{label}」是重查临时表得来的全局前 {preview_shown} 行，不是生成顺序的前几行；\
                     点表头右端的箭头可换向 / 取消，右键同一套。"
                )
            });
            let title_hover = HoverCard::new(ElementId::Name(SharedString::from(format!(
                "mock-preview-tech-{tab_key}"
            ))))
            .trigger(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .text_ellipsis()
                            .child(title),
                    )
                    .child(info_hint_icon(&theme)),
            )
            .content(move |_, _, _cx| {
                let mut card = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .text_xs()
                    .child(format!("临时表 {temp_table} · 生成耗时 {elapsed_ms} ms"));
                if let Some(note) = sort_note.clone() {
                    card = card.child(div().max_w(ui::STATUS_TOOLTIP_MAX_WIDTH).child(note));
                }
                card
            });
            preview_block = preview_block.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(title_hover)
                    .child(self.panel.update(cx, |panel, cx| {
                        panel.render_preview_limit_menu(&tab_key, cx)
                    })),
            );
        }

        // 表格只认「真有行可显示」：生成结果里有行才画 `DataTable`（预览表 = 组件库
        // `DataTable`，与编辑器结果集同一套原语：表头 / 列宽拖拽 / 横向滚动 / 虚拟化 /
        // 键盘选择都是组件的，本处只给容器与一个几十行的 delegate）。
        let has_preview_rows = generated.as_ref().is_some_and(|_| preview_shown > 0);
        if has_preview_rows {
            self.ensure_preview_table(window, cx);
            if let Some(table) = self.preview_table.clone() {
                preview_block = preview_block.child(
                    div()
                        .size_full()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .debug_selector(|| "mock-preview-table".to_string())
                        // 行高走组件档位（`ui::PREVIEW_TABLE_SIZE` = XSmall = 26px，与结果集网格同档），
                        // 单元格字号在 delegate 里取 `text_xs`（与面板其它行的密度一致 —— 这是组件库
                        // px 基准尺寸的既定例外）
                        .child(
                            DataTable::new(&table)
                                // 斑马纹 / 密度 / 内距与结果集网格对齐（同一类东西，观感不齐会在并列对照时显形）
                                .stripe(true)
                                .with_size(ui::PREVIEW_TABLE_SIZE),
                        ),
                );
            }
        } else {
            // 没有可显示的行：空态走 `Empty`（与右 Dock 模板 / 历史空态同一支组件——
            // 自带居中布局 + 虚线框 + 图标变体；此前这里是一行裸文本「（暂无预览）」，
            // 预览段是三段里最大的一块，一行灰字把视觉重心塌掉了）。
            // 「为什么空」各说各的引导（rds-design 空态文案规范：原因 + 下一步）；
            // 表配置 tab 顺手给「生成」主按钮——空态直达主操作，不必回表头找。
            let (title, desc) = if generated.is_some() {
                ("预览为空", "这张表没有可显示的行".to_string())
            } else if dropped_by_project_switch {
                (
                    "结果已随项目切换作废",
                    if is_draft {
                        "切项目会清空 mock 结果；表配置还在，重新生成后再看".to_string()
                    } else {
                        "切项目会清空 mock 结果——回到原项目重开，或重新生成".to_string()
                    },
                )
            } else if is_draft {
                // 空态本身带上「待生成什么」：列名就是这张表的结构，用户不用回列区确认
                // 自己配了什么。这也是「结果区默认显示（常驻）而不是生成后才出现」的
                // 依据：常驻但给信息，三段高度不随生成状态跳，分栏也不会因空态塌陷。
                let columns_note = config_id
                    .map(|id| {
                        let panel = self.panel.read(cx);
                        panel.config_for(id).columns.clone()
                    })
                    .map(|columns| {
                        let mut names: Vec<String> = columns
                            .iter()
                            .take(6)
                            .map(|column| column.def.name.clone())
                            .collect();
                        if columns.len() > names.len() {
                            names.push(format!("…共 {} 列", columns.len()));
                        }
                        names.join(" · ")
                    })
                    .filter(|note| !note.is_empty())
                    .unwrap_or_else(|| "还没有列（先加列再生成）".to_string());
                (
                    "还没有生成结果",
                    format!("{columns_note}——点「生成」在内存临时表里产出数据，预览跟着来"),
                )
            } else {
                (
                    "这一轮没有它的结果",
                    "从右 Dock 结果表清单重新生成，或切到表配置 tab 再跑".to_string(),
                )
            };
            // 生成按钮只给表配置 tab（结果表 tab 只读，不给「能只重跑这一张」的错觉）；
            // 任务进行中禁用（与表头「生成」同一口径，不靠任务层拒绝）
            let generate_action = if is_draft && generated.is_none() {
                let panel_entity = self.panel.clone();
                let running = self.panel.read(cx).is_running();
                Some(
                    button(
                        SharedString::from(format!("mock-preview-generate-{tab_key}")),
                        "生成",
                        ActionKind::Main,
                    )
                    .disabled(running)
                    .on_click(move |_, _, app| {
                        panel_entity.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.run_generate(cx)
                        });
                    }),
                )
            } else {
                None
            };
            preview_block = preview_block.child(
                Empty::new()
                    .header(
                        EmptyHeader::new()
                            // 收件箱 = 「数据还没到」的空态语义（与右 Dock 历史空态同一图标语言）
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(
                                        Icon::new(IconName::Inbox).size(rems(ui::PANEL_ICON_SIZE)),
                                    ),
                            )
                            .title(EmptyTitle::new().child(title))
                            .description(EmptyDescription::new().child(desc)),
                    )
                    .when_some(generate_action, |this, btn| {
                        // `debug_selector` 只是给用例找坐标（真点击检查；`.id(...)` 不登记坐标）
                        let sel_key = tab_key.clone();
                        this.content(
                            EmptyContent::new().child(
                                div()
                                    .debug_selector(move || {
                                        format!("mock-preview-generate-{sel_key}")
                                    })
                                    .child(btn),
                            ),
                        )
                    })
                    .min_h(rems(ui::PREVIEW_MIN_HEIGHT)),
            );
        }

        // 三段可拖分栏（`v_resizable`，仿编辑器结果区分栏先例）：基本信息 / 列区 / 预览。
        // 每段独立滚动（列区字段多、预览表格虚拟化，各滚各的）；拖拽高度经 `on_resize`
        // 回写面板字段（rem），跨 tab / 跨窗口沿用。初始大小：基本信息 100px（可拉到 220
        // 看全进度 / 失败重试 / 来源说明），预览按 `detail_preview_rem` 收住上限（默认
        // 20rem，不再吃满剩余），列区 flex 吃剩余——清单比预览更需要高度，窄窗口也不会溢出。
        let font_size = cx.theme().font_size;
        // 预览的高度上限（同时是拖拽范围末端）：面板字段由 `on_resize` 回写
        let preview_max = (self.panel.read(cx).detail_preview_rem * font_size)
            .max(ui::DETAIL_PREVIEW_MIN_HEIGHT)
            .min(ui::DETAIL_PREVIEW_MAX_HEIGHT);
        let split_panel = self.panel.clone();
        // 分栏 id 拼 tab 键：多个详情 tab 并存时各自独立（a11y 节点 + resizable state
        // 都按 id 区分；尺寸经 on_resize 回写面板字段，跨 tab 仍沿用）
        let split = v_resizable(format!("mock-detail-split-{tab_key}"))
            .child(
                resizable_panel()
                    .size(ui::DETAIL_HEADER_HEIGHT)
                    // 下界必须小于初始高度，否则往下拖一碰到 `min_h` 就停住（「拖不动」）
                    .size_range(ui::DETAIL_HEADER_MIN_HEIGHT..ui::DETAIL_HEADER_MAX_HEIGHT)
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            // 段内内距：三段内容此前都贴着 tab 边缘与分隔条，
                            // 与分栏条的线挤在一起，看着像没排版的表配置
                            // 三段统一 12px（p_3）：列区原本外层 p_2 + 内层 p_3 双层 = 20px，
                            // 已合成一层（card 自己 p_0，内容包 p_3）
                            .p_3()
                            .overflow_y_scrollbar()
                            .child(head_block),
                    ),
            )
            .child(
                resizable_panel()
                    .size_range(ui::DETAIL_COLUMNS_MIN_HEIGHT..px(4000.))
                    // basis 0：不按内容高度撑开（面板默认 basis = auto，列一多会把预览
                    // 段推到视口外），高度完全由 flex 分配——清单比预览更需要高度
                    .flex_basis(px(0.))
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            // 列区不再套 muted 卡片：整个区域的背景色去掉，
                            // 列行直接铺在页面背景上，只有工具栏条（list 色）做视觉分组。
                            // 此前列区是一块灰卡片，和段 1/段 3 一样厚重，没有层次。
                            .child(
                                div()
                                    .size_full()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .size_full()
                                            .min_h_0()
                                            .p_3()
                                            .overflow_y_scrollbar()
                                            .child(columns_block),
                                    ),
                            ),
                    ),
            )
            .child(
                resizable_panel()
                    .size(preview_max)
                    .size_range(ui::DETAIL_PREVIEW_MIN_HEIGHT..ui::DETAIL_PREVIEW_MAX_HEIGHT)
                    .child(
                        div()
                            .size_full()
                            .min_h_0()
                            .p_3()
                            .flex()
                            .flex_col()
                            .child(preview_block),
                    ),
            )
            .on_resize(move |state, _window, app| {
                // 拖到哪记到哪：三段的第 3 段是预览（`sizes()[2]`），换算回 rem 存面板字段
                if let Some(h) = state.read(app).sizes().get(2).copied() {
                    let ratio = h / font_size;
                    split_panel.update(app, |panel, cx| {
                        panel.detail_preview_rem = ratio;
                        cx.notify();
                    });
                }
            });

        // 分栏占满 tab 的剩余高度：`ResizablePanelGroup` 自己是 `size_full()`，
        // 直接塞进 `body` 会按整块高度铺满并压过后面的兄弟；套一层 `flex_1().min_h_0()`
        // 才算「吃剩余」。不套时它按内容撑开，三段（含预览固定高度）合计超出窗口。
        body = body.child(div().flex_1().min_h_0().child(split));

        // 键位落点：`track_focus` 不能省（没有它这条 tab 不在 dispatch path 上，快捷键落不到动作）；
        // `key_context("mock-detail")` 与 `crates/app` 的键位注册（Ctrl+Enter = 生成）成对。
        body.track_focus(&self.focus_handle)
            .key_context("mock-detail")
            .on_action(cx.listener(Self::on_generate_key))
    }
}

/// 清单行里的**状态**：`IconName` 图标（走图标组件，不再用 Unicode 字形）+ 语义色文字。
///
/// 必须是 `h_flex`：根容器是块级 `div` 时图标、「 」、文字会各占一行垂直堆叠，
/// 一个状态格撑成三行高（与 `Button` 被塞进块级 div 会居中是同一类容器陷阱）。
/// 图标与文字的间距走 `gap_1`，不再靠插一个空格字符。
pub(crate) fn render_status_cell(icon: Option<IconName>, text: &str, color: Hsla) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .text_xs()
        .text_color(color)
        .when_some(icon, |cell, icon| {
            cell.child(
                Icon::new(icon)
                    .size(rems(ui::STATUS_ICON_SIZE))
                    .text_color(color),
            )
        })
        .child(SharedString::from(text))
}

/// 推给预览表的那一份快照：表名 + 列名 + 行 + 生效中的排序（`None` = 按生成顺序）。
///
/// 三个字段要一起换：只换行不换列会让右键记下的位置错位，只换排序不重建表头会让箭头与数据不一致。
pub(crate) struct PreviewSnapshot {
    /// 哪张表的结果（右键排序时按它去找临时表）
    pub(crate) table: String,
    /// 数据列名（不含行号槽）
    pub(crate) columns: Vec<String>,
    /// 取样行（引擎已渲染成文本；每行长度 == `columns.len()`）
    pub(crate) rows: Vec<Vec<String>>,
    /// 生效中的排序：列名 + 是否降序（面板那边算好的，delegate 只负责映射成列下标）
    pub(crate) sort: Option<(String, bool)>,
}
