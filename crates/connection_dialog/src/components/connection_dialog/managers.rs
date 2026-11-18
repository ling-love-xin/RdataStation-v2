use super::*;
use crate::services::data_source_service::ReferenceField;
use shared::i18n::tr;

/// 网络配置字段行的标签列宽（容纳「校验服务器证书」这类长标签）。
/// 结构尺寸最终应迁移到 `ui.rs` 登记（与对话框的 `DIALOG_*` 同路）。
const NET_LABEL_W: f32 = 7.;

/// 域的中文名（列表标题 / 面包屑共用一处，避免两处各写一份）。
fn manager_kind_label(kind: ManagerKind) -> String {
    match kind {
        ManagerKind::Auth => tr("认证档案").to_string(),
        ManagerKind::Network => tr("网络代理").to_string(),
        ManagerKind::Env => tr("环境策略").to_string(),
    }
}

/// 列表搜索判据：空词恒真，否则按名称**不区分大小写**包含匹配。
///
/// 内部自己归一（调用方传原始词即可）：列表是过滤后的视图，判据写错会表现为
/// 「搜了却没结果 / 搜了什么都在」，而这种错在窗口测试里很难一眼看出。
fn item_matches_filter(name: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || name.to_lowercase().contains(&q)
}

/// 列表可见判据 = 名称匹配 ∧ 类型筛选（`None` = 全部）。
fn item_visible(item: &ManagerItem, query: &str, type_filter: Option<&str>) -> bool {
    item_matches_filter(&item.name, query) && type_filter.map_or(true, |t| item.tpe == t)
}

/// 取字段输入实体（键不存在 → None）。
fn net_input_entity(mgr: &ManagerWorkspace, key: &str) -> Option<Entity<InputState>> {
    mgr.net_inputs
        .borrow()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, e)| e.clone())
}

/// 当前元数据类型的字段声明（按 `(kind, 类型)` 缓存，只在变化时重算）。
///
/// 认证（kind 0）与网络（kind 1）共用同一批输入实体，所以缓存键必须带 kind 前缀，
/// 避免两个管理器互相作废 / 错用对方的声明。
fn meta_field_specs_now(
    mgr: &ManagerWorkspace,
    kind: ManagerKind,
    type_key: &str,
) -> Vec<NetFieldSpec> {
    let cache_key = format!("{}:{}", kind.index(), type_key.trim().to_ascii_lowercase());
    if mgr.net_specs_for.borrow().as_str() != cache_key {
        *mgr.net_specs.borrow_mut() = match kind {
            ManagerKind::Auth => auth_field_specs(type_key),
            ManagerKind::Network => network_field_specs(type_key),
            ManagerKind::Env => Vec::new(),
        };
        *mgr.net_specs_for.borrow_mut() = cache_key;
    }
    mgr.net_specs.borrow().clone()
}

/// 读取全部字段值（key → 文本）。
fn collect_net_values(mgr: &ManagerWorkspace, cx: &App) -> Vec<(String, String)> {
    mgr.net_inputs
        .borrow()
        .iter()
        .map(|(k, e)| ((*k).to_string(), e.read(cx).value().to_string()))
        .collect()
}

/// 清空字段输入（保存成功 / 取消编辑）。
fn clear_net_values(mgr: &ManagerWorkspace, window: &mut Window, cx: &mut App) {
    for (_, input) in mgr.net_inputs.borrow().iter() {
        input.update(cx, |s, cx| s.set_value("", window, cx));
    }
}

/// 按名称读取网络配置（类型, **解密后**的 config JSON）；服务未就绪 / 不存在 → None。
///
/// 走服务层专用接口：列表接口在服务层已脱敏（`config` 置空）。
fn load_network_config_by_name(name: &str, cx: &mut App) -> Option<(String, String)> {
    let rt = crate::services::rt::runtime()?;
    let service = DataSourceService::global().ok()?;
    let _ = cx;
    rt.block_on(service.network_config_detail_by_name(name))
        .ok()
        .flatten()
}

/// 按名称读取认证配置（类型, **解密后**的 auth_data JSON）；服务未就绪 / 不存在 / 解密失败 → None。
///
/// 走服务层专用接口：列表接口出于脱敏不返回 `auth_data` 明文。
fn load_auth_config_by_name(name: &str, cx: &mut App) -> Option<(String, String)> {
    let rt = crate::services::rt::runtime()?;
    let service = DataSourceService::global().ok()?;
    let _ = cx;
    rt.block_on(service.auth_config_detail_by_name(name))
        .ok()
        .flatten()
}

/// 切到某个配置域：重置编辑态 + 拉列表 + 重建类型下拉。
///
/// 旧弹窗打开时调一次；配置中心页在左侧导航切换域时调同一份（否则页内的类型下拉
/// 会残留在上一个域的值，落库时把认证键写进 `network_type`）。
pub(crate) fn prepare_manager_domain(
    kind: ManagerKind,
    mgr: &Rc<RefCell<ManagerWorkspace>>,
    project_root: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    // 拉取列表（按 kind）。
    {
        let filter = mgr.borrow().filter_text.clone();
        let mut m = mgr.borrow_mut();
        m.kind = kind;
        m.editing = None;
        // 切域一律回到列表态：上一个域停在「编辑某条」时，切过来不该直接看到半张表单。
        m.form_open = false;
        m.type_filter = None;
        m.msg = None;
        m.items.clear();
        m.project_root = project_root;
        m.policy_env = None;
        m.policy_editing = None;
        m.policies.borrow_mut().clear();
        drop(m);
        // 搜索词跟着域走（切到网络域还留着认证域的关键词 = 看起来「一条都没有」）。
        filter.update(cx, |s, cx| s.set_value("", window, cx));
    }
    refresh_manager_items(kind, mgr, cx);

    // 类型下拉按管理器类别填充（认证 / 网络各自的**规范键**）；环境管理器不用类型列。
    // 此前网络管理器沿用认证选项（同一个 SelectState 实体）→ 落库 `network_type` 是
    // `proxy_pwd` 这类错值，解析器永远认不出（审计 #20 第三层根因）。
    let type_items: Vec<SharedString> = match kind {
        ManagerKind::Auth => AUTH_TYPES.iter().map(|t| SharedString::from(*t)).collect(),
        ManagerKind::Network => NETWORK_TYPES
            .iter()
            .map(|t| SharedString::from(*t))
            .collect(),
        ManagerKind::Env => Vec::new(),
    };
    let type_sel = mgr.borrow().new_type.clone();
    type_sel.update(cx, |s, cx| {
        s.set_items(SearchableVec::new(type_items.clone()), window, cx);
        // 旧选中值不在新选项里 → 改选首项（`set_items` 不会自动清理 selection）。
        let cur = s.selected_value().cloned().unwrap_or_default().to_string();
        if !type_items.iter().any(|t| t.as_ref() == cur) {
            match type_items.first() {
                Some(first) => s.set_selected_value(first, window, cx),
                None => s.set_selected_index(None, window, cx),
            }
        }
    });
}

/// 刷新管理器列表（block_on stores + 引用计数）。
///
/// 引用范围：全局库全部连接 + 当前项目根（`mgr.project_root`；未打开项目时仅全局）。
/// 逐条扫库会变成 N × (全局 + 项目) 两次查询，所以用 `count_references_batch` 一次取齐。
pub(crate) fn refresh_manager_items(
    kind: ManagerKind,
    mgr: &Rc<RefCell<ManagerWorkspace>>,
    cx: &mut App,
) {
    let Some(rt) = crate::services::rt::runtime() else {
        return;
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(_) => return,
    };
    let project_root = mgr.borrow().project_root.clone();
    let field = match kind {
        ManagerKind::Auth => ReferenceField::AuthConfig,
        ManagerKind::Network => ReferenceField::NetworkConfig,
        ManagerKind::Env => ReferenceField::Environment,
    };

    let items: Vec<ManagerItem> = rt.block_on(async {
        // (id, 显示名, 类型键)：id 用于引用计数，显示名与类型键用于列表 / 类型筛选。
        let rows: Vec<(String, String, String)> = match kind {
            ManagerKind::Auth => service
                .list_auth_configs()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter_map(|a| a.name.map(|n| (a.id, n, a.auth_type)))
                .collect(),
            ManagerKind::Network => service
                .list_network_configs()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter_map(|n| n.name.map(|name| (n.id, name, n.network_type)))
                .collect(),
            ManagerKind::Env => service
                .list_environments()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|e| (e.id, e.name, String::new()))
                .collect(),
        };
        let ids: Vec<String> = rows.iter().map(|(id, _, _)| id.clone()).collect();
        let counts = service
            .count_references_batch(field, &ids, project_root.as_deref())
            .await;
        rows.into_iter()
            .map(|(id, name, tpe)| ManagerItem {
                refs: counts.get(&id).map(|c| c.total()).unwrap_or(0),
                name,
                tpe,
            })
            .collect()
    });
    mgr.borrow_mut().items = items;
    let _ = cx;
}

/// 新建/更新配置（block_on store CRUD；auth_data 密文由 store 内部处理）。
pub(crate) fn upsert_manager_item(
    kind: ManagerKind,
    name: &str,
    tpe: &str,
    data: &str,
    editing: Option<&str>,
) -> String {
    let Some(rt) = crate::services::rt::runtime() else {
        return crate::services::rt::RT_UNAVAILABLE.to_string();
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
        ManagerKind::Auth => engine::persistence::id_prefix::generate_gid("auth", name),
        ManagerKind::Network => engine::persistence::id_prefix::generate_gid("net", name),
        ManagerKind::Env => engine::persistence::id_prefix::generate_gid("env", name),
    };

    let outcome = rt.block_on(async {
        match kind {
            ManagerKind::Auth => {
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
            ManagerKind::Network => {
                let config = NetworkConfig {
                    id,
                    name: Some(name.to_string()),
                    network_type: {
                        // 只接受规范键（防「下拉残留认证值」写进库）；空 / 未知 → 默认 ssh。
                        let t = tpe.trim().to_ascii_lowercase();
                        if NETWORK_TYPES.contains(&t.as_str()) {
                            t
                        } else {
                            "ssh".to_string()
                        }
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
            ManagerKind::Env => {
                let env = Environment {
                    id,
                    name: name.to_string(),
                    description: Some(if data.is_empty() {
                        tr("新建环境").into()
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

/// 删除配置（按名称查 ID → **引用守卫** → store delete）。
///
/// 原型 §3.6：「被引用的配置不可删除」——守卫在服务层（`ensure_no_references`），
/// 范围与列表计数一致（全局库 + 当前项目）。
pub(crate) fn delete_manager_item(
    kind: ManagerKind,
    name: &str,
    project_root: Option<&str>,
) -> String {
    let Some(rt) = crate::services::rt::runtime() else {
        return crate::services::rt::RT_UNAVAILABLE.to_string();
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(e) => return format!("服务未就绪: {e}"),
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };
    let field = match kind {
        ManagerKind::Auth => ReferenceField::AuthConfig,
        ManagerKind::Network => ReferenceField::NetworkConfig,
        ManagerKind::Env => ReferenceField::Environment,
    };

    let outcome = rt.block_on(async {
        let id = match kind {
            ManagerKind::Auth => db
                .list_auth_configs(None)
                .await?
                .iter()
                .find(|a| a.name.as_deref() == Some(name))
                .map(|a| a.id.clone()),
            ManagerKind::Network => db
                .list_network_configs(None)
                .await?
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .map(|n| n.id.clone()),
            ManagerKind::Env => db
                .list_environments()
                .await?
                .iter()
                .find(|e| e.name == name)
                .map(|e| e.id.clone()),
        };
        let Some(id) = id else {
            return Ok(()); // 不存在视为已删除
        };
        // 引用守卫：被连接引用的档案不允许删除（消息含引用数与范围）。
        service
            .ensure_no_references(field, &id, name, project_root)
            .await?;
        match kind {
            ManagerKind::Auth => db.delete_auth_config(&id).await,
            ManagerKind::Network => db.delete_network_config(&id).await,
            ManagerKind::Env => db.delete_environment(&id).await,
        }
    });

    match outcome {
        Ok(()) => format!("已删除「{name}」"),
        Err(e) => format!("删除失败：{e}"),
    }
}

/// 刷新指定环境的策略摘要（(类型标签, ID, 启用)）。
pub(crate) fn refresh_policy_items(
    env_name: &str,
    mgr: &Rc<RefCell<ManagerWorkspace>>,
    cx: &mut App,
) {
    let Some(rt) = crate::services::rt::runtime() else {
        return;
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
                        // 直接按库里的 `policy_type` 取标签（旧实现把它当 POLICY_KEYS 下标 →
                        // `position()` 永远 None，所有策略都被错标成「只读连接」）。
                        let label = policy_type_label(&p.policy_type);
                        (label, p.id.clone(), p.enabled)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    *mgr.borrow_mut().policies.borrow_mut() = entries;
    let _ = cx;
}

/// 新建/更新环境策略（按环境名定位 environment_id；中文标签 → `policy_type` 落库）。
///
/// 注意：此处只能新建「策略类型的空模板」（`policy_config = NULL`）——具体配置项编辑待环境策略
/// 编辑器实现；结果行会明确提示，**不替库编造配置值**。
pub(crate) fn upsert_policy_item(
    env_name: &str,
    label: &str,
    enabled: bool,
    editing: Option<&str>,
) -> String {
    let Some(rt) = crate::services::rt::runtime() else {
        return crate::services::rt::RT_UNAVAILABLE.to_string();
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };
    // 标签 → 策略类型（security / schema / performance / audit / ui；未知标签原样写回）。
    let ptype = policy_type_from_label(label.trim());

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
            policy_type: ptype.clone(),
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
        Ok(()) => match editing {
            Some(_) => format!(
                "保存成功：{label}（{}）",
                if enabled { tr("启用") } else { tr("停用") }
            ),
            None => format!(
                "已创建「{label}」空策略模板（策略类型 {ptype}）：具体配置项待策略编辑器实现"
            ),
        },
        Err(e) => format!("保存失败: {e}"),
    }
}

/// 删除环境策略。
pub(crate) fn delete_policy_item(id: &str) -> String {
    let Some(rt) = crate::services::rt::runtime() else {
        return crate::services::rt::RT_UNAVAILABLE.to_string();
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

/// 管理器列表 + 表单 UI（旧弹窗与配置中心页共用的一份实现）。
///
/// `notify` 是「重绘宿主」的回调：弹窗传 `Entity<H>` 的 update，配置中心页传自身 `cx.notify`。
/// 抽出的动机见 `docs/architecture/connection/prototypes/config-center.html`：
/// 三域（认证 / 网络 / 环境）在页里要能并排切换，不能只存在于内层对话框闭包里。
pub(crate) fn manager_panel_ui(
    kind: ManagerKind,
    mgr: &Rc<RefCell<ManagerWorkspace>>,
    notify: &Rc<dyn Fn(&mut App)>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let mgr = mgr.clone();
    let notify = notify.clone();
    // 网络字段占位随类型同步：必须在 `cx.theme()` 之前写（占位写入需要 `&mut cx`，
    // 而 theme 持有 `cx` 的不可变借用）；只在类型变化时写，`set_placeholder` 会 notify。
    if kind.is_network() {
            let m0 = mgr.borrow();
            let ty = m0
                .new_type
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string();
            if m0.net_specs_for.borrow().as_str() != ty {
                for spec in network_field_specs(&ty) {
                    if let Some(input) = net_input_entity(&m0, spec.key) {
                        let ph = spec.placeholder;
                        input.update(cx, |s, cx| s.set_placeholder(ph, window, cx));
                    }
                }
            }
        }
        let theme = cx.theme();
        let m = mgr.borrow();
        let form_open = m.form_open;
        // 搜索 + 类型筛选：只过滤显示（`items` / 引用计数 / 删除仍走全量）。
        let raw_query = m.filter_text.read(cx).value().trim().to_string();
        let type_filter = m.type_filter.clone();
        let visible_items: Vec<ManagerItem> = m
            .items
            .iter()
            .filter(|it| item_visible(it, &raw_query, type_filter.as_deref()))
            .cloned()
            .collect();

        // 列表行（表单打开时这段仍需构建判据，但容器里不挂——见函数末尾的组装）。
        let mut rows = div().v_flex().gap_1();
        if !form_open {
            let mgr_new = mgr.clone();
            let notify_new = notify.clone();
            let count = if raw_query.is_empty() {
                format!("{}（{} 条）", manager_kind_label(kind), m.items.len())
            } else {
                format!(
                    "{}（{} / {} 条）",
                    manager_kind_label(kind),
                    visible_items.len(),
                    m.items.len()
                )
            };
            rows = rows.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .pb_1()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .child(count),
                    )
                    .child(
                        Input::new(&m.filter_text)
                            .w(cx.theme().font_size * crate::ui::DIALOG_NAME_INPUT_WIDTH),
                    )
                    .child(
                        Button::new("mgr-new")
                            .primary()
                            .label(tr("新建"))
                            .on_click(move |_, window, app| {
                                let mut m = mgr_new.borrow_mut();
                                m.form_open = true;
                                m.editing = None;
                                m.msg = None;
                                m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                clear_net_values(&m, window, app);
                                drop(m);
                                notify_new(app);
                            }),
                    ),
            );
            // 类型筛选 chips（只在有类型列的域出现）：再点一次当前类型 = 回到「全部」。
            // 放在空态之前：筛出 0 条时仍然看得到 chips，否则用户没法把筛选撤回来。
            if kind.has_type_column() && !m.items.is_empty() {
                let mut types: Vec<String> = Vec::new();
                for it in &m.items {
                    if !it.tpe.is_empty() && !types.iter().any(|t| t == &it.tpe) {
                        types.push(it.tpe.clone());
                    }
                }
                let current = m.type_filter.clone();
                let mut chips = div().h_flex().items_center().gap_1().pb_1();
                let mgr_all = mgr.clone();
                let notify_all = notify.clone();
                let mut all_btn = Button::new("mgr-type-all").label(tr("全部"));
                all_btn = if current.is_none() {
                    all_btn.secondary()
                } else {
                    all_btn.ghost()
                };
                chips = chips.child(all_btn.on_click(move |_, _window, app| {
                    mgr_all.borrow_mut().type_filter = None;
                    notify_all(app);
                }));
                for (ti, tpe) in types.iter().enumerate() {
                    let mgr_chip = mgr.clone();
                    let notify_chip = notify.clone();
                    let active = current.as_deref() == Some(tpe.as_str());
                    let tpe_for = tpe.clone();
                    let mut chip = Button::new(format!("mgr-type-{ti}")).label(tpe.clone());
                    chip = if active { chip.secondary() } else { chip.ghost() };
                    chips = chips.child(chip.on_click(move |_, _window, app| {
                        let mut m = mgr_chip.borrow_mut();
                        m.type_filter = if active { None } else { Some(tpe_for.clone()) };
                        drop(m);
                        notify_chip(app);
                    }));
                }
                rows = rows.child(chips);
            }
        }
        if visible_items.is_empty() {
            let text = if !raw_query.is_empty() {
                format!("没搜到「{}」", raw_query)
            } else if type_filter.is_some() {
                tr("该类型下暂无配置（点「全部」撤销筛选）").to_string()
            } else {
                tr("暂无配置，点右上「新建」").to_string()
            };
            rows = rows.child(
                div().text_xs().text_color(theme.colors.muted_foreground).child(text),
            );
        }
        for (i, item) in visible_items.into_iter().enumerate() {
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
                .child(div().text_sm().flex_1().child(item.name.clone()));
            // 类型列（认证 / 网络有类型键；环境域列表不显示假类型）。
            if !item.tpe.is_empty() {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(item.tpe.clone()),
                );
            }
            // 被引用计数（原型 §3.6）：被引用时删除会被拦下，先在这里告知数量。
            if item.refs > 0 {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("被引用 {}", item.refs)),
                );
            }
            if kind.is_env() {
                row = row.child(
                    Button::new(format!("mgr-policy-{idx}"))
                        .secondary()
                        .label(tr("策略"))
                        .on_click({
                            let item = item.clone();
                            let mgr = mgr.clone();
                            let notify = notify.clone();
                            move |_, _window, app| {
                                let mut m = mgr.borrow_mut();
                                m.policy_env = Some(item.name.clone());
                                m.policy_editing = None;
                                m.policy_enabled.set(true);
                                // 策略面板挂在列表态：先把表单收起来，避免两态互相压。
                                m.form_open = false;
                                drop(m);
                                refresh_policy_items(&item.name, &mgr, app);
                                let mut m = mgr.borrow_mut();
                                m.msg = Some(format!("环境「{}」策略管理", item.name));
                                drop(m);
                                notify(app);
                            }
                        }),
                );
            }
            rows = rows.child(row.child(
                Button::new(format!("mgr-edit-{idx}"))
                            .secondary()
                            .label(tr("编辑"))
                            .on_click({
                                let item = item.clone();
                                let mgr = mgr.clone();
                                let notify = notify.clone();
                                move |_, window, app| {
                                    let item_name = item.name.clone();
                                    let mut m = mgr.borrow_mut();
                                    m.editing = Some(item_name.clone());
                                    m.form_open = true;
                                    m.msg = Some(format!("编辑中：{item_name}（保存后更新既有条目）"));
                                    m.new_name.update(app, |s, cx| s.set_value(item_name.clone(), window, cx));
                                    // 认证与网络：回填真实字段（此前只回填名称 →
                                    // 保存会把已有 config / auth_data 覆盖成空）。
                                    if kind.has_type_column() {
                                        let loaded = if kind.is_auth() {
                                            load_auth_config_by_name(&item_name, app)
                                        } else {
                                            load_network_config_by_name(&item_name, app)
                                        };
                                        if let Some((ty, config)) = loaded {
                                            m.new_data
                                                .update(app, |s, cx| s.set_value(config.clone(), window, cx));
                                            let values = if kind.is_auth() {
                                                auth_config_values(&ty, &config)
                                            } else {
                                                network_config_values(&ty, &config)
                                            };
                                            for (key, value) in values {
                                                if let Some(input) = net_input_entity(&m, &key) {
                                                    input.update(app, |s, cx| {
                                                        s.set_value(value.clone(), window, cx)
                                                    });
                                                }
                                            }
                                            m.new_type.update(app, |s, cx| {
                                                s.set_selected_value(&SharedString::from(ty.clone()), window, cx)
                                            });
                                            // 类型可能已变：立即按新类型重算字段声明（渲染期只读缓存）。
                                            let _ = meta_field_specs_now(&m, kind, &ty);
                                        }
                                    }
                                    drop(m);
                                    notify(app);
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("mgr-del-{idx}"))
                            .secondary()
                            .label(tr("删除"))
                            .on_click({
                                let item = item.clone();
                                let mgr = mgr.clone();
                                let notify = notify.clone();
                                move |_, _window, app| {
                                    let project_root = mgr.borrow().project_root.clone();
                                    let deleted = delete_manager_item(kind, &item.name, project_root.as_deref());
                                    let mut m = mgr.borrow_mut();
                                    m.msg = Some(deleted.clone());
                                    drop(m);
                                    if deleted.starts_with("已删除") || deleted.starts_with("删除成功") {
                                        refresh_manager_items(kind, &mgr, app);
                                    }
                                    notify(app);
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
            // 网络配置字段（Network 域）：按类型显示子集；`chain`（或未选类型）回落到原始 JSON 文本框。
            let net_type_now = m
                .new_type
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string();
            // 认证与网络都按类型声明展开字段；无声明（如 chain）回落原始 JSON。
            let net_specs: Vec<NetFieldSpec> = if kind.has_type_column() {
                meta_field_specs_now(&m, kind, &net_type_now)
            } else {
                Vec::new()
            };
            let net_rows: Vec<(NetFieldSpec, Entity<InputState>)> = net_specs
                .iter()
                .cloned()
                .filter_map(|spec| net_input_entity(&m, spec.key).map(|input| (spec, input)))
                .collect();
            drop(m);
            let mgr = mgr.clone();
            let notify = notify.clone();
            let mut type_row = div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().text_xs().child(tr("类型")))
                // 编辑态类型只读（原型口径）：类型决定字段集与落库列，改它等于换一条档案；
                // 想换类型就新建一条（`Select::disabled` 关掉展开，值仍照常显示）。
                .child(
                    Select::new(&new_type)
                        .disabled(editing.is_some())
                        .placeholder(tr("选择类型…")),
                );
            if editing.is_some() && kind.has_type_column() {
                type_row = type_row.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(tr("类型创建后不可修改；要换类型请新建一条")),
                );
            }
            if net_rows.is_empty() {
                type_row = type_row
                    .child(div().text_xs().child(tr("数据(JSON)")))
                    .child(Input::new(&new_data).w(cx.theme().font_size * crate::ui::DIALOG_JSON_INPUT_WIDTH));
            }
            let mut fields_block = div().v_flex().gap_1();
            for (spec, input) in &net_rows {
                // 凭据行（password / passphrase）：掩码显示 + 眼睛按钮（默认不肩窥也能确认自己敲了没）。
                let mut field = Input::new(input);
                if is_secret_field(spec.key) {
                    field = field.mask_toggle();
                }
                let mut row = div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(rems(NET_LABEL_W))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(spec.label),
                    )
                    .child(field.w(rems(14.)));
                if spec.required {
                    row = row.child(div().text_xs().text_color(theme.colors.danger).child("*"));
                }
                fields_block = fields_block.child(row);
            }
            if !net_rows.is_empty() {
                fields_block = fields_block.child(hint_line(
                    theme,
                    tr("字段按类型展开；协议链请选 chain 并直接填 JSON。"),
                ));
            }
            div()
                .v_flex()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_3()
                .py_2()
                .child({
                    let mgr_back = mgr.clone();
                    let notify_back = notify.clone();
                    let title = match &editing {
                        Some(name) => format!("{} / 编辑：{}", manager_kind_label(kind), name),
                        None => format!("{} / 新建", manager_kind_label(kind)),
                    };
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("mgr-back")
                                .secondary()
                                .label(tr("‹ 返回列表"))
                                .on_click(move |_, window, app| {
                                    let mut m = mgr_back.borrow_mut();
                                    m.form_open = false;
                                    m.editing = None;
                                    m.msg = None;
                                    m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                    m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                    clear_net_values(&m, window, app);
                                    drop(m);
                                    notify_back(app);
                                }),
                        )
                        .child(div().text_sm().font_weight(FontWeight::BOLD).child(title))
                })
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(div().text_xs().child(tr("名称")))
                        .child(Input::new(&new_name).w(cx.theme().font_size * crate::ui::DIALOG_NAME_INPUT_WIDTH)),
                )
                .child(type_row)
                .child(fields_block)
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(
                            Button::new("mgr-save")
                                .primary()
                                .label(tr("保存"))
                                .on_click({
                                    let mgr = mgr.clone();
                                    let notify = notify.clone();
                                    move |_, window, app| {
                                        let (name, tpe, raw_data, editing) = {
                                            let m = mgr.borrow();
                                            (
                                                m.new_name.read(app).value().to_string(),
                                                m.new_type.read(app).selected_value().cloned().unwrap_or_default().to_string(),
                                                m.new_data.read(app).value().to_string(),
                                                m.editing.clone(),
                                            )
                                        };
                                        if name.trim().is_empty() {
                                            mgr.borrow_mut().msg = Some(tr("名称不能为空").into());
                                            notify(app);
                                            return;
                                        }
                                        // 认证 / 网络：有字段声明时由字段组装 JSON；校验失败只提示，不落库。
                                        // 无声明（如 chain / 未知认证类型）→ 继续用原始 JSON 文本框。
                                        let data = if kind.has_type_column() {
                                            let specs = meta_field_specs_now(&mgr.borrow(), kind, &tpe);
                                            if specs.is_empty() {
                                                raw_data
                                            } else {
                                                let values = collect_net_values(&mgr.borrow(), app);
                                                let built = if kind.is_auth() {
                                                    build_auth_config_json(&tpe, &values)
                                                } else {
                                                    build_network_config_json(&tpe, &values)
                                                };
                                                match built {
                                                    Ok(json) => json,
                                                    Err(e) => {
                                                        mgr.borrow_mut().msg = Some(e);
                                                        notify(app);
                                                        return;
                                                    }
                                                }
                                            }
                                        } else {
                                            raw_data
                                        };
                                        let outcome = upsert_manager_item(kind, &name, &tpe, &data, editing.as_deref());
                                        let mut m = mgr.borrow_mut();
                                        m.msg = Some(outcome.clone());
                                        if outcome.starts_with("已保存") || outcome.starts_with("保存成功") {
                                            m.editing = None;
                                            m.form_open = false;
                                            m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                            m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                            clear_net_values(&m, window, app);
                                        }
                                        drop(m);
                                        refresh_manager_items(kind, &mgr, app);
                                        notify(app);
                                    }
                                }),
                        )
                        .child(
                            Button::new("mgr-cancel-edit")
                                .label(tr("取消编辑"))
                                .on_click({
                                    let mgr = mgr.clone();
                                    let notify = notify.clone();
                                    move |_, window, app| {
                                        let mut m = mgr.borrow_mut();
                                        m.editing = None;
                                        m.form_open = false;
                                        m.msg = None;
                                        m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                        m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                        clear_net_values(&m, window, app);
                                        drop(m);
                                        notify(app);
                                    }
                                }),
                        ),
                )
                .child(
                    if let Some(msg) = msg {
                        div().text_xs().text_color(theme.colors.info).child(msg)
                    } else {
                        div().h(rems(crate::ui::DIALOG_HINT_PLACEHOLDER_HEIGHT))
                    },
                )
        };

        // 策略管理面板（仅环境管理器且已点开某环境时显示）。
        let policy_panel = {
            if kind.is_env() {
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
                                div().text_xs().text_color(theme.colors.muted_foreground).child(tr("该环境暂无策略")),
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
                                            .child(if enabled { tr("启用") } else { tr("停用") }),
                                    )
                                    .child(
                                        Button::new(format!("pol-del-{i}"))
                                            .secondary()
                                            .label(tr("删除"))
                                            .on_click({
                                                let pid = pid.clone();
                                                let mgr = mgr.clone();
                                                let notify = notify.clone();
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
                                                    notify(app);
                                                }
                                            }),
                                    ),
                            );
                        }
                        drop(list);
                        let env_name2 = env_name.clone();
                        let mgr2 = mgr.clone();
                        let notify2 = notify.clone();
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
                                    .child(div().text_xs().child(tr("类型")))
                                    .child(Select::new(&policy_type).placeholder(tr("选择策略类型…")))
                                    .child(div().text_xs().child(tr("启用")))
                                    .child(
                                        // 策略启用开关：gpui-component `Switch`（决策 #84；默认尺寸 36×20，
                                        // 替代早期自绘 30×16 轨道，对齐原型 HTML）。
                                        Switch::new("pol-toggle")
                                            .checked(policy_enabled.get())
                                            .on_click({
                                                let policy_enabled = policy_enabled.clone();
                                                let notify = notify.clone();
                                                move |want, _window, app| {
                                                    policy_enabled.set(*want);
                                                    notify(app);
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new("pol-save")
                                            .primary()
                                            .label(if policy_editing.is_some() { tr("更新策略") } else { tr("添加策略") })
                                            .on_click({
                                                let env_name = env_name2.clone();
                                                let mgr = mgr2.clone();
                                                let notify = notify2.clone();
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
                                                        mgr.borrow_mut().msg = Some(tr("请选择策略类型").into());
                                                        notify(app);
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
                                                    notify(app);
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

    // 两态互斥组装：列表态 = 列表 +（环境）策略面板 + 结果行；表单态 = 编辑栏 + 表单 + 结果行。
    let list_msg = mgr.borrow().msg.clone();
    let mut panel = div().v_flex().gap_3();
    if form_open {
        panel = panel.child(mgr_form);
    } else {
        panel = panel.child(rows).child(policy_panel);
        if let Some(msg) = list_msg {
            panel = panel.child(div().text_xs().text_color(theme.colors.info).child(msg));
        }
    }
    panel
        .child(
            div().text_xs().text_color(theme.colors.muted_foreground)
                .child(match kind {
                    ManagerKind::Auth => tr("复用：数据库 A 可直接引用数据库 B 的认证配置；凭据 AES-256 加密存储，列表不回显明文。"),
                    ManagerKind::Network => tr("复用：网络配置（SSH/代理档案）可被多个连接引用，建一次全项目/全局复用。"),
                    ManagerKind::Env => tr("复用：环境（含策略）可被多个数据源共享；点击「策略」直接管理该环境的策略。"),
                }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{item_matches_filter, item_visible};
    use crate::components::connection_dialog::ManagerItem;

    fn item(name: &str, tpe: &str) -> ManagerItem {
        ManagerItem {
            name: name.to_string(),
            tpe: tpe.to_string(),
            refs: 0,
        }
    }

    #[test]
    fn empty_query_matches_every_item() {
        assert!(item_matches_filter("生产库认证", ""));
        assert!(item_matches_filter("", ""));
    }

    #[test]
    fn matching_is_case_insensitive_substring() {
        assert!(item_matches_filter("Prod-Auth", "prod"));
        assert!(item_matches_filter("prod-auth", "AUTH"));
        // 中文按子串（不依赖分词）。
        assert!(item_matches_filter("跳板机-ssh", "跳板"));
    }

    #[test]
    fn non_matching_returns_false() {
        assert!(!item_matches_filter("Prod-Auth", "test"));
        assert!(!item_matches_filter("", "prod"));
    }

    #[test]
    fn type_filter_narrows_within_name_hits() {
        let a = item("prod-auth", "password");
        let b = item("prod-ssh", "ssh_key");
        // 无类型筛选：两条都命中。
        assert!(item_visible(&a, "prod", None) && item_visible(&b, "prod", None));
        // 选了类型：只剩该类型那条。
        assert!(item_visible(&a, "prod", Some("password")));
        assert!(!item_visible(&b, "prod", Some("password")));
        // 名称与类型同时不满足 → 不可见（两个条件是与，不是或）。
        assert!(!item_visible(&b, "test", Some("password")));
    }

    #[test]
    fn empty_type_key_never_matches_a_type_filter() {
        // 环境域条目 `tpe` 为空；若误把它算进任何类型筛选会「越筛越多」。
        let env = item("生产环境", "");
        assert!(!item_visible(&env, "", Some("password")));
        assert!(item_visible(&env, "", None));
    }
}
