//! 浏览区行渲染：分组行、类型组行、生成器项行。

use super::*;

/// 业务分组行：展开/收起按钮 + 行尾「＋」（键盘可达，`Button`）。
pub(crate) fn render_group_row(
    state: &Entity<GeneratorLibraryState>,
    name: &str,
    expanded: bool,
    category: GeneratorCategory,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::Div {
    let state_for_toggle = state.clone();
    let name_for_toggle = name.to_string();
    let state_for_create = state.clone();
    let item_count = generator_catalog::all_specs()
        .iter()
        .filter(|spec| spec.category == category)
        .count()
        + state
            .read(cx)
            .panel
            .read(cx)
            .user_generators_in_category(category)
            .len();

    div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div().h_flex().items_center().flex_1().min_w_0().child(
                button(
                    format!("lib-group-{name}"),
                    format!("{name}（{item_count}）"),
                    ActionKind::Quiet,
                )
                .xsmall()
                .icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .toggled(expanded)
                .on_click(move |_event, _window, app| {
                    state_for_toggle.update(app, |s, cx| {
                        if !s.expanded.remove(&name_for_toggle) {
                            s.expanded.insert(name_for_toggle.clone());
                        }
                        cx.notify();
                    });
                }),
            ),
        )
        .child(
            icon_button(
                format!("lib-group-plus-{name}"),
                IconName::Plus,
                format!("在“{name}”分组新建生成器"),
            )
            .on_click(move |_event, window, app| {
                let (panel, writeback) = {
                    let s = state_for_create.read(app);
                    (s.panel.clone(), s.writeback.clone())
                };
                open_create_dialog(
                    panel,
                    writeback,
                    CreateSeed::New {
                        locked_type: None,
                        locked_category: Some(category),
                    },
                    window,
                    app,
                );
            }),
        )
}

/// 技术分组行：同业务分组，但展开集走类型、入口锁定类型。
pub(crate) fn render_type_group_row(
    state: &Entity<GeneratorLibraryState>,
    data_type: &ColumnDataType,
    title: &str,
    type_key: &str,
    expanded: bool,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::Div {
    let state_for_toggle = state.clone();
    let type_key_for_toggle = type_key.to_string();
    let state_for_create = state.clone();
    let data_type_for_create = data_type.clone();
    // 计数与展开后的实际项数同口径：只算方法级（业务生成器不在数据视角）
    let item_count = generator_catalog::all_specs()
        .iter()
        .filter(|spec| generator_catalog::is_method_spec(spec))
        .filter(|spec| data_type_for_spec(spec) == *data_type)
        .count()
        + state
            .read(cx)
            .panel
            .read(cx)
            .user_generators_in_type(data_type)
            .len();

    div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div().h_flex().items_center().flex_1().min_w_0().child(
                button(
                    format!("lib-type-group-{type_key}"),
                    format!("{title}（{item_count}）"),
                    ActionKind::Quiet,
                )
                .xsmall()
                .icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .toggled(expanded)
                .on_click(move |_event, _window, app| {
                    state_for_toggle.update(app, |s, cx| {
                        if !s.expanded_types.remove(&type_key_for_toggle) {
                            s.expanded_types.insert(type_key_for_toggle.clone());
                        }
                        cx.notify();
                    });
                }),
            ),
        )
        .child(
            icon_button(
                format!("lib-type-group-plus-{type_key}"),
                IconName::Plus,
                format!("在“{title}”数据类型下新建生成器"),
            )
            .on_click(move |_event, window, app| {
                let (panel, writeback) = {
                    let s = state_for_create.read(app);
                    (s.panel.clone(), s.writeback.clone())
                };
                open_create_dialog(
                    panel,
                    writeback,
                    CreateSeed::New {
                        locked_type: Some(data_type_for_create.clone()),
                        locked_category: None,
                    },
                    window,
                    app,
                );
            }),
        )
}

/// 生成器项行：名称 + 徽标（业务视角徽标 = 数据类；技术视角徽标 = 业务组）。
///
/// 点击写回表配置（内建 → 按 `name` 取默认配置，自定义 → 解包 `base`），**不关对话框**：
/// 右栏每帧按表配置重建，点一项就能就地看到它的属性 / 参数 / 样例。
///
/// `key` 是**写回用的稳定键**（内建 = `spec.name`，自定义 = `ug.id`）；`label` 只是给人看的
/// 中文名。两者不能混用：目录按 name 查、自定义按 id 查，拿 label 去查一律落空 →
/// 点列表什么都不会发生（这是真实发生过的 bug，见 `picked_config` 的兜底）。
/// 自定义项行尾另有「编辑」「删除」两个管理按钮（键盘可达，`Button`）。
/// 一行生成器说明（列表项 hover 预览）：业务组 · 产出类型 · 参数清单。
///
/// 目的：不必「点开才知道它是什么」。参数只列键名，完整默认值仍由右栏参数区给。
fn describe_spec(spec: &'static generator_catalog::GeneratorSpec) -> String {
    let output = column_type_label(&data_type_for_spec(spec));
    let params = if spec.params.is_empty() {
        "无参数".to_string()
    } else {
        spec.params
            .iter()
            .map(|field| field.key.to_string())
            .collect::<Vec<_>>()
            .join(" / ")
    };
    format!("{} · 产出 {output} · 参数：{params}", spec.category.label())
}

pub(crate) fn render_item_row(
    state: &Entity<GeneratorLibraryState>,
    key: &str,
    label: &str,
    badge: &str,
    is_user: bool,
    selected: bool,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let state_for_click = state.clone();
    let picked = key.to_string();
    // id 区分内建 / 自定义：用户起的名字可能正好等于某个内建生成器标签
    // （如「姓名」），同名同组会出现重复 ElementId（GPUI a11y 重复 id → panic）
    let item_id = if is_user {
        format!("lib-item-user-{key}")
    } else {
        format!("lib-item-{key}")
    };
    // hover 预览：内建按目录名取规格，自定义按 id 取 base 规格（删掉就退回没有提示）
    let hint = if is_user {
        let panel = state.read(cx).panel.clone();
        panel
            .read(cx)
            .user_generators()
            .iter()
            .find(|ug| ug.id == key)
            .map(|ug| {
                format!(
                    "自定义 · {}",
                    describe_spec(generator_catalog::spec_of(&ug.base))
                )
            })
    } else {
        generator_catalog::spec_by_name(key).map(describe_spec)
    };
    let mut item_button = button(item_id, label, ActionKind::Quiet)
        .xsmall()
        .toggled(selected)
        .on_click(move |_event, window, app| {
            let (panel, writeback) = {
                let s = state_for_click.read(app);
                (s.panel.clone(), s.writeback.clone())
            };
            apply_picked_generator(app, window, &panel, &writeback, &picked, is_user);
        });
    if let Some(hint) = hint {
        item_button = item_button.tooltip(hint).tooltip_show_delay(TOOLTIP_DELAY);
    }
    let mut row = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .px_1()
        .rounded_sm();
    // 选中态：把「这一列现在用的是哪一项」标出来（原先没有任何标记，点完看不出当前是哪个）
    if selected {
        row = row.bg(theme.colors.list_active);
    }
    row = row
        .child(
            div()
                .h_flex()
                .items_center()
                .flex_1()
                .min_w_0()
                .child(item_button),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(badge.to_string()),
        );
    // 自定义项：行尾「编辑」（打开精调层预填，含重命名）/「删除」（会话级移除）
    if is_user {
        let state_edit = state.clone();
        let ug_id_edit = key.to_string();
        row = row
            .child(
                button(format!("lib-item-edit-{key}"), "编辑", ActionKind::Quiet)
                    .xsmall()
                    .tooltip("重命名 / 修改参数")
                    .tooltip_show_delay(TOOLTIP_DELAY)
                    .on_click(move |_event, window, app| {
                        let (panel, writeback, ug) = {
                            let s = state_edit.read(app);
                            let ug = s
                                .panel
                                .read(app)
                                .user_generators()
                                .iter()
                                .find(|ug| ug.id == ug_id_edit)
                                .cloned();
                            (s.panel.clone(), s.writeback.clone(), ug)
                        };
                        let Some(ug) = ug else {
                            return;
                        };
                        open_create_dialog(panel, writeback, CreateSeed::Edit { ug }, window, app);
                    }),
            )
            .child({
                let state_del = state.clone();
                let ug_id_del = key.to_string();
                danger_icon_button(
                    format!("lib-item-delete-{key}"),
                    IconName::Delete,
                    "删除此自定义生成器",
                    theme,
                )
                .on_click(move |_event, _window, app| {
                    let s = state_del.read(app);
                    let panel = s.panel.clone();
                    let ug_id = ug_id_del.clone();
                    // 先删 panel 项，再刷新对话框列表（panel.notify 不驱动对话框实体重绘）
                    panel.update(app, |p, cx| p.delete_user_generator(&ug_id, cx));
                    state_del.update(app, |_, cx| cx.notify());
                })
            });
    }
    row.into_any_element()
}