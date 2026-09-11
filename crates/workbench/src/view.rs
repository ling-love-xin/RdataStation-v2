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

use crate::commands::{
    CloseProject, HideSidebars, RestoreSidebars, SwitchProject, ToggleQuickOpen,
};
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

    /// 活动栏图标（Lucide）：默认图标集（`IconName`）不含这些语义图标，
    /// 应用已注册 `AllAssets`（全量目录），故按资产路径直接引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            LeftPanel::Draft => "icons/notebook-text.svg",
            LeftPanel::Database => "icons/database.svg",
            LeftPanel::Resources => "icons/chart-column.svg",
            LeftPanel::Plugin => "icons/puzzle.svg",
        };
        Icon::default().path(path)
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

    /// 活动栏图标（Lucide），与左侧同样按资产路径引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            RightPanel::Insight => "icons/lightbulb.svg",
            RightPanel::Mock => "icons/dice-5.svg",
            RightPanel::History => "icons/clock.svg",
        };
        Icon::default().path(path)
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
    /// M1 项目管理输入实体（懒创建）。
    project_inputs: Option<crate::components::project_ui::ProjectInputs>,
    /// 订阅句柄（保持连接选中事件的订阅存活）。
    _subscription: Option<Subscription>,
}

impl WorkbenchView {
    pub fn new() -> Self {
        // Round 21：从全局系统库加载真实连接（失败降级为空列表 + 提示）。
        let (connections, notice) = crate::services::workspace_loader::load_persisted_connections();
        let shared = Shared::with_connections(connections, notice);
        // P0：解析当前项目会话（环境变量 → 最近项目 → 空态），供标题栏/草稿箱/连接共用。
        *shared.project.borrow_mut() = crate::services::project_session::resolve();
        // M1：排序偏好（直读 settings.json，无需 cx）与首屏项目列表（无项目时）都在构造期完成，
        // 避免在 `render` 里做 I/O（GPUI-kit 编码指南：副作用不得放在 render）。
        {
            let sort = crate::components::project_ui::ProjectSort::from_key(
                &settings::load_settings().projects.sort_mode,
            );
            let mut ui = shared.project_ui.borrow_mut();
            ui.picker.sort = sort;
            ui.picker.sort_initialized = true;
        }
        if shared.project.borrow().is_none() {
            crate::components::project_ui::load_picker(&shared);
        }
        Self {
            shared,
            area: None,
            sidebar: None,
            editor: None,
            right_sidebar: None,
            quick_open_input: None,
            settings_view: None,
            project_inputs: None,
            _subscription: None,
        }
    }

    /// 刷新项目选择器（项目菜单「切换项目」等外部触发）。
    pub fn refresh_project_picker(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        crate::components::project_ui::refresh_picker(&self.shared, &entity, cx);
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
        // 项目名取自当前项目会话（P0）；未打开项目时显示占位。
        let has_project = self.shared.project.borrow().is_some();
        let project_name = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未打开项目".to_string());
        let slot_shared = shared.clone();
        let slot_entity = entity.clone();
        let slot = div()
            .id("project-slot")
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
                    .child(project_name),
            )
            // M1：有项目时点击弹出项目菜单（切换 / 设置 / 重命名 / 关闭）。
            .when(has_project, move |d| {
                d.cursor_pointer().on_click(move |_, _, app| {
                    let mut ui = slot_shared.project_ui.borrow_mut();
                    ui.menu_open = !ui.menu_open;
                    drop(ui);
                    slot_entity.update(app, |_, cx| cx.notify());
                })
            });

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
        let active_label = self.shared.active_left.get().label();
        let left_hidden = self.shared.left_mode.get() == SidebarMode::Hidden;
        let right_hidden = self.shared.right_mode.get() == SidebarMode::Hidden;

        // 左侧独立开关：完全隐藏 / 恢复（恢复时还原隐藏前模式）。
        // 自绘（对齐设计稿 .sb-btn）：显式前景色 + hover 高亮，不依赖 Button 变体样式。
        let left_shared = shared.clone();
        let left_entity = entity.clone();
        let left_toggle = sb_toggle(
            "toggle-left-sidebar",
            IconName::PanelLeftClose,
            if left_hidden {
                "» 恢复"
            } else {
                "« 完全隐藏"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| toggle_sidebar_hidden(&left_shared, &left_entity, true, app),
        );

        // 右侧独立开关（镜像）。
        let right_shared = shared.clone();
        let right_entity = entity.clone();
        let right_toggle = sb_toggle(
            "toggle-right-sidebar",
            IconName::PanelRightClose,
            if right_hidden {
                "« 恢复"
            } else {
                "完全隐藏 »"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| toggle_sidebar_hidden(&right_shared, &right_entity, false, app),
        );

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
                    .child(left_toggle)
                    .child(div().child(active_label)),
            )
            .right(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(div().child(format!("连接：{selected}")))
                    .child(div().child("DuckDB 就绪"))
                    .child(div().child("UTF-8"))
                    .child(right_toggle),
            )
    }
}

/// 状态栏开关按钮（自绘，对齐设计稿 .sb-btn）：图标 + 文字 + hover 高亮。
fn sb_toggle(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    fg: Hsla,
    hover_bg: Hsla,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .text_color(fg)
        .hover(move |s| s.bg(hover_bg))
        .child(Icon::new(icon).size(px(14.)))
        .child(div().child(label))
        .on_click(move |_, _, app| on_click(app))
}

/// 切换单侧「完全隐藏」：隐藏前记录当前模式快照，恢复时按快照还原，
/// 保证「收起」等状态在完全隐藏 / 恢复往返后不丢失。
fn toggle_sidebar_hidden(
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    is_left: bool,
    app: &mut App,
) {
    let (mode, snapshot) = if is_left {
        (&shared.left_mode, &shared.left_mode_before_hidden)
    } else {
        (&shared.right_mode, &shared.right_mode_before_hidden)
    };
    if mode.get() == SidebarMode::Hidden {
        mode.set(snapshot.get());
    } else {
        snapshot.set(mode.get());
        mode.set(SidebarMode::Hidden);
    }
    entity.update(app, |_, cx| cx.notify());
}

/// 还原快照模式；快照异常为 Hidden 时兜底为展开（防御性）。
fn restore_snapshot(mode: SidebarMode) -> SidebarMode {
    if mode == SidebarMode::Hidden {
        SidebarMode::Expanded
    } else {
        mode
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
        // M1：项目输入实体懒创建（选择器搜索 / 新建 / 删除确认）。
        if self.project_inputs.is_none() {
            self.project_inputs = Some(crate::components::project_ui::ProjectInputs::new(window, cx));
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

        // M1：无项目时以选择器覆盖中央区（保留五段外壳）。
        let inputs = self.project_inputs.clone().expect("project inputs lazy init");
        let no_project = { self.shared.project.borrow().is_none() };
        let mut middle = div().h_flex().items_stretch().flex_1().min_h_0();
        if let Some(bar) = left_bar {
            middle = middle.child(bar);
        }
        if no_project {
            let entity = cx.entity();
            let picker =
                crate::components::project_ui::render_picker(&self.shared, &inputs, &entity, cx);
            middle = middle.child(picker);
        } else {
            middle = middle.child(area);
        }
        if let Some(bar) = right_bar {
            middle = middle.child(bar);
        }

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
                        let s = &this.shared;
                        if s.left_mode.get() != SidebarMode::Hidden {
                            s.left_mode_before_hidden.set(s.left_mode.get());
                            s.left_mode.set(SidebarMode::Hidden);
                        }
                        if s.right_mode.get() != SidebarMode::Hidden {
                            s.right_mode_before_hidden.set(s.right_mode.get());
                            s.right_mode.set(SidebarMode::Hidden);
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &RestoreSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let s = &this.shared;
                        if s.left_mode.get() == SidebarMode::Hidden {
                            s.left_mode
                                .set(restore_snapshot(s.left_mode_before_hidden.get()));
                        }
                        if s.right_mode.get() == SidebarMode::Hidden {
                            s.right_mode
                                .set(restore_snapshot(s.right_mode_before_hidden.get()));
                        }
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
            // M1：切换项目（= 关闭当前 + 回选择器）。
            .on_action({
                let entity = cx.entity();
                move |_: &SwitchProject, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let entity = cx.entity();
                        crate::components::project_ui::request_close(&this.shared, &entity, cx);
                    });
                }
            })
            // M1：关闭项目。
            .on_action({
                let entity = cx.entity();
                move |_: &CloseProject, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let entity = cx.entity();
                        crate::components::project_ui::request_close(&this.shared, &entity, cx);
                    });
                }
            })
            .child(self.render_title_bar(window, cx))
            .child(middle)
            .child(self.render_status_bar(cx));

        if let Some(qo) = quick_open {
            root = root.child(qo);
        }
        if let Some(sp) = settings_panel {
            root = root.child(sp);
        }
        // M1：项目管理菜单 / 设置 / 覆盖对话框（有项目时才渲染菜单与设置）。
        if !no_project {
            let entity = cx.entity();
            if let Some(menu) = crate::components::project_ui::render_menu(&self.shared, &inputs, &entity, cx) {
                root = root.child(menu);
            }
            let entity = cx.entity();
            if let Some(settings) =
                crate::components::project_ui::render_settings(&self.shared, &inputs, &entity, cx)
            {
                root = root.child(settings);
            }
        }
        {
            let entity = cx.entity();
            if let Some(overlay) =
                crate::components::project_ui::render_overlays(&self.shared, &inputs, &entity, cx)
            {
                root = root.child(overlay);
            }
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
            if shared.left_mode.get() != SidebarMode::Hidden {
                shared.left_mode_before_hidden.set(shared.left_mode.get());
                shared.left_mode.set(SidebarMode::Hidden);
            }
            if shared.right_mode.get() != SidebarMode::Hidden {
                shared.right_mode_before_hidden.set(shared.right_mode.get());
                shared.right_mode.set(SidebarMode::Hidden);
            }
        }
        QuickOpenCommand::RestoreSidebars => {
            if shared.left_mode.get() == SidebarMode::Hidden {
                shared
                    .left_mode
                    .set(restore_snapshot(shared.left_mode_before_hidden.get()));
            }
            if shared.right_mode.get() == SidebarMode::Hidden {
                shared
                    .right_mode
                    .set(restore_snapshot(shared.right_mode_before_hidden.get()));
            }
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
