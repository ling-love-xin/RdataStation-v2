//! 资源面板的区域渲染（`render_header` / `render_toolbar` / 空态 / 状态行等）
//! 与 `Render` 协议。
//!
//! 拆分自 `resource_view/mod.rs`（P1）：`impl ResourcesPanel` 的渲染方法原样搬移
//! （子模块可访问父模块的私有字段/方法，`render` 同步点与 `ElementId` 保持不变）；
//! `impl Render for ResourcesPanel` 一并迁入，与区域方法同文件。
//!
//! 渲染纪律（继承 `mod.rs` 头注）：render 期零 I/O，面板只读快照。

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::empty::{Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle};
use gpui_kit::component::input::Input;
use gpui_kit::component::list::List;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::commands;
use crate::filter::{self, SortField};
use crate::model::{ArchiveKind, ArchiveUndo, DeleteTarget, PayloadMode};
use crate::resource_view::{HeaderMenuAction, InstanceFilter, ResourcesPanel, dispatch_header_action};
use crate::ui;

impl ResourcesPanel {
    // ==================== 区域渲染 ====================
    //
    // 返回值写具体类型（`Div` / `AnyElement`）：edition 2024 的 RPIT 会捕获入参生命周期，
    // 返回 `impl IntoElement` 时它会借住 `cx`，连续调两个区域函数就变成"重复可变借用"。

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
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
    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
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
                                    target.update(app, |panel, cx| panel.set_instance(candidate, cx));
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

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.clone()?;
        let (border, warning, info) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.info)
        };
        // 只读是约束提示（warning），其余是信息（info）——同一字段不一律同色。
        let color = if self.snapshot.read_only {
            warning
        } else {
            info
        };
        Some(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .border_b(ui::HAIRLINE)
                .border_color(border)
                .text_color(color)
                .child(notice)
                .into_any_element(),
        )
    }

    /// 列表主体：`List`（虚拟化 + 组件化的 hover / 选中 / 键盘漫游）。
    ///
    /// 行数据在委托里（面板重算可见行时推过去）：`render_item` 在列表渲染期被调用，
    /// 那一刻面板正被借用，不能回头读面板的行集合。滚动归 `List` 自己（虚拟化列表自带）。
    fn render_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(list) = self.list.clone() else {
            // 理论上不可达（`render` 已先 `ensure_list`）：退化成空区而不是 panic。
            let _ = cx;
            return div().flex_1().into_any_element();
        };
        div()
            .flex_1()
            .min_h_0()
            .child(List::new(&list).flex_1())
            .into_any_element()
    }

    /// 归档撤销栏（原型 §4.1）：给"把文件从工作区搬走"一个立即反悔的窗口。
    ///
    /// 固定在状态行**上方**（与 M5 撤销栏同位）：它是刚发生那次动作的出口，不能随列表滚走；
    /// 文案直接引用显示名，用户不用回想刚搬的是哪一条。
    fn render_undo_bar(&self, undo: &ArchiveUndo, cx: &mut Context<Self>) -> Div {
        let (foreground, primary, hover_bg) = {
            let colors = cx.theme().colors;
            (colors.foreground, colors.primary, colors.list_hover)
        };
        let host = self.host.clone();
        let token = undo.clone();
        div()
            .flex_none()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            // 栏高保持 `ROW_HEIGHT`（24px）：`py_1` + 20px 的按钮会多出 4px，“贴着状态行”的
            // 这条栏一高一矮会跟着上下跳。
            .py_0p5()
            .bg(hover_bg)
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .text_color(foreground)
                    .child(format!("已归档「{}」", undo.name)),
            )
            .child(
                // 撤销是**唯一**的入口（`Ctrl+Z` 未绑），所以它得是语义按钮：自绘 div 没有
                // hover / 焦点 / 键盘；`Button` 自带 `track_focus` + `tab_stop`，Enter/Space 可激活。
                Button::new("archive-undo")
                    .ghost()
                    .xsmall()
                    .debug_selector(|| "archive-undo".to_string())
                    .label("撤销")
                    .text_color(primary)
                    .tooltip("把这条存档退回归档前的位置")
                    .on_click(move |_, window, cx| host.request_undo_archive(&token, window, cx)),
            )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> Div {
        let (border, warning, muted) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.muted_foreground)
        };
        let counts = self.snapshot.counts;
        let has_issues = counts.has_issues();
        // 「全局」实例下计数行换成全局口径（`ArchiveCounts` 是项目专用：kind 桶 /
        // 异常桶只对项目行有意义）；异常入口同理只在项目口径下摆。
        let counts_line = match self.instance {
            InstanceFilter::Global => {
                let n = self.snapshot.global_rows.len();
                if self.loading {
                    format!("加载中… · 全局 {n} 项")
                } else {
                    format!("全局 {n} 项")
                }
            }
            _ => counts.status_line(self.loading),
        };
        let has_issues = has_issues && self.instance != InstanceFilter::Global;
        let host = self.host.clone();

        div()
            .flex_none()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .justify_between()
            .px_2()
            .border_t(ui::HAIRLINE)
            .border_color(border)
            .child(
                div()
                    .text_xs()
                    .text_color(if has_issues { warning } else { muted })
                    .child(counts_line),
            )
            .when(has_issues, |bar| {
                bar.child(
                    Button::new("index-repair")
                        .ghost()
                        .label("修复…")
                        .on_click(move |_, window, cx| host.request_index_repair(window, cx)),
                )
            })
    }

    /// D2：索引异常首次进入的一次性提示条（仅项目实例；全局实例没有本地索引口径）。
    ///
    /// 与状态行「修复…」同源数据：这里只在首次进入时把异常"叫出来"一次，关闭后会话内
    /// 不再出现；纯内存标记，不进快照、不进索引统计口径。
    fn render_index_issue_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.instance == InstanceFilter::Global {
            return None;
        }
        let counts = &self.snapshot.counts;
        if !counts.has_issues() || self.announced_index_issue.get() {
            return None;
        }
        let (warning, muted, border) = {
            let colors = cx.theme().colors;
            (colors.warning, colors.muted_foreground, colors.border)
        };
        let mut parts = Vec::new();
        if counts.missing > 0 {
            parts.push(format!("本体缺失 {} 项", counts.missing));
        }
        if counts.drifted > 0 {
            parts.push(format!("内容已变 {} 项", counts.drifted));
        }
        let summary = parts.join("、");
        Some(
            div()
                .flex_none()
                .w_full()
                .h_flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .border_b(ui::HAIRLINE)
                .border_color(border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(warning)
                        .child(format!("发现{summary}，可在状态行「修复…」处理。")),
                )
                .child(
                    Button::new("index-issue-banner-close")
                        .ghost()
                        .xsmall()
                        .label("×")
                        .text_color(muted)
                        .on_click(cx.listener(|panel, _event, _window, cx| {
                            panel.announced_index_issue.set(true);
                            cx.notify();
                        })),
                ),
        )
    }

    /// 加载中骨架（原型 §5）：3 行灰条，**不用转圈**——形状直接提示"这里将要出现行"，
    /// 宽度递减以免看着像真行。
    ///
    /// 灰条走组件的 `Skeleton`（自带 2s 呼吸，只改透明度；`reduce_motion` 下停在全亮）：
    /// 这是**暂态**（首帧取数期间），符合「动效只挂暂态」；列表已有行时不摆它，所以
    /// 不会每帧重绘。
    fn render_loading(&self) -> Div {
        let mut skeleton = div()
            .flex_1()
            .v_flex()
            .w_full()
            .min_h_0()
            .gap_2()
            .p_2()
            // 调试选择器：窗口测试靠它断言"取数期间不摆空态"。
            .debug_selector(|| "archive-loading".to_string());
        for ratio in [0.75_f32, 0.55, 0.65] {
            skeleton = skeleton.child(
                Skeleton::new()
                    .h(rems(ui::ROW_HEIGHT))
                    .w(relative(ratio))
                    .rounded_sm(),
            );
        }
        skeleton
    }

    fn render_empty(&self, _cx: &mut Context<Self>) -> Empty {
        let read_only = self.snapshot.read_only;
        // 空态按实例分流（§13.2）：全局实例不直接归档，只给一句"升级上来"的引导，
        // 不摆草稿箱 / 本地文件两个项目侧入口；项目与「全部」视图保持原归档引导。
        // 组件选型：gpui-kit `empty::Empty`（规格 §7.2 UI 统一；自带居中布局 + 虚线框），
        // 标题 / 说明 / 按钮分别进 `EmptyHeader` / `EmptyContent` 槽位。
        let is_global = self.instance == InstanceFilter::Global;
        let (title, hint) = if is_global {
            (
                "全局实例还没有存档",
                "把项目里值得共享的存档升级到全局后会出现在这里；全局实例不直接归档，跨项目分发都从这里选。",
            )
        } else {
            (
                "还没有任何存档",
                "把草稿箱里值得留存的文件归档进来，它就变成只读、带版本、可追溯的存档",
            )
        };

        let empty = Empty::new().header(
            EmptyHeader::new()
                .title(EmptyTitle::new().child(title))
                .description(EmptyDescription::new().child(hint)),
        );

        if is_global {
            empty
        } else {
            // 两个按钮各拿一份（`Rc` 不是 `Copy`：两个闭包都要 `move` 走它）。
            let host_for_drafts = self.host.clone();
            let host_for_file = self.host.clone();
            empty.content(
                EmptyContent::new()
                    .child(
                        Button::new("archive-for-empty")
                            .primary()
                            .debug_selector(|| "archive-empty-action".to_string())
                            .label("从草稿箱归档…")
                            .disabled(read_only)
                            .on_click(move |_, window, cx| {
                                host_for_drafts.request_archive_from_drafts(window, cx)
                            }),
                    )
                    .child(
                        Button::new("archive-for-empty-file")
                            .secondary()
                            .label("从本地文件归档…")
                            .disabled(read_only)
                            .on_click(move |_, window, cx| {
                                host_for_file.request_archive_from_file(window, cx)
                            }),
                    ),
            )
        }
    }

    /// 搜索 / 筛选无结果：**不是空库态**（原型 §5 状态与空态矩阵）。
    ///
    /// 两态的出口才是关键区别：空库引导"归档"（没有东西可用），无匹配引导"清筛选"
    /// （东西在，只是被挡住了）。文案混用会把用户引到错误的下一步。
    fn render_no_match(&self, cx: &mut Context<Self>) -> Empty {
        let panel = cx.entity();

        // 副文案由数据层给（`filter::no_match_hint`）：搜的是“匹配面”，
        // 能搜哪些字段就该由管匹配面的那处说了算，而不是渲染层自己编一句。
        Empty::new()
            .header(
                EmptyHeader::new()
                    .title(EmptyTitle::new().child("没有匹配的存档"))
                    .description(
                        EmptyDescription::new().child(filter::no_match_hint(&self.filter.query)),
                    ),
            )
            .content(
                EmptyContent::new().child(
                    Button::new("archive-clear-filter")
                        .ghost()
                        .label("清空筛选")
                        .on_click(move |_, window, app| {
                            panel.update(app, |panel, cx| panel.clear_filter(window, cx));
                        }),
                ),
            )
    }
}

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
