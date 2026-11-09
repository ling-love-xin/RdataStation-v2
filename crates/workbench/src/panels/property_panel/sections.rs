use super::*;

impl PropertyHostPanel {
    /// ① 洞察总览：标题 + 状态点 + meta 芯片 + 指标卡 + 关键信息 + 口径注脚。
    pub(crate) fn render_overview_section(&self, props: &ObjectProperties, theme: &Theme) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;
        let ov = &props.overview;
        let status_color = match ov.status {
            property::OverviewStatus::Connected => theme.colors.success,
            property::OverviewStatus::Disconnected => theme.colors.danger,
            property::OverviewStatus::Idle => muted,
        };

        let mut section = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(border)
            .rounded_md()
            .bg(theme.colors.sidebar);

        // 头部：标题 + 状态点 + 状态文案 + 子标题。
        let mut head = div().h_flex().items_center().gap_2().w_full();
        head = head
            .child(
                div()
                    .w(rems(ui::PROPERTY_STATUS_DOT_SIZE))
                    .h(rems(ui::PROPERTY_STATUS_DOT_SIZE))
                    .flex_none()
                    .rounded_full()
                    .bg(status_color),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg)
                    .child(ov.title.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(ov.status_text.clone()),
            )
            .child(div().flex_1())
            .child(div().text_xs().text_color(muted).child(ov.subtitle.clone()));

        // meta 芯片（驱动 / 作用域 / 分组 / 标签——RDS 连接模型独有信息）。
        let mut chips = div().h_flex().items_center().gap_1().w_full().flex_wrap();
        for (label, value) in &ov.meta {
            chips = chips.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_px()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .text_xs()
                    .child(div().text_color(muted).child(label.clone()))
                    .child(div().text_color(fg).child(value.clone())),
            );
        }

        // 指标卡（横排）。
        let mut cards = div().h_flex().items_stretch().gap_2().w_full().flex_wrap();
        for c in &ov.cards {
            cards = cards.child(
                div()
                    .v_flex()
                    .gap_1()
                    .min_w(rems(ui::PROPERTY_CARD_MIN_WIDTH))
                    .flex_1()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .bg(theme.colors.background)
                    .child(div().text_xs().text_color(muted).child(c.label.clone()))
                    .child(
                        div()
                            .h_flex()
                            .items_baseline()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(fg)
                                    .child(c.value.clone()),
                            )
                            .child(div().text_xs().text_color(muted).child(c.unit.clone())),
                    )
                    .when_some(c.delta.clone(), |el, delta| {
                        el.child(div().text_xs().text_color(muted).child(delta))
                    }),
            );
        }

        // 关键信息（键值行）。
        let mut kv = div().v_flex().w_full().gap_px().text_xs();
        for (label, value) in &ov.kv {
            kv = kv.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .w(rems(ui::PROPERTY_KEY_LABEL_WIDTH))
                            .flex_none()
                            .text_color(muted)
                            .child(label.clone()),
                    )
                    .child(div().flex_1().min_w_0().text_color(fg).child(value.clone())),
            );
        }

        section = section.child(head).child(chips);
        if !ov.cards.is_empty() {
            section = section.child(cards);
        }
        if !ov.kv.is_empty() {
            section = section.child(kv);
        }
        if !ov.note.is_empty() {
            section = section.child(div().text_xs().text_color(muted).child(ov.note.clone()));
        }
        section
    }

    /// ② General 双栏网格（DBeaver General 扁平紧凑 + 面积使用率；行数 > 8 自动三栏）。
    /// 大区块卡片化：与洞察卡同一「边框 + 圆角 + 侧栏底」语言。
    pub(crate) fn render_general_grid(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        filter: &str,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = theme.colors.muted_foreground;
        // filter 过滤：label 或 value 包含（大小写不敏感）。
        let needle = filter.to_lowercase();
        let rows: Vec<&PropertyRow> = props
            .properties
            .iter()
            .filter(|r| {
                needle.is_empty()
                    || r.label.to_lowercase().contains(&needle)
                    || r.value.to_lowercase().contains(&needle)
            })
            .collect();
        let mut section = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(theme.colors.border)
            .rounded_md()
            .bg(theme.colors.sidebar);
        section = section.child(self.card_header("通用", Some("定义属性"), theme));
        if rows.is_empty() {
            let note = if needle.is_empty() {
                "（无属性）"
            } else {
                "（无匹配属性）"
            };
            section = section.child(div().text_xs().text_color(muted).child(note));
            return section;
        }
        let cols = if rows.len() > 8 { 3 } else { 2 };
        // 网格容器：`Form::horizontal()` + `columns`（gpui-component 0.9.0）——
        // label 左 / 控件右横排（对齐 DBeaver General 布局），列网格均分。
        // label_width 走 ui.rs 常量 × 主题字号（Form 要 Pixels，契约禁裸 px），紧凑档 Small。
        section =
            section.child(
                Form::horizontal()
                    .with_size(Size::Small)
                    .label_width(theme.font_size * ui::PROPERTY_GENERAL_LABEL_WIDTH)
                    .columns(cols)
                    .children(rows.iter().map(|row| {
                        self.render_property_cell(entity.clone(), row, theme, window, cx)
                    })),
            );
        section
    }

    /// ③ 子项类别区（DBeaver 表属性页形态：**子 tab + 内容**）。
    ///
    /// 多类别 → 左侧竖排子标签栏（名称 + 行数徽标）+ 右侧内容区，
    /// 点击左栏切换右侧表格；单类别 / 无类别 → 退化为直接堆叠（不显示左栏）。
    /// 整体卡片化（与洞察 / General 同语言）。
    pub(crate) fn render_category_list(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        filter: &str,
        theme: &Theme,
    ) -> Div {
        let muted = theme.colors.muted_foreground;
        let needle = filter.to_lowercase();
        // filter 过滤：sections 里任一 row 的 label/value 包含 needle 才保留该 section。
        let filtered: Vec<&PropertySection> = props
            .sections
            .iter()
            .filter(|s| {
                needle.is_empty()
                    || s.table
                        .rows
                        .iter()
                        .any(|row| row.iter().any(|cell| cell.to_lowercase().contains(&needle)))
            })
            .collect();
        let mut section = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(theme.colors.border)
            .rounded_md()
            .bg(theme.colors.sidebar);
        if filtered.is_empty() {
            let note = if needle.is_empty() {
                "（无）"
            } else {
                "（无匹配子项）"
            };
            section = section.child(self.card_header("子项", Some(note), theme));
            return section;
        }
        section = section.child(self.card_header("子项", None, theme));
        if filtered.len() == 1 {
            section = section.child(self.render_category_section(filtered[0], theme));
            return section;
        }

        // 多类别：左栏子标签 + 右栏内容（当前选中越界保护）。
        let active = self.active_category.get().min(filtered.len() - 1);
        let mut body = div().h_flex().items_stretch().w_full().gap_2();
        // 左栏：竖排子标签（固定宽，类别切换）。
        let mut rail = div()
            .v_flex()
            .w(rems(ui::PROPERTY_CAT_RAIL_WIDTH))
            .flex_none()
            .border_1()
            .border_color(theme.colors.border)
            .rounded_md()
            .overflow_y_scrollbar();
        for (ix, s) in filtered.iter().enumerate() {
            rail = rail.child(self.render_category_tab(entity.clone(), s, ix, ix == active, theme));
        }
        // 右栏：选中类别内容。
        let content = div()
            .flex_1()
            .min_w_0()
            .child(self.render_category_section(filtered[active], theme));
        body = body.child(rail).child(content);
        section = section.child(body);
        section
    }

    /// 左栏单个子标签（「子 tab + 内容」的 tab 本体）：
    /// 选中态 = `Button` toggled 底色 + 左侧 2px 激活条（与导航树行同原语）；
    /// 尾注计数（sections 只含可用类别，未实现类别隐藏式不产出，2026-10-24 拍板）。
    pub(crate) fn render_category_tab(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        s: &PropertySection,
        ix: usize,
        selected: bool,
        theme: &Theme,
    ) -> Div {
        let muted = theme.colors.muted_foreground;
        let active_border = theme.colors.sidebar_accent;
        let count = s.table.rows.len();

        // 按钮本体：id 用类别名（业务键，provider 增删类别时焦点 / hover 态不串位）。
        let btn = Button::new(format!("property-cat-{}", s.label))
            .ghost()
            .toggled(selected)
            .small()
            .flex_1()
            .min_w_0()
            .label(s.label.clone());
        let tab = div()
            .relative()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(rems(ui::PROPERTY_CAT_ROW_HEIGHT))
            .px_1()
            .when(selected, |el| el.child(tree::active_bar(active_border)))
            .child(btn.on_click(move |_, _, app| {
                entity.update(app, |this, cx| {
                    this.active_category.set(ix);
                    cx.notify();
                });
            }))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(count.to_string()),
            );
        tab
    }

    /// 单个子项类别：标题 + 表头 + 行（sections 只含可用类别，未实现类别隐藏式
    /// 不产出，2026-10-24 拍板）。
    ///
    /// DDL / 源码（`PageId::Ddl` / `PageId::Source`）是长文本段落：走代码块渲染
    /// （等宽 + 复制 + 长行折行不截断），其余类别走通用表格（列 / 索引 / 约束）。
    /// 区分的是**分区展示形态**而非对象类型，`sections` 遍历 / 零分支原则不变。
    pub(crate) fn render_category_section(&self, s: &PropertySection, theme: &Theme) -> Div {
        if matches!(s.id, PageId::Ddl | PageId::Source) {
            return self.render_text_section(s, theme);
        }
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        let mut section = div()
            .v_flex()
            .w_full()
            .gap_1()
            .p_2()
            .rounded_md()
            .bg(theme.colors.background);
        section = section.child(
            div()
                .pt_1()
                .pb_1()
                .w_full()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(s.label.clone()),
        );
        if s.table.headers.is_empty() {
            section = section.child(div().text_xs().text_color(muted).child("（无）"));
            return section;
        }
        section = section.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .pb_1()
                .border_b_1()
                .border_color(border)
                .text_xs()
                .text_color(muted)
                .children(
                    s.table
                        .headers
                        .iter()
                        .map(|h| div().flex_1().min_w_0().text_ellipsis().child(h.clone())),
                ),
        );
        for data_row in &s.table.rows {
            section = section.child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .text_xs()
                    .text_color(fg)
                    .children(
                        data_row
                            .iter()
                            .map(|c| div().flex_1().min_w_0().text_ellipsis().child(c.clone())),
                    ),
            );
        }
        section
    }

    /// 长文本段落（DDL / 源码）：等宽代码块 + 复制按钮，长行折行不截断——
    /// 对齐 DBeaver 的 DDL 只读代码视图（2026-10-24；DDL 是用户要读要复制的，
    /// 挂 `text_ellipsis` 会截断 SQL，故此处不截断）。
    pub(crate) fn render_text_section(&self, s: &PropertySection, theme: &Theme) -> Div {
        let fg = theme.colors.foreground;
        let border = theme.colors.border;
        let mono = theme.mono_font_family.clone();

        // 复制值 = 原始行拼接（`text_table` 已按行切好；不含「（空）」占位）。
        let copy_value = s
            .table
            .rows
            .iter()
            .map(|r| r.first().map(String::as_str).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");

        let mut section = div()
            .v_flex()
            .w_full()
            .gap_1()
            .p_2()
            .rounded_md()
            .bg(theme.colors.background)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(
                        div()
                            .pt_1()
                            .pb_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child(s.label.clone()),
                    )
                    .child(
                        Clipboard::new(format!("property-copy-{}", s.label))
                            .value(copy_value)
                            .tooltip("复制")
                            .accessibility_label("复制属性文本"),
                    ),
            );

        let mut block = div()
            .v_flex()
            .w_full()
            .min_w_0()
            .gap_0p5()
            .p_2()
            .rounded_sm()
            .border_1()
            .border_color(border)
            .font_family(mono)
            .text_xs()
            .text_color(fg);
        for line in s
            .table
            .rows
            .iter()
            .map(|r| r.first().cloned().unwrap_or_default())
        {
            // 不挂 `text_ellipsis`：长行折行完整显示（DDL 语义是可读可复制）。
            block = block.child(div().w_full().min_w_0().child(line));
        }
        section = section.child(block);
        section
    }
}
