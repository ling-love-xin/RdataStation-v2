//! 样本域渲染：样本区 / 字段选择 / 取样进度 / 列画像宽版。

use super::*;

impl super::InsightWorkspacePanel {
    /// 样本区（原型 .sample-section）：前 5 行表格（列名 + 文本化值）。
    pub(crate) fn render_sample_section(
        &self,
        profile: &TableProfileView,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let Some(sample) = &profile.sample else {
            return panel_box(
                &theme,
                "样本",
                "前 5 行".into(),
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(CatalogIcon::Table)
                            .with_size(Size::Size(inline_icon))
                            .text_color(colors.muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("样本不可用（临时表为空或已过期）"),
                    ),
            );
        };
        if sample.rows.is_empty() {
            return panel_box(
                &theme,
                "样本",
                "前 5 行".into(),
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("表为空，没有可展示的样本行"),
            );
        }
        // 列头
        let mut head = div().h_flex().w_full().gap_2().px_2().py_1();
        for (ix, name) in sample.columns.iter().enumerate() {
            head = head.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(name.clone())
                    .when(ix == 0, |this| {
                        this.w(rems(ui::INSIGHT_WS_SOURCE_WIDTH)).flex_none()
                    }),
            );
        }
        let mut body = div().v_flex().w_full();
        for row in &sample.rows {
            let mut line = div()
                .h_flex()
                .w_full()
                .gap_2()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(colors.border);
            for (ix, cell) in row.iter().enumerate() {
                line = line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(if cell == "NULL" {
                            colors.muted_foreground
                        } else {
                            colors.foreground
                        })
                        .child(cell.clone())
                        .when(ix == 0, |this| {
                            this.w(rems(ui::INSIGHT_WS_SOURCE_WIDTH)).flex_none()
                        }),
                );
            }
            body = body.child(line);
        }
        panel_box(
            theme,
            "样本",
            format!(
                "前 {} 行（共 {} 行）",
                sample.rows.len(),
                fmt_rows(profile.row_count)
            ),
            div().v_flex().w_full().child(head).child(body),
        )
    }

    /// 字段筛选器（原型 .field-picker）：搜索定位 + 类型筛选 + 列多选（200+ 列场景）。
    ///
    /// 组件选型（禁手搓）：搜索 = `Input`（`use_keyed_state` 惰性创建）；类型 =
    /// `Button::dropdown_menu`（`PopupMenuItem::checked`，与目标切换同款）；列多选 =
    /// `Popover` + `Checkbox` 逐列（原生浮层与键盘可达，不做自绘弹窗）。
    pub(crate) fn render_field_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        _theme: &Theme,
    ) -> Div {
        let columns = &profile.columns;
        let sel_len = if self.sel_cols.is_empty() {
            columns.len()
        } else {
            self.sel_cols.len()
        };
        let col_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        let kind_items: Vec<(&'static str, Option<ColumnKind>)> = vec![
            ("全部", None),
            ("数值", Some(ColumnKind::Numeric)),
            ("文本", Some(ColumnKind::Text)),
            ("时间", Some(ColumnKind::DateTime)),
            ("布尔", Some(ColumnKind::Boolean)),
            ("未识别", Some(ColumnKind::Unknown)),
        ];
        // 搜索输入：渲染管线惰性创建（元素消失自动回收，同 property_panel 的常驻
        // SelectState 模式）；订阅只挂一次，Change → 写面板过滤词（只读镜像，
        // 输入权威仍在 InputState）。
        let search_state = window.use_keyed_state(
            SharedString::from("insight-ws-col-search"),
            cx,
            |window, cx| InputState::new(window, cx).placeholder("搜索列名"),
        );
        if self.col_search_subs.borrow().is_empty() {
            let sub = cx.subscribe_in(
                &search_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    this.col_q = emitter.read(cx).value().to_string();
                    cx.notify();
                },
            );
            self.col_search_subs.borrow_mut().push(sub);
        }
        // 弹层内列名过滤框：独立于 `col_q`（只影响勾选清单，不驱动卡片列表）；
        // state 在主体惰性创建、同键只挂一次订阅，弹层 content 直接复用同一实体。
        let picker_search_state = window.use_keyed_state(
            SharedString::from("insight-ws-picker-search"),
            cx,
            |window, cx| InputState::new(window, cx).placeholder("筛选列名"),
        );
        if self.picker_search_subs.borrow().is_empty() {
            let sub = cx.subscribe_in(
                &picker_search_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    this.picker_q = emitter.read(cx).value().to_string();
                    cx.notify();
                },
            );
            self.picker_search_subs.borrow_mut().push(sub);
        }
        let kind_label = kind_items
            .iter()
            .find(|(_, k)| *k == self.kind_filter)
            .map(|(label, _)| *label)
            .unwrap_or("全部");

        div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            // 搜索定位
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&search_state).cleanable(true)),
            )
            // 类型筛选：下拉（PopupMenuItem 自带选中态与键盘；闭包捕获构建时快照，
            // Popover/DropdownMenu 的 content 在每次 render 重建，新值自然生效）
            .child({
                let entity = entity.clone();
                let kind_now = self.kind_filter;
                Button::new("insight-ws-kind-filter")
                    .ghost()
                    .xsmall()
                    .icon(CatalogIcon::ListFilter)
                    .label(kind_label)
                    .tooltip("按类型族筛选列")
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for (label, kind) in &kind_items {
                            let entity = entity.clone();
                            // 解引用为 owned（ColumnKind: Copy）：on_click move 闭包不能捕获 & 迭代引用
                            let kind = *kind;
                            let checked = kind == kind_now;
                            menu = menu.item(
                                PopupMenuItem::new((*label).to_string())
                                    .checked(checked)
                                    .on_click(move |_, _, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.kind_filter = kind;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        menu
                    })
            })
            // 列多选浮层：Popover（trigger = Button，content = 搜索框 + Checkbox 列表）
            .child({
                let entity = entity.clone();
                let picker_search_state = picker_search_state.clone();
                // 弹层内过滤词快照（渲染期只读；输入权威在 InputState，订阅回写 picker_q）
                let panel_q = self.picker_q.clone();
                // 当前显式选集（空 = 全选，此时回显全选）；浮层内容闭包捕获此快照
                let current: Vec<String> = if self.sel_cols.is_empty() {
                    col_names.clone()
                } else {
                    self.sel_cols.clone()
                };
                let all_sel = current.len() == col_names.len();
                Popover::new("insight-ws-col-pick")
                    .anchor(Anchor::TopRight)
                    .trigger(
                        Button::new("insight-ws-col-pick-btn")
                            .ghost()
                            .xsmall()
                            .icon(CatalogIcon::ListFilter)
                            .label(format!("列 {sel_len}/{}", columns.len()))
                            .tooltip("选择要看的列（200+ 列时建议缩到几个字段）"),
                    )
                    .content(move |_state, _window, cx| {
                        let colors = cx.theme().colors;
                        let all_entity = entity.clone();
                        let none_entity = entity.clone();
                        // 拥有副本：content 闭包 move 捕获 col_names 后，for 迭代与
                        // 子闭包各持一份，避免「Fn 闭包 move 后外部再借」冲突
                        let names = col_names.clone();
                        let all_names = names.clone();
                        // 反选按钮专属副本：其 on_click move 捕获后不挤占循环的 all_names
                        let none_names = all_names.clone();
                        let mut body = div()
                            .v_flex()
                            .w(rems(ui::INSIGHT_PICKER_WIDTH))
                            .gap_1()
                            .px_2()
                            .py_2()
                            .child(
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("insight-ws-col-all")
                                            .ghost()
                                            .xsmall()
                                            .disabled(all_sel)
                                            .label("全选")
                                            .on_click(move |_, _, app| {
                                                all_entity.update(app, |panel, cx| {
                                                    panel.sel_cols.clear();
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                    .child(
                                        Button::new("insight-ws-col-none")
                                            .ghost()
                                            .xsmall()
                                            .label("反选")
                                            .on_click(move |_, _, app| {
                                                none_entity.update(app, |panel, cx| {
                                                    // 显式清单 = 全集 − 当前（结果为空时退回全选语义，
                                                    // 避免「空 = 未筛选」歧义）
                                                    let names = none_names.clone();
                                                    let cur = panel.sel_cols.clone();
                                                    let next: Vec<String> = names
                                                        .iter()
                                                        .filter(|n| !cur.contains(n))
                                                        .map(|n| n.to_string())
                                                        .collect();
                                                    panel.sel_cols = if next.is_empty() {
                                                        Vec::new()
                                                    } else {
                                                        next
                                                    };
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            .child(div().w_full().h_px().bg(colors.border))
                            .child(Input::new(&picker_search_state).cleanable(true));
                        // 弹层内列名子串过滤（大小写不敏感）；只定位勾选清单，
                        // 不改变全选 / 反选作用的全集口径。
                        let filtered: Vec<&String> = names
                            .iter()
                            .filter(|n| {
                                panel_q.is_empty()
                                    || n.to_lowercase().contains(&panel_q.to_lowercase())
                            })
                            .collect();
                        if filtered.is_empty() {
                            body = body.child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("没有匹配的列"),
                            );
                        }
                        for name in filtered {
                            let entity = entity.clone();
                            let name = (*name).clone();
                            let label = name.clone();
                            let all = all_names.clone();
                            let checked = current.contains(&name);
                            body = body.child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Checkbox::new(SharedString::from(format!(
                                            "insight-ws-col-{name}"
                                        )))
                                        .checked(checked)
                                        .on_click(
                                            move |_, _, app| {
                                                entity.update(app, |panel, cx| {
                                                    // 首次点选时把「隐式全选」物化为显式清单
                                                    if panel.sel_cols.is_empty() {
                                                        panel.sel_cols = all.clone();
                                                    }
                                                    if let Some(pos) = panel
                                                        .sel_cols
                                                        .iter()
                                                        .position(|n| n == &name)
                                                    {
                                                        panel.sel_cols.remove(pos);
                                                    } else {
                                                        panel.sel_cols.push(name.clone());
                                                    }
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    )
                                    .child(
                                        div().text_xs().text_color(colors.foreground).child(label),
                                    ),
                            );
                        }
                        body
                    })
            })
    }

    pub(crate) fn render_sample_progress(&self, cx: &Context<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let note = self
            .view
            .read(cx)
            .loading_note()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let stage = if note.contains("取样") {
            0
        } else if note.contains("画像") {
            1
        } else {
            0
        };
        let target = self.view.read(cx).target().cloned();

        // 信息表：取样行数 / 源目标 / 临时表（stage ≥ 1 时显示）
        let info = if stage >= 1 {
            let temp = target
                .as_ref()
                .and_then(|t| self.view.read(cx).data().sample_table(t))
                .unwrap_or("—");
            let detail = target
                .as_ref()
                .and_then(|t| t.detail())
                .unwrap_or_else(|| "—".into());
            Some(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(div().child(format!(
                        "取样行数 {SOURCE_SAMPLE_LIMIT} / {SOURCE_SAMPLE_LIMIT}"
                    )))
                    .child(div().child(format!("源目标 {detail}")))
                    .child(div().child(format!("临时表 {temp}"))),
            )
        } else {
            None
        };

        div()
            .v_flex()
            .w_full()
            .gap_3()
            .p(rems(ui::PANEL_PADDING))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(IconName::Eye).text_color(colors.info))
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.foreground)
                            .child("洞察分析中"),
                    )
                    .child(
                        div().text_xs().text_color(colors.muted_foreground).child(
                            target
                                .as_ref()
                                .map(|t| t.title())
                                .unwrap_or_else(|| "…".into()),
                        ),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(step_dot("源取样", stage == 0, stage >= 1, theme))
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .h_px()
                            .bg(colors.border),
                    )
                    .child(step_dot("画像", stage == 1, stage >= 2, theme)),
            )
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .items_center()
                    .child(pulse_dot(theme))
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(note),
                    ),
            )
            .child(
                Progress::new("insight-ws-sample-loading")
                    .loading(true)
                    .with_size(Size::Small)
                    .w_full(),
            )
            .when_some(info, |this, info| this.child(info))
            .child(skeleton(theme))
    }

    /// 列目标画像：复用列画像四区版式（宽版）。
    pub(crate) fn render_column_overview(
        &self,
        cx: &Context<Self>,
        profile: &crate::model::ColumnProfileView,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let view = self.view.clone();
        let rendered = view
            .read(cx)
            .render_column_profile(profile, &view, theme, inline_icon);
        div()
            .v_flex()
            .w_full()
            .p(rems(ui::PANEL_PADDING))
            .child(rendered)
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("列画像 · 右键结果表中的列可查看"),
            )
    }
}
