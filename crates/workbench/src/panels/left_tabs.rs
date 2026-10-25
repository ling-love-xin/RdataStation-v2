//! 左 dock 模块面板（真面板化）：数据源导航 / 草稿箱 / 资产库 / 插件。
//!
//! 每个模块 = 独立 dock tab（实现 gpui-kit `Panel` trait），dock 自动生成 tab 条：
//! 点击 tab 同面切换，拖拽可分栏并排显示多个模块。内容仍是各 crate 自持视图
//! （`NavView` / `ScratchpadView` / `ResourcesPanel`），本文件只做薄壳 + 面板协议：
//! - `tab_name` / `title`：tab 条展示（带活动栏同款图标）；
//! - `set_active`：dock 切 tab 时把活动栏高亮同步回 `Shared::active_left`；
//! - `closable = false`：模块 tab 是布局的一部分，不给 ✕ 关闭（避免重建逻辑）。

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelControl};
use gpui_kit::component::{ActiveTheme, IconName, Size, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::view::LeftPanel;

use analytics_resource::resource_view::ResourcesPanel;
use database::nav_view::NavView;
use plugin::wasm::PluginState;
use plugin::PluginInfo;
use scratchpad::ScratchpadView;

use super::Shared;

/// 数据源导航 tab：内容 = `database` crate 的 `NavView`（搜索框 + 连接树自带 chrome）。
pub struct DatabaseNavPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    nav: Entity<NavView>,
}

impl DatabaseNavPanel {
    pub fn new(shared: Shared, nav: Entity<NavView>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            nav,
        }
    }
}

impl EventEmitter<BasePanelEvent> for DatabaseNavPanel {}

impl Focusable for DatabaseNavPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DatabaseNavPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let nav = self.nav.clone();
        div().v_flex().size_full().min_h_0().child(nav)
    }
}

impl BasePanel for DatabaseNavPanel {
    fn panel_name(&self) -> &'static str {
        "database_nav"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Database);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for DatabaseNavPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Database.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Database.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Database.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}

/// 草稿箱 tab：内容 = `scratchpad` crate 的 `ScratchpadView`。
pub struct ScratchPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    scratchpad: Entity<ScratchpadView>,
}

impl ScratchPanel {
    pub fn new(shared: Shared, scratchpad: Entity<ScratchpadView>, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            scratchpad,
        }
    }
}

impl EventEmitter<BasePanelEvent> for ScratchPanel {}

impl Focusable for ScratchPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScratchPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let scratchpad = self.scratchpad.clone();
        div().v_flex().size_full().min_h_0().child(scratchpad)
    }
}

impl BasePanel for ScratchPanel {
    fn panel_name(&self) -> &'static str {
        "scratchpad"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Draft);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ScratchPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Draft.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Draft.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Draft.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}

/// 资产库 tab：内容 = `analytics_resource` crate 的 `ResourcesPanel`
/// （该面板自身已实现 `Panel` trait，这里薄壳化以统一 tab 语义 + 同步活动栏）。
pub struct ResourcePanelTab {
    shared: Shared,
    focus_handle: FocusHandle,
    resources: Entity<ResourcesPanel>,
}

impl ResourcePanelTab {
    pub fn new(
        shared: Shared,
        resources: Entity<ResourcesPanel>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            resources,
        }
    }
}

impl EventEmitter<BasePanelEvent> for ResourcePanelTab {}

impl Focusable for ResourcePanelTab {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ResourcePanelTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let resources = self.resources.clone();
        div().v_flex().size_full().min_h_0().child(resources)
    }
}

impl BasePanel for ResourcePanelTab {
    fn panel_name(&self) -> &'static str {
        "resources"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Resources);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for ResourcePanelTab {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Resources.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Resources.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Resources.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}

/// 插件 tab：已装插件清单（名称 / 版本 / 状态）+ 刷新 + 启用 / 停用。
///
/// 数据面 = 全局 `PluginManager`（`plugin::manager`，app 启动时初始化）；会话态清单
/// 由事件路径刷新（render 纯读）：首次 `new` 与「刷新 / 启停」操作后重新拉取。
/// 启停是异步管理操作（依赖 tokio 上下文），经全局 runtime 句柄在后台执行，不阻塞 UI。
pub struct PluginPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 会话态插件清单（`plugin::manager::PluginInfo`，事件路径写入）
    plugins: Rc<RefCell<Vec<PluginInfo>>>,
    /// 刷新进行中（防重复触发）
    loading: Rc<Cell<bool>>,
    /// 最近一次刷新/操作的错误文案（展示用，成功即清）
    error: Rc<RefCell<Option<String>>>,
}

impl PluginPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        let this = Self {
            shared,
            focus_handle: cx.focus_handle(),
            plugins: Rc::new(RefCell::new(Vec::new())),
            loading: Rc::new(Cell::new(false)),
            error: Rc::new(RefCell::new(None)),
        };
        this.refresh_plugins(cx);
        this
    }

    /// 重新拉取插件清单（后台执行 `list_plugins`，回主线程写状态）。
    fn refresh_plugins(&self, cx: &mut Context<Self>) {
        if self.loading.get() {
            return;
        }
        self.loading.set(true);
        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            // list_plugins 是同步快照读取；放进后台线程避免任何潜在 I/O 阻塞 UI
            let outcome = plugin::get_plugin_manager()
                .map(|mgr| mgr.list_plugins())
                .transpose();
            let _ = weak.update(cx, |this, cx| {
                this.loading.set(false);
                match outcome {
                    Ok(None) => {
                        this.error.replace(Some("插件运行时未初始化".to_string()));
                    }
                    Ok(Some(items)) => {
                        this.plugins.replace(items);
                        this.error.replace(None);
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                    }
                };
                cx.notify();
            });
        });
    }

    /// 启停插件（`enable_plugin` / `disable_plugin` 依赖 tokio 上下文：DB 异步锁 +
    /// sidecar 进程停启）。经全局 runtime 句柄在后台 `block_on`，完成后回主线程刷新。
    fn set_plugin_enabled(&self, id: String, enabled: bool, cx: &mut Context<Self>) {
        let Some(handle) = plugin::runtime_handle().cloned() else {
            self.error.replace(Some("插件运行时未初始化".to_string()));
            cx.notify();
            return;
        };
        let Some(mgr) = plugin::get_plugin_manager().cloned() else {
            self.error.replace(Some("插件运行时未初始化".to_string()));
            cx.notify();
            return;
        };
        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            // block_on 跑在 gpui 后台线程（cx.spawn 的 executor），不阻塞 UI 线程
            let outcome = if enabled {
                handle.block_on(async { mgr.enable_plugin(&id).await })
            } else {
                handle.block_on(async { mgr.disable_plugin(&id).await })
            };
            let _ = weak.update(cx, |this, cx| {
                match outcome {
                    Ok(()) => this.error.replace(None),
                    Err(e) => this.error.replace(Some(format!("{e}"))),
                };
                this.refresh_plugins(cx);
            });
        });
    }
}

impl EventEmitter<BasePanelEvent> for PluginPanel {}

impl Focusable for PluginPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 插件状态徽标（文案 + 语义色）。
fn plugin_state_label(state: PluginState) -> (&'static str, &'static str) {
    match state {
        PluginState::Active => ("已激活", "success"),
        PluginState::Loaded => ("已加载", "info"),
        PluginState::Inactive => ("已停用", "muted_foreground"),
        PluginState::Error => ("出错", "danger"),
    }
}

impl Render for PluginPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let entity = cx.entity();
        let refresh_btn = Button::new("plugin-refresh")
            .ghost()
            .icon(IconName::RefreshCw)
            .with_size(Size::XSmall)
            .tooltip("刷新插件清单")
            .on_click(move |_, _window, cx| {
                let _ = entity.update(cx, |this, cx| this.refresh_plugins(cx));
            });

        let plugins = self.plugins.borrow().clone();
        let error_text = self.error.borrow().clone();
        let mut list = Vec::new();
        for p in &plugins {
            let id = p.id.clone();
            let active = p.state == PluginState::Active;
            let entity = cx.entity();
            let action = if active {
                Button::new(SharedString::from(format!("plugin-{id}-disable")))
                    .ghost()
                    .with_size(Size::XSmall)
                    .label("停用")
                    .tooltip("停用后本会话立即停止运行（进程停用），下次启动不再激活")
                    .on_click(move |_, _window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            this.set_plugin_enabled(id.clone(), false, cx)
                        });
                    })
            } else {
                Button::new(SharedString::from(format!("plugin-{id}-enable")))
                    .ghost()
                    .with_size(Size::XSmall)
                    .label("启用")
                    .tooltip("启用并立即激活")
                    .on_click(move |_, _window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            this.set_plugin_enabled(id.clone(), true, cx)
                        });
                    })
            };
            let (state_text, state_color) = plugin_state_label(p.state);
            let state_color = match state_color {
                "success" => cx.theme().colors.success,
                "info" => cx.theme().colors.info,
                "danger" => cx.theme().colors.danger,
                _ => muted,
            };
            list.push(
                div()
                    .id(ElementId::Name(SharedString::from(format!("plugin-row-{}", p.id))))
                    .test_support()
                    .v_flex()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(fg)
                                    .child(SharedString::from(format!("{}  v{}", p.name, p.version))),
                            )
                            .child(div().text_xs().text_color(state_color).child(state_text))
                            .child(div().flex_1())
                            .child(action),
                    )
                    .child(div().text_xs().text_color(muted).child(SharedString::from(p.id.clone()))),
            );
        }

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
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .child("插件"),
                    )
                    .child(refresh_btn),
            )
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .children(list)
                    .when(error_text.is_some(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().colors.danger)
                                .child(SharedString::from(error_text.clone().unwrap_or_default())),
                        )
                    })
                    .when(plugins.is_empty() && error_text.is_none(), |this| {
                        this.child(
                            div()
                                .h_6()
                                .px_2()
                                .text_xs()
                                .text_color(muted)
                                .child("没有安装插件"),
                        )
                    }),
            )
    }
}

impl BasePanel for PluginPanel {
    fn panel_name(&self) -> &'static str {
        "plugin"
    }

    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Plugin);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for PluginPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(LeftPanel::Plugin.label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(LeftPanel::Plugin.icon())
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(LeftPanel::Plugin.label()),
            )
    }

    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        None
    }

}
