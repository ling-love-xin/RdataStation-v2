//! 分组头：色条、展开指示、名称、计数与分组自身的菜单。

use super::*;

pub(super) fn render_group_header(
    delegate: &ArchiveListDelegate,
        key: &str,
        label: &str,
        count: usize,
        depth: u8,
        collapsed: bool,
        cx: &mut Context<ListState<ArchiveListDelegate>>,
    ) -> ListItem {
        let (bar, muted, fg) = {
            let colors = cx.theme().colors;
            // 分组色条取 `primary`：与 M4 分组头同角色（`rds-theme.json` 里与
            // `list_active_border` 明暗同值，所以是零视觉变化的口径统一）。不用
            // `sidebar_accent`：那个角色在浅色下与面板底几乎同色。
            (colors.primary, colors.muted_foreground, colors.foreground)
        };
        let panel = delegate.panel.clone();
        let key_owned = key.to_string();
        // 默认分组的标记要画在头上：只拷一份，供后面判断（`render_item` 里读不了面板）。
        let default_group = delegate.default_group.clone();
        let list_id = format!("archive-group-{key}");
        let debug_id = list_id.clone();
        let host = delegate.host.clone();
        let is_real_group =
            key != crate::filter::GROUP_ALL && key != crate::filter::GROUP_UNGROUPED;

        let head = div()
            .id(SharedString::from(format!("{list_id}-row")))
            .debug_selector(move || debug_id.clone())
            .w_full()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .items_center()
            .gap_2()
            .when(depth > 0, |row| row.pl_2())
            .cursor_pointer()
            .on_click({
                let panel = panel.clone();
                move |_, _window, cx| {
                    // 头不是行：不参与选中（面板的选中永远指向一条存档）。
                    cx.stop_propagation();
                    let key = key_owned.clone();
                    let _ = panel.update(cx, |panel, cx| panel.toggle_group_collapse(&key, cx));
                }
            })
            .child(
                // 2px 分组色条：宽取 `ui::GROUP_BAR_WIDTH`（= 外壳的 `NAV_GROUP_BAR_WIDTH`，
                // 固定描边，不随主题字号缩放），高度 / 圆头与 M4 分组头同形。
                div()
                    .w(ui::GROUP_BAR_WIDTH)
                    .h(rems(0.875))
                    .flex_none()
                    .rounded_full()
                    .bg(bar),
            )
            // 展开指示走共用原语：`disclosure_slot` 的 10px 槽宽是缩进算式的一部分
            // （行内容 = `pl` + 槽 + `gap_1`），与 M4/M5 换载体时一样——只动这一行调用，
            // 同层行的标题左边缘不会漂。
            .child(tree::disclosure_slot().child(tree::disclosure_icon(!collapsed, muted)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_ellipsis()
                    .text_color(if depth == 0 { fg } else { muted })
                    .child(label.to_string()),
            )
            .child(
                // 「默认」标记：本项目里带这个标记的分组，就是归档对话框的预选落点。
                // 只有一条时才有——它是**事实提示**，不是控件（设 / 取消在右键菜单里）。
                div().flex_none().text_xs().text_color(muted).when_some(
                    (default_group.as_deref() == Some(key))
                        .then(|| SharedString::from(format!("archive-group-default-{key}"))),
                    |marker, id| {
                        marker
                            .debug_selector(move || id.to_string())
                            .child(DEFAULT_GROUP_MARKER)
                    },
                ),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{count}")),
            );
        // 落点：分组头接住存档行（原型 §3.2「拖动行到分组头 = 移动」）。
        // 高亮用 `list_active`（与选中底同 token，见 `ui-constraints.md` §8.3）：
        // 拖拽期间这行就是「东西要进到这里」的那个容器。
        let drop = dnd::group_drop_target(key);
        let head = if matches!(drop, GroupDrop::NotATarget) {
            head
        } else {
            head.drag_over::<ArchiveDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let panel = panel.clone();
                let drop = drop.clone();
                move |payload: &ArchiveDragPayload, window, app| {
                    let ids = payload.ids.clone();
                    let drop = drop.clone();
                    let _ = panel.update(app, |panel, cx| {
                        panel.drop_rows_onto_group(&ids, drop.clone(), window, cx)
                    });
                }
            })
        };
        // 右键菜单只给真分组（「全部分组」与「未分组」是虚拟分组，没有可改的东西）。
        // `context_menu` 包一层（`ContextMenu<..>` 不是 `Div`）→ 两条分支各自转 `AnyElement` 后再拼。
        let head: AnyElement = if is_real_group {
            let folder_id = key.to_string();
            let is_default = default_group.as_deref() == Some(key);
            let panel_for_default = panel.clone();
            head.context_menu(move |menu, _window, _cx| {
                menu.item(PopupMenuItem::new("重命名…").on_click({
                    let host = host.clone();
                    let folder_id = folder_id.clone();
                    move |_, window, cx| host.request_rename_group(&folder_id, window, cx)
                }))
                // 「设为 / 取消归档默认分组」：默认分组的**唯一入口**（原型 §4.1 的预选来源）。
                // 放在重命名与删除之间：它是这个分组的日常属性，不是破坏性动作。
                .item(
                    PopupMenuItem::new(default_group_entry_label(is_default)).on_click({
                        let folder_id = folder_id.clone();
                        let panel = panel_for_default.clone();
                        move |_, _window, cx| {
                            let folder_id = folder_id.clone();
                            let _ = panel.update(cx, |panel, cx| {
                                panel.toggle_default_group(&folder_id, cx);
                            });
                        }
                    }),
                )
                .item(PopupMenuItem::new("删除分组").on_click({
                    let host = host.clone();
                    let folder_id = folder_id.clone();
                    move |_, window, cx| host.request_delete_group(&folder_id, window, cx)
                }))
                .separator()
                .item(PopupMenuItem::new("新建分组…").on_click({
                    let host = host.clone();
                    move |_, window, cx| host.request_create_group(window, cx)
                }))
            })
            .into_any_element()
        } else {
            head.into_any_element()
        };

        list_row(SharedString::from(list_id.clone())).child(head)
    }

