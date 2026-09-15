//! 资产库面板（M6 自持视图，左 Dock）。
//!
//! # 归属
//!
//! 视图随能力同 crate（与 `project::ui` / `mock::mock_view` 同例）；本 crate **不依赖 workbench**，
//! 宿主能力经 [`ResourcesHost`] 注入（workbench 侧桥接留待接线那一批）。
//!
//! # 渲染纪律
//!
//! - **render 期零 I/O**：面板只读自己持有的快照（[`ArchiveRow`] 由宿主取好并格式化）；
//!   一切服务调用（归档 / 取回 / 打开 / 扫描）都在事件路径经宿主发起。
//! - 面板不知道 `project.db` / `resources/` 的存在，也不该知道。
//!
//! # 本批范围（Phase 1 第一刀）
//!
//! 已落地：面板头（标题 + 归档入口）、提示行（只读与通知分色）、行列表（显示名 / 版本徽标 /
//! **复现强度徽标** / 尾部字段 / 选中态）、底部状态行（异常时给"修复…"入口）、空态、只读禁写。
//!
//! **未落地（下一批，已在开发方案留档）**：搜索框与筛选/排序菜单、虚拟化列表（`list::List`；
//! 当前行用 `Button` 渲染，几百行以上必须换）、右键菜单、详情属性面板、五个对话框、
//! Action 与快捷键、行图标（`IconName` 子集尚未逐一核实，先不引入以免资产缺失时静默为空）。

use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::commands;
use crate::model::{ArchiveKind, ArchiveStatus};
use crate::ui;

// ==================== 视图模型（纯数据，便于单测） ====================

/// 一行存档：**宿主已格式化好的快照**（面板渲染不做计算与 I/O）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRow {
    pub id: String,
    pub name: String,
    pub kind: ArchiveKind,
    pub version: i32,
    pub status: ArchiveStatus,
    /// 尾部字段（按字段优先级规则拼好，见 [`row_tail`]）。
    pub tail: String,
}

/// 状态行计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArchiveCounts {
    pub total: usize,
    pub archived: usize,
    pub analysis: usize,
    pub table_ref: usize,
    pub missing: usize,
    pub drifted: usize,
}

impl ArchiveCounts {
    /// 状态行文案（零桶不显示）。
    pub fn line(&self) -> String {
        let mut parts = vec![
            format!("{} 项", self.total),
            format!("已归档 {}", self.archived),
        ];
        for (count, label) in [
            (self.analysis, "分析表"),
            (self.table_ref, "引用"),
            (self.missing, "缺失"),
            (self.drifted, "索引异常"),
        ] {
            if count > 0 {
                parts.push(format!("{label} {count}"));
            }
        }
        parts.join(" · ")
    }

    /// 是否有需要用户处理的异常（决定状态行是否给"修复…"入口）。
    pub fn has_issues(&self) -> bool {
        self.missing > 0 || self.drifted > 0
    }
}

/// 面板快照：宿主每次推送一整份（结构小，克隆代价可忽略）。
#[derive(Debug, Clone, Default)]
pub struct ResourcesSnapshot {
    pub rows: Vec<ArchiveRow>,
    pub counts: ArchiveCounts,
    pub read_only: bool,
}

/// 复现强度徽标文案（原型 §3.2：**行内唯一的颜色信号**）。
pub fn strength_badge(kind: ArchiveKind, status: ArchiveStatus) -> &'static str {
    match status {
        ArchiveStatus::Missing => "缺失",
        ArchiveStatus::ContentChanged => "内容已变",
        ArchiveStatus::Normal => match kind {
            ArchiveKind::File => "已归档",
            ArchiveKind::Analysis => "分析表",
            ArchiveKind::TableRef => "引用",
        },
    }
}

/// 徽标色调（纯枚举；渲染层再映射到主题色，便于单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Success,
    Info,
    Warning,
    Danger,
}

/// 徽标色调：异常态压过 kind；`引用`与 `内容已变` 同为警示。
pub fn badge_tone(kind: ArchiveKind, status: ArchiveStatus) -> BadgeTone {
    match (status, kind) {
        (ArchiveStatus::Missing, _) => BadgeTone::Danger,
        (ArchiveStatus::ContentChanged, _) => BadgeTone::Warning,
        (ArchiveStatus::Normal, ArchiveKind::File) => BadgeTone::Success,
        (ArchiveStatus::Normal, ArchiveKind::Analysis) => BadgeTone::Info,
        (ArchiveStatus::Normal, ArchiveKind::TableRef) => BadgeTone::Warning,
    }
}

/// 尾部字段：字段优先级为 显示名 > 强度徽标 > 版本徽标 > 相对时间 > 大小/规模。
///
/// 240px 面板放不下的信息一律省略号 + tooltip（渲染层补），**不缩行高、不做两行行**。
pub fn row_tail(detail: &str, modified: &str, version: i32) -> String {
    match (detail.is_empty(), modified.is_empty()) {
        (false, false) => format!("{detail} · {modified}"),
        (false, true) => detail.to_string(),
        (true, false) => modified.to_string(),
        // 两者都缺时给出版本，避免尾部整段空白。
        (true, true) => format!("v{version}"),
    }
}

// ==================== 宿主契约 ====================

/// 宿主能力：面板不认识服务层与会话，动作一律回宿主（事件路径执行）。
pub trait ResourcesHost: 'static {
    /// 归档入口（面板头「归档」）：弹对话框 / 选文件由宿主负责。
    fn request_archive(&self);
    /// 打开（只读）。
    fn request_open(&self, resource_id: &str);
    /// 取回（检出）。
    fn request_checkout(&self, resource_id: &str);
    /// 移入回收站。
    fn request_delete(&self, resource_id: &str);
    /// 索引修复入口（状态行异常段）。
    fn request_index_repair(&self);
}

// ==================== 面板 ====================

pub struct ResourcesPanel {
    host: Rc<dyn ResourcesHost>,
    snapshot: ResourcesSnapshot,
    selected: Option<String>,
    notice: Option<String>,
    focus_handle: FocusHandle,
    group: Option<WeakEntity<TabGroup>>,
}

impl ResourcesPanel {
    pub const PANEL_NAME: &'static str = "analytics_resource";

    pub fn new(host: Rc<dyn ResourcesHost>, cx: &mut Context<Self>) -> Self {
        Self {
            host,
            snapshot: ResourcesSnapshot::default(),
            selected: None,
            notice: None,
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }

    /// 宿主推送数据（事件路径调用；面板不自己取数）。
    pub fn set_snapshot(&mut self, snapshot: ResourcesSnapshot, cx: &mut Context<Self>) {
        self.snapshot = snapshot;
        // 选中项可能已被删除/过滤掉：清掉悬空选中，避免详情面板指向不存在的东西。
        if let Some(selected) = self.selected.as_deref() {
            if !self.snapshot.rows.iter().any(|row| row.id == selected) {
                self.selected = None;
            }
        }
        cx.notify();
    }

    pub fn set_notice(&mut self, notice: Option<String>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn snapshot(&self) -> &ResourcesSnapshot {
        &self.snapshot
    }

    fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.set_selected(id, cx);
    }

    /// 宿主驱动选中（生产入口；面板内点击也走它）。
    pub fn set_selected(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected = id;
        cx.notify();
    }

    // ==================== 区域渲染 ====================
    //
    // 返回值写具体类型（`Div` / `AnyElement`）：edition 2024 的 RPIT 会捕获入参生命周期，
    // 返回 `impl IntoElement` 时它会借住 `cx`，连续调两个区域函数就变成"重复可变借用"。

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().colors.border;
        let read_only = self.snapshot.read_only;
        let host = self.host.clone();

        div()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .flex_none()
            .h_flex()
            .justify_between()
            .px_2()
            // 1px 固定描边：不随字号缩放（允许的 physical boundary 例外）。
            .border_b(px(1.0))
            .border_color(border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("资产库"),
            )
            .child(
                Button::new("archive-add")
                    .ghost()
                    .label("归档")
                    .disabled(read_only)
                    .on_click(move |_, _, _| host.request_archive()),
            )
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.clone()?;
        let (border, warning, info) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.info)
        };
        // 只读是约束提示（warning），其余是信息（info）——同一字段不一律同色。
        let color = if self.snapshot.read_only { warning } else { info };
        Some(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .border_b(px(1.0))
                .border_color(border)
                .text_color(color)
                .child(notice)
                .into_any_element(),
        )
    }

    /// 滚动容器不是 `Div`（`overflow_y_scrollbar` 返回滚动包装类型），故本区域返回 `AnyElement`。
    fn render_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        // 颜色先拷出（Hsla 是 Copy）：循环体内不再碰 `cx`。
        let (foreground, muted, success, info, warning, danger) = {
            let colors = cx.theme().colors;
            (
                colors.foreground,
                colors.muted_foreground,
                colors.success,
                colors.info,
                colors.warning,
                colors.danger,
            )
        };

        let mut list = div().v_flex().w_full().gap_0p5().px_1().py_1();
        let panel = cx.weak_entity();
        let host_for_rows = self.host.clone();

        for row in &self.snapshot.rows {
            let is_selected = self.selected.as_deref() == Some(row.id.as_str());
            let badge = strength_badge(row.kind, row.status);
            let badge_color = match badge_tone(row.kind, row.status) {
                BadgeTone::Success => success,
                BadgeTone::Info => info,
                BadgeTone::Warning => warning,
                BadgeTone::Danger => danger,
            };
            let name_color = if row.status == ArchiveStatus::Missing {
                muted
            } else {
                foreground
            };
            let row_id = row.id.clone();
            let entry_id = SharedString::from(format!("archive-row-{}", row.id));
            let click_panel = panel.clone();
            let tail = row.tail.clone();
            let version = row.version;
            let name = row.name.clone();

            list = list.child(
                Button::new(entry_id)
                    .ghost()
                    .w_full()
                    .h(rems(ui::ROW_HEIGHT))
                    .toggled(is_selected)
                    .on_click(move |_, _, cx| {
                        let id = row_id.clone();
                        let _ = click_panel.update(cx, |panel, cx| panel.select(Some(id), cx));
                    })
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_ellipsis()
                                    .text_color(name_color)
                                    .child(name),
                            )
                            // v1 不显示版本徽标（减少噪声）。
                            .when(version > 1, |line| {
                                line.child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("v{version}")),
                                )
                            })
                            .child(
                                div()
                                    .h(rems(ui::ARCHIVE_BADGE_HEIGHT))
                                    .text_xs()
                                    .text_color(badge_color)
                                    .child(badge),
                            )
                            .when(!tail.is_empty(), |line| {
                                line.child(div().text_xs().text_color(muted).child(tail.clone()))
                            })
                            // 行级操作只在**选中行**上出现（hover 版本随菜单批一起做）：
                            // 与 M4 连接行 / M5 草稿行的行内操作同一惯例，不占默认行宽。
                            .when(is_selected, |line| {
                                line.child(
                                    Button::new(SharedString::from(format!(
                                        "archive-open-{}",
                                        row.id
                                    )))
                                    .ghost()
                                    .label("打开")
                                    .on_click({
                                        let open_host = host_for_rows.clone();
                                        let id = row.id.clone();
                                        move |_, _, _| open_host.request_open(&id)
                                    }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "archive-checkout-{}",
                                        row.id
                                    )))
                                    .ghost()
                                    .label("取回")
                                    .disabled(row.status != ArchiveStatus::Normal)
                                    .on_click({
                                        let checkout_host = host_for_rows.clone();
                                        let id = row.id.clone();
                                        move |_, _, _| checkout_host.request_checkout(&id)
                                    }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "archive-delete-{}",
                                        row.id
                                    )))
                                    .ghost()
                                    .label("移入回收站")
                                    .on_click({
                                        let delete_host = host_for_rows.clone();
                                        let id = row.id.clone();
                                        move |_, _, _| delete_host.request_delete(&id)
                                    }),
                                )
                            }),
                    ),
            );
        }

        div()
            .id("archive-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(list)
            .into_any_element()
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> Div {
        let (border, warning, muted) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.muted_foreground)
        };
        let counts = self.snapshot.counts;
        let has_issues = counts.has_issues();
        let host = self.host.clone();

        div()
            .flex_none()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .justify_between()
            .px_2()
            .border_t(px(1.0))
            .border_color(border)
            .child(
                div()
                    .text_xs()
                    .text_color(if has_issues { warning } else { muted })
                    .child(counts.line()),
            )
            .when(has_issues, |bar| {
                bar.child(
                    Button::new("index-repair")
                        .ghost()
                        .label("修复…")
                        .on_click(move |_, _, _| host.request_index_repair()),
                )
            })
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let read_only = self.snapshot.read_only;
        let host = self.host.clone();

        div()
            .flex_1()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("还没有任何存档"),
            )
            .child(
                div()
                    .px_4()
                    .text_xs()
                    .text_color(muted)
                    .child("把草稿箱里值得留存的文件归档进来，它就变成只读、带版本、可追溯的存档"),
            )
            .child(
                Button::new("archive-for-empty")
                    .primary()
                    .label("从草稿箱归档…")
                    .disabled(read_only)
                    .on_click(move |_, _, _| host.request_archive()),
            )
    }
}

// ==================== 面板协议 ====================

impl EventEmitter<PanelEvent> for ResourcesPanel {}

impl Focusable for ResourcesPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ResourcesPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }
}

impl ComponentPanel for ResourcesPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().child("资产库")
    }
}

impl Render for ResourcesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().colors.background;
        // 各区域函数返回具体类型（见上文注释），故可依次调用、各自持有已完成的元素。
        let header = self.render_header(cx);
        let notice = self.render_notice(cx);
        let body = if self.snapshot.rows.is_empty() {
            self.render_empty(cx).into_any_element()
        } else {
            self.render_rows(cx)
        };
        let status = self.render_status_bar(cx);
        let host = self.host.clone();

        div()
            .id("analytics-resource-panel")
            // 键盘上下文：快捷键由 app 层绑到 `analytics-resource` context（视图不自注全局键）。
            .key_context("analytics-resource")
            .on_action(cx.listener({
                let host = host.clone();
                move |_: &mut Self, _: &commands::RequestArchive, _window, _cx| {
                    host.request_archive()
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::OpenSelected, _window, _cx| {
                    if let Some(id) = panel.selected.clone() {
                        host.request_open(&id);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::CheckoutSelected, _window, _cx| {
                    if let Some(id) = panel.selected.clone() {
                        host.request_checkout(&id);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::DeleteSelected, _window, _cx| {
                    if let Some(id) = panel.selected.clone() {
                        host.request_delete(&id);
                    }
                }
            }))
            .v_flex()
            .size_full()
            // 面板自己拥有滚动（Dock 的 tab-content 不产生滚动）：滚动挂在 body 上。
            .min_h_0()
            .bg(background)
            .child(header)
            .when_some(notice, |panel, notice| panel.child(notice))
            .child(body)
            .child(status)
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入（会与 `#[gpui_kit::test]` 展开的 `#[test]` 自相残杀）。
    use super::{ArchiveCounts, BadgeTone, badge_tone, row_tail, strength_badge};
    use crate::model::{ArchiveKind, ArchiveStatus};

    #[test]
    fn strength_badge_follows_kind_and_status() {
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::Normal),
            "已归档"
        );
        assert_eq!(
            strength_badge(ArchiveKind::Analysis, ArchiveStatus::Normal),
            "分析表"
        );
        assert_eq!(
            strength_badge(ArchiveKind::TableRef, ArchiveStatus::Normal),
            "引用"
        );
        // 异常态压过 kind。
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::Missing),
            "缺失"
        );
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::ContentChanged),
            "内容已变"
        );
    }

    #[test]
    fn badge_tone_marks_weak_strength_as_warning() {
        assert_eq!(
            badge_tone(ArchiveKind::File, ArchiveStatus::Normal),
            BadgeTone::Success
        );
        assert_eq!(
            badge_tone(ArchiveKind::Analysis, ArchiveStatus::Normal),
            BadgeTone::Info
        );
        // `引用`复现强度最弱：必须显眼（warning），不能与"已归档"同为成功色。
        assert_eq!(
            badge_tone(ArchiveKind::TableRef, ArchiveStatus::Normal),
            BadgeTone::Warning
        );
        assert_eq!(
            badge_tone(ArchiveKind::File, ArchiveStatus::Missing),
            BadgeTone::Danger
        );
    }

    #[test]
    fn row_tail_prefers_detail_then_time_then_version() {
        assert_eq!(row_tail("1.2 KB", "3 天前", 3), "1.2 KB · 3 天前");
        assert_eq!(row_tail("1.2 KB", "", 3), "1.2 KB");
        assert_eq!(row_tail("", "3 天前", 3), "3 天前");
        assert_eq!(row_tail("", "", 3), "v3");
    }

    #[test]
    fn counts_line_hides_zero_buckets() {
        let clean = ArchiveCounts {
            total: 3,
            archived: 3,
            ..ArchiveCounts::default()
        };
        assert_eq!(clean.line(), "3 项 · 已归档 3");
        assert!(!clean.has_issues());

        let with_issues = ArchiveCounts {
            total: 4,
            archived: 3,
            analysis: 1,
            missing: 1,
            drifted: 2,
            ..ArchiveCounts::default()
        };
        assert_eq!(
            with_issues.line(),
            "4 项 · 已归档 3 · 分析表 1 · 缺失 1 · 索引异常 2"
        );
        assert!(with_issues.has_issues());
    }
}
