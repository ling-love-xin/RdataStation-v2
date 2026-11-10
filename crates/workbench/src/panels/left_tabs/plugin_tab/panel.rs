use super::super::*;

use super::delegate::PluginListDelegate;
use super::{event_touches_plugin_list, install_receipt, panel_busy};

/// 插件 tab：已装插件清单（名称 / 版本 / 状态）+ 刷新 + 启用 / 停用 / 卸载。
///
/// 数据面 = 全局 `PluginManager`（`plugin::manager`，app 启动时初始化）；会话态清单
/// 由事件路径刷新（render 纯读）：首次 `new` 与「刷新 / 启停 / 卸载」操作后重新拉取。
/// 启停与卸载是异步管理操作（依赖 tokio 上下文），经全局 runtime 句柄在后台执行，不阻塞 UI。
pub struct PluginPanel {
    pub(super) shared: Shared,
    pub(super) focus_handle: FocusHandle,
    /// 会话态插件清单（`plugin::manager::PluginInfo`，事件路径写入）
    pub(super) plugins: Rc<RefCell<Vec<PluginInfo>>>,
    /// 刷新进行中（防重复触发）
    loading: Rc<Cell<bool>>,
    /// 刷新进行中又收到刷新请求：结束后再拉一次，避免事件被 `loading` 直接吞掉
    refresh_again: Rc<Cell<bool>>,
    /// 最近一次操作的错误文案（失败展示，成功即清）
    pub(super) error: Rc<RefCell<Option<String>>>,
    /// 最近一次操作的成功回执（与 `error` 互斥，刷新时不清）
    pub(super) notice: Rc<RefCell<Option<String>>>,
    /// 安装进行中的提示（`Some` 即忙：按钮置灰、防重复触发；完成后清空）
    pub(super) progress: Rc<RefCell<Option<String>>>,
    /// 每个插件的贡献点计数（与 `plugins` 同一次刷新更新）
    pub(super) contributions: Rc<RefCell<HashMap<String, ContributionCounts>>>,
    /// 插件列表状态（懒创建）
    pub(super) plugins_list: Rc<RefCell<Option<Entity<ListState<PluginListDelegate>>>>>,
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

    /// 展示一条轻量操作回执（打开目录 / 日志失败等）。
    ///
    /// 与异步动作共用 `error > progress > notice` 的展示位；这里只写 notice，
    /// 避免一个非业务失败把 `error` 段长期占住。
    pub(crate) fn show_notice(&self, message: String, cx: &mut Context<Self>) {
        self.error.replace(None);
        self.progress.replace(None);
        self.notice.replace(Some(message));
        cx.notify();
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

    /// 手动刷新：先把插件声明的驱动行重新同步进 `global.db`，再重读清单。
    ///
    /// 驱动登记状态只在生命周期动作里刷新；用户手动点「刷新」时若不重算，管理页会
    /// 长期停在「登记状态未知」。同步本身失败只记日志，不拦清单刷新。
    pub(crate) fn refresh_driver_registration(&self, cx: &mut Context<Self>) {
        let Some(handle) = plugin::runtime_handle().cloned() else {
            self.refresh_plugins(cx, true);
            return;
        };
        let Some(mgr) = plugin::get_plugin_manager().cloned() else {
            self.refresh_plugins(cx, true);
            return;
        };
        if panel_busy(&self.progress) {
            return;
        }
        self.progress.replace(Some("正在同步驱动登记…".to_string()));
        cx.notify();

        let weak = cx.entity().downgrade();
        let _ = cx.spawn(async move |_this, cx| {
            handle.block_on(async { mgr.resync_sidecar_driver_rows().await });
            let _ = weak.update(cx, |this, cx| {
                this.progress.replace(None);
                this.refresh_plugins(cx, true);
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
