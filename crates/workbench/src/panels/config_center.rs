//! 配置中心页宿主面板（中央 Dock 常驻 tab，单实例）。
//!
//! 与插件管理页同范式：面板实体由工作台强持有，tab ✕ 关闭后再次打开仍挂回同一实体，
//! 三域列表 / 表单状态不丢。页面本体在 `rds-connection-dialog`（`ConfigCenterPage`），
//! 本文件只负责 Dock 契约与「切域」的转发。
//!
//! 页面**首次打开时才建**（建页要查一次库拉三域列表）：工作台初始化不为此付一次 DB 往返。

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName};
use gpui_kit::*;

use connection_dialog::{ConfigCenterPage, ManagerKind};

pub struct ConfigCenterPanel {
    /// 页面（首次 `show_domain` 时创建；创建前的空态见 `render`）。
    page: Option<Rc<ConfigCenterPage>>,
    focus_handle: FocusHandle,
    group: Option<gpui::WeakEntity<TabGroup>>,
}

impl ConfigCenterPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            page: None,
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }

    /// 打开时落到指定域（宿主「管理认证 / 网络 / 环境」都走这里）。
    ///
    /// 已在别的域时只切域 + 重绘，不重建上下文（用户可能正在编辑另一域的表单）。
    pub fn show_domain(
        &mut self,
        kind: ManagerKind,
        project_root: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &self.page {
            Some(page) => {
                page.set_project_root(project_root);
                page.switch(kind, window, cx);
            }
            None => {
                self.page = Some(Rc::new(ConfigCenterPage::new(
                    kind,
                    project_root,
                    window,
                    cx,
                )));
            }
        }
        cx.notify();
    }

    /// 当前域（`None` = 页面还没建）。测试与宿主据此判断落点。
    pub fn current_kind(&self) -> Option<ManagerKind> {
        self.page.as_ref().map(|page| page.kind())
    }

    /// 直接落到「密钥来源」域（等价于点导航第 4 项；给宿主入口与测试用）。
    pub fn show_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.page {
            Some(page) => page.open_vault(),
            None => {
                let page = Rc::new(ConfigCenterPage::new(ManagerKind::Auth, None, window, cx));
                page.open_vault();
                self.page = Some(page);
            }
        }
        cx.notify();
    }

    /// 「密钥来源」域是否在前台（测试读它判断落点）。
    pub fn vault_open(&self) -> bool {
        self.page.as_ref().is_some_and(|page| page.vault_open())
    }
}

impl EventEmitter<BasePanelEvent> for ConfigCenterPanel {}

impl Focusable for ConfigCenterPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ConfigCenterPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(page) = self.page.clone() else {
            let theme = cx.theme();
            return div()
                .v_flex()
                .gap_2()
                .p_4()
                .child(
                    div()
                        .text_sm()
                        .child("配置中心尚未打开任何域：从连接对话框的「管理」入口进入。"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("认证档案 / 网络代理 / 环境策略三域共用本页。"),
                )
                .into_any_element();
        };
        let entity = cx.entity();
        let notify: Rc<dyn Fn(&mut App)> = Rc::new(move |app: &mut App| {
            entity.update(app, |_, cx| cx.notify());
        });
        page.render(&notify, window, cx)
    }
}

impl BasePanel for ConfigCenterPanel {
    fn panel_name(&self) -> &'static str {
        "config-center"
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

impl ComponentPanel for ConfigCenterPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("配置中心".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .child(Icon::new(IconName::Settings))
            .child("配置中心")
    }
}

/// 把配置中心 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**（同 `focus_plugin_manager_tab`：`TabGroup` 切 tab 会回读
/// 面板实体，在更新闭包内调用会 double lease panic）。
pub fn focus_config_center_tab(
    panel: &Entity<ConfigCenterPanel>,
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
