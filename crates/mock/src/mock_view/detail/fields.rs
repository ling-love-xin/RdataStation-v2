//! 字段区渲染：列编辑区的字段行。

use super::*;

impl MockDetailView {
    pub(crate) fn render_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // 每张 tab 的列操作都落回它自己那份配置
        let config_id = self.config_id();
        // 卡片 / chip / 按钮的形态都从 `style` 原语取，这里只留卡片说明行的两色
        let theme = cx.theme().clone();
        let muted = theme.colors.muted_foreground;
        let info = theme.colors.info;

        // 先看「这张表有没有列」再套筛选：空表与「筛没了」是两句不同的话
        let has_any = {
            let id = self.config_id();
            let panel = self.panel.read(cx);
            id.map(|id| !panel.config_for(id).columns.is_empty())
                .unwrap_or(false)
        };
        if !has_any {
            return div()
                .text_xs()
                .text_color(muted)
                .child("暂无列：在这张 tab 里点「导入结构」或「＋ 加列」开始造表。")
                .into_any_element();
        }
        let columns = {
            let panel = self.panel.read(cx);
            self.visible_columns(panel, config_id.unwrap_or_default(), cx)
        };
        if columns.is_empty() {
            return div()
                .text_xs()
                .text_color(muted)
                .child("没有匹配的列——清空「筛选列」看全部（匹配列名与生成器名）。")
                .into_any_element();
        }

        // 单行卡片之间 4px 就够（8px 会让 20 列多出 80px 纯空白）
        let mut list = div().v_flex().gap_1().w_full();
        // enumerate 拿序号：首 / 末列禁用对应的 ↑/↓（列序 = 生成顺序）
        for (index, column) in columns.iter().enumerate() {
            let spec = generator_catalog::spec_of(&column.def.generator);
            let summary = summarize_params(&column.def.generator);
            let id = column.id;
            // 自定义项显示**用户起的名字**（同一 base 的内建项另有名字，显示 base 名会认错）
            let label = column
                .user_generator
                .as_ref()
                .and_then(|user_id| {
                    self.panel
                        .read(cx)
                        .user_generators()
                        .iter()
                        .find(|ug| ug.id == *user_id)
                        .map(|ug| ug.name.clone())
                })
                .unwrap_or_else(|| spec.label.to_string());
            let unique = if column.def.unique { "唯一 · " } else { "" };
            let null_percent = (column.def.nullable_ratio * 100.0).round() as i64;
            let detail = if summary.is_empty() {
                column.sample_value.clone()
            } else {
                summary.clone()
            };
            // S1④：跨表引用徽标 + 常驻说明（挂在列上的关系，不是外键 DDL）
            let ref_badge = column
                .def
                .dependency
                .as_ref()
                .map(|dep| format!("↳ 引用 {}.{}", dep.ref_table, dep.ref_column));
            let ref_note =
                column.def.dependency.is_some().then(
                    || "引用目标仅支持自增主键，按自增规律推算取值域，不读取父表已生成的数据。",
                );
            let panel = self.panel.clone();
            let entity = cx.entity();
            let is_first = index == 0;
            let is_last = index + 1 == columns.len();
            // 卡片焦点句柄：卡片要能被 Tab 到（四个操作是悬停才显形的，键盘用户没有悬停，
            // 焦点落在卡片上时也必须显形）。按业务键（列 id）缓存句柄——每帧新建会让
            // 焦点在重绘时丢（`use_keyed_state` 是按元素 id 缓存的）。
            let card_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from(format!("mock-field-card-{id}"))),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                // 克隆成自有值：`read(cx)` 返回的引用会一路借住 `cx`，
                // 而下面 `card_border` 的 `transition(..)` 要可变借用 `cx`
                .clone();
            let card_focused = card_handle.is_focused(window);
            // hovered：鼠标扫过 或 键盘焦点停在这行 → 整行高亮
            let active = self.hovered_column == Some(id) || card_focused;

            // ── 单行：列名 · 类型 · 生成器 · 参数摘要 · 操作（常驻）──
            // A 方案的落点：两行卡片 20 列要 1300px+，一屏只看得到 5 列；压成 34px 一行后，
            // 同样高度能看 3 倍，且四样信息落在同一基线上，竖着扫就能比（不上升成表格）。
            // 操作常驻而不是悬停才冒出来：不可发现的按钮等于不存在，用户只会用「删除」。
            let summary_text = format!("{unique}空值 {null_percent}% · {detail}");
            // 悬停全文 = 生成器名 + 参数全文；引用列的「为什么不读父表数据」并进来
            // （单行放不下一整行解释，但这句话必须仍可查）
            let full_text = match ref_note {
                Some(note) if summary.is_empty() => format!("生成器：{label}\n\n{note}"),
                Some(note) => format!("生成器：{label}\n{summary}\n\n{note}"),
                None if summary.is_empty() => format!("生成器：{label}"),
                None => format!("生成器：{label}\n{summary}"),
            };
            let is_reference = ref_badge.is_some();

            // 列名条：宽度自适应内容（最长 12rem=192px 截断），不再定死 176px。
            // 短名字（id）不浪费空间，长名字最多 192px 截断。
            let name_cell = div()
                .h_flex()
                .items_center()
                .flex_none()
                .max_w(rems(12.))
                .px_1()
                .rounded(ui::CHIP_RADIUS)
                .bg(theme.colors.background)
                .border_1()
                .border_color(theme.colors.border)
                .child(card_title(column.def.name.clone()).text_ellipsis());
            // 类型段：类型 + 置信度 + 引用信号，三样都是徽标，紧跟在列名条后面自成一段
            let mut meta_cell = div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .child(data_type_tag(&theme, &column.def.data_type));
            // high 是默认档：每列都挂一个绿色 high，只会把「哪列可信度低」淹掉
            if column.confidence != "high" {
                meta_cell = meta_cell.child(confidence_tag(&theme, &column.confidence));
            }
            // 引用列只留一个 ↳：完整「引用 表.列」在悬停全文里，行内宽度留给参数摘要
            if is_reference {
                meta_cell =
                    meta_cell.child(div().flex_none().text_xs().text_color(info).child("↳"));
            }

            // 操作区常驻（四个都有 tooltip；ghost 图标不抢卡片内容的视觉重量）
            let ops = div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_none()
                .child({
                    let panel = panel.clone();
                    icon_button(
                        format!("mock-moveup-{id}"),
                        IconName::ArrowUp,
                        "上移（列序即生成顺序）",
                    )
                    .disabled(is_first)
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.move_column(id, true, cx)
                        });
                    })
                })
                .child({
                    let panel = panel.clone();
                    icon_button(
                        format!("mock-movedown-{id}"),
                        IconName::ArrowDown,
                        "下移（列序即生成顺序）",
                    )
                    .disabled(is_last)
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.move_column(id, false, cx)
                        });
                    })
                })
                .child({
                    let panel = panel.clone();
                    icon_button(
                        format!("mock-copy-{id}"),
                        IconName::Copy,
                        "复制列（快速造相似字段）",
                    )
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.duplicate_column(id, cx)
                        });
                    })
                })
                .child(
                    danger_icon_button(
                        format!("mock-del-{id}"),
                        IconName::Delete,
                        "删除列",
                        &theme,
                    )
                    .on_click(move |_, _, app| {
                        panel.update(app, |panel, cx| {
                            if let Some(config) = config_id {
                                panel.focus_config(config);
                            }
                            panel.remove_column(id, cx)
                        });
                    }),
                );

            let summary_el = HoverCard::new(ElementId::Name(SharedString::from(format!(
                "mock-hover-{id}"
            ))))
            .open_delay(std::time::Duration::from_millis(300))
            .trigger_style(StyleRefinement::default().flex_1().min_w_0())
            .trigger(card_note(&theme, summary_text))
            .content(move |_, _, cx| {
                // content 是 `Fn`（可多次调用）：每次克隆文本，不消耗捕获值
                div()
                    .p_2()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(full_text.clone())
            });

            // 列行：平时透明（整个区域无背景色），hover / focus 时整行 list_hover 底色反馈。
            // 此前每行都是 card（muted 底 + border），整个区域像一叠小卡片，太重。
            let row = div()
                .id(ElementId::Name(SharedString::from(format!(
                    "mock-field-{id}"
                ))))
                .h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .min_h(ui::COLUMN_ROW_HEIGHT)
                .rounded(theme.radius)
                .bg(if active {
                    theme.colors.list_hover
                } else {
                    transparent_black()
                })
                // 卡片本身仍可 Tab 到：操作虽已常驻，键盘用户要靠焦点知道停在哪一列
                .track_focus(&card_handle)
                .tab_stop(true)
                .on_hover({
                    let entity = cx.entity();
                    move |&hovering, _, app| {
                        entity.update(app, |view, cx| {
                            let next = hovering.then_some(id);
                            if view.hovered_column != next {
                                view.hovered_column = next;
                                cx.notify();
                            }
                        });
                    }
                })
                .child(name_cell)
                // 生成器入口 = 编辑入口（同一个对话框：选生成器 + 调参数）
                .child(
                    div()
                        .flex_none()
                        .overflow_hidden()
                        .child(
                            button(
                                SharedString::from(format!("mock-gen-{id}")),
                                label,
                                ActionKind::Quiet,
                            )
                            .xsmall()
                            .dropdown_caret(true)
                            .tooltip("打开列编辑：选生成器 + 调参数（同一处改完再应用）")
                            .tooltip_show_delay(TOOLTIP_DELAY)
                            .on_click(move |_, window, app| {
                                entity.update(app, |view, cx| {
                                    view.open_column_dialog(id, window, cx);
                                });
                            }),
                        ),
                )
                .child(summary_el)
                // 三个 chip（类型 / 置信度 / 引用）挪到右侧：左边是「字段 + 生成器 + 参数」的工作区，
                // chip 是元信息，和操作区一起靠右，视觉上不挤字段名
                .child(meta_cell)
                .child(ops);
            // （原「正在编辑这一列」的左缘 3px 强调条已撤：它在新表首列上常驻，
            // 与列名条的内凹底叠在一起像个 bug；真要找回，改 `current_bar` 再挂回来）
            list = list.child(row);
        }

        div()
            .id("mock-field-list")
            .v_flex()
            .gap_1()
            .w_full()
            .child(list)
            .into_any_element()
    }
}