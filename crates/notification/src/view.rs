//! 通知中心 UI
//!
//! 状态栏铃铛（`NotificationBell`）+ 点击弹出的历史面板。
//!
//! 为什么铃铛是一个 entity：面板里的「全部已读 / 清空 / 点某条」要立刻反映到
//! 铃铛计数与列表，回调只有 `&mut App`，必须借宿主 entity 的 `cx.notify()` 才能刷新。
//! 计数与内容一律从 `service` 现读，本视图不另存一份状态（避免两套口径）。

use shared::i18n::tr;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::core::{format_relative_time, MAX_HISTORY};
use crate::service;

/// 面板宽度（px，4 的倍数）
///
/// vendor 的 toast 面板总宽是 382（`notification.rs` 的 `DEFAULT_NOTIFICATION_WIDTH`）。
/// 这里的 360 是**内容区**宽，加上 Popover 自身内边距后总宽与之接近，两种提示摆在一起
/// 不会一宽一窄。再窄会让「标题 + 时间」这一行频繁触发省略号。
const PANEL_WIDTH: f32 = 360.;
/// 列表最大高度：超了就滚，避免面板高过一屏
const LIST_MAX_HEIGHT: f32 = 360.;
/// 铃铛未读数上限（超过显示 99+）
const BADGE_MAX: usize = 99;

/// 面板筛选
///
/// 不做「按类型分组」（在 360px 浮层里分组标题会吃掉一半高度），
/// 但用户确实需要「只看要处理的那几条」，所以把错误 + 警告并成一个「问题」档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    /// 全部
    All,
    /// 只看未读
    Unread,
    /// 只看需要处理的（错误 + 警告）
    Issue,
}

impl Filter {
    fn index(self) -> usize {
        match self {
            Self::All => 0,
            Self::Unread => 1,
            Self::Issue => 2,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Unread,
            2 => Self::Issue,
            _ => Self::All,
        }
    }

    fn matches(self, rec: &crate::core::NotificationRecord) -> bool {
        use crate::core::NotificationType;
        match self {
            Self::All => true,
            Self::Unread => !rec.read,
            Self::Issue => matches!(
                rec.type_,
                NotificationType::Error | NotificationType::Warning
            ),
        }
    }
}

/// 状态栏铃铛 + 通知中心
pub struct NotificationBell {
    filter: Filter,
}

/// 铃铛的全局句柄：`service` 写完历史后用它把计数刷新推给视图
///
/// 存 `WeakEntity`：窗口销毁后 `upgrade()` 失败，下次渲染自动重建，
/// 不会留下悬垂强引用（单窗口应用下同一时刻只有一份）。
#[derive(Clone)]
struct BellHandle(WeakEntity<NotificationBell>);

impl Global for BellHandle {}

impl NotificationBell {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.set_global(BellHandle(cx.entity().downgrade()));
        Self { filter: Filter::All }
    }
}

/// 取（必要时创建）全局铃铛视图
///
/// 用 `Global` 而不是 `window.use_keyed_state`：后者绑定在窗口上，
/// 状态栏每帧都会调到这里，`Global` + `WeakEntity` 让「窗口重建」也自然重建一次。
pub fn bell(window: &mut Window, cx: &mut App) -> Entity<NotificationBell> {
    if let Some(handle) = cx.try_global::<BellHandle>()
        && let Some(view) = handle.0.upgrade()
    {
        return view;
    }
    let view = cx.new(|cx| NotificationBell::new(window, cx));
    cx.set_global(BellHandle(view.downgrade()));
    view
}

/// 历史变化后刷新铃铛（`service` 调用；通道被静音时没有 toast，只能靠这条路径刷新）
pub fn notify_changed(cx: &mut App) {
    if let Some(handle) = cx.try_global::<BellHandle>()
        && let Some(view) = handle.0.upgrade()
    {
        view.update(cx, |_, cx| cx.notify());
    }
}

impl Render for NotificationBell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let unread = service::unread_count();
        // 状态栏是品牌色底：按钮必须透明底 + 配前景色，否则会切出一块浅底
        let (fg, hover) = {
            let colors = &cx.theme().colors;
            (colors.primary_foreground, colors.primary_active)
        };
        let entity = cx.entity();

        // 静音时铃铛换成 BellOff：用户看到「有未读数字、却没有弹出」时，
        // 悬停那一下才发现是自己开了静音，代价太高。图标即状态，不用先猜。
        // 只跟随 in-app 静音——铃铛代表「通知会不会到眼前」，系统通道静音
        // 仍在通知中心里有记录，图标不必变。
        let (silent_in_app, silent_system) = crate::settings::read_silent_flags(cx);
        let icon = if silent_in_app {
            gpui_kit::assets::IconName::BellOff
        } else {
            gpui_kit::assets::IconName::Bell
        };
        let tooltip = match (silent_in_app, silent_system) {
            (true, true) => "通知中心（弹出与系统通知均已静音）",
            (true, false) => "通知中心（弹出已静音，系统通知仍开启）",
            (false, true) => "通知中心（系统通知已静音）",
            (false, false) => "通知中心",
        };

        let mut trigger = Button::new("status-notification")
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(cx.theme().transparent)
                    .foreground(fg)
                    .hover(hover),
            )
            .xsmall()
            .icon(icon)
            .tooltip(tooltip);
        // 0 未读时不占位：状态栏横向很紧，一个空 label 也会撑出内距
        if unread > 0 {
            trigger = trigger.label(if unread > BADGE_MAX {
                format!("{BADGE_MAX}+")
            } else {
                unread.to_string()
            });
        }

        Popover::new("status-notification-popover")
            // 状态栏在最下：向左上展开，避开被窗口下缘截断
            .anchor(Anchor::BottomRight)
            .offset(px(8.))
            .trigger(trigger)
            .content(move |_state, _window, app| center(&entity, app))
            .into_any_element()
    }
}

/// 面板内容（每次窗口重绘都从 `service` 现读）
fn center(entity: &Entity<NotificationBell>, cx: &mut App) -> AnyElement {
    let filter = entity.read(cx).filter;
    let records = service::history();
    let unread = records.iter().filter(|r| !r.read).count();
    let muted = cx.theme().colors.muted_foreground;
    // 一帧只取一次「现在」：50 行各调一次 `Local::now()` 会在秒边界上出现
    // 同一屏里「刚刚」与「1 分钟前」并存，属于口径不一致。
    let now = chrono::Local::now();

    let visible: Vec<_> = records.iter().filter(|r| filter.matches(r)).collect();

    // 统计跟着筛选走：切到「未读」后还写「共 50 条」会和眼前 3 行对不上。
    let summary = match filter {
        Filter::All => {
            if records.is_empty() {
                "暂无通知".to_string()
            } else if unread > 0 {
                format!("{} 条 · {unread} 未读", records.len())
            } else {
                format!("{} 条", records.len())
            }
        }
        Filter::Unread => {
            if visible.is_empty() {
                "暂无未读".to_string()
            } else {
                format!("{} 条未读", visible.len())
            }
        }
        Filter::Issue => {
            // 这一档含已读的错误/警告，叫「待处理」名不副实——已读的显然已经看过
            if visible.is_empty() {
                "暂无问题".to_string()
            } else {
                format!("{} 条问题", visible.len())
            }
        }
    };

    let mut body = div()
        .v_flex()
        .w(px(PANEL_WIDTH))
        .gap_2()
        .min_h_0()
        // 头部：标题 + 未读统计 + 操作
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(div().text_sm().child(tr("通知")))
                        .child(div().text_xs().text_color(muted).child(summary)),
                )
                .child(notify_actions(entity, unread, records.is_empty())),
        )
        // 显式给 1px 高度：`Separator` 的横线是 absolute 子元素，不贡献高度，
        // 在 flex 列里会被压成 0 高（既占不到位，也看不出线）。
        .child(Separator::horizontal().h(px(1.)))
        // 筛选：全部 / 未读 / 问题
        .child({
            let filtered_entity = entity.clone();
            TabBar::new("notify-filter")
                .segmented()
                .selected_index(filter.index())
                .children([
                    Tab::new().label(tr("全部")),
                    Tab::new().label(tr("未读")),
                    Tab::new().label(tr("问题")),
                ])
                .on_click(move |&index, _window, app| {
                    let next = Filter::from_index(index);
                    filtered_entity.update(app, |view, cx| {
                        view.filter = next;
                        cx.notify();
                    });
                })
        });

    body = body.child(if visible.is_empty() {
        empty_hint(cx, filter)
    } else {
        let mut list = div()
            .v_flex()
            // 4px 网格（`rds-design` 禁止 6/10 这类非 4 的倍数；2px 也不该用）
            .gap_1()
            .max_h(px(LIST_MAX_HEIGHT))
            .min_h_0()
            .overflow_y_scrollbar();
        for rec in visible {
            list = list.child(notification_row(entity, rec, now, cx));
        }
        // 历史到达上限后最早的那条会被挤掉（`HistoryStore::record` 里 truncate），
        // 不提示的话用户只会觉得「前天那条怎么没了」。
        // 只在「全部」档提示：未读/问题档只是筛选，条数少不代表历史被删。
        if filter == Filter::All && records.len() >= MAX_HISTORY {
            list = list.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("更早的通知已移除，仅保留最近 {MAX_HISTORY} 条")),
            );
        }
        list.into_any_element()
    });
    body.into_any_element()
}

/// 「全部已读 / 清空」按钮组
///
/// 都走 `Button`（键盘可达 + hover + disabled 语义），点击后刷新宿主 entity。
fn notify_actions(entity: &Entity<NotificationBell>, unread: usize, empty: bool) -> AnyElement {
    let read_entity = entity.clone();
    let clear_entity = entity.clone();
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .flex_none()
        .child(
            Button::new("notify-mark-all-read")
                .ghost()
                .xsmall()
                .label(tr("全部已读"))
                .disabled(unread == 0)
                .on_click(move |_, _, app| {
                    service::mark_all_read();
                    read_entity.update(app, |_, cx| cx.notify());
                }),
        )
        .child(
            Button::new("notify-clear")
                .ghost()
                .xsmall()
                .label(tr("清空"))
                .tooltip(tr("清空通知历史（屏上已弹出的提示不受影响），不可恢复"))
                .disabled(empty)
                .on_click(move |_, _, app| {
                    service::clear_history();
                    clear_entity.update(app, |_, cx| cx.notify());
                }),
        )
        .into_any_element()
}

/// 单条通知行
///
/// 用 `ListItem` 而不是自绘 `div().on_click()`：hover / 键盘焦点 / 禁用语义都在组件里，
/// 自绘版本会把这些一并丢掉（见 gpui-kit-dev 的组件选型表）。
fn notification_row(
    entity: &Entity<NotificationBell>,
    rec: &crate::core::NotificationRecord,
    now: chrono::DateTime<chrono::Local>,
    cx: &mut App,
) -> AnyElement {
    // 未读点用强调色而不是类型色：Info 的蓝图标配蓝点几乎看不见，
    // 而「未读」应该是一个稳定的视觉语言，不该随类型漂移。
    let accent = cx.theme().accent;
    let (tint, muted) = {
        let colors = &cx.theme().colors;
        let tint = match rec.type_ {
            crate::core::NotificationType::Info => colors.info,
            crate::core::NotificationType::Success => colors.success,
            crate::core::NotificationType::Warning => colors.warning,
            crate::core::NotificationType::Error => colors.danger,
        };
        (tint, colors.muted_foreground)
    };
    let id = rec.id.clone();
    let row_entity = entity.clone();
    let unread = !rec.read;

    let icon = Icon::new(rec.type_.icon()).size_3p5().text_color(tint);
    // 未读用标准 `Badge` 圆点叠在类型图标右上：手搓圆点会丢尺寸档位与主题取色
    let leading = if unread {
        Badge::new().dot().color(accent).child(icon).into_any_element()
    } else {
        icon.into_any_element()
    };

    // 第一行：标题（占满、超长省略）+ 合并次数 + 时间（右对齐）
    //
    // 时间放在标题同一行而不是单独一行：一条通知从 3 行压到 2 行，
    // 50 条历史的可扫读条数就翻了一倍。
    let head = div()
        .h_flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_ellipsis()
                // 未读用字重而不是整行底色：`ListItem::selected` 用的是「选中」语义色
                // （accent），拿来做未读高亮太扎眼，而且 active 行内部会压掉 hover。
                .when(unread, |el| el.font_weight(FontWeight::MEDIUM))
                .child(text!(&rec.title)),
        )
        .when(rec.count > 1, |row| {
            row.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(tint)
                    .child(format!("×{}", rec.count)),
            )
        })
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(format_relative_time(&rec.timestamp, now)),
        );

    let mut body = div().v_flex().flex_1().min_w_0().gap_1().child(head);
    if let Some(message) = rec.message.as_ref() {
        body = body.child(
            div()
                .text_xs()
                .text_color(muted)
                .text_ellipsis()
                .child(text!(message)),
        );
    }

    ListItem::new(SharedString::from(format!("notify-row::{}", rec.id)))
        .px_2()
        .py_1()
        .on_click(move |_, _, app| {
            // 已经是已读的行不必再刷新（`mark_read` 会告诉我们有没有变更）
            if service::mark_read(&id) {
                row_entity.update(app, |_, cx| cx.notify());
            }
        })
        // 行尾「删除这一条」。走 `suffix` 而不是塞进内容容器：
        // suffix 在 `ListItem` 的 `justify_between` 末尾单独成块，
        // 点击它不会和整行的「标记已读」抢事件（编辑器历史面板是同一套用法）。
        .suffix({
            let suffix_entity = entity.clone();
            let suffix_id = rec.id.clone();
            move |_window, _cx| {
                let bell = suffix_entity.clone();
                let id = suffix_id.clone();
                Button::new(SharedString::from(format!("notify-remove::{}", id)))
                    .ghost()
                    .xsmall()
                    .icon(gpui_kit::assets::IconName::X)
                    .tooltip(tr("从历史里删除这一条（屏上已弹出的提示不受影响）"))
                    .on_click(move |_, _, app| {
                        if service::remove(&id) {
                            bell.update(app, |_, cx| cx.notify());
                        }
                    })
                    .into_any_element()
            }
        })
        // 只塞一个容器元素：`ListItem` 会把所有 children 收进同一个 `w_full` 的 div，
        // 并列多个 child 时的排布依赖默认方向，不可控
        // （项目内其它 `ListItem` 用法也都只给一个容器）。
        .child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(leading)
                .child(body),
        )
        .into_any_element()
}

/// 列表为空时的提示
///
/// 这里不用 `empty::Empty`：它是面板级空态（`flex_1` + `p_6` + 虚线框），
/// 塞进 360px 的浮层会撑出一大块留白；通知中心的空态只是「这一屏没有行」。
fn empty_hint(cx: &mut App, filter: Filter) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    let (icon, text) = match filter {
        Filter::All => (gpui_kit::assets::IconName::Inbox, "暂无通知"),
        Filter::Unread => (gpui_kit::assets::IconName::CircleCheck, "没有未读通知"),
        Filter::Issue => (gpui_kit::assets::IconName::TriangleAlert, "没有错误或警告"),
    };
    div()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_1()
        .py_6()
        .child(Icon::new(icon).size_6().text_color(muted))
        .child(div().text_sm().text_color(muted).child(text))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::Filter;
    use crate::core::{NotificationRecord, NotificationType};
    // 父模块的 `tr` 是私有 `use` 绑定，子模块不自动继承（i18n 合流遗留：测试模块漏了这条）。
    use shared::i18n::tr;

    fn record(type_: NotificationType, read: bool) -> NotificationRecord {
        NotificationRecord {
            id: "test".to_string(),
            type_,
            title: tr("标题").to_string(),
            message: None,
            timestamp: "2026-01-01T00:00:00+08:00".to_string(),
            count: 1,
            read,
        }
    }

    /// TabBar 只给下标，`from_index` 必须能还原回去（写歪会让点「未读」跳到「问题」）
    #[test]
    fn filter_index_round_trips() {
        for filter in [Filter::All, Filter::Unread, Filter::Issue] {
            assert_eq!(Filter::from_index(filter.index()), filter);
        }
        // 越界下标退回「全部」，不能让面板变成空列表
        assert_eq!(Filter::from_index(9), Filter::All);
    }

    #[test]
    fn all_filter_matches_every_record() {
        assert!(Filter::All.matches(&record(NotificationType::Error, true)));
        assert!(Filter::All.matches(&record(NotificationType::Info, false)));
    }

    #[test]
    fn unread_filter_keeps_only_unread() {
        assert!(Filter::Unread.matches(&record(NotificationType::Info, false)));
        assert!(!Filter::Unread.matches(&record(NotificationType::Error, true)));
    }

    #[test]
    fn issue_filter_keeps_errors_and_warnings() {
        assert!(Filter::Issue.matches(&record(NotificationType::Error, true)));
        assert!(Filter::Issue.matches(&record(
            NotificationType::Warning,
            false
        )));
        assert!(!Filter::Issue.matches(&record(NotificationType::Success, true)));
        assert!(!Filter::Issue.matches(&record(NotificationType::Info, false)));
    }
}
