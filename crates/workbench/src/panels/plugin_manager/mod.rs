//! 插件管理页（中央常驻 tab，对齐 `docs/architecture/plugin/prototype/plugin-manager.html`）。
//!
//! 目录结构（避免单文件继续膨胀，后续新增页内区块按文件落位）：
//! - `mod.rs`：面板协议、状态字段、动作转发（刷新 / 安装 / 启停 / 重试 / 卸载 / 重载）；
//! - `render.rs`：整页骨架（标题行 / 统计 / 反馈 / 筛选 / 滚动区 / 右侧详情栏）；
//! - `cards.rs`：两列卡片网格与单卡；
//! - `detail.rs`：右侧固定详情栏；
//! - `widgets.rs`：筛选 / 统计 / 状态文案等纯函数与动作按钮；
//! - `tests.rs`：纯函数单测（不渲染窗口）。
//!
//! 数据面不在本面板复制一份：左 Dock 的 `PluginPanel` 已经持有会话清单、
//! 贡献点计数与后台动作。本面板通过 `Entity<PluginPanel>` 的只读快照 + 动作
//! 转发复用同一条链路，避免「管理页和左栏各刷一份、各自过期」。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use plugin::ContributionCounts;

use super::left_tabs::PluginPanel;
use widgets::PluginFilter;

mod cards;
mod detail;
mod render;
#[cfg(test)]
mod tests;
mod widgets;

/// 插件管理页：中央 Dock 独立 tab（与属性面板 / 搜索结果同级）。
pub struct PluginManagerPanel {
    /// 左 Dock 插件面板实体：清单 / 贡献点 / 后台动作的唯一数据面。
    plugin_panel: Entity<PluginPanel>,
    focus_handle: FocusHandle,
    /// 所在 tab 组（`focus_plugin_manager_tab` 用；被 dock 移除后可能失效）。
    group: Option<gpui::WeakEntity<TabGroup>>,
    /// 搜索关键词（Input 变更时写入；render 只读）。
    query: Rc<RefCell<String>>,
    /// 搜索框实体（首帧懒创建）。
    search_input: Option<Entity<InputState>>,
    /// 搜索框变更订阅（持有即生效）。
    _search_sub: Option<Subscription>,
    /// 状态筛选（全部 / 已启用 / 已停用 / 出错）。
    filter: Rc<Cell<PluginFilter>>,
    /// 当前选中插件 id（右栏详情来源；`None` 时回退到筛选结果第一条）。
    selected: Rc<RefCell<Option<String>>>,
    /// 观察左 Dock 插件面板：刷新 / 动作回执 / 事件刷新都会 notify 本页。
    _plugin_sub: Option<Subscription>,
}

/// 一次渲染用到的插件数据快照（避免在渲染子树里反复 lease 面板实体）。
pub(crate) struct PluginSnapshot {
    pub plugins: Vec<plugin::PluginInfo>,
    pub contributions: HashMap<String, ContributionCounts>,
    pub progress: Option<String>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl PluginManagerPanel {
    pub fn new(plugin_panel: Entity<PluginPanel>, cx: &mut Context<Self>) -> Self {
        let _plugin_sub = cx.observe(&plugin_panel, |_, _, cx| cx.notify());
        Self {
            plugin_panel,
            focus_handle: cx.focus_handle(),
            group: None,
            query: Rc::new(RefCell::new(String::new())),
            search_input: None,
            _search_sub: None,
            filter: Rc::new(Cell::new(PluginFilter::All)),
            selected: Rc::new(RefCell::new(None)),
            _plugin_sub: Some(_plugin_sub),
        }
    }

    /// 读取左 Dock 插件面板的当前快照（克隆，立即释放 RefCell 借用）。
    pub(crate) fn snapshot(&self, cx: &App) -> PluginSnapshot {
        let panel = self.plugin_panel.read(cx);
        PluginSnapshot {
            plugins: panel.plugins_snapshot(),
            contributions: panel.contributions_snapshot(),
            progress: panel.progress_text(),
            error: panel.error_text(),
            notice: panel.notice_text(),
        }
    }

    pub(crate) fn query(&self) -> String {
        self.query.borrow().clone()
    }

    pub(crate) fn filter(&self) -> PluginFilter {
        self.filter.get()
    }

    pub(crate) fn selected_id(&self) -> Option<String> {
        self.selected.borrow().clone()
    }

    pub(crate) fn set_filter(&self, filter: PluginFilter, cx: &mut Context<Self>) {
        self.filter.set(filter);
        cx.notify();
    }

    pub(crate) fn select(&self, id: String, cx: &mut Context<Self>) {
        *self.selected.borrow_mut() = Some(id);
        cx.notify();
    }

    /// 懒创建搜索框并订阅输入变更（只写 query + 重绘，不做重活）。
    pub(crate) fn ensure_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.search_input {
            return input.clone();
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索插件名称 / ID / 描述…"));
        let query = self.query.clone();
        let sub = cx.subscribe_in(
            &input,
            window,
            move |_this: &mut Self, _: &Entity<InputState>, ev: &InputEvent, _window, cx| {
                if let InputEvent::Change = ev {
                    // 输入事件不带值：从 InputState 读取当前文本。
                    // 这里不用 `this.search_input`，避免回调里再次借用 self。
                    if let Some(input) = _this.search_input.as_ref() {
                        *query.borrow_mut() = input.read(cx).value().to_string();
                        cx.notify();
                    }
                }
            },
        );
        self.search_input = Some(input.clone());
        self._search_sub = Some(sub);
        input
    }

    pub(crate) fn refresh(&self, cx: &mut Context<Self>) {
        self.plugin_panel
            .update(cx, |panel, cx| panel.refresh_plugins(cx, true));
    }

    pub(crate) fn install_file(&self, cx: &mut Context<Self>) {
        self.plugin_panel
            .update(cx, |panel, cx| panel.pick_package_file(cx));
    }

    pub(crate) fn install_dir(&self, cx: &mut Context<Self>) {
        self.plugin_panel
            .update(cx, |panel, cx| panel.pick_package_dir(cx));
    }

    pub(crate) fn reload(&self, cx: &mut Context<Self>) {
        self.plugin_panel
            .update(cx, |panel, cx| panel.reload_plugins(cx));
    }

    /// 从驱动冲突告警直接停用某个插件（避免用户还要去列表里找）。
    pub(crate) fn disable_plugin(&self, id: String, cx: &mut Context<Self>) {
        self.plugin_panel
            .update(cx, |panel, cx| panel.set_plugin_enabled(id, false, cx));
    }

    pub(crate) fn plugin_entity(&self) -> Entity<PluginPanel> {
        self.plugin_panel.clone()
    }
}

impl EventEmitter<BasePanelEvent> for PluginManagerPanel {}

impl Focusable for PluginManagerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PluginManagerPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_page(window, cx)
    }
}

impl BasePanel for PluginManagerPanel {
    fn panel_name(&self) -> &'static str {
        "plugin-manager"
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

impl ComponentPanel for PluginManagerPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("插件管理".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .child(crate::view::LeftPanel::Plugin.icon())
            .child("插件管理")
    }
}

/// 把插件管理 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**（同 `focus_property_tab` / `focus_search_tab`：
/// `TabGroup` 切 tab 会回读面板实体，在更新闭包内调用会 double lease panic）。
pub fn focus_plugin_manager_tab(
    panel: &Entity<PluginManagerPanel>,
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
