use super::*;

pub(crate) fn open_manager(
    kind: usize,
    mgr: &Rc<RefCell<ManagerWorkspace>>,
    entity: Entity<crate::panels::EditorPanel>,
    shared: Shared,
    window: &mut Window,
    cx: &mut App,
) {
    let mgr = mgr.clone();
    let entity = entity.clone();
    let shared_for_layer = shared.clone();

    // 拉取列表（按 kind）。
    {
        let mut m = mgr.borrow_mut();
        m.kind = kind;
        m.editing = None;
        m.msg = None;
        m.items.clear();
        m.policy_env = None;
        m.policy_editing = None;
        m.policies.borrow_mut().clear();
    }
    refresh_manager_items(kind, &mgr, cx);

    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let shared_layer = shared_for_layer.clone();
        let m = mgr.borrow();
        let title = match kind {
            0 => "认证配置管理（AuthConfigManager）",
            1 => "网络配置管理（NetworkConfigManager）",
            _ => "环境管理（EnvironmentManager）",
        };

        // 列表行。
        let mut rows = div().v_flex().gap_1();
        if m.items.is_empty() {
            rows = rows.child(
                div().text_xs().text_color(theme.colors.muted_foreground).child("暂无配置，点击下方「新建」"),
            );
        }
        for (i, item) in m.items.iter().enumerate() {
            let item = item.clone();
            let idx = i;
            let mut row = div()
                .h_flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_2()
                .py_1()
                .child(div().text_sm().flex_1().child(item.clone()));
            if kind == 2 {
                row = row.child(
                    Button::new(format!("mgr-policy-{idx}"))
                        .secondary()
                        .label("策略")
                        .on_click({
                            let item = item.clone();
                            let mgr = mgr.clone();
                            let entity = entity.clone();
                            move |_, _window, app| {
                                {
                                    let mut m = mgr.borrow_mut();
                                    m.policy_env = Some(item.clone());
                                    m.policy_editing = None;
                                    m.policy_enabled.set(true);
                                }
                                refresh_policy_items(&item, &mgr, app);
                                let mut m = mgr.borrow_mut();
                                m.msg = Some(format!("环境「{item}」策略管理"));
                                drop(m);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }),
                );
            }
            rows = rows.child(row.child(
                Button::new(format!("mgr-edit-{idx}"))
                            .secondary()
                            .label("编辑")
                            .on_click({
                                let item = item.clone();
                                let mgr = mgr.clone();
                                let entity = entity.clone();
                                move |_, window, app| {
                                    let mut m = mgr.borrow_mut();
                                    m.editing = Some(item.clone());
                                    m.msg = Some(format!("编辑中：{item}（保存后更新既有条目）"));
                                    m.new_name.update(app, |s, cx| s.set_value(item.clone(), window, cx));
                                    drop(m);
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("mgr-del-{idx}"))
                            .secondary()
                            .label("删除")
                            .on_click({
                                let item = item.clone();
                                let mgr = mgr.clone();
                                let entity = entity.clone();
                                move |_, _window, app| {
                                    let deleted = delete_manager_item(kind, &item);
                                    let mut m = mgr.borrow_mut();
                                    m.msg = Some(deleted.clone());
                                    drop(m);
                                    if deleted.starts_with("已删除") || deleted.starts_with("删除成功") {
                                        refresh_manager_items(kind, &mgr, app);
                                    }
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            }),
                    ),
            );
        }
        drop(m);

        // 新建/编辑表单。
        let mgr_form = {
            let m = mgr.borrow();
            let new_name = m.new_name.clone();
            let new_type = m.new_type.clone();
            let new_data = m.new_data.clone();
            let editing = m.editing.clone();
            let msg = m.msg.clone();
            drop(m);
            let mgr = mgr.clone();
            let entity = entity.clone();
            div()
                .v_flex()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_3()
                .py_2()
                .child(
                    div().text_sm().font_weight(FontWeight::BOLD)
                        .child(if editing.is_some() { "编辑配置" } else { "新建配置" }),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(div().text_xs().child("名称"))
                        .child(Input::new(&new_name).w(rems(11.25))),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(div().text_xs().child("类型"))
                        .child(Select::new(&new_type).placeholder("选择类型…"))
                        .child(div().text_xs().child("数据(JSON)"))
                        .child(Input::new(&new_data).w(rems(12.5))),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(
                            Button::new("mgr-save")
                                .primary()
                                .label("保存")
                                .on_click({
                                    let mgr = mgr.clone();
                                    let entity = entity.clone();
                                    move |_, window, app| {
                                        let (name, tpe, data, editing) = {
                                            let m = mgr.borrow();
                                            (
                                                m.new_name.read(app).value().to_string(),
                                                m.new_type.read(app).selected_value().cloned().unwrap_or_default().to_string(),
                                                m.new_data.read(app).value().to_string(),
                                                m.editing.clone(),
                                            )
                                        };
                                        if name.trim().is_empty() {
                                            mgr.borrow_mut().msg = Some("名称不能为空".into());
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let outcome = upsert_manager_item(kind, &name, &tpe, &data, editing.as_deref());
                                        let mut m = mgr.borrow_mut();
                                        m.msg = Some(outcome.clone());
                                        if outcome.starts_with("已保存") || outcome.starts_with("保存成功") {
                                            m.editing = None;
                                            m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                            m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                        }
                                        drop(m);
                                        refresh_manager_items(kind, &mgr, app);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        )
                        .child(
                            Button::new("mgr-cancel-edit")
                                .label("取消编辑")
                                .on_click({
                                    let mgr = mgr.clone();
                                    let entity = entity.clone();
                                    move |_, window, app| {
                                        let mut m = mgr.borrow_mut();
                                        m.editing = None;
                                        m.msg = None;
                                        m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                        m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                        drop(m);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        ),
                )
                .child(
                    if let Some(msg) = msg {
                        div().text_xs().text_color(theme.colors.info).child(msg)
                    } else {
                        div().h(rems(1.125))
                    },
                )
        };

        // 策略管理面板（仅环境管理器且已点开某环境时显示）。
        let policy_panel = {
            if kind == 2 {
                let m = mgr.borrow();
                let policy_env = m.policy_env.clone();
                let policy_type = m.policy_type.clone();
                let policy_enabled = m.policy_enabled.clone();
                let policies = m.policies.clone();
                let policy_editing = m.policy_editing.clone();
                drop(m);
                match policy_env {
                    Some(env_name) => {
                        let mut plist = div().v_flex().gap_1();
                        let list = policies.borrow();
                        if list.is_empty() {
                            plist = plist.child(
                                div().text_xs().text_color(theme.colors.muted_foreground).child("该环境暂无策略"),
                            );
                        }
                        for (i, (ptype, pid, enabled)) in list.iter().enumerate() {
                            let (ptype, pid, enabled) = (ptype.clone(), pid.clone(), *enabled);
                            plist = plist.child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .px_2()
                                    .py_1()
                                    .child(div().text_sm().flex_1().child(ptype.clone()))
                                    .child(
                                        div().text_xs()
                                            .text_color(if enabled { theme.colors.success } else { theme.colors.muted_foreground })
                                            .child(if enabled { "启用" } else { "停用" }),
                                    )
                                    .child(
                                        Button::new(format!("pol-del-{i}"))
                                            .secondary()
                                            .label("删除")
                                            .on_click({
                                                let pid = pid.clone();
                                                let mgr = mgr.clone();
                                                let entity = entity.clone();
                                                move |_, _window, app| {
                                                    let outcome = delete_policy_item(&pid);
                                                    let env = mgr.borrow().policy_env.clone();
                                                    {
                                                        let mut m = mgr.borrow_mut();
                                                        m.msg = Some(outcome);
                                                    }
                                                    if let Some(e) = env {
                                                        refresh_policy_items(&e, &mgr, app);
                                                    }
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            }),
                                    ),
                            );
                        }
                        drop(list);
                        let env_name2 = env_name.clone();
                        let mgr2 = mgr.clone();
                        let entity2 = entity.clone();
                        div()
                            .v_flex()
                            .gap_2()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.colors.border)
                            .px_3()
                            .py_2()
                            .child(
                                div().text_sm().font_weight(FontWeight::BOLD)
                                    .child(format!("策略管理：{}", env_name.clone())),
                            )
                            .child(plist)
                            .child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_xs().child("类型"))
                                    .child(Select::new(&policy_type).placeholder("选择策略类型…"))
                                    .child(div().text_xs().child("启用"))
                                    .child(
                                        div()
                                            .id("pol-toggle")
                                            .cursor_pointer()
                                            .relative()
                                            .w(rems(1.875))
                                            .h_4()
                                            .rounded_full()
                                            .bg(if policy_enabled.get() { theme.colors.primary } else { theme.colors.border })
                                            .on_click({
                                                let policy_enabled = policy_enabled.clone();
                                                let entity = entity.clone();
                                                move |_, _, app| {
                                                    policy_enabled.set(!policy_enabled.get());
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            })
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .m_0p5()
                                                    .w_3()
                                                    .h_3()
                                                    .rounded_full()
                                                    .bg(theme.colors.background)
                                                    .child(""),
                                            ),
                                    )
                                    .child(
                                        Button::new("pol-save")
                                            .primary()
                                            .label(if policy_editing.is_some() { "更新策略" } else { "添加策略" })
                                            .on_click({
                                                let env_name = env_name2.clone();
                                                let mgr = mgr2.clone();
                                                let entity = entity2.clone();
                                                move |_, _window, app| {
                                                    let (label, enabled, editing) = {
                                                        let m = mgr.borrow();
                                                        (
                                                            m.policy_type.read(app).selected_value().cloned().unwrap_or_default().to_string(),
                                                            m.policy_enabled.get(),
                                                            m.policy_editing.clone(),
                                                        )
                                                    };
                                                    if label.is_empty() {
                                                        mgr.borrow_mut().msg = Some("请选择策略类型".into());
                                                        entity.update(app, |_, cx| cx.notify());
                                                        return;
                                                    }
                                                    let outcome = upsert_policy_item(&env_name, &label, enabled, editing.as_deref());
                                                    let ok = outcome.starts_with("已保存") || outcome.starts_with("保存成功");
                                                    let mut m = mgr.borrow_mut();
                                                    m.msg = Some(outcome);
                                                    if ok {
                                                        m.policy_editing = None;
                                                        m.policy_enabled.set(true);
                                                    }
                                                    drop(m);
                                                    refresh_policy_items(&env_name, &mgr, app);
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            }),
                                    ),
                            )
                    }
                    None => div().h_0(),
                }
            } else {
                div().h_0()
            }
        };

        dialog
            .title(title)
            .w(cx.theme().font_size * 35.)
            .overlay(true)
            .overlay_closable(true)
            .keyboard(true)
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(rows)
                    .child(policy_panel)
                    .child(mgr_form)
                    .child(
                        div().text_xs().text_color(theme.colors.muted_foreground)
                            .child(if kind == 0 {
                                "复用：数据库 A 可直接引用数据库 B 的认证配置；凭据 AES-256 加密存储，列表不回显明文。"
                            } else if kind == 1 {
                                "复用：网络配置（SSH/代理档案）可被多个连接引用，建一次全项目/全局复用。"
                            } else {
                                "复用：环境（含策略）可被多个数据源共享；点击「策略」直接管理该环境的策略。"
                            }),
                    ),
            )
            .footer(
                div().h_flex().justify_end().gap_2()
                    .child(
                        Button::new("mgr-close")
                            .label("关闭")
                            .on_click({
                                let shared = shared_layer.clone();
                                move |_, window, app| {
                                    window.close_dialog(app);
                                    // 宿主重绘：移除内层对话框
                                    shared.notify_host(app);
                                }
                            }),
                    ),
            )
            .on_close({
                let shared = shared_layer.clone();
                move |_, _, app| shared.notify_host(app)
            })
    });
    // 宿主重绘：内层（管理器）对话框由宿主 render 渲染，打开后需重建元素树。
    shared.notify_host(cx);
}

/// 刷新管理器列表（block_on stores）。
pub(crate) fn refresh_manager_items(kind: usize, mgr: &Rc<RefCell<ManagerWorkspace>>, cx: &mut App) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(_) => return,
    };
    let names: Vec<String> = match kind {
        0 => rt
            .block_on(service.list_auth_configs())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| a.name)
            .collect(),
        1 => rt
            .block_on(service.list_network_configs())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|n| n.name)
            .collect(),
        _ => rt
            .block_on(service.list_environments())
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.name)
            .collect(),
    };
    mgr.borrow_mut().items = names;
    let _ = cx;
}

/// 新建/更新配置（block_on store CRUD；auth_data 密文由 store 内部处理）。
pub(crate) fn upsert_manager_item(
    kind: usize,
    name: &str,
    tpe: &str,
    data: &str,
    editing: Option<&str>,
) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(e) => return format!("服务未就绪: {e}"),
    };
    let _ = service;
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };

    let id = match kind {
        0 => engine::persistence::id_prefix::generate_gid("auth", name),
        1 => engine::persistence::id_prefix::generate_gid("net", name),
        _ => engine::persistence::id_prefix::generate_gid("env", name),
    };

    let outcome = rt.block_on(async {
        match kind {
            0 => {
                let config = AuthConfig {
                    id,
                    name: Some(name.to_string()),
                    auth_type: if tpe.is_empty() {
                        "password".into()
                    } else {
                        tpe.into()
                    },
                    auth_data: if data.is_empty() {
                        "{}".into()
                    } else {
                        data.into()
                    },
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                };
                if editing.is_some() {
                    db.update_auth_config(&config).await
                } else {
                    db.create_auth_config(&config).await
                }
            }
            1 => {
                let config = NetworkConfig {
                    id,
                    name: Some(name.to_string()),
                    network_type: if tpe.is_empty() {
                        "SSH".into()
                    } else {
                        tpe.into()
                    },
                    config: if data.is_empty() {
                        "{}".into()
                    } else {
                        data.into()
                    },
                    auth_config_id: None,
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                };
                if editing.is_some() {
                    db.update_network_config(&config).await
                } else {
                    db.create_network_config(&config).await
                }
            }
            _ => {
                let env = Environment {
                    id,
                    name: name.to_string(),
                    description: Some(if data.is_empty() {
                        "新建环境".into()
                    } else {
                        data.into()
                    }),
                    color: None,
                    sort_order: 0,
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: String::new(),
                };
                if editing.is_some() {
                    db.update_environment(&env).await
                } else {
                    db.create_environment(&env).await
                }
            }
        }
    });

    match outcome {
        Ok(()) => "保存成功".to_string(),
        Err(e) => format!("保存失败: {e}"),
    }
}

/// 删除配置（按名称查 ID → store delete）。
pub(crate) fn delete_manager_item(kind: usize, name: &str) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(e) => return format!("服务未就绪: {e}"),
    };
    let _ = service;
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };

    let outcome = rt.block_on(async {
        match kind {
            0 => {
                let items = db.list_auth_configs(None).await?;
                let id = items
                    .iter()
                    .find(|a| a.name.as_deref() == Some(name))
                    .map(|a| a.id.clone());
                match id {
                    Some(id) => db.delete_auth_config(&id).await,
                    None => Ok(()), // 不存在视为已删除
                }
            }
            1 => {
                let items = db.list_network_configs(None).await?;
                let id = items
                    .iter()
                    .find(|n| n.name.as_deref() == Some(name))
                    .map(|n| n.id.clone());
                match id {
                    Some(id) => db.delete_network_config(&id).await,
                    None => Ok(()),
                }
            }
            _ => {
                let items = db.list_environments().await?;
                let id = items.iter().find(|e| e.name == name).map(|e| e.id.clone());
                match id {
                    Some(id) => db.delete_environment(&id).await,
                    None => Ok(()),
                }
            }
        }
    });

    match outcome {
        Ok(()) => format!("已删除「{name}」"),
        Err(e) => format!("删除失败: {e}"),
    }
}

/// 刷新指定环境的策略摘要（(类型标签, ID, 启用)）。
pub(crate) fn refresh_policy_items(env_name: &str, mgr: &Rc<RefCell<ManagerWorkspace>>, cx: &mut App) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else { return };
    let entries = rt
        .block_on(async {
            let envs = db.list_environments().await?;
            let Some(env) = envs.iter().find(|e| e.name == env_name) else {
                return Ok::<_, shared::error::CoreError>(Vec::new());
            };
            let ps = db.list_environment_policies(&env.id).await?;
            Ok::<_, shared::error::CoreError>(
                ps.iter()
                    .map(|p| {
                        let pos = POLICY_KEYS
                            .iter()
                            .position(|k| k == &p.policy_type)
                            .unwrap_or(0);
                        let label = POLICY_ITEMS
                            .get(pos)
                            .copied()
                            .unwrap_or(&p.policy_type)
                            .to_string();
                        (label, p.id.clone(), p.enabled)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    *mgr.borrow_mut().policies.borrow_mut() = entries;
    let _ = cx;
}

/// 新建/更新环境策略（按环境名定位 environment_id；类型标签 → POLICY_KEYS 落库）。
pub(crate) fn upsert_policy_item(env_name: &str, label: &str, enabled: bool, editing: Option<&str>) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };
    let pos = POLICY_ITEMS.iter().position(|p| *p == label).unwrap_or(0);
    let ptype = POLICY_KEYS
        .get(pos)
        .copied()
        .unwrap_or("read_only")
        .to_string();

    let outcome = rt.block_on(async {
        use shared::error::CommonError;
        let envs = db.list_environments().await?;
        let env_id = envs
            .iter()
            .find(|e| e.name == env_name)
            .map(|e| e.id.clone())
            .ok_or_else(|| {
                shared::error::CoreError::common(CommonError::General(format!(
                    "环境「{env_name}」不存在"
                )))
            })?;
        let id = match editing {
            Some(pid) => pid.to_string(),
            None => engine::persistence::id_prefix::generate_gid(
                "pol",
                &format!("{}_{}", env_name, ptype),
            ),
        };
        let policy = engine::persistence::env_store::EnvironmentPolicy {
            id,
            environment_id: env_id,
            policy_type: ptype,
            policy_config: None,
            enabled,
            created_at: String::new(),
        };
        if editing.is_some() {
            db.update_environment_policy(&policy).await
        } else {
            db.create_environment_policy(&policy).await
        }
    });

    match outcome {
        Ok(()) => format!(
            "保存成功：{label}（{}）",
            if enabled { "启用" } else { "停用" }
        ),
        Err(e) => format!("保存失败: {e}"),
    }
}

/// 删除环境策略。
pub(crate) fn delete_policy_item(id: &str) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };
    match rt.block_on(db.delete_environment_policy(id)) {
        Ok(()) => "已删除策略".to_string(),
        Err(e) => format!("删除失败: {e}"),
    }
}
