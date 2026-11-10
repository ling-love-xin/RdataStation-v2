//! 工作台视图构造、编辑器文档与项目宿主装配。
//!
//! 自 `view.rs` 拆分（行为不变）：只搬运原 `impl WorkbenchView` 行段，
//! 私有方法提为 `pub(super)` 供 `view` 子模块互相调用。

use super::*;
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
    pub(super) fn assemble(shared: Shared, spawn_lsp: bool, cx: &mut Context<Self>) -> Self {
        // B11：编辑器的共享状态**提前建好**——M1 项目桥（未保存草稿拦截）与连接端口都要它，
        // 而下面 `build_host` 就会用到；它是 `Rc` 句柄（不是 gpui 实体），clone 很便宜。
        let editor_service = editor::shared::EditorShared::new();
        editor_service.open(editor::service::OpenRequest::untitled(
            "",
            editor::model::EditorMode::Sql,
        ));
        // P2-a：编辑器宿主端口（连接快照 / 项目根 / 项目锁 / 宿主命令转发给 Shared）——
        // 已下沉 `editor::ports` 的端口实现（exec 等）只依赖这个 trait，不再碰 Shared 具体类型
        let editor_host = Rc::new(WorkbenchEditorHost::new(shared.clone()));
        // 联邦源清单是工作线程端口（`QueryRunner: Send + Sync`，不持 Shared）
        let fed_source = crate::components::editor_host::federated_source();
        // A14：把执行端口接上（文档绑定/活动连接）。未接时执行动作会明确报“未接入执行”。
        crate::services::editor_exec::attach(&editor_service, fed_source);
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
        crate::services::editor_connections::attach(&editor_service, editor_host.clone());
        // B13：把执行通道端口接上（源库 / 本地加速 / 联邦 的门控；未接时后两档都不可选并给原因）
        crate::services::editor_channels::attach(&editor_service, editor_host.clone());
        // T1.6：把源清单端口接上（联邦会话快照 → 「源清单 ▾」浮层；未接时浮层只有空态说明）
        crate::services::editor_sources::attach(&editor_service);
        // M8：把「洞察此列」端口接上（结果集列头右键 → 右 Dock 洞察面板；未接时不摆这一项）
        crate::services::editor_insight::attach(&editor_service, editor_host.clone());
        // 【B8 余项】把「打开历史面板」端口接上（工具栏「⟲ 历史」→ 右 Dock 历史面板）
        crate::services::editor_history::attach(&editor_service, editor_host.clone());
        // 项目锁：把项目态端口接上（项目只读时，写源库对象的语句在提交前就被拒）
        crate::services::editor_project::attach(&editor_service, editor_host.clone());
        // B9：把补全端口接上（连接级元数据缓存 → 候选目录；未接时只有关键字与函数）
        let completion =
            crate::services::editor_completion::attach(&editor_service, editor_host.clone());
        // #54 B：语义诊断目录快照注入（复用补全的 L1 目录；活动文档绑连变化 → 按文档推）。
        // completion 存入视图字段（泵每拍取用；见 `ensure_catalog_pump`）
        let editor_completion = Some(completion);
        // #54 C6/C7：跳转定义目标解析注入（`rds://navigator/<限定表名>` → 导航树定位，
        // 复用 Quick Open `⌥↵` 的 `request_reveal`；未接 = 内核默认文档内移动）
        crate::services::editor_catalog::attach_jump(&editor_service, editor_host.clone());
        // T2c-1：把库 / schema 端口接上（导航缓存 → 状态栏面包屑可切项；未接时退文本连接段）
        crate::services::editor_schema::attach(&editor_service, editor_host.clone());
        // #20：内核注册表端口（后台探测一次 + 内存快照；未接时执行源「内核」档菜单置灰）
        crate::services::editor_kernels::attach(&editor_service, cx);
        // B7 切片三：把表结构端口接上（导航缓存 → 导出 SQL Updates 的键列；未接时菜单不摆该档）
        crate::services::editor_table_keys::attach(&editor_service, editor_host.clone());
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
            plugin_manager: None,
            right_insight_tab: None,
            right_mock_tab: None,
            right_history_tab: None,
            right_archive_tab: None,
            quick_open_palette: None,
            quick_open_open_pending: false,
            quick_open_mode_pending: Mode::Default,
            settings_window: None,
            project_inputs: None,
            project_picker: None,
            project_host: Some(host),
            _editor_subscription: None,
            _archive_subscription: None,
            _insight_ws_subs: Vec::new(),
            _insight_rules_watcher: None,
            editor_service,
            editor_hosts: Vec::new(),
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
    pub(super) fn lock_startup_project(shared: &Shared, project_root: Option<std::path::PathBuf>) {
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
    pub(super) fn install_startup_side_effects(&mut self) {
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
        self.open_in_editor_with(path, editor::model::ReadOnly::none(), None, window, cx)
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
    pub(super) fn scratchpad_preferred_connection(&self, path: &std::path::Path) -> Option<String> {
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
    pub(super) fn reusable_query_document(
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
    pub(super) fn editor_panel(
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
            |_this, _panel, event: &editor::view::host::EditorHostEvent, _window, cx| {
                match event {
                    editor::view::host::EditorHostEvent::StatusChanged => {
                        cx.notify();
                    }
                    editor::view::host::EditorHostEvent::ExecutionFailed(reason) => {
                        // 执行失败 → 弹全局错误通知（Error 类型，不自动消失）。
                        notification::notify_error("SQL 执行失败", Some(reason.as_str()), cx);
                        cx.notify();
                    }
                }
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
    pub(super) fn selected_connection_id(&self) -> Option<String> {
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
            self.shared.say_error(format!("打开文件失败：{error}"), cx);
        }
    }

    /// 项目视图宿主（构造期已装配）。
    pub(super) fn project_host(&self) -> &project::ui::ProjectUiHost {
        self.project_host
            .as_ref()
            .expect("project host initialized")
    }

    /// 刷新项目选择器（项目菜单「切换项目」等外部触发）。
    pub fn refresh_project_picker(&mut self, cx: &mut Context<Self>) {
        project::ui::refresh_picker(self.project_host(), cx);
    }
}
