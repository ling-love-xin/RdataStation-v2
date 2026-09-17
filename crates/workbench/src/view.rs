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
use crate::panels::{
    EditorPanel, ProjectActionRequest, QueryRequest, RightSidebarPanel, Shared, SidebarPanel,
};
use crate::ui;
use mock::mock_view::{MockDetailView, focus_detail_tab};
use settings::commands::{CloseSettings, OpenSettings};
use settings::settings_page::{SettingsHost, SettingsPage};

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
    /// Quick Open 输入状态（render 首次懒创建）。
    quick_open_input: Option<Entity<InputState>>,
    /// 设置页实体（首次打开时懒创建）。
    settings_page: Option<Entity<SettingsPage>>,
    /// M1 项目管理输入实体（懒创建）。
    project_inputs: Option<project::ui::ProjectInputs>,
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
    /// 草稿执行回执 → 元数据回写泵（1 s 一拍；句柄必须被持有，drop 即停）。
    scratchpad_meta_pump: Option<Task<()>>,
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
        // 启动恢复的项目也要**取写锁**：`project_ui.lock/read_only` 原本只在交互式打开时写，
        // 而启动路径直接装会话——结果是「另一实例占着同一个项目」时两边都以为自己可写，
        // 各模块的只读护栅（mock 的四个出口 / 资源库 / 草稿箱 / 编辑器替换）全部不生效。
        // 处置与交互式打开一致：被占用 → 只读打开 + 提示。
        if let Some(root) = project_root.clone() {
            let opened = match project::service::open(&root) {
                Ok(project::service::OpenOutcome::Opened(opened)) => Ok(opened),
                Ok(project::service::OpenOutcome::Busy(_)) => {
                    project::service::open_read_only(&root)
                }
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
        // A12：把会话存储接上（光标 / 选区 / 模式跨重启保留；未接时不持久化但编辑可用）
        crate::services::editor_session::attach(&editor_service);
        // A9：把“另存为”的路径选择接上（系统文件对话框；未接时另存为会明确报未接入）
        crate::services::editor_files::attach(&editor_service);
        // B1：把连接端口接上（连接列表 + 自动建连；未接时选择器说“未接入连接列表”）
        crate::services::editor_connections::attach(&editor_service, &editor_shared_for_conn);
        // B13：把执行通道端口接上（源库 / 本地加速 / 联邦 的门控；未接时后两档都不可选并给原因）
        crate::services::editor_channels::attach(&editor_service, &editor_shared_for_conn);
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
            settings_page: None,
            project_inputs: None,
            project_host: Some(host),
            _editor_subscription: None,
            _archive_subscription: None,
            _insight_rules_watcher: insight_rules_watcher,
            editor_service,
            editor_hosts: Vec::new(),
            scratchpad_meta_pump: None,
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
        self.open_in_editor_with(path, editor::model::ReadOnly::none(), window, cx)
    }

    /// 同上，但带上**只读维度**（M6 的存档本体：编辑器只读，改它要先去取回）。
    ///
    /// 只读由发起方判定（编辑器不认识 `resources/`），两条路只在“建文档时置不置只读”上分叉：
    /// 已打开的文档仍旧只激活（不悄悄把用户手上的文档锁掉）。
    pub(crate) fn open_in_editor_with(
        &mut self,
        path: std::path::PathBuf,
        read_only: editor::model::ReadOnly,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
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
        let task = cx.spawn(async move |_this, cx| loop {
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
        });
        self.scratchpad_meta_pump = Some(task);
    }

    /// 首次 render 时装配 DockArea：创建面板实体、订阅事件；左右 dock 由
    /// `apply_left_mode` / `apply_right_mode` 按 `Shared` 初始状态装配。
    fn init_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        let sidebar = cx.new(|cx| SidebarPanel::new(shared.clone(), &self.editor_service, cx));
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let right_sidebar = cx.new(|cx| RightSidebarPanel::new(shared.clone(), window, cx));
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

        // 宿主重绘桥再挂一层：除宿主自身，编辑区也要跟上。
        //
        // 导航选中连接会递增 `Shared::nav_cache_epoch`（编辑区下一帧据此丢掉分析库
        // 导航树缓存）；原先由 `SidebarEvent::SelectConnection` 的订阅回调顺带
        // `editor.notify()` 驱动，事件通道退役后收到这里：只唤醒“脏了但自己不知道”
        // 的编辑区，不再依赖它恰好因别的原因重渲染。
        {
            let weak = cx.entity().downgrade();
            let editor_for_redraw = editor.clone();
            *shared.host_redraw.borrow_mut() = Some(Rc::new(move |cx: &mut App| {
                editor_for_redraw.update(cx, |_, cx| cx.notify());
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |_, cx| cx.notify());
                }
            }));
        }

        // 导航面板的事件通道（`SidebarEvent`）已退役：选中 / 编辑 / 新建 / 开右栏
        // 都改走宿主端口（`database::nav_host::NavHost`，实现在 `components/nav_host.rs`），
        // 动作在事件路径上直接完成，不再绕宿主 render 转发一轮。

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
                    area_for_detail.update(cx, |area, cx| {
                        area.add_panel(detail.clone(), DockPlacement::Center, None, window, cx);
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
            let right_for_archive = right_sidebar.clone();
            self._archive_subscription = Some(cx.observe(&panel, move |_, _, cx| {
                if shared_for_archive.active_right.get() == RightPanel::Archive {
                    right_for_archive.update(cx, |_, cx| cx.notify());
                }
            }));
        }
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
                // 互斥：打开 Quick Open 时收起设置页
                qo_shared.settings_open.set(false);
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
                                    // M6：切到资产库时取一次列表（结果由面板轮询回填）。
                                    if activating_resources {
                                        this.request_resources_refresh(cx);
                                    }
                                    cx.notify();
                                });
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
            let host = SettingsHost {
                on_close,
                on_open_cache,
                on_open_log_dir,
                on_open_log_view,
            };
            self.settings_page = Some(cx.new(|cx| SettingsPage::new(window, host, cx)));
        }
        let theme = cx.theme().clone();
        let page = self.settings_page.clone().expect("settings page initialized");
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

        // 分析库元数据树由编辑区自持（见 `panels-modules.md` §5 P0）：
        // Quick Open 只取一份快照，不再直读别家的缓存字段。
        let nav_tables = self
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).nav_table_names())
            .unwrap_or_default();
        let list = quick_open_results(&input, shared, entity, &nav_tables, cx);

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
        // M5 草稿箱 / M6 资产库：双击 / Enter / 右键「打开」→ 中央编辑器
        //（同路径已打开只激活，不重读；M6 的本体带编辑器只读）。
        // 消费点在宿主 render（文档与 Dock 面板属宿主状态）；失败走通知栏。
        if let Some(request) = self.shared.take_open_in_editor() {
            if let Err(e) = self.open_in_editor_with(request.path, request.read_only, window, cx) {
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
                        // 互斥：Quick Open 与设置页不同时显示（两个 overlay 会互相压住）
                        this.shared.settings_open.set(false);
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
    nav_tables: &[String],
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
        let tables: Vec<String> = nav_tables.to_vec();
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
            // M6：与活动栏点击同一口径——激活即取一次列表。
            entity.update(cx, |this, cx| this.request_resources_refresh(cx));
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

/// Quick Open 选中连接：与侧边栏点击一致（清空导航缓存，防止串数据）。
fn select_connection(
    idx: usize,
    _name: &str,
    _driver: &str,
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    shared.selected.set(Some(idx));
    shared.invalidate_nav_cache();
    shared.invalidate_nav_cache();
    shared.quick_open.set(false);
    entity.update(cx, |_, cx| cx.notify());
}
