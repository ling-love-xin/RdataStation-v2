//! 面板头与工具条：搜索、筛选、排序与命令按钮。

use super::*;

impl ResourcesPanel {
    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let (border, muted, foreground) = {
            let colors = cx.theme().colors;
            (colors.border, colors.muted_foreground, colors.foreground)
        };
        let read_only = self.snapshot.read_only;
        let host = self.host.clone();

        div()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .flex_none()
            .h_flex()
            .justify_between()
            .gap_2()
            .px_2()
            // 固定描边：不随字号缩放（`ui::HAIRLINE` = 1px，与 editor / settings 同口径）。
            .border_b(ui::HAIRLINE)
            .border_color(border)
            .child(
                // 标题 = 前缀图标 + 文字（原型 §2.1）：图标形状与活动栏该面板的图标一致，
                // 一眼就对得上"这里是资产库"。
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .child(
                        Icon::default()
                            .path("icons/chart-column.svg")
                            .flex_none()
                            .size_3p5()
                            .text_color(muted),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(foreground)
                            .child("资产库"),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        // 原型 §2.1：`＋ ▾` 主操作，两项——**存档的本体必须来自某处**，
                        // 所以菜单里是两个来源，而不是"新建存档"。
                        Button::new("archive-add")
                            .ghost()
                            .label("＋ ▾")
                            .disabled(read_only)
                            .dropdown_menu({
                                let host = host.clone();
                                move |menu, _window, _cx| {
                                    let drafts_host = host.clone();
                                    let file_host = host.clone();
                                    menu.item(PopupMenuItem::new("从草稿箱归档…").on_click(
                                        move |_, window, cx| {
                                            drafts_host.request_archive_from_drafts(window, cx)
                                        },
                                    ))
                                    .item(
                                        PopupMenuItem::new("从本地文件归档…").on_click(
                                            move |_, window, cx| {
                                                file_host.request_archive_from_file(window, cx)
                                            },
                                        ),
                                    )
                                }
                            }),
                    )
                    .child(
                        // 原型 §2.1：`⋯` 更多四项。写操作（重建索引）不单独置灰：
                        // 菜单里三项与只读无关，置灰整个 `⋯` 反而把刷新也锁上了。
                        Button::new("archive-more")
                            .ghost()
                            .label("⋯")
                            .debug_selector(|| "archive-more".to_string())
                            .dropdown_menu({
                                let host = host.clone();
                                move |menu, _window, _cx| {
                                    let mut menu = menu;
                                    for action in HeaderMenuAction::ALL {
                                        let target = host.clone();
                                        menu =
                                            menu.item(PopupMenuItem::new(action.label()).on_click(
                                                move |_, window, cx| {
                                                    dispatch_header_action(
                                                        &target, action, window, cx,
                                                    )
                                                },
                                            ));
                                    }
                                    menu
                                }
                            }),
                    ),
            )
    }
    /// 工具栏（高 2rem）：搜索框 + 筛选 + 排序（原型 §2.2）。
    ///
    /// 空库时也照常渲染：工具栏是面板版式的一部分，不能随"有没有存档"上下跳。
    /// 条件在渲染时快照一份给菜单（菜单开启时读一次，之后用 `checked` 表达状态）。
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().colors.border;
        let panel = cx.entity();
        let kinds = self.filter.kinds.clone();
        let tags = self.filter.tags.clone();
        let only_issues = self.filter.only_issues;
        // 菜单里设了几个条件就标在按钮上：搜索词不计数（它在输入框里看得见）。
        let filter_label = match self.filter.menu_dims() {
            0 => "筛选 ▾".to_string(),
            dims => format!("筛选 {dims} ▾"),
        };
        let (field, order) = (self.sort_field, self.sort_order);
        let sort_label = format!("{} {} ▾", field.short_label(), order.arrow());

        // 实例维度（§13）：全部 / 项目 / 全局。独立于筛选/排序——它是**行源**维度，
        // 全局行没有项目标签 / 分组概念，不能混进 `ResourcesFilter` 的维度里。
        let instance = self.instance;
        let instance_button = Button::new("archive-instance")
            .ghost()
            .small()
            .label(format!("{} ▾", instance.label()))
            .debug_selector(|| "archive-instance".to_string())
            .dropdown_menu({
                let panel = panel.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label("实例"));
                    for candidate in [
                        InstanceFilter::All,
                        InstanceFilter::Project,
                        InstanceFilter::Global,
                    ] {
                        let target = panel.clone();
                        let is_current = candidate == instance;
                        menu = menu.item(
                            PopupMenuItem::new(candidate.label())
                                .checked(is_current)
                                .on_click(move |_, _, app| {
                                    target
                                        .update(app, |panel, cx| panel.set_instance(candidate, cx));
                                }),
                        );
                    }
                    menu
                }
            });

        let filter_button = Button::new("archive-filter")
            .ghost()
            .small()
            .label(filter_label)
            .dropdown_menu({
                let panel = panel.clone();
                let dictionary = self.snapshot.tags.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label("种类"));
                    for kind in ArchiveKind::ALL {
                        let target = panel.clone();
                        let is_checked = kinds.contains(&kind);
                        menu = menu.item(
                            PopupMenuItem::new(kind.label())
                                .checked(is_checked)
                                .on_click(move |_, _, app| {
                                    target.update(app, |panel, cx| panel.toggle_kind(kind, cx));
                                }),
                        );
                    }
                    // 标签组：一个标签都没有时不摆（空菜单节只会占地方）。
                    if !dictionary.is_empty() {
                        menu = menu.separator().item(PopupMenuItem::label("标签"));
                        for option in &dictionary {
                            let target = panel.clone();
                            let is_checked = tags.iter().any(|id| id == &option.id);
                            let label = format!("{}（{}）", option.name, option.count);
                            let tag_id = option.id.clone();
                            menu =
                                menu.item(PopupMenuItem::new(label).checked(is_checked).on_click(
                                    move |_, _, app| {
                                        let tag_id = tag_id.clone();
                                        target
                                            .update(app, |panel, cx| panel.toggle_tag(&tag_id, cx));
                                    },
                                ));
                        }
                    }
                    menu.separator().item(
                        PopupMenuItem::new("只看需处理")
                            .checked(only_issues)
                            .on_click({
                                let target = panel.clone();
                                move |_, _, app| {
                                    target.update(app, |panel, cx| panel.toggle_only_issues(cx));
                                }
                            }),
                    )
                }
            });

        let sort_button = Button::new("archive-sort")
            .ghost()
            .small()
            .label(sort_label)
            .dropdown_menu({
                let panel = panel.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label("排序"));
                    for candidate in SortField::ALL {
                        let target = panel.clone();
                        let is_current = candidate == field;
                        // 当前字段带上方向箭头：不然用户得回忆上次点的是哪个方向。
                        let text = if is_current {
                            format!("{} {}", candidate.label(), order.arrow())
                        } else {
                            candidate.label().to_string()
                        };
                        menu = menu.item(PopupMenuItem::new(text).checked(is_current).on_click(
                            move |_, _, app| {
                                target.update(app, |panel, cx| panel.choose_sort(candidate, cx));
                            },
                        ));
                    }
                    menu
                }
            });

        div()
            .flex_none()
            .h(rems(ui::TOOLBAR_HEIGHT))
            .h_flex()
            .gap_1()
            .px_1()
            .border_b(ui::HAIRLINE)
            .border_color(border)
            .child(
                // 搜索框占满除两个按钮之外的宽度（`min_w_0`：窄面板下允许被压缩）。
                div()
                    .flex_1()
                    .min_w_0()
                    .when_some(self.search_input.clone(), |row, input| {
                        row.child(
                            Input::new(&input)
                                .h(rems(ui::CONTROL_HEIGHT_SM))
                                .cleanable(true),
                        )
                    }),
            )
            .child(instance_button)
            .child(filter_button)
            .child(sort_button)
    }
}
