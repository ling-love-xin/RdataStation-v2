//! 资源面板的区域渲染（`render_header` / `render_toolbar` / 空态 / 状态行等）
//! 与 `Render` 协议。
//!
//! 拆分自 `resource_view/mod.rs`（P1）：`impl ResourcesPanel` 的渲染方法原样搬移
//! （子模块可访问父模块的私有字段/方法，`render` 同步点与 `ElementId` 保持不变）；
//! `impl Render for ResourcesPanel` 一并迁入，与区域方法同文件。
//!
//! 渲染纪律（继承 `mod.rs` 头注）：render 期零 I/O，面板只读快照。

pub(crate) use gpui_kit::base::{Disableable as _, StyledExt};
pub(crate) use gpui_kit::component::Sizable as _;
pub(crate) use gpui_kit::component::button::{Button, ButtonVariants};
pub(crate) use gpui_kit::component::empty::{Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle};
pub(crate) use gpui_kit::component::input::Input;
pub(crate) use gpui_kit::component::list::List;
pub(crate) use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
pub(crate) use gpui_kit::component::skeleton::Skeleton;
pub(crate) use gpui_kit::component::{ActiveTheme, Icon};
pub(crate) use gpui_kit::prelude::FluentBuilder as _;
pub(crate) use gpui_kit::*;

pub(crate) use crate::commands;
pub(crate) use crate::filter::{self, SortField};
pub(crate) use crate::model::{ArchiveKind, ArchiveUndo, DeleteTarget, PayloadMode};
pub(crate) use crate::resource_view::{
    HeaderMenuAction, InstanceFilter, ResourcesPanel, dispatch_header_action,
};
pub(crate) use crate::ui;

mod banners;
mod chrome;
mod empty;

impl Render for ResourcesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 首帧创建搜索框与列表状态（懒创建，见字段注释）：之后本帧即可渲染工具栏与行。
        self.ensure_search_input(window, cx);
        self.ensure_list(window, cx);
        // 宿主推送（无窗口）改不了列表索引，这里把面板的选中镜像回去。
        self.sync_list_selection(window, cx);
        let background = cx.theme().colors.background;
        // 各区域函数返回具体类型（见上文注释），故可依次调用、各自持有已完成的元素。
        let header = self.render_header(cx);
        let toolbar = self.render_toolbar(cx);
        let notice = self.render_notice(cx);
        let issue_banner = self.render_index_issue_banner(cx);
        // 两种空：**空库**（条件为空，没有东西可用）与**无匹配**（条件非空，东西被挡住了）。
        // `ResourcesFilter::is_empty` 就是为这行判定存在的（见 `filter.rs` 模块头）。
        let body = if self.view_rows.is_empty() {
            if self.filter.is_empty() {
                // 首个快照到达前不摆空态：那是"还没读到"，不是"没有存档"（原型 §5）。
                if self.loading {
                    self.render_loading().into_any_element()
                } else {
                    self.render_empty(cx).into_any_element()
                }
            } else {
                self.render_no_match(cx).into_any_element()
            }
        } else {
            self.render_rows(cx)
        };
        let status = self.render_status_bar(cx);
        // 撤销栏固定在上方（与 M5 撤销栏同位）：它是刚发生那次动作的出口，不能随列表滚走。
        let undo_bar = self
            .undo
            .as_ref()
            .map(|undo| self.render_undo_bar(undo, cx));
        let host = self.host.clone();

        div()
            .id("analytics-resource-panel")
            .role(gpui::Role::List)
            // `track_focus` 不能省：没有它面板不在 dispatch path 上，快捷键就落不到动作（已踩）。
            .track_focus(&self.focus_handle)
            // 键盘上下文：快捷键由 app 层绑到 `analytics-resource` context（视图不自注全局键）。
            .key_context("analytics-resource")
            .on_action(cx.listener({
                let host = host.clone();
                move |_: &mut Self, _: &commands::RequestArchive, window, cx| {
                    // 动作没有"选菜单"这一步，落到**本地文件**那条（与面板头第二项一致）。
                    host.request_archive_from_file(window, cx)
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::OpenSelected, window, cx| {
                    let detail = panel
                        .selected
                        .as_deref()
                        .and_then(|id| panel.snapshot.details.get(id))
                        .cloned();
                    if let Some(detail) = detail.as_ref() {
                        host.request_open(detail, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::CheckoutSelected, window, cx| {
                    let detail = panel
                        .selected
                        .as_deref()
                        .and_then(|id| panel.snapshot.details.get(id))
                        .cloned();
                    if let Some(detail) = detail.as_ref() {
                        host.request_checkout(detail, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::DeleteSelected, window, cx| {
                    // 多选时对整个选择集生效（原型 §3.2：删除是唯一多选可用的项）。
                    let ids = panel.selected_ids();
                    if !ids.is_empty() {
                        let targets: Vec<DeleteTarget> = ids
                            .iter()
                            .map(|id| DeleteTarget {
                                id: id.clone(),
                                mode: panel
                                    .snapshot
                                    .details
                                    .get(id)
                                    .map(|detail| detail.payload_mode)
                                    .unwrap_or(PayloadMode::DEFAULT),
                            })
                            .collect();
                        host.request_delete(&targets, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                // `F2`：重命名焦点行的**显示名**（与菜单里那项同一判据，见 `can_rename`）。
                move |panel: &mut Self, _: &commands::RenameSelected, window, cx| {
                    let Some(id) = panel.rename_target() else {
                        return;
                    };
                    host.request_rename(&id, window, cx);
                }
            }))
            .on_action(cx.listener({
                // `Ctrl+A`：全选当前可见行（分组未落，即全部可见行）。
                move |panel: &mut Self, _: &commands::SelectAllRows, _window, cx| {
                    panel.select_all(cx);
                }
            }))
            .on_action(cx.listener({
                // `Ctrl+F`：聚焦搜索框（`InputState` 需要窗口，而 action 处理器能拿到）。
                move |panel: &mut Self, _: &commands::FocusSearch, window, cx| {
                    if let Some(input) = panel.search_input.clone() {
                        input.update(cx, |state, cx| state.focus(window, cx));
                    }
                }
            }))
            .on_action(cx.listener({
                // `Esc`：只清搜索词（种类 / 只看需处理留在菜单里，误清会让人以为筛选坏了）。
                move |panel: &mut Self, _: &commands::ClearSearch, window, cx| {
                    panel.set_query("", window, cx);
                }
            }))
            .v_flex()
            .size_full()
            // 面板自己拥有滚动（Dock 的 tab-content 不产生滚动）：滚动挂在 body 上。
            .min_h_0()
            .bg(background)
            .child(header)
            .child(toolbar)
            .when_some(notice, |panel, notice| panel.child(notice))
            .when_some(issue_banner, |panel, banner| panel.child(banner))
            .child(body)
            .when_some(undo_bar, |panel, bar| panel.child(bar))
            .child(status)
    }
}
