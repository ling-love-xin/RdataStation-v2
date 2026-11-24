//! 面板头与工具条：搜索、筛选、排序与命令按钮。

use shared::i18n::tr;
use super::*;

impl ResourcesPanel {
    pub(super) fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let (_, muted, foreground) = {
            let colors = cx.theme().colors;
            (colors.border, colors.muted_foreground, colors.foreground)
        };
        let read_only = self.snapshot.read_only;
        let host = self.host.clone();

        // 统一容器（`PanelToolbar`）：36 高 / 左缘 12 / 1px 下边界 / 左槽标题 + 右槽动作。
        PanelToolbar::new("archive-header")
            // 左槽：标题 = 前缀图标 + 文字（原型 §2.1）。图标形状与活动栏该面板的图标一致，
            // 一眼就对得上"这里是资产库"。
            .left(
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
                            .child(tr("资产库")),
                    ),
            )
            // 右槽：`＋ ▾` 主操作 → `⋯` 更多
            .right({
                let host = host.clone();
                // 原型 §2.1：`＋ ▾` 主操作，两项——**存档的本体必须来自某处**，
                // 所以菜单里是两个来源，而不是"新建存档"。
                Button::new("archive-add")
                    .ghost()
                    .label("＋ ▾")
                    .disabled(read_only)
                    .dropdown_menu({
                        move |menu, _window, _cx| {
                            let drafts_host = host.clone();
                            let file_host = host.clone();
                            menu.item(PopupMenuItem::new(tr("从草稿箱归档…")).on_click(
                                move |_, window, cx| {
                                    drafts_host.request_archive_from_drafts(window, cx)
                                },
                            ))
                            .item(
                                PopupMenuItem::new(tr("从本地文件归档…")).on_click(
                                    move |_, window, cx| {
                                        file_host.request_archive_from_file(window, cx)
                                    },
                                ),
                            )
                        }
                    })
            })
            .right({
                let host = host.clone();
                // 原型 §2.1：`⋯` 更多四项。写操作（重建索引）不单独置灰：
                // 菜单里三项与只读无关，置灰整个 `⋯` 反而把刷新也锁上了。
                Button::new("archive-more")
                    .ghost()
                    .label("⋯")
                    .debug_selector(|| "archive-more".to_string())
                    .dropdown_menu({
                        move |menu, _window, _cx| {
                            let mut menu = menu;
                            for action in HeaderMenuAction::ALL {
                                let target = host.clone();
                                menu = menu.item(PopupMenuItem::new(action.label()).on_click(
                                    move |_, window, cx| {
                                        dispatch_header_action(&target, action, window, cx)
                                    },
                                ));
                            }
                            menu
                        }
                    })
            })
            .into_any_element()
    }
    /// 工具栏（高 2rem）：搜索框 + 筛选 + 排序（原型 §2.2）。
    ///
    /// 空库时也照常渲染：工具栏是面板版式的一部分，不能随"有没有存档"上下跳。
    /// 条件在渲染时快照一份给菜单（菜单开启时读一次，之后用 `checked` 表达状态）。
    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
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
            // 和搜索框同高：同一行里 24 与 26 的边框对不齐
            .h(rems(ui::CONTROL_HEIGHT_SM))
            .label(format!("{} ▾", instance.label()))
            .debug_selector(|| "archive-instance".to_string())
            .dropdown_menu({
                let panel = panel.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label(tr("实例")));
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
            // 和搜索框同高：同一行里 24 与 26 的边框对不齐
            .h(rems(ui::CONTROL_HEIGHT_SM))
            .label(filter_label)
            .dropdown_menu({
                let panel = panel.clone();
                let dictionary = self.snapshot.tags.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label(tr("种类")));
                    // 只摆两档（`MENU`）：引用档没有新写入点，摆它只会让人以为还能归档引用；
                    // 历史引用行仍照常显示尾部徽标，只是不能拿它当筛选项。
                    for kind in ArchiveKind::MENU {
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
                        menu = menu.separator().item(PopupMenuItem::label(tr("标签")));
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
                        PopupMenuItem::new(tr("只看需处理"))
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
            // 和搜索框同高：同一行里 24 与 26 的边框对不齐
            .h(rems(ui::CONTROL_HEIGHT_SM))
            .label(sort_label)
            .dropdown_menu({
                let panel = panel.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label(tr("排序")));
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

        // 统一容器（`PanelToolbar`），高度取 `TOOLBAR_HEIGHT`（32）——这是面板内
        // **第二行**（头行 36 已在 `render_header`），不跟着头行走 36。
        PanelToolbar::new("archive-toolbar")
            .height(ui::TOOLBAR_HEIGHT)
            // 左槽：搜索框占满除三个按钮之外的宽度（`min_w_0`：窄面板下允许被压缩）。
            .left(
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
            // 右槽：实例维度 → 筛选 → 排序
            .right(instance_button)
            .right(filter_button)
            .right(sort_button)
            .into_any_element()
    }
}
