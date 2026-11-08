//! 连接选择器（工具栏「连接名称」点击弹窗）。

use super::*;

// ==== 连接选择器（工具栏「连接名称」点击弹窗；2026-10-27）====
//
// 图：标题「选择数据源」+ 连接列表（短码·名称 + 类型 + 在线标记 + 当前勾选）+
// 底部「仅显示连接的数据库」复选（取消 = 显示全部）。点击 / 回车绑定该连接并关闭；
// Esc / 点遮罩 = 取消（不改绑定）。回调 `None` 语义留给调用方（绑定 None = 跟随当前连接，
// 但此选择器只给出 `Some(id)`——跟随项由工具栏 ▾ 下拉提供）。

/// 连接选择列表代理：一行 = 一条连接；`only_connected` 过滤由底部复选切换
/// （`ListState::delegate_mut` 重绘，不重建对话框）。
pub struct ConnectionPickerDelegate {
    /// 全部连接（内存快照；渲染路径只读）
    pub(crate) options: Vec<crate::connection::ConnectionOption>,
    /// 仅显示已建连的连接（图 1 底部复选；默认开）
    pub(crate) only_connected: bool,
    /// 当前文档绑定的连接 id（勾选标记）
    pub(crate) current: Option<String>,
    pub(crate) selected: Option<IndexPath>,
    /// 宿主绑定回调（`Some(id)` = 绑定该连接）
    pub(crate) on_pick: Rc<dyn Fn(Option<String>, &mut Window, &mut App)>,
}

impl ConnectionPickerDelegate {
    pub(crate) fn filtered(&self) -> Vec<&crate::connection::ConnectionOption> {
        self.options
            .iter()
            .filter(|o| !self.only_connected || o.connected)
            .collect()
    }
}

impl ListDelegate for ConnectionPickerDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.filtered().len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let filtered = self.filtered();
        let opt = filtered.get(ix.row)?;
        let theme = cx.theme();
        let checked = self.current.as_deref() == Some(opt.id.as_str());
        Some(
            ListItem::new(SharedString::from(format!("conn-picker-{}", opt.id)))
                .h(rems(ui::ENCODING_DIALOG_ROW_HEIGHT))
                .py_0()
                .px_2()
                .gap_2()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .text_color(theme.colors.foreground)
                                .child(format!("{}·{}", opt.short, opt.name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(opt.db_type.clone()),
                                ),
                        )
                        .when(checked, |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .size_3()
                                    .text_color(theme.colors.success),
                            )
                        })
                        .when(!opt.connected, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("未连接"),
                            )
                        }),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .v_flex()
            .items_center()
            .py_6()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child("（无可用连接）")
            .into_any_element()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(ix) = self.selected else {
            return;
        };
        let filtered = self.filtered();
        let Some(opt) = filtered.get(ix.row) else {
            return;
        };
        let id = opt.id.clone();
        let on_pick = self.on_pick.clone();
        window.close_dialog(cx);
        on_pick(Some(id), window, cx);
    }
}

/// 打开连接选择器对话框
///
/// `on_pick`：确认后携带选中连接 id 回调宿主（宿主负责 `bind_connection` + 状态刷新）。
/// 返回 `ListState` 实体：测试经它直接驱动确认（headless 口径，见模块头）。
pub fn open_connection_picker(
    window: &mut Window,
    cx: &mut App,
    options: Vec<crate::connection::ConnectionOption>,
    current: Option<String>,
    on_pick: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
) -> Entity<ListState<ConnectionPickerDelegate>> {
    let delegate = ConnectionPickerDelegate {
        options,
        only_connected: true,
        current,
        selected: None,
        on_pick: Rc::new(on_pick),
    };
    let state = cx.new(|cx| {
        ListState::new(delegate, window, cx)
            .searchable(false)
            .selectable(true)
    });
    let list = state.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let only = list.clone();
        let check = Checkbox::new("conn-picker-only-connected")
            .checked(list.read(cx).delegate().only_connected)
            .label("仅显示连接的数据库")
            .on_click(move |checked, _window, cx| {
                only.update(cx, |list, cx| {
                    list.delegate_mut().only_connected = *checked;
                    cx.notify();
                });
            });
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("选择要绑定到本文档的连接（回车 / 点击确认，Esc 关闭）"),
            )
            .child(List::new(&list).max_h(rems(ui::PICKER_LIST_MAX_HEIGHT)))
            .child(check);
        dialog
            .title("选择数据源")
            .w(theme.font_size * ui::ENCODING_DIALOG_WIDTH)
            .child(body)
    });
    state
}
