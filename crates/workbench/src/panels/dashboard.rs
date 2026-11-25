//! 统计看板标签页（中央 Dock 独立 tab）：只读业务概览四卡。
//!
//! 数据链路：面板自持后台取数（复用 `services::overview::load`，后台线程纯函数），
//! render 只读 [`OverviewCounts`] 快照；「刷新」按钮按同一批输入重新取一次。挂载与唤起
//! 由宿主 `WorkbenchView::ensure_dashboard_panel` 负责（单例：弱句柄有效则聚焦复用）。
//!
//! 设置项 `layout.dashboard_enabled` 关闭后：状态栏不渲染入口 chip，面板不挂载、不发起
//! 概览取数（开关消费点在 `actions.rs` 状态栏与宿主装配处读取）。

use std::path::PathBuf;

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use shared::i18n::tr;

use super::shared::Shared;
use crate::services::overview::OverviewCounts;

/// 看板宿主：中央 tab 内单实例，自持概览快照与后台取数任务。
pub struct DashboardPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`BasePanel::on_added_to` 注入；重复打开时用于聚焦自身）。
    group: Option<gpui::WeakEntity<TabGroup>>,
    /// 概览计数快照（`None` = 首帧尚未回填，卡片显示「…」）。
    counts: Option<OverviewCounts>,
    /// 概览取数任务句柄（drop = 取消；重新取数时覆盖旧的）。
    overview_task: Option<Task<()>>,
    /// 本次取数对应的项目根（项目未开时为 `None`；换项目后重取）。
    overview_for: Option<PathBuf>,
    /// 是否已发起过取数（`overview_for` 区分不出「没发起」与「未开项目」）。
    overview_started: bool,
}

impl DashboardPanel {
    pub fn new(shared: Shared, cx: &mut gpui::Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            group: None,
            counts: None,
            overview_task: None,
            overview_for: None,
            overview_started: false,
        }
    }

    /// 首帧 / 项目切换后起一次概览取数（`render` 入口调用，取数本身在后台线程）。
    ///
    /// 设置关闭看板时不发起：此时面板 `visible() = false` 不挂树，这里再兜一道，
    /// 避免设置态与渲染态错位时白跑一趟后台 I/O。
    fn ensure_overview(&mut self, cx: &mut Context<Self>) {
        if !settings::SettingsService::dashboard_enabled(cx) {
            return;
        }
        let project = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|p| (p.name.clone(), p.root.clone()));
        let current_root = project.as_ref().map(|(_, root)| root.clone());
        if self.overview_started && self.overview_for == current_root {
            return;
        }
        self.overview_for = current_root;
        self.counts = None;
        self.overview_started = true;
        self.spawn_overview(project, cx);
    }

    /// 「刷新」按钮：忽略当前快照，按同一批输入重新取一次。
    fn reload_overview(&mut self, cx: &mut Context<Self>) {
        let project = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|p| (p.name.clone(), p.root.clone()));
        self.overview_for = project.as_ref().map(|(_, root)| root.clone());
        self.counts = None;
        self.overview_started = true;
        self.spawn_overview(project, cx);
    }

    /// 真正起后台任务：阻塞读库放 `background_executor`，回填在主线程。
    ///
    /// 连接数与连接状态直接从 `Shared` 克隆一份过去（背景线程不能碰 `Rc<RefCell<..>>`）。
    fn spawn_overview(
        &mut self,
        project: Option<(String, PathBuf)>,
        cx: &mut Context<Self>,
    ) {
        let flags: Vec<bool> = self
            .shared
            .connections
            .borrow()
            .iter()
            .map(|item| item.connected)
            .collect();
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        let task = cx.spawn(async move |_this, cx| {
            let counts = executor
                .spawn(async move {
                    let project_ref = project
                        .as_ref()
                        .map(|(name, root)| (name.as_str(), root.as_path()));
                    crate::services::overview::load(project_ref, &flags)
                })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.counts = Some(counts);
                cx.notify();
            });
        });
        self.overview_task = Some(task);
    }
}

impl EventEmitter<PanelEvent> for DashboardPanel {}

impl Focusable for DashboardPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DashboardPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        self.ensure_overview(cx);
        let theme = cx.theme().clone();
        let entity = cx.entity();
        let refresh = move |_window: &mut Window, app: &mut App| {
            entity.update(app, |this, cx| this.reload_overview(cx));
        };
        let cards =
            crate::panels::overview::render_overview_cards(self.counts.as_ref(), &theme, refresh);

        div()
            .id("dashboard-panel")
            .role(gpui::Role::Group)
            .test_support()
            .size_full()
            .bg(theme.colors.background)
            .overflow_y_scrollbar()
            .p_4()
            .v_flex()
            .gap_2()
            .child(cards)
    }
}

impl BasePanel for DashboardPanel {
    fn panel_name(&self) -> &'static str {
        "dashboard"
    }

    /// 设置关闭后不挂树（状态栏 chip 同步不渲染）。
    fn visible(&self, cx: &App) -> bool {
        settings::SettingsService::dashboard_enabled(cx)
    }

    fn on_added_to(
        &mut self,
        group: gpui::WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        self.group = Some(group);
    }
}

impl ComponentPanel for DashboardPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(tr("统计看板").into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(tr("统计看板"))
    }
}

/// 把看板 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// 与 `focus_search_tab` 同因：必须在实体更新之外调用（`TabGroup` 切 tab 会回读面板实体，
/// 在更新闭包内调用会 double lease panic）。选中按面板 `entity_id` 定位。
pub fn focus_dashboard_tab(
    panel: &gpui::Entity<DashboardPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    let (focus_handle, group) = {
        let view = panel.read(cx);
        (view.focus_handle.clone(), view.group.clone())
    };
    window.focus(&focus_handle, cx);
    let Some(group) = group else {
        return;
    };
    let me = panel.entity_id();
    group
        .update(cx, |group, cx| {
            let ix = group
                .panels()
                .iter()
                .position(|p| p.view().entity_id() == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        })
        .ok();
}
