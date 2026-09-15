//! RdataStation v2 工作台视图（WorkbenchView）— v5 布局
//!
//! 按 GPUI-kit 编码规范搭建 DockArea 可拖拽布局工作台：
//! - 依赖只向下：视图只使用 gpui-kit（gpui / base / component），不承载业务逻辑；
//! - 窗口第一层由 App Shell 的 `Root` 装配；本视图负责标题栏 + 左右活动栏 + DockArea + 状态栏；
//! - 左 Dock：草稿箱 / 数据库导航 / 资源分析 / 插件；右 Dock：洞察 / Mock 生成 / 历史；
//! - 边栏三模式：展开 / 收起（`toggle_dock`）/ 完全隐藏（`remove_dock`，gpui-kit 0.6）；
//!   模式状态存于 `Shared`，`render` 统一同步到 Dock（权威同步点）；
//! - Quick Open：搜索 + 命令融合（标题栏居中入口 / Ctrl+P），受控自绘弹层；
//! - 颜色一律取自 `cx.theme().colors`（禁止 raw hex/rgb）。

use std::path::Path;
use std::rc::Rc;

use gpui_kit::base::{Selectable, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{DockArea, DockLayout, DockPlacement, DockSkin, panel_handle};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Root, TitleBar};
use gpui_kit::*;

use crate::commands::{
    CloseProject, FocusNavSearch, HideSidebars, RestoreSidebars, SwitchProject, ToggleQuickOpen,
};
use crate::panels::{EditorPanel, ProjectActionRequest, RightSidebarPanel, Shared, SidebarEvent, SidebarPanel};
use crate::ui;
use mock::mock_view::{MockDetailView, focus_detail_tab};
use settings::commands::OpenSettings;
use settings::settings_view::SettingsView;

/// 左侧活动栏面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftPanel {
    /// 草稿箱（M5 草稿能力占位）
    Draft,
    /// 数据库导航（M4，数据源连接 + 对象树）
    Database,
    /// 资源分析（M6）
    Resources,
    /// 插件（M9）
    Plugin,
}

/// 右侧活动栏面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    /// 洞察（M8）
    Insight,
    /// Mock 数据生成（M7）
    Mock,
    /// 历史（查询历史）
    History,
}

/// 边栏三模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    /// 展开：活动栏 + 边栏可见
    Expanded,
    /// 收起：边栏隐藏、活动栏保留（dock 保留、离屏）
    Collapsed,
    /// 完全隐藏：活动栏 + 边栏均隐藏（dock 移除）
    Hidden,
}

impl LeftPanel {
    pub const ALL: [LeftPanel; 4] = [
        LeftPanel::Draft,
        LeftPanel::Database,
        LeftPanel::Resources,
        LeftPanel::Plugin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftPanel::Draft => "草稿箱",
            LeftPanel::Database => "数据库导航",
            LeftPanel::Resources => "资产库",
            LeftPanel::Plugin => "插件",
        }
    }

    /// 活动栏图标（Lucide）：默认图标集（`IconName`）不含这些语义图标，
    /// 应用已注册 `AllAssets`（全量目录），故按资产路径直接引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            LeftPanel::Draft => "icons/notebook-text.svg",
            LeftPanel::Database => "icons/database.svg",
            LeftPanel::Resources => "icons/chart-column.svg",
            LeftPanel::Plugin => "icons/puzzle.svg",
        };
        Icon::default().path(path)
    }
}

impl RightPanel {
    pub const ALL: [RightPanel; 3] = [RightPanel::Insight, RightPanel::Mock, RightPanel::History];

    pub fn label(self) -> &'static str {
        match self {
            RightPanel::Insight => "洞察",
            RightPanel::Mock => "Mock 生成",
            RightPanel::History => "历史",
        }
    }

    /// 活动栏图标（Lucide），与左侧同样按资产路径引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            RightPanel::Insight => "icons/lightbulb.svg",
            RightPanel::Mock => "icons/dice-5.svg",
            RightPanel::History => "icons/clock.svg",
        };
        Icon::default().path(path)
    }
}

/// 连接条目（Round 21 起由全局系统库真实数据填充；Round 22 扩展完整元数据）。
#[derive(Debug, Clone)]
pub struct ConnectionItem {
    pub id: String,
    pub name: String,
    pub driver: String,
    /// 运行时是否已连接（连接管理器维护，由加载器填充；记录有效性由持久化层过滤）。
    pub connected: bool,
    /// 真实元数据（Round 22）：主机 / 端口 / 数据库 / Schema。
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub description: Option<String>,
    /// DuckDB 联邦（本地加速）开关。
    pub use_duckdb_fed: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 工作台视图：标题栏 + 左右活动栏 + DockArea（左/右/中央）+ 状态栏。
pub struct WorkbenchView {
    shared: Shared,
    /// DockArea 实体（render 首次调用时懒初始化，需要 &mut Window）。
    area: Option<Entity<DockArea>>,
    sidebar: Option<Entity<SidebarPanel>>,
    editor: Option<Entity<EditorPanel>>,
    right_sidebar: Option<Entity<RightSidebarPanel>>,
    /// Quick Open 输入状态（render 首次懒创建）。
    quick_open_input: Option<Entity<InputState>>,
    /// 设置面板实体（首次打开时懒创建）。
    settings_view: Option<Entity<SettingsView>>,
    /// M1 项目管理输入实体（懒创建）。
    project_inputs: Option<project::ui::ProjectInputs>,
    /// M1 项目视图宿主（构造期组装；项目视图位于 `project` crate）。
    project_host: Option<project::ui::ProjectUiHost>,
    /// 订阅句柄（保持连接选中事件的订阅存活）。
    _subscription: Option<Subscription>,
    /// 编辑面板观察句柄（其 notify 级联到宿主，保证对话框层内容同步）。
    _editor_subscription: Option<Subscription>,
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
        // B1：编辑器的连接端口要用它（连接列表快照 + 项目根）——先 clone 出来，
        // 因为下面构造 `editor_service` 时不能再借 `self`。
        let editor_shared_for_conn = shared.clone();
        // M1：排序偏好（直读 settings.json，无需 cx）与首屏项目列表（无项目时）都在构造期完成，
        // 避免在 `render` 里做 I/O（GPUI-kit 编码指南：副作用不得放在 render）。
        {
            let sort =
                project::ui::ProjectSort::from_key(&settings::load_settings().projects.sort_mode);
            shared.project_ui.borrow_mut().picker.sort = sort;
        }
        // 项目视图宿主：注入状态句柄 / 重绘 / 编辑区桥 / 排序偏好 / 打开后刷新。
        let host = crate::components::project_host::build_host(&shared, cx.entity().downgrade());
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
        if shared.project.borrow().is_none() {
            project::ui::load_picker(&host);
        }
        // M8：启动规则目录监听。放在构造期（而非 render）——沿用 GPUI-kit 编码指南
        // 「副作用不得放在 render」，也保证窗口首帧前监听已就位。
        // 目录由「当前项目根」现算，故先告知监听器当前项目（切换时在 refresh_after_open 再告知）。
        insight::set_watched_project_root(project_root.clone());
        let insight_rules_watcher = Some(insight::RulesWatcher::spawn());
        Self {
            shared,
            area: None,
            sidebar: None,
            editor: None,
            right_sidebar: None,
            quick_open_input: None,
            settings_view: None,
            project_inputs: None,
            project_host: Some(host),
            _subscription: None,
            _editor_subscription: None,
            _insight_rules_watcher: insight_rules_watcher,
            editor_service: {
                // 初始一份未命名 SQL 文档：打开 app 就有可写的编辑区，
                // 而不是空白（后续可由 A12 的会话恢复替换为上次的文档）。
                let service = editor::shared::EditorShared::new();
                service.open(editor::service::OpenRequest::untitled(
                    "",
                    editor::model::EditorMode::Sql,
                ));
                // A14：把执行端口接上（当前活动连接）。未接时执行动作会明确报“未接入执行”。
                crate::services::editor_exec::attach(&service);
                // A12：把会话存储接上（光标 / 选区 / 模式跨重启保留；未接时不持久化但编辑可用）
                crate::services::editor_session::attach(&service);
                // A9：把“另存为”的路径选择接上（系统文件对话框；未接时另存为会明确报未接入）
                crate::services::editor_files::attach(&service);
                // B1：把连接端口接上（连接列表 + 自动建连；未接时选择器说“未接入连接列表”）
                crate::services::editor_connections::attach(&service, &editor_shared_for_conn);
                service
            },
            editor_hosts: Vec::new(),
        }
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
        let mode = editor::mode::resolve_mode(&path, None);
        let outcome = editor::persist::open_file(&self.editor_service, &path, mode)
            .map_err(|error| error.to_string())?;
        self.show_document(outcome.id().clone(), window, cx);
        Ok(())
    }

    /// 把一份已存在于 `EditorService` 的文档接到界面上（建面板 / 复用已有面板）
    pub fn show_document(
        &mut self,
        document: editor::model::DocumentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 已关闭的面板不参与复用（面板被 Dock 移除 = 文档已关，两者一一对应）
        self.editor_hosts.retain(|panel| !panel.read(cx).is_closed());

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
        let panel = cx.new(|cx| {
            editor::view::host::EditorHostPanel::new(service, document, window, cx)
        });
        self.editor_hosts.push(panel.clone());

        if let Some(area) = self.area.clone() {
            area.update(cx, |area, cx| {
                area.add_panel(panel, DockPlacement::Center, None, window, cx);
            });
        }
    }

    /// 恢复上次的编辑器会话（A12）
    ///
    /// 内容与模式取自会话（**不是从磁盘重读**：用户上次可能没保存），光标/选区交回面板。
    /// 恢复成功后把启动时那份空的未命名文档收掉，免得每启动一次多一个空标签。
    ///
    /// 调用时机：`init_workspace` 的 `cx.defer_in`（读库是 I/O，不在构造与渲染里做）。
    pub fn restore_last_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(Some(session)) = self.editor_service.load_latest_session() else {
            return;
        };
        let Some(path) = session.path.clone() else {
            return;
        };

        // 启动时那份空的未命名文档（在 `new` 里开的）——恢复成功后收掉
        let blank = self
            .editor_service
            .service()
            .documents()
            .iter()
            .find(|doc| doc.path().is_none() && doc.content().trim().is_empty())
            .map(|doc| doc.id().clone());

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
        if let Some(blank) = blank
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

    /// 关闭当前编辑器文档（`Ctrl+W`）
    ///
    /// 键位绑在编辑器面板的 `editor` context 上（A10），但**由宿主执行**：
    /// 面板移除时会回调它自己的 `on_removed`，而 `DockArea` 移除面板要读面板本体，
    /// 从面板自己的 `update` 里发起就是重入。宿主不在那个 `update` 中，可以安全地做。
    /// 脏文档由 [`Self::close_editor_document`] 弹三态确认（保存 / 不保存 / 取消）。
    pub fn close_active_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_hosts.retain(|panel| !panel.read(cx).is_closed());
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
        self.editor_hosts.retain(|panel| !panel.read(cx).is_closed());
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

    /// 首次 render 时装配 DockArea：创建面板实体、订阅事件；左右 dock 由
    /// `apply_left_mode` / `apply_right_mode` 按 `Shared` 初始状态装配。
    fn init_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        let sidebar = cx.new(|cx| SidebarPanel::new(shared.clone(), cx));
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let right_sidebar = cx.new(|cx| RightSidebarPanel::new(shared.clone(), cx));
        // M1：宿主命令——清空编辑区（项目保存 / 放弃未保存草稿后调用）。
        {
            let editor_for_clear = editor.clone();
            *shared.editor_clear.borrow_mut() =
                Some(Rc::new(move |window: &mut Window, cx: &mut App| {
                    editor_for_clear.update(cx, |panel, cx| panel.clear_sql(window, cx));
                }));
        }

        // 订阅侧边栏事件：连接选中 -> 更新共享状态并重绘编辑器。
        let subscription = cx.subscribe(&sidebar, |this, _entity, event: &SidebarEvent, cx| {
            match event {
                SidebarEvent::SelectConnection(idx) => {
                    this.shared.selected.set(Some(*idx));
                    // Round 30：切换连接 → 清空导航树 / SQL 结果残留，防止串数据。
                    *this.shared.nav_for.borrow_mut() = None;
                    this.shared.nav_tables.borrow_mut().clear();
                    *this.shared.sql_for.borrow_mut() = None;
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
                SidebarEvent::EditConnection(_) => {
                    // 编辑请求已写入 shared.open_edit；通知编辑器渲染消费并打开对话框。
                    // 注意：此处处于宿主自身的 update 上下文，不能回调 `notify_host`
                    // （会重入借用宿主）；末尾的 `cx.notify()` 已足够让宿主重绘。
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
                SidebarEvent::NewConnectionRequest => {
                    // 请求已写入 shared.new_connection_request；通知编辑区渲染消费。
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
                SidebarEvent::EditorSqlRequest => {
                    // SQL 已写入 shared.editor_set；通知编辑区渲染消费。
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
                SidebarEvent::OpenSqlEditor(conn_id) => {
                    // 连接右键「在 SQL 编辑器中打开」：选中该连接（与侧边栏点击一致，
                    // 清空导航树 / SQL 结果残留），并通知编辑区重绘。
                    let idx = this
                        .shared
                        .connections
                        .borrow()
                        .iter()
                        .position(|c| c.id == *conn_id);
                    this.shared.selected.set(idx);
                    *this.shared.nav_for.borrow_mut() = None;
                    this.shared.nav_tables.borrow_mut().clear();
                    *this.shared.sql_for.borrow_mut() = None;
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }
                SidebarEvent::OpenRightPanel(panel) => {
                    // 连接右键「生成 Mock 数据 / 查看洞察」：展开右 Dock 并切面板
                    // （与 Quick Open 的 OpenInsight / OpenMock 同一处理口径）。
                    this.shared.active_right.set(*panel);
                    this.shared.right_mode.set(SidebarMode::Expanded);
                }
            }
            cx.notify();
        });

        let (area, _skin) = DockSkin::dock_area("workspace", Some(1), window, cx);

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
        let editor_host = cx.new(|cx| {
            editor::view::host::EditorHostPanel::new(host_service, document, window, cx)
        });
        self.editor_hosts.push(editor_host.clone());

        let editor_handle = panel_handle(editor.clone());
        let host_handle = panel_handle(editor_host);
        area.update(cx, |area, cx| {
            // 中央编辑区单独占满（左右 dock 独立装配，不用 h_split）。
            // A9：标签条交给 Dock——同一个 tab 组里两个面板 = 两个标签。
            area.set_center(
                DockLayout::tabs()
                    .panel_view(editor_handle, cx)
                    .panel_view(host_handle, cx),
                window,
                cx,
            );
        });

        // M7：宿主命令——打开 Mock 详情 tab（面板「查看详情」调用）。
        // 详情与配置面板同属 mock crate：面板实体在 `RightSidebarPanel` 构造期已登记弱句柄，
        // 这里只负责把它接入中央 tab 组（首次加入，已存在则聚焦自身 tab）。
        {
            let shared_for_detail = shared.clone();
            let area_for_detail = area.clone();
            *shared.open_mock_detail.borrow_mut() =
                Some(Rc::new(move |window: &mut Window, cx: &mut App| {
                    let Some(panel) = shared_for_detail
                        .mock_panel
                        .borrow()
                        .clone()
                        .and_then(|weak| weak.upgrade())
                    else {
                        return;
                    };
                    let alive = shared_for_detail
                        .mock_detail
                        .borrow()
                        .clone()
                        .and_then(|weak| weak.upgrade());
                    if let Some(detail) = alive {
                        // 已在 Dock 中（tab 被切走也只是失焦）：聚焦即可，不重复加入。
                        // 必须在实体更新之外调用：闭包里调会因 TabGroup 回读本实体而 double lease panic。
                        focus_detail_tab(&detail, window, cx);
                        return;
                    }
                    let detail = cx.new(|cx| MockDetailView::new(panel.clone(), cx));
                    *shared_for_detail.mock_detail.borrow_mut() = Some(detail.downgrade());
                    area_for_detail.update(cx, |area, cx| {
                        area.add_panel(detail.clone(), DockPlacement::Center, None, window, cx);
                    });
                    focus_detail_tab(&detail, window, cx);
                }));
        }

        self._subscription = Some(subscription);
        // 编辑面板通知级联到宿主：对话框层挂在宿主 render 中（`Root` 的 notify
        // 不会传到子视图），而对话框内部的状态变化（切 Tab / 增删跳 / 测试结果等）
        // 都以 EditorPanel 的 notify 驱动，需同步宿主重绘才能更新层内容。
        self._editor_subscription = Some(cx.observe(&editor, |_, _, cx| cx.notify()));
        self.area = Some(area);
        self.sidebar = Some(sidebar);
        self.editor = Some(editor);
        self.right_sidebar = Some(right_sidebar);

        // A12：恢复上次会话（读库是 I/O → 放到 `defer_in`，不在构造/渲染里做）
        cx.defer_in(window, |this, window, cx| {
            this.restore_last_session(window, cx);
        });
    }

    // ===== 三模式：Shared 状态 → Dock 同步（render 权威） =====

    fn apply_left_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.left_mode.get();
        let (Some(area), Some(sidebar)) = (&self.area, &self.sidebar) else {
            return;
        };
        area.update(cx, |area, cx| match mode {
            SidebarMode::Expanded => {
                if !area.has_dock(DockPlacement::Left) {
                    let handle = panel_handle(sidebar.clone());
                    area.set_dock(
                        DockPlacement::Left,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
                    // 起步宽度 240px（= 15rem 基准，随界面缩放；layout-design §2.3，用户拖拽可调）。
                    area.set_dock_size(
                        DockPlacement::Left,
                        cx.theme().font_size * ui::LEFT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                } else if !area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                }
            }
            SidebarMode::Collapsed => {
                if !area.has_dock(DockPlacement::Left) {
                    let handle = panel_handle(sidebar.clone());
                    area.set_dock(
                        DockPlacement::Left,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
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
            }
            SidebarMode::Hidden => {
                if area.has_dock(DockPlacement::Left) {
                    area.remove_dock(DockPlacement::Left, window, cx);
                }
            }
        });
    }

    fn apply_right_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.right_mode.get();
        let (Some(area), Some(right)) = (&self.area, &self.right_sidebar) else {
            return;
        };
        area.update(cx, |area, cx| match mode {
            SidebarMode::Expanded => {
                if !area.has_dock(DockPlacement::Right) {
                    let handle = panel_handle(right.clone());
                    area.set_dock(
                        DockPlacement::Right,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
                    // 起步宽度 280px（= 17.5rem 基准）。
                    area.set_dock_size(
                        DockPlacement::Right,
                        cx.theme().font_size * ui::RIGHT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                } else if !area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
            }
            SidebarMode::Collapsed => {
                if !area.has_dock(DockPlacement::Right) {
                    let handle = panel_handle(right.clone());
                    area.set_dock(
                        DockPlacement::Right,
                        DockLayout::tabs().panel_view(handle, cx),
                        window,
                        cx,
                    );
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
            }
            SidebarMode::Hidden => {
                if area.has_dock(DockPlacement::Right) {
                    area.remove_dock(DockPlacement::Right, window, cx);
                }
            }
        });
    }

    // ===== 标题栏 =====
    // 差异点：无窗口标题文本。承载 gpui-kit 官方 TitleBar（对应 layout-design.md §2.1）：
    // - 窗口拖拽（Drag hitbox，Windows 系统级）、双击最大化、窗口控制按钮（─ □ ✕）由组件自带；
    // - 高度覆盖为 36px、背景覆盖为主题 title_bar 纯色（对齐 layout-proposal.html v5）。
    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let pt = settings::product_tokens::get(cx);
        let shared = self.shared.clone();
        let entity = cx.entity();

        // 软件图标（明亮版；暗黑版未设计，dark 主题先复用，见 theme-design.md）。
        let icon_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/32x32.png");
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
        // 触发元素必须是语义控件（`Popover::trigger` 要求 `Selectable`），用 ghost Button
        // 承载自定义外观，而非 clickable div。
        let slot = Button::new("project-slot")
            .ghost()
            .h(rems(1.625))
            .px_3p5()
            .rounded_md()
            .bg(pt.title_bar_slot_background(theme))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
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

        // M1：有项目时用 `Popover` 承载项目菜单（焦点 / 键盘 / 点击外部关闭 / Escape 由组件负责，
        // 不再自绘弹层）。项目视图由 `project` crate 提供，宿主只提供状态与重绘。
        let slot: AnyElement = if has_project {
            let host = self.project_host().clone();
            let menu_open = host.state.borrow().menu_open;
            let menu_inputs = self
                .project_inputs
                .clone()
                .expect("project inputs lazy init");
            let menu_host = host.clone();
            let open_host = host;
            Popover::new("project-menu")
                .open(menu_open)
                .on_open_change(move |open, _window, app| {
                    open_host.state.borrow_mut().menu_open = *open;
                    open_host.notify(app);
                })
                .trigger(slot)
                .content(move |_state, _window, cx| {
                    project::ui::render_menu_content(&menu_host, &menu_inputs, cx)
                })
                .into_any_element()
        } else {
            slot.into_any_element()
        };

        // Quick Open 入口（点击唤起，Ctrl+P 见 commands.rs 绑定）。
        // 320×26 居中；底色取 border 角色（dark #3C3C3C，与示意 v5 一致）。
        let qo_shared = shared.clone();
        let qo_entity = entity.clone();
        let quick_open = div()
            .id("quick-open-entry")
            .h_flex()
            .items_center()
            .gap_2()
            .w_80()
            .h(rems(1.625))
            .px_2p5()
            .rounded_sm()
            .bg(theme.colors.border)
            .cursor_pointer()
            .child(Icon::new(IconName::Search).size_3p5())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("搜索或输入命令…"),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("Ctrl+P"),
            )
            .on_click(move |_, _, app| {
                qo_shared.quick_open.set(true);
                qo_entity.update(app, |_, cx| cx.notify());
            });

        // 三栏布局：左右 flex_1 占位对称，Quick Open 严格居中；
        // 右侧窗口控制按钮（─ □ ✕）由 TitleBar 自带渲染，无需自绘。
        TitleBar::new()
            .h_9()
            .pl_2p5()
            .bg(theme.colors.title_bar)
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(logo)
                    .child(slot),
            )
            .child(quick_open)
            .child(div().flex_1())
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
                            .on_click(move |_, _, app| {
                                let mode = shared.left_mode.get();
                                if mode == SidebarMode::Expanded
                                    && shared.active_left.get() == panel
                                {
                                    // 再次点击当前激活项 → 收起。
                                    shared.left_mode.set(SidebarMode::Collapsed);
                                } else {
                                    shared.active_left.set(panel);
                                    shared.left_mode.set(SidebarMode::Expanded);
                                }
                                entity.update(app, |_, cx| cx.notify());
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
                            .on_click(move |_, _, app| {
                                let mode = shared.right_mode.get();
                                if mode == SidebarMode::Expanded
                                    && shared.active_right.get() == panel
                                {
                                    shared.right_mode.set(SidebarMode::Collapsed);
                                } else {
                                    shared.active_right.set(panel);
                                    shared.right_mode.set(SidebarMode::Expanded);
                                }
                                entity.update(app, |_, cx| cx.notify());
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
                        entity.update(app, |_, cx| cx.notify());
                    }),
            );
        bar
    }

    /// 设置面板 overlay：居中渲染 SettingsView（懒创建实体）。
    /// 关闭按钮由 SettingsView 内部回调 Shared.settings_open。
    fn render_settings_panel(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.shared.settings_open.get() {
            return None;
        }
        if self.settings_view.is_none() {
            let entity = cx.entity();
            let shared = self.shared.clone();
            let on_close: std::rc::Rc<dyn Fn(&mut App)> = std::rc::Rc::new(move |app| {
                shared.settings_open.set(false);
                entity.update(app, |_, cx| cx.notify());
            });
            let on_close = on_close.clone();
            let shared_cache = self.shared.clone();
            let on_open_cache: std::rc::Rc<dyn Fn(&mut Window, &mut App)> =
                std::rc::Rc::new(move |window, app| {
                    crate::components::cache_dialog::open_cache_dialog(window, app, &shared_cache);
                });
            self.settings_view =
                Some(cx.new(move |cx| SettingsView::new(cx, on_close, on_open_cache)));
        }
        let theme = cx.theme().clone();
        let view = self.settings_view.clone().expect("settings initialized");
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.colors.overlay)
                .child(view),
        )
    }

    // ===== Quick Open（搜索 + 命令融合） =====

    fn render_quick_open(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.shared.quick_open.get() {
            return None;
        }
        let theme = cx.theme().clone();
        let input = self.quick_open_input.clone().expect("lazy init");
        let shared = self.shared.clone();
        let entity = cx.entity();
        let shared_close = shared.clone();
        let entity_close = entity.clone();

        let list = quick_open_results(&input, shared, entity, cx);

        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .bg(theme.colors.overlay)
                .child(
                    div()
                        // 示意 v5：面板水平居中、顶部距标题栏 42px。
                        .mt(rems(2.625))
                        .w(rems(35.))
                        .max_h(rems(26.25))
                        .v_flex()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.colors.border)
                        .bg(theme.colors.popover)
                        .shadow_lg()
                        .child(Input::new(&input))
                        .child(list.overflow_y_scrollbar()),
                )
                .on_mouse_down(MouseButton::Left, move |_, _, app| {
                    shared_close.quick_open.set(false);
                    entity_close.update(app, |_, cx| cx.notify());
                }),
        )
    }

    // ===== 状态栏 =====

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(div().child(format!("连接：{selected}")))
                    .child(div().child("DuckDB 就绪"))
                    .child(div().child("UTF-8"))
                    .child(right_toggle),
            )
    }
}

/// 状态栏开关按钮（自绘，对齐设计稿 .sb-btn）：图标 + 文字 + hover 高亮。
fn sb_toggle(
    id: &'static str,
    icon: IconName,
    label: &'static str,
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
        .child(div().child(label))
        .on_click(move |_, _, app| on_click(app))
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

impl EventEmitter<SidebarEvent> for WorkbenchView {}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {
            self.init_workspace(window, cx);
        }
        if self.quick_open_input.is_none() {
            self.quick_open_input = Some(cx.new(|cx| InputState::new(window, cx)));
        }
        // M1：项目输入实体懒创建（选择器搜索 / 新建 / 删除确认）。
        if self.project_inputs.is_none() {
            self.project_inputs = Some(project::ui::ProjectInputs::new(window, cx));
        }
        // 三模式权威同步点：Shared 状态 → Dock。
        self.apply_left_mode(window, cx);
        self.apply_right_mode(window, cx);
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
        // M5 草稿箱：双击 / Enter / 右键「打开」→ 中央编辑器（同路径已打开只激活，不重读）。
        // 消费点在宿主 render（文档与 Dock 面板属宿主状态）；失败走通知栏。
        if let Some(path) = self.shared.take_open_file_request() {
            if let Err(e) = self.open_in_editor(path, window, cx) {
                *self.shared.notice.borrow_mut() = Some(format!("打开文件失败: {e}"));
            }
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
        let settings_panel = self.render_settings_panel(cx);

        // M1：无项目时以选择器覆盖中央区（保留五段外壳）。
        let inputs = self
            .project_inputs
            .clone()
            .expect("project inputs lazy init");
        let no_project = { self.shared.project.borrow().is_none() };
        let mut middle = div().h_flex().items_stretch().flex_1().min_h_0();
        if let Some(bar) = left_bar {
            middle = middle.child(bar);
        }
        if no_project {
            let host = self.project_host().clone();
            let picker = project::ui::render_picker(&host, &inputs, cx);
            middle = middle.child(picker);
        } else {
            middle = middle.child(area);
        }
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
                    entity.update(cx, |this, cx| {
                        let open = this.shared.quick_open.get();
                        this.shared.quick_open.set(!open);
                        cx.notify();
                    });
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
                        this.shared.settings_open.set(!open);
                        cx.notify();
                    });
                }
            })
            // M1：切换项目（= 关闭当前 + 回选择器）。
            .on_action({
                let entity = cx.entity();
                move |_: &SwitchProject, window, cx| {
                    entity.update(cx, |this, cx| {
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
        // M1：项目设置（菜单 / 对话框已改由 Popover 与语义 Dialog 承载，不在此渲染）。
        if !no_project {
            let host = self.project_host().clone();
            if let Some(settings) = project::ui::render_settings(&host, &inputs, cx) {
                root = root.child(settings);
            }
        }
        // 对话框层（Root::render_dialog_layer）——连接对话框与项目对话框均在此渲染。
        if let Some(dialog_layer) = Root::render_dialog_layer(window, cx) {
            root = root.child(dialog_layer);
        }
        root
    }
}

/// Quick Open 结果列表：命令组 + 资源组（连接 / 分析表），按输入过滤。
fn quick_open_results(
    input: &Entity<InputState>,
    shared: Shared,
    entity: Entity<WorkbenchView>,
    cx: &mut App,
) -> Div {
    let theme = cx.theme();
    let pt = settings::product_tokens::get(cx);
    let query = input.read(cx).value().to_string();
    let commands_only = query.trim_start().starts_with('>');
    let needle = if commands_only {
        query.trim_start_matches('>').trim().to_lowercase()
    } else {
        query.trim().to_lowercase()
    };
    let matches = |s: &str| needle.is_empty() || s.to_lowercase().contains(&needle);

    let mut list = div().v_flex().gap_1().mt_0p5().max_h(rems(21.25));

    // ---- 命令组 ----
    let mut cmd_group = div().v_flex().gap_1();
    cmd_group = cmd_group.child(
        div()
            .px_1()
            .py_0p5()
            .rounded_sm()
            .bg(pt.quick_open_group_header(theme))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.colors.muted_foreground)
            .child("命令"),
    );
    let commands: &[(&str, QuickOpenCommand)] = &[
        // 新建入口放最前：Quick Open 是工作台的命令面（Ctrl+P），“新建”是最常敲的一条
        ("新建查询", QuickOpenCommand::NewQuery),
        ("新建笔记", QuickOpenCommand::NewNote),
        ("新建文件", QuickOpenCommand::NewFile),
        ("打开草稿箱", QuickOpenCommand::OpenDraft),
        ("打开数据库导航", QuickOpenCommand::OpenDatabase),
        ("打开资产库", QuickOpenCommand::OpenResources),
        ("打开插件", QuickOpenCommand::OpenPlugin),
        ("打开洞察", QuickOpenCommand::OpenInsight),
        ("打开 Mock 生成", QuickOpenCommand::OpenMock),
        ("打开历史", QuickOpenCommand::OpenHistory),
        ("打开设置", QuickOpenCommand::OpenSettings),
        ("完全隐藏侧边栏", QuickOpenCommand::HideSidebars),
        ("恢复侧边栏", QuickOpenCommand::RestoreSidebars),
    ];
    let mut has_cmd = false;
    for (label, cmd) in commands {
        if !matches(label) {
            continue;
        }
        has_cmd = true;
        let shared = shared.clone();
        let entity = entity.clone();
        let cmd = *cmd;
        cmd_group = cmd_group.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "qo-cmd-{label}"
                ))))
                .h_7()
                .pl_2p5()
                .pr_2p5()
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.foreground)
                .on_mouse_down(MouseButton::Left, move |_, window, app| {
                    run_quick_command(cmd, &shared, &entity, window, app);
                })
                .child(*label),
        );
    }
    if has_cmd {
        list = list.child(cmd_group);
    }

    // ---- 资源组：连接 + 分析表（仅非 > 前缀时展示） ----
    if !commands_only {
        let mut res_group = div().v_flex().gap_1();
        res_group = res_group.child(
            div()
                .px_1()
                .py_0p5()
                .rounded_sm()
                .bg(pt.quick_open_group_header(theme))
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.colors.muted_foreground)
                .child("文件 / 表"),
        );
        let conns: Vec<(String, String)> = shared
            .connections
            .borrow()
            .iter()
            .map(|c| (c.name.clone(), c.driver.clone()))
            .collect();
        let mut has_res = false;
        for (idx, (name, driver)) in conns.iter().enumerate() {
            if !matches(name) {
                continue;
            }
            has_res = true;
            let shared = shared.clone();
            let entity = entity.clone();
            let name = name.clone();
            let driver = driver.clone();
            res_group = res_group.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "qo-conn-{idx}"
                    ))))
                    .h_7()
                    .pl_2p5()
                    .pr_2p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .on_mouse_down(MouseButton::Left, {
                        let name = name.clone();
                        let driver = driver.clone();
                        let shared = shared.clone();
                        let entity = entity.clone();
                        move |_, _, app| {
                            select_connection(idx, &name, &driver, &shared, &entity, app);
                        }
                    })
                    .child(format!("{name}  ·  {driver}")),
            );
        }
        let tables: Vec<String> = shared
            .nav_tables
            .borrow()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        for t in tables {
            if !matches(&t) {
                continue;
            }
            has_res = true;
            res_group = res_group.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!("qo-table-{t}"))))
                    .h_7()
                    .pl_2p5()
                    .pr_2p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("表：{t}")),
            );
        }
        if has_res {
            list = list.child(res_group);
        }
        if !has_res && !has_cmd {
            list = list.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("无匹配结果"),
            );
        }
    } else if !has_cmd {
        list = list.child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("无匹配命令"),
        );
    }
    list
}

/// Quick Open 命令集合。
#[derive(Debug, Clone, Copy)]
enum QuickOpenCommand {
    NewQuery,
    NewNote,
    NewFile,
    OpenDraft,
    OpenDatabase,
    OpenResources,
    OpenPlugin,
    OpenInsight,
    OpenMock,
    OpenHistory,
    OpenSettings,
    HideSidebars,
    RestoreSidebars,
}

/// 执行 Quick Open 命令：更新 Shared 状态 + 关闭弹层 + notify（Dock 由 render 同步）。
fn run_quick_command(
    cmd: QuickOpenCommand,
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    window: &mut Window,
    cx: &mut App,
) {
    match cmd {
        // 新建文档要 `window`（建面板），因此在命令这里直接落到宿主方法上
        QuickOpenCommand::NewQuery => {
            let mode = editor::model::EditorMode::Sql;
            entity.update(cx, |this, cx| this.new_editor_document(mode, window, cx));
        }
        QuickOpenCommand::NewNote => {
            let mode = editor::model::EditorMode::Analysis;
            entity.update(cx, |this, cx| this.new_editor_document(mode, window, cx));
        }
        QuickOpenCommand::NewFile => {
            let mode = editor::model::EditorMode::Text;
            entity.update(cx, |this, cx| this.new_editor_document(mode, window, cx));
        }
        QuickOpenCommand::OpenDraft => {
            shared.active_left.set(LeftPanel::Draft);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenDatabase => {
            shared.active_left.set(LeftPanel::Database);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenResources => {
            shared.active_left.set(LeftPanel::Resources);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenPlugin => {
            shared.active_left.set(LeftPanel::Plugin);
            shared.left_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenInsight => {
            shared.active_right.set(RightPanel::Insight);
            shared.right_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenMock => {
            shared.active_right.set(RightPanel::Mock);
            shared.right_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenHistory => {
            shared.active_right.set(RightPanel::History);
            shared.right_mode.set(SidebarMode::Expanded);
        }
        QuickOpenCommand::OpenSettings => {
            shared.settings_open.set(true);
        }
        QuickOpenCommand::HideSidebars => {
            if shared.left_mode.get() != SidebarMode::Hidden {
                shared.left_mode_before_hidden.set(shared.left_mode.get());
                shared.left_mode.set(SidebarMode::Hidden);
            }
            if shared.right_mode.get() != SidebarMode::Hidden {
                shared.right_mode_before_hidden.set(shared.right_mode.get());
                shared.right_mode.set(SidebarMode::Hidden);
            }
        }
        QuickOpenCommand::RestoreSidebars => {
            if shared.left_mode.get() == SidebarMode::Hidden {
                shared
                    .left_mode
                    .set(restore_snapshot(shared.left_mode_before_hidden.get()));
            }
            if shared.right_mode.get() == SidebarMode::Hidden {
                shared
                    .right_mode
                    .set(restore_snapshot(shared.right_mode_before_hidden.get()));
            }
        }
    }
    shared.quick_open.set(false);
    entity.update(cx, |_, cx| cx.notify());
}

/// Quick Open 选中连接：与侧边栏点击一致（清空导航 / SQL 残留，防止串数据）。
fn select_connection(
    idx: usize,
    _name: &str,
    _driver: &str,
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    shared.selected.set(Some(idx));
    *shared.nav_for.borrow_mut() = None;
    shared.nav_tables.borrow_mut().clear();
    *shared.sql_for.borrow_mut() = None;
    shared.quick_open.set(false);
    entity.update(cx, |_, cx| cx.notify());
}
