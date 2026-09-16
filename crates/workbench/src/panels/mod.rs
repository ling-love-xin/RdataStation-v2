//! 工作台 Dock 面板（Round 20）
//!
//! DockArea 布局系统接入后，原侧边栏/内容区拆为独立面板实体：
//! - `SidebarPanel`：按活动工具渲染连接列表 / 导航树 / 资源 / 设置；
//! - `EditorPanel`：中央内容区（连接详情 + 新建连接）；
//! - `Shared`：面板与工作台之间共享的状态（工具 / 选中连接 / 连接数据 / 通知文案）。
//!
//! 面板只实现 GPUI 面板协议（base `Panel` + component `Panel`），不承载业务逻辑；
//! 数据仍来自 `ConnectionItem::sample()`（占位），下一轮接 ConnectionService。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;


use crate::view::LeftPanel;

use analytics_resource::resource_view::ResourcesPanel;
use database::nav_view::NavView;
use scratchpad::ScratchpadView;
// 前导 `::`：本模块另有一个 `mod editor;`（面板子模块），不能靠名字消歧。
use ::editor::shared::EditorShared;

mod editor;
mod resources;
mod right;
mod shared;

// 对外路径保持 `crate::panels::X` 不变：组件层、宿主与集成测试零改动。
pub use database::model::PropertyRequest;
pub use editor::EditorPanel;
pub use right::RightSidebarPanel;
pub use scratchpad::ScratchpadSearchView;
pub use shared::append_sql;
pub use shared::{EditorBridge, ProjectActionRequest, QueryRequest, ResourcesBridge, ScratchpadBridge, Shared};

/// 侧边栏面板：按活动工具渲染内容。
///
/// 各工具自己带视图（`database` 导航 / `scratchpad` 草稿箱 / `analytics_resource` 资产库），
/// 本面板只做“外壳 + 转发”：与右栏 `RightSidebarPanel` 同一职责。
pub struct SidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// M4 数据源导航面板实体（视图与状态在 `rds-database` crate）。
    nav_panel: Entity<NavView>,
    /// M5 草稿箱面板实体（视图与状态在 `rds-scratchpad` crate）。
    scratchpad_panel: Entity<ScratchpadView>,
    /// M6 资产库面板实体（视图与状态在 `rds-analytics-resource` crate；构造期创建，无 I/O）。
    resources_panel: Entity<ResourcesPanel>,
    /// 资产库刷新结果的轮询任务（与草稿箱轮询同一形态）。
    resources_pump: RefCell<Option<Task<()>>>,
}

impl SidebarPanel {
    pub fn new(
        shared: Shared,
        editor: &EditorShared,
        cx: &mut Context<Self>,
    ) -> Self {
        // M4：导航面板实体——宿主端口在此注入（视图在 `database` crate，不依赖 workbench）。
        let nav_host = Rc::new(crate::components::nav_host::WorkbenchNavHost::new(
            shared.clone(),
        ));
        let nav_panel = cx.new(|cx| NavView::new(nav_host, cx));
        // M5：草稿箱面板实体（同上；重活走 `scratchpad::jobs` 的工作线程）。
        // 编辑器共享状态只用于脏点（编辑器里未保存修改 → 草稿树上的点）。
        let scratchpad_host = crate::components::scratchpad_host::build_host(&shared, editor);
        let scratchpad_panel = cx.new(|cx| ScratchpadView::new(scratchpad_host, cx));
        // M6：资产库面板实体（视图与状态在 `analytics_resource` crate；构造期创建，无 I/O）。
        let resources_panel = Self::build_resources_panel(&shared, cx);
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            nav_panel,
            scratchpad_panel,
            resources_panel,
            resources_pump: RefCell::new(None),
        }
    }

    /// Ctrl+F：聚焦导航搜索框（先到位的请求由导航面板自己等一帧）。
    pub(crate) fn focus_nav_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_panel
            .update(cx, |panel, cx| panel.focus_nav_search(window, cx));
    }

    /// 转发草稿箱面板渲染。
    fn render_scratchpad_panel(&mut self) -> Div {
        let panel = self.scratchpad_panel.clone();
        div().v_flex().size_full().min_h_0().child(panel)
    }

    /// 确保草稿箱轮询在跑（编辑区「全部替换」经 `ScratchpadBridge` 调用）。
    pub(crate) fn ensure_scratchpad_pump(&self, cx: &mut Context<Self>) {
        self.scratchpad_panel
            .update(cx, |panel, cx| panel.ensure_scratchpad_pump(cx));
    }

    /// 转发导航面板渲染（含滚动与空态都在 crate 内）。
    fn render_nav_panel(&mut self) -> Div {
        let panel = self.nav_panel.clone();
        div().v_flex().size_full().min_h_0().child(panel)
    }

    fn render_plugin_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("插件"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 插件市场（下一轮接入）"),
            )
    }
}

impl EventEmitter<BasePanelEvent> for SidebarPanel {}

impl Focusable for SidebarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidebarPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().colors.background;
        let fg = cx.theme().colors.foreground;
        let active = self.shared.active_left.get();
        let content: Div = match active {
            LeftPanel::Draft => self.render_scratchpad_panel(),
            LeftPanel::Database => self.render_nav_panel(),
            LeftPanel::Resources => self.render_resources_panel(cx),
            LeftPanel::Plugin => self.render_plugin_placeholder(fg),
        };
        div().v_flex().size_full().min_h_0().bg(bg).child(content)
    }
}

impl BasePanel for SidebarPanel {
    fn panel_name(&self) -> &'static str {
        "sidebar"
    }
}

impl ComponentPanel for SidebarPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.shared.active_left.get().label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(self.shared.active_left.get().label())
    }
}

/// 注入草稿箱命令端口（装配期调用；生产入口 `WorkbenchView::init_workspace`）。
pub fn install_scratchpad_bridge(shared: &Shared, sidebar: Entity<SidebarPanel>) {
    *shared.scratchpad_bridge.borrow_mut() = Some(ScratchpadBridge {
        ensure_pump: Rc::new(move |cx: &mut App| {
            sidebar.update(cx, |panel, cx| panel.ensure_scratchpad_pump(cx));
        }),
    });
}

/// 注入资产库刷新端口（M6；装配期调用，生产入口同上）。
///
/// 归档 / 取回完成后，发起方（右栏详情、对话框回调）只有 `Shared`，
/// 刷新与轮询回填都在侧栏面板手里——这条线由端口牵，面板之间不互订。
pub fn install_resources_bridge(shared: &Shared, sidebar: Entity<SidebarPanel>) {
    *shared.resources_bridge.borrow_mut() = Some(ResourcesBridge {
        refresh: Rc::new(move |cx: &mut App| {
            sidebar.update(cx, |panel, cx| panel.request_resources_refresh(cx));
        }),
    });
}

/// 注入编辑区命令端口（装配期调用）。
///
/// 生产入口：`WorkbenchView::init_workspace`；与宿主同构的测试宿主（`tests/dialog_host_layer.rs`）
/// 也调本函数——接线只此一份，端口形状变化时两侧一起变。
pub fn install_editor_bridge(shared: &Shared, editor: Entity<EditorPanel>) {
    let editor_for_edit = editor.clone();
    let editor_for_new = editor.clone();
    let editor_for_search = editor.clone();
    *shared.editor_bridge.borrow_mut() = Some(EditorBridge {
        edit_connection: Rc::new(move |id: String, window: &mut Window, cx: &mut App| {
            editor_for_edit
                .update(cx, |panel, cx| panel.request_edit_connection(id, window, cx));
        }),
        new_connection: Rc::new(move |window: &mut Window, cx: &mut App| {
            editor_for_new.update(cx, |panel, cx| panel.request_new_connection(window, cx));
        }),
        show_properties: Rc::new(move |request: PropertyRequest, cx: &mut App| {
            editor.update(cx, |panel, cx| panel.request_properties(request, cx));
        }),
        show_search_results: Rc::new(
            move |view: Option<ScratchpadSearchView>, cx: &mut App| {
                editor_for_search.update(cx, |panel, cx| panel.set_scratchpad_search(view, cx));
            },
        ),
    });
}
