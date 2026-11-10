use super::super::*;

use super::panel::PluginPanel;
use super::{
    contribution_breakdown, panel_busy, plugin_row_subtitle, plugin_state_label,
    uninstall_confirm_description,
};

pub(super) struct PluginListDelegate {
    /// 插件数据引用（与 PluginPanel 共享）
    plugins: Rc<RefCell<Vec<PluginInfo>>>,
    /// 面板当前进行中的操作（`Some` 即忙：行内启停 / 卸载 / 重试一并置灰）
    progress: Rc<RefCell<Option<String>>>,
    /// 每个插件的贡献点计数（刷新时随清单一起更新；行上显示「贡献 N」）
    contributions: Rc<RefCell<HashMap<String, ContributionCounts>>>,
    /// PluginPanel 实体引用（用于调用 set_plugin_enabled）
    entity: Entity<PluginPanel>,
    /// 行数缓存（渲染期同步）
    len: usize,
}

impl PluginListDelegate {
    pub(super) fn new(
        plugins: Rc<RefCell<Vec<PluginInfo>>>,
        progress: Rc<RefCell<Option<String>>>,
        contributions: Rc<RefCell<HashMap<String, ContributionCounts>>>,
        entity: Entity<PluginPanel>,
    ) -> Self {
        Self {
            plugins,
            progress,
            contributions,
            entity,
            len: 0,
        }
    }

    /// 渲染期同步：更新行数缓存。
    pub(super) fn sync(&mut self) {
        self.len = self.plugins.borrow().len();
    }
}

impl ListDelegate for PluginListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _: &App) -> usize {
        self.len
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let plugins = self.plugins.borrow();
        let p = plugins.get(ix.row)?;
        let id = p.id.clone();
        let name = p.name.clone();
        let version = p.version.clone();
        let counts = self.contributions.borrow().get(&id).copied();
        let active = p.state == PluginState::Active;
        let error = p.error.clone();
        let (state_text, state_color) = plugin_state_label(p.state);
        let state_color = match state_color {
            "success" => cx.theme().colors.success,
            "info" => cx.theme().colors.info,
            "danger" => cx.theme().colors.danger,
            _ => cx.theme().colors.muted_foreground,
        };
        // 行上只放总数，明细进悬停卡片：240px 面板塞不下「命令 2 · 面板 1 · 设置 1」。
        // 驱动型插件同样有贡献（驱动不在注册表里，但仍算插件声明）。
        let contribution_hint = counts.filter(|counts| !counts.is_empty()).map(|counts| {
            let muted = cx.theme().colors.muted_foreground;
            HoverCard::new(ElementId::Name(SharedString::from(format!(
                "plugin-{id}-contributions"
            ))))
            .open_delay(Duration::from_millis(300))
            .trigger(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(SharedString::from(format!("贡献 {}", counts.total()))),
            )
            .content(move |_, _, cx| {
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.foreground)
                    .child(SharedString::from(contribution_breakdown(counts)))
            })
        });

        // 启用/停用按钮（行尾）。清单都没解析出来的目录没有可激活的运行时：
        // 给「重试」而不是「启用」——修好清单或卸载冲突插件后，重试是唯一不重启的恢复入口。
        // 任一操作进行中（安装 / 重载 / 启停 / 卸载 / 重试）都置灰：启停与卸载也会动
        // 同一个 `plugins/<id>` 目录与进程，不能和安装并发。
        let busy = panel_busy(&self.progress);
        let entity = self.entity.clone();
        let action_id = id.clone();
        let button = if p.kind == PluginKind::Unknown {
            Button::new(SharedString::from(format!("plugin-{id}-retry")))
                .ghost()
                .with_size(Size::XSmall)
                .label("重试")
                .tooltip("重新解析清单并加载（修好清单或卸载冲突插件后使用）")
                .disabled(busy)
                .on_click(move |_, _window, app| {
                    let _ = entity.update(app, |this, cx| this.retry_plugin(action_id.clone(), cx));
                    app.stop_propagation();
                })
        } else if active {
            Button::new(SharedString::from(format!("plugin-{id}-disable")))
                .ghost()
                .with_size(Size::XSmall)
                .label("停用")
                .tooltip("停用后本会话立即停止运行（进程停用），下次启动不再激活")
                .disabled(busy)
                .on_click(move |_, _window, app| {
                    let _ = entity.update(app, |this, cx| {
                        this.set_plugin_enabled(action_id.clone(), false, cx)
                    });
                    app.stop_propagation();
                })
        } else {
            Button::new(SharedString::from(format!("plugin-{id}-enable")))
                .ghost()
                .with_size(Size::XSmall)
                .label("启用")
                .tooltip("启用并立即激活")
                .disabled(busy)
                .on_click(move |_, _window, app| {
                    let _ = entity.update(app, |this, cx| {
                        this.set_plugin_enabled(action_id.clone(), true, cx)
                    });
                    app.stop_propagation();
                })
        };

        // 卸载按钮（行尾，破坏性动作放最后）：先确认再落盘删除。
        // 确认框要 Window（`open_alert_dialog`），只能在点击回调里开；真正删除走
        // 面板方法（后台 runtime + tokio，完成后刷新清单）。
        let uninstall_entity = self.entity.clone();
        let uninstall_id = id.clone();
        let uninstall_description = uninstall_confirm_description(&name, &id);
        let uninstall = Button::new(SharedString::from(format!("plugin-{id}-uninstall")))
            .ghost()
            .with_size(Size::XSmall)
            .label("卸载")
            .tooltip("从磁盘删除该插件（保留 plugin-data 用户数据）")
            .disabled(busy)
            .on_click(move |_, window, app| {
                let entity = uninstall_entity.clone();
                let id = uninstall_id.clone();
                let description = uninstall_description.clone();
                window.open_alert_dialog(app, move |alert, _window, _cx| {
                    let entity = entity.clone();
                    let id = id.clone();
                    alert
                        .confirm()
                        .title("卸载插件")
                        .description(SharedString::from(description.clone()))
                        .ok_text("卸载")
                        .cancel_text("取消")
                        .ok_variant(ButtonVariant::Danger)
                        .on_ok(move |_window, _cx, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.uninstall_plugin(id.clone(), cx);
                            });
                            true
                        })
                });
                app.stop_propagation();
            });

        // 出错行把原因并进第二行：240px 面板放不下第三行，完整堆栈仍看日志。
        let subtitle = plugin_row_subtitle(&id, error.as_deref());
        let subtitle_color = if error.is_some() {
            cx.theme().colors.danger
        } else {
            cx.theme().colors.muted_foreground
        };

        let item = ListItem::new(ElementId::Name(SharedString::from(format!(
            "plugin-row-{id}"
        ))))
        .h(rems(3.))
        .px_2()
        .rounded(cx.theme().radius)
        .child(
            div()
                .v_flex()
                .gap_1()
                .flex_1()
                .min_w(rems(0.))
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(cx.theme().colors.foreground)
                                .child(SharedString::from(format!("{name}  v{version}"))),
                        )
                        .child(div().text_xs().text_color(state_color).child(state_text))
                        .when_some(contribution_hint, |row, hint| row.child(hint))
                        .child(button),
                )
                .child(
                    // 第二行 = id + 卸载：破坏性动作与启停分开放，
                    // 240px 面板里一行塞四个控件会互相挤扁
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .text_xs()
                                .text_color(subtitle_color)
                                .child(subtitle),
                        )
                        .child(uninstall),
                ),
        );

        Some(item)
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child("没有安装插件")
    }

    fn set_selected_index(
        &mut self,
        _ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        // 插件列表不需要选中态（每行都有独立按钮）
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) {
        // 插件列表不需要 confirm 动作
    }
}
