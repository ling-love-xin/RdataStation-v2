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
use gpui_kit::component::input::InputState;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use scratchpad::ScratchpadWatcher;

use database::model::NavSource;

use crate::view::{LeftPanel, RightPanel};

mod editor;
mod nav;
mod right;
// 带 `_panel` 后缀：若名为 `scratchpad`，会遮蔽 `scratchpad` crate（本模块的导入与
// `SidebarPanel::scratchpad_watch` 字段类型都指向该 crate）；与既有测试模块名一致。
mod scratchpad_panel;
mod shared;

// 对外路径保持 `crate::panels::X` 不变：组件层、宿主与集成测试零改动。
pub use editor::EditorPanel;
pub use nav::PropertyRequest;
pub use right::RightSidebarPanel;
pub use scratchpad_panel::ScratchpadSearchView;
pub use shared::{EditorBridge, ProjectActionRequest, Shared};

use nav::{DatabaseNavView, NavOrderItem};
use scratchpad_panel::ScratchpadView;

/// 侧边栏面板发出的事件（由工作台订阅）。
#[derive(Clone, Debug)]
pub enum SidebarEvent {
    /// 用户点击了连接条目。
    SelectConnection(usize),
    /// 用户点击了连接「编辑」。
    EditConnection(String),
    /// 用户点击了导航面板头「＋」/ 空态「新建连接」（已置位 `shared.new_connection_request`）。
    NewConnectionRequest,
    /// 导航右键「查看数据」已写入 `shared.editor_set`，请编辑区渲染时消费。
    EditorSqlRequest,
    /// 通用入口：连接 / 对象右键「在 SQL 编辑器中打开」——选中该连接并聚焦中央编辑区。
    OpenSqlEditor(String),
    /// 通用入口：右键「生成 Mock 数据」（仅表 / 视图）/「查看洞察」——展开右 Dock 并切面板。
    OpenRightPanel(RightPanel),
}

/// 侧边栏面板：按活动工具渲染内容。
pub struct SidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// M5 草稿箱面板状态（首次渲染触发加载）。
    scratchpad: Rc<RefCell<ScratchpadView>>,
    /// 数据源导航面板状态（懒加载对象树）。
    database_nav: Rc<RefCell<DatabaseNavView>>,
    /// 数据源导航搜索框（懒创建）。
    nav_search: Option<Entity<InputState>>,
    _nav_search_sub: Option<Subscription>,
    /// 连接行内联标签输入框（组织编辑器打开时创建，关闭时销毁）。
    nav_tag_input: Option<Entity<InputState>>,
    /// 当前标签输入框对应的连接 ID（切换连接时重建并重新预填）。
    nav_tag_input_for: Option<String>,
    _nav_tag_sub: Option<Subscription>,
    /// 行内「复制为模板」输入框（打开时创建，关闭时销毁）。
    nav_copy_input: Option<Entity<InputState>>,
    /// 当前复制输入框对应的连接 ID（切换连接时重建）。
    nav_copy_input_for: Option<String>,
    _nav_copy_sub: Option<Subscription>,
    /// 分组名内联重命名输入框（重命名时创建，关闭时销毁）。
    nav_group_input: Option<Entity<InputState>>,
    _nav_group_sub: Option<Subscription>,
    /// 渲染顺序重建的可见项（键盘 ↑↓ 移动 / 展开折叠 / 打开属性）。
    nav_order: Rc<RefCell<Vec<NavOrderItem>>>,
    /// 正在轮询预热进度的后台任务（避免重复启动）。
    warm_poll: Option<Task<()>>,
    /// 正在轮询导航加载结果的后台任务（避免重复启动；`&self` 路径也要访问）。
    nav_pump: RefCell<Option<Task<()>>>,
    /// 正在轮询草稿箱加载结果的后台任务（同上）。
    scratchpad_pump: RefCell<Option<Task<()>>>,
    /// 草稿箱目录监控器（外部改动 → 去抖重拉；每个项目根一个）。
    scratchpad_watch: Option<ScratchpadWatcher>,
    /// 监控轮询任务（常驻，每 ~1.2 s 探查一次变更标记）。
    scratchpad_watch_poll: RefCell<Option<Task<()>>>,
}

impl SidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        // 从 settings.json 恢复 facet 筛选（UI 偏好，跨项目）。
        let saved = settings::SettingsService::nav_filters(cx);
        let mut nav_view = DatabaseNavView::default();
        nav_view.source_filter = saved.source.as_deref().and_then(NavSource::from_key);
        nav_view.type_filter = saved.db_type.clone();
        nav_view.driver_filter = saved.driver.clone();
        nav_view.tag_filter = saved.tag.clone();
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            scratchpad: Rc::new(RefCell::new(ScratchpadView::default())),
            database_nav: Rc::new(RefCell::new(nav_view)),
            nav_search: None,
            _nav_search_sub: None,
            nav_tag_input: None,
            nav_tag_input_for: None,
            _nav_tag_sub: None,
            nav_copy_input: None,
            nav_copy_input_for: None,
            _nav_copy_sub: None,
            nav_group_input: None,
            _nav_group_sub: None,
            nav_order: Rc::new(RefCell::new(Vec::new())),
            warm_poll: None,
            nav_pump: RefCell::new(None),
            scratchpad_pump: RefCell::new(None),
            scratchpad_watch: None,
            scratchpad_watch_poll: RefCell::new(None),
        }
    }

    fn render_resources_placeholder(&self, fg: Hsla) -> Div {
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
                    .child("分析资源（下一轮接入）"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· 数据源连接引用"),
            )
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("· DuckDB 分析表"),
            )
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

impl EventEmitter<SidebarEvent> for SidebarPanel {}

impl Focusable for SidebarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidebarPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 草稿箱后台任务的结果回填：必须在**所有**面板模式下都消费，
        // 因为编辑区的「全部替换」也复用这个轮询印（此时左侧可能停在数据源面板）。
        // （仅剩一种例外：“完全隐藏”时侧栏整体不渲染，请求会留在标记里，
        //  恢复侧栏后当帧补上——任务本身已在后台完成，不会丢结果。）
        if self.shared.scratchpad_pump_request.take() {
            self.ensure_scratchpad_pump(cx);
        }
        let bg = cx.theme().colors.background;
        let fg = cx.theme().colors.foreground;
        let active = self.shared.active_left.get();
        let content: Div = match active {
            LeftPanel::Draft => self.render_scratchpad(window, cx),
            LeftPanel::Database => self.render_database_nav(window, cx),
            LeftPanel::Resources => self.render_resources_placeholder(fg),
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

/// 注入编辑区命令端口（装配期调用）。
///
/// 生产入口：`WorkbenchView::init_workspace`；与宿主同构的测试宿主（`tests/dialog_host_layer.rs`）
/// 也调本函数——接线只此一份，端口形状变化时两侧一起变。
pub fn install_editor_bridge(shared: &Shared, editor: Entity<EditorPanel>) {
    let editor_for_edit = editor.clone();
    let editor_for_new = editor;
    *shared.editor_bridge.borrow_mut() = Some(EditorBridge {
        edit_connection: Rc::new(move |id: String, window: &mut Window, cx: &mut App| {
            editor_for_edit
                .update(cx, |panel, cx| panel.request_edit_connection(id, window, cx));
        }),
        new_connection: Rc::new(move |window: &mut Window, cx: &mut App| {
            editor_for_new.update(cx, |panel, cx| panel.request_new_connection(window, cx));
        }),
    });
}
