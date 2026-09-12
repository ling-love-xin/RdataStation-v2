use super::*;
use crate::services::data_source_service::ReferenceField;

/// 网络配置字段行的标签列宽（容纳「校验服务器证书」这类长标签）。
/// 结构尺寸最终应迁移到 `ui.rs` 登记（与对话框的 `DIALOG_*` 同路）。
const NET_LABEL_W: f32 = 7.;

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
fn meta_field_specs_now(mgr: &ManagerWorkspace, kind: usize, type_key: &str) -> Vec<NetFieldSpec> {
    let cache_key = format!("{kind}:{}", type_key.trim().to_ascii_lowercase());
    if mgr.net_specs_for.borrow().as_str() != cache_key {
        *mgr.net_specs.borrow_mut() = match kind {
            0 => auth_field_specs(type_key),
            1 => network_field_specs(type_key),
            _ => Vec::new(),
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

/// 按名称读取网络配置（类型, config JSON）；服务未就绪 / 不存在 → None。
fn load_network_config_by_name(name: &str, cx: &mut App) -> Option<(String, String)> {
    let rt = tokio::runtime::Runtime::new().ok()?;
    let service = DataSourceService::global().ok()?;
    let list = rt.block_on(service.list_network_configs()).ok()?;
    let _ = cx;
    list.into_iter()
        .find(|n| n.name.as_deref() == Some(name))
        .map(|n| (n.network_type, n.config))
}

/// 按名称读取认证配置（类型, **解密后**的 auth_data JSON）；服务未就绪 / 不存在 / 解密失败 → None。
///
/// 走服务层专用接口：列表接口出于脱敏不返回 `auth_data` 明文。
fn load_auth_config_by_name(name: &str, cx: &mut App) -> Option<(String, String)> {
    let rt = tokio::runtime::Runtime::new().ok()?;
    let service = DataSourceService::global().ok()?;
    let _ = cx;
    rt.block_on(service.auth_config_detail_by_name(name))
        .ok()
        .flatten()
}

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

    // 当前项目根：引用计数与删除守卫需要它才能看到 P_/GP_ 连接（未打开项目时仅全局）。
    let project_root = shared
        .project
        .borrow()
        .as_ref()
        .map(|p| p.root.to_string_lossy().to_string());

    // 拉取列表（按 kind）。
    {
        let mut m = mgr.borrow_mut();
        m.kind = kind;
        m.editing = None;
        m.msg = None;
        m.items.clear();
        m.project_root = project_root;
        m.policy_env = None;
        m.policy_editing = None;
        m.policies.borrow_mut().clear();
    }
    refresh_manager_items(kind, &mgr, cx);

    // 类型下拉按管理器类别填充（认证 / 网络各自的**规范键**）；环境管理器不用类型列。
    // 此前网络管理器沿用认证选项（同一个 SelectState 实体）→ 落库 `network_type` 是
    // `proxy_pwd` 这类错值，解析器永远认不出（审计 #20 第三层根因）。
    {
        let type_items: Vec<SharedString> = match kind {
            0 => AUTH_TYPES.iter().map(|t| SharedString::from(*t)).collect(),
            1 => NETWORK_TYPES.iter().map(|t| SharedString::from(*t)).collect(),
            _ => Vec::new(),
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

    window.open_dialog(cx, move |dialog, window, cx| {
        // 网络字段占位随类型同步：必须在 `cx.theme()` 之前写（占位写入需要 `&mut cx`，
        // 而 theme 持有 `cx` 的不可变借用）；只在类型变化时写，`set_placeholder` 会 notify。
        if kind == 1 {
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
                .child(div().text_sm().flex_1().child(item.name.clone()));
            // 被引用计数（原型 §3.6）：被引用时删除会被拦下，先在这里告知数量。
            if item.refs > 0 {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("被引用 {}", item.refs)),
                );
            }
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
                                let mut m = mgr.borrow_mut();
                                m.policy_env = Some(item.name.clone());
                                m.policy_editing = None;
                                m.policy_enabled.set(true);
                                drop(m);
                                refresh_policy_items(&item.name, &mgr, app);
                                let mut m = mgr.borrow_mut();
                                m.msg = Some(format!("环境「{}」策略管理", item.name));
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
                                    let item_name = item.name.clone();
                                    let mut m = mgr.borrow_mut();
                                    m.editing = Some(item_name.clone());
                                    m.msg = Some(format!("编辑中：{item_name}（保存后更新既有条目）"));
                                    m.new_name.update(app, |s, cx| s.set_value(item_name.clone(), window, cx));
                                    // 认证（0）与网络（1）：回填真实字段（此前只回填名称 →
                                    // 保存会把已有 config / auth_data 覆盖成空）。
                                    if kind <= 1 {
                                        let loaded = if kind == 0 {
                                            load_auth_config_by_name(&item_name, app)
                                        } else {
                                            load_network_config_by_name(&item_name, app)
                                        };
                                        if let Some((ty, config)) = loaded {
                                            m.new_data
                                                .update(app, |s, cx| s.set_value(config.clone(), window, cx));
                                            let values = if kind == 0 {
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
                                    let project_root = mgr.borrow().project_root.clone();
                                    let deleted = delete_manager_item(kind, &item.name, project_root.as_deref());
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
            // 网络配置字段（kind=1）：按类型显示子集；`chain`（或未选类型）回落到原始 JSON 文本框。
            let net_type_now = m
                .new_type
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string();
            // 认证（kind 0）与网络（kind 1）都按类型声明展开字段；无声明（如 chain）回落原始 JSON。
            let net_specs: Vec<NetFieldSpec> = if kind <= 1 {
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
            let entity = entity.clone();
            let mut type_row = div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().text_xs().child("类型"))
                .child(Select::new(&new_type).placeholder("选择类型…"));
            if net_rows.is_empty() {
                type_row = type_row
                    .child(div().text_xs().child("数据(JSON)"))
                    .child(Input::new(&new_data).w(rems(12.5)));
            }
            let mut fields_block = div().v_flex().gap_1();
            for (spec, input) in &net_rows {
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
                    .child(Input::new(input).w(rems(14.)));
                if spec.required {
                    row = row.child(div().text_xs().text_color(theme.colors.danger).child("*"));
                }
                fields_block = fields_block.child(row);
            }
            if !net_rows.is_empty() {
                fields_block = fields_block.child(hint_line(
                    theme,
                    "字段按类型展开；协议链请选 chain 并直接填 JSON。",
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
                .child(
                    div().text_sm().font_weight(FontWeight::BOLD)
                        .child(if editing.is_some() { "编辑配置" } else { "新建配置" }),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(div().text_xs().child("名称"))
                        .child(Input::new(&new_name).w(rems(11.25))),
                )
                .child(type_row)
                .child(fields_block)
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
                                            mgr.borrow_mut().msg = Some("名称不能为空".into());
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        // 认证 / 网络：有字段声明时由字段组装 JSON；校验失败只提示，不落库。
                                        // 无声明（如 chain / 未知认证类型）→ 继续用原始 JSON 文本框。
                                        let data = if kind <= 1 {
                                            let specs = meta_field_specs_now(&mgr.borrow(), kind, &tpe);
                                            if specs.is_empty() {
                                                raw_data
                                            } else {
                                                let values = collect_net_values(&mgr.borrow(), app);
                                                let built = if kind == 0 {
                                                    build_auth_config_json(&tpe, &values)
                                                } else {
                                                    build_network_config_json(&tpe, &values)
                                                };
                                                match built {
                                                    Ok(json) => json,
                                                    Err(e) => {
                                                        mgr.borrow_mut().msg = Some(e);
                                                        entity.update(app, |_, cx| cx.notify());
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
                                            m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                            m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                            clear_net_values(&m, window, app);
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
                                        clear_net_values(&m, window, app);
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

/// 刷新管理器列表（block_on stores + 引用计数）。
///
/// 引用范围：全局库全部连接 + 当前项目根（`mgr.project_root`；未打开项目时仅全局）。
/// 逐条扫库会变成 N × (全局 + 项目) 两次查询，所以用 `count_references_batch` 一次取齐。
pub(crate) fn refresh_manager_items(kind: usize, mgr: &Rc<RefCell<ManagerWorkspace>>, cx: &mut App) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(_) => return,
    };
    let project_root = mgr.borrow().project_root.clone();
    let field = match kind {
        0 => ReferenceField::AuthConfig,
        1 => ReferenceField::NetworkConfig,
        _ => ReferenceField::Environment,
    };

    let items: Vec<ManagerItem> = rt.block_on(async {
        // (id, 显示名)：id 用于引用计数，显示名用于列表。
        let rows: Vec<(String, String)> = match kind {
            0 => service
                .list_auth_configs()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter_map(|a| a.name.map(|n| (a.id, n)))
                .collect(),
            1 => service
                .list_network_configs()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter_map(|n| n.name.map(|name| (n.id, name)))
                .collect(),
            _ => service
                .list_environments()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|e| (e.id, e.name))
                .collect(),
        };
        let ids: Vec<String> = rows.iter().map(|(id, _)| id.clone()).collect();
        let counts = service
            .count_references_batch(field, &ids, project_root.as_deref())
            .await;
        rows.into_iter()
            .map(|(id, name)| ManagerItem {
                refs: counts.get(&id).map(|c| c.total()).unwrap_or(0),
                name,
            })
            .collect()
    });
    mgr.borrow_mut().items = items;
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

/// 删除配置（按名称查 ID → **引用守卫** → store delete）。
///
/// 原型 §3.6：「被引用的配置不可删除」——守卫在服务层（`ensure_no_references`），
/// 范围与列表计数一致（全局库 + 当前项目）。
pub(crate) fn delete_manager_item(kind: usize, name: &str, project_root: Option<&str>) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
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
        0 => ReferenceField::AuthConfig,
        1 => ReferenceField::NetworkConfig,
        _ => ReferenceField::Environment,
    };

    let outcome = rt.block_on(async {
        let id = match kind {
            0 => db
                .list_auth_configs(None)
                .await?
                .iter()
                .find(|a| a.name.as_deref() == Some(name))
                .map(|a| a.id.clone()),
            1 => db
                .list_network_configs(None)
                .await?
                .iter()
                .find(|n| n.name.as_deref() == Some(name))
                .map(|n| n.id.clone()),
            _ => db
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
            0 => db.delete_auth_config(&id).await,
            1 => db.delete_network_config(&id).await,
            _ => db.delete_environment(&id).await,
        }
    });

    match outcome {
        Ok(()) => format!("已删除「{name}」"),
        Err(e) => format!("删除失败：{e}"),
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
pub(crate) fn upsert_policy_item(env_name: &str, label: &str, enabled: bool, editing: Option<&str>) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
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
                if enabled { "启用" } else { "停用" }
            ),
            None => format!("已创建「{label}」空策略模板（策略类型 {ptype}）：具体配置项待策略编辑器实现"),
        },
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
