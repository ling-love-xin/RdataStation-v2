//! 选中态计算：从状态里算出当前选中的生成器配置。

use super::*;

/// 完全相同；`picked_user` 才说明「用户点的是列表里的哪一行」。
fn current_picked(
    state: &Entity<GeneratorLibraryState>,
    cx: &App,
) -> (Option<GeneratorConfig>, Option<String>) {
    let writeback = state.read(cx).writeback.clone();
    let Some(detail) = writeback.detail.upgrade() else {
        return (None, None);
    };
    let view = detail.read(cx);
    match view.draft.as_ref() {
        Some(draft) => (
            Some(draft.def.generator.clone()),
            draft.picked_user().map(str::to_string),
        ),
        None => (None, None),
    }
}

pub(crate) fn render_browse_pane(
    state: &Entity<GeneratorLibraryState>,
    _theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    // `state` 转 owned：按钮闭包捕获要 `'static`，参数引用只活到本函数返回
    let state = state.clone();
    let s = state.read(cx);
    let view_index: usize = match s.view {
        LibraryView::Business => 0,
        LibraryView::Tech => 1,
    };
    // `s`（read guard）持有 cx 不可变借用；clone 出所需字段后 guard 在最后一次使用后自动释放
    let expanded: HashSet<String> = s.expanded.clone();
    let expanded_types: HashSet<String> = s.expanded_types.clone();
    let query = s.query.clone();
    // 选中态：当前这一列用的生成器（内建按目录名比；自定义按**点中的那一项** id 比）
    let (current, picked) = current_picked(&state, cx);

    let mut list = div().v_flex().gap_1().w_full();
    match view_index {
        0 => {
            for category in GeneratorCategory::ALL {
                let name = category.label();
                let is_expanded = !query.is_empty() || expanded.contains(name);
                // 标题行与子项列表必须**垂直**包一层：`render_group_row` 的根是 `h_flex`，
                // 直接 `.child()` 会把子项堆当成水平项塞进去——整行被撑到 N 行高，
                // 标题又 `items_center` 垂直居中，结果正好压在第 N/2 个子项上（图文重叠）。
                let mut group = div().v_flex().gap_1().w_full().child(render_group_row(
                    &state,
                    name,
                    is_expanded,
                    category,
                    window,
                    cx,
                ));
                if is_expanded {
                    // 每项：`(写回键, 显示名, 徽标, 是否自定义, 是否选中)`。写回键必须是目录/
                    // 自定义库里真正的主键（spec.name / ug.id），显示名只用来画按钮文案
                    let mut items: Vec<(String, String, String, bool, bool)> = Vec::new();
                    items.extend(
                        generator_catalog::all_specs()
                            .iter()
                            .copied()
                            .filter(|spec| spec.category == category)
                            .filter(|spec| query.is_empty() || spec_matches(spec, &query))
                            .map(|spec| {
                                // 用户点的是自定义项时，内建项一律不高亮（即使 base 用的是同一个算法）
                                let selected = picked.is_none()
                                    && current.as_ref().is_some_and(|g| {
                                        generator_catalog::spec_of(g).name == spec.name
                                    });
                                (
                                    spec.name.to_string(),
                                    spec.label.to_string(),
                                    column_type_label(&data_type_for_spec(spec)),
                                    false,
                                    selected,
                                )
                            }),
                    );
                    items.extend(
                        state
                            .read(cx)
                            .panel
                            .read(cx)
                            .user_generators_in_category(category)
                            .into_iter()
                            .map(|ug| {
                                // 精确到「点中的那一项」：同一 base 的其它自定义项不再跟着亮
                                let selected = picked.as_deref() == Some(ug.id.as_str());
                                (
                                    ug.id.clone(),
                                    ug.name.clone(),
                                    column_type_label(&ug.data_type),
                                    true,
                                    selected,
                                )
                            }),
                    );
                    // 立即求值成 Vec（惰性迭代器会把闭包里的 &String 借到渲染期，生命周期不够）
                    let item_rows: Vec<gpui_kit::AnyElement> = items
                        .into_iter()
                        .map(|(key, label, badge, is_user, selected)| {
                            render_item_row(
                                &state, &key, &label, &badge, is_user, selected, window, cx,
                            )
                        })
                        .collect();
                    group = group.child(
                        div()
                            .v_flex()
                            .gap_1()
                            .w_full()
                            .pl(ui::GENERATOR_PICK_INDENT)
                            .children(item_rows),
                    );
                }
                list = list.child(group);
            }
        }
        _ => {
            for data_type in COLUMN_TYPES {
                // `column_type_label` 不唯一（Text/Uuid → VARCHAR，DateTime/Timestamp →
                // TIMESTAMP）：ElementId 与展开态 key 必须用唯一键，否则重复 id 直接 panic
                let key = data_type_key(&data_type);
                let title = data_type_title(&data_type);
                let is_expanded = !query.is_empty() || expanded_types.contains(key);
                // 同上：标题行与子项列表垂直包一层，别让子项堆进标题行的 `h_flex`
                let mut group = div().v_flex().gap_1().w_full().child(render_type_group_row(
                    &state,
                    &data_type,
                    &title,
                    key,
                    is_expanded,
                    window,
                    cx,
                ));
                if is_expanded {
                    // 同上：`(写回键, 显示名, 徽标, 是否自定义, 是否选中)`，键不能拿显示名顶替
                    let mut items: Vec<(String, String, String, bool, bool)> = Vec::new();
                    items.extend(
                        generator_catalog::all_specs()
                            .iter()
                            .copied()
                            // 技术视角只留**方法级**生成器（#1：数据类型下展开的都是方法，
                            // 业务生成器只在「业务分类」Tab 可见）；自定义项仍全量按类型挂
                            .filter(|spec| generator_catalog::is_method_spec(spec))
                            .filter(|spec| {
                                data_type_for_spec(spec) == data_type
                                    && (query.is_empty() || spec_matches(spec, &query))
                            })
                            // 徽标 = 数据类（与所在组一致）：技术视角是「按类型看能用哪些生成方法」，
                            // 不再标业务组名（避免整组看起来像业务组的翻版）
                            .map(|spec| {
                                // 用户点的是自定义项时，内建项一律不高亮（即使 base 用的是同一个算法）
                                let selected = picked.is_none()
                                    && current.as_ref().is_some_and(|g| {
                                        generator_catalog::spec_of(g).name == spec.name
                                    });
                                (
                                    spec.name.to_string(),
                                    spec.label.to_string(),
                                    column_type_label(&data_type),
                                    false,
                                    selected,
                                )
                            }),
                    );
                    items.extend(
                        state
                            .read(cx)
                            .panel
                            .read(cx)
                            .user_generators_in_type(&data_type)
                            .into_iter()
                            .map(|ug| {
                                // 精确到「点中的那一项」：同一 base 的其它自定义项不再跟着亮
                                let selected = picked.as_deref() == Some(ug.id.as_str());
                                (
                                    ug.id.clone(),
                                    ug.name.clone(),
                                    column_type_label(&ug.data_type),
                                    true,
                                    selected,
                                )
                            }),
                    );
                    // 立即求值成 Vec（惰性迭代器会把闭包里的 &String 借到渲染期，生命周期不够）
                    let item_rows: Vec<gpui_kit::AnyElement> = items
                        .into_iter()
                        .map(|(key, label, badge, is_user, selected)| {
                            render_item_row(
                                &state, &key, &label, &badge, is_user, selected, window, cx,
                            )
                        })
                        .collect();
                    group = group.child(
                        div()
                            .v_flex()
                            .gap_1()
                            .w_full()
                            .pl(ui::GENERATOR_PICK_INDENT)
                            .children(item_rows),
                    );
                }
                list = list.child(group);
            }
        }
    }

    div()
        .v_flex()
        .gap_2()
        .w_full()
        .flex_1()
        .min_h_0()
        .child(
            TabBar::new("mock-lib-view")
                .segmented()
                .children([Tab::new().label("业务分类"), Tab::new().label("数据类型")])
                .selected_index(view_index)
                .on_click(move |index, _window, app| {
                    state.update(app, |s, cx| {
                        s.view = if *index == 0 {
                            LibraryView::Business
                        } else {
                            LibraryView::Tech
                        };
                        cx.notify();
                    });
                }),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .w_full()
                .child(list),
        )
        .into_any_element()
}

/// 「历史生成器」块：本会话用过的生成器（最多 [`RECENT_GENERATORS`] 条）。
///
/// **只有一行**（不换行、不折行）：行 2 的高度必须可预测，否则会把行 3 的库挤矮；
/// 超宽由 `overflow_hidden` 裁掉（5 条 xsmall 按钮在对话框宽度内通常放得下）。
/// 点击直接应用到表配置，**不关对话框**——右栏要就地跟着变。空时给一句灰字，
/// 免得右侧那块看起来像没渲染。
///
/// 历史同时收内建与自定义：内建按目录名反查中文名，自定义按 `ug.id` 反查用户起的名字；
/// 自定义项被删掉后不再显示（历史里不该留点不动的项）。
pub(crate) fn render_history_chips(
    state: &Entity<GeneratorLibraryState>,
    theme: &Theme,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let recent: Vec<(String, String, bool)> = {
        let panel = state.read(cx).panel.clone();
        let panel_ref = panel.read(cx);
        panel_ref
            .recent_generators()
            .iter()
            .filter_map(|(key, is_user)| {
                if *is_user {
                    panel_ref
                        .user_generators()
                        .iter()
                        .find(|ug| ug.id == *key)
                        .map(|ug| (key.clone(), ug.name.clone(), true))
                } else {
                    generator_catalog::all_specs()
                        .iter()
                        .copied()
                        .find(|spec| spec.name == key.as_str())
                        .map(|spec| (key.clone(), spec.label.to_string(), false))
                }
            })
            .collect()
    };
    if recent.is_empty() {
        return div()
            .id("mock-lib-history")
            .test_support()
            .h_flex()
            .items_center()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child("本会话还没用过生成器")
            .into_any_element();
    }
    let mut chips = div()
        .id("mock-lib-history")
        .test_support()
        .h_flex()
        .items_center()
        .gap_1()
        .flex_1()
        .min_w_0()
        .overflow_hidden();
    for (key, label, is_user) in recent {
        let state_for_click = state.clone();
        let chip_id = if is_user {
            format!("mock-lib-recent-user-{key}")
        } else {
            format!("mock-lib-recent-{key}")
        };
        chips = chips.child(
            button(chip_id, label, ActionKind::Quiet)
                .xsmall()
                .tooltip("最近使用：点击直接应用")
                .tooltip_show_delay(TOOLTIP_DELAY)
                .on_click(move |_event, window, app| {
                    let (panel, writeback) = {
                        let s = state_for_click.read(app);
                        (s.panel.clone(), s.writeback.clone())
                    };
                    apply_picked_generator(app, window, &panel, &writeback, &key, is_user);
                }),
        );
    }
    chips.into_any_element()
}