//! 选择器与菜单：列表渲染、卡片、卡片 `⋯`/右键菜单、标题栏项目菜单。

use super::*;

// ==================== 加载 / 刷新 ====================

/// 刷新选择器列表（切换 / 关闭后调用，会重绘）。
pub fn refresh_picker(host: &ProjectUiHost, cx: &mut App) {
    load_picker(host);
    host.notify(cx);
}

/// 加载列表（不 notify；供 render 首帧调用，避免 render 内 notify 循环）。
pub fn load_picker(host: &ProjectUiHost) {
    let (tab, sort) = {
        let ui = host.state.borrow();
        (ui.picker.tab, ui.picker.sort)
    };
    let result = match tab {
        PickerTab::Recent => project_service::list_recent(12),
        PickerTab::All => project_service::list_all(),
        PickerTab::Removed => project_service::list_removed(),
    };

    let mut ui = host.state.borrow_mut();
    match result {
        Ok(mut items) => {
            sort_items(&mut items, sort);
            ui.picker.items = items;
            ui.picker.error = None;
        }
        Err(e) => {
            ui.picker.items.clear();
            ui.picker.error = Some(e);
        }
    }
    ui.picker.loaded = true;
}

pub(super) fn sort_items(items: &mut [ProjectSummary], sort: ProjectSort) {
    items.sort_by(|a, b| {
        b.is_pinned.cmp(&a.is_pinned).then_with(|| match sort {
            ProjectSort::LastOpened => b.last_opened_at.cmp(&a.last_opened_at),
            ProjectSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            ProjectSort::Created => b.created_at.cmp(&a.created_at),
        })
    });
}

// ==================== 动作 ====================

/// 切换 Tab 并刷新。
pub fn set_tab(host: &ProjectUiHost, tab: PickerTab, cx: &mut App) {
    tracing::debug!(tab = %tab.key(), "项目选择器切换 Tab");
    host.state.borrow_mut().picker.tab = tab;
    refresh_picker(host, cx);
}

/// 直接设置排序（下拉菜单单选；交宿主持久化）并刷新。
pub fn set_sort(host: &ProjectUiHost, sort: ProjectSort, cx: &mut App) {
    host.state.borrow_mut().picker.sort = sort;
    (host.save_sort)(sort, cx);
    refresh_picker(host, cx);
}

/// 直接设置状态筛选（下拉菜单单选；不持久化：临时视图状态，不影响查询）。
pub fn set_status_filter(host: &ProjectUiHost, filter: StatusFilter, cx: &mut App) {
    host.state.borrow_mut().picker.status_filter = filter;
    host.notify(cx);
}

// ==================== 渲染 ====================

/// 项目选择器（覆盖中央区）。
pub fn render_picker(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) -> Div {
    let theme = cx.theme();
    let ui = host.state.borrow();
    let picker = &ui.picker;

    // 搜索与状态筛选在渲染时过滤（避免每次输入都重查数据库）。
    let needle = inputs.search.read(cx).value().to_string();
    let items = visible_items(&picker.items, &needle, picker.status_filter);
    let count = items.len();
    // facet 生效时在计数旁说明——否则用户会以为「搜不到东西」是数据没了
    let facet_hint = parse_search(&needle).has_facet();

    // ---- 顶部：标题 + Tab ----
    // 语义 `TabBar`（分段控件，而非三个 Button 拼装）：选中态 / hover / 键盘导航由组件负责。
    let tabs = TabBar::new("picker-tabs")
        .segmented()
        .selected_index(match picker.tab {
            PickerTab::Recent => 0,
            PickerTab::All => 1,
            PickerTab::Removed => 2,
        })
        .children(
            [PickerTab::Recent, PickerTab::All, PickerTab::Removed]
                .into_iter()
                .map(|tab| {
                    let host_t = host.clone();
                    // tab_stop(true)：TabBar 的 Tab 默认不进 Tab 焦点序（Button 默认 true），
                    // 不补会丢失键盘可达（C4 契约：Tab 停到控件 Enter 激活）。
                    Tab::new()
                        .label(tab.label())
                        .tab_stop(true)
                        .on_click(move |_, _, app| set_tab(&host_t, tab, app))
                }),
        );

    // ---- 搜索 + 状态筛选 + 排序（筛选/排序为语义 DropdownMenu，单选直接设值） ----
    let status_current = picker.status_filter;
    let host_status = host.clone();
    let status_menu = move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
        let mut menu = menu;
        for spec in status_filter_menu_entries(status_current) {
            let host = ProjectUiHost::clone(&host_status);
            let filter = spec.filter;
            menu = menu.item(
                PopupMenuItem::new(filter.label())
                    .checked(spec.checked)
                    .disabled(spec.checked)
                    .on_click(move |_, _, app| set_status_filter(&host, filter, app)),
            );
        }
        menu
    };
    let sort_current = picker.sort;
    let host_sort = host.clone();
    let sort_menu = move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
        let mut menu = menu;
        for spec in sort_menu_entries(sort_current) {
            let host = ProjectUiHost::clone(&host_sort);
            let sort = spec.sort;
            menu = menu.item(
                PopupMenuItem::new(sort.label())
                    .checked(spec.checked)
                    .disabled(spec.checked)
                    .on_click(move |_, _, app| set_sort(&host, sort, app)),
            );
        }
        menu
    };
    let search_row = div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().child(Input::new(&inputs.search)))
        .child(
            Button::new("picker-status")
                .ghost()
                .small()
                .label(format!("状态：{}", picker.status_filter.label()))
                .dropdown_menu(status_menu),
        )
        .child(
            Button::new("picker-sort")
                .ghost()
                .small()
                .label(format!("排序：{}", picker.sort.label()))
                .dropdown_menu(sort_menu),
        );

    // ---- 列表 ----
    let mut list = div()
        .v_flex()
        .gap_2()
        .flex_1()
        .min_h_0()
        .overflow_y_scrollbar();
    if let Some(err) = &picker.error {
        list = list.child(error_line(err, theme));
    }
    if picker.items.is_empty() {
        list = list.child(empty_state(picker.tab, picker.loaded));
    } else if items.is_empty() {
        list = list.child(
            div()
                .v_flex()
                .items_center()
                .py_8()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("无匹配项目"),
        );
    } else {
        for item in &items {
            list = list.child(project_card(host, inputs, item, picker.tab, theme));
        }
    }

    // ---- 右侧操作栏 ----
    let host_new = host.clone();
    let inputs_new = inputs.clone();
    let host_open = host.clone();
    let inputs_open = inputs.clone();
    let host_sample = host.clone();

    let rail = div()
        .v_flex()
        .gap_2()
        // 侧栏宽引用 workbench_shell::ui 常量（与工作台左 Dock 同档，随界面缩放）
        .w(rems(ui::LEFT_DOCK_WIDTH))
        .flex_none()
        .p_3()
        .bg(theme.colors.sidebar)
        .border_l_1()
        .border_color(theme.colors.border)
        .child(
            Button::new("picker-new")
                .primary()
                .icon(IconName::Plus)
                .label("新建项目")
                // 走与标题栏菜单同一入口：有当前项目（项目中心）时先拦截未保存草稿。
                .on_click(move |_, window, app| {
                    request_create_project(&host_new, &inputs_new, window, app);
                }),
        )
        .child(
            Button::new("picker-open")
                .secondary()
                .icon(IconName::FolderOpen)
                .label("打开文件夹…")
                .on_click(move |_, window, app| {
                    request_open_folder(&host_open, &inputs_open, window, app);
                }),
        )
        .child(
            Button::new("picker-sample")
                .secondary()
                .icon(IconName::Copy)
                .label("从示例项目开始")
                .on_click(move |_, window, app| create_sample(&host_sample, window, app)),
        )
        .child(
            div()
                .mt_2()
                .v_flex()
                .gap_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(div().font_weight(FontWeight::MEDIUM).child("快速开始"))
                .child("· 新建空项目")
                .child("· 从示例项目开始")
                .child("· 打开现有目录（.RSmeta）"),
        );

    if let Some(notice) = &ui.notice {
        // 只读提示等
        list = list.child(error_line(notice, theme));
    }

    div()
        .h_flex()
        .items_stretch()
        .size_full()
        .bg(theme.colors.background)
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_3()
                .p_4()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("项目"),
                        )
                        .child(tabs)
                        .child(
                            div()
                                .ml_auto()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(if facet_hint {
                                    format!("{count} 个项目 · 已按 facet 收窄")
                                } else {
                                    format!("{count} 个项目")
                                }),
                        ),
                )
                .child(search_row)
                .child(list),
        )
        .child(rail)
}

/// 卡片对应的是否就是当前打开的项目（项目中心「当前」标记与切换 no-op 的判据）。
pub fn is_current_project(root: Option<&Path>, item: &ProjectSummary) -> bool {
    root.is_some_and(|root| root == item.path.as_path())
}

/// 项目卡片：一个可见主操作（打开）+ 次要命令收进 `DropdownMenu`；同一批命令另有
/// **右键菜单**（指南：作用于指针下对象的命令应当同时提供 `ContextMenu`）。
///
/// 设计指南不允许列表行铺一排 hover 才可见的图标命令：主操作常显，其余命令由
/// 带可见触发按钮的菜单承载（方向键、dismiss、焦点恢复由组件负责）。
pub(super) fn project_card(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    tab: PickerTab,
    theme: &gpui_kit::component::Theme,
) -> impl IntoElement {
    let host_open = host.clone();
    let path_open = item.path.clone();
    // 项目中心里当前项目要能被认出来（点它不该重开，否则白丢一次锁）。
    let is_current = is_current_project(host.root().as_deref(), item);

    let status_color = match item.status.as_str() {
        "active" => theme.colors.success,
        "archived" => theme.colors.muted_foreground,
        "syncing" => theme.colors.info,
        "offline" => theme.colors.warning,
        _ => theme.colors.muted_foreground,
    };

    let open = Button::new(ElementId::Name(SharedString::from(format!(
        "open-{}",
        item.id
    ))))
    .small()
    .label("打开")
    // 已有项目打开时点卡片＝切换：先释放当前项目再打开目标（不覆盖会话与锁）。
    .on_click(move |_, window, app| request_switch(&host_open, &path_open, window, app));

    let more = more_menu(host, inputs, item, tab);

    let mut card = div()
        // 右键菜单的开关状态按 id 记：id 要稳（按项目 id，不按下标）
        .id(ElementId::Name(SharedString::from(format!(
            "project-card-{}",
            item.id
        ))))
        .v_flex()
        .gap_1()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(if item.path_exists {
            theme.colors.border
        } else {
            theme.colors.warning
        })
        .bg(theme.colors.background)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.colors.foreground)
                        .child(item.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_label(&item.status).to_string()),
                )
                .when(item.is_pinned, |d| {
                    d.child(Icon::new(IconName::Star).text_color(theme.colors.primary))
                })
                // 项目中心里标记「当前」：告诉用户这一条就是正在用的项目，
                // 点它不会重开（`request_switch` 对同一路径直接返回）。
                .when(is_current, |d| d.child(Tag::primary().child("当前")))
                .child(
                    div().ml_auto().child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(open)
                            .child(more),
                    ),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(item.path.to_string_lossy().to_string()),
        )
        // R7：描述属于卡片元信息（有就显示，空 / 纯空白视作没有）
        .when_some(card_description(item), |d, description| {
            d.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .text_ellipsis()
                    .child(description),
            )
        })
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(meta_line(item)),
        );

    if !item.path_exists {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.warning)
                .child("⚠ 路径已失效，可能已移动或删除"),
        );
    } else if !item.missing_drivers.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.warning)
                .child(format!("⚠ 缺失驱动：{}", item.missing_drivers.join("、"))),
        );
    }
    if item.lock.is_some() {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.info)
                .child("● 已在另一实例打开"),
        );
    }
    // 右键菜单与 `⋯` 下拉同源（`card_menu_entries`）：右键一张卡片也能打开 / 固定 / 删除
    card.context_menu({
        let host = ProjectUiHost::clone(host);
        let inputs = inputs.clone();
        let item = item.clone();
        move |menu, _window, _cx| card_menu(menu, &host, &inputs, &item, tab)
    })
}

/// 卡片命令菜单的规格：`⋯` 下拉与**右键菜单**共用一份（同一对象上的命令不该有两套口径）。
///
/// 「打开」也列进来：卡片上它是可见主操作，右键菜单里则必须有——右键一张卡片最想做的事
/// 就是打开它，而右键菜单没有别的地方能提供这个命令。
pub fn card_menu_entries(
    item: &ProjectSummary,
    tab: PickerTab,
    read_only: bool,
) -> Vec<ProjectMenuEntry> {
    let mut entries = vec![ProjectMenuEntry {
        id: "open",
        label: "打开",
        icon: IconName::FolderOpen,
        enabled: true,
        separator_before: false,
    }];
    entries.push(ProjectMenuEntry {
        id: "pin",
        label: if item.is_pinned {
            "取消固定"
        } else {
            "固定"
        },
        icon: if item.is_pinned {
            IconName::StarOff
        } else {
            IconName::Star
        },
        enabled: !read_only,
        separator_before: true,
    });

    // 已移除视图：只有「恢复」一条路（磁盘不在名册里，没什么可管理）
    if tab == PickerTab::Removed {
        entries.push(ProjectMenuEntry {
            id: "restore",
            label: "恢复",
            icon: IconName::RotateCw,
            enabled: !read_only,
            separator_before: true,
        });
        return entries;
    }

    if item.path_exists {
        entries.push(ProjectMenuEntry {
            id: "reveal",
            label: "在资源管理器中显示",
            icon: IconName::ExternalLink,
            enabled: true,
            separator_before: true,
        });
        // 路径正常：软删（名册上留痕，可在「已移除」找回）
        entries.push(ProjectMenuEntry {
            id: "remove",
            label: "移出列表",
            icon: IconName::Close,
            enabled: !read_only,
            separator_before: false,
        });
        entries.push(ProjectMenuEntry {
            id: "delete",
            label: "删除数据…",
            icon: IconName::Delete,
            enabled: !read_only,
            separator_before: true,
        });
    } else {
        entries.push(ProjectMenuEntry {
            id: "relocate",
            label: "重新定位…",
            icon: IconName::RotateCw,
            enabled: !read_only,
            separator_before: true,
        });
        // 路径已失效：没什么可重定位的目标，移出 = 清掉名册记录（`forget`）
        entries.push(ProjectMenuEntry {
            id: "remove",
            label: "移出列表",
            icon: IconName::Close,
            enabled: !read_only,
            separator_before: false,
        });
    }
    entries
}

/// 卡片命令菜单内容（`⋯` 下拉与右键菜单同一个构造器）。
pub(super) fn card_menu(
    menu: PopupMenu,
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    tab: PickerTab,
) -> PopupMenu {
    let read_only = host.state.borrow().read_only;
    let mut menu = menu;
    for (ix, entry) in card_menu_entries(item, tab, read_only)
        .into_iter()
        .enumerate()
    {
        if ix > 0 && entry.separator_before {
            menu = menu.separator();
        }
        let menu_item = PopupMenuItem::new(entry.label)
            .icon(entry.icon)
            .disabled(!entry.enabled);
        menu = menu.item(attach_card_menu_handler(
            menu_item, entry.id, host, inputs, item,
        ));
    }
    menu
}

/// 按规格 id 给卡片菜单项挂事件（同一个 id 在两种「移出」语义下挂不同函数）。
pub(super) fn attach_card_menu_handler(
    menu_item: PopupMenuItem,
    id: &'static str,
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
) -> PopupMenuItem {
    let item_id = item.id.clone();
    let path = item.path.clone();
    match id {
        "open" => {
            let host = ProjectUiHost::clone(host);
            // 与卡片主按钮同路：已有当前项目时是「切换」，不是覆盖会话。
            menu_item.on_click(move |_, window, app| request_switch(&host, &path, window, app))
        }
        "pin" => {
            let host = ProjectUiHost::clone(host);
            let item = item.clone();
            menu_item.on_click(move |_, _, app| toggle_pin(&host, &item, app))
        }
        "restore" => {
            let host = ProjectUiHost::clone(host);
            menu_item.on_click(move |_, _, app| restore(&host, &item_id, app))
        }
        "reveal" => menu_item.on_click(move |_, _, _| reveal_in_explorer(&path)),
        // 路径失效时「移出列表」是 forget（清名册记录），否则是 soft_remove（可找回）
        "remove" if !item.path_exists => {
            let host = ProjectUiHost::clone(host);
            menu_item.on_click(move |_, _, app| forget(&host, &item_id, app))
        }
        "remove" => {
            let host = ProjectUiHost::clone(host);
            menu_item.on_click(move |_, _, app| soft_remove(&host, &item_id, app))
        }
        "delete" => {
            let host = ProjectUiHost::clone(host);
            let inputs = inputs.clone();
            let item = item.clone();
            menu_item.on_click(move |_, window, app| {
                open_delete_dialog(&host, &inputs, &item, window, app)
            })
        }
        "relocate" => {
            let host = ProjectUiHost::clone(host);
            let inputs = inputs.clone();
            let item = item.clone();
            menu_item.on_click(move |_, window, app| {
                open_relocate_dialog(&host, &inputs, &item, window, app)
            })
        }
        _ => menu_item,
    }
}

/// 卡片「更多」菜单：主操作之外的次要命令。
///
/// 破坏性命令用分隔线与普通命令分开，名称带「…」表示会再开对话框；内容与右键菜单同源
/// （[`card_menu_entries`]），不各写一套。
pub(super) fn more_menu(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    tab: PickerTab,
) -> impl IntoElement {
    let item = item.clone();
    let inputs = inputs.clone();
    let host = ProjectUiHost::clone(host);
    let menu_id = SharedString::from(format!("more-{}", item.id));

    Button::new(ElementId::Name(menu_id))
        .ghost()
        .small()
        .icon(IconName::Ellipsis)
        .dropdown_menu(move |menu, _window, _cx| card_menu(menu, &host, &inputs, &item, tab))
}

/// 卡片里的描述行（R7 元信息包含描述）：空串 / 纯空白视作「没写描述」，不占位。
pub fn card_description(item: &ProjectSummary) -> Option<String> {
    item.description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
}

/// 名册状态值 → 界面文案。
///
/// 名册里存的是英文 key（`active` / `archived` / `syncing` / `offline`，见
/// `service::status_key`），界面不能直出英文。**认不出的状态原样保留**：未知值
/// 说明名册被别处写坏了，宁可显示原文也不要悄悄吞成「活跃」。
pub fn status_label(status: &str) -> &str {
    match status {
        "active" => "活跃",
        "archived" => "已归档",
        "syncing" => "同步中",
        "offline" => "离线",
        other => other,
    }
}

pub(super) fn meta_line(item: &ProjectSummary) -> String {
    let last = item
        .last_opened_at
        .clone()
        .unwrap_or_else(|| "从未打开".to_string());
    // 状态已在卡片首行（带色）显示，这里只补最后打开时间，不再重复一遍。
    format!("最后打开：{last}")
}

/// 选择器空态：语义 `Empty`（图标 + 标题 + 描述居中列），未加载时标题显示「加载中…」。
pub(super) fn empty_state(tab: PickerTab, loaded: bool) -> impl IntoElement {
    let (title, desc) = match tab {
        PickerTab::Recent => (
            "还没有最近项目",
            "新建一个项目，或打开包含 .RSmeta 的现有目录。",
        ),
        PickerTab::All => ("还没有项目", "新建一个项目开始使用。"),
        PickerTab::Removed => ("没有已移除的项目", "被移出名册的项目会出现在这里，可恢复。"),
    };
    Empty::new().header(
        EmptyHeader::new()
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .child(Icon::new(IconName::Inbox)),
            )
            .title(EmptyTitle::new().child(if loaded { title } else { "加载中…" }))
            .description(EmptyDescription::new().child(desc)),
    )
}

pub(super) fn error_line(message: &str, theme: &gpui_kit::component::Theme) -> Div {
    div()
        .p_2()
        .rounded_md()
        .text_xs()
        .text_color(theme.colors.danger)
        .child(message.to_string())
}

// ==================== 标题栏项目菜单 ====================

/// 项目菜单的一项**规格**：顺序 / 文案 / 图标 / 可用性。
///
/// 与渲染分离：`PopupMenu` 不暴露内部项，不经过窗口就无法逐项断言；把「菜单长什么样」
/// 收成纯函数（与 `editor` 的 `channel::menu_items` / `export::menu_items` 同一套路），
/// 测试直接断言规格，渲染只负责按 id 挂事件。
pub struct ProjectMenuEntry {
    /// 事件分派的稳定标识（与文案解耦：文案会变，标识不变）。
    pub id: &'static str,
    pub label: &'static str,
    pub icon: IconName,
    /// 只读模式下写命令为 `false`（置灰），读命令保持可用。
    pub enabled: bool,
    /// 与上一项之间画分隔线（首项忽略）。
    pub separator_before: bool,
}

/// 标题栏项目槽下拉菜单的规格（原型 §2.4 / §5）。
///
/// 只读模式（写锁被另一实例持有）下：**当前项目的**写命令（重命名 / 归档）置灰，并在顶部
/// 插一条说明为什么点不动；「新建项目 / 打开现有目录」作用于**别的目录**（不写当前项目），
/// 切换 / 项目设置 / 显示位置 / 关闭项目也都保持可用——只读是「不许写」，不是「不许看、
/// 不许走」，用户始终能退出去。
///
/// 「切换项目…」= 打开项目中心（不关闭当前项目）；「关闭项目」才是释放当前项目。
pub fn project_menu_entries(read_only: bool) -> Vec<ProjectMenuEntry> {
    let mut entries = Vec::with_capacity(9);
    if read_only {
        entries.push(ProjectMenuEntry {
            id: "read-only",
            label: "只读模式：写操作不可用",
            icon: IconName::Info,
            enabled: false,
            separator_before: false,
        });
    }
    // 有项目时也能创建 / 打开别的项目（此前只能先「切换项目」退回选择器）。
    entries.push(ProjectMenuEntry {
        id: "new",
        label: "新建项目…",
        icon: IconName::Plus,
        enabled: true,
        separator_before: read_only,
    });
    entries.push(ProjectMenuEntry {
        id: "open-folder",
        label: "打开现有目录…",
        icon: IconName::FolderOpen,
        enabled: true,
        separator_before: false,
    });
    // 「项目中心…」不关闭当前项目：先看列表，选中目标才切换（一次点击完成）。
    entries.push(ProjectMenuEntry {
        id: "switch",
        label: "项目中心…",
        icon: IconName::ArrowLeft,
        enabled: true,
        separator_before: true,
    });
    entries.push(ProjectMenuEntry {
        id: "settings",
        label: "项目设置…",
        icon: IconName::Settings,
        enabled: true,
        separator_before: false,
    });
    entries.push(ProjectMenuEntry {
        id: "rename",
        label: "重命名…",
        icon: IconName::FileText,
        enabled: !read_only,
        separator_before: false,
    });
    entries.push(ProjectMenuEntry {
        id: "reveal",
        label: "在资源管理器中显示",
        icon: IconName::ExternalLink,
        enabled: true,
        separator_before: false,
    });
    entries.push(ProjectMenuEntry {
        id: "archive",
        label: "归档 / 取消归档",
        icon: IconName::Inbox,
        enabled: !read_only,
        separator_before: true,
    });
    entries.push(ProjectMenuEntry {
        id: "close",
        label: "关闭项目",
        icon: IconName::Close,
        enabled: true,
        separator_before: true,
    });
    entries
}

/// 标题栏项目菜单内容（宿主用 `Button::dropdown_menu` 承载表面 / 焦点 / 关闭）。
///
/// 用语义 `PopupMenu` / `PopupMenuItem`（而非自绘弹层）：方向键导航、Escape 关闭、
/// 焦点恢复、禁用项与分隔线都由组件负责。顺序 / 文案 / 可用性来自
/// [`project_menu_entries`]，这里只负责按 id 挂事件。
pub fn build_project_menu(
    menu: PopupMenu,
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
) -> PopupMenu {
    // 菜单在宿主 render 里构造，状态在此一次性读完（逐项再读会把 `RefCell` 借用拖长）。
    let (read_only, root) = (host.state.borrow().read_only, host.root());
    let root = root.unwrap_or_default();
    let mut menu = menu;
    for (ix, entry) in project_menu_entries(read_only).into_iter().enumerate() {
        if ix > 0 && entry.separator_before {
            menu = menu.separator();
        }
        let item = PopupMenuItem::new(entry.label)
            .icon(entry.icon)
            .disabled(!entry.enabled);
        menu = menu.item(attach_project_menu_handler(
            item, entry.id, host, inputs, &root,
        ));
    }
    menu
}

/// 按规格 id 给菜单项挂事件；规格里没有的 id（例如只读提示）保持不可点。
pub(super) fn attach_project_menu_handler(
    item: PopupMenuItem,
    id: &'static str,
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    root: &std::path::Path,
) -> PopupMenuItem {
    match id {
        // 有项目时也能创建 / 打开别的项目：走与连接对话框项目栏相同的入口，
        // 脏草稿同样先拦截（确认后直达目标对话框）。
        "new" => {
            let host = host.clone();
            let inputs = inputs.clone();
            item.on_click(move |_, window, app| request_create_project(&host, &inputs, window, app))
        }
        "open-folder" => {
            let host = host.clone();
            let inputs = inputs.clone();
            item.on_click(move |_, window, app| request_open_folder(&host, &inputs, window, app))
        }
        // 「项目中心…」：不关闭当前项目，只在上面盖一层列表（选中目标才切换）。
        "switch" => {
            let host = host.clone();
            item.on_click(move |_, _, app| open_project_hub(&host, app))
        }
        // 「关闭项目」才是释放当前项目（回无项目态的选择器）。
        "close" => {
            let host = host.clone();
            item.on_click(move |_, window, app| request_close(&host, window, app))
        }
        // 「重命名…」是「项目设置…」的直达别名：同一个面板，焦点本来就落在改名输入上。
        "settings" | "rename" => {
            let host = host.clone();
            let inputs = inputs.clone();
            item.on_click(move |_, window, app| {
                // 预填当前名称与描述（名册反查；事件上下文允许 I/O）。
                let name = host
                    .session
                    .borrow()
                    .as_ref()
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                let description = current_summary(&host)
                    .and_then(|s| s.description)
                    .unwrap_or_default();
                inputs
                    .rename
                    .update(app, |s, cx| s.set_value(name, window, cx));
                inputs
                    .description
                    .update(app, |s, cx| s.set_value(description, window, cx));
                host.state.borrow_mut().settings_open = true;
                // 设置面板要显示磁盘结构树与描述：在事件路径取快照（render 不做 I/O）。
                load_settings_snapshot(&host, app);
            })
        }
        "reveal" => {
            let path = root.to_path_buf();
            item.on_click(move |_, _, _| reveal_in_explorer(&path))
        }
        "archive" => {
            let host = host.clone();
            item.on_click(move |_, window, app| toggle_archive(&host, window, app))
        }
        _ => item,
    }
}

pub(super) fn reveal_in_explorer(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(target_os = "windows"))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
