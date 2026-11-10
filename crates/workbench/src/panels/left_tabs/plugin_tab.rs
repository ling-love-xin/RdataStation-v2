use super::*;

struct PluginListDelegate {
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
    fn new(
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
    fn sync(&mut self) {
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

/// 插件 tab：已装插件清单（名称 / 版本 / 状态）+ 刷新 + 启用 / 停用 / 卸载。
///
/// 数据面 = 全局 `PluginManager`（`plugin::manager`，app 启动时初始化）；会话态清单
/// 由事件路径刷新（render 纯读）：首次 `new` 与「刷新 / 启停 / 卸载」操作后重新拉取。
/// 启停与卸载是异步管理操作（依赖 tokio 上下文），经全局 runtime 句柄在后台执行，不阻塞 UI。
pub struct PluginPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 会话态插件清单（`plugin::manager::PluginInfo`，事件路径写入）
    plugins: Rc<RefCell<Vec<PluginInfo>>>,
    /// 刷新进行中（防重复触发）
    loading: Rc<Cell<bool>>,
    /// 刷新进行中又收到刷新请求：结束后再拉一次，避免事件被 `loading` 直接吞掉
    refresh_again: Rc<Cell<bool>>,
    /// 最近一次操作的错误文案（失败展示，成功即清）
    error: Rc<RefCell<Option<String>>>,
    /// 最近一次操作的成功回执（与 `error` 互斥，刷新时不清）
    notice: Rc<RefCell<Option<String>>>,
    /// 安装进行中的提示（`Some` 即忙：按钮置灰、防重复触发；完成后清空）
    progress: Rc<RefCell<Option<String>>>,
    /// 每个插件的贡献点计数（与 `plugins` 同一次刷新更新）
    contributions: Rc<RefCell<HashMap<String, ContributionCounts>>>,
    /// 插件列表状态（懒创建）
    plugins_list: Rc<RefCell<Option<Entity<ListState<PluginListDelegate>>>>>,
}

impl PluginPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        let this = Self {
            shared,
            focus_handle: cx.focus_handle(),
            plugins: Rc::new(RefCell::new(Vec::new())),
            loading: Rc::new(Cell::new(false)),
            refresh_again: Rc::new(Cell::new(false)),
            error: Rc::new(RefCell::new(None)),
            notice: Rc::new(RefCell::new(None)),
            progress: Rc::new(RefCell::new(None)),
            contributions: Rc::new(RefCell::new(HashMap::new())),
            plugins_list: Rc::new(RefCell::new(None)),
        };
        this.refresh_plugins(cx, true);
        Self::subscribe_events(cx);
        this
    }

    /// 订阅插件生命周期事件：热插拔扫描 / 安装 / 卸载 / 启停都不经过面板按钮，
    /// 不订阅就只能靠用户手动「刷新」才看到列表变化（`events.rs` 此前零宿主订阅方）。
    fn subscribe_events(cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let mut receiver = plugin::events::get_event_manager().subscribe();
        let _ = cx.spawn(async move |_this, cx| {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if !event_touches_plugin_list(&event) {
                            continue;
                        }
                        let _ = weak.update(cx, |this, cx| this.refresh_plugins(cx, false));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        // 慢消费者丢事件时不能静默停掉循环：强制再拉一次当前清单。
                        tracing::warn!(dropped, "插件面板事件订阅落后，重新拉取插件清单");
                        let _ = weak.update(cx, |this, cx| this.refresh_plugins(cx, false));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// 只读快照：中央插件管理页与左 Dock 共用同一份会话清单，
    /// 避免管理页自己再拉一份、两份各自过期。
    pub(crate) fn plugins_snapshot(&self) -> Vec<PluginInfo> {
        self.plugins.borrow().clone()
    }

    pub(crate) fn contributions_snapshot(&self) -> HashMap<String, ContributionCounts> {
        self.contributions.borrow().clone()
    }

    pub(crate) fn progress_text(&self) -> Option<String> {
        self.progress.borrow().clone()
    }

    pub(crate) fn error_text(&self) -> Option<String> {
        self.error.borrow().clone()
    }

    pub(crate) fn notice_text(&self) -> Option<String> {
        self.notice.borrow().clone()
    }

    /// 重新拉取插件清单（后台执行 `list_plugins`，回主线程写状态）。
    ///
    /// `clear_error`：下拉成功时是否清掉已有错误。操作路径传 `false`——否则一次
    /// 失败的操作会被随后成功的 `list_plugins` 把错误文案擦掉，用户看不到原因。
    pub(crate) fn refresh_plugins(&self, cx: &mut Context<Self>, clear_error: bool) {
        if self.loading.get() {
            // 刷新是一次异步快照读取：期间来的事件可能读不到最新清单，记一个补刷。
            self.refresh_again.set(true);
            return;
        }
        self.loading.set(true);
        self.refresh_again.set(false);
        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            // list_plugins 是同步快照读取；放进后台线程避免任何潜在 I/O 阻塞 UI。
            // 贡献点计数是展示用的补充信息：注册表异常（理论上已被加载期冲突检查挡住）
            // 不该让整个清单刷新失败，失败时按空计数处理。
            let outcome = plugin::get_plugin_manager()
                .map(|mgr| {
                    let counts = mgr.contribution_counts().unwrap_or_default();
                    mgr.list_plugins().map(|items| (items, counts))
                })
                .transpose();
            let _ = weak.update(cx, |this, cx| {
                this.loading.set(false);
                match outcome {
                    Ok(None) => {
                        this.error.replace(Some("插件运行时未初始化".to_string()));
                    }
                    Ok(Some((items, counts))) => {
                        this.plugins.replace(items);
                        this.contributions.replace(counts);
                        if clear_error {
                            this.error.replace(None);
                        }
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                    }
                };
                cx.notify();
                if this.refresh_again.replace(false) {
                    this.refresh_plugins(cx, false);
                }
            });
        });
    }

    /// 重试一个加载失败的插件（错误行上的「重试」）。
    ///
    /// 冲突拒绝时清单本身没变：等冲突方卸载后，热插拔扫描不会把它当成「新增 / 变化」
    /// 自动重试，所以必须给用户一个显式入口，否则只能重启应用才恢复。
    pub(crate) fn retry_plugin(&self, id: String, cx: &mut Context<Self>) {
        if panel_busy(&self.progress) {
            return;
        }
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
        self.progress.replace(Some(format!("正在重试 {id}…")));
        self.error.replace(None);
        self.notice.replace(None);
        cx.notify();

        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            let outcome = handle.block_on(async { mgr.retry_load_plugin(&id).await });
            let _ = weak.update(cx, |this, cx| {
                this.progress.replace(None);
                match outcome {
                    Ok(loaded) => {
                        this.error.replace(None);
                        this.notice.replace(Some(format!(
                            "已重新加载插件「{}」",
                            loaded.manifest.plugin.name
                        )));
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                        this.notice.replace(None);
                    }
                };
                this.refresh_plugins(cx, false);
            });
        });
    }

    /// 启停插件（`enable_plugin` / `disable_plugin` 依赖 tokio 上下文：DB 异步锁 +
    /// sidecar 进程停启）。经全局 runtime 句柄在后台 `block_on`，完成后回主线程刷新。
    pub(crate) fn set_plugin_enabled(&self, id: String, enabled: bool, cx: &mut Context<Self>) {
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
        // 启停会写 global.db 并停 / 起进程；同一时间只允许一个操作，避免连点
        // 造成 disable 与 enable 交错、最终状态取决于完成顺序。
        if panel_busy(&self.progress) {
            return;
        }
        let action = if enabled { "启用" } else { "停用" };
        self.progress.replace(Some(format!("正在{action} {id}…")));
        self.error.replace(None);
        self.notice.replace(None);
        cx.notify();

        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            // block_on 跑在 gpui 后台线程（cx.spawn 的 executor），不阻塞 UI 线程
            let outcome = if enabled {
                handle.block_on(async { mgr.enable_plugin(&id).await })
            } else {
                handle.block_on(async { mgr.disable_plugin(&id).await })
            };
            let _ = weak.update(cx, |this, cx| {
                this.progress.replace(None);
                match outcome {
                    Ok(()) => {
                        this.error.replace(None);
                        this.notice.replace(Some(if enabled {
                            format!("已启用 {id}")
                        } else {
                            format!("已停用 {id}")
                        }));
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                        this.notice.replace(None);
                    }
                };
                this.refresh_plugins(cx, false);
            });
        });
    }

    /// 卸载插件（磁盘删除，保留 `plugin-data` 用户数据）。
    ///
    /// 依赖检查 / 停进程 / 删目录 / 清登记全在 `manager::uninstall_plugin` 里；
    /// 被其他插件依赖时会返回带引用者清单的错误，这里只需展示。
    pub(crate) fn uninstall_plugin(&self, id: String, cx: &mut Context<Self>) {
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
        // 卸载会停进程、删 `plugins/<id>/`；必须与安装 / 启停互斥，否则可能删掉
        // 正在换入的目标目录。
        if panel_busy(&self.progress) {
            return;
        }
        self.progress.replace(Some(format!("正在卸载 {id}…")));
        self.error.replace(None);
        self.notice.replace(None);
        cx.notify();

        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            let outcome = handle.block_on(async { mgr.uninstall_plugin(&id).await });
            let _ = weak.update(cx, |this, cx| {
                this.progress.replace(None);
                match outcome {
                    Ok(()) => {
                        this.error.replace(None);
                        this.notice.replace(Some(format!("已卸载 {id}")));
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                        this.notice.replace(None);
                    }
                };
                this.refresh_plugins(cx, false);
            });
        });
    }

    /// 选一个插件包文件（`.zip` / `.tar.gz`）安装。
    pub(crate) fn pick_package_file(&self, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择插件包")
            .add_filter("插件包", &["zip", "gz"])
            .pick_file()
        else {
            return;
        };
        self.install_package(path, cx);
    }

    /// 选一个插件目录（含 `plugin.toml`）导入（开发目录 / 解压后的包）。
    pub(crate) fn pick_package_dir(&self, cx: &mut Context<Self>) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("选择插件目录（含 plugin.toml）")
            .pick_folder()
        else {
            return;
        };
        self.install_package(path, cx);
    }

    /// 全量重载插件（VS Code「Reload Window」口径）。
    ///
    /// 热插拔只把「清单变了」记进 `pending_reloads`，不自动热更正在运行的插件；
    /// 没有这个入口，用户改完插件文件只能重启应用才生效，`pending_reloads` 也永远没人消费。
    pub(crate) fn reload_plugins(&self, cx: &mut Context<Self>) {
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
        if panel_busy(&self.progress) {
            return;
        }
        self.progress.replace(Some("正在重载插件…".to_string()));
        self.error.replace(None);
        self.notice.replace(None);
        cx.notify();

        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            let outcome = handle.block_on(async { mgr.reload_plugins().await });
            let _ = weak.update(cx, |this, cx| {
                this.progress.replace(None);
                match outcome {
                    Ok(diff) => {
                        this.error.replace(None);
                        this.notice
                            .replace(Some(format!("已重载插件（加载 {} 个）", diff.added.len())));
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                        this.notice.replace(None);
                    }
                };
                this.refresh_plugins(cx, false);
            });
        });
    }

    /// 安装 / 导入插件包。解包、校验、复制都是磁盘操作（大包可达数十 MB）：
    /// 放后台线程跑，UI 只显示「正在安装 …」，完成后再回主线程写回执并刷新清单。
    ///
    /// 安装目录与 global.db 由 `plugin::installer::install_from_path` 内部解析
    /// （宿主不复制 `plugins_dir()` 这类目录约定）；来源检查也在那里做——
    /// 选中 `plugins/<id>` 自身会被拒（否则等于先删源目录再复制）。
    fn install_package(&self, package_path: PathBuf, cx: &mut Context<Self>) {
        if panel_busy(&self.progress) {
            return;
        }
        let Some(handle) = plugin::runtime_handle().cloned() else {
            self.error.replace(Some("插件运行时未初始化".to_string()));
            cx.notify();
            return;
        };

        let label = package_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("插件包")
            .to_string();
        self.progress.replace(Some(format!("正在安装 {label}…")));
        self.error.replace(None);
        self.notice.replace(None);
        cx.notify();

        let weak = cx.entity().downgrade();
        let work = cx.background_executor().spawn(async move {
            handle.block_on(async { plugin::installer::install_from_path(&package_path).await })
        });
        let _ = cx.spawn(async move |_this, cx| {
            let outcome = work.await;
            let _ = weak.update(cx, |this, cx| {
                this.progress.replace(None);
                match outcome {
                    Ok(plugin) => {
                        this.error.replace(None);
                        // 回执要如实：停用的插件装完仍停用、激活失败要说失败——
                        // 用户不会去翻日志，只写「已安装」等于把问题藏起来。
                        let state = plugin::get_plugin_manager()
                            .and_then(|m| m.get_loaded_plugin(&plugin.code))
                            .map(|p| p.status.session_state());
                        this.notice.replace(Some(install_receipt(
                            &plugin.name,
                            &plugin.version,
                            &plugin.code,
                            plugin.is_enabled,
                            state,
                        )));
                    }
                    Err(e) => {
                        this.error.replace(Some(format!("{e}")));
                        this.notice.replace(None);
                    }
                };
                this.refresh_plugins(cx, false);
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

/// 面板是否有进行中的操作（纯函数，便于单测）。
///
/// `progress` 既是状态栏文案也是忙碌标记：安装 / 重载 / 启停 / 卸载 / 重试任一在跑，
/// 其它入口都必须拒绝或置灰——它们会同时改 `plugins/<id>` 目录或进程状态。
pub(super) fn panel_busy(progress: &Rc<RefCell<Option<String>>>) -> bool {
    progress.borrow().is_some()
}

/// 贡献点明细（纯函数，便于单测）：只列非零类别，驱动也算贡献。
pub(super) fn contribution_breakdown(counts: ContributionCounts) -> String {
    let mut parts = Vec::new();
    if counts.commands > 0 {
        parts.push(format!("命令 {}", counts.commands));
    }
    if counts.panels > 0 {
        parts.push(format!("面板 {}", counts.panels));
    }
    if counts.settings > 0 {
        parts.push(format!("设置 {}", counts.settings));
    }
    if counts.drivers > 0 {
        parts.push(format!("驱动 {}", counts.drivers));
    }
    parts.join(" · ")
}

/// 插件行第二行文案（纯函数，便于单测）：无错误时只显示 id；
/// 出错时把原因接在后面（`id · 原因：…`），面板行高固定 3rem，放不下第三行。
pub(super) fn plugin_row_subtitle(id: &str, error: Option<&str>) -> String {
    match error {
        Some(reason) => format!("{id} · 原因：{reason}"),
        None => id.to_string(),
    }
}

/// 安装回执（纯函数，便于单测）：把「库里的启用状态 + 运行时状态」翻成一句人话。
///
/// 覆盖安装 / 重装会沿用库里的 `is_enabled`：停用中的插件装完仍停用，回执不能只说
/// 「已安装」；激活失败（`PluginState::Error`，含 `ActivationFailed` / `LoadFailed`）
/// 也要在这里说，否则用户以为装好了却连不上。
pub(super) fn install_receipt(
    name: &str,
    version: &str,
    code: &str,
    is_enabled: bool,
    state: Option<PluginState>,
) -> String {
    if !is_enabled {
        format!("已安装 {name} v{version}（{code}，保持停用）")
    } else {
        match state {
            Some(PluginState::Error) => {
                format!("已安装 {name} v{version}（{code}），但加载或激活失败，详见日志")
            }
            Some(PluginState::Active) => format!("已安装并启用 {name} v{version}（{code}）"),
            _ => format!("已安装 {name} v{version}（{code}）"),
        }
    }
}

/// 该事件是否可能改变插件列表（纯函数，便于单测）。
///
/// 生命周期事件都会改列表或状态；`Custom` 是插件自定义事件，与面板清单无关，
/// 订阅侧不能因为它每来一条就拉一次 `list_plugins`。
pub(super) fn event_touches_plugin_list(event: &plugin::events::PluginEvent) -> bool {
    !matches!(event, plugin::events::PluginEvent::Custom { .. })
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

/// 卸载确认框正文（纯函数，便于单测）：说清删什么、留什么、什么情况会被拒。
pub(crate) fn uninstall_confirm_description(name: &str, id: &str) -> String {
    format!(
        "将删除插件「{name}」（{id}）的安装目录，并停掉它正在运行的进程。\n\
         插件的用户数据（plugin-data/{id}）会保留。\n\
         若其他插件声明依赖它，卸载会被拒绝并列出引用者。确定卸载？"
    )
}

impl Render for PluginPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let entity = cx.entity();
        let refresh_btn = Button::new("plugin-refresh")
            .ghost()
            .icon(IconName::RefreshCw)
            .with_size(Size::XSmall)
            .tooltip("刷新插件清单")
            .on_click(move |_, _window, cx| {
                let _ = entity.update(cx, |this, cx| this.refresh_plugins(cx, true));
            });

        // 「管理」入口：左 Dock 只做紧凑清单，搜索 / 筛选 / 详情 / 安装都去中央管理页。
        // 走 `Shared::open_plugin_manager` 宿主回调（与属性面板同形），不在窄栏里再造一份。
        let manager_shared = self.shared.clone();
        let manager_btn = Button::new("plugin-open-manager")
            .ghost()
            .icon(IconName::Settings)
            .with_size(Size::XSmall)
            .tooltip("打开插件管理页（搜索、筛选、详情与安装）")
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
                .tooltip("插件文件已变更，重载后生效（会停掉所有插件再重新激活）")
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
            .label("安装插件")
            .with_size(Size::XSmall)
            .disabled(busy)
            .tooltip("从 .zip / .tar.gz 插件包安装")
            .on_click(move |_, _window, cx| {
                let _ = install_file_entity.update(cx, |this, cx| this.pick_package_file(cx));
            });
        let install_dir_entity = cx.entity();
        let install_dir_btn = Button::new("plugin-install-directory")
            .ghost()
            .label("从目录")
            .with_size(Size::XSmall)
            .disabled(busy)
            .tooltip("从本地插件目录（含 plugin.toml）导入")
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
                        .title(EmptyTitle::new().child("还没有插件"))
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
        false
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
