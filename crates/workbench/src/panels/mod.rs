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
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::*;

use crate::view::LeftPanel;

use analytics_resource::dialogs::index_repair::{RepairDialogState, open_index_repair_dialog};
use analytics_resource::dialogs::tag::{TagDialogState, open_tag_dialog};
use analytics_resource::dialogs::trash::{TrashDialogState, open_trash_dialog};
use analytics_resource::dialogs::version::{VersionDialogState, open_version_dialog};
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
pub use scratchpad::ScratchpadDiffView;
pub use scratchpad::ScratchpadSearchView;
pub use shared::append_sql;
pub use shared::{
    EditorBridge, ProjectActionRequest, QueryRequest, ResourcesBridge, ScratchpadBridge, Shared,
};

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
    pub fn new(shared: Shared, editor: &EditorShared, cx: &mut Context<Self>) -> Self {
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

    /// 消费「待开的版本历史对话框」（M6）：建状态 → 存会话 → 开窗。
    ///
    /// 开窗与关窗都要 `Window`，而轮询任务里没有——所以取数回来只置 `pending`，
    /// 在这一帧（render）里开。关窗时清会话：再开同一存档才会走"新开"那条。
    fn ensure_version_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rows) = self.shared.version_dialog.borrow_mut().pending.take() else {
            return;
        };
        let read_only = self.shared.project_ui.borrow().read_only;
        let state = VersionDialogState::new();
        state.set_read_only(read_only);
        state.set_rows(rows.seed.rows.clone());

        let resource_id = rows.resource_id.clone();
        let name = rows.seed.name.clone();
        {
            let mut flow = self.shared.version_dialog.borrow_mut();
            flow.session = Some(shared::VersionDialogSession {
                resource_id: resource_id.clone(),
                state: state.clone(),
            });
        }

        let entity = cx.entity();
        let shared_for_close = self.shared.clone();
        let resource_id_for_close = resource_id.clone();
        open_version_dialog(
            window,
            cx,
            rows.seed,
            state,
            move |action, _window, cx| {
                let resource_id = resource_id.clone();
                let name = name.clone();
                entity.update(cx, |this, cx| {
                    this.request_version_action(&resource_id, &name, action, cx)
                });
            },
            move |_cx| {
                let mut flow = shared_for_close.version_dialog.borrow_mut();
                let same = flow
                    .session
                    .as_ref()
                    .map(|session| session.resource_id.as_str())
                    == Some(resource_id_for_close.as_str());
                if same {
                    flow.session = None;
                }
            },
        );
    }

    /// 消费「待开的索引修复对话框」（M6）：与版本历史同一形态（见 `ensure_version_dialog`）。
    ///
    /// 不分资源且一次只开一个：关窗就把会话清掉（下次扫描回来重新开）。
    fn ensure_repair_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scan) = self.shared.repair_dialog.borrow_mut().pending.take() else {
            return;
        };
        let read_only = self.shared.project_ui.borrow().read_only;
        let state = RepairDialogState::new();
        state.set_read_only(read_only);
        state.set_rows(scan.rows.clone());
        {
            let mut flow = self.shared.repair_dialog.borrow_mut();
            flow.session = Some(shared::RepairDialogSession {
                state: state.clone(),
            });
        }

        let entity = cx.entity();
        let shared_for_close = self.shared.clone();
        open_index_repair_dialog(
            window,
            cx,
            state,
            move |action, _window, cx| {
                entity.update(cx, |this, cx| this.request_repair_action(action, cx));
            },
            move |_cx| {
                shared_for_close.repair_dialog.borrow_mut().session = None;
            },
        );
    }

    /// 消费「待开的回收站对话框」（M6）：与索引修复同一形态（见 `ensure_repair_dialog`）。
    ///
    /// 项目级回收站一个项目只有一处，同样一次只开一个：关窗就把会话清掉
    /// （下次取数回来重新开，拿到的是当时的最新条目）。
    fn ensure_trash_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(seed) = self.shared.trash_dialog.borrow_mut().pending.take() else {
            return;
        };
        let read_only = self.shared.project_ui.borrow().read_only;
        let state = TrashDialogState::new();
        state.set_read_only(read_only);
        state.set_rows(seed.rows.clone());
        state.set_foreign(seed.foreign.clone());
        {
            let mut flow = self.shared.trash_dialog.borrow_mut();
            flow.session = Some(shared::TrashDialogSession {
                state: state.clone(),
            });
        }

        let entity = cx.entity();
        let shared_for_close = self.shared.clone();
        open_trash_dialog(
            window,
            cx,
            seed,
            state,
            move |action, _window, cx| {
                entity.update(cx, |this, cx| this.request_trash_action(action, cx));
            },
            move |_cx| {
                shared_for_close.trash_dialog.borrow_mut().session = None;
            },
        );
    }

    /// 消费「待开的标签对话框」（M6）：与版本历史同一形态（针对某条存档）。
    ///
    /// 输入实体在这里建（开窗要 `Window`，轮询任务里没有）；会话带 `resource_id`——
    /// 换一条存档要重开（同一存档再要一次只是刷新行）。
    fn ensure_tag_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rows) = self.shared.tag_dialog.borrow_mut().pending.take() else {
            return;
        };
        let read_only = self.shared.project_ui.borrow().read_only;
        let state = TagDialogState::new(rows.seed.selected.clone());
        state.set_read_only(read_only);
        state.set_options(rows.seed.options.clone());
        let name_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx).placeholder("新建标签")
        });
        {
            let mut flow = self.shared.tag_dialog.borrow_mut();
            flow.session = Some(shared::TagDialogSession {
                resource_id: rows.resource_id.clone(),
                state: state.clone(),
            });
        }

        let entity = cx.entity();
        let shared_for_close = self.shared.clone();
        let resource_id = rows.resource_id.clone();
        let resource_name = rows.resource_name.clone();
        let seed = rows.seed.clone();
        open_tag_dialog(
            window,
            cx,
            seed,
            state,
            name_input,
            move |event, _window, cx| {
                entity.update(cx, |this, cx| {
                    this.request_tag_action(&resource_id, &resource_name, event, cx)
                });
            },
            move |_cx| {
                let mut flow = shared_for_close.tag_dialog.borrow_mut();
                // 只清“当前这一条”的会话：已经换成另一条存档时不动它（与版本历史同门口径）。
                let same = flow
                    .session
                    .as_ref()
                    .map(|session| session.resource_id.as_str())
                    == Some(rows.resource_id.as_str());
                if same {
                    flow.session = None;
                }
            },
        );
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // M6：版本历史 / 索引修复 / 回收站 / 标签四个对话框的待开数据都在这一帧消费（开窗要 `Window`）。
        self.ensure_version_dialog(window, cx);
        self.ensure_repair_dialog(window, cx);
        self.ensure_trash_dialog(window, cx);
        self.ensure_tag_dialog(window, cx);
        // 「在导航树中定位」（Quick Open 的 ⌥↵）在这一帧被消费：render 是权威同步点，
        // 不在事件路径上碰面板状态。发起侧已把左 Dock 切到数据源；这里不再判断面板是否在前台，
        // 因为请求一旦落在这里就应当被完成（漏掉就会「点了没反应」）。
        if let Some(object) = self.shared.take_reveal() {
            let panel = self.nav_panel.clone();
            panel.update(cx, |panel, cx| panel.reveal_ref(&object, cx));
        }
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

/// 注入宿主重绘桥（装配期调用）：`Shared::notify_host` 的去处。
///
/// 为何需要它：模态层的挂载点在**宿主 render** 里，而 `cx.notify()` 只重渲染该视图
/// 子树——只通知 `Root` 的话，对话框状态激活了但层不进元素树（表现为「点了没反应」）。
///
/// **必须 `defer`，这是本条桥的硬约束**：它也会被「在编辑区自己的 update 里」发起的
/// 动作调到——典型是导航栏「＋」→ `install_editor_bridge` 的 `editor.update` →
/// `EditorPanel::request_new_connection` → `Shared::notify_host`。若桥里同步再进一次
/// 编辑区，GPUI 的 double-lease 检查会 panic（`cannot update … while it is already
/// being updated`），而那个 panic 会从**窗口过程**里 unwind 出去拿不到捕获 → abort。
/// 真机表现就是：点「新增数据源」直接 `0xc0000409`（2026-09-20 定位，见连接开发方案
/// 运行时稳定性 ㉒）。
///
/// 生产与同构测试宿主（`tests/dialog_host_layer.rs`）调的就是这一份：接线只此一处。
pub fn install_host_redraw_bridge<T: Render + 'static>(
    shared: &Shared,
    view: gpui_kit::WeakEntity<T>,
    editor: Entity<EditorPanel>,
) {
    *shared.host_redraw.borrow_mut() = Some(Rc::new(move |cx: &mut App| {
        // 每次调用先克隆句柄（闭包是 `Fn`，会被反复调）。
        let editor = editor.clone();
        let view = view.clone();
        cx.defer(move |cx| {
            editor.update(cx, |_, cx| cx.notify());
            if let Some(view) = view.upgrade() {
                view.update(cx, |_, cx| cx.notify());
            }
        });
    }));
}

/// 注入编辑区命令端口（装配期调用）。
///
/// 生产入口：`WorkbenchView::init_workspace`；与宿主同构的测试宿主（`tests/dialog_host_layer.rs`）
/// 也调本函数——接线只此一份，端口形状变化时两侧一起变。
pub fn install_editor_bridge(shared: &Shared, editor: Entity<EditorPanel>) {
    let editor_for_edit = editor.clone();
    let editor_for_new = editor.clone();
    let editor_for_search = editor.clone();
    let editor_for_diff = editor.clone();
    *shared.editor_bridge.borrow_mut() = Some(EditorBridge {
        edit_connection: Rc::new(move |id: String, window: &mut Window, cx: &mut App| {
            editor_for_edit.update(cx, |panel, cx| {
                panel.request_edit_connection(id, window, cx)
            });
        }),
        new_connection: Rc::new(move |window: &mut Window, cx: &mut App| {
            editor_for_new.update(cx, |panel, cx| panel.request_new_connection(window, cx));
        }),
        show_properties: Rc::new(move |request: PropertyRequest, cx: &mut App| {
            editor.update(cx, |panel, cx| panel.request_properties(request, cx));
        }),
        show_search_results: Rc::new(move |view: Option<ScratchpadSearchView>, cx: &mut App| {
            editor_for_search.update(cx, |panel, cx| panel.set_scratchpad_search(view, cx));
        }),
        show_diff: Rc::new(move |view: Option<ScratchpadDiffView>, cx: &mut App| {
            editor_for_diff.update(cx, |panel, cx| panel.set_scratchpad_diff(view, cx));
        }),
    });
}
