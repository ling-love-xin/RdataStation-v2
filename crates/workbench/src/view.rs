//! RdataStation v2 工作台视图（WorkbenchView）— v5 布局
//!
//! 按 GPUI-kit 编码规范搭建 DockArea 可拖拽布局工作台：
//! - 依赖只向下：视图只使用 gpui-kit（gpui / base / component），不承载业务逻辑；
//! - 窗口第一层由 App Shell 的 `Root` 装配；本视图负责标题栏 + 左右活动栏 + DockArea + 状态栏；
//! - 左 Dock：草稿箱 / 数据库导航 / 资源分析 / 插件；右 Dock：洞察 / Mock 生成 / 历史；
//! - 边栏三模式：展开 / 收起（`toggle_dock`）/ 完全隐藏（`remove_dock`，gpui-kit 0.7.1）；
//!   模式状态存于 `Shared`，`render` 统一同步到 Dock（权威同步点）；
//! - Quick Open：搜索 + 命令融合（标题栏居中入口 / Ctrl+P），受控自绘弹层；
//! - 颜色一律取自 `cx.theme().colors`（禁止 raw hex/rgb）。

use std::rc::Rc;

use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::base::{Disableable as _, Selectable, StyledExt};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::dialog::{Dialog, DialogButtonProps, DialogContent};
use gpui_kit::base::dock::PanelId;
use gpui_kit::component::dock::{
    DockArea, DockLayout, DockPlacement, DockSkin, PanelStyle, panel_handle,
};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::TabVariant;
use gpui_kit::component::{ActiveTheme, Icon, IconName, TitleBar};
use gpui_kit::*;

use scratchpad::scratchpad_view::render_scratchpad_file_info_pane;

use insight::InsightWorkspacePanel;

use crate::commands::{
    CloseProject, FocusNavSearch, HideSidebars, RestoreSidebars, SwitchProject, ToggleQuickGoto,
    ToggleQuickOpen,
};
use crate::panels::{
    ArchivePanel, DatabaseNavPanel, EditorPanel, HistoryPanelTab, InsightPanelTab, MockPanelTab,
    PluginPanel, ProjectActionRequest, PropertyRequest, QueryRequest, ResourcePanelTab,
    ResultPanelDock, RightSidebarPanel, ScratchPanel, Shared, SidebarPanel,
};
use crate::quick_open::model::{Action, Mode};
use crate::quick_open::palette::{QuickOpenHost, QuickOpenPalette};
use crate::services::editor_lsp::{LspBlock, WorkbenchLsp};
use crate::services::editor_completion::WorkbenchCompletion;
use crate::ui;
use mock::mock_view::{MockDetailView, MockPanel, focus_detail_tab, status_chip_text};
use settings::commands::{CloseSettings, OpenSettings, ToggleThemeMode};
use settings::settings_page::{SettingsHost, SettingsPage};
use settings::SettingsService;

/// 左侧活动栏面板。
///
/// `LeftPanel` / `RightPanel` / `SidebarMode` / `ConnectionItem` 已下沉外壳 crate
/// `workbench_shell::model`（视图下沉的前置：特性 crate 也要能命名它们）：
/// 这里重导保持 `crate::view::X` / `rds_workbench::X` 路径不变。
/// 三模式状态机 {@link toggle_hidden_mode} 仍在本文件（属逻辑，不属数据）。
pub use workbench_shell::model::{ConnectionItem, LeftPanel, RightPanel, SidebarMode};

/// 工作台视图：标题栏 + 左右活动栏 + DockArea（左/右/中央）+ 状态栏。
pub struct WorkbenchView {
    shared: Shared,
    /// DockArea 实体（render 首次调用时懒初始化，需要 &mut Window）。
    area: Option<Entity<DockArea>>,
    sidebar: Option<Entity<SidebarPanel>>,
    editor: Option<Entity<EditorPanel>>,
    right_sidebar: Option<Entity<RightSidebarPanel>>,
    // 真面板化（左右 dock 模块 tab 薄壳）：每个模块 = 独立 dock tab。
    // 壳持有内层 crate 视图实体（经 sidebar / right_sidebar 的访问器取），
    // dock 自动生成 tab 条 → 同面切换 / 拖拽分栏；活动栏点击走 `select_*_panel`。
    left_nav_tab: Option<Entity<DatabaseNavPanel>>,
    left_scratch_tab: Option<Entity<ScratchPanel>>,
    left_resource_tab: Option<Entity<ResourcePanelTab>>,
    left_plugin_tab: Option<Entity<PluginPanel>>,
    right_insight_tab: Option<Entity<InsightPanelTab>>,
    right_mock_tab: Option<Entity<MockPanelTab>>,
    right_history_tab: Option<Entity<HistoryPanelTab>>,
    right_archive_tab: Option<Entity<ArchivePanel>>,
    /// Quick Open 浮层实体（首帧渲染时懒创建）；浮层自持输入 / 结果 / 键盘通道。
    quick_open_palette: Option<Entity<QuickOpenPalette>>,
    /// 打开后待办：通知浮层去清输入 / 落选中 / 聚焦（浮层可能上一帧才创建）。
    quick_open_open_pending: bool,
    /// 本次打开待生效的浮层档位（`Default` = Ctrl+P，`Goto` = Ctrl+Shift+O）。
    quick_open_mode_pending: Mode,
    /// 设置页实体（首次打开时懒创建）。
    settings_page: Option<Entity<SettingsPage>>,
    /// M1 项目管理输入实体（懒创建）。
    project_inputs: Option<project::ui::ProjectInputs>,
    /// M1 项目选择器面板（中央 tab 组常驻 tab；强持有防 GC，见 `project_picker.rs`）。
    project_picker: Option<Entity<crate::panels::ProjectPickerPanel>>,
    /// M1 项目视图宿主（构造期组装；项目视图位于 `project` crate）。
    project_host: Option<project::ui::ProjectUiHost>,
    /// 订阅句柄（保持连接选中事件的订阅存活）。
    /// 编辑面板观察句柄（其 notify 级联到宿主，保证对话框层内容同步）。
    _editor_subscription: Option<Subscription>,
    /// 资产库面板观察句柄（M6：左栏选中 / 数据变化 → 唤醒右栏「存档详情」）。
    ///
    /// 观察的是**面板实体**而不是 `SidebarPanel`：子实体的 `notify` 不级联到父面板，
    /// 订在侧栏上一条都收不到。句柄必须被持有——drop 即解除观察。
    _archive_subscription: Option<Subscription>,
    /// M8 洞察**中间工作页**的取数 / 动作订阅句柄（`jobs::attach` +
    /// `insight_actions::attach` + 右栏联动 observe；面板懒创建时建立一次）。
    ///
    /// 面板实体本身只经 `Shared::insight_workspace_panel` 弱句柄持有：tab ✕ 关闭后
    /// 实体随 Dock 移除而 drop，下次入口自动重建（与 search_results 同例）。
    _insight_ws_subs: Vec<Subscription>,
    /// M8 洞察规则目录监听句柄（改动 `.rule.toml` 后自动重载规则集）。
    ///
    /// 句柄必须**被持有**：它是 RAII 语义——drop 即停后台线程，
    /// 因此不能建成临时值（否则会在行尾被回收，监听静默失效）。
    /// 前缀下划线表示「仅为持有，不读取」，与 `_subscription` 同一约定。
    _insight_rules_watcher: Option<insight::RulesWatcher>,
    /// SQL 编辑器的文档集合（跨面板共享；一个面板 = 一个标签 = 一份文档）。
    ///
    /// 放在 workbench 是因为它是**装配宿主**：`crates/editor` 不依赖本 crate，
    /// 由宿主把服务句柄交给各面板，并把它接进中央 Dock 的 tab 组。
    editor_service: editor::shared::EditorShared,
    /// 已开的编辑面板（按 `DocumentId` 复用：同文档不重复建面板）。
    editor_hosts: Vec<Entity<editor::view::host::EditorHostPanel>>,
    /// 底部结果 dock 薄壳（Dock 化阶段 3：只渲染活动文档的结果面板；开/关由
    /// render 权威点 `sync_result_dock` 按「有结果自动开 / 无结果自动关」驱动）
    result_dock: Option<Entity<ResultPanelDock>>,
    /// 编辑器面板状态变化订阅（#47 状态栏上抛：host 发 `EditorHostEvent::StatusChanged`
    /// 时宿主重绘，全局状态栏活动文档上下文段随之刷新）。句柄必须被持有，drop 即失效；
    /// 面板关闭后订阅随实体 drop 失效，Vec 里留着无害（文档数有限）。
    _editor_status_subs: Vec<Subscription>,
    /// 草稿执行回执 → 元数据回写泵（1 s 一拍；句柄必须被持有，drop 即停）。
    scratchpad_meta_pump: Option<Task<()>>,
    /// 【#54 B】语义诊断目录快照注入泵（1 s 一拍；句柄必须被持有，drop 即停）。
    /// 复用 [`editor_completion`] 的 L1 目录：补全端口实例由本字段持有，
    /// 泵每拍读「活动文档绑定连接 → 推快照」（见 `services/editor_catalog.rs`）。
    catalog_pump: Option<Task<()>>,
    /// 【#54 B】补全端口实例（`editor_completion::attach` 的返回值；目录快照唯一来源）。
    editor_completion: Option<Rc<WorkbenchCompletion>>,
    /// 【LSP】语言服务端口句柄（状态栏 LSP 管理器持有它做启停 / 回收）；
    /// 测试装配 / 未启用 = `None`（状态栏不摆这一项）。
    lsp: Option<Rc<WorkbenchLsp>>,
    /// 【LSP】空闲回收 + 状态栏指示刷新泵（1 s 一拍；句柄必须被持有，drop 即停）。
    lsp_reaper: Option<Task<()>>,
    /// 【LSP】上一次看到的端口状态版本（不同则 `notify` 重绘状态栏指示）。
    last_lsp_epoch: u64,
    /// Dock 皮肤句柄（`init_workspace` 装配时持有；设置页「标签栏最大宽度 / 样式」
    /// 变更经 `on_settings_changed` 应用到这里，渲染读取点在 vendor `dock/tab_panel.rs`）。
    dock_skin: Option<Rc<DockSkin>>,
}

impl WorkbenchView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // P0：先解析当前项目会话（环境变量 → 最近项目 → 空态），供列表作用域 / 标题栏 /
        // 草稿箱共用；**连接列表必须按作用域加载**（全局 + 项目 P_/GP_），否则启动后
        // 只看到全局连接，要等重新打开项目才出现项目侧连接。
        let project = crate::services::project_session::resolve();
        let project_root = project.as_ref().map(|p| p.root.clone());
        let (connections, notice) =
            crate::services::workspace_loader::load_connections_for_scope(project_root.as_deref());
        let shared = Shared::with_connections(connections, notice);
        *shared.project.borrow_mut() = project;
        // 启动恢复的项目也要**取写锁**——与改动前同一时机：**在端口装配之前**
        //（端口只是拄着 `Shared` 的句柄，真正读锁状态是调用 / 渲染那一刻）。
        Self::lock_startup_project(&shared, project_root);
        let mut view = Self::assemble(shared, true, cx);
        view.install_startup_side_effects();
        view
    }

    /// 测试装配：项目会话由调用方给（临时目录），**跳过有真机副作用的四步**——
    /// 会话解析、项目**写锁**（会锁住开发机上的示例项目）、项目名册读盘、规则目录监听（后台线程）；
    /// 连接列表为空（用例不需要真连接）。
    ///
    /// 其余（端口注入 / 面板装配 / `render` 消费请求）与 [`Self::new`] **同一条路**：
    /// 「草稿双击 → 编辑器」那条宿主分支过去盖不住的原因就是拿不到工作台本身
    /// （同源账：`docs/architecture/connection/connection-dialog-architecture.md` §14 #9）。
    pub fn new_for_test(project: Option<project::ui::OpenProject>, cx: &mut Context<Self>) -> Self {
        let shared = Shared::with_connections(Vec::new(), None);
        *shared.project.borrow_mut() = project;
        // 测试装配不 spawn LSP server（真起子进程是测试副作用）：editor 侧测试用假端口直注，
        // workbench 侧不测 LSP——这里保持「未注入 = 无语言服务」的口径即可。
        Self::assemble(shared, false, cx)
    }

    /// 装配核心（真机与测试**共用这一份**）：端口注入 → 项目宿主 → 重绘桥。
    ///
    /// 调用方负责先建好 `Shared`（带项目会话与连接快照），以及自己那档的启动副作用。
    /// `spawn_lsp`：真机传 `true`（**接语言服务端口为例，按需启动，这里不 spawn**），
    /// 测试传 `false`（不接，保持「未注入 = 无语言服务」）。
    fn assemble(shared: Shared, spawn_lsp: bool, cx: &mut Context<Self>) -> Self {
        // B1：编辑器的连接端口要用它（连接列表快照 + 项目根）——先 clone 出来，
        // 因为下面构造 `editor_service` 时不能再借 `self`。
        let editor_shared_for_conn = shared.clone();
        // B11：编辑器的共享状态**提前建好**——M1 项目桥（未保存草稿拦截）与连接端口都要它，
        // 而下面 `build_host` 就会用到；它是 `Rc` 句柄（不是 gpui 实体），clone 很便宜。
        let editor_service = editor::shared::EditorShared::new();
        editor_service.open(editor::service::OpenRequest::untitled(
            "",
            editor::model::EditorMode::Sql,
        ));
        // A14：把执行端口接上（文档绑定/活动连接）。未接时执行动作会明确报“未接入执行”。
        crate::services::editor_exec::attach(&editor_service);
        // 【2b】把执行环境端口接上（通用模式脚本执行；attach 时探测解释器并缓存，
        // 渲染路径零 I/O。未接时环境列表为空 → 运行按钮不亮）。
        crate::services::editor_env::attach(&editor_service);
        // 【LSP】把语言服务端口接上（通用模式 Python 文档 → jedi-language-server）：
        // **按需启动**（这里只建端口不 spawn，首个要服务的文档才拉起）+ 按需回收（视图 1 s 泵）。
        // 测试装配不接（见 `new_for_test`，否则会真起子进程）。
        let lsp = if spawn_lsp {
            Some(crate::services::editor_lsp::attach(&editor_service))
        } else {
            None
        };
        // A12：把会话存储接上（光标 / 选区 / 模式跨重启保留；未接时不持久化但编辑可用）
        crate::services::editor_session::attach(&editor_service);
        // A9：把“另存为”的路径选择接上（系统文件对话框；未接时另存为会明确报未接入）
        crate::services::editor_files::attach(&editor_service);
        // B1：把连接端口接上（连接列表 + 自动建连；未接时选择器说“未接入连接列表”）
        crate::services::editor_connections::attach(&editor_service, &editor_shared_for_conn);
        // B13：把执行通道端口接上（源库 / 本地加速 / 联邦 的门控；未接时后两档都不可选并给原因）
        crate::services::editor_channels::attach(&editor_service, &editor_shared_for_conn);
        // T1.6：把源清单端口接上（联邦会话快照 → 「源清单 ▾」浮层；未接时浮层只有空态说明）
        crate::services::editor_sources::attach(&editor_service);
        // M8：把「洞察此列」端口接上（结果集列头右键 → 右 Dock 洞察面板；未接时不摆这一项）
        crate::services::editor_insight::attach(&editor_service, &shared);
        // 【B8 余项】把「打开历史面板」端口接上（工具栏「⟲ 历史」→ 右 Dock 历史面板）
        crate::services::editor_history::attach(&editor_service, &shared);
        // 项目锁：把项目态端口接上（项目只读时，写源库对象的语句在提交前就被拒）
        crate::services::editor_project::attach(&editor_service, &shared);
        // B9：把补全端口接上（连接级元数据缓存 → 候选目录；未接时只有关键字与函数）
        let completion = crate::services::editor_completion::attach(&editor_service, &shared);
        // #54 B：语义诊断目录快照注入（复用补全的 L1 目录；活动文档绑连变化 → 按文档推）。
        // completion 存入视图字段（泵每拍取用；见 `ensure_catalog_pump`）
        let editor_completion = Some(completion);
        // #54 C6/C7：跳转定义目标解析注入（`rds://navigator/<限定表名>` → 导航树定位，
        // 复用 Quick Open `⌥↵` 的 `request_reveal`；未接 = 内核默认文档内移动）
        crate::services::editor_catalog::attach_jump(&editor_service, &shared);
        // T2c-1：把库 / schema 端口接上（导航缓存 → 状态栏面包屑可切项；未接时退文本连接段）
        crate::services::editor_schema::attach(&editor_service, &shared);
        // #20：内核注册表端口（后台探测一次 + 内存快照；未接时执行源「内核」档菜单置灰）
        crate::services::editor_kernels::attach(&editor_service, cx);
        // B7 切片三：把表结构端口接上（导航缓存 → 导出 SQL Updates 的键列；未接时菜单不摆该档）
        crate::services::editor_table_keys::attach(&editor_service, &shared);
        // M1：排序偏好与首屏项目列表（无项目时）都在构造期完成，避免在 `render` 里做 I/O
        //（GPUI-kit 编码指南：副作用不得放在 render）。偏好走 `SettingsService`（唯一读路径），
        // 不直读 `settings.json`：`app` 已在开窗前 `SettingsService::init`（K1）。
        {
            let saved_sort = settings::SettingsService::project_sort_mode(cx);
            let sort = project::ui::ProjectSort::from_key(&saved_sort);
            shared.project_ui.borrow_mut().picker.sort = sort;
        }
        // 项目视图宿主：注入状态句柄 / 重绘 / 编辑区桥 / 排序偏好 / 打开后刷新。
        // 编辑区桥拿的是 `EditorShared`（Rc 句柄）：未保存草稿的归属在编辑器一侧（B11/B12）。
        let host = crate::components::project_host::build_host(
            &shared,
            cx.entity().downgrade(),
            editor_service.clone(),
        );
        // 对话框层挂载点在 `WorkbenchView::render`；`Root` 的 notify 不会传到子视图，
        // 因此把宿主重绘桥注入 `Shared`，供打开 / 关闭对话框的入口调用。
        {
            let weak = cx.entity().downgrade();
            *shared.host_redraw.borrow_mut() = Some(Rc::new(move |cx: &mut App| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |_, cx| cx.notify());
                }
            }));
        }
        let mut view = Self {
            shared,
            area: None,
            sidebar: None,
            editor: None,
            right_sidebar: None,
            left_nav_tab: None,
            left_scratch_tab: None,
            left_resource_tab: None,
            left_plugin_tab: None,
            right_insight_tab: None,
            right_mock_tab: None,
            right_history_tab: None,
            right_archive_tab: None,
            quick_open_palette: None,
            quick_open_open_pending: false,
            quick_open_mode_pending: Mode::Default,
            settings_page: None,
            project_inputs: None,
            project_picker: None,
            project_host: Some(host),
            _editor_subscription: None,
            _archive_subscription: None,
            _insight_ws_subs: Vec::new(),
            _insight_rules_watcher: None,
            editor_service,
            editor_hosts: Vec::new(),
            result_dock: None,
            _editor_status_subs: Vec::new(),
            scratchpad_meta_pump: None,
            catalog_pump: None,
            editor_completion,
            lsp,
            lsp_reaper: None,
            last_lsp_epoch: 0,
            dock_skin: None,
        };
        // 【LSP】空闲回收 + 状态栏指示刷新（1 s 一拍）：关掉最后一个需要服务的面板 → 停 server
        if view.lsp.is_some() {
            view.lsp_reaper = view.spawn_lsp_reaper(cx);
        }
        view
    }

    /// 启动恢复的项目也要**取写锁**：`project_ui.lock/read_only` 原本只在交互式打开时写，
    /// 而启动路径直接装会话——结果是「另一实例占着同一个项目」时两边都以为自己可写，
    /// 各模块的只读护栅（mock 的四个出口 / 资源库 / 草稿箱 / 编辑器替换）全部不生效。
    /// 处置与交互式打开一致：被占用 → 只读打开 + 提示。
    fn lock_startup_project(shared: &Shared, project_root: Option<std::path::PathBuf>) {
        let Some(root) = project_root else {
            return;
        };
        let opened = match project::service::open(&root) {
            Ok(project::service::OpenOutcome::Opened(opened)) => Ok(opened),
            Ok(project::service::OpenOutcome::Busy(_)) => project::service::open_read_only(&root),
            Err(e) => Err(e),
        };
        match opened {
            Ok(opened) => {
                let (store, lock, read_only, _summary) = opened.into_parts();
                // store 仅用于确认加载成功（与 `project::ui::apply_opened` 同口径）
                drop(store);
                let mut ui = shared.project_ui.borrow_mut();
                ui.lock = lock;
                ui.read_only = read_only;
                ui.notice = read_only.then(|| "只读打开：该项目已被另一实例占用".to_string());
            }
            // 取锁 / 载入失败：不动会话（项目仍是当前项目），只提示一声
            Err(e) => shared.project_ui.borrow_mut().notice = Some(e),
        }
    }

    /// 启动期的两处副作用：项目下拉列表（无名册项目时）与洞察规则目录监听。
    ///
    /// 规则监听放在构造期（而非 render）——沿用 GPUI-kit 编码指南「副作用不得放在 render」，
    /// 也保证窗口首帧前监听已就位；目录由「当前项目根」现算，故先告知监听器当前项目
    /// （切换时在 `refresh_after_open` 再告知）。
    fn install_startup_side_effects(&mut self) {
        // 条件先算好再借用 host：`load_picker` 会回到 `Shared`，借着 `project` 不放会撞 RefCell。
        let no_project = self.shared.project.borrow().is_none();
        if no_project && let Some(host) = self.project_host.as_ref() {
            project::ui::load_picker(host);
        }
        insight::set_watched_project_root(self.shared.project_root());
        self._insight_rules_watcher = Some(insight::RulesWatcher::spawn());
    }

    /// 【草稿箱 → 编辑器】已开的编辑面板（供窗口级用例断言「请求消费后真的有一份文档进了中央 Dock」）。
    pub fn editor_panels_for_test(&self) -> Vec<Entity<editor::view::host::EditorHostPanel>> {
        self.editor_hosts.clone()
    }

    /// 共享状态句柄（供窗口级用例从**生产请求槽**发起打开：`Shared::request_open_in_editor`）。
    pub fn shared_for_test(&self) -> Shared {
        self.shared.clone()
    }

    /// 在编辑器中打开一个文件（打开菜单 / 数据库导航“在 SQL 编辑器中打开”调用）
    ///
    /// 走 `editor::persist::open_file`：
    /// - **同路径已打开只激活、不重读**（重读会冲掉未保存的编辑）；
    /// - 新文档则建面板并加入中央 tab 组（`DockArea::add_panel`）。
    pub fn open_in_editor(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.open_in_editor_with(
            path,
            editor::model::ReadOnly::none(),
            None,
            window,
            cx,
        )
    }

    /// 同上，但带上**只读维度**（M6 的存档本体：编辑器只读，改它要先去取回）。
    ///
    /// 只读由发起方判定（编辑器不认识 `resources/`），两条路只在“建文档时置不置只读”上分叉：
    /// 已打开的文档仍旧只激活（不悄悄把用户手上的文档锁掉）。
    pub(crate) fn open_in_editor_with(
        &mut self,
        path: std::path::PathBuf,
        read_only: editor::model::ReadOnly,
        reveal: Option<crate::panels::RevealTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        tracing::info!(path = %path.display(), read_only = !read_only.can_edit(), "【探针】open_in_editor_with 进入：在编辑器打开文件");
        let mode = editor::mode::resolve_mode(&path, None);
        let outcome = if read_only.can_edit() {
            editor::persist::open_file(&self.editor_service, &path, mode)
        } else {
            editor::persist::open_file_read_only(&self.editor_service, &path, mode)
        }
        .map_err(|error| error.to_string())?;
        // M5 Phase C-2：草稿带连接绑定（`file_meta`）时，打开即预选。
        // 只对**新建**文档生效——同路径已打开走的是“只激活”，不覆盖用户手动改过的连接。
        if !outcome.is_activated() {
            if let Some(conn_id) = self.scratchpad_preferred_connection(&path) {
                self.editor_service
                    .update(|service| service.set_connection(outcome.id(), Some(conn_id)));
            }
        }
        self.show_document(outcome.id().clone(), window, cx);
        // M5 Phase C：命中跳行——打开（或激活）后把光标/选区落到目标行。
        // `show_document` 已把面板接到界面上；新建 / 复用的面板都在这份列表里。
        if let Some(target) = reveal {
            if let Some(panel) = self
                .editor_hosts
                .iter()
                .find(|panel| panel.read(cx).document() == outcome.id())
                .cloned()
            {
                panel.update(cx, |panel, cx| {
                    panel.reveal_search_target(target.line, target.span, window, cx)
                });
            }
        }
        Ok(())
    }

    /// 草稿路径 → 打开时应预选的连接（非草稿 / 未绑定 / 连接已不在下拉里 → `None`）。
    ///
    /// 读的是草稿元数据（`.RSmeta/scratchpad/config.json` 的 `file_meta`），属「元数据级操作
    /// 保持同步」的既定口径（K1c），与侧栏引用增删改同路；任何一步失败都退化为“不预选”。
    fn scratchpad_preferred_connection(&self, path: &std::path::Path) -> Option<String> {
        let (store, runtime) = self.shared.scratchpad_store().ok()?;
        let relative = store.relative_path_of(path)?;
        let meta = runtime.block_on(store.file_meta(&relative)).ok()?;
        let conn_id = meta.preferred_connection()?;
        // 已被删除的连接不预选：它不在下拉里，绑上去只会让执行报错。
        let known = self
            .editor_service
            .connection_options()
            .iter()
            .any(|option| option.id == conn_id);
        known.then(|| conn_id.to_string())
    }

    /// 打开一条查询（B11）：导航「在 SQL 编辑器中打开 / 查看数据 / 生成 SQL」与拖拽共用
    ///
    /// 语义（原型 §1.2 入口语义）：
    /// - 复用条件很窄：**未命名 + 内容空 + 同一绑定**的 SQL 文档（连续点几次“打开”不刷标签）；
    /// - 否则新建一份绑定了该连接的未命名 SQL 文档；
    /// - 带 SQL 时追加到目标文档（`Shared::append_sql`：**追加不覆盖**，用户手里的草稿不会没了）；
    /// - `run = true`（「查看数据」）时打开后立即执行整篇（关掉 M4 遗留的“查看数据不自动执行”）。
    pub fn open_query_document(
        &mut self,
        request: QueryRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use editor::model::EditorMode;
        use editor::service::OpenRequest;

        // 请求带了连接就把导航的当前连接也切过去（与点该连接同口径）
        if let Some(conn_id) = request.conn_id.as_deref() {
            let idx = self
                .shared
                .connections
                .borrow()
                .iter()
                .position(|c| c.id == conn_id);
            if let Some(idx) = idx {
                self.shared.selected.set(Some(idx));
                self.shared.invalidate_nav_cache();
            }
        }

        let sql = request.sql.trim().to_string();
        let target = self.reusable_query_document(request.conn_id.as_deref(), &sql);
        let document = match target {
            Some(id) => {
                if !sql.is_empty() {
                    let merged = self
                        .editor_service
                        .service()
                        .find(&id)
                        .map(|doc| crate::panels::append_sql(doc.content(), &sql))
                        .unwrap_or_else(|| sql.clone());
                    self.editor_service
                        .update(|service| service.set_content(&id, merged));
                }
                id
            }
            None => {
                let mut open = OpenRequest::untitled(&sql, EditorMode::Sql);
                if let Some(conn_id) = request.conn_id.clone() {
                    open = open.with_connection(conn_id);
                }
                self.editor_service.open(open).id().clone()
            }
        };

        self.show_document(document.clone(), window, cx);
        // 面板的内容可能落后于服务层（复用文档时刚追加过）：从文档重载一次再执行
        if let Some(panel) = self.editor_panel(&document, cx) {
            panel.update(cx, |panel, cx| {
                panel.reload_from_document(window, cx);
                if request.run {
                    panel.run_all(cx);
                }
            });
        }
    }

    /// 可复用的查询文档：未命名 + 内容空 + 同一绑定的 SQL 文档（只复用这一种）
    fn reusable_query_document(
        &self,
        conn_id: Option<&str>,
        sql: &str,
    ) -> Option<editor::model::DocumentId> {
        // 带 SQL 的请求总是新建：它有自己的内容要放，不该往别人正在写的草稿里塞
        if !sql.is_empty() {
            return None;
        }
        self.editor_service
            .service()
            .documents()
            .iter()
            .find(|doc| {
                doc.path().is_none()
                    && doc.mode() == editor::model::EditorMode::Sql
                    && doc.content().trim().is_empty()
                    && doc.connection() == conn_id
            })
            .map(|doc| doc.id().clone())
    }

    /// 某文档对应的编辑器面板（没有面板则 `None`）
    fn editor_panel(
        &self,
        document: &editor::model::DocumentId,
        cx: &App,
    ) -> Option<Entity<editor::view::host::EditorHostPanel>> {
        self.editor_hosts
            .iter()
            .find(|panel| panel.read(cx).document() == document)
            .cloned()
    }

    /// 关闭所有**未命名**文档（M1 项目切换的「未保存草稿」拦截用：内容已另存或用户选择丢弃）
    ///
    /// 只关未命名的：有路径的文档是真实文件，项目切换不该把它们关掉。
    pub fn close_untitled_editor_documents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let untitled: Vec<editor::model::DocumentId> = self
            .editor_service
            .service()
            .documents()
            .iter()
            .filter(|doc| doc.path().is_none())
            .map(|doc| doc.id().clone())
            .collect();
        let Some(area) = self.area.clone() else {
            return;
        };
        for id in untitled {
            if let Some(panel) = self.editor_panel(&id, cx) {
                editor::view::host::close_document_now(&area, panel, window, cx);
            } else {
                // 没有面板（已不在 Dock 上）：直接让服务层收尾，不留孤儿文档
                let _ = self.editor_service.update(|service| service.close(&id));
            }
        }
    }

    /// 把一份已存在于 `EditorService` 的文档接到界面上（建面板 / 复用已有面板）
    pub fn show_document(
        &mut self,
        document: editor::model::DocumentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 已关闭的面板不参与复用（面板被 Dock 移除 = 文档已关，两者一一对应）
        self.editor_hosts
            .retain(|panel| !panel.read(cx).is_closed());

        // 已有面板：不重建——**切换到它**（`TabGroup::select_tab`），否则用户看到的
        // 是“点了打开却没反应”：面板就在隔壁标签里，但不在前台。
        let existing = self
            .editor_hosts
            .iter()
            .find(|panel| panel.read(cx).document() == &document)
            .cloned();
        if let Some(panel) = existing {
            panel.update(cx, |panel, cx| panel.focus_self(window, cx));
            return;
        }

        // 走到这里有两种情形，做同一件事（建面板）：
        // - 新文档（`Opened`）；
        // - 文档已在服务层但没有面板（`Activated`，例如将来由 Quick Open 直接开的文档）：
        //   补一个面板指向同一文档——内容只有一份（全在 `EditorService` 里），不会分身。
        let service = self.editor_service.clone();
        let panel =
            cx.new(|cx| editor::view::host::EditorHostPanel::new(service, document, window, cx));
        self.editor_hosts.push(panel.clone());
        // 状态变化 → 宿主重绘 → 全局状态栏活动文档上下文段刷新
        // （内容 / 连接 / Schema / 通道 / 事务 / 执行 / 消息都会发 StatusChanged）
        let _status_sub = cx.subscribe_in(
            &panel,
            window,
            |_this, _panel, _event: &editor::view::host::EditorHostEvent, _window, cx| {
                cx.notify();
            },
        );
        self._editor_status_subs.push(_status_sub);

        if let Some(area) = self.area.clone() {
            // 工程债（用户点名「tab 显示 editor 而非文件名」）：面板必须带 presentation
            // handle 注册——`add_panel_view(panel_handle(..))` 而非裸 `add_panel(Entity)`。
            // 裸注册时 Dock 取不到 `tab_name`（标题回退成 `panel_name()` "editor"）与
            // `title_suffix`（脏点 / 自绘 ✕），见 gpui-kit `panel_title` 的回退分支。
            panel.update(cx, |panel, _cx| panel.set_area(area.downgrade()));
            let handle = panel_handle(panel);
            area.update(cx, |area, cx| {
                area.add_panel_view(handle, DockPlacement::Center, None, window, cx);
            });
        }
    }

    /// 恢复上次的全部编辑器会话（A12 + E3 多文档恢复）
    ///
    /// 内容与模式取自会话（**不是从磁盘重读**：用户上次可能没保存），光标/选区交回面板。
    /// 顺序：最早编辑的先开（tab 从左到右按历史），最近编辑的最后开并成为活动文档。
    /// 恢复成功后把启动时那份空的未命名文档收掉，免得每启动一次多一个空标签。
    ///
    /// 调用时机：`init_workspace` 的 `cx.defer_in`（读库是 I/O，不在构造与渲染里做）。
    pub fn restore_sessions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(sessions) = self.editor_service.load_all_sessions() else {
            return;
        };
        if sessions.is_empty() {
            return;
        }

        // 启动时那份空的未命名文档（在 `new` 里开的）——恢复成功后收掉
        let blank = self
            .editor_service
            .service()
            .documents()
            .iter()
            .find(|doc| doc.path().is_none() && doc.content().trim().is_empty())
            .map(|doc| doc.id().clone());

        // 全局库按最近更新倒序（DESC）；反转成"最早编辑的先开"，
        // 让最近编辑的文档最后 open 并成为活动标签（与 VSCode 重启后的直觉一致）
        for session in sessions.into_iter().rev() {
            let Some(path) = session.path.clone() else {
                continue;
            };
            if path.is_empty() {
                continue;
            }

            let outcome = self.editor_service.open(editor::service::OpenRequest::file(
                std::path::PathBuf::from(path),
                session.content.clone(),
                session.mode,
            ));
            let document = outcome.id().clone();
            self.show_document(document.clone(), window, cx);

            let restored = self
                .editor_hosts
                .iter()
                .find(|panel| panel.read(cx).document() == &document)
                .cloned();
            if let Some(panel) = restored {
                panel.update(cx, |panel, cx| panel.restore_session(&session, window, cx));
            }

            // 收掉空标签（走标准关闭路径：面板移除 → `on_removed` → 文档从服务层关掉）
            if let Some(blank) = blank.clone()
                && blank != document
                && let Some(area) = self.area.clone()
            {
                let blank_panel = self
                    .editor_hosts
                    .iter()
                    .find(|panel| panel.read(cx).document() == &blank)
                    .cloned();
                if let Some(panel) = blank_panel {
                    editor::view::host::close_document_in_dock(&area, panel, window, cx);
                }
            }
        }
    }

    /// 窗口真正退出前：把**每一份**打开的文档的会话落库（E3）。
    ///
    /// 为什么必须有这一步：会话此前只在 `Ctrl+S` 与「关掉这一份文档」时写，
    /// 而点 ✕ / `Alt+F4` 走的是平台关闭路径——没有任何钩子，敲了一半的 SQL 会直接丢。
    /// 本方法由 app 层注册在 `Window::on_window_should_close` 上（见 `crates/app/src/main.rs`）。
    ///
    /// 只存会话、**不问用户**：这里是兜底而不是拦截（拦截在「关单个文档」那条路上，
    /// 有完整的三态确认）。存失败不阻断退出——绝不能因为写库失败把人关在窗口里。
    pub fn save_all_editor_sessions(&mut self, cx: &mut Context<Self>) {
        // 先清掉已关掉的面板（与 `close_active_editor` 同一收尾），免得对尸体调保存
        self.editor_hosts
            .retain(|panel| !panel.read(cx).is_closed());
        let panels = self.editor_hosts.clone();
        editor::view::host::save_sessions_for(&panels, cx);
    }

    /// 关闭当前编辑器文档（`Ctrl+W`）
    ///
    /// 键位绑在编辑器面板的 `editor` context 上（A10），但**由宿主执行**：
    /// 面板移除时会回调它自己的 `on_removed`，而 `DockArea` 移除面板要读面板本体，
    /// 从面板自己的 `update` 里发起就是重入。宿主不在那个 `update` 中，可以安全地做。
    /// 脏文档由 [`Self::close_editor_document`] 弹三态确认（保存 / 不保存 / 取消）。
    pub fn close_active_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_hosts
            .retain(|panel| !panel.read(cx).is_closed());
        let Some(id) = self.editor_service.service().active_id().cloned() else {
            return;
        };
        tracing::info!(doc = %id, "【探针】close_active_editor 进入：关闭当前编辑器文档（Ctrl+W）");
        let Some(panel) = self
            .editor_hosts
            .iter()
            .find(|panel| panel.read(cx).document() == &id)
            .cloned()
        else {
            return;
        };
        self.close_editor_document(panel, window, cx);
    }

    // ══════════════════════════════════════════════════════════════════
    // 编辑器文档的宿主级动作（A9）：关闭三态 / 另存为 / 打开文件
    //
    // 这几件事只能宿主做：① 让 Dock 移除面板（面板自己发起是重入）；② 弹对话框（要
    // `Root` 与对话框层，宿主 render 已挂）；③ 弹系统文件对话框（`rfd` 是宿主依赖）。
    // 编辑器侧只提供纯判定（`mode::plan_switch`）、文案（`mode::ConfirmKind`）与
    // 对话框渲染（`editor::view::dialogs`）。
    // ══════════════════════════════════════════════════════════════════

    /// 关闭一份编辑器文档：干净的直接关，脏的先问（保存 / 不保存 / 取消）
    ///
    /// 流程本身在编辑器侧（`editor::view::host::request_close_document`）——“先保存再关”、
    /// “未命名先另存为”、“写盘失败二次确认”都是编辑器语义；宿主只提供两样东西：
    /// 能让 Dock 移除面板的入口、以及系统文件对话框（以路径选择端口注入）。
    pub fn close_editor_document(
        &mut self,
        panel: Entity<editor::view::host::EditorHostPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(area) = self.area.clone() else {
            return;
        };
        editor::view::host::request_close_document(&area, panel, window, cx);
    }

    /// 另存为当前文档（`Ctrl+Shift+S`）：系统文件对话框 → 写盘 → 标题跟随
    ///
    /// 未接入面板时在状态栏留原因，不静默。
    pub fn save_active_editor_as(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor_hosts
            .retain(|panel| !panel.read(cx).is_closed());
        let Some(id) = self.editor_service.service().active_id().cloned() else {
            return;
        };
        let Some(panel) = self
            .editor_hosts
            .iter()
            .find(|panel| panel.read(cx).document() == &id)
            .cloned()
        else {
            return;
        };
        editor::view::host::request_save_as(&panel, cx);
    }

    /// 新建一份未命名文档（Quick Open 的「新建查询 / 新建笔记 / 新建文件」）
    ///
    /// 三档模式走**同一条路**：开文档 → 建面板（`show_document`）；标题由服务层的未命名
    /// 编号保证不重名（`未命名-1` / `未命名-2`…）。
    ///
    /// **新建查询会带上当前选中的连接**（原型 §1.2 入口语义“SQL 模式并绑定该连接”）：
    /// 导航里选了哪个库，新查询就默认发到那个库——绑不绑在状态栏与工具栏都看得见。
    /// 没选连接时保持未绑定（执行跟随当前活动连接）。
    pub fn new_editor_document(
        &mut self,
        mode: editor::model::EditorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(mode = ?mode, "【探针】new_editor_document 进入：新建未命名文档");
        let mut request = editor::service::OpenRequest::untitled("", mode);
        if mode == editor::model::EditorMode::Sql
            && let Some(id) = self.selected_connection_id()
        {
            request = request.with_connection(id);
        }
        let outcome = self.editor_service.open(request);
        self.show_document(outcome.id().clone(), window, cx);
    }

    /// 导航里当前选中的连接 id（Quick Open 选中 / 导航树点击都会置位）
    fn selected_connection_id(&self) -> Option<String> {
        let index = self.shared.selected.get()?;
        self.shared
            .connections
            .borrow()
            .get(index)
            .map(|item| item.id.clone())
    }

    /// 「打开文件」（`Ctrl+O`）：系统文件对话框 → 在编辑器中打开
    ///
    /// 已在编辑器里打开过的路径**只激活、不重读**（去重规则在 `editor::persist::open_file`）。
    /// 这条入口在宿主侧：它要开**新文档**（建面板），不是某份文档上的动作。
    pub fn open_file_via_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = crate::services::editor_files::pick_open_path() else {
            return;
        };
        tracing::info!(path = %path.display(), "【探针】open_file_via_dialog：系统文件对话框已选定路径（Ctrl+O）");
        if let Err(error) = self.open_in_editor(path, window, cx) {
            *self.shared.notice.borrow_mut() = Some(format!("打开文件失败：{error}"));
        }
    }

    /// 项目视图宿主（构造期已装配）。
    fn project_host(&self) -> &project::ui::ProjectUiHost {
        self.project_host
            .as_ref()
            .expect("project host initialized")
    }

    /// 刷新项目选择器（项目菜单「切换项目」等外部触发）。
    pub fn refresh_project_picker(&mut self, cx: &mut Context<Self>) {
        project::ui::refresh_picker(self.project_host(), cx);
    }

    /// 启动「草稿执行回执 → 元数据回写」泵（装配期一次）。
    ///
    /// 编辑器把「哪份文档用了哪个连接」留在 `EditorShared`；这里每秒取一次，只对草稿箱
    /// 模块内的文件写回 `file_meta`。**不挂在草稿箱面板上**：侧栅切到别的工具时面板不渲染，
    /// 而执行照样会发生。
    fn ensure_scratchpad_meta_pump(&mut self, cx: &mut Context<Self>) {
        if self.scratchpad_meta_pump.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(1000)).await;
                let alive = weak
                    .update(cx, |this, cx| {
                        crate::services::scratchpad_meta::write_back(
                            &this.shared,
                            &this.editor_service,
                            cx,
                        )
                    })
                    .is_ok();
                if !alive {
                    return;
                }
            }
        });
        self.scratchpad_meta_pump = Some(task);
    }

    /// 启动「语义诊断目录快照注入」泵（装配期一次）。
    ///
    /// 每 1 秒推一次活动文档绑定连接的目录快照（复用补全 L1 目录）。**不挂在编辑
    /// 面板上**：侧栅切到别的工具时面板不渲染，而语义诊断（扫描线程）照样会跑。
    /// 目录未载好 → 推 `None`（语义段跳过）；文档关闭 → 残留清理（见
    /// `services/editor_catalog.rs` 的边界说明）。
    fn ensure_catalog_pump(&mut self, cx: &mut Context<Self>) {
        if self.catalog_pump.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(1000)).await;
                let alive = weak
                    .update(cx, |this, _cx| {
                        // 补全实例已装配（assemble 里 attach 过）；测试装配不接则跳过
                        let Some(completion) = this.editor_completion.clone() else {
                            return true;
                        };
                        crate::services::editor_catalog::pump(
                            &this.editor_service,
                            &completion,
                        );
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    return;
                }
            }
        });
        self.catalog_pump = Some(task);
    }

    /// 搜索结果标签页（VSCode「搜索: query」）：面板未建 → 建并加入中央 tab 组 + 聚焦；
    /// 已建（弱句柄有效）→ 只聚焦不重建。
    ///
    /// **只在 `cx.defer_in` 里调用**（render 检测到结果后 defer，事件路径可改 dock）；
    /// 与属性面板同例：面板实体只建一次（`Shared::search_results_panel` 弱句柄）。
    fn ensure_search_results_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self
            .shared
            .search_results_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            let same_query = {
                let s = self.shared.scratchpad_search.borrow();
                panel.read(cx).query() == s.as_ref().map(|v| v.query.clone())
            };
            if same_query {
                crate::panels::focus_search_tab(&panel, window, cx);
                return;
            }
            // 查询词不同 → 新建独立 tab（多开），旧面板保留显示其快照。
        }
        let Some(area) = self.area.clone() else {
            return;
        };
        let shared = self.shared.clone();
        let panel =
            cx.new(|cx| crate::panels::SearchResultsHostPanel::new(shared.clone(), cx));
        self.shared
            .search_results_panel
            .borrow_mut()
            .replace(panel.downgrade());
        let handle = panel_handle(panel.clone());
        area.update(cx, |area, cx| {
            area.add_panel_view(handle, DockPlacement::Center, None, window, cx);
        });
        crate::panels::focus_search_tab(&panel, window, cx);
    }

    /// 元数据搜索标签页（P0，VSCode「搜索: query」形态，搜的是连接级元数据缓存）：
    /// 面板未建 → 建并加入中央 tab 组 + 聚焦；已建（弱句柄有效）→ 只聚焦不重建。
    ///
    /// **只在 `cx.defer_in` 里调用**（render 检测到结果后 defer，事件路径可改 dock）；
    /// 与 `ensure_search_results_panel` 同例：面板实体只建一次（`Shared::metadata_search_panel`
    /// 弱句柄）；面板自持结果回填泵（`new` 内 `ensure_pump`）。
    fn ensure_metadata_search_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self
            .shared
            .metadata_search_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            let same_query = {
                let s = self.shared.metadata_search.borrow();
                panel.read(cx).query() == s.as_ref().map(|v| v.query.clone())
            };
            if same_query {
                crate::panels::focus_metadata_search_tab(&panel, window, cx);
                return;
            }
            // 查询词不同 → 新建独立 tab（多开），旧面板保留显示其快照。
        }
        let Some(area) = self.area.clone() else {
            return;
        };
        let shared = self.shared.clone();
        let panel =
            cx.new(|cx| crate::panels::MetadataSearchHostPanel::new(shared.clone(), cx));
        self.shared
            .metadata_search_panel
            .borrow_mut()
            .replace(panel.downgrade());
        let handle = panel_handle(panel.clone());
        area.update(cx, |area, cx| {
            area.add_panel_view(handle, DockPlacement::Center, None, window, cx);
        });
        crate::panels::focus_metadata_search_tab(&panel, window, cx);
    }

    /// 洞察**中间工作页**（D63 中央洞察 tab）：消费 `Shared::pending_insight_target`，
    /// 建（或激活）面板并递目标。
    ///
    /// **只在 `cx.defer_in` 里调用**（render 检测到待打开目标后 defer，事件路径可改 dock）；
    /// 与 `ensure_search_results_panel` 同例：面板实体只建一次（`Shared::insight_workspace_panel`
    /// 弱句柄）。面板被 tab ✕ 关闭后实体 drop（宿主只持弱句柄），下次入口自动重建。
    fn ensure_insight_workspace_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 只消费一次：入口连点多个目标时以后到者为准（同帧内不排队）
        let Some(target) = self.shared.pending_insight_target.borrow_mut().take() else {
            return;
        };
        let Some(area) = self.area.clone() else {
            return;
        };
        if let Some(panel) = self
            .shared
            .insight_workspace_panel
            .borrow()
            .clone()
            .and_then(|weak| weak.upgrade())
        {
            // 已建（tab 被切走也只是失焦）：递目标 + 聚焦，不重复加入。
            panel.update(cx, |panel, cx| panel.open_target(target, cx));
            insight::focus_insight_workspace_tab(&panel, window, cx);
            return;
        }
        // 首次：建面板 + 登记弱句柄 + 订阅取数 / 动作事件 + 加入中央 tab 组。
        // 订阅与右栏同款（`RightSidebarPanel::new`）：事件接缝在 insight crate，
        // 宿主只提供「项目根在哪」；动作类（下钻 / 导出）要窗口与工作台状态。
        self._insight_ws_subs.clear();
        let shared = self.shared.clone();
        let panel = cx.new(|cx| InsightWorkspacePanel::new(cx));
        self.shared
            .insight_workspace_panel
            .borrow_mut()
            .replace(panel.downgrade());
        let insight_view = panel.read(cx).insight_view().clone();
        let root_shared = self.shared.clone();
        self._insight_ws_subs
            .push(insight::jobs::attach(&insight_view, cx, move || {
                root_shared
                    .project
                    .borrow()
                    .as_ref()
                    .map(|session| session.root.clone())
            }));
        self._insight_ws_subs
            .push(crate::components::insight_actions::attach(&insight_view, &shared, cx));
        // 全量统计确认弹窗（subscribe_in 要窗口）+ 装配洞察偏好（设置 → 面板 + 引擎）。
        // 右栏装配（RightSidebarPanel::new）已做过一次；懒创建路径补做，保证
        // 中间页先于右栏打开时设置也生效（apply_insight_prefs 幂等）。
        self._insight_ws_subs
            .push(crate::components::insight_actions::attach_dialogs(
                &insight_view, window, cx,
            ));
        crate::components::insight_actions::apply_insight_prefs(&shared, cx);
        // D65 双栏单源：中间工作页目标变化（对比点行 / 后续工具栏切换）→ 右栏摘要联动。
        // 观察回调在事件路径触发，比较两栏当前目标，只有不同才同步（避免 set_target
        // 把右栏载荷也清掉重发请求——同目标重复设置会白白重取样）。
        let shared_for_sync = self.shared.clone();
        let panel_for_sync = panel.clone();
        self._insight_ws_subs.push(cx.observe(&panel_for_sync.clone(), move |_, _, cx| {
            let Some(ws_target) = panel_for_sync
                .read(cx)
                .insight_view()
                .read(cx)
                .target()
                .cloned()
            else {
                return;
            };
            let Some(right) = shared_for_sync
                .insight_panel
                .borrow()
                .clone()
                .and_then(|weak| weak.upgrade())
            else {
                return;
            };
            let right_target = right.read(cx).target().cloned();
            if right_target != Some(ws_target.clone()) {
                right.update(cx, |panel, cx| panel.set_target(ws_target, cx));
            }
            // T18：已洞察表集合快照 → 右栏管理列表（中间页 `opened` 变化都经此同步）
            let entries = panel_for_sync.read(cx).insight_set_entries(cx);
            right.update(cx, |panel, cx| panel.set_insight_set(entries, cx));
        }));
        let handle = panel_handle(panel.clone());
        area.update(cx, |area, cx| {
            area.add_panel_view(handle, DockPlacement::Center, None, window, cx);
        });
        panel.update(cx, |panel, cx| panel.open_target(target, cx));
        insight::focus_insight_workspace_tab(&panel, window, cx);
    }

    /// T18：消费右栏集合的「对比」请求——中间页勾选该表 + 切到对比形态 + 激活中央 tab。
    ///
    /// 面板未建时集合里不可能有表（集合来自中间页 `opened`，同源），此分支只防御：
    /// 丢弃请求，不编候选。
    fn consume_insight_compare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.shared.pending_insight_compare.borrow_mut().take() else {
            return;
        };
        let Some(area) = self.area.clone() else {
            return;
        };
        let Some(panel) = self
            .shared
            .insight_workspace_panel
            .borrow()
            .clone()
            .and_then(|weak| weak.upgrade())
        else {
            return;
        };
        let exists = panel.read(cx).opened().iter().any(|entry| entry.key == key);
        if !exists {
            return;
        }
        panel.update(cx, |panel, cx| {
            panel.toggle_candidate(&key, cx);
            panel.set_mode(insight::workspace::WorkspaceMode::Compare, cx);
        });
        let _ = area;
        insight::focus_insight_workspace_tab(&panel, window, cx);
    }

    /// 首次 render 时装配 DockArea：创建面板实体、订阅事件；左右 dock 由
    /// `apply_left_mode` / `apply_right_mode` 按 `Shared` 初始状态装配。
    fn init_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {        // 启动默认模块（用户点名）：设置 `startup.default_left_panel` 覆盖构造期默认。
        // 必须在这里（首次 render 装配期）读——早于 `apply_left_mode`，晚于设置加载
        //（app 开窗前 `SettingsService::init` 已跑）。未知值回退默认（`from_key` 的约定）。
        {
            let saved = settings::SettingsService::startup_default_left_panel(cx);
            if let Some(panel) = workbench_shell::model::LeftPanel::from_key(&saved) {
                self.shared.active_left.set(panel);
            }
        }
        // 真面板化：初始 tab 集 = 当前激活模块（tab 条只显示点过的模块，
        // 其余模块等用户点活动栏时经 `open_left_panel` 动态挂载）。
        *self.shared.left_open_panels.borrow_mut() = vec![self.shared.active_left.get()];
        *self.shared.right_open_panels.borrow_mut() = vec![self.shared.active_right.get()];
        let shared = self.shared.clone();
        let sidebar = cx.new(|cx| SidebarPanel::new(shared.clone(), &self.editor_service, cx));
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let right_sidebar = cx.new(|cx| RightSidebarPanel::new(shared.clone(), window, cx));
        // 真面板化：左右 dock 各注册 4 个模块 tab（薄壳面板持有内层 crate 视图）。
        // 实体先取出再 `cx.new`：闭包里不能再借 `sidebar` / `right_sidebar`；
        // 薄壳 `set_active` 把 dock 切 tab 同步回活动栏（双向同步的另一半）。
        let nav_entity = sidebar.read(cx).nav_entity();
        let scratch_entity = sidebar.read(cx).scratchpad_entity();
        let resources_entity = sidebar.read(cx).resources_entity();
        let insight_entity = right_sidebar.read(cx).insight_entity();
        let mock_entity = right_sidebar.read(cx).mock_entity();
        let history_entity = right_sidebar.read(cx).history_entity();
        self.left_nav_tab = Some(cx.new(|cx| {
            DatabaseNavPanel::new(shared.clone(), nav_entity, cx)
        }));
        self.left_scratch_tab = Some(cx.new(|cx| {
            ScratchPanel::new(shared.clone(), scratch_entity, cx)
        }));
        self.left_resource_tab = Some(cx.new(|cx| {
            ResourcePanelTab::new(shared.clone(), resources_entity, cx)
        }));
        self.left_plugin_tab = Some(cx.new(|cx| PluginPanel::new(shared.clone(), cx)));
        self.right_insight_tab = Some(cx.new(|cx| {
            InsightPanelTab::new(shared.clone(), insight_entity, cx)
        }));
        self.right_mock_tab = Some(cx.new(|cx| {
            MockPanelTab::new(shared.clone(), mock_entity, cx)
        }));
        self.right_history_tab = Some(cx.new(|cx| {
            HistoryPanelTab::new(shared.clone(), history_entity, cx)
        }));
        self.right_archive_tab = Some(cx.new(|cx| ArchivePanel::new(shared.clone(), cx)));
        // 底部结果 dock 薄壳（Dock 化阶段 3）：只渲染活动文档的结果面板；
        // 开/关由 render 权威点 `sync_result_dock` 按「有结果自动开 / 无结果自动关」驱动
        self.result_dock = Some(cx.new(|cx| ResultPanelDock::new(cx)));
        // B12：旧“编辑区”的 SQL 框已删；M1 的未保存草稿拦截改看**编辑器的未命名文档**
        // （见 `components/project_host.rs` 的 `EditorBridge`），不再需要 `Shared::editor_clear`。
        // S2：编辑区命令端口——导航 / 草稿箱改调这里，不再直写 `Shared` 的请求字段
        // （接线只此一份，见 `docs/architecture/layout/panels-coupling-plan.md` §3）。
        crate::panels::install_editor_bridge(&shared, editor.clone());
        // 草稿箱命令端口（编辑区「全部替换」需要草稿箱轮询在跑）。
        crate::panels::install_scratchpad_bridge(&shared, sidebar.clone());
        // M6：资产库刷新端口（归档 / 取回完成后，发起方只有 `Shared`，而刷新归侧栏面板）。
        crate::panels::install_resources_bridge(&shared, sidebar.clone());
        // M5 Phase C-2 后半：草稿执行回执 → 元数据回写（1 s 一拍，与侧栅是否渲染无关）。
        self.ensure_scratchpad_meta_pump(cx);
        // #54 B：语义诊断目录快照注入（1 s 一拍；复用补全 L1 目录）。
        self.ensure_catalog_pump(cx);

        // 宿主重绘桥再挂一层：除宿主自身，编辑区也要跟上。
        //
        // 导航选中连接会递增 `Shared::nav_cache_epoch`（编辑区下一帧据此丢掉分析库
        // 导航树缓存）；原先由 `SidebarEvent::SelectConnection` 的订阅回调顺带
        // `editor.notify()` 驱动，事件通道退役后收到这里：只唤醒“脏了但自己不知道”
        // 的编辑区，不再依赖它恰好因别的原因重渲染。
        //
        // 为什么这层要 defer、而不是直接同步 notify：见 `install_host_redraw_bridge`
        // 的注释（它会被「编辑区自己 update 里」的动作调到，同步就是 double-lease）。
        crate::panels::install_host_redraw_bridge(&shared, cx.entity().downgrade(), editor.clone());

        // 导航面板的事件通道（`SidebarEvent`）已退役：选中 / 编辑 / 新建 / 开右栏
        // 都改走宿主端口（`database::nav_host::NavHost`，实现在 `components/nav_host.rs`），
        // 动作在事件路径上直接完成，不再绕宿主 render 转发一轮。

        let (area, skin) = DockSkin::dock_area("workspace", Some(1), window, cx);
        // 工程债修复（用户点名）：gpui-kit Dock 的每 tab 关闭按钮默认隐藏
        // （`close_button_visible=false`），此前拿到 skin 后直接丢弃，导致
        // 编辑器 tab 只有标签、没有 ✕ 管理按钮。这里显式启用；面板自身的
        // `closable` 约束（gpui-base `Panel::closable` 默认 true）仍然生效。
        skin.set_close_button_visible(true, cx);
        // 搜索结果标签页对齐 VSCode（2026-10-10 用户截图：搜索 tab 单独存在时
        // 只有标题栏、没有 tab 栏 →「没有展开页面」）：默认 `PanelStyle::Auto`
        // 单面板只画简单标题，改 `TabBar` **总是显示 tab 栏**（VSCode 编辑区/搜索
        // 结果恒有标签页形态；单 tab 也显示「搜索: query」标签 + ✕）。
        skin.set_panel_style(PanelStyle::TabBar, cx);
        // T-1：标签栏「最大宽度 / 样式」设置项装配到 DockSkin（启动一次 + 设置变更
        // 经 `on_settings_changed` 再应用；渲染读取点在 vendor `dock/tab_panel.rs`）。
        // 句柄存字段：设置页回调与后续装配（如重建）都要复用同一份皮肤配置。
        self.dock_skin = Some(skin.clone());
        Self::apply_tab_bar_settings(&skin, cx);

        // 编辑器宿主面板（A9 接线）：把 `editor_service` 的当前文档接进中央 tab 组。
        // 先 clone 到局部再 `cx.new`：闭包里不能再借 `self`。
        let editor_service = self.editor_service.clone();
        let active = editor_service.service().active_id().cloned();
        let document = match active {
            Some(id) => id,
            None => editor_service
                .open(editor::service::OpenRequest::untitled(
                    "",
                    editor::model::EditorMode::Sql,
                ))
                .id()
                .clone(),
        };
        let host_service = editor_service.clone();
        let editor_host = cx
            .new(|cx| editor::view::host::EditorHostPanel::new(host_service, document, window, cx));
        self.editor_hosts.push(editor_host.clone());
        // 脏 tab 自绘 ✕ 走「保存/不保存/取消」需要 area 弱句柄（见 host.rs `set_area`）。
        editor_host.update(cx, |panel, _cx| panel.set_area(area.downgrade()));

        let editor_handle = panel_handle(editor.clone());
        let host_handle = panel_handle(editor_host);
        // M1：项目选择器迁入中央 tab 组（第三个 tab）。
        //
        // 原设计是无项目时用选择器覆盖中央区；但那样 DockArea 在无项目时不挂树，
        // 左右 Dock（Mock / 洞察 / 历史面板）整体不可达。Mock 面板「纯生成不受
        // 影响」（mock-architecture I8「未打开项目也能跑」），无项目时点 Mock
        // 图标必须能打开右 Dock——选择器面板 `visible()` 随 `host.session` 开关：
        // 无项目可见并激活（打开项目入口），有项目隐藏（Dock 渲染回退编辑器）。
        if self.project_inputs.is_none() {
            self.project_inputs = Some(project::ui::ProjectInputs::new(window, cx));
        }
        let inputs = self
            .project_inputs
            .clone()
            .expect("project inputs initialized");
        let picker_panel = cx.new(|cx| {
            crate::panels::ProjectPickerPanel::new(
                shared.clone(),
                self.project_host().clone(),
                inputs,
                cx,
            )
        });
        self.project_picker = Some(picker_panel.clone());
        let picker_handle = panel_handle(picker_panel);
        // 先取 tab id 再进闭包：`area.update` 的闭包会捕获（move）`picker_handle`，
        // 激活用的 id 提前算好，避免 Arc 在闭包外被二次使用（E0382）。
        let picker_id = picker_handle.panel_id(cx);
        area.update(cx, |area, cx| {
            // 中央编辑区单独占满（左右 dock 独立装配，不用 h_split）。
            // A9：标签条交给 Dock——同一个 tab 组里三个面板 = 三个标签。
            area.set_center(
                DockLayout::tabs()
                    .panel_view(editor_handle, cx)
                    .panel_view(host_handle, cx)
                    .panel_view(picker_handle, cx),
                window,
                cx,
            );
        });
        // 无项目启动：激活选择器 tab（有项目时默认激活编辑器——set_center 后首个 tab）。
        if self.project_host().session.borrow().is_none() {
            area.update(cx, |area, cx| area.select_panel(picker_id, window, cx));
        }

        // 属性面板（中央 Dock 独立 tab，单实例）：双击对象 / F4 → 打开或聚焦。
        // 与 Mock 详情同例：面板实体只建一次，重复请求聚焦 + 替换内容。
        {
            let shared_for_prop = shared.clone();
            let area_for_prop = area.clone();
            *shared.open_property.borrow_mut() = Some(Rc::new(
                move |request: PropertyRequest, window: &mut Window, cx: &mut App| {
                    let Some(panel) = shared_for_prop
                        .property_panel
                        .borrow()
                        .clone()
                        .and_then(|weak| weak.upgrade())
                    else {
                        // 面板未建 / 已关闭：重建并加入中央 tab 组。
                        let panel = cx.new(|cx| {
                            crate::panels::PropertyHostPanel::new(shared_for_prop.clone(), cx)
                        });
                        shared_for_prop
                            .property_panel
                            .borrow_mut()
                            .replace(panel.downgrade());
                        // 同 editor host：带 presentation handle 注册，tab 标题用 `tab_name`
                        // 而非 panel_name（属性面板实现了 `tab_name`，见 panels/property_panel.rs）。
                        let prop_handle = panel_handle(panel.clone());
                        area_for_prop.update(cx, |area, cx| {
                            area.add_panel_view(
                                prop_handle,
                                DockPlacement::Center,
                                None,
                                window,
                                cx,
                            );
                        });
                        panel.update(cx, |panel, cx| panel.show_properties(request, cx));
                        crate::panels::focus_property_tab(&panel, window, cx);
                        return;
                    };
                    // 已存在：聚焦即可，内容替换（同一 tab 复用）。
                    crate::panels::focus_property_tab(&panel, window, cx);
                    panel.update(cx, |panel, cx| panel.show_properties(request, cx));
                },
            ));
        }

        // M7：宿主命令——打开某张表 / 草稿的详情 tab（面板「查看详情」与结果表清单调用）。
        // 详情与配置面板同属 mock crate：**一个目标一个 tab**（按 `DetailTarget::key` 去重），
        // 已存在就聚焦。面板实体在 `RightSidebarPanel` 构造期已登记弱句柄，这里只负责接入中央 tab 组。
        {
            let shared_for_detail = shared.clone();
            let area_for_detail = area.clone();
            *shared.open_mock_detail.borrow_mut() = Some(Rc::new(
                move |target: mock::mock_view::DetailTarget, window: &mut Window, cx: &mut App| {
                    let Some(panel) = shared_for_detail
                        .mock_panel
                        .borrow()
                        .clone()
                        .and_then(|weak| weak.upgrade())
                    else {
                        return;
                    };
                    let key = target.key();
                    let alive = shared_for_detail
                        .mock_details
                        .borrow()
                        .get(&key)
                        .cloned()
                        .and_then(|weak| weak.upgrade());
                    if let Some(detail) = alive {
                        // 已在 Dock 中（tab 被切走也只是失焦）：聚焦即可，不重复加入。
                        // 必须在实体更新之外调用：闭包里调会因 TabGroup 回读本实体而 double lease panic。
                        focus_detail_tab(&detail, window, cx);
                        return;
                    }
                    let detail = cx.new(|cx| MockDetailView::new(panel.clone(), target, cx));
                    shared_for_detail
                        .mock_details
                        .borrow_mut()
                        .insert(key, detail.downgrade());
                    let detail_handle = panel_handle(detail.clone());
                    area_for_detail.update(cx, |area, cx| {
                        area.add_panel_view(
                            detail_handle,
                            DockPlacement::Center,
                            None,
                            window,
                            cx,
                        );
                    });
                    focus_detail_tab(&detail, window, cx);
                },
            ));
        }

        // 编辑面板通知级联到宿主：对话框层挂在宿主 render 中（`Root` 的 notify
        // 不会传到子视图），而对话框内部的状态变化（切 Tab / 增删跳 / 测试结果等）
        // 都以 EditorPanel 的 notify 驱动，需同步宿主重绘才能更新层内容。
        self._editor_subscription = Some(cx.observe(&editor, |_, _, cx| cx.notify()));

        // M6：左栏「资产库」的选中 / 快照变化要唤醒右栏「存档详情」。**面板之间不互相订阅**
        // （跨 crate 的视图无从知道对方存在），这条线由宿主持有——右侧详情只读左栏的选中，
        // 方向单一。右栏没在显示存档详情时不唤醒：切过去的那一帧会自然重渲染。
        if let Some(panel) = shared
            .resources_panel
            .borrow()
            .clone()
            .and_then(|weak| weak.upgrade())
        {
            let shared_for_archive = shared.clone();
            // 真面板化：唤醒目标是右 dock 的「存档详情」tab（不再是右栏外壳实体）。
            let right_for_archive = self.right_archive_tab.clone();
            self._archive_subscription = Some(cx.observe(&panel, move |_, _, cx| {
                if shared_for_archive.active_right.get() == RightPanel::Archive {
                    if let Some(archive) = right_for_archive.clone() {
                        archive.update(cx, |_, cx| cx.notify());
                    }
                }
            }));
        }
        self.area = Some(area);
        self.sidebar = Some(sidebar);
        self.editor = Some(editor);
        self.right_sidebar = Some(right_sidebar);

        // A12 + E3：恢复上次的全部编辑器会话（读库是 I/O → 放到 `defer_in`，不在构造/渲染里做）
        cx.defer_in(window, |this, window, cx| {
            this.restore_sessions(window, cx);
        });
    }

    // ===== 三模式：Shared 状态 → Dock 同步（render 权威） =====

    /// M6：请求一次资产库列表刷新（**事件路径**：激活面板 / 打开或切换项目之后）。
    ///
    /// 入队要带项目根与只读标志（都在 `Shared` 上），并且入队后要启动轮询回填 ——
    /// 两件事都由侧栏面板持有，所以统一经它转一手（宿主不直接碰 `resource_jobs`）。
    pub(crate) fn request_resources_refresh(&self, cx: &mut Context<Self>) {
        let Some(sidebar) = self.sidebar.clone() else {
            return;
        };
        sidebar.update(cx, |panel, cx| panel.request_resources_refresh(cx));
    }

    /// 草稿箱随项目切换重新加载（on_opened 事件路径，与资产库同一形态）。
    pub(crate) fn request_scratchpad_refresh(&self, cx: &mut Context<Self>) {
        let Some(sidebar) = self.sidebar.clone() else {
            return;
        };
        sidebar.update(cx, |panel, cx| panel.request_scratchpad_refresh(cx));
    }

    /// 真面板化装配：只挂**已打开过**的模块 tab（`Shared::left_open_panels`）。
    /// 空列表兜底为当前激活模块（启动早期 / 状态被清空的防御）。
    fn left_dock_layout(&self, cx: &Context<Self>) -> Option<DockLayout> {
        // 类型交给推断：`panel_handle` 返回 `Arc<dyn base::PanelView>`。
        let mut handles = Vec::new();
        let open = self.shared.left_open_panels.borrow();
        for panel in open.iter() {
            match panel {
                LeftPanel::Draft => {
                    if let Some(tab) = &self.left_scratch_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                LeftPanel::Database => {
                    if let Some(tab) = &self.left_nav_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                LeftPanel::Resources => {
                    if let Some(tab) = &self.left_resource_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                LeftPanel::Plugin => {
                    if let Some(tab) = &self.left_plugin_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
            }
        }
        drop(open);
        if handles.is_empty() {
            // 兜底：空列表时至少挂当前激活模块（各 arm 同构为 `Option<Arc<dyn PanelView>>`）。
            let active = self.shared.active_left.get();
            let tab = match active {
                LeftPanel::Draft => self.left_scratch_tab.clone().map(panel_handle),
                LeftPanel::Database => self.left_nav_tab.clone().map(panel_handle),
                LeftPanel::Resources => self.left_resource_tab.clone().map(panel_handle),
                LeftPanel::Plugin => self.left_plugin_tab.clone().map(panel_handle),
            };
            if let Some(handle) = tab {
                handles.push(handle);
            }
        }
        if handles.is_empty() {
            return None;
        }
        let mut layout = DockLayout::tabs();
        for handle in handles {
            layout = layout.panel_view(handle, cx);
        }
        Some(layout)
    }

    /// 底部结果 dock 布局（Dock 化阶段 3）：单面板 = 结果薄壳。
    fn bottom_dock_layout(&self, cx: &Context<Self>) -> Option<DockLayout> {
        let Some(dock) = self.result_dock.clone() else {
            return None;
        };
        let mut layout = DockLayout::tabs();
        layout = layout.panel_view(panel_handle(dock), cx);
        Some(layout)
    }

    /// 底部结果 dock 同步（render 权威点，Dock 化阶段 3）：
    /// 活动文档有结果集 → 自动开（12rem 起步，用户可拖拽）；无结果 / 无活动 → 自动关不占位。
    /// 结果变化走 `StatusChanged` → 宿主重绘 → 每帧重判（`has_dock` / `is_dock_open` 幂等，
    /// 不会反复开关）。薄壳槽与渲染共用同一 Rc<RefCell>，这里换槽即换渲染目标。
    fn sync_result_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dock) = self.result_dock.clone() else {
            return;
        };
        let active_id = self.editor_service.service().active_id().cloned();
        let active_host = active_id.as_ref().and_then(|id| {
            self.editor_hosts
                .iter()
                .find(|panel| panel.read(cx).document() == id)
                .cloned()
        });
        let has_results = active_host
            .as_ref()
            .map(|host| host.read(cx).result_panel().read(cx).has_content())
            .unwrap_or(false);
        let slot = dock.read(cx).active_host();
        *slot.borrow_mut() = active_host.as_ref().map(|host| host.downgrade());
        let layout = self.bottom_dock_layout(cx);
        if let Some(area) = self.area.clone()
            && let Some(layout) = layout
        {
            area.update(cx, |area, cx| {
                if has_results {
                    if !area.has_dock(DockPlacement::Bottom) {
                        area.set_dock(DockPlacement::Bottom, layout, window, cx);
                        // 起步高度 12rem（用户已确认；结构尺寸登记在 ui.rs）
                        area.set_dock_size(
                            DockPlacement::Bottom,
                            cx.theme().font_size * ui::RESULT_DOCK_HEIGHT,
                            window,
                            cx,
                        );
                    } else if !area.is_dock_open(DockPlacement::Bottom) {
                        area.toggle_dock(DockPlacement::Bottom, window, cx);
                    }
                } else if area.has_dock(DockPlacement::Bottom)
                    && area.is_dock_open(DockPlacement::Bottom)
                {
                    area.toggle_dock(DockPlacement::Bottom, window, cx);
                }
            });
        }
    }

    fn apply_left_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.left_mode.get();
        let (Some(area), Some(layout)) = (self.area.clone(), self.left_dock_layout(cx)) else {
            return;
        };
        // Hidden → Expanded 重建 dock 时，把当前激活模块设为 dock 初始 tab。
        // 这里同时留句柄：dock 已存在时要把 active 面板**挂载并激活**（与右 dock
        // 同一补丁——`Shared::open_left_panel` 等无窗口入口只改状态，dock 同步必须
        // 由宿主这拍完成，否则命令面板切不过去 tab）。
        let active_tab = match self.shared.active_left.get() {
            LeftPanel::Draft => self.left_scratch_tab.clone().map(panel_handle),
            LeftPanel::Database => self.left_nav_tab.clone().map(panel_handle),
            LeftPanel::Resources => self.left_resource_tab.clone().map(panel_handle),
            LeftPanel::Plugin => self.left_plugin_tab.clone().map(panel_handle),
        };
        let active_id = active_tab.as_ref().map(|handle| handle.panel_id(cx));
        area.update(cx, |area, cx| {
            // DockLayout 不可 clone：Expanded / Collapsed 两分支互斥，各自消费 layout。
            if mode == SidebarMode::Expanded {
                if !area.has_dock(DockPlacement::Left) {
                    area.set_dock(DockPlacement::Left, layout, window, cx);
                    // 起步宽度 240px（= 15rem 基准，随界面缩放；layout-design §2.3，用户拖拽可调）。
                    area.set_dock_size(
                        DockPlacement::Left,
                        cx.theme().font_size * ui::LEFT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                    if let Some(id) = active_id.clone() {
                        area.select_panel(id, window, cx);
                    }
                } else if !area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                } else if let (Some(handle), Some(id)) = (active_tab, active_id) {
                    // 面板化后左 dock 是动态 tab 集：active 模块未挂载 → 挂载（自动激活）；
                    // 已挂载但未激活 → 切激活。`select_panel` / `add_panel_view` 幂等。
                    if area.panel(id.clone()).is_none() {
                        area.add_panel_view(handle.clone(), DockPlacement::Left, None, window, cx);
                    } else {
                        area.select_panel(id.clone(), window, cx);
                    }
                }
            } else if mode == SidebarMode::Collapsed {
                if !area.has_dock(DockPlacement::Left) {
                    area.set_dock(DockPlacement::Left, layout, window, cx);
                    area.set_dock_size(
                        DockPlacement::Left,
                        cx.theme().font_size * ui::LEFT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                }
                if area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                }
            } else if area.has_dock(DockPlacement::Left) {
                area.remove_dock(DockPlacement::Left, window, cx);
            }
        });
    }

    /// 右 dock 装配（与 `left_dock_layout` 同构：只挂已打开的模块 tab）。
    fn right_dock_layout(&self, cx: &Context<Self>) -> Option<DockLayout> {
        // 类型交给推断：`panel_handle` 返回 `Arc<dyn base::PanelView>`。
        let mut handles = Vec::new();
        let open = self.shared.right_open_panels.borrow();
        for panel in open.iter() {
            match panel {
                RightPanel::Insight => {
                    if let Some(tab) = &self.right_insight_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                RightPanel::Mock => {
                    if let Some(tab) = &self.right_mock_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                RightPanel::History => {
                    if let Some(tab) = &self.right_history_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                RightPanel::Archive => {
                    if let Some(tab) = &self.right_archive_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
            }
        }
        drop(open);
        if handles.is_empty() {
            // 兜底：空列表时至少挂当前激活模块（各 arm 同构为 `Option<Arc<dyn PanelView>>`）。
            let active = self.shared.active_right.get();
            let tab = match active {
                RightPanel::Insight => self.right_insight_tab.clone().map(panel_handle),
                RightPanel::Mock => self.right_mock_tab.clone().map(panel_handle),
                RightPanel::History => self.right_history_tab.clone().map(panel_handle),
                RightPanel::Archive => self.right_archive_tab.clone().map(panel_handle),
            };
            if let Some(handle) = tab {
                handles.push(handle);
            }
        }
        if handles.is_empty() {
            return None;
        }
        let mut layout = DockLayout::tabs();
        for handle in handles {
            layout = layout.panel_view(handle, cx);
        }
        Some(layout)
    }

    fn apply_right_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.right_mode.get();
        let (Some(area), Some(layout)) = (self.area.clone(), self.right_dock_layout(cx)) else {
            return;
        };
        // Hidden → Expanded 重建 dock 时，把当前激活模块设为 dock 初始 tab。
        // 这里同时留句柄：dock 已存在时要把 active 面板**挂载并激活**（见下方
        // Expanded 分支的补丁——`Shared::open_right_panel` 等无窗口入口只改状态，
        // dock 同步必须由宿主这拍完成，否则 tab 切不过去）。
        let active_tab = match self.shared.active_right.get() {
            RightPanel::Insight => self.right_insight_tab.clone().map(panel_handle),
            RightPanel::Mock => self.right_mock_tab.clone().map(panel_handle),
            RightPanel::History => self.right_history_tab.clone().map(panel_handle),
            RightPanel::Archive => self.right_archive_tab.clone().map(panel_handle),
        };
        let active_id = active_tab.as_ref().map(|handle| handle.panel_id(cx));
        area.update(cx, |area, cx| {
            // DockLayout 不可 clone：Expanded / Collapsed 两分支互斥，各自消费 layout。
            if mode == SidebarMode::Expanded {
                if !area.has_dock(DockPlacement::Right) {
                    area.set_dock(DockPlacement::Right, layout, window, cx);
                    // 起步宽度 280px（= 17.5rem 基准）。
                    area.set_dock_size(
                        DockPlacement::Right,
                        cx.theme().font_size * ui::RIGHT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                    if let Some(id) = active_id.clone() {
                        area.select_panel(id, window, cx);
                    }
                } else if !area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
                // 面板化后右 dock 是动态 tab 集：active 模块未挂载 → 挂载（自动激活）；
                // 已挂载但未激活 → 切激活。`select_panel` / `add_panel_view` 幂等，
                // render 每帧调用无副作用（与 `apply_left_mode` 的 set_dock 同先例）。
                if let (Some(handle), Some(id)) = (&active_tab, &active_id) {
                    if area.panel(id.clone()).is_none() {
                        area.add_panel_view(
                            handle.clone(),
                            DockPlacement::Right,
                            None,
                            window,
                            cx,
                        );
                    } else {
                        area.select_panel(id.clone(), window, cx);
                    }
                }
            } else if mode == SidebarMode::Collapsed {
                if !area.has_dock(DockPlacement::Right) {
                    area.set_dock(DockPlacement::Right, layout, window, cx);
                    area.set_dock_size(
                        DockPlacement::Right,
                        cx.theme().font_size * ui::RIGHT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                }
                if area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
            } else if area.has_dock(DockPlacement::Right) {
                area.remove_dock(DockPlacement::Right, window, cx);
            }
        });
    }

    // ===== 活动栏 ↔ dock tab 双向同步（事件路径，另一半在模块 tab 的 `set_active`） =====

    /// 左活动栏点击 → 打开对应模块：
    /// 1. 登记进 `left_open_panels`（tab 条只显示点过的模块）；
    /// 2. 已在 dock → `select_panel` 切激活；不在 → `add_panel_view` 挂载（自动激活）；
    /// 3. dock 不存在（Hidden）→ 不动作，render 的 `apply_left_mode` 按 open 列表重建。
    fn open_left_panel(&self, panel: LeftPanel, window: &mut Window, cx: &mut Context<Self>) {
        {
            let mut open = self.shared.left_open_panels.borrow_mut();
            if !open.contains(&panel) {
                open.push(panel);
            }
        }
        let Some(area) = self.area.clone() else {
            return;
        };
        // 各 tab 类型不同：match 统一返回 `(handle, id)` 元组（类型一致）。
        let (handle, id) = match panel {
            LeftPanel::Draft => match self.left_scratch_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            LeftPanel::Database => match self.left_nav_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            LeftPanel::Resources => match self.left_resource_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            LeftPanel::Plugin => match self.left_plugin_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
        };
        area.update(cx, |area, cx| {
            if area.panel(id).is_some() {
                // 已打开过：切 tab（dock 关闭时也改树，render 打开后即正确）。
                area.select_panel(id, window, cx);
            } else if area.has_dock(DockPlacement::Left) {
                // 首次打开：挂载进左 dock 第一个 tab 组（`add_panel_view` 自带激活）。
                area.add_panel_view(handle, DockPlacement::Left, None, window, cx);
            }
        });
    }

    /// 右活动栏点击 → 打开对应模块（同上，右侧 dock）。
    fn open_right_panel(&self, panel: RightPanel, window: &mut Window, cx: &mut Context<Self>) {
        {
            let mut open = self.shared.right_open_panels.borrow_mut();
            if !open.contains(&panel) {
                open.push(panel);
            }
        }
        let Some(area) = self.area.clone() else {
            return;
        };
        // 各 tab 类型不同：match 统一返回 `(handle, id)` 元组（类型一致）。
        let (handle, id) = match panel {
            RightPanel::Insight => match self.right_insight_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            RightPanel::Mock => match self.right_mock_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            RightPanel::History => match self.right_history_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            RightPanel::Archive => match self.right_archive_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
        };
        area.update(cx, |area, cx| {
            if area.panel(id).is_some() {
                area.select_panel(id, window, cx);
            } else if area.has_dock(DockPlacement::Right) {
                area.add_panel_view(handle, DockPlacement::Right, None, window, cx);
            }
        });
    }

    // ===== 标题栏 =====
    // 差异点：无窗口标题文本。承载 gpui-kit 官方 TitleBar（对应 layout-design.md §2.1）：
    // - 窗口拖拽（Drag hitbox，Windows 系统级）、双击最大化、窗口控制按钮（─ □ ✕）由组件自带；
    // - 高度覆盖为 36px、背景覆盖为主题 title_bar 纯色（对齐 layout-proposal.html v5）。

    /// 快速布局预设（UI 先行，未接线）：仅提供菜单外观与文案，点击暂不改变布局状态。
    /// 接线时每项应一次性写入 `left_mode`/`right_mode`（复用 `apply_*_mode` 同步到 dock）。
    const QUICK_LAYOUT_PRESETS: &[(&str, &str)] = &[
        ("left", "仅左栏"),
        ("right", "仅右栏"),
        ("both", "双栏"),
        ("full", "全屏"),
    ];

    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let pt = settings::product_tokens::get(cx);
        let entity = cx.entity();

        // 软件图标随当前实际明暗主题取 light/ 或 dark/ 下的 32px PNG：System 策略已在
        // Theme::change 阶段解析为 Light/Dark，这里以 theme.is_dark() 为准（与取色同源）。
        // 路径走 `paths::assets_dir()`：发布包取可执行文件同级 `assets/`，开发期回退仓库那份。
        let icon_dir = if theme.is_dark() { "dark" } else { "light" };
        let icon_path = paths::assets_dir()
            .join("icons")
            .join(icon_dir)
            .join("32x32.png");
        let logo = img(icon_path.as_path()).w_5().h_5().rounded_sm();

        // 挖空项目槽：产品语义 token `title_bar.slot.background`。
        // 项目名取自当前项目会话（P0）；未打开项目时显示占位。
        let has_project = self.shared.project.borrow().is_some();
        let project_name = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未打开项目".to_string());
        // 触发器用语义控件（`Selectable` + 键盘可达由 `Button` 自带），不自己画可点 div。
        // 2026-10-19：随标题栏调高到 28px；加文件夹图标 + 1px 边框，让“挖空”槽在
        // 标题栏上有明确的可视轮廓（此前只有背景色，显得“显示不清楚”）。
        let slot = Button::new("project-slot")
            .ghost()
            .h(rems(ui::TITLE_SLOT_HEIGHT))
            .px_3p5()
            .rounded_md()
            .border_1()
            .border_color(theme.colors.border)
            .bg(pt.title_bar_slot_background(theme))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(CatalogIcon::Folder).size_3p5())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("项目"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.colors.foreground)
                            .child(project_name),
                    ),
            );

        // M1：有项目时项目槽本身就是菜单触发器（`Button::dropdown_menu`）——方向键导航 /
        // Enter / Escape / 点击外部关闭、焦点恢复都由语义组件负责，宿主不再持有 `menu_open`
        // 之类的开关状态（组件内部状态才是权威）。菜单内容由 `project` crate 提供。
        let slot: AnyElement = if has_project {
            let host = self.project_host().clone();
            let inputs = self
                .project_inputs
                .clone()
                .expect("project inputs lazy init");
            slot.dropdown_menu(move |menu, _, _| {
                project::ui::build_project_menu(menu, &host, &inputs)
            })
            .into_any_element()
        } else {
            slot.into_any_element()
        };

        // Quick Open 入口（点击唤起，Ctrl+P 见 commands.rs 绑定）：与快捷键同一入口——
        // 打开、清空上次输入、聚焦输入框；真正输入在浮层里（单一权威输入）。
        let qo_entity = entity.clone();
        let quick_open = div()
            .id("quick-open-entry")
            .role(gpui::Role::Button)
            .test_support()
            .h_flex()
            .items_center()
            .gap_2()
            .w_80()
            .h(rems(ui::QUICK_OPEN_ENTRY_HEIGHT))
            .px_2p5()
            .rounded_sm()
            .bg(theme.colors.border)
            .cursor_pointer()
            .child(Icon::new(IconName::Search).size_3p5())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("搜索连接、命令…"),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("Ctrl+P"),
            )
            .on_click(move |_, window, app| {
                qo_entity.update(app, |this, cx| {
                    let _ = window;
                    this.toggle_quick_open(Mode::Default, cx)
                });
            })
            // 2026-10-19 修复：gpui-kit TitleBar 把整个 bar（含全部 children）标记为
            // `WindowControlArea::Drag`，Windows 无边框窗口下点击命中 HTCAPTION 会被
            // 系统当作标题栏拖动吞掉，on_click 永远收不到（主题按钮“无法切换”根因）。
            // `.occlude()` 让本元素的 hitbox 挡掉后方（父 bar）的 Drag hitbox，命中时
            // 不再返回 HTCAPTION，点击正常派发。
            .occlude();

        // 主题切换入口：图标随当前外观策略（暗→月、亮→日、跟随系统→显示器），
        // 点击走与 Ctrl+Shift+L 同一条命令路径（`ToggleThemeMode`）。
        let appearance = SettingsService::theme_mode(cx);
        let theme_icon = match appearance {
            settings::model::AppearanceMode::Dark => CatalogIcon::Moon,
            settings::model::AppearanceMode::System => CatalogIcon::Monitor,
            settings::model::AppearanceMode::Light => CatalogIcon::Sun,
        };
        let theme_toggle = Button::new("theme-toggle")
            .ghost()
            .icon(Icon::new(theme_icon).size_3p5())
            .tooltip("切换明暗主题 (Ctrl+Shift+L)")
            .on_click(|_, window, app| {
                SettingsService::toggle_theme_mode(Some(window), app);
            });
        // 标题栏可点击控件统一包 `occlude()`：从父 bar 的 `WindowControlArea::Drag`
        // 命中区剔除（见 quick-open-entry 注释），否则 Windows 上点击被系统吞掉。
        let theme_toggle = div()
            .h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(theme_toggle);

        // 语言切换入口：先做入口与写库，翻译覆盖是渐进的（`tr()` 逐处替换）。
        let lang_switch = Button::new("lang-switch")
            .ghost()
            .icon(Icon::new(CatalogIcon::Languages).size_3p5())
            .tooltip("界面语言")
            .dropdown_menu(|menu, _, _| {
                let mut menu = menu.item(PopupMenuItem::label("界面语言"));
                menu = menu.item(PopupMenuItem::separator());
                menu = menu.item(
                    PopupMenuItem::new("简体中文").on_click(|_, window, app| {
                        SettingsService::set_locale("zh-CN", Some(window), app);
                    }),
                );
                menu.item(PopupMenuItem::new("English").on_click(
                    |_, window, app| {
                        SettingsService::set_locale("en-US", Some(window), app);
                    },
                ))
            });
        let lang_switch = div()
            .h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(lang_switch);

        // 三栏布局：左右 flex_1 占位对称，Quick Open 严格居中；
        // 右侧窗口控制按钮（─ □ ✕）由 TitleBar 自带渲染，无需自绘。
        TitleBar::new()
            .h(rems(ui::TITLE_BAR_HEIGHT))
            .pl_2p5()
            .bg(theme.colors.title_bar)
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(logo)
                    .child(
                        div()
                            .h_flex()
                            .flex_none()
                            .items_center()
                            .occlude()
                            .child(slot),
                    ),
            )
            .child(quick_open)
            .child(div().flex_1())
            .child(theme_toggle)
            .child(lang_switch)
            .child(
                div()
                    .h_flex()
                    .flex_none()
                    .items_center()
                    .occlude()
                    .child(self.render_quick_layout_button(cx)),
            )
    }

    // 快速布局下拉：布局图标 + 预设菜单。已接线——每项一次性写入左右 dock 模式
    // （复用 `apply_left_mode`/`apply_right_mode` 同步到 dock，见 `apply_quick_layout`）。
    fn render_quick_layout_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("quick-layout")
            .ghost()
            .icon(IconName::LayoutDashboard)
            .tooltip("快速布局")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.item(PopupMenuItem::label("布局预设"));
                menu = menu.item(PopupMenuItem::separator());
                for (key, label) in Self::QUICK_LAYOUT_PRESETS.iter().copied() {
                    let entity = entity.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label).on_click(move |_, window, app| {
                            entity.update(app, |this, cx| {
                                this.apply_quick_layout(key, window, cx);
                            });
                        }),
                    );
                }
                menu
            })
    }

    /// 快速布局预设接线：一次性写入左右 dock 模式（复用 `apply_*_mode` 同步）。
    fn apply_quick_layout(&self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        use SidebarMode::*;
        let (lm, rm) = match key {
            "left" => (Expanded, Collapsed),
            "right" => (Collapsed, Expanded),
            "both" => (Expanded, Expanded),
            // 全屏：双栏收起（活动栏保留，dock 以 strip 收边，可再点开）。
            "full" => (Collapsed, Collapsed),
            _ => return,
        };
        if self.shared.left_mode.get() != lm {
            self.shared.left_mode.set(lm);
        }
        if self.shared.right_mode.get() != rm {
            self.shared.right_mode.set(rm);
        }
        self.apply_left_mode(window, cx);
        self.apply_right_mode(window, cx);
    }

    // ===== 活动栏 =====

    fn render_left_activity_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let pt = settings::product_tokens::get(cx);
        let active = self.shared.active_left.get();
        let mut bar = div()
            .v_flex()
            .items_center()
            .w_12()
            .h_full()
            .pt_2()
            .pb_2()
            .gap_1()
            .border_r_1()
            .border_color(theme.colors.border)
            // 活动栏背景（产品语义 token activity_bar.background）。
            .bg(pt.activity_bar_background(theme));

        for panel in LeftPanel::ALL {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let selected = active == panel;
            // 右键菜单需要独立句柄（左键 on_click 已 move 上面的 shared/entity）。
            let shared_menu = shared.clone();
            let entity_menu = entity.clone();
            // VSCode 行为：激活项左侧 2px 亮条（activity_bar.active_border）。
            bar = bar.child(
                div()
                    .w_11()
                    .h_10()
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .border_l_2()
                    .border_color(if selected {
                        pt.activity_bar_active_border(theme)
                    } else {
                        transparent_black()
                    })
                    .child(
                        Button::new(format!("left-activity-{}", panel.label()))
                            .icon(panel.icon())
                            .size_7()
                            .ghost()
                            .selected(selected)
                            .toggled(selected)
                            .on_click(move |_, window, app| {
                                let mode = shared.left_mode.get();
                                // 激活（而非收起）时才需要取数；收起路径不动面板状态。
                                let activating = !(mode == SidebarMode::Expanded
                                    && shared.active_left.get() == panel);
                                if !activating {
                                    // 再次点击当前激活项 → 收起。
                                    shared.left_mode.set(SidebarMode::Collapsed);
                                } else {
                                    shared.active_left.set(panel);
                                    shared.left_mode.set(SidebarMode::Expanded);
                                }
                                let activating_resources =
                                    activating && panel == LeftPanel::Resources;
                                entity.update(app, |this, cx| {
                                    // 真面板化：展开时打开对应模块 tab（首次挂载 / 已开则切换）。
                                    if activating {
                                        this.open_left_panel(panel, window, cx);
                                    }
                                    // M6：切到资产库时取一次列表（结果由面板轮询回填）。
                                    if activating_resources {
                                        this.request_resources_refresh(cx);
                                    }
                                    cx.notify();
                                });
                            })
                            // 右键：隐藏 / 收起整个左侧栏（mode 写入后由 render 的
                            // `apply_left_mode` 同步 dock 树）。
                            .context_menu(move |menu, _, _| {
                                let mut menu = menu.item(PopupMenuItem::label("左侧栏"));
                                menu = menu.item(PopupMenuItem::separator());
                                // builder 是 `Fn`（可多次调用）：每次求值时 clone 再 move，
                                // 保证每轮 on_click 闭包持有自己的句柄。
                                menu = menu.item(
                                    PopupMenuItem::new("隐藏侧边栏").on_click({
                                        let shared = shared_menu.clone();
                                        let entity = entity_menu.clone();
                                        move |_, _, app| {
                                            shared.left_mode.set(SidebarMode::Hidden);
                                            entity.update(app, |_, cx| cx.notify());
                                        }
                                    }),
                                );
                                menu.item(PopupMenuItem::new("收起侧边栏").on_click({
                                    let shared = shared_menu.clone();
                                    let entity = entity_menu.clone();
                                    move |_, _, app| {
                                        shared.left_mode.set(SidebarMode::Collapsed);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }))
                            }),
                    ),
            );
        }

        // 底部：弹性占位 + 分隔线 + 设置入口（Step 5 接 crates/settings）。
        let entity = cx.entity();
        let shared = self.shared.clone();
        bar = bar
            .child(div().flex_1())
            .child(
                div()
                    .w(rems(ui::ACTIVITY_ICON_SIZE))
                    .h(ui::HAIRLINE)
                    .bg(theme.colors.border),
            )
            .child(
                Button::new("left-settings")
                    .icon(IconName::Settings)
                    .size_7()
                    .ghost()
                    .on_click(move |_, _, app| {
                        shared.settings_open.set(true);
                        shared.quick_open.set(false); // 互斥：与 Quick Open 不同时显示
                        entity.update(app, |_, cx| cx.notify());
                    }),
            );
        bar
    }

    fn render_right_activity_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let pt = settings::product_tokens::get(cx);
        let active = self.shared.active_right.get();
        let mut bar = div()
            .v_flex()
            .items_center()
            .w_12()
            .h_full()
            .pt_2()
            .pb_2()
            .gap_1()
            .border_l_1()
            .border_color(theme.colors.border)
            .bg(pt.activity_bar_background(theme));

        for panel in RightPanel::ALL {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let selected = active == panel;
            // 右键菜单需要独立句柄（左键 on_click 已 move 上面的 shared/entity）。
            let shared_menu = shared.clone();
            let entity_menu = entity.clone();
            // 与左侧镜像：激活项 2px 亮条在右。
            bar = bar.child(
                div()
                    .w_11()
                    .h_10()
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .border_r_2()
                    .border_color(if selected {
                        pt.activity_bar_active_border(theme)
                    } else {
                        transparent_black()
                    })
                    .child(
                        Button::new(format!("right-activity-{}", panel.label()))
                            .icon(panel.icon())
                            .size_7()
                            .ghost()
                            .selected(selected)
                            .toggled(selected)
                            .on_click(move |_, window, app| {
                                let mode = shared.right_mode.get();
                                // 激活（而非收起）时才需要重读面板状态；收起路径不动面板。
                                let activating = !(mode == SidebarMode::Expanded
                                    && shared.active_right.get() == panel);
                                if !activating {
                                    // 再次点击当前激活项 → 收起。
                                    shared.right_mode.set(SidebarMode::Collapsed);
                                } else {
                                    // 展开走 `Shared::open_right_panel`：Mock 面板的候选清单与生成历史
                                    // 就在那儿重读（四个入口共用这一条，见该方法的注释）。
                                    shared.open_right_panel(panel, app);
                                }
                                entity.update(app, |this, cx| {
                                    // 真面板化：展开时打开对应模块 tab（首次挂载 / 已开则切换）。
                                    if activating {
                                        this.open_right_panel(panel, window, cx);
                                    }
                                    cx.notify();
                                });
                            })
                            // 右键：隐藏 / 收起整个右侧栏（mode 写入后由 render 的
                            // `apply_right_mode` 同步 dock 树）。
                            .context_menu(move |menu, _, _| {
                                let mut menu = menu.item(PopupMenuItem::label("右侧栏"));
                                menu = menu.item(PopupMenuItem::separator());
                                // builder 是 `Fn`（可多次调用）：每次求值时 clone 再 move，
                                // 保证每轮 on_click 闭包持有自己的句柄。
                                menu = menu.item(
                                    PopupMenuItem::new("隐藏侧边栏").on_click({
                                        let shared = shared_menu.clone();
                                        let entity = entity_menu.clone();
                                        move |_, _, app| {
                                            shared.right_mode.set(SidebarMode::Hidden);
                                            entity.update(app, |_, cx| cx.notify());
                                        }
                                    }),
                                );
                                menu.item(PopupMenuItem::new("收起侧边栏").on_click({
                                    let shared = shared_menu.clone();
                                    let entity = entity_menu.clone();
                                    move |_, _, app| {
                                        shared.right_mode.set(SidebarMode::Collapsed);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }))
                            }),
                    ),
            );
        }

        let entity = cx.entity();
        let shared = self.shared.clone();
        bar = bar
            .child(div().flex_1())
            .child(
                div()
                    .w(rems(ui::ACTIVITY_ICON_SIZE))
                    .h(ui::HAIRLINE)
                    .bg(theme.colors.border),
            )
            .child(
                Button::new("right-settings")
                    .icon(IconName::Settings)
                    .size_7()
                    .ghost()
                    .on_click(move |_, _, app| {
                        shared.settings_open.set(true);
                        shared.quick_open.set(false); // 互斥：与 Quick Open 不同时显示
                        entity.update(app, |_, cx| cx.notify());
                    }),
            );
        bar
    }

    /// 设置页 overlay：居中渲染 `SettingsPage`（懒创建实体）。
    ///
    /// 页面自持两栏结构（分节与行由 `settings::registry` 驱动）；宿主只提供 overlay、
    /// **宿主桥**（关闭 / 缓存管理，副作用在宿主侧）与互斥规则（与 Quick Open 不同时显示）。
    /// T-1：把「标签栏最大宽度 / 样式」设置项装配到 DockSkin（启动一次 + 设置变更
    /// 经 `on_settings_changed` 重应用）。vendor `dock/tab_panel.rs` 渲染时从
    /// `SkinShared` 读取这两个值；未知样式字符串回退 `Tab`（设置页只给五档预设）。
    fn apply_tab_bar_settings(skin: &Rc<DockSkin>, cx: &mut App) {
        let appearance = &settings::SettingsService::get(cx).appearance;
        skin.set_tab_bar_max_width(appearance.tab_bar_max_width, cx);
        let variant = match appearance.tab_bar_variant.as_str() {
            "outline" => TabVariant::Outline,
            "pill" => TabVariant::Pill,
            "segmented" => TabVariant::Segmented,
            "underline" => TabVariant::Underline,
            _ => TabVariant::Tab,
        };
        skin.set_tab_bar_variant(variant, cx);
    }

    fn render_settings_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.shared.settings_open.get() {
            return None;
        }
        if self.settings_page.is_none() {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let on_close: std::rc::Rc<dyn Fn(&mut App)> = std::rc::Rc::new(move |app| {
                shared.settings_open.set(false);
                entity.update(app, |_, cx| cx.notify());
            });
            let shared_cache = self.shared.clone();
            let on_open_cache: std::rc::Rc<dyn Fn(&mut Window, &mut App)> =
                std::rc::Rc::new(move |window, app| {
                    crate::components::cache_dialog::open_cache_dialog(window, app, &shared_cache);
                });
            // 「打开日志目录」/「查看日志」：设置层不依赖 engine / opener，这两个副作用由宿主实现。
            // 目录不存在（还没写过日志）就先建出来——"打开一个不存在的目录"是纯失败体验。
            let on_open_log_dir: std::rc::Rc<dyn Fn(&mut Window, &mut App)> =
                std::rc::Rc::new(move |_window, _app| {
                    let dir = paths::log_dir();
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        eprintln!("[settings] 创建日志目录失败 {}: {e}", dir.display());
                        return;
                    }
                    if let Err(e) = opener::open(&dir) {
                        eprintln!("[settings] 打开日志目录失败 {}: {e}", dir.display());
                    }
                });
            let on_open_log_view: std::rc::Rc<dyn Fn(&mut Window, &mut App)> =
                std::rc::Rc::new(move |window, app| {
                    crate::components::log_dialog::open_log_dialog(window, app);
                });
            let on_open_type_short_map: std::rc::Rc<dyn Fn(&mut Window, &mut App)> =
                std::rc::Rc::new(move |window, app| {
                    crate::components::type_short_map_dialog::open_type_short_map_dialog(window, app);
                });
            // T-1：设置落盘 → 重应用标签栏外观到 DockSkin（启动值在 `init_workspace`
            // 已应用一次；这里是运行时路径）。皮肤句柄在装配期已存字段。
            let dock_skin = self.dock_skin.clone();
            // 洞察偏好（`insight.*` 四项）变更也在这里推送：内存上限走 engine 连接级
            // 设置（即时生效），面板偏好（内存显示开关 / 工作集阈值）转发给面板实体。
            // 与 sink（`install_insight_memory_limit_sink`）双写幂等，这里兜底启动与面板重建。
            let shared_for_insight = self.shared.clone();
            let on_settings_changed: std::rc::Rc<dyn Fn(&mut App)> =
                std::rc::Rc::new(move |app| {
                    if let Some(skin) = &dock_skin {
                        Self::apply_tab_bar_settings(skin, app);
                    }
                    crate::components::insight_actions::apply_insight_prefs(
                        &shared_for_insight,
                        app,
                    );
                });
            let host = SettingsHost {
                on_close,
                on_open_cache,
                on_open_log_dir,
                on_open_log_view,
                on_open_type_short_map,
                on_settings_changed,
            };
            self.settings_page = Some(cx.new(|cx| SettingsPage::new(window, host, cx)));
        }
        let theme = cx.theme().clone();
        let page = self
            .settings_page
            .clone()
            .expect("settings page initialized");
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.colors.overlay)
                .child(page),
        )
    }

    // ===== Quick Open（统一检索 / 命令面板）宿主装配 =====
    //
    // 浮层本体（输入 / 结果 / 键盘通道 / 防抖与回填）在 `quick_open::palette`；
    // 这里只负责：懒创建、挂 overlay、注入动作端口、开关（`Shared::quick_open` 是全应用一份的权威）。
    // 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`。

    /// 打开 / 关闭 Quick Open（标题栏入口与 `Ctrl+P` 共用默认档；`Ctrl+Shift+O` 开跳转档）。
    ///
    /// `mode` 只在**打开边沿**落给浮层；关闭由共享开关统一收口（浮层 Esc / 点遮罩 / 执行完）。
    fn toggle_quick_open(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let open = !self.shared.quick_open.get();
        self.shared.quick_open.set(open);
        tracing::info!(open, mode = ?mode, "【探针】toggle_quick_open：Quick Open 浮层开关");
        // 互斥：Quick Open 与设置页不同时显示（两个 overlay 会互相压住）。
        self.shared.settings_open.set(false);
        if open {
            self.quick_open_open_pending = true;
            self.quick_open_mode_pending = mode;
        }
        cx.notify();
    }

    /// 懒创建浮层（首帧渲染；输入框与结果列表由浮层自己按需创建）。
    fn ensure_quick_open_palette(&mut self, cx: &mut Context<Self>) -> Entity<QuickOpenPalette> {
        if let Some(palette) = self.quick_open_palette.clone() {
            return palette;
        }
        let host: Rc<dyn QuickOpenHost> = Rc::new(WorkbenchQuickOpenHost {
            view: cx.entity().downgrade(),
        });
        let shared = self.shared.clone();
        let palette = cx.new(|cx| QuickOpenPalette::new(shared, host, cx));
        self.quick_open_palette = Some(palette.clone());
        palette
    }

    /// 挂载浮层（未打开时返回 `None`，overlay 不进元素树）。
    fn render_quick_open(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shared.quick_open.get() {
            return None;
        }
        let palette = self.ensure_quick_open_palette(cx);
        if self.quick_open_open_pending {
            self.quick_open_open_pending = false;
            palette.update(cx, |palette, cx| {
                palette.on_opened(self.quick_open_mode_pending, cx)
            });
        }
        Some(palette.into_any_element())
    }

    /// 草稿箱文件属性弹窗（右键 → 属性；用户点名 2026-10-10：信息少，走弹窗不走 panel）。
    ///
    /// 读 `Shared::scratchpad_file_info`（草稿箱经端口写入）；宿主用 gpui-kit `Dialog`
    /// 包一层（内置 overlay / Esc / 遮罩点击关闭），内容只有 6 行键值（`render_scratchpad_file_info_pane`）。
    fn render_scratchpad_file_info_popover(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let view = (*self.shared.scratchpad_file_info.borrow()).clone()?;
        let theme = cx.theme().clone();
        let shared = self.shared.clone();
        let close = move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
            *shared.scratchpad_file_info.borrow_mut() = None;
            shared.notify_host(app);
        };
        Some(
            Dialog::new(cx)
                .title(div().child(format!("属性 · {}", view.name)))
                .content({
                    let theme = theme.clone();
                    move |content: DialogContent, window: &mut Window, cx: &mut App| {
                        let _ = (window, cx);
                        content.child(render_scratchpad_file_info_pane(&view, &theme))
                    }
                })
                .button_props(DialogButtonProps::default().show_cancel(false))
                .on_close(close)
                .into_any_element(),
        )
    }

    /// 执行一条结果（浮层经宿主端口回传；动作语义只在这里实现）。
    pub(crate) fn execute_quick_open_action(
        &mut self,
        action: Action,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(action = ?action, keep_open, "【探针】execute_quick_open_action：命令面板执行一条动作");
        match action {
            Action::NewDocument(mode) => self.new_editor_document(mode, window, cx),
            Action::ShowProperties(request) => {
                // 与导航搜索结果同一去向：中央 Dock 属性 tab（请求已带全定位信息）。
                self.shared.show_properties(*request, window, cx);
            }
            Action::RevealMetadata(object) => {
                // 与「打开左面板」同一口径（切到数据源 + 展开），再把请求交给导航面板：
                // 展开链路与选中都在那边做，工作台不重复一份“树长什么样”。
                self.shared.active_left.set(LeftPanel::Database);
                self.shared.left_mode.set(SidebarMode::Expanded);
                self.shared.request_reveal(object);
            }
            Action::OpenLeftPanel(panel) => {
                // 与右 dock `Shared::open_right_panel` 同一口径：先登记进 `left_open_panels`
                //（tab 条只显示点过的模块；漏登记 → dock 已存在时新模块永不挂载）。
                {
                    let mut open = self.shared.left_open_panels.borrow_mut();
                    if !open.contains(&panel) {
                        open.push(panel);
                    }
                }
                self.shared.active_left.set(panel);
                self.shared.left_mode.set(SidebarMode::Expanded);
                if panel == LeftPanel::Resources {
                    // 与活动栏点击同一口径：激活即取一次列表（M6）。
                    self.request_resources_refresh(cx);
                }
                // 状态改完由宿主这拍同步 dock：挂载 / 激活 active 面板（apply_left_mode
                // 的已存在分支负责补齐未挂载 tab 并切到激活项）。
                self.apply_left_mode(window, cx);
            }
            Action::OpenRightPanel(panel) => {
                let open_before = self
                    .shared
                    .right_open_panels
                    .borrow()
                    .iter()
                    .map(|p| format!("{p:?}"))
                    .collect::<Vec<_>>()
                    .join(",");
                // 与右活动栏点击同一口径（Mock 的候选清单与生成历史在展开这一拍重读）。
                self.shared.open_right_panel(panel, cx);
                // 与 `OpenLeftPanel` 对称：状态改完由宿主这拍同步 dock（挂载 / 激活 / 展开）。
                // 旧实现只委托 `open_right_panel`，布局同步依赖 `notify_host` 的 defer 重绘——
                // 弱引用升级失败或事件时序错位时右 dock 会停在收起态，表现为「命令点了进不去面板」。
                self.apply_right_mode(window, cx);
                let open_after = self
                    .shared
                    .right_open_panels
                    .borrow()
                    .iter()
                    .map(|p| format!("{p:?}"))
                    .collect::<Vec<_>>()
                    .join(",");
                tracing::info!(
                    target = "rds_workbench::quick_open",
                    panel = format!("{panel:?}"),
                    right_mode = format!("{:?}", self.shared.right_mode.get()),
                    open_before = open_before,
                    open_after = open_after,
                    "命令面板：打开右侧面板"
                );
            }
            Action::OpenSettings => {
                self.shared.settings_open.set(true);
            }
            Action::HideSidebars => {
                let s = &self.shared;
                if s.left_mode.get() != SidebarMode::Hidden {
                    s.left_mode_before_hidden.set(s.left_mode.get());
                    s.left_mode.set(SidebarMode::Hidden);
                }
                if s.right_mode.get() != SidebarMode::Hidden {
                    s.right_mode_before_hidden.set(s.right_mode.get());
                    s.right_mode.set(SidebarMode::Hidden);
                }
            }
            Action::RestoreSidebars => {
                let s = &self.shared;
                if s.left_mode.get() == SidebarMode::Hidden {
                    s.left_mode
                        .set(restore_snapshot(s.left_mode_before_hidden.get()));
                }
                if s.right_mode.get() == SidebarMode::Hidden {
                    s.right_mode
                        .set(restore_snapshot(s.right_mode_before_hidden.get()));
                }
            }
            Action::SelectConnection(ix) => {
                // 与侧边栏点击一致：清导航缓存，防止串数据；不自动连接。
                self.shared.selected.set(Some(ix));
                self.shared.invalidate_nav_cache();
            }
            Action::OpenDocument(path) => {
                // 与草稿箱面板双击同一条路（先存请求，宿主 render 消费时落到编辑器）：
                // 可写 / 只读由编辑器按路径自己判定，这里不重复一份“哪些路径只读”的口径。
                self.shared.request_open_in_editor(path);
            }
        }
        if !keep_open {
            self.shared.quick_open.set(false);
        }
        cx.notify();
    }

    // ===== LSP 管理器（状态栏入口 + 空闲回收） =====

    /// 【LSP】空闲回收 + 状态栏指示刷新（1 s 一拍；句柄必须被持有，drop 即停）
    ///
    /// 两件事共用一个节拍：① `reap_if_idle`——**关掉最后一个需要服务的面板 → 停 server**
    /// （只停服务、不设拦住原因，下次需要会自动重启）；② 端口状态版本变了就 `notify`，
    /// 让状态栏指示（图标 / 文案）跟上（启动 / 停止 / 就绪 / 崩溃）。
    fn spawn_lsp_reaper(&self, cx: &mut Context<Self>) -> Option<Task<()>> {
        self.lsp.as_ref()?;
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        Some(cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(1000)).await;
                let keep = weak
                    .update(cx, |view, cx| {
                        // 复用自持端口句柄（与 `view.lsp` 同一个 Rc；先 clone 开，
                        // 避开 `view` 的借用与 `cx.notify` 的写冲突）
                        let Some(lsp) = view.lsp.clone() else {
                            return false;
                        };
                        lsp.reap_if_idle();
                        let epoch = lsp.epoch();
                        if epoch != view.last_lsp_epoch {
                            view.last_lsp_epoch = epoch;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        }))
    }

    /// 【LSP】立即把状态版本对齐到端口最新值（手动启停后立刻重绘，不等 1 s 泵）
    fn sync_lsp_epoch(&mut self) {
        if let Some(lsp) = self.lsp.as_ref() {
            self.last_lsp_epoch = lsp.epoch();
        }
    }

    /// 【LSP】状态栏「语言服务」指示 + 管理器浮层（点击弹：启动 / 停止）
    ///
    /// 未接端口（测试 / 未启用）→ 空元素（不摆这一项）。触发元素必须是 `Button`
    /// （`Popover::trigger` 要求 `Selectable`，元素侧唯一实现者是 `Button`）；用 `.xsmall()`
    /// + 自定义变体对齐状态栏本色（`primary` / `primary_foreground`），避开把 h_6 的按钮
    /// 挤进 `text_xs` 的状态栏里把整条撑高。
    fn lsp_status_item(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(lsp) = self.lsp.as_ref() else {
            return div().into_any_element();
        };
        let (icon, label) = lsp_status_chip(
            lsp.is_running(),
            lsp.is_initialized(),
            lsp.block().as_ref(),
        );
        let (fg, hover) = {
            let colors = &cx.theme().colors;
            (colors.primary_foreground, colors.primary_active)
        };
        let entity = cx.entity();
        Popover::new("status-lsp-popover")
            // 状态栏在最下：向左上展开，避开被窗口下缘截断
            .anchor(Anchor::BottomRight)
            .offset(ui::STATUS_LSP_OFFSET)
            .trigger(
                Button::new("status-lsp")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(cx.theme().transparent)
                            .foreground(fg)
                            .hover(hover),
                    )
                    .xsmall()
                    .icon(icon)
                    .label(label)
                    .tooltip("语言服务（LSP）"),
            )
            .content(move |_state, _window, cx| render_lsp_manager(&entity, cx))
            .into_any_element()
    }

    // ===== 状态栏 =====

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // 【LSP】语言服务指示 + 管理器浮层（未接端口 = 空元素）——
        // 必须在 `cx.theme()` 之前算：那一步把 cx 借出去了（gpui-kit-dev 的借用约定）。
        let lsp_item = self.lsp_status_item(cx);
        // M7：Mock 任务在跑时给状态栏一个指示 + 取消入口（读面板实体自己的状态，不另存一份）——
        // 必须在 `cx.theme()` 之前算：那一步把 cx 借出去了（gpui-kit-dev 的借用约定）。
        let mock_chip = self
            .shared
            .mock_panel
            .borrow()
            .clone()
            .and_then(|weak| weak.upgrade())
            .map(|panel| mock_status_chip(panel, cx));
        let theme = cx.theme();
        let entity = cx.entity();
        let shared = self.shared.clone();
        let selected = self
            .shared
            .selected_connection()
            .map(|c| c.name)
            .unwrap_or_else(|| "未选择连接".to_string());
        let active_label = self.shared.active_left.get().label();
        let left_hidden = self.shared.left_mode.get() == SidebarMode::Hidden;
        let right_hidden = self.shared.right_mode.get() == SidebarMode::Hidden;

        // 左侧独立开关：完全隐藏 / 恢复（恢复时还原隐藏前模式）。
        // 自绘（对齐设计稿 .sb-btn）：显式前景色 + hover 高亮，不依赖 Button 变体样式。
        let left_shared = shared.clone();
        let left_entity = entity.clone();
        let left_toggle = sb_toggle(
            "toggle-left-sidebar",
            IconName::PanelLeftClose,
            if left_hidden {
                "» 恢复"
            } else {
                "« 完全隐藏"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| toggle_sidebar_hidden(&left_shared, &left_entity, true, app),
        );

        // 右侧独立开关（镜像）。
        let right_shared = shared.clone();
        let right_entity = entity.clone();
        let right_toggle = sb_toggle(
            "toggle-right-sidebar",
            IconName::PanelRightClose,
            if right_hidden {
                "« 恢复"
            } else {
                "完全隐藏 »"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| toggle_sidebar_hidden(&right_shared, &right_entity, false, app),
        );

        // 状态栏背景品牌珊瑚色（theme.colors.primary 派生，见 theme-design §5.4），文字取配对前景色。
        StatusBar::new()
            .bg(theme.colors.primary)
            .text_color(theme.colors.primary_foreground)
            .left(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(left_toggle)
                    .child(div().child(active_label)),
            )
            .right(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    // 状态栏去杂乱（需求）：文档级上下文已迁到编辑器自身状态栏，
                    // 全局栏只留应用级信息——Mock 任务 / 选中连接 / LSP / 右侧开关，
                    // 去掉静态的「DuckDB 就绪」「UTF-8」。
                    .children(mock_chip)
                    .child(div().child(format!("连接：{selected}")))
                    .child(lsp_item)
                    .child(right_toggle),
            )
    }
}

/// 状态栏的 Mock 任务指示（有任务在跑时才给）：`◐ Mock 生成中 40%`（出口类任务给阶段名）
/// 与一个可取消任务尾部的「取消」。
///
/// 为何放在状态栏、而不与 D38 的「状态单点」相冲：D38 管的是**读数放在哪**（进度详情仍在
/// 它自己的中央表头 / 右 Dock 清单下），而这里解决另一个问题——面板被切走、中央 tab 被关掉时
/// **任务还在跑却看不见也取消不了**。所以它只给「在跑 + 取消」，不重复进度条。
///
/// 状态取自面板实体（单一权威）而不是往 `Shared` 再存一份：读别的面板实体在宿主 render 里
/// 已有先例（右栏「存档详情」读资产库面板的选中项）。
fn mock_status_chip(panel: Entity<MockPanel>, cx: &mut App) -> Div {
    // 颜色先取出来：`cx.theme()` 借了 cx，而下面读面板 / 挂回调还要用 cx
    let (fg, hover_bg) = {
        let theme = cx.theme();
        (theme.colors.primary_foreground, theme.colors.primary_active)
    };
    let (text, cancellable, cancel_requested) = {
        let view = panel.read(cx);
        // 文案的口径归 mock crate（`status_chip_text`：没任务 / 拿不到进度就不画）
        let Some(text) = status_chip_text(view.job_progress()) else {
            return div();
        };
        (
            text,
            view.job_kind().is_some_and(|kind| kind.generates()),
            view.cancel_requested(),
        )
    };

    let mut chip = div()
        .h_flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded_sm()
        .text_color(fg)
        .child(div().child(text));
    if cancellable {
        // 出口类任务不可取消（DuckDB / 文件系统内没有中断点），所以那一档只给文字
        let mut cancel = div()
            .id("status-mock-cancel")
            .role(gpui::Role::Button)
            .px_1()
            .rounded_sm()
            .text_color(fg)
            .child(if cancel_requested {
                "正在取消…"
            } else {
                "取消"
            });
        if !cancel_requested {
            cancel = cancel
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_click(move |_, _, app| {
                    panel.update(app, |panel, cx| panel.cancel_job(cx));
                });
        }
        chip = chip.child(cancel);
    }
    chip
}

/// 状态栏开关按钮（自绘，对齐设计稿 .sb-btn）：图标 + 文字 + hover 高亮。
fn sb_toggle(
    id: &'static str,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    fg: Hsla,
    hover_bg: Hsla,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h_flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .text_color(fg)
        .hover(move |s| s.bg(hover_bg))
        .child(Icon::new(icon).size_3p5())
        .child(div().child(label.into()))
        .on_click(move |_, _, app| on_click(app))
}

/// 状态栏 LSP 指示的（图标，文案）（**纯函数，可单测**）
///
/// 状态优先级：**就绪 > 启动中 > 拦住（停止 / 启动失败 / 断开）> 未启动**。
/// 图标用完整 Lucide 目录（组件子集里没有 `server-*` 系列）。
pub fn lsp_status_chip(
    running: bool,
    initialized: bool,
    block: Option<&LspBlock>,
) -> (CatalogIcon, String) {
    if running && initialized {
        (CatalogIcon::Server, "语言服务：就绪".to_string())
    } else if running {
        (CatalogIcon::ServerCog, "语言服务：启动中".to_string())
    } else {
        match block {
            Some(LspBlock::Manual) => (CatalogIcon::ServerOff, "语言服务：已停止".to_string()),
            Some(LspBlock::StartFailed(reason)) => {
                (CatalogIcon::ServerOff, format!("语言服务：未启动（{reason}）"))
            }
            Some(LspBlock::Crashed(reason)) => {
                (CatalogIcon::ServerCrash, format!("语言服务：已断开（{reason}）"))
            }
            None => (CatalogIcon::ServerOff, "语言服务：未启动".to_string()),
        }
    }
}

/// LSP 管理器浮层内容：状态 + 在用面板数 + 拦住原因 + 启动 / 停止
///
/// 从 `entity` **现读**端口状态（浮层每次重绘都重读，启动 / 停止后立刻反映）；`cx` 是
/// `Context<PopoverState>`，只当 `App` 用（与编辑器工具栏定制面板同一约定）。
fn render_lsp_manager(entity: &Entity<WorkbenchView>, cx: &mut App) -> AnyElement {
    let Some(lsp) = entity.read(cx).lsp.clone() else {
        return div().into_any_element();
    };
    let running = lsp.is_running();
    let (icon, label) = lsp_status_chip(running, lsp.is_initialized(), lsp.block().as_ref());
    let count = lsp.active_document_count();
    let reason = match lsp.block() {
        Some(LspBlock::StartFailed(reason)) | Some(LspBlock::Crashed(reason)) => Some(reason),
        _ => None,
    };
    let muted = cx.theme().colors.muted_foreground;

    let mut body = div()
        .v_flex()
        .gap_2()
        .p_3()
        .w(rems(ui::STATUS_LSP_MANAGER_WIDTH));
    body = body.child(
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(Icon::new(icon).size_3p5())
            .child(div().text_sm().child("语言服务（LSP）")),
    );
    body = body.child(div().text_sm().child(label));
    body = body.child(
        div()
            .text_xs()
            .text_color(muted)
            .child(format!("在用面板：{count}")),
    );
    if let Some(reason) = reason {
        body = body.child(div().text_xs().text_color(muted).child(reason));
    }
    // 面板为空时提醒：服务会在空闲回收时自动停（下次需要再启）
    if count == 0 && running {
        body = body.child(
            div()
                .text_xs()
                .text_color(muted)
                .child("无面板使用，稍后会自动停止"),
        );
    }

    let start_entity = entity.clone();
    let stop_entity = entity.clone();
    body = body.child(
        div()
            .h_flex()
            .gap_2()
            .child(
                Button::new("lsp-manager-start")
                    .small()
                    .label("启动")
                    .disabled(running)
                    .on_click(move |_, _, app| {
                        if let Some(lsp) = start_entity.read(app).lsp.clone() {
                            lsp.start();
                        }
                        start_entity.update(app, |view, cx| {
                            view.sync_lsp_epoch();
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("lsp-manager-stop")
                    .small()
                    .label("停止")
                    .disabled(!running)
                    .on_click(move |_, _, app| {
                        if let Some(lsp) = stop_entity.read(app).lsp.clone() {
                            lsp.stop();
                        }
                        stop_entity.update(app, |view, cx| {
                            view.sync_lsp_epoch();
                            cx.notify();
                        });
                    }),
            ),
    );
    body.into_any_element()
}

/// 完全隐藏切换的纯状态转移：返回 `(新模式, 新快照)`。
///
/// - 非隐藏态 → 隐藏：记录当前模式到快照；
/// - 隐藏态 → 恢复：按快照还原（快照异常为 Hidden 时兜底展开）。
///
/// 纯函数，供状态栏开关与契约测试共用。
pub fn toggle_hidden_mode(mode: SidebarMode, snapshot: SidebarMode) -> (SidebarMode, SidebarMode) {
    if mode == SidebarMode::Hidden {
        (restore_snapshot(snapshot), snapshot)
    } else {
        (SidebarMode::Hidden, mode)
    }
}

/// 切换单侧「完全隐藏」：隐藏前记录当前模式快照，恢复时按快照还原，
/// 保证「收起」等状态在完全隐藏 / 恢复往返后不丢失。
/// Quick Open 的宿主端口：把浮层的动作落回工作台。
///
/// 只有工作台知道这些副作用怎么做（新建文档要 `Window`、资产库要顺带刷新、
/// 连接选中要清导航缓存）；浮层因此不依赖 `WorkbenchView`，可在窗口测试里用哑宿主驱动。
struct WorkbenchQuickOpenHost {
    /// 宿主体（浮层与工作台同生命周期；升级失败 = 视图已销毁，动作静默丢弃）。
    view: WeakEntity<WorkbenchView>,
}

impl QuickOpenHost for WorkbenchQuickOpenHost {
    fn execute(&self, action: Action, keep_open: bool, window: &mut Window, cx: &mut App) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        view.update(cx, |view, cx| {
            view.execute_quick_open_action(action, keep_open, window, cx)
        });
    }
}

fn toggle_sidebar_hidden(
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    is_left: bool,
    app: &mut App,
) {
    let (mode, snapshot) = if is_left {
        (&shared.left_mode, &shared.left_mode_before_hidden)
    } else {
        (&shared.right_mode, &shared.right_mode_before_hidden)
    };
    let (next_mode, next_snapshot) = toggle_hidden_mode(mode.get(), snapshot.get());
    mode.set(next_mode);
    snapshot.set(next_snapshot);
    entity.update(app, |_, cx| cx.notify());
}

/// 还原快照模式；快照异常为 Hidden 时兜底为展开（防御性）。
fn restore_snapshot(mode: SidebarMode) -> SidebarMode {
    if mode == SidebarMode::Hidden {
        SidebarMode::Expanded
    } else {
        mode
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {
            self.init_workspace(window, cx);
        }
        // 搜索结果标签页（「搜索: query」，VSCode 全局搜索形态）：草稿箱搜索完成
        // （`Shared::scratchpad_search` 有值）且面板未建 → `cx.defer_in` 建面板
        // （事件路径可改 dock；render 路径本身只读）。面板已建（含死弱引用升级成功）
        // 则不重复入队——新搜索结果只更新全局状态，面板每帧读到最新内容。
        // 用户 tab ✕ 关闭过标签页（`search_results_dismissed`）则不自动重建——
        // 关标签保留结果，直到再次搜索（`show_scratchpad_search(Some)` 清标志）。
        let search_matches = self
            .shared
            .search_results_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .map(|panel| {
                let s = self.shared.scratchpad_search.borrow();
                panel.read(cx).query() == s.as_ref().map(|v| v.query.clone())
            })
            .unwrap_or(false);
        if self.shared.scratchpad_search.borrow().is_some()
            && !*self.shared.search_results_dismissed.borrow()
            && !search_matches
        {
            cx.defer_in(window, |this, window, cx| {
                this.ensure_search_results_panel(window, cx);
            });
        }
        // 元数据搜索标签页（P0，VSCode「搜索: query」形态，搜连接级元数据缓存）：
        // 面板自持泵把结果写进 `Shared::metadata_search`（有值）且面板未建 →
        // `cx.defer_in` 建面板（事件路径可改 dock）。用户 tab ✕ 关闭过则不自动重建——
        // 关标签保留结果，直到再次搜索（`publish_metadata_search(Some)` 清标志）。
        let md_search_matches = self
            .shared
            .metadata_search_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .map(|panel| {
                let s = self.shared.metadata_search.borrow();
                panel.read(cx).query() == s.as_ref().map(|v| v.query.clone())
            })
            .unwrap_or(false);
        if self.shared.metadata_search.borrow().is_some()
            && !*self.shared.metadata_search_dismissed.borrow()
            && !md_search_matches
        {
            cx.defer_in(window, |this, window, cx| {
                this.ensure_metadata_search_panel(window, cx);
            });
        }
        // 洞察中间工作页（D63）：入口（导航树 / 结果列表 / SQL 编辑器）登记了待打开
        // 目标 → `cx.defer_in` 建 / 激活中央洞察 tab（事件路径可改 dock；render 纯读）。
        // 面板已建且正在渲染同一目标时，入口重复触发只聚焦不重发（ensure 消费后为空）。
        if self.shared.pending_insight_target.borrow().is_some() {
            cx.defer_in(window, |this, window, cx| {
                this.ensure_insight_workspace_panel(window, cx);
            });
        }
        // T18：右栏集合「对比」请求 → 中间页切到对比形态（勾选该表 + 激活中央 tab）。
        if self.shared.pending_insight_compare.borrow().is_some() {
            cx.defer_in(window, |this, window, cx| {
                this.consume_insight_compare(window, cx);
            });
        }
        // M1：项目输入实体懒创建（选择器搜索 / 新建 / 删除确认）。
        if self.project_inputs.is_none() {
            self.project_inputs = Some(project::ui::ProjectInputs::new(window, cx));
        }
        // 三模式权威同步点：Shared 状态 → Dock。
        self.apply_left_mode(window, cx);
        self.apply_right_mode(window, cx);
        // 【2026-10-27】结果区从全局底部 dock 拆回编辑器宿主内嵌（DBeaver 口径：
        // 结果集跟编辑器 tab 走，不再挂全局 dock）。sync_result_dock 不再需要——
        // 结果区有无内容由编辑器宿主自己 render 判断（v_resizable 分栏，无内容时不占位）。
        // self.sync_result_dock(window, cx);
        // 真面板化：版本 / 修复 / 回收站 / 标签对话框与导航「定位」请求原在
        // `SidebarPanel::render` 消费（外壳已不再渲染），提升到宿主 render——
        // render 仍是权威同步点，副作用不放到事件路径；动作仍经 `sidebar` 实体转发。
        if let Some(sidebar) = self.sidebar.clone() {
            sidebar.update(cx, |panel, cx| {
                panel.ensure_version_dialog(window, cx);
                panel.ensure_repair_dialog(window, cx);
                panel.ensure_trash_dialog(window, cx);
                panel.ensure_tag_dialog(window, cx);
                panel.consume_reveal(cx);
            });
        }
        // 连接对话框项目下拉的动作项 → 复用项目管理的入口：
        // 「＋ 新增项目」开新建对话框；「打开现有目录…」开目录选择对话框。
        // 两者都会切换项目；有未保存草稿时先走确认，**确认后直接推进到目标对话框**（#4）。
        // 决策与“只消费一次”收在 `Shared::take_project_action_request`（可单测，#9）。
        if let Some(request) = self.shared.take_project_action_request() {
            let host = self.project_host().clone();
            if let Some(inputs) = self.project_inputs.clone() {
                match request {
                    ProjectActionRequest::CreateProject => {
                        project::ui::request_create_project(&host, &inputs, window, cx);
                    }
                    ProjectActionRequest::OpenFolder => {
                        project::ui::request_open_folder(&host, &inputs, window, cx);
                    }
                }
            }
        }
        // M5 草稿箱 / M6 资产库：双击 / Enter / 右键「打开」→ 中央编辑器
        //（同路径已打开只激活，不重读；M6 的本体带编辑器只读）。
        // 消费点在宿主 render（文档与 Dock 面板属宿主状态）；失败走通知栏。
        if let Some(request) = self.shared.take_open_in_editor() {
            if let Err(e) = self.open_in_editor_with(
                request.path,
                request.read_only,
                request.reveal,
                window,
                cx,
            ) {
                *self.shared.notice.borrow_mut() = Some(format!("打开文件失败: {e}"));
            }
        }
        // B11：导航的「打开查询」请求（在 SQL 编辑器中打开 / 查看数据 / 生成 SQL / 拖拽）。
        // 入队方拿不到 `Window`，消费点在宿主 render（与上一块同一口径）。
        if let Some(request) = self.shared.take_query_request() {
            self.open_query_document(request, window, cx);
        }

        let area = self.area.clone().expect("workspace initialized");
        // edition 2024：`.then(|| ...)` 闭包会同时独占 `cx`/`self`，改为显式 if（也更符合
        // 编码指南「分支代表不同 interface 时用普通控制流」）。
        let left_bar = if self.shared.left_mode.get() != SidebarMode::Hidden {
            Some(self.render_left_activity_bar(cx))
        } else {
            None
        };
        let right_bar = if self.shared.right_mode.get() != SidebarMode::Hidden {
            Some(self.render_right_activity_bar(cx))
        } else {
            None
        };
        let quick_open = self.render_quick_open(cx);
        let settings_panel = self.render_settings_panel(window, cx);
        let scratchpad_info = self.render_scratchpad_file_info_popover(window, cx);

        // M1：项目选择器已是中央 tab 组常驻面板（`ProjectPickerPanel::visible` 随
        // 项目开关），DockArea 不再分「有项目 / 无项目」两套挂法——无项目时左右
        // Dock 照常渲染，Mock 面板「纯生成不受影响」（mock-architecture I8）。
        let mut middle = div().h_flex().items_stretch().flex_1().min_h_0();
        if let Some(bar) = left_bar {
            middle = middle.child(bar);
        }
        middle = middle.child(area);
        if let Some(bar) = right_bar {
            middle = middle.child(bar);
        }

        let mut root = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .relative()
            .key_context("workbench")
            .on_action({
                let entity = cx.entity();
                move |_: &ToggleQuickOpen, _window, cx| {
                    entity.update(cx, |this, cx| this.toggle_quick_open(Mode::Default, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &ToggleQuickGoto, _window, cx| {
                    entity.update(cx, |this, cx| this.toggle_quick_open(Mode::Goto, cx));
                }
            })
            // M4：Ctrl+F 聚焦数据源导航搜索（先切到数据源面板并展开左侧 Dock）。
            .on_action({
                let sidebar = self.sidebar.clone();
                let entity = cx.entity();
                move |_: &FocusNavSearch, window, cx| {
                    if let Some(sidebar) = &sidebar {
                        sidebar.update(cx, |panel, cx| panel.focus_nav_search(window, cx));
                    }
                    entity.update(cx, |this, cx| {
                        this.shared.active_left.set(LeftPanel::Database);
                        if this.shared.left_mode.get() == SidebarMode::Hidden {
                            this.shared.left_mode.set(SidebarMode::Expanded);
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &HideSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let s = &this.shared;
                        if s.left_mode.get() != SidebarMode::Hidden {
                            s.left_mode_before_hidden.set(s.left_mode.get());
                            s.left_mode.set(SidebarMode::Hidden);
                        }
                        if s.right_mode.get() != SidebarMode::Hidden {
                            s.right_mode_before_hidden.set(s.right_mode.get());
                            s.right_mode.set(SidebarMode::Hidden);
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &RestoreSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let s = &this.shared;
                        if s.left_mode.get() == SidebarMode::Hidden {
                            s.left_mode
                                .set(restore_snapshot(s.left_mode_before_hidden.get()));
                        }
                        if s.right_mode.get() == SidebarMode::Hidden {
                            s.right_mode
                                .set(restore_snapshot(s.right_mode_before_hidden.get()));
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &OpenSettings, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let open = this.shared.settings_open.get();
                        tracing::info!(open = !open, "【探针】OpenSettings：打开/关闭设置页（Ctrl+,）");
                        this.shared.settings_open.set(!open);
                        // 互斥：设置页与 Quick Open 不同时显示
                        this.shared.quick_open.set(false);
                        cx.notify();
                    });
                }
            })
            // 设置页内按 `Esc` 关闭（键位绑在 `key_context("settings")` 上）。
            .on_action({
                let entity = cx.entity();
                move |_: &CloseSettings, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.shared.settings_open.set(false);
                        cx.notify();
                    });
                }
            })
            // 主题切换（`Ctrl+Shift+L`，绑在 workbench context）：显式模式两态切换；
            // 跟随系统时按系统当前外观反切到显式对侧。处理在 SettingsService（设置层）。
            .on_action({
                move |_: &ToggleThemeMode, window, cx| {
                    SettingsService::toggle_theme_mode(Some(window), cx);
                }
            })
            // M1：切换项目（= 关闭当前 + 回选择器）。
            .on_action({
                let entity = cx.entity();
                move |_: &SwitchProject, window, cx| {
                    entity.update(cx, |this, cx| {
                        tracing::info!("【探针】SwitchProject：切换项目动作触发（关闭当前回选择器）");
                        let host = this.project_host().clone();
                        project::ui::request_close(&host, window, cx);
                    });
                }
            })
            // M1：关闭项目。
            .on_action({
                let entity = cx.entity();
                move |_: &CloseProject, window, cx| {
                    entity.update(cx, |this, cx| {
                        tracing::info!("【探针】CloseProject：关闭项目动作触发");
                        let host = this.project_host().clone();
                        project::ui::request_close(&host, window, cx);
                    });
                }
            })
            // A10：关闭当前编辑器文档（键位绑在编辑器面板的 `editor` context 上，
            // 但在这里执行：面板不在自己的 `update` 里，Dock 移除它时才不会重入）。
            .on_action({
                let entity = cx.entity();
                move |_: &editor::commands::CloseDocument, window, cx| {
                    entity.update(cx, |this, cx| this.close_active_editor(window, cx));
                }
            })
            // A9：另存为 / 打开文件——同样在这里执行（系统文件对话框是宿主依赖，
            // 且另存为要落到“当前是哪份文档”上）。
            .on_action({
                let entity = cx.entity();
                move |_: &editor::commands::SaveDocumentAs, window, cx| {
                    entity.update(cx, |this, cx| this.save_active_editor_as(window, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &editor::commands::OpenDocument, window, cx| {
                    entity.update(cx, |this, cx| this.open_file_via_dialog(window, cx));
                }
            })
            .child(self.render_title_bar(window, cx))
            .child(middle)
            .child(self.render_status_bar(cx));

        if let Some(qo) = quick_open {
            root = root.child(qo);
        }
        if let Some(sp) = settings_panel {
            root = root.child(sp);
        }
        if let Some(info) = scratchpad_info {
            root = root.child(info);
        }
        // M1：项目设置浮层（菜单 / 对话框已改由 Popover 与语义 Dialog 承载，不在此渲染）。
        if self.shared.project.borrow().is_some() {
            let host = self.project_host().clone();
            let inputs = self
                .project_inputs
                .clone()
                .expect("project inputs initialized");
            if let Some(settings) = project::ui::render_settings(&host, &inputs, cx) {
                root = root.child(settings);
            }
        }
        // 对话框层：gpui-kit 0.7.1 的 Root 已自动挂载 dialog 层（WindowState::dialog_layer），
        // 连接对话框与项目对话框不再需要手动渲染。
        root
    }
}
