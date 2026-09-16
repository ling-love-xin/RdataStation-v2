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
//! # 本批范围（Phase 1 第三刀）
//!
//! 已落地：面板头（标题 + 归档入口）、**工具栏（搜索框 / 筛选 / 排序）**、提示行（只读与通知分色）、
//! 行列表（显示名 / 版本徽标 / **复现强度徽标** / 尾部字段 / 选中态）、底部状态行（异常时给"修复…"入口）、
//! 空态**与"无匹配"两态分开**、只读禁写。
//!
//! **未落地（下一批，已在开发方案留档）**：虚拟化列表（`list::List`；当前行用 `Button` 渲染，
//! 几百行以上必须换）、右键菜单、详情属性面板接入、五个对话框、Action 与快捷键（`Ctrl+F` 聚焦 /
//! `Esc` 清空在 Action 批）、行图标（`IconName` 子集尚未逐一核实，先不引入以免资产缺失时静默为空）。

use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::commands;
use crate::filter::{self, ResourcesFilter, SortField, SortOrder};
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
///
/// 每个动作都带上 `Window` / `App`：宿主侧要开对话框与只读打开（都需要窗口句柄），
/// 而面板天然持有它们（`on_click` / `on_action` 都提供）——先把上下文带出去，比将来改签名轻。
pub trait ResourcesHost: 'static {
    /// 归档入口（面板头「归档」）：弹对话框 / 选文件由宿主负责。
    fn request_archive(&self, window: &mut Window, cx: &mut App);
    /// 打开（只读）。
    fn request_open(&self, resource_id: &str, window: &mut Window, cx: &mut App);
    /// 取回（检出）。
    fn request_checkout(&self, resource_id: &str, window: &mut Window, cx: &mut App);
    /// 移入回收站。
    fn request_delete(&self, resource_id: &str, window: &mut Window, cx: &mut App);
    /// 索引修复入口（状态行异常段）。
    fn request_index_repair(&self, window: &mut Window, cx: &mut App);
}

// ==================== 面板 ====================

pub struct ResourcesPanel {
    host: Rc<dyn ResourcesHost>,
    snapshot: ResourcesSnapshot,
    /// 筛选 + 排序后的可见行。
    ///
    /// **事件路径维护**（快照推送 / 条件变更时重算），render 只读：每帧重算会在
    /// 滚动时持续分配（行集合克隆），且把"规则"搬回渲染路径。
    view_rows: Vec<ArchiveRow>,
    /// 工具栏条件（搜索词 / 种类 / 只看需处理）。
    filter: ResourcesFilter,
    sort_field: SortField,
    sort_order: SortOrder,
    /// 搜索框（**首帧渲染时创建**）。
    ///
    /// `InputState::new` 需要 `&mut Window`，而工作台侧的侧栏面板构造期拿不到窗口
    /// （`SidebarPanel::new` 无窗口参数）——懒创建是本仓库 crate 内面板的既定处理
    /// （`mock::mock_view::MockPanel` 的输入框同例）。
    search_input: Option<Entity<InputState>>,
    /// 搜索框订阅句柄（仅持有；释放即取消）。
    _search_sub: Option<Subscription>,
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
            view_rows: Vec::new(),
            filter: ResourcesFilter::default(),
            sort_field: SortField::default(),
            sort_order: SortOrder::default(),
            search_input: None,
            _search_sub: None,
            selected: None,
            notice: None,
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }

    /// 懒创建搜索框与订阅（首帧渲染时执行一次，见 `search_input` 字段注释）。
    fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索存档"));
        // 用户真在打字的那条路径：输入变化 → 条件更新 → 重算可见行。
        //
        // 注意：`InputState::set_value` **不会**发 `InputEvent::Change`（gpui-component 内部注释明说），
        // 所以程序性改词（清空筛选 / 宿主预填）必须自己同步条件（见 `set_query` / `clear_filter`），
        // 不能指望这个订阅——否则输入框里的字与实际生效的条件会静默不一致。
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // 不用 `set_query`：它会把同一个值再写回输入框（正在输入时是无谓的往返）。
                this.filter.query = emitter.read(cx).value().to_string();
                this.refresh_view_rows();
                cx.notify();
            },
        );
        self.search_input = Some(input);
        self._search_sub = Some(sub);
    }

    /// 重算可见行（筛选 → 排序）并清理悬空选中。
    ///
    /// 选中清理放在这里而不是只放在 `set_snapshot` 里：**筛选与排序也会让行消失**，
    /// 只盯着快照会留下一个指向"看不见的行"的选中态。
    fn refresh_view_rows(&mut self) {
        self.view_rows = filter::apply_view(
            &self.snapshot.rows,
            &self.filter,
            self.sort_field,
            self.sort_order,
        );
        if let Some(selected) = self.selected.as_deref() {
            if !self.view_rows.iter().any(|row| row.id == selected) {
                self.selected = None;
            }
        }
    }

    /// 宿主推送数据（事件路径调用；面板不自己取数）。
    pub fn set_snapshot(&mut self, snapshot: ResourcesSnapshot, cx: &mut Context<Self>) {
        self.snapshot = snapshot;
        self.refresh_view_rows();
        cx.notify();
    }

    /// 换一套筛选条件（新建 / 清空走这里）。
    pub fn set_filter(&mut self, filter: ResourcesFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.refresh_view_rows();
        cx.notify();
    }

    /// 设置搜索词：条件与输入框**同一次改**（见 `ensure_search_input` 里的注释）。
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.query = text.to_string();
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.refresh_view_rows();
        cx.notify();
    }

    /// 勾选 / 取消一个种类（全选会被规范化为不限，见 `ResourcesFilter::toggle_kind`）。
    pub fn toggle_kind(&mut self, kind: ArchiveKind, cx: &mut Context<Self>) {
        self.filter.toggle_kind(kind);
        self.refresh_view_rows();
        cx.notify();
    }

    /// 只看需处理的异常（缺失 / 内容已变）。
    pub fn toggle_only_issues(&mut self, cx: &mut Context<Self>) {
        self.filter.only_issues = !self.filter.only_issues;
        self.refresh_view_rows();
        cx.notify();
    }

    /// 选排序字段：同一字段再点一次翻转方向；换字段保持当前方向
    /// （沿用 M4 与 v1 `use-pagination` 的语义——用户刚调完方向再换字段，不该被重置）。
    pub fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if field == self.sort_field {
            self.sort_order = self.sort_order.flipped();
        } else {
            self.sort_field = field;
        }
        self.refresh_view_rows();
        cx.notify();
    }

    /// 清空筛选（含搜索框）：只在"没有匹配"的空态给这个入口。
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = ResourcesFilter::default();
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        self.refresh_view_rows();
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

    /// 可见行（筛选 + 排序后）：render 与测试都读它。
    pub fn view_rows(&self) -> &[ArchiveRow] {
        &self.view_rows
    }

    /// 当前筛选条件。
    pub fn filter(&self) -> &ResourcesFilter {
        &self.filter
    }

    /// 当前排序（字段 + 方向）。
    pub fn sort(&self) -> (SortField, SortOrder) {
        (self.sort_field, self.sort_order)
    }

    /// 搜索框实体（宿主接 `Ctrl+F` 聚焦 / `Esc` 清空时用，见 Action 批）。
    ///
    /// 首帧渲染前为 `None`（懒创建，见字段注释）。
    pub fn search_input(&self) -> Option<&Entity<InputState>> {
        self.search_input.as_ref()
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
                    .on_click(move |_, window, cx| host.request_archive(window, cx)),
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
        let only_issues = self.filter.only_issues;
        // 菜单里设了几个条件就标在按钮上：搜索词不计数（它在输入框里看得见）。
        let filter_label = match self.filter.menu_dims() {
            0 => "筛选 ▾".to_string(),
            dims => format!("筛选 {dims} ▾"),
        };
        let (field, order) = (self.sort_field, self.sort_order);
        let sort_label = format!("{} {} ▾", field.label(), order.arrow());

        let filter_button = Button::new("archive-filter")
            .ghost()
            .small()
            .label(filter_label)
            .dropdown_menu({
                let panel = panel.clone();
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
                    for candidate in [SortField::Name, SortField::Version] {
                        let target = panel.clone();
                        let is_current = candidate == field;
                        // 当前字段带上方向箭头：不然用户得回忆上次点的是哪个方向。
                        let text = if is_current {
                            format!("{} {}", candidate.label(), order.arrow())
                        } else {
                            candidate.label().to_string()
                        };
                        menu = menu.item(
                            PopupMenuItem::new(text)
                                .checked(is_current)
                                .on_click(move |_, _, app| {
                                    target.update(app, |panel, cx| {
                                        panel.choose_sort(candidate, cx)
                                    });
                                }),
                        );
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
            .border_b(px(1.0))
            .border_color(border)
            .child(
                // 搜索框占满除两个按钮之外的宽度（`min_w_0`：窄面板下允许被压缩）。
                div().flex_1().min_w_0().when_some(self.search_input.clone(), |row, input| {
                    row.child(
                        Input::new(&input)
                            .h(rems(ui::CONTROL_HEIGHT_SM))
                            .cleanable(true),
                    )
                }),
            )
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

        for row in &self.view_rows {
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
                                        move |_, window, cx| open_host.request_open(&id, window, cx)
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
                                        move |_, window, cx| checkout_host.request_checkout(&id, window, cx)
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
                                        move |_, window, cx| delete_host.request_delete(&id, window, cx)
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
                        .on_click(move |_, window, cx| host.request_index_repair(window, cx)),
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
                    .on_click(move |_, window, cx| host.request_archive(window, cx)),
            )
    }

    /// 搜索 / 筛选无结果：**不是空库态**（原型 §5 状态与空态矩阵）。
    ///
    /// 两态的出口才是关键区别：空库引导"归档"（没有东西可用），无匹配引导"清筛选"
    /// （东西在，只是被挡住了）。文案混用会把用户引到错误的下一步。
    fn render_no_match(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let panel = cx.entity();

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
                    .child("没有匹配的存档"),
            )
            .child(
                div()
                    .px_4()
                    .text_xs()
                    .text_color(muted)
                    .child("换个关键词，或去掉几个筛选条件"),
            )
            .child(
                Button::new("archive-clear-filter")
                    .ghost()
                    .label("清空筛选")
                    .on_click(move |_, window, app| {
                        panel.update(app, |panel, cx| panel.clear_filter(window, cx));
                    }),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 首帧创建搜索框（懒创建，见字段注释）：之后本帧即可渲染工具栏。
        self.ensure_search_input(window, cx);
        let background = cx.theme().colors.background;
        // 各区域函数返回具体类型（见上文注释），故可依次调用、各自持有已完成的元素。
        let header = self.render_header(cx);
        let toolbar = self.render_toolbar(cx);
        let notice = self.render_notice(cx);
        // 两种空：**空库**（条件为空，没有东西可用）与**无匹配**（条件非空，东西被挡住了）。
        // `ResourcesFilter::is_empty` 就是为这行判定存在的（见 `filter.rs` 模块头）。
        let body = if self.view_rows.is_empty() {
            if self.filter.is_empty() {
                self.render_empty(cx).into_any_element()
            } else {
                self.render_no_match(cx).into_any_element()
            }
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
                move |_: &mut Self, _: &commands::RequestArchive, window, cx| {
                    host.request_archive(window, cx)
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::OpenSelected, window, cx| {
                    if let Some(id) = panel.selected.clone() {
                        host.request_open(&id, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::CheckoutSelected, window, cx| {
                    if let Some(id) = panel.selected.clone() {
                        host.request_checkout(&id, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::DeleteSelected, window, cx| {
                    if let Some(id) = panel.selected.clone() {
                        host.request_delete(&id, window, cx);
                    }
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
