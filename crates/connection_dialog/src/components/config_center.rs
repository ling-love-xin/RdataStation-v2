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

use super::config_center_vault::VaultView;
use super::connection_dialog::{
    manager_panel_ui, manager_workspace, prepare_manager_domain, ManagerKind, ManagerWorkspace,
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
    /// 各域条目数（只在域被打开过之后才有值；`None` = 还没查库，导航显示「—」）。
    known: Rc<RefCell<[Option<usize>; 3]>>,
    /// 「密钥来源」域（导航第 4 项：三档选择 + 切档；不属于三管理器 workspace）
    vault: Rc<RefCell<VaultView>>,
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
            known: Rc::new(RefCell::new([None; 3])),
            vault: Rc::new(RefCell::new(VaultView::new(window, cx))),
        };
        prepare_manager_domain(kind, &page.mgr, project_root, window, cx);
        page.remember_count(kind);
        page
    }

    /// 记录当前域条目数（导航 chip 用）。
    fn remember_count(&self, kind: ManagerKind) {
        let n = self.mgr.borrow().items.len();
        self.known.borrow_mut()[kind.index()] = Some(n);
    }

    /// 当前域（宿主「编辑某条档案」时可据此跳到对应页签）。
    pub fn kind(&self) -> ManagerKind {
        self.mgr.borrow().kind
    }

    /// 落到「密钥来源」域（宿主入口 / 测试用；等价于点导航第 4 项）。
    pub fn open_vault(&self) {
        let mut vault = self.vault.borrow_mut();
        vault.open = true;
        vault.refresh();
    }

    /// 「密钥来源」域是否在前台。
    pub fn vault_open(&self) -> bool {
        self.vault.borrow().open
    }

    /// 更新引用计数口径用的项目根（每次从宿主打开时刷新：项目会话可能已切换）。
    pub fn set_project_root(&self, project_root: Option<String>) {
        self.mgr.borrow_mut().project_root = project_root;
    }

    /// 切域（同域再点不重新拉库，避免误清正在编辑的表单）。
    pub fn switch(&self, kind: ManagerKind, window: &mut Window, cx: &mut App) {
        // 从密钥域切回管理器域：先关掉密钥视图（同域再点也不误清表单）
        self.vault.borrow_mut().open = false;
        if self.kind() == kind {
            return;
        }
        let project_root = self.mgr.borrow().project_root.clone();
        prepare_manager_domain(kind, &self.mgr, project_root, window, cx);
        self.remember_count(kind);
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
            let active = kind == cur && !self.vault.borrow().open;
            let mgr = self.mgr.clone();
            let known = self.known.clone();
            let vault = self.vault.clone();
            let notify = notify.clone();
            let mut btn = Button::new(format!("cc-nav-{}", kind.index()))
                .label(tr(label))
                .flex_1();
            btn = if active { btn.primary() } else { btn.ghost() };
            // 计数 chip：只有打开过的域才给真数字，其余显示「—」（没查库就不编造）。
            let count_chip = div()
                .id(format!("cc-count-{}", kind.index()))
                .test_support()
                .px_1()
                .rounded_sm()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(match (*self.known.borrow())[kind.index()] {
                    Some(n) => n.to_string(),
                    None => "—".to_string(),
                });
            nav = nav.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(btn.on_click(move |_, window, app| {
                        vault.borrow_mut().open = false;
                        let project_root = mgr.borrow().project_root.clone();
                        prepare_manager_domain(kind, &mgr, project_root, window, app);
                        known.borrow_mut()[kind.index()] = Some(mgr.borrow().items.len());
                        notify(app);
                    }))
                    .child(count_chip),
            );
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
        // 第四项：密钥来源（不挂在三管理器 workspace 上：它是全局单选，不是列表 + 表单）
        let vault_open = self.vault.borrow().open;
        let current_label = self.vault.borrow().current().label();
        let vault_click = self.vault.clone();
        let notify_vault = notify.clone();
        let mut vault_btn = Button::new("cc-nav-3").label(tr("密钥来源")).flex_1();
        vault_btn = if vault_open {
            vault_btn.primary()
        } else {
            vault_btn.ghost()
        };
        nav = nav.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(vault_btn.on_click(move |_, _window, app| {
                    let mut v = vault_click.borrow_mut();
                    v.open = true;
                    v.refresh();
                    drop(v);
                    notify_vault(app);
                }))
                .child(
                    div()
                        .id("cc-count-3")
                        .test_support()
                        .px_1()
                        .rounded_sm()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(current_label.to_string()),
                ),
        );
        if vault_open {
            nav = nav.child(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(tr("密钥三档：机器绑定 / 系统凭据 / 主密码")),
            );
        }
        let _ = cx;
        nav
    }

    /// 域头统计条（原型三格）：条目数 / 被引用 / 未被引用。
    ///
    /// 只统计**当前域**已拉到的列表——另外两域没查库，所以不冒充「全量数字」。
    fn stats(&self, theme: &Theme) -> Div {
        let (total, referenced) = {
            let m = self.mgr.borrow();
            (
                m.items.len(),
                m.items.iter().filter(|it| it.refs > 0).count(),
            )
        };
        let cell = |id: &'static str, label: &'static str, value: usize, hued: Hsla| {
            div()
                .id(id)
                .test_support()
                .flex_1()
                .v_flex()
                .gap_0p5()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_3()
                .py_2()
                .child(div().text_lg().text_color(hued).child(value.to_string()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(tr(label)),
                )
        };
        div()
            .h_flex()
            .gap_2()
            .child(cell(
                "cc-stat-total",
                "条目",
                total,
                theme.colors.foreground,
            ))
            .child(cell(
                "cc-stat-referenced",
                "被连接引用",
                referenced,
                theme.colors.success,
            ))
            .child(cell(
                "cc-stat-unreferenced",
                "尚未被引用",
                total.saturating_sub(referenced),
                theme.colors.muted_foreground,
            ))
    }

    /// 右栏（面包屑 + 域头 + 三域共用面板体；密钥域走独立正文）。
    pub fn render(
        self: &Rc<Self>,
        notify: &Rc<dyn Fn(&mut App)>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = cx.theme();
        if self.vault.borrow().open {
            let crumb = div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(tr("配置中心"))
                .child(tr("›"))
                .child(tr("密钥来源"));
            let view = self.vault.clone();
            let body = {
                let vault_ref = view.borrow();
                vault_ref.render(notify, &view, &theme)
            };
            let nav = self.nav(notify, &theme, cx);
            return div()
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
                .into_any_element();
        }
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
        let stats = self.stats(&theme);
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
                    .child(stats)
                    .child(body),
            )
            .into_any_element()
    }
}
