use super::*;

impl PropertyHostPanel {
    /// 属性面板（DBeaver 式：tab 页签 + 描述驱动渲染）。
    ///
    /// 页集合来自 [`PropertyRegistry`]（`PropertyRegistryImpl`），渲染只遍历页描述，
    /// 不按 `PropertyKind` 写分支；暂不支持页（权限 / 依赖）先绘制、置灰标注
    /// （`docs/architecture/property/property-module-spec.md` §8）。
    pub(crate) fn render_property_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let Some(target) = self.property_target.borrow().clone() else {
            let muted = cx.theme().colors.muted_foreground;
            return div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(muted)
                .child("未选择对象：双击导航树中的连接 / 数据库 / 表等对象查看属性");
        };

        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;

        let st = self.property.borrow();

        // clone 主题对象：渲染子树需要同时拿 `&Theme` 与 `&mut cx`（`cx.theme()` 会一直
        // 借用 cx，直接传会 E0502；Theme 是轻量值类型，clone 成本可忽略）。
        let theme = cx.theme().clone();
        let entity = cx.entity();

        // 惰性创建顶部 filter 搜索框（DBeaver 属性面板右上角搜索）：首次渲染挂订阅，
        // Change 事件把输入值写回 self.filter → render 时过滤 rows / sections。
        if self.filter_input.borrow().is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("过滤属性…"));
            let sub = cx.subscribe_in(
                &input,
                window,
                move |this: &mut Self, _: &Entity<InputState>, ev: &InputEvent, _window, cx| {
                    if let InputEvent::Change = ev {
                        let value = this
                            .filter_input
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .read(cx)
                            .value()
                            .to_string();
                        *this.filter.borrow_mut() = value;
                        cx.notify();
                    }
                },
            );
            *self.filter_input.borrow_mut() = Some(input);
            *self.filter_sub.borrow_mut() = Some(sub);
        }
        let filter = self.filter.borrow().clone();
        let filter_input = self.filter_input.borrow().clone().unwrap();
        // 表 / 视图两页签【属性 | 数据】；其余对象为单属性视图（无页签条）。
        let is_tabbed = matches!(
            target.property.kind,
            property::PropertyKind::Table | property::PropertyKind::View
        );
        let active = st.active_page;
        let tabs: Vec<(PageId, &str)> = if is_tabbed {
            vec![(PageId::General, "属性"), (PageId::Data, "数据")]
        } else {
            vec![(PageId::General, "属性")]
        };
        let active_index = tabs.iter().position(|(id, _)| *id == active).unwrap_or(0);

        let mut body = div().v_flex().w_full().gap_1();
        if st.loading {
            body = body.child(div().text_xs().text_color(muted).child("加载中…"));
        }
        if let Some(err) = &st.error {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(err.clone()),
            );
        }
        if let Some(props) = &st.props {
            if active == PageId::Data {
                body = body.child(self.render_data_page(entity.clone(), props, &theme));
            } else {
                body = body.child(self.render_property_view(
                    entity.clone(),
                    props,
                    &filter,
                    &theme,
                    window,
                    cx,
                ));
            }
        }

        let mut tab_bar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .border_b_1()
            .border_color(border);
        if is_tabbed {
            tab_bar = tab_bar.child(
                TabBar::new("property-tabs")
                    .underline()
                    .with_size(Size::Small)
                    .selected_index(active_index)
                    .children(
                        tabs.iter()
                            .map(|(_, label)| Tab::new().label(label.to_string())),
                    )
                    .on_click({
                        let entity = cx.entity();
                        let property = self.property.clone();
                        let target = self.property_target.clone();
                        let tabs = tabs.clone();
                        move |ix, _window, app| {
                            if let Some((page_id, _)) = tabs.get(*ix) {
                                property.borrow_mut().active_page = *page_id;
                                entity.update(app, |this, cx| {
                                    // 「数据」页签懒加载：切到数据页且未请求过数据时，
                                    // 自动带默认分页参数重载（首屏 200 行）。
                                    if *page_id == PageId::Data {
                                        let has_data = this
                                            .property
                                            .borrow()
                                            .props
                                            .as_ref()
                                            .map(|p| p.data.is_some())
                                            .unwrap_or(false);
                                        if !has_data {
                                            if let Some(req) = target.borrow().clone() {
                                                let mut req = req;
                                                req.data = Some(TableDataRequest::default());
                                                this.show_properties(req, cx);
                                                return;
                                            }
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
        }

        // 顶部 filter 搜索框（仅 General 页签显示；Data 页签不过滤）。
        let filter_bar = if active == PageId::General {
            div()
                .h_flex()
                .items_center()
                .w_full()
                .gap_1()
                .px_3()
                .py_1()
                .border_b_1()
                .border_color(border)
                .child(Input::new(&filter_input).w_full().with_size(Size::Small))
        } else {
            div().w_full()
        };

        // 底部 Apply/Revert 按钮行（DBeaver：有未提交草稿时才显示）。
        let dirty_count = self.dirty.borrow().len();
        let footer = if dirty_count > 0 {
            let entity = cx.entity();
            let entity2 = cx.entity();
            div()
                .h_flex()
                .items_center()
                .justify_end()
                .gap_2()
                .w_full()
                .px_3()
                .py_1()
                .border_t_1()
                .border_color(border)
                .child(
                    div().text_xs().text_color(muted).child(format!("{} 项未保存", dirty_count)),
                )
                .child(
                    Button::new("property-revert")
                        .label("Revert")
                        .with_size(Size::Small)
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |panel, cx| panel.revert_dirty(cx));
                        }),
                )
                .child(
                    Button::new("property-apply")
                        .label("Apply")
                        .primary()
                        .with_size(Size::Small)
                        .on_click(move |_, _, cx| {
                            entity2.update(cx, |panel, cx| panel.apply_dirty(cx));
                        }),
                )
        } else {
            div().w_full()
        };

        div()
            .v_flex()
            .w_full()
            .h_full()
            .flex_none()
            .child(tab_bar)
            .child(filter_bar)
            .child(
                div()
                    .id("property-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px_3()
                    .py_2()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(footer)
    }

    /// 属性视图（三部分布局，DBeaver 之上加「洞察总览」——见 §16 设计）：
    /// ① 洞察总览（运行时 / 统计事实，provider 组装）
    /// ② General 双栏网格（定义属性；行数多时自动三栏，增加面积使用率）
    /// ③ 子项类别列表（驱动能力掩码；未实现类别隐藏式不产出，2026-10-24 拍板）
    ///
    /// 大区块统一「卡片语言」：`border + rounded + sidebar 底 + 标题头`（与洞察卡一致，
    /// 连接编辑器 / 洞察 / Mock 模块同规范，见 `property-module-spec.md` §17）。
    pub(crate) fn render_property_view(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        filter: &str,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = div().v_flex().w_full().gap_3();
        body = body.child(self.render_overview_section(props, theme));
        body =
            body.child(self.render_general_grid(entity.clone(), props, filter, theme, window, cx));
        body = body.child(self.render_category_list(entity.clone(), props, filter, theme));
        body
    }

    /// 卡片头：标题 + 可选尾注（规范：`text_xs` 中字重标题 + `muted` 尾注）。
    pub(crate) fn card_header(&self, title: &str, note: Option<&str>, theme: &Theme) -> Div {
        let mut head = div().h_flex().items_center().gap_2().w_full().child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.colors.foreground)
                .child(title.to_string()),
        );
        if let Some(note) = note {
            head = head.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(note.to_string()),
            );
        }
        head
    }
}
