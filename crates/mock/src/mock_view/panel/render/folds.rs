//! 底部折叠段：生成历史 + 用户模板（默认收起）。

use super::*;

impl MockPanel {
    /// 底部折叠段：生成历史 + 用户模板（默认收起）。
    ///
    /// 为何收起来：面板天天用的是上面那份「有哪些表」清单，历史与模板是偶尔翻一次的落盘记录；
    /// 展开状态存实体（不是每帧派生的），展开体走 `Collapsible`（不手搮揭示动画）。
    pub(crate) fn render_folds(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
            button("mock-fold-toggle", label, ActionKind::Quiet)
                .xsmall()
                .icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
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
            .child(self.render_history(window, cx))
            // S5 起场景运行也记入历史（标注：scene_id + seed + 场景名，不落单表列行），
            // 所以这里没有「场景不进历史」的说明行——那条口径已随 S5 反转
            .child(self.render_templates(window, cx));

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
    pub(crate) fn render_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // 主题一次 clone：下面循环里还要可变借用 `cx`（`cx.entity()` / `use_keyed_state`）
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;

        let header = {
            let entity = cx.entity();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().flex_1().min_w_0().child(section(
                    &theme,
                    format!("用户模板（{}）", self.templates.len()),
                )))
                .child(
                    button("mock-template-save", "保存为模板…", ActionKind::Quiet)
                        .xsmall()
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
                        .description(
                            EmptyDescription::new().child("把当前列配置存下来，下次一键套用"),
                        ),
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
                button(
                    SharedString::from(format!("mock-template-apply-{id}")),
                    "应用",
                    ActionKind::Quiet,
                )
                .xsmall()
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.apply_template(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = template.id.clone();
                danger_text_button(
                    SharedString::from(format!("mock-template-delete-{id}")),
                    "删除",
                    cx.theme(),
                )
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_template(id, cx));
                })
            };

            // 模板卡片：与历史卡片、列卡片同走 `card` 原语（表面色 + 内距 + 卡内分隔）。
            // 操作是悬停 / 聚焦才显形的——一排卡片上每张都常驻两个按钮，
            // 摆满一屏就是一屏灰按钮，那是表配置不是产品。
            let card_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from(format!(
                        "mock-template-card-{}",
                        template.id
                    ))),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx);
            let card_focused = card_handle.is_focused(window);
            let active = self.hovered_template == Some(template.id.clone()) || card_focused;
            section = section.child(
                card_interactive(&theme)
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-template-{}",
                        template.id
                    ))))
                    .track_focus(&card_handle)
                    .tab_stop(true)
                    .when(card_focused, |c| c.border_color(theme.colors.ring))
                    .on_hover({
                        let entity = cx.entity();
                        let hover_id = template.id.clone();
                        move |&hovering, _, app| {
                            entity.update(app, |panel, cx| {
                                let next = hovering.then(|| hover_id.clone());
                                if panel.hovered_template != next {
                                    panel.hovered_template = next;
                                    cx.notify();
                                }
                            });
                        }
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
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(template.name.clone()),
                            )
                            .child(if active {
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_none()
                                    .child(apply)
                                    .child(delete)
                            } else {
                                div().flex_none().w(rems(ui::ROW_OPS_WIDTH))
                            }),
                    )
                    .child(card_divider(&theme))
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_hint(&theme))
                            .text_ellipsis()
                            .child(meta),
                    ),
            );
        }
        section
    }
}
