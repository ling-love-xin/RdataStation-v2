//! 配置中心页：认证 / 网络 / 环境三域的页级容器。
//!
//! 形态依据 `docs/architecture/connection/prototypes/config-center.html`
//! （左域导航 + 右内容；列表与表单复用 `managers::manager_panel_ui` 同一份实现）。
//! 旧的三管理器内层对话框在迁移完成前保留为兜底，两处渲染同一份面板体。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::*;

use super::connection_dialog::{
    ManagerKind, ManagerWorkspace, manager_panel_ui, manager_workspace, prepare_manager_domain,
};
use shared::i18n::tr;

/// 域的显示口径：导航短名、表名（面包屑用）。
fn domain_meta(kind: ManagerKind) -> (&'static str, &'static str) {
    match kind {
        ManagerKind::Auth => ("认证档案", "auth_configs"),
        ManagerKind::Network => ("网络代理", "network_configs"),
        ManagerKind::Env => ("环境策略", "environments + environment_policies"),
    }
}

/// 配置中心页状态（工作台面板持有；`Rc` 与对话框同路，窗口参与输入实体构造）。
pub struct ConfigCenterPage {
    mgr: Rc<RefCell<ManagerWorkspace>>,
}

impl ConfigCenterPage {
    /// 建页并落到指定域（列表 / 类型下拉在这一步拉好，首帧就有内容）。
    pub fn new(
        kind: ManagerKind,
        project_root: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let page = Self {
            mgr: Rc::new(RefCell::new(manager_workspace(window, cx))),
        };
        prepare_manager_domain(kind, &page.mgr, project_root, window, cx);
        page
    }

    /// 当前域（宿主「编辑某条档案」时可据此跳到对应页签）。
    pub fn kind(&self) -> ManagerKind {
        self.mgr.borrow().kind
    }

    /// 更新引用计数口径用的项目根（每次从宿主打开时刷新：项目会话可能已切换）。
    pub fn set_project_root(&self, project_root: Option<String>) {
        self.mgr.borrow_mut().project_root = project_root;
    }

    /// 切域（同域再点不重新拉库，避免误清正在编辑的表单）。
    pub fn switch(&self, kind: ManagerKind, window: &mut Window, cx: &mut App) {
        if self.kind() == kind {
            return;
        }
        let project_root = self.mgr.borrow().project_root.clone();
        prepare_manager_domain(kind, &self.mgr, project_root, window, cx);
    }

    /// 左栏（三域导航）。
    fn nav(&self, notify: &Rc<dyn Fn(&mut App)>, theme: &Theme, cx: &App) -> Div {
        let cur = self.kind();
        let mut nav = div()
            .v_flex()
            .gap_1()
            .w(rems(13.))
            .flex_shrink_0()
            .p_2()
            .border_r_1()
            .border_color(theme.colors.border)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(tr("配置中心")),
            );
        for kind in ManagerKind::ALL {
            let (label, _) = domain_meta(kind);
            let active = kind == cur;
            let mgr = self.mgr.clone();
            let notify = notify.clone();
            let mut btn = Button::new(format!("cc-nav-{}", kind.index())).label(tr(label));
            btn = if active { btn.primary() } else { btn.ghost() };
            nav = nav.child(btn.on_click(move |_, window, app| {
                let project_root = mgr.borrow().project_root.clone();
                prepare_manager_domain(kind, &mgr, project_root, window, app);
                notify(app);
            }));
            // 引用口径提示（原型：导航项下方一行小字说明该域管什么）。
            if active {
                nav = nav.child(
                    div()
                        .px_2()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(tr(match kind {
                            ManagerKind::Auth => "凭据档案，可被多个连接引用",
                            ManagerKind::Network => "SSH / 代理 / SSL / 链，可被复用",
                            ManagerKind::Env => "环境及其执行策略",
                        })),
                );
            }
        }
        let _ = cx;
        nav
    }

    /// 右栏（面包屑 + 域头 + 三域共用面板体）。
    pub fn render(
        &self,
        notify: &Rc<dyn Fn(&mut App)>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        let kind = self.kind();
        let (label, table) = domain_meta(kind);
        let nav = self.nav(notify, &theme, cx);
        let crumb = div()
            .h_flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child(tr("配置中心"))
            .child(tr("›"))
            .child(tr(label))
            .child(
                div()
                    .ml_2()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.colors.border)
                    .child(table),
            );
        let body = manager_panel_ui(kind, &self.mgr, notify, window, cx);

        div()
            .h_flex()
            .size_full()
            .child(nav)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .p_4()
                    .child(crumb)
                    .child(body),
            )
            .into_any_element()
    }
}
