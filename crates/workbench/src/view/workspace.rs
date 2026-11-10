//! 面板保活 / 装配与工作区初始化。
//!
//! 自 `view.rs` 拆分（行为不变）。

use super::*;
impl WorkbenchView {
    /// 启动「草稿执行回执 → 元数据回写」泵（装配期一次）。
    ///
    /// 编辑器把「哪份文档用了哪个连接」留在 `EditorShared`；这里每秒取一次，只对草稿箱
    /// 模块内的文件写回 `file_meta`。**不挂在草稿箱面板上**：侧栅切到别的工具时面板不渲染，
    /// 而执行照样会发生。
    pub(super) fn ensure_scratchpad_meta_pump(&mut self, cx: &mut Context<Self>) {
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
    pub(super) fn ensure_catalog_pump(&mut self, cx: &mut Context<Self>) {
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
                        crate::services::editor_catalog::pump(&this.editor_service, &completion);
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
    pub(super) fn ensure_search_results_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let panel = cx.new(|cx| crate::panels::SearchResultsHostPanel::new(shared.clone(), cx));
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
    pub(super) fn ensure_metadata_search_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let panel = cx.new(|cx| crate::panels::MetadataSearchHostPanel::new(shared.clone(), cx));
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
    pub(super) fn ensure_insight_workspace_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            .push(crate::components::insight_actions::attach(
                &insight_view,
                &shared,
                cx,
            ));
        // 全量统计确认弹窗（subscribe_in 要窗口）+ 装配洞察偏好（设置 → 面板 + 引擎）。
        // 右栏装配（RightSidebarPanel::new）已做过一次；懒创建路径补做，保证
        // 中间页先于右栏打开时设置也生效（apply_insight_prefs 幂等）。
        self._insight_ws_subs
            .push(crate::components::insight_actions::attach_dialogs(
                &insight_view,
                &shared,
                window,
                cx,
            ));
        crate::components::insight_actions::apply_insight_prefs(&shared, cx);
        // D65 双栏单源：中间工作页目标变化（对比点行 / 后续工具栏切换）→ 右栏摘要联动。
        // 观察回调在事件路径触发，比较两栏当前目标，只有不同才同步（避免 set_target
        // 把右栏载荷也清掉重发请求——同目标重复设置会白白重取样）。
        let shared_for_sync = self.shared.clone();
        let panel_for_sync = panel.clone();
        self._insight_ws_subs
            .push(cx.observe(&panel_for_sync.clone(), move |_, _, cx| {
                // 探查确认窗开着（源目标未确认）：target 还没落下，此时**不联动**
                // ——否则右栏被推着一个「未确认源目标」，而右栏 set_target 对源目标
                // 只开确认窗不落 target（C3 设计），比较永不相等 → 中间页每次回填
                // （评估逐列、每秒多次）都重推右栏 → 确认窗无限重开（2026-11-05
                // 真机 panic：同帧几十个对话框 a11y 重复节点崩溃）。
                if panel_for_sync
                    .read(cx)
                    .insight_view()
                    .read(cx)
                    .probe_confirm_open()
                {
                    return;
                }
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
                    // 中间页能落下 target = 该源目标已确认（C3 拦截在中间页）。
                    // 右栏同步走**会话内导航**路径（set_target_loaded）——右栏对源
                    // 目标走 set_target 只会开一个永远没人确认的确认窗（确认在
                    // 中间页），target 永不落下，联动比较永不收敛（本条修复的另一半）。
                    right.update(cx, |panel, cx| panel.set_target_loaded(ws_target, cx));
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
    pub(super) fn consume_insight_compare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn init_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 启动默认模块（用户点名）：设置 `startup.default_left_panel` 覆盖构造期默认。
        // 必须在这里（首次 render 装配期）读——早于 `apply_left_mode`，晚于设置加载
        //（app 开窗前 `SettingsService::init` 已跑）。未知值回退默认（`from_key` 的约定）。
        {
            let saved = settings::SettingsService::startup_default_left_panel(cx);
            if let Some(panel) = workbench_shell::model::LeftPanel::from_key(&saved) {
                self.shared.active_left.set(panel);
            }
        }
        // 读取布局设置：布局锁定、侧边栏宽度。
        {
            let locked = settings::SettingsService::layout_locked(cx);
            self.shared.layout_locked.set(locked);
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
        self.left_nav_tab =
            Some(cx.new(|cx| DatabaseNavPanel::new(shared.clone(), nav_entity, cx)));
        self.left_scratch_tab =
            Some(cx.new(|cx| ScratchPanel::new(shared.clone(), scratch_entity, cx)));
        self.left_resource_tab =
            Some(cx.new(|cx| ResourcePanelTab::new(shared.clone(), resources_entity, cx)));
        self.left_plugin_tab = Some(cx.new(|cx| PluginPanel::new(shared.clone(), cx)));
        self.right_insight_tab =
            Some(cx.new(|cx| InsightPanelTab::new(shared.clone(), insight_entity, cx)));
        self.right_mock_tab = Some(cx.new(|cx| MockPanelTab::new(shared.clone(), mock_entity, cx)));
        self.right_history_tab =
            Some(cx.new(|cx| HistoryPanelTab::new(shared.clone(), history_entity, cx)));
        self.right_archive_tab = Some(cx.new(|cx| ArchivePanel::new(shared.clone(), cx)));
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

        // 插件管理页（中央常驻 tab，单实例）：左 Dock 插件面板「管理」按钮打开 / 聚焦。
        // 面板实体由本视图强持有，关 tab 后再次点击仍能挂回同一实体（搜索/筛选状态不丢）。
        {
            let plugin_manager = self
                .left_plugin_tab
                .clone()
                .map(|left| cx.new(|cx| PluginManagerPanel::new(left, cx)));
            self.plugin_manager = plugin_manager.clone();
            if let Some(panel) = plugin_manager {
                let area_for_pm = area.clone();
                let panel_for_open = panel.clone();
                *shared.open_plugin_manager.borrow_mut() =
                    Some(Rc::new(move |window: &mut Window, cx: &mut App| {
                        let id = PanelId::from(panel_for_open.entity_id());
                        // 已在任一区域（用户可能把 tab 拖到别处）就只聚焦，不能再 add：
                        // `add_panel_inner` 对已在树中的 panel 会再插一个 tab 节点。
                        let present = area_for_pm.update(cx, |area, cx| {
                            let held = [
                                DockPlacement::Center,
                                DockPlacement::Left,
                                DockPlacement::Right,
                                DockPlacement::Bottom,
                            ]
                            .into_iter()
                            .any(|placement| {
                                area.layout(placement)
                                    .is_some_and(|tree| tree.contains_panel(id))
                            });
                            if held {
                                area.select_panel(id, window, cx);
                            }
                            held
                        });
                        if !present {
                            let handle = panel_handle(panel_for_open.clone());
                            area_for_pm.update(cx, |area, cx| {
                                area.add_panel_view(
                                    handle,
                                    DockPlacement::Center,
                                    None,
                                    window,
                                    cx,
                                );
                            });
                        }
                        crate::panels::focus_plugin_manager_tab(&panel_for_open, window, cx);
                    }));
            }
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
                        area.add_panel_view(detail_handle, DockPlacement::Center, None, window, cx);
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
}
