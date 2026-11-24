use shared::i18n::tr;
use super::super::*;

use super::delegate::PluginListDelegate;
use super::panel::PluginPanel;
use super::panel_busy;

impl Render for PluginPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let entity = cx.entity();
        let refresh_btn = Button::new("plugin-refresh")
            .ghost()
            .icon(IconName::RefreshCw)
            .with_size(Size::XSmall)
            .tooltip(tr("刷新插件清单与驱动登记"))
            .on_click(move |_, _window, cx| {
                let _ = entity.update(cx, |this, cx| this.refresh_driver_registration(cx));
            });

        // 「管理」入口：左 Dock 只做紧凑清单，搜索 / 筛选 / 详情 / 安装都去中央管理页。
        // 走 `Shared::open_plugin_manager` 宿主回调（与属性面板同形），不在窄栏里再造一份。
        let manager_shared = self.shared.clone();
        let manager_btn = Button::new("plugin-open-manager")
            .ghost()
            .icon(IconName::Settings)
            .with_size(Size::XSmall)
            .tooltip(tr("打开插件管理页（搜索、筛选、详情与安装）"))
            .on_click(move |_, window, cx| {
                manager_shared.show_plugin_manager(window, cx);
            });

        // 清单有变更（热插拔检测到 manifest hash 变化）时才出现：重载是用户明确动作，
        // 不自动热更正在运行的插件。按钮出现即说明「不重载就不生效」。
        let pending = plugin::get_plugin_manager()
            .map(|m| m.pending_reloads().len())
            .unwrap_or(0);
        let reload_btn = (pending > 0).then(|| {
            let reload_entity = cx.entity();
            Button::new("plugin-reload")
                .ghost()
                .label(format!("重载 {pending}"))
                .with_size(Size::XSmall)
                .tooltip(tr("插件文件已变更，重载后生效（会停掉所有插件再重新激活）"))
                .on_click(move |_, _window, cx| {
                    let _ = reload_entity.update(cx, |this, cx| this.reload_plugins(cx));
                })
        });

        // 安装中置灰：解包 + 校验 + 复制在后台跑，期间不接受第二个包（否则并发写
        // 同一个 `plugins/<id>`）。文案收窄成「安装插件 / 从目录」——240px 面板里
        // 标题 + 两个动作 + 刷新刚好一行。
        let busy = panel_busy(&self.progress);
        let install_file_entity = cx.entity();
        let install_file_btn = Button::new("plugin-install-package")
            .ghost()
            .label(tr("安装插件"))
            .with_size(Size::XSmall)
            .disabled(busy)
            .tooltip(tr("从 .zip / .tar.gz 插件包安装"))
            .on_click(move |_, _window, cx| {
                let _ = install_file_entity.update(cx, |this, cx| this.pick_package_file(cx));
            });
        let install_dir_entity = cx.entity();
        let install_dir_btn = Button::new("plugin-install-directory")
            .ghost()
            .label(tr("从目录"))
            .with_size(Size::XSmall)
            .disabled(busy)
            .tooltip(tr("从本地插件目录（含 plugin.toml）导入"))
            .on_click(move |_, _window, cx| {
                let _ = install_dir_entity.update(cx, |this, cx| this.pick_package_dir(cx));
            });

        let error_text = self.error.borrow().clone();
        let notice_text = self.notice.borrow().clone();
        let progress_text = self.progress.borrow().clone();

        // 懒创建 ListState（只建一次——重建会丢掉列表的选中行状态）
        if self.plugins_list.borrow().is_none() {
            let panel_entity = cx.entity();
            let plugins = Rc::clone(&self.plugins);
            let progress = Rc::clone(&self.progress);
            let contributions = Rc::clone(&self.contributions);
            let list = cx.new(move |cx| {
                ListState::new(
                    PluginListDelegate::new(plugins, progress, contributions, panel_entity),
                    window,
                    cx,
                )
            });
            *self.plugins_list.borrow_mut() = Some(list);
        }

        // 同步 delegate 的行数
        if let Some(list_state) = self.plugins_list.borrow().as_ref() {
            let _ = list_state.update(cx, |state, cx| {
                state.delegate_mut().sync();
                cx.notify();
            });
        }

        let list_state = self.plugins_list.borrow().clone();
        let list_element = list_state.map(|ls| List::new(&ls));

        // 空态：插件清单为 0 时整块留白会让「装哪儿 / 从哪装」全靠猜，
        // 用标准 Empty 组件把两个安装入口的用途说清楚（actions 就在头上）。
        let is_empty = self.plugins.borrow().is_empty() && error_text.is_none();
        let empty_state = is_empty.then(|| {
            Empty::new()
                .header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(LeftPanel::Plugin.icon()),
                        )
                        .title(EmptyTitle::new().child(tr("还没有插件")))
                        .description(EmptyDescription::new().child(
                            "「安装插件」装 .zip / .tar.gz 包；「从目录」导入含 plugin.toml 的本地目录",
                        )),
                )
                .into_any_element()
        });
        let body: Option<AnyElement> = match (empty_state, list_element) {
            (Some(empty), _) => Some(empty),
            (None, Some(list)) => Some(list.into_any_element()),
            (None, None) => None,
        };

        div()
            .v_flex()
            .size_full()
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
                            .child(tr("插件")),
                    )
                    .child(install_file_btn)
                    .child(install_dir_btn)
                    .child(manager_btn)
                    .child(refresh_btn),
            )
            // 变更提示单独一行（240px 头部塞不下第四个控件），按钮即「重载」动作
            .children(reload_btn.map(|btn| {
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().colors.info)
                            .child(format!("{pending} 个插件文件有变更，重载后生效")),
                    )
                    .child(btn)
            }))
            .children(error_text.map(|err| {
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().colors.danger)
                    .child(err)
            }))
            .children(notice_text.map(|notice| {
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().colors.success)
                    .child(notice)
            }))
            .children(progress_text.map(|progress| {
                // 安装中是过程态，不是结果回执：用 info 色，完成/失败时被 notice / error 取代
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().colors.info)
                    .child(progress)
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(body, |this, body| this.child(body)),
            )
    }
}

impl BasePanel for PluginPanel {
    fn panel_name(&self) -> &'static str {
        "plugin"
    }

    fn closable(&self, _cx: &App) -> bool {
        // 侧栏 tab 与中央标签一致（2026-11-24 用户验收）：悬停出 ✕、点击关掉这个 tab。
        true
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            tracing::info!(panel = "plugin", "【探针】左 dock Tab 激活：插件");
            let shared = self.shared.clone();
            shared.active_left.set(LeftPanel::Plugin);
            shared.notify_host(cx);
        }
    }
}

impl ComponentPanel for PluginPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        // 返回 None：tab 条用 title() 渲染（图标 + 文字），对齐 VSCode 风格
        None
    }

    /// 【关闭前询问】侧栏 tab 的 ✕：没有脏概念，直接撤登记交还皮肤移除。
    fn request_close(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::dock::CloseRequest {
        self.shared.close_left_panel(LeftPanel::Plugin, cx);
        gpui_kit::component::dock::CloseRequest::Proceed
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
        Some(PanelControl::Menu)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        // 我们自己的面板有自己的工具栏，不需要 Dock 再加顶部 padding
        false
    }

    fn toolbar_buttons(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Vec<Button>> {
        // 业务按钮移到模块内部工具栏，Dock 标题栏只做容器管理。
        None
    }

    fn dropdown_menu(
        &mut self,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> PopupMenu {
        // 业务项已移到模块内部工具栏，Dock 标题栏菜单只留容器管理项。
        menu
    }
}
