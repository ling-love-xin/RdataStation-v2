use super::*;

impl ConnectionDialogState {
    /// 订阅项目下拉确认事件（返回的句柄必须由 `EditorPanel` 持有，释放即取消）。
    ///
    /// 选中「＋ 新增项目」→ 置位 `Shared::project_new_request`（宿主 render 消费并开新建入口）；
    /// 选中普通项目 → 项目根写回路径输入。
    ///
    /// 建立时机：由面板的请求入口（`EditorPanel::request_*`）调用——**不能**放进 `open()`：
    /// `open()` 处于面板的 `update` 上下文中，在那里再 `update` 面板会触发重入 panic。
    pub fn subscribe_project_confirm(
        self: &Rc<Self>,
        shared: &Shared,
        window: &mut Window,
        cx: &mut Context<crate::panels::EditorPanel>,
    ) -> Subscription {
        let state = Rc::clone(self);
        let shared_sel = shared.clone();
        cx.subscribe_in(
            &self.project_sel,
            window,
            move |_editor, _sel, event: &SelectEvent<SearchableVec<ProjectItem>>, window, cx| {
                let SelectEvent::Confirm(value) = event;
                state.handle_project_confirm(value.as_ref(), &shared_sel, window, cx);
                shared_sel.notify_host(cx);
            },
        )
    }

    /// 打开数据源连接对话框（状态由 EditorPanel 持有；open_dialog builder 每次渲染重建 UI）。
    ///
    /// 注：项目下拉的确认订阅**不**在此建立（本方法在面板的 `update` 上下文中被调用，
    /// 再 `update` 面板会触发重入 panic）——由 [`Self::subscribe_project_confirm`] 在
    /// 面板的请求入口（`EditorPanel::request_*`）建立。
    pub fn open(
        self: &Rc<Self>,
        entity: Entity<crate::panels::EditorPanel>,
        shared: Shared,
        editing_id: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 暂存列表回调需要克隆状态句柄（Rc）。
        let state = Rc::clone(self);
        // 重入保护：先关闭已有的本对话框层，避免连续 open 叠加（幂等打开）。
        window.close_dialog(cx);
        *self.editing_id.borrow_mut() = editing_id.clone();
        // 当前项目会话接入（C2）：项目根先取会话快照，编辑回读（项目侧 P_/GP_ 只存项目库）
        // 与项目栏预填共用它。
        let session_root = shared
            .project
            .borrow()
            .as_ref()
            .map(|p| p.root.to_string_lossy().to_string());
        if let Some(id) = &editing_id {
            self.load_for_edit(id, session_root.as_deref(), window, cx);
        }
        // 项目根自动预填，项目作用域无需手输；已填值（如编辑回读）不覆盖。
        if let Some(root) = session_root {
            if self.project_path.read(cx).value().trim().is_empty() {
                self.project_path
                    .update(cx, |s, cx| s.set_value(root, window, cx));
            }
        }
        // 项目下拉确认订阅（选中「＋ 新增项目」→ 请求宿主打开项目新建入口；选中项目 → 路径写回）。
        // 订阅由 `EditorPanel` 持有（句柄释放即取消），建立入口见 `subscribe_project_confirm`。
        let name = self.name.clone();
        let url = self.url.clone();
        let user = self.user.clone();
        let pass = self.pass.clone();
        let driver = self.driver.clone();
        let driver_filter = self.driver_filter.clone();
        let types_list = self.types.clone();
        let drivers_list = self.drivers.clone();
        let selected_type = self.selected_type.clone();
        let drafts_list = self.drafts.clone();
        let draft_cursor = self.draft_cursor.clone();
        let remark = self.remark.clone();
        let active_tab = self.active_tab.clone();
        let hops = self.hops.clone();
        let env = self.env.clone();
        let env_list = self.env_list.clone();
        let auth_method = self.auth_method.clone();
        let auth_method_loaded_for = self.auth_method_loaded_for.clone();
        let auth_ref = self.auth_ref.clone();
        let network_ref = self.network_ref.clone();
        let auth_list = self.auth_list.clone();
        let network_list = self.network_list.clone();
        let duckdb_fed = self.duckdb_fed.clone();
        let cache_path = self.cache_path.clone();
        let props = self.props.clone();
        let prop_key = self.prop_key.clone();
        let prop_val = self.prop_val.clone();
        let mgr = self.mgr.clone();
        let result = self.result.clone();
        let result_ok = self.result_ok.clone();
        let editing_id = self.editing_id.clone();
        let scope = self.scope.clone();
        let project_path = self.project_path.clone();
        let project_sel = self.project_sel.clone();
        let project_options = self.project_options.clone();
        let tags_input = self.tags_input.clone();
        let group_checks = self.group_checks.clone();
        let ssl_mode = self.ssl_mode.clone();
        let ssl_ca = self.ssl_ca.clone();
        let ssl_cert = self.ssl_cert.clone();
        let ssl_key = self.ssl_key.clone();
        let sec_overrides = self.policy_override_keys.clone();
        let env_policies = self.env_policies.clone();
        let env_policies_loaded_for = self.env_policies_loaded_for.clone();
        // 连接设置字段（主机 / 端口 / 数据库）与同步标记：与 Header URI 双向同步。
        let host_input = self.host_input.clone();
        let port_input = self.port_input.clone();
        let db_input = self.db_input.clone();
        let fields_synced_for = self.fields_synced_for.clone();

        // 项目会话变更检测（每帧执行，开销仅一次借用比较）：对话框打开期间「＋ 新增项目」
        // 或外部切换项目后，项目下拉选项与选中项必须跟上新会话。
        let session_now = shared.project.borrow().as_ref().map(|p| {
            (p.name.clone(), p.root.to_string_lossy().to_string())
        });
        if *self.session_project.borrow() != session_now {
            *self.session_project.borrow_mut() = session_now.clone();
            self.refresh_project_options(window, cx);
            match &session_now {
                // 新会话项目直接成为项目栏选中项（「新增项目」的语义就是切换到它）；
                // 路径由下方 render 同步块写回（单一数据源仍是路径输入）。
                Some((name, _)) => self.set_project_value(name, window, cx),
                None => self.set_project_value("", window, cx),
            }
        }

        // 环境变更检测（每帧比较）：策略清单来自 `environment_policies`，切环境必须重查，
        // 否则勾选项与所选环境的真实策略不一致（旧版是硬编码 6 项，无此问题也无此信息）。
        let env_now = self
            .env
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        if env_policies_loaded_for.borrow().as_deref() != Some(env_now.as_str()) {
            self.refresh_env_policies(&env_now);
        }

        // 驱动变更检测（每帧比较）：认证方法选项来自 `drivers.supported_auth_types`，
        // 换驱动必须重建（否则会显示上一个驱动的方法，落库值与驱动不符）。
        let driver_now_for_auth = self
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        if auth_method_loaded_for.borrow().as_deref() != Some(driver_now_for_auth.as_str()) {
            self.refresh_auth_method_items(&driver_now_for_auth, window, cx);
        }

        // 元数据与暂存恢复：dialog builder 每次渲染都会执行，用一次性标记避开
        // 反复建 tokio runtime + 查库（hover / 切 Tab 引发的重渲染不应再查库）。
        // 每次「打开对话框」入口（`EditorPanel::request_*`）会重置标记以重新拉取。
        if !self.meta_refreshed.replace(true) {
            // 项目根已预填时同步项目栏显示（编辑回读路径优先于会话默认值）。
            let prefilled = self.project_path.read(cx).value().to_string();
            self.sync_project_selection(&prefilled, window, cx);
            // 拉取一次元数据（认证/网络/环境引用选项 + 类型 / 驱动目录）。
            self.refresh_meta(window, cx);
            // 暂存列表：首次打开时恢复上次会话的草稿（关栏不丢失，跨会话延续）。
            self.staging_restore(window, cx);
            // 暂存列表：合并已保存连接条目（多连接连续编辑，原型设计 §2.2）。
            self.staging_merge_saved();
        }

        // 输入占位（InputState 构造后设置；Input 组件本身无 placeholder 方法）。
        // 地址占位**不在这里固定**：它随当前驱动推导（见 builder 内的 `address_placeholder`），
        // 否则选了 SQLite 还会提示 mysql:// 示例（真机反馈）。
        name.update(cx, |s, cx| s.set_placeholder("名称（如 生产 PG）", window, cx));
        remark.update(cx, |s, cx| s.set_placeholder("备注（可选）", window, cx));
        driver_filter.update(cx, |s, cx| s.set_placeholder("搜索类型…", window, cx));
        tags_input.update(cx, |s, cx| {
            s.set_placeholder("prod, core（逗号分隔）", window, cx)
        });
        prop_key.update(cx, |s, cx| s.set_placeholder("key", window, cx));
        prop_val.update(cx, |s, cx| s.set_placeholder("value", window, cx));
        project_path.update(cx, |s, cx| {
            s.set_placeholder("项目根目录（含 .RSmeta）", window, cx)
        });
        ssl_ca.update(cx, |s, cx| {
            s.set_placeholder("CA 证书路径（可选）", window, cx)
        });
        ssl_cert.update(cx, |s, cx| {
            s.set_placeholder("客户端证书路径（可选）", window, cx)
        });
        ssl_key.update(cx, |s, cx| {
            s.set_placeholder("私钥路径（可选）", window, cx)
        });
        host_input.update(cx, |s, cx| s.set_placeholder("127.0.0.1", window, cx));
        port_input.update(cx, |s, cx| s.set_placeholder("3306", window, cx));
        db_input.update(cx, |s, cx| s.set_placeholder("可选，留空表示全部", window, cx));

        // 测试连接（block_on，与 workbench 现有服务调用模式一致）。
        let run_test = move |input: DataSourceSaveInput| -> (bool, String) {
            let service = match DataSourceService::global() {
                Ok(s) => s,
                Err(e) => return (false, format!("服务未就绪: {e}")),
            };
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => return (false, format!("运行时错误: {e}")),
            };
            let t = rt.block_on(service.test(&input));
            // 反馈拼上探测到的服务器版本（原型："成功（版本＋延迟）"）。
            let detail = match t.version.as_deref() {
                Some(v) => format!("{} · 版本 {}", t.message, v),
                None => t.message,
            };
            (t.success, detail)
        };

        window.open_dialog(cx, move |dialog, window, cx| {
            let scope_sel = scope.read(cx).selected_value().cloned().unwrap_or_default().to_string();
            let includes_project = scope_from_label(&scope_sel).includes_project();
            // 项目下拉 → 路径输入同步（render 为权威同步点，兜底）：选中普通项目时写回路径，
            // 保存与作用域预检统一读路径输入（单一数据源）；「＋ 新增项目」由确认订阅处理。
            if includes_project {
                if let Some(label) = project_sel.read(cx).selected_value().cloned() {
                    let path = project_options
                        .borrow()
                        .iter()
                        .find(|(l, _)| l.as_str() == label.as_ref())
                        .map(|(_, p)| p.clone())
                        .unwrap_or_default();
                    if !path.is_empty() && project_path.read(cx).value().trim() != path {
                        project_path.update(cx, |s, cx| s.set_value(path.clone(), window, cx));
                    }
                }
            }
            // ---- 当前驱动 / 类型同步（render 为权威同步点）----
            // 放在 `cx.theme()` 之前：地址占位需要 `url.update(cx, …)`（可变借用），
            // 而 `theme` 持有 `cx` 的不可变借用（两者不能并存）。
            let driver_now = driver
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string();
            let drivers_snapshot: Vec<Driver> = drivers_list.borrow().clone();
            let types_snapshot: Vec<DataSourceType> = types_list.borrow().clone();
            // 驱动选择值 → 驱动：优先当前类型下匹配（下拉选项按类型过滤），再回退全量
            // （兼容回读 / 旧草稿的完整名）。
            let resolve_in = |pool: &[Driver]| find_driver_by_value(pool, &driver_now).cloned();
            let current_driver = {
                let tid = selected_type.borrow().clone();
                if tid.is_empty() {
                    resolve_in(&drivers_snapshot)
                } else {
                    resolve_in(&enabled_drivers_of_type(&drivers_snapshot, &tid))
                        .or_else(|| resolve_in(&drivers_snapshot))
                }
            };
            if let Some(d) = &current_driver {
                let mut st = selected_type.borrow_mut();
                // 仅在未选类型时用驱动反推补全；不覆盖用户显式选择的类型
                // （否则“点了类型但驱动仍是旧值”时侧栏高亮会弹回旧类型）。
                if st.is_empty() {
                    *st = d.type_id.clone();
                }
            }
            let selected_type_id = selected_type.borrow().clone();
            // 类型徽标（缩小的数据库类型 UI）：Header 与暂存条目共用。
            let type_badge_now = type_badge(&types_snapshot, &selected_type_id);
            // 文件型判定优先用驱动元数据（drivers.is_file），无驱动记录时回退类型名。
            let is_file_db = current_driver
                .as_ref()
                .map(|d| d.is_file)
                .unwrap_or_else(|| matches!(selected_type_id.as_str(), "sqlite" | "duckdb"));
            // 未选类型 / 驱动：表单整体置灰不可输入（控件正常显示，只改可用性）。
            let form_disabled = current_driver.is_none();
            // 驱动声明的连接字段（`drivers.config_schema.fields[]`）：行的存在性 / 标签 / 占位均由它决定。
            let form_fields: Vec<FormField> = current_driver
                .as_ref()
                .map(|d| driver_form_fields(&d.config_schema))
                .unwrap_or_default();
            // 地址占位随驱动推导（每帧写入；与上方其它输入占位同一约定）：
            // 文件型优先用 schema 声明的字段占位（如「选择 .db 或 .sqlite 文件」），否则回退类型字典。
            let want_address_ph = if is_file_db {
                address_field(&form_fields)
                    .and_then(|f| f.placeholder.clone())
                    .unwrap_or_else(|| address_placeholder(current_driver.as_ref(), &selected_type_id))
            } else {
                address_placeholder(current_driver.as_ref(), &selected_type_id)
            };
            url.update(cx, |s, cx| s.set_placeholder(want_address_ph, window, cx));

            // ---- 连接设置字段 ⇄ Header URI 双向同步（render 为权威同步点）----
            // 方向 A（URI → 字段）：URI 或驱动变化时反向填字段（编辑回读 / 手改 URI / 切类型）；
            // 方向 B（字段 → URI）：字段被编辑时用 `rebuild_url_from_fields` 回写 URI（保留凭据 / 查询串）。
            // 标记 `fields_synced_for` 记录上次同步的（驱动 id, URI）—— 避免两个方向互相覆盖。
            if !is_file_db {
                let driver_id_fields = current_driver
                    .as_ref()
                    .map(|d| d.id.clone())
                    .unwrap_or_default();
                let url_now = url.read(cx).value().to_string();
                let already_synced = matches!(
                    fields_synced_for.borrow().as_ref(),
                    Some((d, u)) if d == &driver_id_fields && u == &url_now
                );
                if !already_synced {
                    let (h, p, d) = crate::services::data_source_service::parse_url_host_port_db(
                        &driver_id_fields,
                        &url_now,
                    );
                    set_input_value(&host_input, h.unwrap_or_default(), window, cx);
                    set_input_value(
                        &port_input,
                        p.map(|v| v.to_string()).unwrap_or_default(),
                        window,
                        cx,
                    );
                    set_input_value(&db_input, d.unwrap_or_default(), window, cx);
                    *fields_synced_for.borrow_mut() = Some((driver_id_fields, url_now));
                } else {
                    let rebuilt = crate::services::data_source_service::rebuild_url_from_fields(
                        &driver_id_fields,
                        &url_now,
                        &host_input.read(cx).value(),
                        &port_input.read(cx).value(),
                        &db_input.read(cx).value(),
                        current_driver.as_ref().and_then(|d| d.default_port),
                    );
                    if rebuilt != url_now {
                        set_input_value(&url, rebuilt.clone(), window, cx);
                        *fields_synced_for.borrow_mut() = Some((driver_id_fields, rebuilt));
                    }
                }
            }

            let theme = cx.theme();

            // ---- Tab 条（自绘；gpui-component 无 Tabs 组件）----
            // 文件型驱动（SQLite/DuckDB）按原型隐藏「网络」Tab（无协议链 / SSL 语义）。
            let tab_defs: Vec<(&'static str, usize)> = if is_file_db {
                vec![("常规", 0), ("能力", 2), ("驱动属性", 3), ("高级", 4)]
            } else {
                vec![
                    ("常规", 0),
                    ("网络", 1),
                    ("能力", 2),
                    ("驱动属性", 3),
                    ("高级", 4),
                ]
            };
            let mut tab_bar = div()
                .h_flex()
                .items_center()
                .gap_1()
                .border_b_1()
                .border_color(theme.colors.border);
            for (label, tab_ix) in tab_defs {
                let on = active_tab.get() == tab_ix;
                let idx = tab_ix;
                let active_tab = active_tab.clone();
                let entity = entity.clone();
                tab_bar = tab_bar.child(
                    div()
                        .id(ElementId::Name(SharedString::from(format!("tab-{label}"))))
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .font_weight(if on { FontWeight::BOLD } else { FontWeight::NORMAL })
                        .text_color(if on { theme.colors.foreground } else { theme.colors.muted_foreground })
                        .border_b_2()
                        .border_color(if on { theme.colors.primary } else { hsla(0., 0., 0., 0.) })
                        .child(label)
                        .on_click(move |_, _, app| {
                            active_tab.set(idx);
                            entity.update(app, |_, cx| cx.notify());
                        }),
                );
            }

            // 项目会话（供常规 Tab 组织卡片与 Header 共用）：已打开项目时 Header 只显示项目名
            // （悬停气泡展示完整路径）；未打开项目则保留可编辑的项目根输入。
            let project_session: Option<(String, String)> = shared
                .project
                .borrow()
                .as_ref()
                .map(|p| (p.name.clone(), p.root.to_string_lossy().to_string()));

            // ---- 单列分组大纲：共用的分组构造器（常规与高级 Tab 同一套排版）----
            // 分组标题行 = 折叠手柄；折叠态存 `collapsed_sections`（纯 UI 偏好）。
            let make_section = {
                let state = state.clone();
                let entity = entity.clone();
                move |id: &'static str, icon: Icon, color: Hsla, title: &str, body: Div| -> Div {
                    let collapsed = state.section_collapsed(id);
                    let state_click = state.clone();
                    let entity_click = entity.clone();
                    outline_section(
                        theme,
                        id,
                        icon,
                        color,
                        title,
                        collapsed,
                        move |_window, app| {
                            state_click.toggle_section(id);
                            entity_click.update(app, |_, cx| cx.notify());
                        },
                        body,
                    )
                }
            };

            // ---- 各 Tab 内容 ----
            let tab_content = match active_tab.get() {
                1 => {
                    // ===== 网络：协议链 + 拓扑 =====
                    let network_ref_selected = network_ref.read(cx).selected_value().cloned();
                    let is_ref = network_ref_selected.is_some();
                    let hops_ui = {
                        let hops_outer = hops.clone();
                        let hops_ref = hops.borrow();
                        let mut rows = div().v_flex().gap_1();
                        if hops_ref.is_empty() {
                            rows = rows.child(
                                div().text_xs().text_color(theme.colors.muted_foreground)
                                    .child("无协议链跳——直接连接目标数据库"),
                            );
                        }
                        for (i, hop) in hops_ref.iter().enumerate() {
                            let hop = hop.clone();
                            let idx = i;
                            let hops_outer = hops_outer.clone();
                            let entity = entity.clone();
                            let kind_color = if hop.kind == "SSH" {
                                theme.colors.success
                            } else {
                                theme.colors.warning
                            };
                            rows = rows.child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .px_2()
                                    .py_1()
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child(format!("{}", idx + 1)))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(kind_color)
                                            .child(hop.kind.clone()),
                                    )
                                    .child(div().text_sm().flex_1().child(hop.label.clone()))
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("hop-en-{idx}"))))
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(if hop.enabled { theme.colors.success } else { theme.colors.muted_foreground })
                                            .child(if hop.enabled { "启用" } else { "停用" })
                                            .on_click({
                                                let hops = hops_outer.clone();
                                                let entity = entity.clone();
                                                move |_, _, app| {
                                                    let mut h = hops.borrow_mut();
                                                    if let Some(hop) = h.get_mut(idx) { hop.enabled = !hop.enabled; }
                                                    drop(h);
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            }),
                                    )
                                    .child(
                                        div().h_flex().gap_1()
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(format!("hop-up-{idx}"))))
                                                    .cursor_pointer()
                                                    .text_xs()
                                                    .text_color(theme.colors.muted_foreground)
                                                    .child("↑")
                                                    .on_click({
                                                        let hops = hops_outer.clone();
                                                        let entity = entity.clone();
                                                        move |_, _, app| {
                                                            let mut h = hops.borrow_mut();
                                                            if idx > 0 { h.swap(idx - 1, idx); }
                                                            drop(h);
                                                            entity.update(app, |_, cx| cx.notify());
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(format!("hop-dn-{idx}"))))
                                                    .cursor_pointer()
                                                    .text_xs()
                                                    .text_color(theme.colors.muted_foreground)
                                                    .child("↓")
                                                    .on_click({
                                                        let hops = hops_outer.clone();
                                                        let entity = entity.clone();
                                                        move |_, _, app| {
                                                            let mut h = hops.borrow_mut();
                                                            if idx + 1 < h.len() { h.swap(idx, idx + 1); }
                                                            drop(h);
                                                            entity.update(app, |_, cx| cx.notify());
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(format!("hop-del-{idx}"))))
                                                    .cursor_pointer()
                                                    .text_xs()
                                                    .text_color(theme.colors.danger)
                                                    .child("删除")
                                                    .on_click({
                                                        let hops = hops_outer.clone();
                                                        let entity = entity.clone();
                                                        move |_, _, app| {
                                                            hops.borrow_mut().remove(idx);
                                                            entity.update(app, |_, cx| cx.notify());
                                                        }
                                                    }),
                                            ),
                                    ),
                            );
                        }
                        rows
                    };

                    let add_hop = div().h_flex().items_center().gap_2()
                        .child(
                            Button::new("add-ssh")
                                .secondary()
                                .label("+ SSH 跳")
                                .on_click({
                                    let hops = hops.clone();
                                    let entity = entity.clone();
                                    let result = result.clone();
                                    let result_ok = result_ok.clone();
                                    move |_, _, app| {
                                        let mut h = hops.borrow_mut();
                                        if h.len() >= MAX_HOPS {
                                            *result.borrow_mut() =
                                                Some(format!("协议链最多 {} 跳", MAX_HOPS));
                                            result_ok.set(false);
                                            drop(h);
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let n = h.len() + 1;
                                        h.push(Hop::ssh(format!("跳板机·ssh-{n}")));
                                        drop(h);
                                        *result.borrow_mut() = Some("已添加 SSH 跳".into());
                                        result_ok.set(true);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        )
                        .child(
                            Button::new("add-proxy")
                                .secondary()
                                .label("+ Proxy 跳")
                                .on_click({
                                    let hops = hops.clone();
                                    let entity = entity.clone();
                                    let result = result.clone();
                                    let result_ok = result_ok.clone();
                                    move |_, _, app| {
                                        let mut h = hops.borrow_mut();
                                        if h.len() >= MAX_HOPS {
                                            *result.borrow_mut() =
                                                Some(format!("协议链最多 {} 跳", MAX_HOPS));
                                            result_ok.set(false);
                                            drop(h);
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let n = h.len() + 1;
                                        h.push(Hop::proxy(format!("代理·http-{n}")));
                                        drop(h);
                                        *result.borrow_mut() = Some("已添加 Proxy 跳".into());
                                        result_ok.set(true);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        )
                        .child(
                            div().text_xs().text_color(theme.colors.muted_foreground)
                                .child(format!("≤ {} 跳（SSH / HTTP(S) 代理）", MAX_HOPS)),
                        );

                    // 拓扑预览（DB 节点带 TLS 徽标，SSL 在常规→连接安全）。
                    let hops = hops.borrow();
                    let mut path = div().h_flex().items_center().gap_2().flex_wrap();
                    path = path.child(
                        div()
                            .text_xs()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.colors.border)
                            .px_2()
                            .py_1()
                            .child("本机客户端"),
                    );
                    for hop in hops.iter() {
                        path = path
                            .child(div().text_xs().text_color(theme.colors.muted_foreground).child("→"))
                            .child(
                                div()
                                    .text_xs()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .px_2()
                                    .py_1()
                                    .child(format!("{} · {}", hop.kind, hop.label)),
                            );
                    }
                    path = path
                        .child(div().text_xs().text_color(theme.colors.muted_foreground).child("→"))
                        .child(
                            div()
                                .text_xs()
                                .rounded_md()
                                .border_1()
                                .border_color(theme.colors.border)
                                .px_2()
                                .py_1()
                                .child("目标数据库")
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.info)
                                        .child(" TLS"),
                                ),
                        );

                    let mut content = div().v_flex().gap_3();
                    content = content.child(
                        div().v_flex().gap_2()
                            .child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::BOLD).child("协议链"))
                                    .child(
                                        div().text_xs().text_color(theme.colors.muted_foreground)
                                            .child("引用网络配置："),
                                    )
                                    .child(Select::new(&network_ref).placeholder("（内联编辑）"))
                                    .child(
                                        Button::new("mgr-network")
                                            .ghost()
                                            .label("管理")
                                            .on_click({
                                                let mgr = mgr.clone();
                                                let entity = entity.clone();
                                                let shared = shared.clone();
                                                move |_, window, app| {
                                                    open_manager(1, &mgr, entity.clone(), shared.clone(), window, app);
                                                }
                                            }),
                                    ),
                            ),
                    );
                    if is_ref {
                        content = content.child(
                            div().text_xs().text_color(theme.colors.info)
                                .child("已引用网络档案 · 链字段只读（改档案一处全量生效）"),
                        );
                    } else {
                        content = content.child(hops_ui).child(add_hop);
                    }
                    content = content.child(
                        div().v_flex().gap_1()
                            .child(div().text_sm().font_weight(FontWeight::BOLD).child("数据路径预览"))
                            .child(path),
                    );
                    content
                }
                2 => {
                    // ===== 能力：只读矩阵（清单与命中均来自 `drivers.capabilities`，UI 只做标签映射）=====
                    let caps = driver_capabilities(
                        current_driver.as_ref().and_then(|d| d.capabilities.as_deref()),
                    );
                    let declared_count = caps.len();
                    let mut chips = div().h_flex().gap_2().flex_wrap();
                    for (label, ok) in capability_rows(&caps) {
                        chips = chips.child(
                            div()
                                .text_xs()
                                .rounded_full()
                                .border_1()
                                .px_3()
                                .py_1()
                                .border_color(if ok { theme.colors.success } else { theme.colors.border })
                                .text_color(if ok { theme.colors.success } else { theme.colors.muted_foreground })
                                .child(label),
                        );
                    }
                    let hint = match current_driver.as_ref() {
                        None => "先在左侧选择数据库类型与驱动实现".to_string(),
                        Some(d) if declared_count == 0 => {
                            format!("{}：{CAP_EMPTY_HINT}", driver_short_name(&d.name))
                        }
                        Some(d) => format!(
                            "{}：drivers.capabilities 声明 {declared_count} 项 · 只读展示",
                            driver_short_name(&d.name)
                        ),
                    };
                    div().v_flex().gap_2()
                        .child(div().text_xs().text_color(theme.colors.muted_foreground).child(hint))
                        .child(chips)
                }
                3 => {
                    // ===== 驱动属性：key-value 动态增删 =====
                    let props_ui = {
                        let props_outer = props.clone();
                        let props_ref = props.borrow();
                        let mut rows = div().v_flex().gap_1();
                        for (i, (k, v)) in props_ref.iter().enumerate() {
                            let idx = i;
                            let k = k.clone();
                            let v = v.clone();
                            let props = props_outer.clone();
                            let entity = entity.clone();
                            rows = rows.child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_xs().child(k))
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child("="))
                                    .child(div().text_xs().flex_1().child(v))
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("prop-del-{idx}"))))
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(theme.colors.danger)
                                            .child("删除")
                                            .on_click(move |_, _, app| {
                                                props.borrow_mut().remove(idx);
                                                entity.update(app, |_, cx| cx.notify());
                                            }),
                                    ),
                            );
                        }
                        rows
                    };
                    let add_prop = div().h_flex().items_center().gap_2()
                        .child(Input::new(&prop_key))
                        .child(Input::new(&prop_val))
                        .child(
                            Button::new("add-prop")
                                .secondary()
                                .label("添加")
                                .on_click({
                                    let props = props.clone();
                                    let entity = entity.clone();
                                    let prop_key = prop_key.clone();
                                    let prop_val = prop_val.clone();
                                    let result = result.clone();
                                    let result_ok = result_ok.clone();
                                    move |_, window, app| {
                                        let k = prop_key.read(app).value().to_string();
                                        let v = prop_val.read(app).value().to_string();
                                        if k.trim().is_empty() {
                                            *result.borrow_mut() = Some("属性 key 不能为空".into());
                                            result_ok.set(false);
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let mut p = props.borrow_mut();
                                        if let Some(e) = p.iter_mut().find(|(ek, _)| ek == &k) {
                                            e.1 = v;
                                        } else {
                                            p.push((k.trim().to_string(), v));
                                        }
                                        drop(p);
                                        prop_key.update(app, |s, cx| s.set_value("", window, cx));
                                        prop_val.update(app, |s, cx| s.set_value("", window, cx));
                                        *result.borrow_mut() = Some("驱动属性已更新".into());
                                        result_ok.set(true);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        );
                    div().v_flex().gap_2()
                        .child(
                            div().text_xs().text_color(theme.colors.muted_foreground)
                                .child("driver_properties · key-value（随连接落库，覆盖驱动默认）"),
                        )
                        .child(props_ui)
                        .child(add_prop)
                }
                4 => {
                    // ===== 高级：环境 + 策略覆盖 + DuckDB 加速 =====
                    let is_network_db = {
                        let db = driver.read(cx).selected_value().cloned().unwrap_or_default();
                        db != "sqlite" && db != "duckdb" && !db.is_empty()
                    };
                    let env_summary = {
                        let sel = env.read(cx).selected_value().cloned();
                        if let Some(n) = sel {
                            let list = env_list.borrow();
                            list.iter().find(|e| e.name == n.as_str())
                                .map(|e| format!("{} · 策略生效中", e.name))
                                .unwrap_or_else(|| format!("{n} · 策略生效中"))
                        } else {
                            "未选择环境（使用连接默认策略）".to_string()
                        }
                    };

                    let mut content = div().w_full().v_flex().gap(rems(GAP_MD));
                    // 分组① 环境（引用 + 管理入口 + 策略生效摘要）
                    content = content.child(make_section(
                        "env",
                        lucide("icons/sliders-horizontal.svg"),
                        theme.colors.primary,
                        "环境",
                        div()
                            .w_full()
                            .v_flex()
                            .gap(rems(GAP_SM))
                            .child(form_row(
                                theme,
                                "环境",
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap(rems(GAP_SM))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .child(Select::new(&env).placeholder("选择环境…")),
                                    )
                                    .child(
                                        Button::new("mgr-env")
                                            .secondary()
                                            .label("管理环境")
                                            .on_click({
                                                let mgr = mgr.clone();
                                                let entity = entity.clone();
                                                let shared = shared.clone();
                                                move |_, window, app| {
                                                    open_manager(2, &mgr, entity.clone(), shared.clone(), window, app);
                                                }
                                            }),
                                    ),
                            ))
                            .child(hint_line(theme, &env_summary)),
                    ));
                    // 安全策略覆盖：清单来自 `environment_policies`（当前选中环境），覆盖键存策略类型。
                    let sec_rows = {
                        let keys_outer = sec_overrides.clone();
                        let policies = env_policies.borrow().clone();
                        let keys = keys_outer.borrow().clone();
                        let mut rows = div().v_flex().gap_1();
                        for (p_type, label, summary) in policies.iter() {
                            let on = keys.iter().any(|k| k == p_type);
                            let p_type_click = p_type.clone();
                            let state_click = state.clone();
                            let entity = entity.clone();
                            rows = rows.child(
                                div().h_flex().items_center().gap_2()
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("sec-{p_type}"))))
                                            .cursor_pointer()
                                            .w_8()
                                            .h(rems(1.125))
                                            .rounded_full()
                                            .bg(if on { theme.colors.primary } else { theme.colors.border })
                                            .relative()
                                            .on_click(move |_, _, app| {
                                                state_click.toggle_policy_override(&p_type_click);
                                                entity.update(app, |_, cx| cx.notify());
                                            })
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .m_0p5()
                                                    .w_3p5()
                                                    .h_3p5()
                                                    .rounded_full()
                                                    .bg(theme.colors.background)
                                                    .child(""),
                                            ),
                                    )
                                    .child(div().text_xs().child(label.clone()))
                                    .child(
                                        div().text_xs().text_color(theme.colors.muted_foreground)
                                            .child(format!("（{summary}）")),
                                    )
                                    .child(
                                        if on {
                                            div().text_xs().text_color(theme.colors.info).child("已覆盖")
                                        } else {
                                            div().text_xs().text_color(theme.colors.muted_foreground).child("默认")
                                        },
                                    ),
                            );
                        }
                        if policies.is_empty() {
                            rows = rows.child(
                                div().text_xs().text_color(theme.colors.muted_foreground)
                                    .child("无可覆盖策略：请先在上方选择环境（策略清单来自 environment_policies）"),
                            );
                        }
                        rows
                    };
                    content = content.child(make_section(
                        "policy",
                        lucide("icons/shield.svg"),
                        theme.colors.info,
                        "安全策略（覆盖环境默认）",
                        div().w_full().v_flex().gap(rems(GAP_SM)).child(sec_rows),
                    ));
                    // 分组③ DuckDB 本地加速（仅网络型库可见）：warning 色标题，开关 + 参数行均在分组内。
                    if is_network_db {
                        let toggle = div()
                            .id("duckdb-fed")
                            .cursor_pointer()
                            .w_8()
                            .h(rems(1.125))
                            .rounded_full()
                            .bg(if duckdb_fed.get() {
                                theme.colors.primary
                            } else {
                                theme.colors.border
                            })
                            .relative()
                            .on_click({
                                let duckdb_fed = duckdb_fed.clone();
                                let entity = entity.clone();
                                move |_, _, app| {
                                    duckdb_fed.set(!duckdb_fed.get());
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            })
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .m_0p5()
                                    .w_3p5()
                                    .h_3p5()
                                    .rounded_full()
                                    .bg(theme.colors.background)
                                    .child(""),
                            );
                        let mut accel_body = div()
                            .w_full()
                            .v_flex()
                            .gap(rems(GAP_SM))
                            .child(hint_line(
                                theme,
                                "仅网络数据库可用 · 凭据注册为 DuckDB Secret · 不落明文",
                            ))
                            .child(form_row(
                                theme,
                                "启用",
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap(rems(GAP_SM))
                                    .child(toggle)
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.colors.muted_foreground)
                                            .child(if duckdb_fed.get() { "已开启" } else { "已关闭" }),
                                    ),
                            ));
                        if duckdb_fed.get() {
                            accel_body = accel_body
                                .child(form_row(theme, "缓存路径", Input::new(&cache_path)))
                                .child(hint_line(
                                    theme,
                                    "已开启：凭据注册为 DuckDB Secret（不落明文），分析引擎可直接联邦查询；缓存上限 / 自动刷新 / 压缩由分析引擎默认策略管理",
                                ));
                        } else {
                            accel_body =
                                accel_body.child(hint_line(theme, "关闭：联邦查询不可用"));
                        }
                        content = content.child(make_section(
                            "accel",
                            lucide("icons/database-zap.svg"),
                            theme.colors.warning,
                            "DuckDB 本地加速（联邦查询直连源库）",
                            accel_body,
                        ));
                    }
                    content
                }
                _ => {
                    // ===== 常规：单列分组大纲（按驱动动态渲染）=====
                    let driver_value = driver_now.clone();
                    let auth_ref_selected = auth_ref.read(cx).selected_value().cloned();
                    let is_auth_ref = auth_ref_selected.is_some();

                    // 顶部信息条（info-banner）：类型 + 驱动实现（短名）。
                    let type_label = type_badge_now
                        .as_ref()
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    let info_text = if type_badge_now.is_none() {
                        "未选择数据库类型 · 请在左侧选择类型后选择驱动实现".to_string()
                    } else if driver_value.is_empty() {
                        format!("{type_label} · 未选择驱动实现（右侧下拉）")
                    } else if is_file_db {
                        format!("{type_label} · {driver_value} · 文件型数据库 · 无网络与 SSL 配置")
                    } else {
                        // 支持的方法取自驱动声明（不写死 password / ssh_key）。
                        let methods = driver_auth_types(
                            current_driver
                                .as_ref()
                                .and_then(|d| d.supported_auth_types.as_deref()),
                        );
                        let methods_text = if methods.is_empty() {
                            "驱动未声明认证方法".to_string()
                        } else {
                            format!("支持 {} 认证", methods.join(" / "))
                        };
                        format!("{type_label} · {driver_value} · 原生连接 · {methods_text}")
                    };
                    let info_banner = div()
                        .w_full()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.5))
                        .rounded(rems(0.5))
                        .border_1()
                        .border_color(theme.colors.info.opacity(0.28))
                        .bg(theme.colors.info.opacity(0.08))
                        .px(rems(0.75))
                        .py(rems(0.4375))
                        .text_xs()
                        .text_color(theme.colors.info)
                        .child(
                            Icon::new(IconName::Info)
                                .size(px(14.))
                                .text_color(theme.colors.info),
                        )
                        .child(info_text);

                    // 驱动声明的连接字段（`drivers.config_schema`）已在入口计算（`form_fields`）。
                    // 地址行标签固定为「地址 / URI」（用户明确要求），字段标签不覆盖它。
                    let addr_label = address_label(is_file_db);

                    // 未选类型 / 驱动：表单整体置灰不可输入（控件仍在，只改可用性）。
                    // 分组① 连接设置（**按驱动动态渲染**）：
                    // 文件型：地址输入 + 系统文件选择 / 新建（地址框唯一，Header 不再重复）；
                    // 网络型：主机 / 端口 / 数据库为解析摘要（纯文本行，编辑在 Header URI）。
                    let settings_body = if is_file_db {
                        div()
                            .w_full()
                            .v_flex()
                            .gap(rems(GAP_SM))
                            .child(form_row(
                                theme,
                                addr_label,
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap(rems(0.5))
                                    .child(div().flex_1().min_w(px(0.)).child(
                                        Input::new(&url).disabled(form_disabled),
                                    ))
                                    .child(
                                        Button::new("pick-db-file")
                                            .secondary()
                                            .label("打开文件…")
                                            .disabled(form_disabled)
                                            .on_click({
                                                let state = state.clone();
                                                let entity = entity.clone();
                                                move |_, window, app| {
                                                    state.pick_db_file(false, entity.clone(), window, app);
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new("new-db-file")
                                            .secondary()
                                            .label("新建文件…")
                                            .disabled(form_disabled)
                                            .on_click({
                                                let state = state.clone();
                                                let entity = entity.clone();
                                                move |_, window, app| {
                                                    state.pick_db_file(true, entity.clone(), window, app);
                                                }
                                            }),
                                    ),
                            ))
                            .child(hint_line(
                                theme,
                                "本地文件路径；「新建文件…」在所选位置创建空库文件（首次连接自动初始化结构）",
                            ))
                    } else {
                        // 网络型：主机 / 端口 / 数据库 = **可编辑字段**（与 Header URI 双向同步；
                        // 编辑字段会重建 URI（保留凭据与查询串），保存仍以 URI 为权威）。
                        let mut body = div().w_full().v_flex().gap(rems(GAP_SM));
                        for (key, fallback, input) in [
                            ("host", "主机", host_input.clone()),
                            ("port", "端口", port_input.clone()),
                            ("database", "数据库", db_input.clone()),
                        ] {
                            let spec = field_spec(&form_fields, key);
                            if !form_fields.is_empty() && spec.is_none() {
                                continue;
                            }
                            let label = spec
                                .map(|f| f.label.clone())
                                .unwrap_or_else(|| fallback.to_string());
                            body = body.child(form_row(
                                theme,
                                &label,
                                Input::new(&input).disabled(form_disabled),
                            ));
                        }
                        if form_fields.is_empty() && !form_disabled {
                            body = body.child(hint_line(theme, "驱动未声明连接字段：按内置字段展示"));
                        }
                        body.child(hint_line(
                            theme,
                            "字段与上方 URI 双向同步（凭据与查询串保留）；保存以 URI 为准",
                        ))
                    };

                    // 卡片 2：数据库认证（引用已保存配置 + 管理入口）。
                    // 认证方法下拉：选项来自当前驱动的 supported_auth_types；
                    // 引用认证配置时以配置声明的类型为准（只读展示）。
                    let auth_method_now = auth_method
                        .read(cx)
                        .selected_value()
                        .cloned()
                        .unwrap_or_default()
                        .to_string();
                    let driver_has_auth_types = !driver_auth_types(
                        current_driver
                            .as_ref()
                            .and_then(|d| d.supported_auth_types.as_deref()),
                    )
                    .is_empty();
                    let auth_method_row = {
                        let mut row = div()
                            .h_flex()
                            .items_center()
                            .gap(rems(0.5))
                            .child(
                                div()
                                    .w(rems(5.75))
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("认证方法"),
                            );
                        let cell = if is_auth_ref {
                            // 引用档案：方法由档案声明，不可在此改。
                            Select::new(&auth_method)
                                .placeholder("由认证档案声明")
                                .disabled(true)
                                .into_any_element()
                        } else if current_driver.is_none() || !driver_has_auth_types {
                            Select::new(&auth_method)
                                .placeholder("驱动未声明认证方法")
                                .disabled(true)
                                .into_any_element()
                        } else {
                            Select::new(&auth_method)
                                .placeholder("选择认证方法…")
                                .into_any_element()
                        };
                        row = row.child(div().flex_1().min_w(px(0.)).child(cell));
                        row
                    };
                    let auth_body = div()
                        .v_flex()
                        .gap(rems(0.375))
                        .child(auth_method_row)
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap(rems(0.5))
                                .child(
                                    div()
                                        .w(rems(5.75))
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("引用配置"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .child(
                                            Select::new(&auth_ref)
                                                .placeholder("引用已保存配置…")
                                                .disabled(form_disabled),
                                        ),
                                )
                                .child(
                                    Button::new("mgr-auth")
                                        .ghost()
                                        .label("管理")
                                        .on_click({
                                            let mgr = mgr.clone();
                                            let entity = entity.clone();
                                            let shared = shared.clone();
                                            move |_, window, app| {
                                                open_manager(0, &mgr, entity.clone(), shared.clone(), window, app);
                                            }
                                        }),
                                ),
                        )
                        .child(if is_auth_ref {
                            reuse_note(
                                theme,
                                &format!(
                                    "已引用认证档案 · 认证方法 {} · 用户名/密码只读（修改档案一处全量生效）",
                                    if auth_method_now.is_empty() {
                                        "（未声明）".to_string()
                                    } else {
                                        auth_method_now.clone()
                                    }
                                ),
                            )
                        } else {
                            div()
                                .w_full()
                                .v_flex()
                                .gap(rems(GAP_SM))
                                .child(form_row(
                                    theme,
                                    field_spec(&form_fields, "username")
                                        .map(|f| f.label.as_str())
                                        .unwrap_or("用户名"),
                                    Input::new(&user).disabled(form_disabled),
                                ))
                                .child(form_row(
                                    theme,
                                    field_spec(&form_fields, "password")
                                        .map(|f| f.label.as_str())
                                        .unwrap_or("密码"),
                                    Input::new(&pass).disabled(form_disabled),
                                ))
                        });

                    // 卡片 3：连接安全（SSL/TLS）——**按驱动声明显示**（supported_auth_types 含 ssl）。
                    let driver_declares_ssl = driver_auth_types(
                        current_driver
                            .as_ref()
                            .and_then(|d| d.supported_auth_types.as_deref()),
                    )
                    .iter()
                    .any(|m| m == "ssl");
                    let ssl_selected = ssl_mode
                        .read(cx)
                        .selected_value()
                        .cloned()
                        .unwrap_or_default()
                        .to_string();
                    let ssl_body = div()
                        .w_full()
                        .v_flex()
                        .gap(rems(GAP_SM))
                        .child(form_row(
                            theme,
                            "模式",
                            Select::new(&ssl_mode)
                                .placeholder("选择 SSL 模式…")
                                .disabled(form_disabled),
                        ))
                        .child(if ssl_selected == "disable" {
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("未启用 TLS（disable / prefer / require / verify-ca / verify-full）")
                        } else {
                            div()
                                .w_full()
                                .v_flex()
                                .gap(rems(GAP_SM))
                                .child(form_row(
                                    theme,
                                    "CA 证书",
                                    Input::new(&ssl_ca).disabled(form_disabled),
                                ))
                                .child(form_row(
                                    theme,
                                    "客户端证书",
                                    Input::new(&ssl_cert).disabled(form_disabled),
                                ))
                                .child(form_row(
                                    theme,
                                    "私钥",
                                    Input::new(&ssl_key).disabled(form_disabled),
                                ))
                        });

                    // 卡片 4：组织（标签 + 项目分组勾选；分组为项目级能力）。
                    let mut org_body = div()
                        .w_full()
                        .v_flex()
                        .gap(rems(GAP_SM))
                        .child(form_row(
                            theme,
                            "标签",
                            Input::new(&tags_input).disabled(form_disabled),
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("多个标签用逗号分隔（导航检索用）"),
                        );
                    {
                        let checks = group_checks.borrow().clone();
                        if project_session.is_none() {
                            org_body = org_body.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("分组：打开项目后可用（分组为项目级）"),
                            );
                        } else if checks.is_empty() {
                            org_body = org_body.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("分组：本项目暂无分组"),
                            );
                        } else {
                            let mut list = div().v_flex().gap(px(2.));
                            for (gid, gname, checked) in checks.iter() {
                                list = list.child(
                                    Checkbox::new(SharedString::from(format!("grp-{gid}")))
                                        .label(gname.clone())
                                        .checked(*checked)
                                        .disabled(form_disabled)
                                        .on_click({
                                            let state = state.clone();
                                            let entity = entity.clone();
                                            let gid = gid.clone();
                                            move |now, _, app| {
                                                if let Some((_, _, c)) = state
                                                    .group_checks
                                                    .borrow_mut()
                                                    .iter_mut()
                                                    .find(|(id, _, _)| *id == gid)
                                                {
                                                    *c = *now;
                                                }
                                                entity.update(app, |_, cx| cx.notify());
                                            }
                                        }),
                                );
                            }
                            org_body = org_body.child(
                                div()
                                    .v_flex()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.colors.muted_foreground)
                                            .child("分组"),
                                    )
                                    .child(list),
                            );
                        }
                    }

                    // 常规 Tab：**单列分组大纲**（info-banner 常驻 + 分组；分组可折叠，集合随驱动类型）。
                    // 卡片式并排已弃用：窄宽下会换行、卡高不齐、白底白框的输入框几乎看不见。
                    let mut outline = div().w_full().v_flex().gap(rems(GAP_MD)).child(info_banner);
                    outline = outline.child(make_section(
                        "conn",
                        lucide("icons/database.svg"),
                        theme.colors.primary,
                        "连接设置",
                        settings_body,
                    ));
                    // 认证 / SSL 仅网络型有意义：文件型只有「连接设置 + 组织」；
                    // SSL 分组另按驱动声明（supported_auth_types 含 ssl）显示。
                    if !is_file_db {
                        outline = outline.child(make_section(
                            "auth",
                            lucide("icons/lock.svg"),
                            theme.colors.primary,
                            "数据库认证",
                            auth_body,
                        ));
                        if driver_declares_ssl || form_disabled {
                            outline = outline.child(make_section(
                                "ssl",
                                lucide("icons/shield-check.svg"),
                                theme.colors.info,
                                "连接安全（SSL/TLS）",
                                ssl_body,
                            ));
                        }
                    }
                    outline.child(make_section(
                        "org",
                        lucide("icons/tag.svg"),
                        theme.colors.success,
                        "组织（标签 / 分组）",
                        org_body,
                    ))
                }
            };

            // Tab 内容区固定高度 + 垂直滚动：切换 Tab 不再改变对话框高度，
            // 侧栏（暂存列表 / 类型树）与 Header 位置保持稳定（布局不跳动）。
            let tab_body = div()
                .id("conn-tab-body")
                .w_full()
                .h(rems(TAB_BODY_H))
                .min_h_0()
                .overflow_y_scrollbar()
                .child(tab_content);

            // ---- 左侧栏（对齐原型 §2）：类型搜索 + 数据库类型分类树 ----
            let filter_text = driver_filter.read(cx).value().trim().to_lowercase();
            let mut db_tree = div().v_flex().gap_1();
            for (cat, cat_label) in [
                ("relational", "关系型"),
                ("file-based", "文件型"),
                ("analytics", "分析型"),
                ("nosql", "NoSQL"),
            ] {
                let matched: Vec<DataSourceType> = types_snapshot
                    .iter()
                    .filter(|t| t.category == cat)
                    .filter(|t| {
                        filter_text.is_empty()
                            || t.name.to_lowercase().contains(&filter_text)
                            || t.id.to_lowercase().contains(&filter_text)
                            || drivers_snapshot.iter().any(|d| {
                                d.type_id == t.id && d.name.to_lowercase().contains(&filter_text)
                            })
                    })
                    .cloned()
                    .collect();
                if matched.is_empty() {
                    continue;
                }
                db_tree = db_tree.child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("{cat_label}（{}）", matched.len())),
                );
                for t in matched {
                    let on = selected_type_id == t.id;
                    // 无可用驱动的类型：置灰 + 标注「暂无驱动」+ 点击不切换（见 `select_type` 守卫）。
                    let has_driver = type_has_driver(&drivers_snapshot, &t.id);
                    let type_id = t.id.clone();
                    let type_icon = t
                        .icon
                        .clone()
                        .filter(|i| !i.trim().is_empty())
                        .unwrap_or_else(|| "🗄".to_string());
                    let type_name = t.name.clone();
                    let name_color = if !has_driver {
                        theme.colors.border
                    } else if on {
                        theme.colors.foreground
                    } else {
                        theme.colors.muted_foreground
                    };
                    let mut row = div()
                        .id(ElementId::Name(SharedString::from(format!("type-{}", t.id))))
                        .h_flex()
                        .items_center()
                        .gap(rems(0.375))
                        .h(rems(1.75))
                        .px(rems(0.5))
                        .rounded(rems(0.375))
                        .cursor_pointer();
                    if on {
                        row = row.bg(theme.colors.sidebar_accent);
                    }
                    // 固定行高内右对齐提示：不加宽行高，避免侧栏布局跳动。
                    let hint: Option<Div> = (!has_driver).then(|| {
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_right()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("暂无驱动")
                    });
                    let mut row_el = row.child(
                        div()
                            .w(px(2.))
                            .h(rems(1.))
                            .rounded_full()
                            .bg(if on { theme.colors.primary } else { theme.colors.border }),
                    )
                    .child(div().flex_shrink_0().text_color(theme.colors.muted_foreground).child(type_icon))
                    .child(
                        div()
                            .text_xs()
                            .text_color(name_color)
                            .child(type_name),
                    );
                    if let Some(h) = hint {
                        row_el = row_el.child(h);
                    }
                    db_tree = db_tree.child(row_el.on_click({
                        let state = state.clone();
                        let entity = entity.clone();
                        move |_, window, app| {
                            // 侧栏选定类型 → Header 驱动下拉仅列该类型驱动（短名）；
                            // 无可用驱动时 `select_type` 只写提示，不改选中。
                            state.select_type(&type_id, window, app);
                            entity.update(app, |_, cx| cx.notify());
                        }
                    }));
                }
            }
            // ---- 暂存列表（多连接连续编辑；原型设计 §2.2）：草稿 + 已保存条目 ----
            let drafts_snapshot: Vec<ConnectionDraft> = drafts_list.borrow().clone();
            let cursor_now = draft_cursor.get();
            let mut staging_list = div().v_flex().gap(rems(0.25));
            for (i, d) in drafts_snapshot.iter().enumerate() {
                let on = i == cursor_now;
                let is_saved = d.saved_id.is_some();
                let label = d.display_name();
                let mut row = div()
                    .id(ElementId::Name(SharedString::from(format!("draft-{i}"))))
                    .h_flex()
                    .items_center()
                    .gap(rems(0.375))
                    .h(rems(ROW_H))
                    .px(rems(0.5))
                    .rounded(rems(0.375))
                    .cursor_pointer();
                if on {
                    row = row.bg(theme.colors.sidebar_accent);
                }
                row = row
                    .child(
                        div()
                            .w(px(2.))
                            .h(rems(1.))
                            .rounded_full()
                            .bg(if on { theme.colors.primary } else { theme.colors.border }),
                    )
                    .child({
                        // 缩小的数据库类型 UI（条目类型徽标）：与左侧类型树同一套 emoji；
                        // 无类型信息（旧草稿 / 未选类型）时回退状态点（已保存 success / 草稿 primary）。
                        match type_badge(&types_snapshot, &d.type_id) {
                            Some((icon, _name)) => div()
                                .flex_shrink_0()
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .w(px(16.))
                                .h(px(16.))
                                .rounded(rems(0.25))
                                .bg(theme.colors.sidebar_accent)
                                .text_xs()
                                .child(icon),
                            None => div()
                                .w(px(7.))
                                .h(px(7.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(if is_saved {
                                    theme.colors.success
                                } else {
                                    theme.colors.primary
                                }),
                        }
                    })
                    .child({
                        // 脏标记（●）：当前条目有未写回快照的表单修改（仅未保存草稿）。
                        let dirty = i == cursor_now && state.draft_dirty(i, cx);
                        // 来源短码（P/G/GP）：已保存条目按 ID 前缀标示作用域来源（原型 §2.2）。
                        let scope_code = d
                            .saved_id
                            .as_deref()
                            .and_then(saved_scope_short);
                        let mut name_el = div()
                            .h_flex()
                            .items_center()
                            .gap(rems(0.25))
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(if on {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(label);
                        if let Some(code) = scope_code {
                            name_el = name_el.child(
                                div()
                                    .flex_shrink_0()
                                    .px(px(3.))
                                    .rounded(px(2.))
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .text_color(theme.colors.muted_foreground)
                                    .child(code),
                            );
                        }
                        if dirty {
                            name_el = name_el.child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(theme.colors.warning)
                                    .child("●"),
                            );
                        }
                        name_el
                    });
                if !is_saved {
                    row = row.child(
                        div()
                            .id(ElementId::Name(SharedString::from(format!("draft-del-{i}"))))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("✕")
                            .on_click({
                                let state = state.clone();
                                let shared = shared.clone();
                                move |_, window, app| {
                                    state.staging_remove(i, window, app);
                                    shared.notify_host(app);
                                }
                            }),
                    );
                }
                staging_list = staging_list.child(row.on_click({
                    let state = state.clone();
                    let shared = shared.clone();
                    move |_, window, app| {
                        state.staging_select(i, window, app);
                        shared.notify_host(app);
                    }
                }));
            }
            let side_panel = div()
                .w(rems(12.5))
                .h_full()
                .min_h_0()
                .flex_shrink_0()
                .v_flex()
                .gap(rems(0.75))
                .pr(rems(0.75))
                .border_r_1()
                .border_color(theme.colors.border)
                .child(
                    Input::new(&driver_filter).prefix(
                        lucide("icons/search.svg")
                            .size(px(13.))
                            .text_color(theme.colors.muted_foreground),
                    ),
                )
                .child(
                    div()
                        .v_flex()
                        .gap(rems(0.375))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.colors.muted_foreground)
                                        .child("暂存列表"),
                                )
                                .child(
                                    div()
                                        .id("staging-add")
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme.colors.primary)
                                        .child("+ 添加")
                                        .on_click({
                                            let state = state.clone();
                                            let shared = shared.clone();
                                            move |_, window, app| {
                                                state.staging_add(window, app);
                                                shared.notify_host(app);
                                            }
                                        }),
                                ),
                        )
                        // 暂存区固定高度 + 滚动：条目再多也只在区域内滚动，不拉长对话框。
                        .child(
                            div()
                                .id("staging-scroll")
                                .w_full()
                                .h(rems(STAGING_H))
                                .min_h_0()
                                .overflow_y_scrollbar()
                                .child(staging_list),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.muted_foreground)
                        .child("数据库类型"),
                )
                // 类型树占满侧栏剩余高度并内部滚动（与暂存区共同保证侧栏高度恒定）。
                .child(
                    div()
                        .id("type-scroll")
                        .w_full()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .child(db_tree),
                );

            // ---- Header（对齐原型 §2）：名称 + 驱动类型 + 作用域 / 备注 / URI / 提示行 ----
            // 项目栏：固定位置（备注行右侧）；作用域为「仅全局」时不参与落库 → 置灰不可编辑，
            // 保证三态切换时布局不跳动。
            // 项目栏（固定位置与宽度 17rem）：不参与项目作用域时置灰禁用；
            // 参与时为一个下拉框（项目名 + 路径左右结构；末项「＋ 新增项目」）。
            let project_ui = {
                let mut wrap = div()
                    .h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(rems(GAP_MD))
                    .w(rems(PROJECT_W))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("项目"),
                    );
                if !includes_project {
                    // 仅全局：项目不参与落库，仍以禁用下拉占位（保持控件形态与行高一致）。
                    wrap = wrap.child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                Select::new(&project_sel)
                                    .placeholder("仅全局不需要")
                                    .disabled(true),
                            ),
                    );
                } else {
                    wrap = wrap.child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Select::new(&project_sel).placeholder("选择项目…")),
                    );
                }
                wrap
            };
            // 作用域：三态分段控件（自绘，颜色走主题 token；底层仍写 scope Select）。
            // 作用域：三态分段控件（自绘，颜色走主题 token；底层仍写 scope Select）。
            let scope_seg = {
                let scope_now = if scope_sel.is_empty() {
                    SCOPE_LABELS[0]
                } else {
                    scope_sel.as_str()
                };
                // 分段按钮文本用短版（写库仍用全称标签）。
                let mut seg = div()
                    .h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(1.))
                    .p(px(1.))
                    .rounded(rems(GAP_SM))
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background);
                for (short, full) in SCOPE_SEG_LABELS {
                    let active = scope_now == full;
                    let mut item = div()
                        .id(SharedString::from(format!("scope-{full}")))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .h(rems(SEG_ITEM_H))
                        .px(rems(GAP_MD))
                        .rounded(rems(GAP_XS))
                        .text_xs()
                        .cursor_pointer()
                        .child(short);
                    item = if active {
                        item.bg(theme.colors.primary)
                            .text_color(theme.colors.primary_foreground)
                    } else {
                        item.text_color(theme.colors.muted_foreground)
                            .hover(|s| s.bg(theme.colors.list_hover))
                    };
                    let item = item.on_click({
                        let scope = scope.clone();
                        let entity = entity.clone();
                        move |_, window, app| {
                            set_select_value(&scope, full, window, app);
                            entity.update(app, |_, cx| cx.notify());
                        }
                    });
                    seg = seg.child(item);
                }
                seg
            };
            // 类型徽标（仅图标，定宽 1.75rem：类型名长短不移动后续元素；未辨识时显示 ?）。
            let type_badge_ui = {
                let mut badge = div()
                    .flex_shrink_0()
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .w(rems(BADGE_W))
                    .h(rems(BADGE_H))
                    .rounded(rems(GAP_SM))
                    .border_1()
                    .text_xs();
                match &type_badge_now {
                    Some((icon, _name)) => {
                        badge = badge
                            .border_color(theme.colors.border)
                            .text_color(theme.colors.muted_foreground)
                            .child(icon.clone());
                    }
                    None => {
                        badge = badge
                            .border_color(theme.colors.warning)
                            .text_color(theme.colors.warning)
                            .child("?");
                    }
                }
                badge
            };
            // Header（3 行，按建议布局）：
            // ① 类型徽标 + 名称 + 作用域分段；② 备注 + 项目；③ 驱动 + URI。
            let header_ui = div()
                .v_flex()
                .gap(rems(GAP_MD))
                .pb(rems(GAP_MD))
                .border_b_1()
                .border_color(theme.colors.border)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(GAP_LG))
                        .min_w(px(0.))
                        .child(type_badge_ui)
                        .child(header_label(theme, "名称"))
                        .child(Input::new(&name).flex_1())
                        .child(scope_seg),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(GAP_LG))
                        .min_w(px(0.))
                        .child(header_label(theme, "备注"))
                        .child(Input::new(&remark).flex_1())
                        .child(project_ui),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(GAP_LG))
                        .min_w(px(0.))
                        .child(header_label(theme, "驱动"))
                        .child(
                            div()
                                .w(rems(DRIVER_W))
                                .flex_shrink_0()
                                .child(if type_badge_now.is_some() {
                                    if type_has_driver(&drivers_snapshot, &selected_type_id) {
                                        Select::new(&driver)
                                            .placeholder("选择驱动实现…")
                                            .into_any_element()
                                    } else {
                                        // 类型已选但无可用驱动（旧草稿 / 目录缺驱动）：只读占位。
                                        Select::new(&driver)
                                            .placeholder("该类型暂无可用驱动")
                                            .disabled(true)
                                            .into_any_element()
                                    }
                                } else {
                                    // 未选类型：仍以禁用下拉占位（保持控件形态与行高一致）。
                                    Select::new(&driver)
                                        .placeholder("先选类型")
                                        .disabled(true)
                                        .into_any_element()
                                }),
                        )
                        .child(header_label(theme, address_label(is_file_db)))
                        .child(if is_file_db {
                            // 文件型：地址在「常规 → 连接设置」编辑（那里带「打开文件 / 新建文件」），
                            // Header 只显示当前路径（截断）或引导文案，避免两个地址输入框。
                            let path_now = url.read(cx).value().to_string();
                            let text = if path_now.trim().is_empty() {
                                "在「常规 → 连接设置」选择或新建数据库文件".to_string()
                            } else {
                                path_now
                            };
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .text_xs()
                                .text_ellipsis()
                                .text_color(theme.colors.muted_foreground)
                                .child(text)
                                .into_any_element()
                        } else {
                            Input::new(&url)
                                .disabled(form_disabled)
                                .flex_1()
                                .into_any_element()
                        }),
                );

            let result_ui = {
                let msg = result.borrow().clone();
                if let Some(msg) = msg {
                    let color = if result_ok.get() { theme.colors.success } else { theme.colors.danger };
                    div().text_xs().text_color(color).child(msg)
                } else {
                    div().h(rems(1.125))
                }
            };

            // 测试连接动作（按钮 / Ctrl+T 共用；返回是否成功）。
            let do_test: Rc<dyn Fn(&mut Window, &mut App) -> bool> = {
                let name = name.clone();
                let url = url.clone();
                let user = user.clone();
                let pass = pass.clone();
                let driver = driver.clone();
                let result = result.clone();
                let result_ok = result_ok.clone();
                let state = ConnectionDialogState::cloned_state(
                    name.clone(), url.clone(), user.clone(), pass.clone(), driver.clone(),
                    remark.clone(), auth_method.clone(), auth_ref.clone(), network_ref.clone(), env.clone(),
                    auth_list.clone(), network_list.clone(), env_list.clone(),
                    duckdb_fed.clone(), cache_path.clone(), props.clone(),
                    hops.clone(), scope.clone(), ssl_mode.clone(), ssl_ca.clone(),
                    ssl_cert.clone(), ssl_key.clone(), sec_overrides.clone(),
                    drivers_list.clone(), selected_type.clone(), tags_input.clone(),
                );
                let run_test = run_test.clone();
                let entity = entity.clone();
                Rc::new(move |_window, app| {
                    let Some(input) = state.collect(&name, &driver, &url, &user, &pass, app) else {
                        *result.borrow_mut() = Some("请填写名称、驱动与连接 URL".into());
                        result_ok.set(false);
                        entity.update(app, |_, cx| cx.notify());
                        return false;
                    };
                    if let Err(e) = state.hops_valid(app) {
                        *result.borrow_mut() = Some(e);
                        result_ok.set(false);
                        entity.update(app, |_, cx| cx.notify());
                        return false;
                    }
                    *result.borrow_mut() = Some("测试中…".into());
                    result_ok.set(true);
                    entity.update(app, |_, cx| cx.notify());
                    let (ok, msg) = run_test(input);
                    *result.borrow_mut() = Some(msg);
                    result_ok.set(ok);
                    entity.update(app, |_, cx| cx.notify());
                    ok
                })
            };

            // 保存动作（按钮 / Ctrl+Enter 共用；返回是否成功，供「保存并关闭」判断）。
            let do_save: Rc<dyn Fn(&mut Window, &mut App) -> bool> = {
                let name = name.clone();
                let url = url.clone();
                let user = user.clone();
                let pass = pass.clone();
                let driver = driver.clone();
                let result = result.clone();
                let result_ok = result_ok.clone();
                // 暂存列表需要对话框状态句柄（Rc）——在 `state`（ClonedDialogState）遮蔽前取出。
                let dialog = Rc::clone(&state);
                let state = ConnectionDialogState::cloned_state(
                    name.clone(), url.clone(), user.clone(), pass.clone(), driver.clone(),
                    remark.clone(), auth_method.clone(), auth_ref.clone(), network_ref.clone(), env.clone(),
                    auth_list.clone(), network_list.clone(), env_list.clone(),
                    duckdb_fed.clone(), cache_path.clone(), props.clone(),
                    hops.clone(), scope.clone(), ssl_mode.clone(), ssl_ca.clone(),
                    ssl_cert.clone(), ssl_key.clone(), sec_overrides.clone(),
                    drivers_list.clone(), selected_type.clone(), tags_input.clone(),
                );
                let entity = entity.clone();
                let shared = shared.clone();
                let editing_id = editing_id.clone();
                let project_path = project_path.clone();
                Rc::new(move |window, app| {
                    let Some(input) = state.collect(&name, &driver, &url, &user, &pass, app) else {
                        *result.borrow_mut() = Some("请填写名称、驱动与连接 URL".into());
                        result_ok.set(false);
                        entity.update(app, |_, cx| cx.notify());
                        return false;
                    };
                    if let Err(e) = state.hops_valid(app) {
                        *result.borrow_mut() = Some(e);
                        result_ok.set(false);
                        entity.update(app, |_, cx| cx.notify());
                        return false;
                    }
                    // 编辑模式走 update（按 ID 前缀路由 G_/P_/GP_）；新建走 save（按作用域落库）。
                    let editing = editing_id.borrow().clone();
                    let project_path_val = {
                        let v = project_path.read(app).value().to_string();
                        if v.trim().is_empty() { None } else { Some(v.trim().to_string()) }
                    };
                    let save = DataSourceService::global().and_then(|service| {
                        let rt = tokio::runtime::Runtime::new().map_err(|e| {
                            shared::error::CoreError::common(
                                shared::error::CommonError::General(format!("tokio: {e}")),
                            )
                        })?;
                        match &editing {
                            Some(cid) => rt
                                .block_on(service.update(cid, &input, project_path_val.as_deref()))
                                .map(|_| cid.clone()),
                            None => rt.block_on(service.save(&input, project_path_val.as_deref())),
                        }
                    });
                    let ok = match save {
                        Ok(conn_id) => {
                            // 刷新列表：带上当前项目根，项目侧 P_/GP_ 连接一并可见。
                            let root = shared
                                .project
                                .borrow()
                                .as_ref()
                                .map(|p| p.root.clone());
                            let (items, _) = crate::services::workspace_loader::load_connections_for_scope(root.as_deref());
                            *shared.connections.borrow_mut() = items;
                            *shared.notice.borrow_mut() =
                                Some(format!("连接「{}」已保存（{}）", input.name, conn_id));
                            // 分组同步（替换语义；项目级，未打开项目时服务侧自动忽略）。
                            // 标签已在服务内部随保存 / 更新同步到 connection_tags。
                            let group_ids: Vec<String> = dialog
                                .group_checks
                                .borrow()
                                .iter()
                                .filter(|(_, _, checked)| *checked)
                                .map(|(gid, _, _)| gid.clone())
                                .collect();
                            if let Ok(service) = DataSourceService::global() {
                                service.set_connection_groups(
                                    &conn_id,
                                    &group_ids,
                                    project_path_val.as_deref(),
                                );
                            }
                            // 暂存列表（原型设计 §2.2 规则 4）：草稿转正式 + 自动补空草稿；
                            // 保存后保持对话框打开，支持连续编辑多个连接。
                            dialog.staging_after_save(&conn_id, &input.name, window, app);
                            *result.borrow_mut() =
                                Some(format!("已保存：{conn_id}（已加入暂存列表，可继续新建）"));
                            result_ok.set(true);
                            // 宿主重绘：刷新层内容（暂存列表 + 连接列表）
                            shared.notify_host(app);
                            true
                        }
                        Err(e) => {
                            *result.borrow_mut() = Some(format!("保存失败: {e}"));
                            result_ok.set(false);
                            false
                        }
                    };
                    entity.update(app, |_, cx| cx.notify());
                    ok
                })
            };

            // GP_ 快照连接：提供「从全局定义同步」（快照是独立副本，需显式同步）。
            let snapshot_id = editing_id
                .borrow()
                .clone()
                .filter(|id| id_prefix::is_snapshot(id));
            let sync_from_global: Option<Button> = snapshot_id.map(|gpid| {
                Button::new("sync-from-global")
                    .secondary()
                    .label("从全局定义同步")
                    .on_click({
                        let state = state.clone();
                        let shared = shared.clone();
                        let project_path = project_path.clone();
                        move |_, _window, app| {
                            let root = project_path.read(app).value().trim().to_string();
                            let outcome = (|| -> Result<(), String> {
                                let service =
                                    DataSourceService::global().map_err(|e| e.to_string())?;
                                let rt = tokio::runtime::Runtime::new()
                                    .map_err(|e| format!("运行时错误: {e}"))?;
                                rt.block_on(service.sync_snapshot_from_global(&gpid, &root))
                                    .map_err(|e| e.to_string())?;
                                Ok(())
                            })();
                            match outcome {
                                Ok(()) => {
                                    // 同步后重载表单（让用户看到同步结果；此时已写项目库）。
                                    state.load_for_edit(&gpid, Some(root.as_str()), _window, app);
                                    *state.result.borrow_mut() = Some(format!(
                                        "已从全局定义同步（{gpid}）：项目快照已更新"
                                    ));
                                    state.result_ok.set(true);
                                }
                                Err(e) => {
                                    *state.result.borrow_mut() = Some(format!("同步失败: {e}"));
                                    state.result_ok.set(false);
                                }
                            }
                            shared.notify_host(app);
                        }
                    })
            });
            // footer：0.6 中为 `impl IntoElement`，直接传按钮容器。
            let footer_ui = div()
                .h_flex()
                .justify_end()
                .items_center()
                .gap_2()
                .child(
                    Button::new("test-connection")
                        .secondary()
                        .label("测试连接")
                        .on_click({
                            let do_test = do_test.clone();
                            move |_, window, app| {
                                do_test(window, app);
                            }
                        }),
                )
                .when_some(sync_from_global, |d, b| d.child(b))
                .child(
                    Button::new("cancel-connection")
                        .label("取消")
                        .on_click({
                            let shared = shared.clone();
                            let state = state.clone();
                            move |_, window, app| {
                                // 关闭前写回当前草稿（暂存列表不丢失，原型设计 §2.2 规则 5）
                                state.staging_flush(app);
                                window.close_dialog(app);
                                // 宿主重绘：层才会从元素树移除（Root 的 notify 到不了子视图）
                                shared.notify_host(app);
                            }
                        }),
                )
                .child(
                    Button::new("save-connection")
                        .primary()
                        .icon(IconName::Plus)
                        .label("保存")
                        .on_click({
                            let do_save = do_save.clone();
                            move |_, window, app| {
                                do_save(window, app);
                            }
                        }),
                )
                .child(
                    Button::new("save-close-connection")
                        .label("保存并关闭")
                        .on_click({
                            let do_save = do_save.clone();
                            let shared = shared.clone();
                            move |_, window, app| {
                                if do_save(window, app) {
                                    window.close_dialog(app);
                                    // 宿主重绘：移除层（Root 的 notify 到不了子视图）
                                    shared.notify_host(app);
                                }
                            }
                        }),
                );

            dialog
                .title(if editing_id.borrow().is_some() { "编辑数据源连接" } else { "新建数据源连接" })
                .w(cx.theme().font_size * 61.25)
                .overlay(true)
                .overlay_closable(true)
                .keyboard(true)
                // Esc / 点遮罩等关闭路径：同样需宿主重绘才会移除层；关闭前写回草稿。
                .on_close({
                    let shared = shared.clone();
                    let state = state.clone();
                    move |_, _, app| {
                        state.staging_flush(app);
                        shared.notify_host(app)
                    }
                })
                .child(
                    div()
                        .h_flex()
                        .gap(rems(1.))
                        // 快捷键 context：绑定在 app 层（Ctrl+Enter 保存 / Ctrl+T 测试 / ↑↓ 切换条目）。
                        // 焦点在输入框内时，Input 处理键后 `Enter` 会 propagate 到此兜底。
                        .key_context("connection-dialog")
                        .on_action({
                            let do_save = do_save.clone();
                            move |_: &SaveConnection, window, app| {
                                do_save(window, app);
                            }
                        })
                        .on_action({
                            let do_test = do_test.clone();
                            move |_: &TestConnection, window, app| {
                                do_test(window, app);
                            }
                        })
                        .on_action({
                            let state = state.clone();
                            let shared = shared.clone();
                            move |_: &DraftPrev, window, app| {
                                let cur = state.draft_cursor.get();
                                if cur > 0 {
                                    state.staging_select(cur - 1, window, app);
                                    shared.notify_host(app);
                                }
                            }
                        })
                        .on_action({
                            let state = state.clone();
                            let shared = shared.clone();
                            move |_: &DraftNext, window, app| {
                                let cur = state.draft_cursor.get();
                                let len = state.drafts.borrow().len();
                                if cur + 1 < len {
                                    state.staging_select(cur + 1, window, app);
                                    shared.notify_host(app);
                                }
                            }
                        })
                        // 单行 Input 在 Enter 时不拦截而是 emit + propagate（gpui-base 行为）：
                        // 此处只接管 secondary（Ctrl+Enter）作保存，普通 Enter 不处理。
                        .on_action({
                            let do_save = do_save.clone();
                            move |action: &InputEnter, window, app| {
                                if action.secondary {
                                    do_save(window, app);
                                }
                            }
                        })
                        .child(side_panel)
                        .child(
                            div()
                                .v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .gap_2()
                                .child(header_ui)
                                .child(tab_bar)
                                .child(tab_body)
                                .child(result_ui),
                        ),
                )
                .footer(footer_ui)
        });
    }
}
