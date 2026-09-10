//! RdataStation v2 工作台视图（WorkbenchView）— v5 布局
//!
//! 按 GPUI-kit 编码规范搭建 DockArea 可拖拽布局工作台：
//! - 依赖只向下：视图只使用 gpui-kit（gpui / base / component），不承载业务逻辑；
//! - 窗口第一层由 App Shell 的 `Root` 装配；本视图负责标题栏 + 左右活动栏 + DockArea + 状态栏；
//! - 左 Dock：草稿箱 / 数据库导航 / 资源分析 / 插件；右 Dock：洞察 / Mock 生成 / 历史；
//! - 边栏三模式：展开 / 收起（`toggle_dock`）/ 完全隐藏（`remove_dock`，gpui-kit 0.6）；
//!   模式状态存于 `Shared`，`render` 统一同步到 Dock（权威同步点）；
//! - Quick Open：搜索 + 命令融合（标题栏居中入口 / Ctrl+P），受控自绘弹层；
//! - 颜色一律取自 `cx.theme().colors`（禁止 raw hex/rgb）。

use std::path::Path;

use gpui_kit::base::{Selectable, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{panel_handle, DockArea, DockLayout, DockPlacement, DockSkin};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Root, TitleBar};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::commands::{HideSidebars, RestoreSidebars, ToggleQuickOpen};
use crate::panels::{EditorPanel, RightSidebarPanel, Shared, SidebarEvent, SidebarPanel};
use settings::commands::OpenSettings;
use settings::settings_view::SettingsView;

/// 左侧活动栏面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftPanel {
    /// 草稿箱（M5 草稿能力占位）
    Draft,
    /// 数据库导航（M4，数据源连接 + 对象树）
    Database,
    /// 资源分析（M6）
    Resources,
    /// 插件（M9）
    Plugin,
}

/// 右侧活动栏面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    /// 洞察（M8）
    Insight,
    /// Mock 数据生成（M7）
    Mock,
    /// 历史（查询历史）
    History,
}

/// 边栏三模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    /// 展开：活动栏 + 边栏可见
    Expanded,
    /// 收起：边栏隐藏、活动栏保留（dock 保留、离屏）
    Collapsed,
    /// 完全隐藏：活动栏 + 边栏均隐藏（dock 移除）
    Hidden,
}

impl LeftPanel {
    pub const ALL: [LeftPanel; 4] = [
        LeftPanel::Draft,
        LeftPanel::Database,
        LeftPanel::Resources,
        LeftPanel::Plugin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftPanel::Draft => "草稿箱",
            LeftPanel::Database => "数据库导航",
            LeftPanel::Resources => "资源分析",
            LeftPanel::Plugin => "插件",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            LeftPanel::Draft => IconName::FileText,
            LeftPanel::Database => IconName::HardDrive,
            LeftPanel::Resources => IconName::FolderOpen,
            LeftPanel::Plugin => IconName::Bot,
        }
    }
}

impl RightPanel {
    pub const ALL: [RightPanel; 3] = [RightPanel::Insight, RightPanel::Mock, RightPanel::History];

    pub fn label(self) -> &'static str {
        match self {
            RightPanel::Insight => "洞察",
            RightPanel::Mock => "Mock 生成",
            RightPanel::History => "历史",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            RightPanel::Insight => IconName::Eye,
            RightPanel::Mock => IconName::RotateCw,
            RightPanel::History => IconName::Undo,
        }
    }
}

/// 连接条目（Round 21 起由全局系统库真实数据填充；Round 22 扩展完整元数据）。
#[derive(Debug, Clone)]
pub struct ConnectionItem {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub connected: bool,
    /// 真实元数据（Round 22）：主机 / 端口 / 数据库 / Schema。
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub description: Option<String>,
    /// DuckDB 联邦（本地加速）开关。
    pub use_duckdb_fed: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 工作台视图：标题栏 + 左右活动栏 + DockArea（左/右/中央）+ 状态栏。
pub struct WorkbenchView {
    shared: Shared,
    /// DockArea 实体（render 首次调用时懒初始化，需要 &mut Window）。
    area: Option<Entity<DockArea>>,
    sidebar: Option<Entity<SidebarPanel>>,
    editor: Option<Entity<EditorPanel>>,
    right_sidebar: Option<Entity<RightSidebarPanel>>,
    /// Quick Open 输入状态（render 首次懒创建）。
    quick_open_input: Option<Entity<InputState>>,
    /// 设置面板实体（首次打开时懒创建）。
    settings_view: Option<Entity<SettingsView>>,
    /// 订阅句柄（保持连接选中事件的订阅存活）。
    _subscription: Option<Subscription>,
}

impl WorkbenchView {
    pub fn new() -> Self {
        // Round 21：从全局系统库加载真实连接（失败降级为空列表 + 提示）。
        let (connections, notice) = crate::services::workspace_loader::load_persisted_connections();
        Self {
            shared: Shared::with_connections(connections, notice),
            area: None,
            sidebar: None,
            editor: None,
            right_sidebar: None,
            quick_open_input: None,
            settings_view: None,
            _subscription: None,
        }
    }

    /// 首次 render 时装配 DockArea：创建面板实体、订阅事件；左右 dock 由
    /// `apply_left_mode` / `apply_right_mode` 按 `Shared` 初始状态装配。
    fn init_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        let sidebar = cx.new(|cx| SidebarPanel::new(shared.clone(), cx));
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let right_sidebar = cx.new(|cx| RightSidebarPanel::new(shared.clone(), cx));

        // 订阅侧边栏事件：连接选中 -> 更新共享状态并重绘编辑器。
        let subscription = cx.subscribe(&sidebar, |this, _entity, event: &SidebarEvent, cx| {
            match event {
                SidebarEvent::SelectConnection(idx) => {
                    this.shared.selected.set(Some(*idx));
                    // Round 30：切换连接 → 清空导航树 / SQL 结果残留，防止串数据。
                    *this.shared.nav_for.borrow_mut() = None;
                    this.shared.nav_tables.borrow_mut().clear();
                    *this.shared.sql_for.borrow_mut() = None;
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
                SidebarEvent::EditConnection(_) => {
                    // 编辑请求已写入 shared.open_edit；通知编辑器渲染消费并打开对话框。
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
            }
            cx.notify();
        });

        let (area, _skin) = DockSkin::dock_area("workspace", Some(1), window, cx);
        let editor_handle = panel_handle(editor.clone());
        area.update(cx, |area, cx| {
            // 中央编辑区单独占满（左右 dock 独立装配，不用 h_split）。
            area.set_center(DockLayout::tabs().panel_view(editor_handle, cx), window, cx);
        });

        self._subscription = Some(subscription);
        self.area = Some(area);
        self.sidebar = Some(sidebar);
        self.editor = Some(editor);
        self.right_sidebar = Some(right_sidebar);
    }

    // ===== 三模式：Shared 状态 → Dock 同步（render 权威） =====

    fn apply_left_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.left_mode.get();
        let (Some(area), Some(sidebar)) = (&self.area, &self.sidebar) else {
            return;
        };
        area.update(cx, |area, cx| match mode {
            SidebarMode::Expanded => {
                if !area.has_dock(DockPlacement::Left) {
                    let handle = panel_handle(sidebar.clone());
                    area.set_dock(
                        DockPlacement::Left,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
                    // 起步宽度 240px（layout-design §2.3；用户拖拽可调，dock 重建时恢复）。
                    area.set_dock_size(DockPlacement::Left, px(240.), window, cx);
                } else if !area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                }
            }
            SidebarMode::Collapsed => {
                if !area.has_dock(DockPlacement::Left) {
                    let handle = panel_handle(sidebar.clone());
                    area.set_dock(
                        DockPlacement::Left,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
                    area.set_dock_size(DockPlacement::Left, px(240.), window, cx);
                }
                if area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                }
            }
            SidebarMode::Hidden => {
                if area.has_dock(DockPlacement::Left) {
                    area.remove_dock(DockPlacement::Left, window, cx);
                }
            }
        });
    }

    fn apply_right_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.right_mode.get();
        let (Some(area), Some(right)) = (&self.area, &self.right_sidebar) else {
            return;
        };
        area.update(cx, |area, cx| match mode {
            SidebarMode::Expanded => {
                if !area.has_dock(DockPlacement::Right) {
                    let handle = panel_handle(right.clone());
                    area.set_dock(
                        DockPlacement::Right,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
                    // 起步宽度 280px（layout-design §2.3）。
                    area.set_dock_size(DockPlacement::Right, px(280.), window, cx);
                } else if !area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
            }
            SidebarMode::Collapsed => {
                if !area.has_dock(DockPlacement::Right) {
                    let handle = panel_handle(right.clone());
                    area.set_dock(
                        DockPlacement::Right,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
                    area.set_dock_size(DockPlacement::Right, px(280.), window, cx);
                }
                if area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
            }
            SidebarMode::Hidden => {
                if area.has_dock(DockPlacement::Right) {
                    area.remove_dock(DockPlacement::Right, window, cx);
                }
            }
        });
    }

    // ===== 标题栏 =====
    // 差异点：无窗口标题文本。承载 gpui-kit 官方 TitleBar（对应 layout-design.md §2.1）：
    // - 窗口拖拽（Drag hitbox，Windows 系统级）、双击最大化、窗口控制按钮（─ □ ✕）由组件自带；
    // - 高度覆盖为 36px、背景覆盖为主题 title_bar 纯色（对齐 layout-proposal.html v5）。
    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let shared = self.shared.clone();
        let entity = cx.entity();

        // 软件图标（明亮版；暗黑版未设计，dark 主题先复用，见 theme-design.md）。
        let icon_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/32x32.png");
        let logo = img(icon_path.as_path()).w(px(20.)).h(px(20.)).rounded_sm();

        // 挖空项目槽：标题栏背景深一档。设计语义 token 为 `title_bar.slot.background`
        // （dark #252526 / light #F3F3F3），语义 token 注册落地前以 sidebar 角色同值替代。
        let slot = div()
            .h_flex()
            .items_center()
            .gap_2()
            .h(px(26.))
            .px(px(14.))
            .rounded_md()
            .bg(theme.colors.sidebar)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("项目"),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child("营销分析"),
            );

        // Quick Open 入口（点击唤起，Ctrl+P 见 commands.rs 绑定）。
        // 320×26 居中；底色取 border 角色（dark #3C3C3C，与示意 v5 一致）。
        let qo_shared = shared.clone();
        let qo_entity = entity.clone();
        let quick_open = div()
            .id("quick-open-entry")
            .h_flex()
            .items_center()
            .gap_2()
            .w(px(320.))
            .h(px(26.))
            .px(px(10.))
            .rounded_sm()
            .bg(theme.colors.border)
            .cursor_pointer()
            .child(Icon::new(IconName::Search).size(px(14.)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("搜索或输入命令…"),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("Ctrl+P"),
            )
            .on_click(move |_, _, app| {
                qo_shared.quick_open.set(true);
                qo_entity.update(app, |_, cx| cx.notify());
            });

        // 三栏布局：左右 flex_1 占位对称，Quick Open 严格居中；
        // 右侧窗口控制按钮（─ □ ✕）由 TitleBar 自带渲染，无需自绘。
        TitleBar::new()
            .h(px(36.))
            .pl(px(10.))
            .bg(theme.colors.title_bar)
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(logo)
                    .child(slot),
            )
            .child(quick_open)
            .child(div().flex_1())
    }

    // ===== 活动栏 =====

    fn render_left_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active = self.shared.active_left.get();
        let mut bar = div()
            .v_flex()
            .items_center()
            .w(px(48.))
            .h_full()
            .pt(px(8.))
            .pb(px(8.))
            .gap_1()
            .border_r_1()
            .border_color(theme.colors.border)
            // 活动栏背景（产品语义 token activity_bar.background，dark #333333；
            // 语义 token 注册落地前以 secondary 角色同值替代，见 theme-design §5.4）。
            .bg(theme.colors.secondary);

        for panel in LeftPanel::ALL {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let selected = active == panel;
            // VSCode 行为：激活项左侧 2px 亮条 + 亮色图标（activity_bar.active_border，
            // 取 sidebar_accent_foreground 同值替代；右侧活动栏镜像在右）。
            bar = bar.child(
                div()
                    .w(px(44.))
                    .h(px(40.))
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .border_l_2()
                    .border_color(if selected {
                        theme.colors.sidebar_accent_foreground
                    } else {
                        transparent_black()
                    })
                    .child(
                        Button::new(format!("left-activity-{}", panel.label()))
                            .icon(panel.icon())
                            .size(px(28.))
                            .ghost()
                            .selected(selected)
                            .toggled(selected)
                            .on_click(move |_, _, app| {
                                let mode = shared.left_mode.get();
                                if mode == SidebarMode::Expanded
                                    && shared.active_left.get() == panel
                                {
                                    // 再次点击当前激活项 → 收起。
                                    shared.left_mode.set(SidebarMode::Collapsed);
                                } else {
                                    shared.active_left.set(panel);
                                    shared.left_mode.set(SidebarMode::Expanded);
                                }
                                entity.update(app, |_, cx| cx.notify());
                            }),
                    ),
            );
        }

        // 底部：弹性占位 + 分隔线 + 设置入口（Step 5 接 crates/settings）。
        let entity = cx.entity();
        let shared = self.shared.clone();
        bar = bar
            .child(div().flex_1())
            .child(div().w(px(28.)).h(px(1.)).bg(theme.colors.border))
            .child(
                Button::new("left-settings")
                    .icon(IconName::Settings)
                    .size(px(28.))
                    .ghost()
                    .on_click(move |_, _, app| {
                        shared.settings_open.set(true);
                        entity.update(app, |_, cx| cx.notify());
                    }),
            );
        bar
    }

    fn render_right_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active = self.shared.active_right.get();
        let mut bar = div()
            .v_flex()
            .items_center()
            .w(px(48.))
            .h_full()
            .pt(px(8.))
            .pb(px(8.))
            .gap_1()
            .border_l_1()
            .border_color(theme.colors.border)
            .bg(theme.colors.secondary);

        for panel in RightPanel::ALL {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let selected = active == panel;
            // 与左侧镜像：激活项 2px 亮条在右。
            bar = bar.child(
                div()
                    .w(px(44.))
                    .h(px(40.))
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .border_r_2()
                    .border_color(if selected {
                        theme.colors.sidebar_accent_foreground
                    } else {
                        transparent_black()
                    })
                    .child(
                        Button::new(format!("right-activity-{}", panel.label()))
                            .icon(panel.icon())
                            .size(px(28.))
                            .ghost()
                            .selected(selected)
                            .toggled(selected)
                            .on_click(move |_, _, app| {
                                let mode = shared.right_mode.get();
                                if mode == SidebarMode::Expanded
                                    && shared.active_right.get() == panel
                                {
                                    shared.right_mode.set(SidebarMode::Collapsed);
                                } else {
                                    shared.active_right.set(panel);
                                    shared.right_mode.set(SidebarMode::Expanded);
                                }
                                entity.update(app, |_, cx| cx.notify());
                            }),
                    ),
            );
        }

        let entity = cx.entity();
        let shared = self.shared.clone();
        bar = bar
            .child(div().flex_1())
            .child(div().w(px(28.)).h(px(1.)).bg(theme.colors.border))
            .child(
                Button::new("right-settings")
                    .icon(IconName::Settings)
                    .size(px(28.))
                    .ghost()
                    .on_click(move |_, _, app| {
                        shared.settings_open.set(true);
                        entity.update(app, |_, cx| cx.notify());
                    }),
            );
        bar
    }

    /// 设置面板 overlay：居中渲染 SettingsView（懒创建实体）。
    /// 关闭按钮由 SettingsView 内部回调 Shared.settings_open。
    fn render_settings_panel(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.shared.settings_open.get() {
            return None;
        }
        if self.settings_view.is_none() {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let on_close: std::rc::Rc<dyn Fn(&mut App)> = std::rc::Rc::new(move |app| {
                shared.settings_open.set(false);
                entity.update(app, |_, cx| cx.notify());
            });
            let on_close = on_close.clone();
            self.settings_view = Some(cx.new(move |cx| SettingsView::new(cx, on_close)));
        }
        let theme = cx.theme().clone();
        let view = self.settings_view.clone().expect("settings initialized");
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.colors.overlay)
                .child(view),
        )
    }

    // ===== Quick Open（搜索 + 命令融合） =====

    fn render_quick_open(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.shared.quick_open.get() {
            return None;
        }
        let theme = cx.theme().clone();
        let input = self.quick_open_input.clone().expect("lazy init");
        let shared = self.shared.clone();
        let entity = cx.entity();
        let shared_close = shared.clone();
        let entity_close = entity.clone();

        let list = quick_open_results(&input, shared, entity, cx);

        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .bg(theme.colors.overlay)
                .child(
                    div()
                        // 示意 v5：面板水平居中、顶部距标题栏 42px。
                        .mt(px(42.))
                        .w(px(560.))
                        .max_h(px(420.))
                        .v_flex()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.colors.border)
                        .bg(theme.colors.popover)
                        .shadow_lg()
                        .child(Input::new(&input))
                        .child(list.overflow_y_scrollbar()),
                )
                .on_mouse_down(MouseButton::Left, move |_, _, app| {
                    shared_close.quick_open.set(false);
                    entity_close.update(app, |_, cx| cx.notify());
                }),
        )
    }

    // ===== 状态栏 =====

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = cx.entity();
        let shared = self.shared.clone();
        let selected = self
            .shared
            .selected_connection()
            .map(|c| c.name)
            .unwrap_or_else(|| "未选择连接".to_string());
        let left_label = self.shared.active_left.get().label();
        let right_label = self.shared.active_right.get().label();
        let all_hidden = self.shared.left_mode.get() == SidebarMode::Hidden
            && self.shared.right_mode.get() == SidebarMode::Hidden;

        // 状态栏背景品牌珊瑚色（theme.colors.primary 派生，见 theme-design §5.4），文字取配对前景色。
        StatusBar::new()
            .bg(theme.colors.primary)
            .text_color(theme.colors.primary_foreground)
            .left(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        Button::new("hide-sidebars")
                            .icon(IconName::PanelLeftClose)
                            .size(px(18.))
                            .ghost()
                            .text_color(theme.colors.primary_foreground)
                            .label(if all_hidden {
                                "« 恢复"
                            } else {
                                "« 完全隐藏"
                            })
                            .on_click(move |_, _, app| {
                                if all_hidden {
                                    shared.left_mode.set(SidebarMode::Expanded);
                                    shared.right_mode.set(SidebarMode::Expanded);
                                } else {
                                    shared.left_mode.set(SidebarMode::Hidden);
                                    shared.right_mode.set(SidebarMode::Hidden);
                                }
                                entity.update(app, |_, cx| cx.notify());
                            }),
                    )
                    .child(div().child(format!("左：{left_label} · 右：{right_label}"))),
            )
            .right(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(div().child(format!("连接：{selected}")))
                    .child(div().child("DuckDB 就绪"))
                    .child(div().child("UTF-8")),
            )
    }
}

impl EventEmitter<SidebarEvent> for WorkbenchView {}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {
            self.init_workspace(window, cx);
        }
        if self.quick_open_input.is_none() {
            self.quick_open_input = Some(cx.new(|cx| InputState::new(window, cx)));
        }
        // 三模式权威同步点：Shared 状态 → Dock。
        self.apply_left_mode(window, cx);
        self.apply_right_mode(window, cx);

        let area = self.area.clone().expect("workspace initialized");
        let left_bar = (self.shared.left_mode.get() != SidebarMode::Hidden)
            .then(|| self.render_left_activity_bar(cx));
        let right_bar = (self.shared.right_mode.get() != SidebarMode::Hidden)
            .then(|| self.render_right_activity_bar(cx));
        let quick_open = self.render_quick_open(cx);
        let settings_panel = self.render_settings_panel(cx);

        let mut root = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .relative()
            .key_context("workbench")
            .on_action({
                let entity = cx.entity();
                move |_: &ToggleQuickOpen, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let open = this.shared.quick_open.get();
                        this.shared.quick_open.set(!open);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &HideSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.shared.left_mode.set(SidebarMode::Hidden);
                        this.shared.right_mode.set(SidebarMode::Hidden);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &RestoreSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.shared.left_mode.set(SidebarMode::Expanded);
                        this.shared.right_mode.set(SidebarMode::Expanded);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &OpenSettings, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let open = this.shared.settings_open.get();
                        this.shared.settings_open.set(!open);
                        cx.notify();
                    });
                }
            })
            .child(self.render_title_bar(window, cx))
            .child(
                div()
                    .h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .when_some(left_bar, |this, bar| this.child(bar))
                    .child(area)
                    .when_some(right_bar, |this, bar| this.child(bar)),
            )
            .child(self.render_status_bar(cx));

        if let Some(qo) = quick_open {
            root = root.child(qo);
        }
        if let Some(sp) = settings_panel {
            root = root.child(sp);
        }
        // Phase A：对话框层（Root::render_dialog_layer）——"新建连接"模态框在此渲染。
        if let Some(dialog_layer) = Root::render_dialog_layer(window, cx) {
            root = root.child(dialog_layer);
        }
        root
    }
}

/// Quick Open 结果列表：命令组 + 资源组（连接 / 分析表），按输入过滤。
fn quick_open_results(
    input: &Entity<InputState>,
    shared: Shared,
    entity: Entity<WorkbenchView>,
    cx: &mut App,
) -> Div {
    let theme = cx.theme();
    let query = input.read(cx).value().to_string();
    let commands_only = query.trim_start().starts_with('>');
    let needle = if commands_only {
        query.trim_start_matches('>').trim().to_lowercase()
    } else {
        query.trim().to_lowercase()
    };
    let matches = |s: &str| needle.is_empty() || s.to_lowercase().contains(&needle);

    let mut list = div().v_flex().gap_1().mt(px(2.)).max_h(px(340.));

    // ---- 命令组 ----
    let mut cmd_group = div().v_flex().gap_1();
    cmd_group = cmd_group.child(
        div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.colors.muted_foreground)
            .child("命令"),
    );
    let commands: &[(&str, QuickOpenCommand)] = &[
        ("打开草稿箱", QuickOpenCommand::OpenDraft),
        ("打开数据库导航", QuickOpenCommand::OpenDatabase),
        ("打开资源分析", QuickOpenCommand::OpenResources),
        ("打开插件", QuickOpenCommand::OpenPlugin),
        ("打开洞察", QuickOpenCommand::OpenInsight),
        ("打开 Mock 生成", QuickOpenCommand::OpenMock),
        ("打开历史", QuickOpenCommand::OpenHistory),
        ("打开设置", QuickOpenCommand::OpenSettings),
        ("完全隐藏侧边栏", QuickOpenCommand::HideSidebars),
        ("恢复侧边栏", QuickOpenCommand::RestoreSidebars),
    ];
    let mut has_cmd = false;
    for (label, cmd) in commands {
        if !matches(label) {
            continue;
        }
        has_cmd = true;
        let shared = shared.clone();
        let entity = entity.clone();
        let cmd = *cmd;
        cmd_group = cmd_group.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "qo-cmd-{label}"
                ))))
                .h(px(28.))
                .pl(px(10.))
                .pr(px(10.))
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.foreground)
                .on_mouse_down(MouseButton::Left, move |_, _, app| {
                    run_quick_command(cmd, &shared, &entity, app);
                })
                .child(*label),
        );
    }
    if has_cmd {
        list = list.child(cmd_group);
    }

    // ---- 资源组：连接 + 分析表（仅非 > 前缀时展示） ----
    if !commands_only {
        let mut res_group = div().v_flex().gap_1();
        res_group = res_group.child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.colors.muted_foreground)
                .child("文件 / 表"),
        );
        let conns: Vec<(String, String)> = shared
            .connections
            .borrow()
            .iter()
            .map(|c| (c.name.clone(), c.driver.clone()))
            .collect();
        let mut has_res = false;
        for (idx, (name, driver)) in conns.iter().enumerate() {
            if !matches(name) {
                continue;
            }
            has_res = true;
            let shared = shared.clone();
            let entity = entity.clone();
            let name = name.clone();
            let driver = driver.clone();
            res_group = res_group.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "qo-conn-{idx}"
                    ))))
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(10.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .on_mouse_down(MouseButton::Left, {
                        let name = name.clone();
                        let driver = driver.clone();
                        let shared = shared.clone();
                        let entity = entity.clone();
                        move |_, _, app| {
                            select_connection(idx, &name, &driver, &shared, &entity, app);
                        }
                    })
                    .child(format!("{name}  ·  {driver}")),
            );
        }
        let tables: Vec<String> = shared
            .nav_tables
            .borrow()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        for t in tables {
            if !matches(&t) {
                continue;
            }
            has_res = true;
            res_group = res_group.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!("qo-table-{t}"))))
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(10.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("表：{t}")),
            );
        }
        if has_res {
            list = list.child(res_group);
        }
        if !has_res && !has_cmd {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("无匹配结果"),
            );
        }
    } else if !has_cmd {
        list = list.child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("无匹配命令"),
        );
    }
    list
}

/// Quick Open 命令集合。
#[derive(Debug, Clone, Copy)]
enum QuickOpenCommand {
    OpenDraft,
    OpenDatabase,
    OpenResources,
    OpenPlugin,
    OpenInsight,
    OpenMock,
    OpenHistory,
    OpenSettings,
    HideSidebars,
    RestoreSidebars,
}

/// 执行 Quick Open 命令：更新 Shared 状态 + 关闭弹层 + notify（Dock 由 render 同步）。
fn run_quick_command(
    cmd: QuickOpenCommand,
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    match cmd {
        QuickOpenCommand::OpenDraft => {
            shared.active_left.set(LeftPanel::Draft);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenDatabase => {
            shared.active_left.set(LeftPanel::Database);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenResources => {
            shared.active_left.set(LeftPanel::Resources);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenPlugin => {
            shared.active_left.set(LeftPanel::Plugin);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenInsight => {
            shared.active_right.set(RightPanel::Insight);
            shared.right_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenMock => {
            shared.active_right.set(RightPanel::Mock);
            shared.right_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenHistory => {
            shared.active_right.set(RightPanel::History);
            shared.right_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenSettings => {
            shared.settings_open.set(true);
        }
        QuickOpenCommand::HideSidebars => {
            shared.left_mode.set(SidebarMode::Hidden);
            shared.right_mode.set(SidebarMode::Hidden);
        }
        QuickOpenCommand::RestoreSidebars => {
            shared.left_mode.set(SidebarMode::Expanded);
            shared.right_mode.set(SidebarMode::Expanded);
        }
    }
    shared.quick_open.set(false);
    entity.update(cx, |_, cx| cx.notify());
}

/// Quick Open 选中连接：与侧边栏点击一致（清空导航 / SQL 残留，防止串数据）。
fn select_connection(
    idx: usize,
    _name: &str,
    _driver: &str,
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    shared.selected.set(Some(idx));
    *shared.nav_for.borrow_mut() = None;
    shared.nav_tables.borrow_mut().clear();
    *shared.sql_for.borrow_mut() = None;
    shared.quick_open.set(false);
    entity.update(cx, |_, cx| cx.notify());
}
