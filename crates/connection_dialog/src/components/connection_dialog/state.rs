use super::*;

impl<H: 'static> ConnectionDialogState<H> {
    /// 当前结果行级别（无提示 → `None`；UI 与测试共用）。
    pub fn result_level(&self) -> Option<ResultLevel> {
        self.result.borrow().as_ref().map(|l| l.level)
    }

    /// 当前结果行摘要（无提示 → `None`）。
    pub fn result_summary(&self) -> Option<String> {
        self.result.borrow().as_ref().map(|l| l.summary.clone())
    }

    /// 懒创建全部受控状态（window 参与 InputState / SelectState 构造）。
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        // 驱动下拉初始为空：数据库类型在左侧栏选定后，选项才按类型填充（实现短名）。
        let drivers = SearchableVec::new(Vec::<SharedString>::new());
        // 密码单独建（掩码显示）：`state_inputs` 的第四个槽位在别处是路径类字段（SSL 私钥等），
        // 不能一概掩码。
        let (name, url, user, _) = state_inputs(window, cx);
        let pass = secret_input(window, cx);
        let (remark, cache_path, prop_key, prop_val) = state_inputs(window, cx);
        let driver_filter = cx.new(|cx| InputState::new(window, cx));
        let (host_input, port_input, db_input, _) = state_inputs(window, cx);
        let (new_name, new_data, _, _) = state_inputs(window, cx);
        let (project_path, ssl_ca, ssl_cert, ssl_key) = state_inputs(window, cx);
        // 网络配置字段输入（按类型显示子集；键固定，见 `NET_FIELD_KEYS`）：凭据类字段从一开始就是掩码输入。
        let net_inputs: Vec<(&'static str, Entity<InputState>)> = NET_FIELD_KEYS
            .iter()
            .map(|key| {
                let entity = if is_secret_field(key) {
                    secret_input(window, cx)
                } else {
                    cx.new(|cx| InputState::new(window, cx))
                };
                (*key, entity)
            })
            .collect();
        let new_type = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    AUTH_TYPES
                        .iter()
                        .map(|t| SharedString::from(*t))
                        .collect::<Vec<_>>(),
                ),
                None,
                window,
                cx,
            )
        });
        let policy_type = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
        });

        Self {
            name,
            url,
            user,
            pass,
            driver: cx.new(|cx| SelectState::new(drivers, None, window, cx)),
            driver_filter,
            types: Rc::new(RefCell::new(Vec::new())),
            drivers: Rc::new(RefCell::new(Vec::new())),
            selected_type: Rc::new(RefCell::new(String::new())),
            result: Rc::new(RefCell::new(None)),
            result_expanded: Rc::new(Cell::new(false)),
            remark,
            active_tab: Rc::new(Cell::new(0)),
            env: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            env_list: Rc::new(RefCell::new(Vec::new())),
            auth_method: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            auth_method_loaded_for: Rc::new(RefCell::new(None)),
            auth_ref: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            network_ref: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            auth_list: Rc::new(RefCell::new(Vec::new())),
            network_list: Rc::new(RefCell::new(Vec::new())),
            duckdb_fed: Rc::new(Cell::new(true)),
            save_credentials: Rc::new(Cell::new(true)),
            cache_path,
            env_policies: Rc::new(RefCell::new(Vec::new())),
            policy_override_keys: Rc::new(RefCell::new(Vec::new())),
            env_policies_loaded_for: Rc::new(RefCell::new(None)),
            props: Rc::new(RefCell::new(Vec::new())),
            props_synced: Rc::new(RefCell::new((None, Vec::new()))),
            prop_key,
            prop_val,
            mgr: Rc::new(RefCell::new(ManagerWorkspace {
                kind: 0,
                items: Vec::new(),
                project_root: None,
                new_name,
                new_type,
                new_data,
                net_inputs: Rc::new(RefCell::new(net_inputs)),
                net_specs_for: Rc::new(RefCell::new(String::new())),
                net_specs: Rc::new(RefCell::new(Vec::new())),
                editing: None,
                msg: None,
                policy_env: None,
                policy_type,
                policy_enabled: Rc::new(Cell::new(true)),
                policies: Rc::new(RefCell::new(Vec::new())),
                policy_editing: None,
            })),
            editing_id: Rc::new(RefCell::new(None)),
            scope: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        SCOPE_LABELS
                            .iter()
                            .map(|s| SharedString::from(*s))
                            .collect::<Vec<_>>(),
                    ),
                    None,
                    window,
                    cx,
                )
            }),
            project_path,
            tags_input: cx.new(|cx| InputState::new(window, cx)),
            groups: Rc::new(RefCell::new(Vec::new())),
            group_checks: Rc::new(RefCell::new(Vec::new())),
            ssl_mode: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        SSL_MODES
                            .iter()
                            .map(|s| SharedString::from(*s))
                            .collect::<Vec<_>>(),
                    ),
                    None,
                    window,
                    cx,
                )
            }),
            ssl_ca,
            ssl_cert,
            ssl_key,
            drafts: Rc::new(RefCell::new(vec![ConnectionDraft::empty()])),
            draft_cursor: Rc::new(Cell::new(0)),
            drafts_restored: Rc::new(Cell::new(false)),
            meta_refreshed: Rc::new(Cell::new(false)),
            project_sel: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<ProjectItem>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            project_options: Rc::new(RefCell::new(Vec::new())),
            session_project: Rc::new(RefCell::new(None)),
            collapsed_sections: Rc::new(RefCell::new(Vec::new())),
            busy: Rc::new(Cell::new(BusyOp::None)),
            blocked_hint: Rc::new(Cell::new(false)),
            type_tree: Rc::new(RefCell::new(None)),
            draft_list: Rc::new(RefCell::new(None)),
            host_input,
            port_input,
            db_input,
            fields_synced_for: Rc::new(RefCell::new(None)),
            url_placeholder_for: Rc::new(RefCell::new(String::new())),
            driver_derived: Rc::new(RefCell::new(DriverDerived::default())),
            pending_driver_value: Rc::new(RefCell::new(None)),
            // 分组卡「＋ 新建分组」输入行（与 mod.rs 字段声明对应）。
            group_new_open: Rc::new(Cell::new(false)),
            group_new_name: cx.new(|cx| InputState::new(window, cx)),
        }
    }

    /// 拉取元数据（认证/网络/环境列表 + Select 选项）；首次打开时调用。
    pub(crate) fn refresh_meta(&self, window: &mut Window, cx: &mut App) {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        if let Ok(service) = DataSourceService::global() {
            if let Ok(list) = rt.block_on(service.list_auth_configs()) {
                let names: Vec<SharedString> = list
                    .iter()
                    .filter_map(|a| a.name.clone())
                    .map(SharedString::from)
                    .collect();
                *self.auth_list.borrow_mut() = list;
                self.auth_ref.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(names), window, cx)
                });
            }
            if let Ok(list) = rt.block_on(service.list_network_configs()) {
                let names: Vec<SharedString> = list
                    .iter()
                    .filter_map(|n| n.name.clone())
                    .map(SharedString::from)
                    .collect();
                *self.network_list.borrow_mut() = list;
                self.network_ref.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(names), window, cx)
                });
            }
            if let Ok(list) = rt.block_on(service.list_environments()) {
                let names: Vec<SharedString> = list
                    .iter()
                    .map(|e| SharedString::from(e.name.clone()))
                    .collect();
                *self.env_list.borrow_mut() = list;
                self.env.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(names), window, cx)
                });
            }
            // 数据源类型目录（侧栏分类树）+ 驱动目录（Header「驱动」下拉）。
            // 语义：数据库类型（mysql/postgresql/…）= 侧栏；驱动实现（sqlx / Official /
            // rusqlite / duckdb-rs）= Header（仅当前类型下驱动，显示短名去噪）。
            if let Ok(types) = rt.block_on(service.list_data_source_types()) {
                *self.types.borrow_mut() = types;
            }
            if let Ok(drivers) = rt.block_on(service.list_drivers()) {
                *self.drivers.borrow_mut() = drivers;
                self.refresh_driver_items(window, cx);
            }
            // 项目分组目录（项目级：需要当前项目根；未打开项目为空列表）。
            let root = self.project_path.read(cx).value().trim().to_string();
            let groups = service.list_groups(if root.is_empty() {
                None
            } else {
                Some(root.as_str())
            });
            *self.groups.borrow_mut() = groups;
            self.sync_group_checks();
        }
        // 项目下拉选项：当前项目（若打开）+ 最近项目（名册）+ 动作项（不用项目 / 打开目录 / 新增项目）。
        self.refresh_project_options(window, cx);
    }

    /// 刷新项目下拉选项（当前项目置顶 + 最近项目去重 + 「＋ 新增项目」）。
    ///
    /// 保存落库只认路径，因此选项与路径的映射存在 `project_options`（项目名 → 路径）。
    pub(crate) fn refresh_project_options(&self, window: &mut Window, cx: &mut App) {
        let mut options: Vec<(String, String)> = Vec::new();
        let mut current_path: Option<String> = None;
        if let Some((name, path)) = self.session_project.borrow().clone() {
            options.push((name, path.clone()));
            current_path = Some(path);
        }
        if let Some(list) = project::service::list_recent(8).ok() {
            for item in list {
                let path = item.path.to_string_lossy().to_string();
                if Some(path.clone()) == current_path || options.iter().any(|(_, v)| *v == path) {
                    continue;
                }
                options.push((item.name.clone(), path));
            }
        }
        let items: Vec<ProjectItem> = options
            .iter()
            .map(|(name, path)| ProjectItem::project(name.clone(), path.clone()))
            .collect();
        let mut items = items;
        // 动作项顺序：「不需要项目」→「打开现有目录…」→ 末项固定「＋ 新增项目」（按用户约定）。
        items.push(ProjectItem::no_project());
        items.push(ProjectItem::open_folder());
        items.push(ProjectItem::new_project());
        *self.project_options.borrow_mut() = options;
        self.project_sel.update(cx, |s, cx| {
            s.set_items(SearchableVec::new(items), window, cx)
        });
    }

    /// 项目根路径 → 下拉选项 label（命中则返回；否则 None，清空选中）。
    pub(crate) fn project_label_for(&self, path: &str) -> Option<String> {
        let p = path.trim();
        if p.is_empty() {
            return None;
        }
        self.project_options
            .borrow()
            .iter()
            .find(|(_, value)| value == p)
            .map(|(label, _)| label.clone())
    }

    /// 设置项目下拉选中值（空 → 清空）。
    pub(crate) fn set_project_value(&self, label: &str, window: &mut Window, cx: &mut App) {
        let v = SharedString::from(label.trim().to_string());
        self.project_sel.update(cx, |s, cx| {
            if v.is_empty() {
                s.set_selected_index(None, window, cx);
            } else {
                s.set_selected_value(&v, window, cx);
            }
        });
    }

    /// 项目下拉确认处理（`SelectEvent::Confirm` 的落点；独立成函数便于测试）：
    ///
    /// - 「＋ 新增项目」→ 置位 `Shared::project_new_request`（宿主开新建入口）并清空选中；
    /// - 「打开现有目录…」→ 置位 `Shared::project_open_request`（宿主开目录选择）并清空选中；
    /// - 「不需要项目（仅全局）」→ 作用域切为「仅全局」并清空项目路径；
    /// - 普通项目 → 写回 `project_path`（保存 / 作用域预检统一读路径输入）。
    ///
    /// 返回 `true` 表示本次确认是一个动作项（未写入项目路径）。
    pub fn handle_project_confirm(
        &self,
        value: Option<&SharedString>,
        shared: &Rc<dyn ConnectionDialogHost>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(value) = value else {
            return false;
        };
        let label = value.to_string();
        if label == PROJECT_NEW_LABEL {
            shared.request_new_project(cx);
            self.set_project_value("", window, cx);
            return true;
        }
        if label == PROJECT_OPEN_LABEL {
            shared.request_open_project(cx);
            self.set_project_value("", window, cx);
            return true;
        }
        if label == PROJECT_NONE_LABEL {
            // 「不需要项目」＝ 切作用域为「仅全局」（项目作用域必须有项目，不应停在无效态）。
            set_select_value(&self.scope, SCOPE_LABELS[0], window, cx);
            self.project_path
                .update(cx, |s, cx| s.set_value(String::new(), window, cx));
            self.set_project_value("", window, cx);
            return true;
        }
        let path = self
            .project_options
            .borrow()
            .iter()
            .find(|(l, _)| l == &label)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        if !path.is_empty() {
            self.project_path
                .update(cx, |s, cx| s.set_value(path, window, cx));
        }
        false
    }

    /// 按项目路径同步项目下拉选中（命中 → 选中；未命中 → 清空）。
    pub(crate) fn sync_project_selection(&self, path: &str, window: &mut Window, cx: &mut App) {
        match self.project_label_for(path) {
            Some(label) => self.set_project_value(&label, window, cx),
            None => self.set_project_value("", window, cx),
        }
    }

    /// 按当前选中的环境加载启用策略（数据源：`environment_policies`）。
    ///
    /// 环境为空 / 环境不存在 / 查库失败 → 清空清单（UI 显示「无策略可覆盖」）。
    /// **不造默认值**：没选环境就不显示可覆盖项。
    pub(crate) fn refresh_env_policies(&self, env_name: &str) {
        let name = env_name.trim().to_string();
        *self.env_policies_loaded_for.borrow_mut() = Some(name.clone());
        if name.is_empty() {
            self.env_policies.borrow_mut().clear();
            return;
        }
        let rows: Vec<(String, String, String)> = (|| {
            let service = DataSourceService::global().ok()?;
            let rt = tokio::runtime::Runtime::new().ok()?;
            let policies = rt
                .block_on(service.list_environment_policies_by_name(&name))
                .ok()?;
            Some(
                policies
                    .into_iter()
                    .filter(|p| p.enabled)
                    .map(|p| {
                        (
                            p.policy_type.clone(),
                            policy_type_label(&p.policy_type),
                            policy_summary(p.policy_config.as_deref()),
                        )
                    })
                    .collect(),
            )
        })()
        .unwrap_or_default();
        *self.env_policies.borrow_mut() = rows;
    }

    /// 按当前选中的环境**异步**加载启用策略（数据源：`environment_policies`）。
    ///
    /// 查库在工作线程（`background_executor`），完成回主线程写清单并通知宿主重绘；
    /// 避免同步 `block_on` 阻塞 UI 线程（决策 #6）。环境为空 / 不存在 / 查库失败 → 清空清单。
    /// 由 `subscribe_env_confirm`（切环境事件）驱动；程序化回填（编辑回读）仍走同步版兜底。
    pub(crate) fn refresh_env_policies_async(
        &self,
        env_name: &str,
        entity: &Entity<H>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let name = env_name.trim().to_string();
        *self.env_policies_loaded_for.borrow_mut() = Some(name.clone());
        if name.is_empty() {
            self.env_policies.borrow_mut().clear();
            return;
        }
        let policies_c = self.env_policies.clone();
        let work = cx.background_executor().spawn(async move {
            (|| {
                let service = DataSourceService::global().ok()?;
                let rt = tokio::runtime::Runtime::new().ok()?;
                let policies = rt
                    .block_on(service.list_environment_policies_by_name(&name))
                    .ok()?;
                Some(
                    policies
                        .into_iter()
                        .filter(|p| p.enabled)
                        .map(|p| {
                            (
                                p.policy_type.clone(),
                                policy_type_label(&p.policy_type),
                                policy_summary(p.policy_config.as_deref()),
                            )
                        })
                        .collect(),
                )
            })()
            .unwrap_or_default()
        });
        // 回主线程写清单：`spawn_in` 是实体上下文方法，用宿主实体包一层（订阅回调里
        // 更新宿主实体是安全的——此处不在宿主的 update 上下文中）。
        entity.update(cx, |_, cx| {
            cx.spawn_in(window, async move |editor, cx| {
                let rows = work.await;
                cx.update(|_window, app| {
                    *policies_c.borrow_mut() = rows;
                    // 策略清单变了：对话框层挂宿主 render，通知宿主重绘即可见。
                    let _ = editor.update(app, |_, cx| cx.notify());
                })
                .ok();
            })
            .detach();
        });
    }

    /// 提交「＋ 新建分组」（名称非空才建；建后重载目录 + 同步勾选态）。
    ///
    /// 走服务层 `create_group`（与导航侧分组管理同一存储，建完导航栏立即可见——#7 闭环）；
    /// 未打开项目时输入行不渲染，此处仍按「无项目根」防御（项目级能力）。
    pub(crate) fn submit_group_new(&self, window: &mut Window, cx: &mut App) {
        let name = self.group_new_name.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let root = self.project_path.read(cx).value().trim().to_string();
        let project_path = if root.is_empty() { None } else { Some(root.as_str()) };
        match DataSourceService::global()
            .and_then(|s| s.create_group(&name, "", project_path))
        {
            Ok(_) => {
                // 重载目录 + 同步勾选态（新组默认未勾选；多对多保存时以勾选态为准）。
                if let Some(service) = DataSourceService::global().ok() {
                    let groups =
                        service.list_groups(if root.is_empty() { None } else { Some(root.as_str()) });
                    *self.groups.borrow_mut() = groups;
                    self.sync_group_checks();
                }
                self.group_new_open.set(false);
                self.group_new_name
                    .update(cx, |s, cx| s.set_value(String::new(), window, cx));
                set_result(&self.result, ResultLevel::Success, format!("已创建分组「{name}」"));
            }
            Err(e) => {
                set_result(&self.result, ResultLevel::Error, format!("创建分组失败：{e}"));
            }
        }
    }

    /// 切换策略覆盖勾选（按策略类型增删）。
    /// 按**目标状态**设置策略覆盖（Switch 回调传的是“请求值”而非“取反”，决策 #84）。
    pub(crate) fn set_policy_override(&self, policy_type: &str, on: bool) {
        let mut keys = self.policy_override_keys.borrow_mut();
        let pos = keys.iter().position(|k| k == policy_type);
        match (on, pos) {
            (true, None) => keys.push(policy_type.to_string()),
            (false, Some(pos)) => {
                keys.remove(pos);
            }
            _ => {}
        }
    }

    /// 单列大纲分组是否已折叠（缺省 = 展开；纯 UI 偏好，不落库）。
    pub fn section_collapsed(&self, id: &str) -> bool {
        self.collapsed_sections.borrow().iter().any(|s| s == id)
    }

    /// 折叠 / 展开大纲分组。
    pub fn toggle_section(&self, id: &str) {
        let mut list = self.collapsed_sections.borrow_mut();
        match list.iter().position(|s| s == id) {
            Some(pos) => {
                list.remove(pos);
            }
            None => list.push(id.to_string()),
        }
    }

    /// 按当前驱动刷新「认证方法」选项（数据源：`drivers.supported_auth_types`）。
    ///
    /// 驱动变化时调用（每帧检测驱动值）：选项变更后校正选中（值不在新选项 → 清空），
    /// 首次加载驱动时默认选中第一个方法（与驱动下拉“默认选中第一个实现”一致）。
    pub(crate) fn refresh_auth_method_items(
        &self,
        driver_value: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        *self.auth_method_loaded_for.borrow_mut() = Some(driver_value.to_string());
        let methods = self
            .resolve_driver(driver_value)
            .map(|d| driver_auth_types(d.supported_auth_types.as_deref()))
            .unwrap_or_default();
        let current = self
            .auth_method
            .read(cx)
            .selected_value()
            .cloned()
            .map(|v| v.to_string());
        let keep = current
            .as_ref()
            .map(|v| methods.iter().any(|m| m == v))
            .unwrap_or(false);
        let items: Vec<SharedString> = methods.iter().map(SharedString::from).collect();
        self.auth_method.update(cx, |s, cx| {
            s.set_items(SearchableVec::new(items), window, cx)
        });
        if !keep {
            // 未选 / 失效：默认第一个；驱动未声明则清空（UI 显示禁用占位）。
            let next = methods.first().cloned().unwrap_or_default();
            set_select_value(&self.auth_method, &next, window, cx);
        }
    }

    /// 用分组目录刷新勾选列表（保留已勾选；新出现的分组默认未勾选）。
    pub(crate) fn sync_group_checks(&self) {
        let groups = self.groups.borrow();
        let mut checks = self.group_checks.borrow_mut();
        let old: Vec<(String, bool)> = checks.iter().map(|(id, _, c)| (id.clone(), *c)).collect();
        *checks = groups
            .iter()
            .map(|g| {
                let checked = old
                    .iter()
                    .find(|(id, _)| *id == g.id)
                    .map(|(_, c)| *c)
                    .unwrap_or(false);
                (g.id.clone(), g.name.clone(), checked)
            })
            .collect();
    }

    /// 文件型数据库：系统文件选择器（打开 / 新建）→ 写回地址输入。
    ///
    /// - `create_new = false` → **打开**对话框（只能选已存在的文件）；
    /// - `create_new = true` → **保存**对话框（`prompt_for_new_path`：可输入新文件名）；
    ///   文件不存在则按驱动族创建（DuckDB 立即初始化；SQLite 空文件），已存在则直接引用（**不清空**，避免误损数据）。
    ///
    /// 上一版的教训：用打开对话框（Windows 带 `FOS_FILEMUSTEXIST`）当“新建”用，输入新文件名会被系统拒掉，
    /// 表现为“新建没实现”；且取消 / 失败都被静默吞掉，看起来像按钮没反应。现在三种结局都写结果行。
    pub(crate) fn pick_db_file(
        self: &Rc<Self>,
        create_new: bool,
        entity: Entity<H>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let target = self.url.clone();
        let result = self.result.clone();
        // 默认目录：当前地址的父目录（已有值时），否则工作目录。
        let current = target.read(cx).value().to_string();
        let start_dir = std::path::Path::new(current.trim())
            .parent()
            .filter(|p| p.is_dir())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
        // 建议文件名按**驱动 id** 给（与 `is_file_db` 同一来源，避免“类型 / 驱动两套事实”
        // 在文件型下打架：DuckDB → `.duckdb`，SQLite / 其余 → `.db`）。
        let driver_value = self
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        let driver_id = find_driver_by_value(&self.drivers.borrow(), &driver_value)
            .map(|d| d.id.clone())
            .unwrap_or_default();
        let suggested = new_db_file_suggested_name(&driver_id);
        if create_new {
            let receiver = cx.prompt_for_new_path(&start_dir, Some(suggested));
            window
                .spawn(cx, async move |cx| {
                    let (message, ok, value) = match receiver.await {
                        Ok(Ok(Some(path))) => create_new_db_file(&path, &driver_id),
                        Ok(Ok(None)) => ("已取消新建（地址保持原值）".to_string(), true, None),
                        Ok(Err(e)) => (format!("新建文件对话框失败：{e}"), false, None),
                        Err(_) => ("新建文件对话框无响应".to_string(), false, None),
                    };
                    apply_file_pick(&target, &result, value, message, ok, &entity, cx);
                })
                .detach();
        } else {
            let receiver = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("选择数据库文件".into()),
            });
            window
                .spawn(cx, async move |cx| {
                    let (message, ok, value) = match receiver.await {
                        Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                            Some(path) => {
                                let value = path.to_string_lossy().to_string();
                                (format!("已选择数据库文件：{value}"), true, Some(value))
                            }
                            None => ("已取消选择（地址保持原值）".to_string(), true, None),
                        },
                        Ok(Ok(None)) => ("已取消选择（地址保持原值）".to_string(), true, None),
                        Ok(Err(e)) => (format!("打开文件对话框失败：{e}"), false, None),
                        Err(_) => ("打开文件对话框无响应".to_string(), false, None),
                    };
                    apply_file_pick(&target, &result, value, message, ok, &entity, cx);
                })
                .detach();
        }
    }

    /// 侧栏选择数据库类型：更新选中、刷新 Header 驱动选项（该类型启用驱动、短名显示）
    /// 并默认选中第一个驱动。
    ///
    /// 类型下**无可用驱动**时拒绝切换并在结果行给出原因：当前只内置四个驱动，
    /// 其余类型选了也保存不了（驱动 id 解析不到 → `collect` 返回 None）。
    pub fn select_type(&self, type_id: &str, window: &mut Window, cx: &mut App) {
        if !type_has_driver(&self.drivers.borrow(), type_id) {
            let name = self
                .types
                .borrow()
                .iter()
                .find(|t| t.id == type_id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| type_id.to_string());
            *self.result.borrow_mut() = Some(ResultLine::new(
                ResultLevel::Error,
                format!(
                    "「{name}」暂无可用驱动：当前版本只内置 MySQL / PostgreSQL / SQLite / DuckDB，其余类型待驱动插件能力开放"
                ),
            ));
            return;
        }
        *self.selected_type.borrow_mut() = type_id.to_string();
        // 规则 1（决策 #110）：新建文件型连接时地址默认 = 当前项目根（地址=本项目）。
        // 仅文件型（SQLite / DuckDB）适用——网络型的 URI 没有「本项目」语义；已填值不覆盖。
        if crate::services::data_source_service::is_file_db_driver(type_id)
            && self.url.read(cx).value().trim().is_empty()
        {
            let root = self.project_path.read(cx).value().to_string();
            if !root.trim().is_empty() {
                self.url.update(cx, |s, cx| s.set_value(root, window, cx));
            }
        }
        self.refresh_driver_items(window, cx);
        let first = enabled_drivers_of_type(&self.drivers.borrow(), type_id)
            .first()
            .map(|d| driver_short_name(&d.name));
        match first {
            Some(name) => {
                set_select_value(&self.driver, &name, window, cx);
                self.refresh_auth_method_items(&name, window, cx);
            }
            None => set_select_value(&self.driver, "", window, cx),
        }
    }

    /// 按当前选中类型刷新驱动下拉选项（短名）；未选类型时置空（提示先在左侧选类型）。
    ///
    /// 选项变更后校正选中：当前值不在新选项中则清空（`set_items` 不会自动清理 selection，
    /// 会留下“下拉显示旧驱动但列表里没有”的不一致）。
    pub fn refresh_driver_items(&self, window: &mut Window, cx: &mut App) {
        let type_id = self.selected_type.borrow().clone();
        let names: Vec<SharedString> = if type_id.is_empty() {
            Vec::new()
        } else {
            enabled_drivers_of_type(&self.drivers.borrow(), &type_id)
                .iter()
                .map(|d| SharedString::from(driver_short_name(&d.name)))
                .collect()
        };
        let current = self.driver.read(cx).selected_value().cloned();
        let still_valid = current
            .as_ref()
            .map(|v| names.iter().any(|n| n.as_ref() == v.as_ref()))
            .unwrap_or(true);
        self.driver.update(cx, |s, cx| {
            s.set_items(SearchableVec::new(names), window, cx)
        });
        if !still_valid {
            set_select_value(&self.driver, "", window, cx);
        }
    }

    /// 解析下拉当前值对应的驱动：优先当前类型的启用驱动，其次全量目录
    /// （兼容旧草稿的完整名 / 跨类型回读）。
    pub fn resolve_driver(&self, value: &str) -> Option<Driver> {
        let drivers = self.drivers.borrow();
        let type_id = self.selected_type.borrow().clone();
        if !type_id.is_empty() {
            let pool = enabled_drivers_of_type(&drivers, &type_id);
            if let Some(d) = find_driver_by_value(&pool, value) {
                return Some(d.clone());
            }
        }
        find_driver_by_value(&drivers, value).cloned()
    }

    /// 取当前驱动的派生数据（表单字段 / 能力 / 认证方法）。
    ///
    /// 渲染期每帧都要用这三份数据，而它们来自驱动行的三段 JSON 声明——直接解析会每帧重复
    /// 反序列化（§14 #16）。这里按（驱动 id + 声明原文）缓存：声明不变就只克隆已解析结果，
    /// 声明变了（含切驱动、驱动目录刷新）自动重算。
    ///
    /// 与 `fields_synced_for` 同一约定：**`render` 是权威同步点**，事件回调只改状态。
    pub(crate) fn driver_derived(&self, driver: Option<&Driver>) -> DriverDerived {
        let key = driver.map(DriverDerived::cache_key).unwrap_or_default();
        if self.driver_derived.borrow().key == key {
            return self.driver_derived.borrow().clone();
        }
        let fresh = DriverDerived::resolve(driver);
        *self.driver_derived.borrow_mut() = fresh.clone();
        fresh
    }

    /// 按驱动值（短名 / 完整名 / 驱动 id）选中驱动，并保证类型与下拉选项一致
    /// （编辑回读、草稿恢复共用）；返回命中的驱动。
    pub fn set_driver_by_value(
        &self,
        type_id: &str,
        value: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Driver> {
        if !type_id.is_empty() {
            *self.selected_type.borrow_mut() = type_id.to_string();
        }
        self.refresh_driver_items(window, cx);
        let mut hit = self.resolve_driver(value);
        // 旧草稿 / 仅给驱动值时类型可能为空：命中后补全类型并重建选项，保证下拉可显示。
        if let Some(d) = &hit {
            if self.selected_type.borrow().is_empty() {
                *self.selected_type.borrow_mut() = d.type_id.clone();
                self.refresh_driver_items(window, cx);
                hit = self.resolve_driver(value).or(hit);
            }
        }
        match &hit {
            Some(d) => {
                let short = driver_short_name(&d.name);
                set_select_value(&self.driver, &short, window, cx);
                // 驱动变了 → 认证方法选项跟着重建（与 render 的每帧检测互为兵兵）。
                self.refresh_auth_method_items(&short, window, cx);
            }
            None => set_select_value(&self.driver, "", window, cx),
        }
        hit
    }

    /// 目录就绪后重放「编辑回读的驱动定位」（见 `pending_driver_value` 字段文档）。
    ///
    /// 只重放一次：命中即清空；目录仍为空（服务降级）则保留，等下次 `refresh_meta` 后再试。
    pub(crate) fn replay_pending_driver_locator(&self, window: &mut Window, cx: &mut App) {
        let Some(value) = self.pending_driver_value.borrow().clone() else {
            return;
        };
        if self.drivers.borrow().is_empty() {
            return;
        }
        *self.pending_driver_value.borrow_mut() = None;
        self.set_driver_by_value("", &value, window, cx);
    }

    /// 编辑回读：按连接 ID 预填全部 Tab 字段（协议链 / 驱动属性 / SSL / 策略覆盖 / 作用域 / 缓存路径）。
    ///
    /// `project_root`：项目侧（P_/GP_）连接只存在项目库里，回读必须带上项目根。
    pub(crate) fn load_for_edit(
        &self,
        id: &str,
        project_root: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        let Ok(service) = DataSourceService::global() else {
            return;
        };
        let Ok(Some(ds)) = rt.block_on(service.get_with_project(id, project_root)) else {
            return;
        };

        self.name
            .update(cx, |s, cx| s.set_value(ds.name.clone(), window, cx));
        self.url
            .update(cx, |s, cx| s.set_value(reconstruct_url(&ds), window, cx));
        self.user.update(cx, |s, cx| {
            s.set_value(ds.username.clone().unwrap_or_default(), window, cx)
        });
        self.remark.update(cx, |s, cx| {
            s.set_value(ds.description.clone().unwrap_or_default(), window, cx)
        });
        self.cache_path.update(cx, |s, cx| {
            s.set_value(ds.metadata_path.clone().unwrap_or_default(), window, cx)
        });
        self.duckdb_fed.set(ds.use_duckdb_fed);
        // 驱动回读：记录里的 db_type/driver_id 均为驱动 id（如 mysql_native）；
        // 反查得到类型与实现短名，先选类型再选驱动（下拉按类型过滤）。
        let did = if ds.db_type.is_empty() {
            ds.driver_id.clone().unwrap_or_default()
        } else {
            ds.db_type.clone()
        };
        if !did.is_empty() {
            let (tid, value) = {
                let drivers = self.drivers.borrow();
                find_driver_by_value(&drivers, &did)
                    .map(|d| (d.type_id.clone(), driver_short_name(&d.name)))
                    .unwrap_or((String::new(), did.clone()))
            };
            // 首帧之前调用时驱动目录为空（`refresh_meta` 还没跑）→ 定位必然失败；
            // 记下驱动值，等目录就绪后重放（否则左侧类型树无选中、驱动下拉为空）。
            let located = self.set_driver_by_value(&tid, &value, window, cx).is_some();
            *self.pending_driver_value.borrow_mut() =
                if located { None } else { Some(did.clone()) };
        }
        self.scope.update(cx, |s, cx| {
            s.set_selected_value(&SharedString::from(scope_label(&ds.scope)), window, cx)
        });
        // 认证方法：记录里为空但引用了认证配置时，用配置声明的方法（连接链路同样回退到它）。
        let auth_method_value = ds.auth_method.clone().or_else(|| {
            ds.auth_config_id.as_ref().and_then(|aid| {
                self.auth_list
                    .borrow()
                    .iter()
                    .find(|a| &a.id == aid)
                    .map(|a| a.auth_type.clone())
            })
        });
        if let Some(m) = auth_method_value {
            set_select_value(&self.auth_method, &m, window, cx);
        }

        // 标签：JSON 数组 → 逗号分隔文本（回显）；分组：读所属分组 id → 勾选。
        self.tags_input.update(cx, |s, cx| {
            s.set_value(tags_from_json(ds.tags.as_deref()), window, cx)
        });
        {
            // 分组是项目级数据：用回读时传入的项目根（而不是尚未预填的路径输入）。
            let ids = service.groups_of(id, project_root.filter(|p| !p.trim().is_empty()));
            let mut checks = self.group_checks.borrow_mut();
            for (gid, _name, checked) in checks.iter_mut() {
                *checked = ids.iter().any(|x| x == gid);
            }
        }

        // 引用选择：名称已存在于下拉选项中则选中（选项在 refresh_meta 后已就绪）。
        if let Some(aid) = &ds.auth_config_id {
            if let Some(a) = self.auth_list.borrow().iter().find(|a| &a.id == aid) {
                if let Some(n) = &a.name {
                    let n = n.clone();
                    self.auth_ref.update(cx, |s, cx| {
                        s.set_selected_value(&SharedString::from(n), window, cx)
                    });
                }
            }
        }
        if let Some(nid) = &ds.network_config_id {
            if let Some(c) = self.network_list.borrow().iter().find(|c| &c.id == nid) {
                if let Some(n) = &c.name {
                    let n = n.clone();
                    self.network_ref.update(cx, |s, cx| {
                        s.set_selected_value(&SharedString::from(n), window, cx)
                    });
                }
            }
        }
        if let Some(eid) = &ds.environment_id {
            if let Some(e) = self.env_list.borrow().iter().find(|e| &e.id == eid) {
                let n = e.name.clone();
                self.env.update(cx, |s, cx| {
                    s.set_selected_value(&SharedString::from(n), window, cx)
                });
            }
        }

        // 驱动属性（JSON → key-value 列表）。
        // **不造默认值**（§15）：库里没有就留空，由 render 的同步点按**驱动声明**填
        // （`drivers.driver_properties`）——以前这里会在空值塞一条 `connect_timeout`，
        // 而该键对 `mysql_native` 是未知参数（连接直接报错）。
        if let Some(props_json) = &ds.driver_properties {
            if let Ok(map) =
                serde_json::from_str::<std::collections::BTreeMap<String, String>>(props_json)
            {
                *self.props.borrow_mut() = map.into_iter().collect();
            }
        }

        // 高级选项：ssl / policy_overrides（`network_chain` 内联链已撤下，见架构 §14 #25）。
        if let Some(adv) = &ds.advanced_options {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(adv) {
                if let Some(ssl) = v.get("ssl").and_then(|c| c.as_object()) {
                    if let Some(mode) = ssl.get("mode").and_then(|m| m.as_str()) {
                        let mode = mode.to_string();
                        self.ssl_mode.update(cx, |s, cx| {
                            s.set_selected_value(&SharedString::from(mode), window, cx)
                        });
                    }
                    if let Some(ca) = ssl.get("ca").and_then(|m| m.as_str()) {
                        let ca = ca.to_string();
                        self.ssl_ca.update(cx, |s, cx| s.set_value(ca, window, cx));
                    }
                    if let Some(cert) = ssl.get("cert").and_then(|m| m.as_str()) {
                        let cert = cert.to_string();
                        self.ssl_cert
                            .update(cx, |s, cx| s.set_value(cert, window, cx));
                    }
                    if let Some(key) = ssl.get("key").and_then(|m| m.as_str()) {
                        let key = key.to_string();
                        self.ssl_key
                            .update(cx, |s, cx| s.set_value(key, window, cx));
                    }
                }
                if let Some(overrides) = v.get("policy_overrides").and_then(|c| c.as_array()) {
                    // 存的是策略类型（environment_policies.policy_type）；旧版本存的 6 项布尔键
                    // （read_only / no_ddl …）已不匹配现有策略清单，忽略即可（不报错）。
                    let keys: Vec<String> = overrides
                        .iter()
                        .filter_map(|o| o.as_str().map(|s| s.to_string()))
                        .collect();
                    *self.policy_override_keys.borrow_mut() = keys;
                }
            }
        }
    }
}

/// 由受控 Entity 重建轻量视图（测试/保存按钮复用 collect）。
pub(crate) struct ClonedDialogState {
    remark: Entity<InputState>,
    auth_method: Entity<SelectState<SearchableVec<SharedString>>>,
    auth_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    network_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    env: Entity<SelectState<SearchableVec<SharedString>>>,
    auth_list: Rc<RefCell<Vec<AuthConfig>>>,
    network_list: Rc<RefCell<Vec<NetworkConfig>>>,
    env_list: Rc<RefCell<Vec<Environment>>>,
    duckdb_fed: Rc<Cell<bool>>,
    save_credentials: Rc<Cell<bool>>,
    cache_path: Entity<InputState>,
    props: Rc<RefCell<Vec<(String, String)>>>,
    // ---- Phase C ----
    scope: Entity<SelectState<SearchableVec<SharedString>>>,
    ssl_mode: Entity<SelectState<SearchableVec<SharedString>>>,
    ssl_ca: Entity<InputState>,
    ssl_cert: Entity<InputState>,
    ssl_key: Entity<InputState>,
    policy_override_keys: Rc<RefCell<Vec<String>>>,
    /// 驱动目录（drivers 表）：将下拉显示的驱动短名解析为驱动 id（db_type / driver_id 落库）。
    drivers: Rc<RefCell<Vec<Driver>>>,
    /// 当前选中的数据库类型（驱动反查先去该类型下匹配，避免跨类型短名歧义）。
    selected_type: Rc<RefCell<String>>,
    /// 标签输入（逗号分隔文本；collect 解析为 JSON 数组写入 `tags`）。
    tags_input: Entity<InputState>,
}

impl<H: 'static> ConnectionDialogState<H> {
    pub(crate) fn cloned_state(
        _name: Entity<InputState>,
        _url: Entity<InputState>,
        _user: Entity<InputState>,
        _pass: Entity<InputState>,
        _driver: Entity<SelectState<SearchableVec<SharedString>>>,
        remark: Entity<InputState>,
        auth_method: Entity<SelectState<SearchableVec<SharedString>>>,
        auth_ref: Entity<SelectState<SearchableVec<SharedString>>>,
        network_ref: Entity<SelectState<SearchableVec<SharedString>>>,
        env: Entity<SelectState<SearchableVec<SharedString>>>,
        auth_list: Rc<RefCell<Vec<AuthConfig>>>,
        network_list: Rc<RefCell<Vec<NetworkConfig>>>,
        env_list: Rc<RefCell<Vec<Environment>>>,
        duckdb_fed: Rc<Cell<bool>>,
        save_credentials: Rc<Cell<bool>>,
        cache_path: Entity<InputState>,
        props: Rc<RefCell<Vec<(String, String)>>>,
        scope: Entity<SelectState<SearchableVec<SharedString>>>,
        ssl_mode: Entity<SelectState<SearchableVec<SharedString>>>,
        ssl_ca: Entity<InputState>,
        ssl_cert: Entity<InputState>,
        ssl_key: Entity<InputState>,
        policy_override_keys: Rc<RefCell<Vec<String>>>,
        drivers: Rc<RefCell<Vec<Driver>>>,
        selected_type: Rc<RefCell<String>>,
        tags_input: Entity<InputState>,
    ) -> ClonedDialogState {
        ClonedDialogState {
            remark,
            auth_method,
            auth_ref,
            network_ref,
            env,
            auth_list,
            network_list,
            env_list,
            duckdb_fed,
            save_credentials,
            cache_path,
            props,
            scope,
            ssl_mode,
            ssl_ca,
            ssl_cert,
            ssl_key,
            policy_override_keys,
            drivers,
            selected_type,
            tags_input,
        }
    }
}

impl ClonedDialogState {
    pub(crate) fn collect(
        &self,
        name: &Entity<InputState>,
        driver: &Entity<SelectState<SearchableVec<SharedString>>>,
        url: &Entity<InputState>,
        user: &Entity<InputState>,
        pass: &Entity<InputState>,
        cx: &mut App,
    ) -> Option<DataSourceSaveInput> {
        // db_type 承载驱动 id（引擎 registry key）；下拉显示的是驱动实现短名（如 sqlx），
        // 此处先用当前类型下驱动解析，再回退全量目录（兼容完整名 / 手输值）。
        let driver_value = driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        let selected_driver = {
            let drivers = self.drivers.borrow();
            let type_id = self.selected_type.borrow().clone();
            let scoped = if type_id.is_empty() {
                None
            } else {
                find_driver_by_value(&enabled_drivers_of_type(&drivers, &type_id), &driver_value)
                    .cloned()
            };
            scoped.or_else(|| {
                // 回退全量目录（兼容完整名 / 手输值）时同样要求驱动启用——
                // 否则可能拿一个被禁用的驱动落库，后续连接必然失败。
                let enabled: Vec<Driver> = drivers.iter().filter(|d| d.enabled).cloned().collect();
                find_driver_by_value(&enabled, &driver_value).cloned()
            })
        };
        let db_type = selected_driver
            .as_ref()
            .map(|d| d.id.clone())
            .unwrap_or_else(|| driver_value.to_string());
        if db_type.is_empty() {
            return None;
        }
        // 文件型（SQLite / DuckDB）没有凭据 / 网络 / TLS 语义：表单里从上个类型残留的用户名、
        // 密码、认证引用、SSL 覆盖一律不落库（避免“文件库带着 mysql 凭据”这类脏数据）。
        let is_file = crate::services::data_source_service::is_file_db_driver(&db_type);
        let name = name.read(cx).value().to_string();
        let url = url.read(cx).value().to_string();
        if name.trim().is_empty() || url.trim().is_empty() {
            return None;
        }
        let username = {
            let v = user.read(cx).value().to_string();
            if v.trim().is_empty() {
                None
            } else {
                Some(v.trim().to_string())
            }
        };
        let password = {
            let v = pass.read(cx).value().to_string();
            if v.is_empty() { None } else { Some(v) }
        };
        let auth_config_id = self
            .auth_ref
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|n| {
                self.auth_list
                    .borrow()
                    .iter()
                    .find(|a| a.name.as_deref() == Some(n.as_str()))
                    .map(|a| a.id.clone())
            });
        let network_config_id = self
            .network_ref
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|n| {
                self.network_list
                    .borrow()
                    .iter()
                    .find(|c| c.name.as_deref() == Some(n.as_str()))
                    .map(|c| c.id.clone())
            });
        let environment_id = self.env.read(cx).selected_value().cloned().and_then(|n| {
            self.env_list
                .borrow()
                .iter()
                .find(|e| e.name == n.as_str())
                .map(|e| e.id.clone())
        });
        let driver_properties = if self.props.borrow().is_empty() {
            None
        } else {
            let map: std::collections::BTreeMap<String, String> =
                self.props.borrow().iter().cloned().collect();
            serde_json::to_string(&map).ok()
        };
        // DuckDB 缓存路径 → metadata_path 落库（非空时）。
        let metadata_path = {
            let v = self.cache_path.read(cx).value().to_string();
            if v.trim().is_empty() {
                None
            } else {
                Some(v.trim().to_string())
            }
        };
        // 作用域（UI 标签 → ConnectionScope）。
        let scope = scope_from_label(
            self.scope
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .as_str(),
        );
        // 高级选项组装：ssl + policy_overrides 合并为一个 JSON。
        let mut adv = serde_json::Map::new();
        let ssl_mode = self
            .ssl_mode
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        // SSL 覆盖仅当驱动声明 ssl 时落库（卡片同规则显示；上个驱动残留的模式不写入）。
        let driver_declares_ssl = selected_driver
            .as_ref()
            .map(|d| {
                driver_auth_types(d.supported_auth_types.as_deref())
                    .iter()
                    .any(|m| m == "ssl")
            })
            .unwrap_or(false);
        if driver_declares_ssl && !ssl_mode.is_empty() && ssl_mode != "disable" {
            let mut ssl = serde_json::Map::new();
            ssl.insert("mode".into(), serde_json::json!(ssl_mode));
            let ca = self.ssl_ca.read(cx).value().to_string();
            if !ca.trim().is_empty() {
                ssl.insert("ca".into(), serde_json::json!(ca.trim()));
            }
            let cert = self.ssl_cert.read(cx).value().to_string();
            if !cert.trim().is_empty() {
                ssl.insert("cert".into(), serde_json::json!(cert.trim()));
            }
            let key = self.ssl_key.read(cx).value().to_string();
            if !key.trim().is_empty() {
                ssl.insert("key".into(), serde_json::json!(key.trim()));
            }
            adv.insert("ssl".into(), serde_json::Value::Object(ssl));
        }
        // 策略覆盖：直接落已勾选的策略类型（environment_policies.policy_type）。
        let overrides: Vec<serde_json::Value> = self
            .policy_override_keys
            .borrow()
            .iter()
            .map(|k| serde_json::json!(k))
            .collect();
        if !overrides.is_empty() {
            adv.insert(
                "policy_overrides".into(),
                serde_json::Value::Array(overrides),
            );
        }
        let advanced_options = if adv.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(adv).to_string())
        };
        let mut out = DataSourceSaveInput {
            name: name.trim().to_string(),
            db_type: db_type.to_string(),
            url: url.trim().to_string(),
            username,
            password,
            scope,
            description: {
                let r = self.remark.read(cx).value().to_string();
                if r.trim().is_empty() {
                    None
                } else {
                    Some(r.trim().to_string())
                }
            },
            tags: tags_to_json(&self.tags_input.read(cx).value().to_string()),
            driver_id: selected_driver.as_ref().map(|d| d.id.clone()),
            environment_id,
            auth_config_id,
            auth_method: {
                let v = self
                    .auth_method
                    .read(cx)
                    .selected_value()
                    .cloned()
                    .unwrap_or_default()
                    .to_string();
                if v.trim().is_empty() { None } else { Some(v) }
            },
            network_config_id,
            driver_properties,
            advanced_options,
            options: None,
            use_duckdb_fed: Some(self.duckdb_fed.get()),
            // P0-4（2026-10-24）：表单「保存凭据」开关——关 = 密码不落库
            // （对齐 DBeaver `setSavePassword(false)`）。
            save_password: Some(self.save_credentials.get()),
            schema_name: None,
            metadata_path,
        };
        // 文件型（SQLite / DuckDB）没有凭据 / 网络 / TLS 语义：表单里从上个类型残留的值
        // 一律不落库（避免“文件库带着 mysql 凭据”这类脏数据）。
        if is_file {
            strip_file_db_noise(&mut out);
        }
        Some(out)
    }

    /// 草稿快照 → 保存输入（「保存并退出」批量提交用；与 [`Self::collect`] 同一口径，
    /// 但字段全部来自暂存条目而非表单——批量提交时条目不在光标位，不能读表单控件）。
    ///
    /// 返回 `None` 表示条目不完整（缺驱动 / 名称 / 地址），调用方据 `ConnectionDraft` 字段
    /// 具体提示缺什么。引用类字段（认证 / 网络档案 / 环境）存的是**名字**，在此解析为 ID。
    pub(crate) fn draft_to_save_input(&self, d: &ConnectionDraft) -> Option<DataSourceSaveInput> {
        // 与 collect 同一校验：驱动 id / 名称 / 地址任一为空即不可提交。
        if d.driver_id.is_empty() || d.name.trim().is_empty() || d.url.trim().is_empty() {
            return None;
        }
        let is_file = crate::services::data_source_service::is_file_db_driver(&d.driver_id);
        let username = if d.user.trim().is_empty() {
            None
        } else {
            Some(d.user.trim().to_string())
        };
        let password = if d.pass.is_empty() { None } else { Some(d.pass.clone()) };
        // 引用档案：名字 → ID（列表在对话框内已加载；批量提交在后台只带解析后的 ID 过线）。
        let auth_config_id = d.auth_ref.as_deref().and_then(|n| {
            self.auth_list
                .borrow()
                .iter()
                .find(|a| a.name.as_deref() == Some(n))
                .map(|a| a.id.clone())
        });
        let network_config_id = d.network_ref.as_deref().and_then(|n| {
            self.network_list
                .borrow()
                .iter()
                .find(|c| c.name.as_deref() == Some(n))
                .map(|c| c.id.clone())
        });
        let environment_id = d.env.as_deref().and_then(|n| {
            self.env_list
                .borrow()
                .iter()
                .find(|e| e.name == n)
                .map(|e| e.id.clone())
        });
        let driver_properties = if d.props.is_empty() {
            None
        } else {
            let map: std::collections::BTreeMap<String, String> =
                d.props.iter().cloned().collect();
            serde_json::to_string(&map).ok()
        };
        // 高级选项组装（与 collect 同规则）：SSL 覆盖仅当驱动声明 ssl 时落库；策略覆盖落已勾选的类型。
        let mut adv = serde_json::Map::new();
        let driver_declares_ssl = self
            .drivers
            .borrow()
            .iter()
            .find(|dr| dr.id == d.driver_id)
            .map(|dr| {
                driver_auth_types(dr.supported_auth_types.as_deref())
                    .iter()
                    .any(|m| m == "ssl")
            })
            .unwrap_or(false);
        if driver_declares_ssl && !d.ssl_mode.is_empty() && d.ssl_mode != "disable" {
            let mut ssl = serde_json::Map::new();
            ssl.insert("mode".into(), serde_json::json!(d.ssl_mode));
            if !d.ssl_ca.trim().is_empty() {
                ssl.insert("ca".into(), serde_json::json!(d.ssl_ca.trim()));
            }
            if !d.ssl_cert.trim().is_empty() {
                ssl.insert("cert".into(), serde_json::json!(d.ssl_cert.trim()));
            }
            if !d.ssl_key.trim().is_empty() {
                ssl.insert("key".into(), serde_json::json!(d.ssl_key.trim()));
            }
            adv.insert("ssl".into(), serde_json::Value::Object(ssl));
        }
        let overrides: Vec<serde_json::Value> = d
            .sec_overrides
            .iter()
            .map(|k| serde_json::json!(k))
            .collect();
        if !overrides.is_empty() {
            adv.insert(
                "policy_overrides".into(),
                serde_json::Value::Array(overrides),
            );
        }
        let advanced_options = if adv.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(adv).to_string())
        };
        let mut out = DataSourceSaveInput {
            name: d.name.trim().to_string(),
            db_type: d.driver_id.clone(),
            url: d.url.trim().to_string(),
            username,
            password,
            scope: scope_from_label(&d.scope),
            description: if d.remark.trim().is_empty() {
                None
            } else {
                Some(d.remark.trim().to_string())
            },
            tags: tags_to_json(&d.tags),
            driver_id: Some(d.driver_id.clone()),
            environment_id,
            auth_config_id,
            auth_method: if d.auth_method.trim().is_empty() {
                None
            } else {
                Some(d.auth_method.clone())
            },
            network_config_id,
            driver_properties,
            advanced_options,
            options: None,
            use_duckdb_fed: Some(d.duckdb_fed),
            save_password: Some(d.save_credentials),
            schema_name: None,
            metadata_path: if d.cache_path.trim().is_empty() {
                None
            } else {
                Some(d.cache_path.trim().to_string())
            },
        };
        if is_file {
            strip_file_db_noise(&mut out);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    // 不通配导入（`super::*` 会把 gpui 的 `test` 宏带入作用域）；需用到的类型显式引入。
    use super::{
        AuthConfig, ConnectionDialogState, ConnectionDraft, ConnectionScope, Driver,
        Environment, NetworkConfig, ResultLevel,
    };
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::OnceLock;

    use engine::migration::install_global_db_manager;
    use engine::persistence::global_db::GlobalDatabaseManager;
    use gpui_kit::{
        IntoElement, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
    };

    struct Host;
    impl Render for Host {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            div().size_full()
        }
    }

    fn auth_cfg(id: &str, name: &str) -> AuthConfig {
        AuthConfig {
            id: id.into(),
            name: Some(name.into()),
            auth_type: "password".into(),
            auth_data: "{}".into(),
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    fn net_cfg(id: &str, name: &str) -> NetworkConfig {
        NetworkConfig {
            id: id.into(),
            name: Some(name.into()),
            network_type: "ssh".into(),
            config: "{}".into(),
            auth_config_id: None,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    fn env_cfg(id: &str, name: &str) -> Environment {
        Environment {
            id: id.into(),
            name: name.into(),
            description: None,
            color: None,
            sort_order: 0,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "t".into(),
        }
    }

    /// 构造对话框 + 注入引用列表 / 驱动目录，返回 `ClonedDialogState` 的等价句柄。
    fn dialog(cx: &mut VisualTestContext) -> super::ClonedDialogState {
        let dialog = cx.update(|window, cx| ConnectionDialogState::<Host>::new(window, cx));
        cx.update(|_, _cx| {
            *dialog.auth_list.borrow_mut() = vec![auth_cfg("auth-1", "跳板机凭据")];
            *dialog.network_list.borrow_mut() = vec![net_cfg("net-1", "跳板")];
            *dialog.env_list.borrow_mut() = vec![env_cfg("env-1", "prod")];
            *dialog.drivers.borrow_mut() = vec![
                // 网络型（声明 ssl）：postgres
                Driver {
                    id: "postgres".into(),
                    type_id: "postgres".into(),
                    name: "PostgreSQL".into(),
                    driver_kind: "network".into(),
                    is_file: false,
                    default_port: Some(5432),
                    url_template: None,
                    download_url: None,
                    download_checksum: None,
                    version: None,
                    config_schema: "{}".into(),
                    supported_auth_types: Some(r#"["password","ssl"]"#.into()),
                    capabilities: None,
                    driver_properties: None,
                    enabled: true,
                },
                // 文件型：sqlite
                Driver {
                    id: "sqlite".into(),
                    type_id: "sqlite".into(),
                    name: "SQLite".into(),
                    driver_kind: "file".into(),
                    is_file: true,
                    default_port: None,
                    url_template: None,
                    download_url: None,
                    download_checksum: None,
                    version: None,
                    config_schema: "{}".into(),
                    supported_auth_types: None,
                    capabilities: None,
                    driver_properties: None,
                    enabled: true,
                },
            ];
        });
        super::ConnectionDialogState::<Host>::cloned_state(
            dialog.name.clone(),
            dialog.url.clone(),
            dialog.user.clone(),
            dialog.pass.clone(),
            dialog.driver.clone(),
            dialog.remark.clone(),
            dialog.auth_method.clone(),
            dialog.auth_ref.clone(),
            dialog.network_ref.clone(),
            dialog.env.clone(),
            dialog.auth_list.clone(),
            dialog.network_list.clone(),
            dialog.env_list.clone(),
            dialog.duckdb_fed.clone(),
            dialog.save_credentials.clone(),
            dialog.cache_path.clone(),
            dialog.props.clone(),
            dialog.scope.clone(),
            dialog.ssl_mode.clone(),
            dialog.ssl_ca.clone(),
            dialog.ssl_cert.clone(),
            dialog.ssl_key.clone(),
            dialog.policy_override_keys.clone(),
            dialog.drivers.clone(),
            dialog.selected_type.clone(),
            dialog.tags_input.clone(),
        )
    }

    /// 草稿快照 → 保存输入：网络型（postgres）全字段映射 + 作用域标签转换 + 引用名→ID。
    #[gpui_kit::test]
    fn draft_to_save_input_maps_snapshot_fields(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _cx| Host);
        let cx: &mut VisualTestContext = cx;
        let cl = dialog(cx);

        let draft = ConnectionDraft {
            name: "生产 PG".into(),
            driver_id: "postgres".into(),
            driver_name: "postgres".into(),
            url: "postgres://10.0.0.5:5432/prod".into(),
            user: "app".into(),
            pass: "s3cret".into(),
            remark: " 主库 ".into(),
            tags: "prod, core".into(),
            scope: "仅项目".into(),
            project_path: "/data/a1".into(),
            ssl_mode: "verify-full".into(),
            ssl_ca: "/ca.pem".into(),
            cache_path: "/cache".into(),
            props: vec![("sslmode".into(), "require".into())],
            auth_method: "password".into(),
            auth_ref: Some("跳板机凭据".into()),
            network_ref: Some("跳板".into()),
            env: Some("prod".into()),
            groups: vec!["g1".into()],
            ..ConnectionDraft::empty()
        };

        let input = cl.draft_to_save_input(&draft).expect("应可提交");
        assert_eq!(input.name, "生产 PG");
        assert_eq!(input.db_type, "postgres");
        assert_eq!(input.url, "postgres://10.0.0.5:5432/prod");
        assert_eq!(input.username.as_deref(), Some("app"));
        assert_eq!(input.password.as_deref(), Some("s3cret"));
        assert_eq!(input.scope, ConnectionScope::Project, "「仅项目」→ Project");
        assert_eq!(input.auth_config_id.as_deref(), Some("auth-1"), "认证档案名→ID");
        assert_eq!(input.network_config_id.as_deref(), Some("net-1"), "网络档案名→ID");
        assert_eq!(input.environment_id.as_deref(), Some("env-1"), "环境名→ID");
        assert_eq!(input.description.as_deref(), Some("主库"), "remark 应 trim");
        assert_eq!(input.driver_id.as_deref(), Some("postgres"));
        assert_eq!(input.auth_method.as_deref(), Some("password"));
        assert_eq!(input.metadata_path.as_deref(), Some("/cache"));
        assert_eq!(input.use_duckdb_fed, Some(true), "草稿 duckdb_fed 默认 true");
        assert_eq!(input.save_password, Some(true), "草稿 save_credentials 默认 true（P0-4）");
        // ssl_mode=verify-full（驱动声明 ssl）→ advanced_options 含 ssl.mode。
        let adv = input.advanced_options.as_deref().unwrap_or_default();
        assert!(adv.contains("\"ssl\"") && adv.contains("verify-full"), "{adv}");
        assert!(!adv.contains("policy_overrides"), "无策略覆盖时不写");
        // P0-4（2026-10-24）：关「保存凭据」→ save_password=false（密码不落库）。
        let mut draft_off = draft.clone();
        draft_off.save_credentials = false;
        let input_off = cl.draft_to_save_input(&draft_off).expect("应可提交");
        assert_eq!(input_off.save_password, Some(false), "关开关应不落库");
    }

    /// 文件型（sqlite）：残留凭据 / 引用一律清噪，不落库。
    #[gpui_kit::test]
    fn draft_to_save_input_strips_file_db_noise(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _cx| Host);
        let cx: &mut VisualTestContext = cx;
        let cl = dialog(cx);

        // 模拟「上个类型残留」：sqlite 草稿带着 mysql 凭据与网络/SSL 引用。
        let draft = ConnectionDraft {
            name: "本地库".into(),
            driver_id: "sqlite".into(),
            url: "sqlite:///tmp/a.db".into(),
            user: "root".into(),
            pass: "pw".into(),
            scope: "全局".into(),
            ssl_mode: "verify-full".into(),
            ssl_ca: "/ca.pem".into(),
            auth_ref: Some("跳板机凭据".into()),
            network_ref: Some("跳板".into()),
            auth_method: "password".into(),
            ..ConnectionDraft::empty()
        };

        let input = cl.draft_to_save_input(&draft).expect("应可提交");
        assert_eq!(input.db_type, "sqlite");
        assert!(input.username.is_none(), "文件型不应落凭据用户名");
        assert!(input.password.is_none(), "文件型不应落密码");
        assert!(input.auth_config_id.is_none(), "文件型不应落认证引用");
        assert!(input.network_config_id.is_none(), "文件型不应落网络引用");
        assert!(input.auth_method.is_none(), "文件型不应落认证方法");
        assert!(
            input.advanced_options.as_deref().unwrap_or_default().is_empty()
                || !input.advanced_options.as_deref().unwrap_or_default().contains("\"ssl\""),
            "文件型不应组装 SSL 高级选项"
        );
    }

    /// 不完整草稿（缺驱动 / 名称 / 地址任一）→ `None`，批量提交据此中止并提示。
    #[gpui_kit::test]
    fn draft_to_save_input_rejects_incomplete(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _cx| Host);
        let cx: &mut VisualTestContext = cx;
        let cl = dialog(cx);

        assert!(
            cl.draft_to_save_input(&ConnectionDraft {
                name: "有名字".into(),
                ..ConnectionDraft::empty()
            })
            .is_none(),
            "缺驱动应拒绝"
        );
        assert!(
            cl.draft_to_save_input(&ConnectionDraft {
                driver_id: "postgres".into(),
                url: "postgres://x".into(),
                ..ConnectionDraft::empty()
            })
            .is_none(),
            "缺名称应拒绝"
        );
        assert!(
            cl.draft_to_save_input(&ConnectionDraft {
                name: "x".into(),
                driver_id: "postgres".into(),
                ..ConnectionDraft::empty()
            })
            .is_none(),
            "缺地址应拒绝"
        );
    }

    /// 测试专用临时全局库单例（进程内一次，与 workbench 集成测试同一注入模式）。
    static TEST_BASE: OnceLock<PathBuf> = OnceLock::new();
    fn test_base_dir() -> PathBuf {
        TEST_BASE
            .get_or_init(|| {
                let dir = std::env::temp_dir().join(format!(
                    "rds_conn_dlg_test_{}",
                    std::process::id()
                ));
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).expect("create temp dir");
                let rt = tokio::runtime::Runtime::new().expect("runtime");
                let manager = rt
                    .block_on(GlobalDatabaseManager::new(
                        dir.join("global.db"),
                        dir.join("analytics.duckdb"),
                        2,
                    ))
                    .expect("init global db");
                // 已有注入（其他测试）时忽略：全局单例进程内唯一。
                let _ = install_global_db_manager(manager);
                std::mem::forget(rt);
                dir
            })
            .clone()
    }

    /// 新建分组（#7 重建验证）：非空名 → 服务层建组 → 目录重载 + 输入清空 + 成功提示。
    /// 分组为项目级能力：先建项目根（`.RSmeta`）并把项目路径写进输入。
    #[gpui_kit::test]
    fn submit_group_new_creates_group_via_service(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let base = test_base_dir();
        let project_root = base.join("proj-groups");
        std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
        let root_str = project_root.to_string_lossy().to_string();

        let (_, cx) = cx.add_window_view(|_, _cx| Host);
        let cx: &mut VisualTestContext = cx;
        let dialog = cx.update(|window, cx| {
            let d = Rc::new(ConnectionDialogState::<Host>::new(window, cx));
            d.project_path
                .update(cx, |s, cx| s.set_value(root_str.clone(), window, cx));
            d
        });
        cx.update(|window, cx| {
            dialog
                .group_new_name
                .update(cx, |s, cx| s.set_value("测试分组".to_string(), window, cx));
            dialog.submit_group_new(window, cx);
        });
        let (has, cleared, success) = cx.update(|_, cx| {
            let has = dialog.groups.borrow().iter().any(|g| g.name == "测试分组");
            let cleared = dialog.group_new_name.read(cx).value().is_empty();
            let success = dialog
                .result
                .borrow()
                .as_ref()
                .map(|r| r.level) == Some(ResultLevel::Success);
            (has, cleared, success)
        });
        assert!(has, "新分组应出现在目录");
        assert!(cleared, "输入应清空");
        assert!(success, "结果行应为成功级");
    }

    /// 空名早退：不建组、不改目录（名称输入为空时直接返回）。
    #[gpui_kit::test]
    fn submit_group_new_empty_name_is_noop(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _cx| Host);
        let cx: &mut VisualTestContext = cx;
        let dialog =
            cx.update(|window, cx| Rc::new(ConnectionDialogState::<Host>::new(window, cx)));
        cx.update(|window, cx| {
            dialog.submit_group_new(window, cx);
        });
        assert!(dialog.groups.borrow().is_empty(), "空名不应建组");
    }

    /// 策略异步刷新的空环境早退（#6 重建验证）：空名 → 清空清单 + 记录 `loaded_for`，不发查询。
    #[gpui_kit::test]
    fn refresh_env_policies_async_empty_name_clears(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (host_entity, cx) = cx.add_window_view(|_, _cx| Host);
        let cx: &mut VisualTestContext = cx;
        let dialog =
            cx.update(|window, cx| Rc::new(ConnectionDialogState::<Host>::new(window, cx)));
        // 预置一条清单，验证空环境会清空（而不是保留旧策略）。
        cx.update(|_, _cx| {
            *dialog.env_policies.borrow_mut() = vec![("x".into(), "y".into(), "z".into())];
        });
        cx.update(|window, cx| {
            dialog.refresh_env_policies_async("", &host_entity, window, cx);
        });
        let (cleared, marked) = cx.update(|_, _cx| {
            let cleared = dialog.env_policies.borrow().is_empty();
            let marked = dialog.env_policies_loaded_for.borrow().as_deref() == Some("");
            (cleared, marked)
        });
        assert!(cleared, "空环境应清空清单");
        assert!(marked, "应记录已加载环境（避免渲染期重复触发）");
    }
}

/// 写回文件选择结果（打开 / 新建共用）：有值时更新地址输入；所有结局都写结果行并通知宿主重渲。
///
/// `cx` 为窗口异步上下文（`App::prompt_for_paths` / `prompt_for_new_path` 的 oneshot 回传约定），
/// `set_value` 需要窗口句柄，故统一在 `cx.update` 内完成。
fn apply_file_pick<H: 'static>(
    target: &Entity<InputState>,
    result: &Rc<RefCell<Option<ResultLine>>>,
    value: Option<String>,
    message: String,
    ok: bool,
    entity: &Entity<H>,
    cx: &mut gpui_kit::AsyncWindowContext,
) {
    let _ = cx.update(|window, cx| {
        if let Some(value) = value {
            target.update(cx, |s, cx| s.set_value(value, window, cx));
        }
        set_result_ok(result, ok, message);
        entity.update(cx, |_, cx| cx.notify());
    });
}
