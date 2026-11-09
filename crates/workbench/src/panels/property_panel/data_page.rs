use super::*;

impl PropertyHostPanel {
    /// 「数据」页签：只读结果网格（行号 + 列名 + 单元格，NULL 置灰）+ 分页 / 排序。
    ///
    /// 数据来自 [`ObjectProperties::data`]（`rds-database::table_data` 分页抓取）；
    /// 排序 / 翻页走事件路径（`reload_properties_data`），渲染零 I/O。
    pub(crate) fn render_data_page(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        theme: &Theme,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        // 与洞察 / General / 子项同卡片语言：侧栏底 + 边框 + 标题头。
        let mut body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(border)
            .rounded_md()
            .bg(theme.colors.sidebar)
            .child(self.card_header("数据", None, theme));
        let Some(data) = props.data.as_ref() else {
            body = body.child(div().text_xs().text_color(muted).child("（未加载）"));
            return body;
        };
        if data.columns.is_empty() {
            body = body.child(div().text_xs().text_color(muted).child("（无）"));
            return body;
        }

        // 当前请求参数（翻页 / 排序基线）。
        let current = self
            .property_target
            .borrow()
            .clone()
            .and_then(|r| r.data)
            .unwrap_or_default();
        let current_offset = current.offset;
        let current_limit = if current.limit == 0 {
            DEFAULT_PAGE_LIMIT
        } else {
            current.limit
        };

        // 表头：行号槽 + 列名（可点击排序——DBeaver 式）。
        let mut header = div()
            .h_flex()
            .w_full()
            .gap_2()
            .pt_1()
            .pb_1()
            .text_xs()
            .text_color(muted);
        header = header.child(
            div()
                .w(rems(ui::PROPERTY_ROW_NUMBER_WIDTH))
                .flex_none()
                .child("#"),
        );
        for (ix, col) in data.columns.iter().enumerate() {
            let sorted = current
                .order_by
                .iter()
                .find(|(c, _)| c == &col.name)
                .map(|(_, asc)| if *asc { " ↑" } else { " ↓" })
                .unwrap_or("");
            let entity = entity.clone();
            let col_name = col.name.clone();
            let current = current.clone();
            header = header.child(
                // 列头排序迁移到语义 `Button`（2026-10-15）：拿到 hover / 焦点 / 键盘，
                // 不再自绘可点 div；id 保留业务键（列名 + 下标，与行渲染一致）。
                Button::new(SharedString::from(format!("data-col-{}-{ix}", col.name)))
                    .ghost()
                    .xsmall()
                    .flex_1()
                    .min_w_0()
                    .label(format!("{}{}", col.name, sorted))
                    .on_click(move |_ev, _window, app| {
                        // 点击列头：同列切向 → 升序 → 降序（循环），回到第一页。
                        let asc = match current.order_by.iter().find(|(c, _)| c == &col_name) {
                            Some((_, true)) => false,
                            Some((_, false)) => true,
                            None => true,
                        };
                        let mut order_by = current
                            .order_by
                            .iter()
                            .filter(|(c, _)| c != &col_name)
                            .cloned()
                            .collect::<Vec<_>>();
                        order_by.push((col_name.clone(), asc));
                        entity.update(app, |this, cx| {
                            this.reload_properties_data(
                                TableDataRequest {
                                    offset: 0,
                                    limit: current.limit,
                                    order_by,
                                },
                                cx,
                            );
                        });
                    }),
            );
        }
        body = body.child(header);

        // 数据行。
        for (i, row) in data.rows.iter().enumerate() {
            let row_no = data.offset + i + 1;
            let mut line = div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .border_b_1()
                .border_color(border)
                .text_color(fg);
            line = line.child(
                div()
                    .w(rems(ui::PROPERTY_ROW_NUMBER_WIDTH))
                    .flex_none()
                    .text_color(muted)
                    .child(row_no.to_string()),
            );
            for cell in row {
                line = match cell {
                    Some(v) => line.child(div().flex_1().min_w_0().py_px().child(v.clone())),
                    None => line.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .py_px()
                            .text_color(muted)
                            .child("NULL"),
                    ),
                };
            }
            body = body.child(line);
        }

        // 页脚：行数 + 顺序提示 + 分页按钮。
        let total_text = data
            .total
            .map(|t| format!("共 {t} 行"))
            .unwrap_or_else(|| "行数未知".to_string());
        let mut footer = div()
            .h_flex()
            .items_center()
            .gap_2()
            .pt_2()
            .text_xs()
            .text_color(muted)
            .child(div().child(total_text));
        if !data.is_ordered {
            footer = footer.child(div().child("（本页无定义顺序）"));
        }
        if current_offset > 0 {
            let entity = entity.clone();
            let current = current.clone();
            footer = footer.child(
                Button::new("data-prev")
                    .label("‹ 上一页")
                    .with_size(Size::Small)
                    .on_click(move |_, _, app| {
                        let new_offset = current_offset.saturating_sub(current_limit);
                        entity.update(app, |this, cx| {
                            let mut req = current.clone();
                            req.offset = new_offset;
                            this.reload_properties_data(req, cx);
                        });
                    }),
            );
        }
        if data.has_more {
            let entity = entity.clone();
            let current = current.clone();
            footer = footer.child(
                Button::new("data-next")
                    .label("下一页 ›")
                    .with_size(Size::Small)
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| {
                            let mut req = current.clone();
                            req.offset = current_offset + current_limit;
                            this.reload_properties_data(req, cx);
                        });
                    }),
            );
        }
        body = body.child(footer);
        body
    }
}
