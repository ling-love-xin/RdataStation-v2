//! 双视角生成器库对话框 + 分组「＋」自定义生成器精调层（§9）。
//!
//! # 结构与入口
//!
//! - 打开：字段行生成器菜单 →「生成器库…」→ `panel::open_generator_library` → [`open_library`]；
//! - 浏览：顶部搜索框 + `TabBar` 双视角（业务分类 / 数据类型）+ 分组行（`Button` 展开/收起 +
//!   行尾「＋」`Button`，键盘可达）+ 展开的生成器项（内建在前、自定义在后，点击写回列并关对话框）；
//! - 精调层**内嵌**（避免嵌套 `open_dialog`，gpui 对话框单实例）：点「＋」→ `mode=Create`，
//!   保存成功后回 `Browse`。精调层三个下拉用 `select::{Select, SelectState}` + `SearchableVec`——
//!   **不用** `Button`+`dropdown_menu`：vendor `dropdown_button.rs:207` 触发器 id 硬编码 `"popup"`，
//!   同帧只允许一个 DropdownButton，而这里一屏要三个下拉。
//!
//! # 数据模型
//!
//! 自定义生成器 = **命名变体**（已有生成器 + 参数 + 名称），**不是**新 `GeneratorConfig` 变体
//! （`spec_of` / `generate_cell` 编译期穷尽 match，运行时新算法不成立）。保存时把参数 patch 回
//! 基础生成器的默认配置，产出 [`UserGenerator`]（硬属性 `data_type` + 可选 `category`，双向挂组）。

use std::collections::HashSet;
use std::time::Duration;

use super::*;
use fake::rand::SeedableRng as _;
use gpui_kit::component::Theme;
use gpui_kit::component::searchable_list::{SearchableListDelegate, SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};

use crate::mock_view::panel::MockPanel;
use crate::mock_view::pure::{
    column_type_label, complex_param_hint, complex_param_text, param_text, patch_param,
    patch_param_value, parse_complex_param,
};

/// 样例预览固定种子（与 `detail.rs::SAMPLE_PREVIEW_SEED` 同值同源：样例可复现，纯示意，
/// 与最终生成的随机种子无关）。
const SAMPLE_PREVIEW_SEED: u64 = 20261030;

/// 业务组下拉的项：`Option<GeneratorCategory>` 包装（孤儿规则不允许直接 `impl
/// SearchableListItem for Option<...>`——`Option` 是外部类型；`CategoryChoice` 是本地类型）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CategoryChoice(pub(crate) Option<GeneratorCategory>);

impl SearchableListItem for CategoryChoice {
    type Value = CategoryChoice;

    fn title(&self) -> SharedString {
        match self.0 {
            Some(category) => category.label().into(),
            None => "（不分组）".into(),
        }
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

// ---------- Select 下拉项（本地类型实现外部 trait，孤儿规则允许） ----------

impl SearchableListItem for &'static GeneratorSpec {
    type Value = &'static GeneratorSpec;

    fn title(&self) -> SharedString {
        (*self).label.into()
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

impl SearchableListItem for ColumnDataType {
    type Value = ColumnDataType;

    fn title(&self) -> SharedString {
        column_type_label(self).into()
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

// ---------- 对话框状态 ----------

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryView {
    Business,
    Tech,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryMode {
    Browse,
    Create,
}

/// 生成器库对话框的写回目标：字段行打开写**面板列**（现状，含 `remember_generator` 与
/// 结果清空）；列编辑对话框内打开写**列编辑草稿**（`MockDetailView::apply_generator_config_to_draft`，
/// 参数区 / 样例预览随写回重建——列还没应用，面板列不动）。
#[derive(Clone)]
pub(crate) enum LibraryWriteback {
    Panel { column_id: u64 },
    ColumnDraft { detail: gpui::WeakEntity<MockDetailView>, column_id: u64 },
}

/// 生成器库对话框的状态（实体由 [`open_library`] 创建；builder 每帧从它读，写入走事件路径）。
pub(crate) struct GeneratorLibraryState {
    pub(crate) panel: Entity<MockPanel>,
    pub(crate) writeback: LibraryWriteback,
    pub(crate) mode: LibraryMode,
    pub(crate) view: LibraryView,
    /// 顶部搜索词（`search_input` 的 Change 订阅回填；浏览时过滤两组列表）
    pub(crate) query: String,
    /// 业务视角展开的分组名（`GeneratorCategory::label()`）
    pub(crate) expanded: HashSet<String>,
    /// 技术视角展开的类型标签（`column_type_label`）
    pub(crate) expanded_types: HashSet<String>,
    /// 顶部搜索框（占位与搜索词存在它里，不另存一份）
    pub(crate) search_input: Entity<InputState>,
    /// 精调层草稿（`mode=Create` 时非空）
    pub(crate) create: Option<CreateDraft>,
    /// 编辑中的自定义生成器 id（`Some` = 精调层是编辑而非新建；保存走 `update_user_generator`）
    pub(crate) editing: Option<String>,
    _search_sub: Option<Subscription>,
}

/// 精调层草稿（分组「＋」进入时构建，保存成功后丢弃）。
pub(crate) struct CreateDraft {
    /// 从技术组「＋」进入 → 类型锁定（`Some(t)`）；业务组「＋」进入 → `None`（可自选）
    pub(crate) locked_type: Option<ColumnDataType>,
    /// 从业务组「＋」进入 → 业务组锁定（`Some(c)`）；技术组「＋」进入 → `None`（可自选/不分组）
    pub(crate) locked_category: Option<GeneratorCategory>,
    /// 样例预览语言（面板当前语言偏好，创建时快照；与列编辑同口径）
    pub(crate) locale: crate::models::Locale,
    pub(crate) name_input: Entity<InputState>,
    /// 类型下拉（`locked_type` 非空时 `None`——类型锁定不可改）
    pub(crate) type_state: Option<Entity<SelectState<SearchableVec<ColumnDataType>>>>,
    /// 业务组下拉（`locked_category` 非空时 `None`）；含「（不分组）」项
    pub(crate) category_state: Option<Entity<SelectState<SearchableVec<CategoryChoice>>>>,    /// 基础生成器下拉（技术组进入时按锁定类型过滤；项是 `&'static GeneratorSpec`；
    /// 编辑模式 `locked_base` 非空时该字段是空壳不用——行渲染走只读文本）
    pub(crate) base_state: Entity<SelectState<SearchableVec<&'static GeneratorSpec>>>,
    /// 编辑模式：被编辑项保存过的完整配置（`locked_base` 的默认配置 + 参数 patch）。
    /// `Some` 时 `base_config()` 的兜底不生效，参数区 / 样例以它为基准（保住用户改过的参数值）；
    /// 新建模式 `None`（从基础生成器默认配置开始）。
    pub(crate) preset: Option<GeneratorConfig>,
    /// 编辑模式锁定的基础生成器（编辑不改 base——换 base 等于换个生成器，删了重建更直接；
    /// `Some` 时基础生成器行渲染为只读文本，也不建 `base_state` 的 Confirm 订阅）
    pub(crate) locked_base: Option<&'static GeneratorSpec>,
    /// 参数输入行：按 `base` 当前选中的 spec 重建（身份变化由 `Confirm` 订阅驱动）
    pub(crate) params: Vec<DraftParam>,
    /// 样例预览（2 条示意值，固定种子；参数 Change 即刷新）
    pub(crate) preview: String,
    pub(crate) error: Option<String>,
    /// 参数 / 基础生成器订阅（必须持有，丢了自动退订）
    pub(crate) _subs: Vec<Subscription>,
}

/// 精调层参数行（Bool 用 Switch，不再退化成文本框——质询的直接回应）。
pub(crate) enum DraftParam {
    Scalar {
        key: String,
        kind: ParamKind,
        state: Entity<InputState>,
    },
    Bool {
        key: String,
        value: bool,
    },
    Complex {
        key: String,
        state: Entity<TextareaState>,
    },
}

fn param_key(param: &DraftParam) -> &str {
    match param {
        DraftParam::Scalar { key, .. } | DraftParam::Bool { key, .. } | DraftParam::Complex { key, .. } => key,
    }
}

// ---------- 打开入口（panel::open_generator_library 调用） ----------

/// 打开生成器库对话框。`cx` 是面板的 Context；对话框状态实体、输入实体都在这里创建，
/// 订阅挂在状态实体上（`subscribe_in` 的订阅者 = 状态实体自身）。
pub(crate) fn open_library(
    panel: Entity<MockPanel>,
    writeback: LibraryWriteback,
    window: &mut Window,
    cx: &mut Context<MockPanel>,
) {
    let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("按名称 / 中文标签搜索"));
    let state = cx.new(|_cx| GeneratorLibraryState {
        panel: panel.clone(),
        writeback,
        mode: LibraryMode::Browse,
        view: LibraryView::Business,
        query: String::new(),
        expanded: HashSet::new(),
        expanded_types: HashSet::new(),
        search_input: search_input.clone(),
        create: None,
        editing: None,
        _search_sub: None,
    });
    // 搜索词订阅挂在状态实体上（实体已创建完；在 `cx.new` 构造闭包内订阅会推不出回调参数类型）
    state.update(cx, |s, cx| {
        let _search_sub = cx.subscribe_in(
            &search_input,
            window,
            move |s, emitter, ev: &InputEvent, _window, cx| {
                if matches!(ev, InputEvent::Change) {
                    s.query = emitter.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );
        s._search_sub = Some(_search_sub);
    });

    window.open_dialog(cx, move |dialog, window, cx| {
        // `cx.theme()` 借用 cx：先克隆出值，避免后续把 `cx` 可变借给渲染函数时冲突
        let theme = cx.theme().clone();
        let body: gpui_kit::AnyElement = match state.read(cx).mode {
            LibraryMode::Browse => render_browse(&state, &theme, window, cx),
            LibraryMode::Create => render_create(&state, &theme, window, cx),
        };
        let footer: Option<gpui_kit::AnyElement> = match state.read(cx).mode {
            LibraryMode::Browse => None,
            LibraryMode::Create => Some(render_create_footer(&state, window, cx)),
        };
        // 标题随模式动态：open_dialog 的 build 闭包每帧重建（vendor root.rs），重绘时重新求值
        let title = match state.read(cx).mode {
            LibraryMode::Browse => "生成器库",
            LibraryMode::Create => {
                if state.read(cx).editing.is_some() {
                    "编辑自定义生成器"
                } else {
                    "新建自定义生成器"
                }
            }
        };
        let dialog = dialog.title(title).width(px(560.0)).child(body);
        match footer {
            Some(footer) => dialog.footer(footer),
            None => dialog,
        }
    });
}

// ---------- 浏览模式 ----------

/// 统一写回：`picked` 是内建生成器名（`is_user=false`）或自定义生成器 id（`is_user=true`）。
///
/// - [`LibraryWriteback::Panel`]（字段行入口）：保持原行为——写面板列（`set_generator` /
///   `apply_user_generator`，含 `remember_generator` 与结果清空）并关对话框；
/// - [`LibraryWriteback::ColumnDraft`]（列编辑入口）：把生成器配置写回列编辑草稿
///   （`MockDetailView::apply_generator_config_to_draft` 重建参数区与样例）并关对话框。
fn apply_picked_generator(
    app: &mut App,
    window: &mut Window,
    panel: &Entity<MockPanel>,
    writeback: &LibraryWriteback,
    picked: &str,
    is_user: bool,
) {
    match writeback {
        LibraryWriteback::Panel { column_id } => {
            panel.update(app, |p, cx| {
                if is_user {
                    p.apply_user_generator(*column_id, picked, cx);
                } else {
                    p.set_generator(*column_id, picked, cx);
                }
                window.close_dialog(cx);
            });
        }
        LibraryWriteback::ColumnDraft { detail, column_id } => {
            // 内建取规格默认配置；自定义解包 base（与 `apply_user_generator` 同源）
            let config = if is_user {
                panel
                    .read(app)
                    .user_generators
                    .iter()
                    .find(|ug| ug.id == picked)
                    .map(|ug| ug.base.clone())
            } else {
                generator_catalog::default_of(picked)
            };
            if let Some(config) = config {
                let _ = detail.update(app, |v, cx| {
                    v.apply_generator_config_to_draft(*column_id, config, window, cx);
                    window.close_dialog(cx);
                });
            }
        }
    }
}

/// 顶部搜索 + TabBar 双视角 + 分组列表（业务视角：15 分类；技术视角：13 数据类）。
fn render_browse(
    state: &Entity<GeneratorLibraryState>,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    // `state` 转 owned：按钮闭包捕获要 `'static`，参数引用只活到本函数返回
    let state = state.clone();
    let s = state.read(cx);
    let search = s.search_input.clone();
    let view_index: usize = match s.view {
        LibraryView::Business => 0,
        LibraryView::Tech => 1,
    };
    // `s`（read guard）持有 cx 不可变借用；clone 出所需字段后 guard 在最后一次使用后自动释放
    let expanded: HashSet<String> = s.expanded.clone();
    let expanded_types: HashSet<String> = s.expanded_types.clone();
    let query = s.query.clone();

    let mut list = div().v_flex().gap_1().w_full();
    // 最近使用（本会话点过的，菜单退役后这里就是快速路径）：非空才渲染，不受搜索词过滤
    if let Some(recent) = render_recent(&state, theme, window, cx) {
        list = list.child(recent);
    }
    match view_index {
        0 => {
            for category in GeneratorCategory::ALL {
                let name = category.label();
                let is_expanded = expanded.contains(name);
                let mut row = render_group_row(&state, name, is_expanded, category, window, cx);
                if is_expanded {
                    let mut items: Vec<(String, String, bool)> = Vec::new();
                    items.extend(
                        generator_catalog::all_specs()
                            .iter()
                            .copied()
                            .filter(|spec| spec.category == category)
                            .filter(|spec| query.is_empty() || spec_matches(spec, &query))
                            .map(|spec| {
                                (
                                    spec.label.to_string(),
                                    column_type_label(&data_type_for_spec(spec)),
                                    false,
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
                                (
                                    ug.name.clone(),
                                    column_type_label(&ug.data_type),
                                    true,
                                )
                            }),
                    );
                    // 立即求值成 Vec（惰性迭代器会把闭包里的 &String 借到渲染期，生命周期不够）
                    let item_rows: Vec<gpui_kit::AnyElement> = items
                        .into_iter()
                        .map(|(label, badge, is_user)| {
                            render_item_row(&state, &label, &badge, is_user, window, cx)
                        })
                        .collect();
                    row = row.child(div().v_flex().gap_1().w_full().pl(px(20.)).children(item_rows));
                }
                list = list.child(row);
            }
        }
        _ => {
            for data_type in COLUMN_TYPES {
                let label = column_type_label(&data_type);
                let is_expanded = expanded_types.contains(&label);
                let mut row = render_type_group_row(
                    &state,
                    &data_type,
                    &label,
                    is_expanded,
                    window,
                    cx,
                );
                if is_expanded {
                    let mut items: Vec<(String, String, bool)> = Vec::new();
                    items.extend(
                        generator_catalog::all_specs()
                            .iter()
                            .copied()
                            .filter(|spec| {
                                data_type_for_spec(spec) == data_type
                                    && (query.is_empty() || spec_matches(spec, &query))
                            })
                            .map(|spec| {
                                (spec.label.to_string(), spec.category.label().to_string(), false)
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
                                (
                                    ug.name.clone(),
                                    ug.category
                                        .map(|c| c.label().to_string())
                                        .unwrap_or_else(|| "（不分组）".to_string()),
                                    true,
                                )
                            }),
                    );
                    // 立即求值成 Vec（惰性迭代器会把闭包里的 &String 借到渲染期，生命周期不够）
                    let item_rows: Vec<gpui_kit::AnyElement> = items
                        .into_iter()
                        .map(|(label, badge, is_user)| {
                            render_item_row(&state, &label, &badge, is_user, window, cx)
                        })
                        .collect();
                    row = row.child(div().v_flex().gap_1().w_full().pl(px(20.)).children(item_rows));
                }
                list = list.child(row);
            }
        }
    }

    div()
        .v_flex()
        .gap_3()
        .w_full()
        .h(px(420.))
        .child(div().w_full().child(Input::new(&search)))
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

/// 最近使用（本会话点过的生成器，最多 [`RECENT_GENERATORS`] 条）：非空才渲染，
/// 点击直接应用（静态 spec → `set_generator`）并关闭对话框——菜单退役后的快速路径。
fn render_recent(
    state: &Entity<GeneratorLibraryState>,
    theme: &Theme,
    _window: &mut Window,
    cx: &mut App,
) -> Option<gpui_kit::AnyElement> {
    let recent: Vec<(&'static str, String)> = state
        .read(cx)
        .panel
        .read(cx)
        .recent_generators()
        .iter()
        .filter_map(|name| {
            generator_catalog::all_specs()
                .iter()
                .copied()
                .find(|spec| spec.name == name.as_str())
                .map(|spec| (spec.name, spec.label.to_string()))
        })
        .collect();
    if recent.is_empty() {
        return None;
    }
    let mut chips = div().h_flex().gap_1().w_full().flex_wrap();
    for (name, label) in recent {
        let state_for_click = state.clone();
        chips = chips.child(
            Button::new(format!("mock-lib-recent-{name}"))
                .ghost()
                .xsmall()
                .label(label)
                .tooltip("最近使用：点击直接应用")
                .tooltip_show_delay(Duration::from_millis(500))
                .on_click(move |_event, window, app| {
                    let s = state_for_click.read(app);
                    let panel = s.panel.clone();
                    let writeback = s.writeback.clone();
                    apply_picked_generator(app, window, &panel, &writeback, name, false);
                }),
        );
    }
    Some(
        div()
            .v_flex()
            .gap_1()
            .w_full()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("最近使用"),
            )
            .child(chips)
            .into_any_element(),
    )
}

/// 业务分组行：展开/收起按钮 + 行尾「＋」（键盘可达，`Button`）。
fn render_group_row(
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
            div()
                .flex_1()
                .min_w_0()
                .child(
                    Button::new(format!("lib-group-{name}"))
                        .ghost()
                        .xsmall()
                        .icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .label(format!("{name}（{item_count}）"))
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
            Button::new(format!("lib-group-plus-{name}"))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .tooltip(format!("在“{name}”分组新建生成器"))
                .tooltip_show_delay(Duration::from_millis(500))
                .on_click(move |_event, window, app| {
                    state_for_create.update(app, |s, cx| {
                        s.mode = LibraryMode::Create;
                        s.create = Some(new_create_draft(
                            s.panel.clone(),
                            None,
                            Some(category),
                            window,
                            cx,
                        ));
                        s.rebuild_params(window, cx);
                        s.refresh_preview(cx);
                        cx.notify();
                    });
                }),
        )
}

/// 技术分组行：同业务分组，但展开集走类型、入口锁定类型。
fn render_type_group_row(
    state: &Entity<GeneratorLibraryState>,
    data_type: &ColumnDataType,
    label: &str,
    expanded: bool,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::Div {
    let state_for_toggle = state.clone();
    let label_for_toggle = label.to_string();
    let state_for_create = state.clone();
    let data_type_for_create = data_type.clone();
    let item_count = generator_catalog::all_specs()
        .iter()
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
            div()
                .flex_1()
                .min_w_0()
                .child(
                    Button::new(format!("lib-type-group-{label}"))
                        .ghost()
                        .xsmall()
                        .icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .label(format!("{label}（{item_count}）"))
                        .toggled(expanded)
                        .on_click(move |_event, _window, app| {
                            state_for_toggle.update(app, |s, cx| {
                                if !s.expanded_types.remove(&label_for_toggle) {
                                    s.expanded_types.insert(label_for_toggle.clone());
                                }
                                cx.notify();
                            });
                        }),
                ),
        )
        .child(
            Button::new(format!("lib-type-group-plus-{label}"))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .tooltip(format!("在“{label}”数据类型下新建生成器"))
                .tooltip_show_delay(Duration::from_millis(500))
                .on_click(move |_event, window, app| {
                    state_for_create.update(app, |s, cx| {
                        s.mode = LibraryMode::Create;
                        s.create = Some(new_create_draft(
                            s.panel.clone(),
                            Some(data_type_for_create.clone()),
                            None,
                            window,
                            cx,
                        ));
                        s.rebuild_params(window, cx);
                        s.refresh_preview(cx);
                        cx.notify();
                    });
                }),
        )
}

/// 生成器项行：名称 + 徽标（业务视角徽标 = 数据类；技术视角徽标 = 业务组）。
/// 点击写回列（内建走 `set_generator`，自定义解包 base）并关对话框。
/// 自定义项行尾另有「编辑」「删除」两个管理按钮（键盘可达，`Button`）。
fn render_item_row(
    state: &Entity<GeneratorLibraryState>,
    label: &str,
    badge: &str,
    is_user: bool,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let theme = cx.theme();
    let state_for_click = state.clone();
    let spec_name = label.to_string();
    let spec_for_click = spec_name.clone();
    let mut row = div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    Button::new(format!("lib-item-{label}"))
                        .ghost()
                        .xsmall()
                        .label(label)
                        .on_click(move |_event, window, app| {
                            let s = state_for_click.read(app);
                            let panel = s.panel.clone();
                            let writeback = s.writeback.clone();
                            let ug_id = spec_for_click.clone();
                            apply_picked_generator(app, window, &panel, &writeback, &ug_id, is_user);
                        }),
                ),
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
        let ug_id_edit = spec_name.clone();
        row = row
            .child(
                Button::new(format!("lib-item-edit-{label}"))
                    .ghost()
                    .xsmall()
                    .label("编辑")
                    .tooltip("重命名 / 修改参数")
                    .tooltip_show_delay(Duration::from_millis(500))
                    .on_click(move |_event, window, app| {
                        let s = state_edit.read(app);
                        let panel = s.panel.clone();
                        let Some(ug) = panel
                            .read(app)
                            .user_generators()
                            .iter()
                            .find(|ug| ug.id == ug_id_edit)
                            .cloned()
                        else {
                            return;
                        };
                        state_edit.update(app, |s, cx| {
                            s.editing = Some(ug.id.clone());
                            s.mode = LibraryMode::Create;
                            s.create = Some(new_edit_draft(panel.clone(), &ug, window, cx));
                            s.rebuild_params(window, cx);
                            s.refresh_preview(cx);
                            cx.notify();
                        });
                    }),
            )
            .child(
                {
                    let state_del = state.clone();
                    let ug_id_del = spec_name.clone();
                    Button::new(format!("lib-item-delete-{label}"))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Delete)
                        .tooltip("删除此自定义生成器")
                        .tooltip_show_delay(Duration::from_millis(500))
                        .on_click(move |_event, _window, app| {
                            let s = state_del.read(app);
                            let panel = s.panel.clone();
                            let ug_id = ug_id_del.clone();
                            // 先删 panel 项，再刷新对话框列表（panel.notify 不驱动对话框实体重绘）
                            panel.update(app, |p, cx| p.delete_user_generator(&ug_id, cx));
                            state_del.update(app, |_, cx| cx.notify());
                        })
                },
            );
    }
    row.into_any_element()
}

// ---------- 精调层（创建自定义生成器） ----------

/// 新建精调层草稿：锁定一个维度（类型或业务组），另一维可自选，基础生成器按维度过滤。
fn new_create_draft(
    panel: Entity<MockPanel>,
    locked_type: Option<ColumnDataType>,
    locked_category: Option<GeneratorCategory>,
    window: &mut Window,
    cx: &mut Context<GeneratorLibraryState>,
) -> CreateDraft {
    // 样例预览语言：面板当前语言偏好（生成器大多与语言无关，但姓名/电话等随语言）
    let locale = panel.read(cx).draft().options.locale.clone();
    // 类型下拉（未锁定才建）
    let type_state = locked_type.as_ref().map(|_| {
        let delegate = SearchableVec::new(COLUMN_TYPES.to_vec());
        cx.new(|cx| SelectState::new(delegate, None, window, cx))
    });
    // 业务组下拉（未锁定才建；含「（不分组）」）
    let mut category_items: Vec<CategoryChoice> = vec![CategoryChoice(None)];
    category_items.extend(GeneratorCategory::ALL.iter().copied().map(|c| CategoryChoice(Some(c))));
    let category_state = locked_category.as_ref().map(|_| {
        let delegate = SearchableVec::new(category_items);
        cx.new(|cx| SelectState::new(delegate, None, window, cx))
    });
    // 基础生成器：技术组进入按锁定类型过滤；业务组进入全部
    let base_items: Vec<&'static GeneratorSpec> = generator_catalog::all_specs()
        .iter()
        .copied()
        .filter(|spec| {
            locked_type
                .as_ref()
                .map(|t| data_type_for_spec(spec) == *t)
                .unwrap_or(true)
        })
        .collect();
    let base_delegate = SearchableVec::new(base_items);
    let base_state = cx.new(|cx| SelectState::new(base_delegate, None, window, cx));

    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("生成器名（必填）"));

    let mut draft = CreateDraft {
        locked_type,
        locked_category,
        locale,
        name_input,
        type_state,
        category_state,
        base_state,
        preset: None,
        locked_base: None,
        params: Vec::new(),
        preview: String::new(),
        error: None,
        _subs: Vec::new(),
    };
    // 基础生成器身份变化 → 重建参数行 + 刷样例（订阅一次，草稿生命周期内常驻；
    // 换生成器 = 换参数集，旧输入不保留）
    let base_for_sub = draft.base_state.clone();
    let _sub = cx.subscribe_in(
        &base_for_sub,
        window,
        move |state, _emitter, ev, window, cx| {
            if matches!(ev, SelectEvent::Confirm(_)) {
                state.rebuild_params(window, cx);
                state.refresh_preview(cx);
            }
        },
    );
    draft._subs.push(_sub);
    draft
}

/// 编辑精调层草稿：复用创建草稿（类型 / 业务组锁定 = 被编辑项的值），再覆写为编辑态——
/// 基础生成器锁定（编辑不改 base：换 base = 换个生成器，删了重建更直接）、参数区 / 样例以
/// 被编辑项**保存过的配置**为基准（`preset`，用户改过的参数值在编辑重开时不丢）、名字预填。
fn new_edit_draft(
    panel: Entity<MockPanel>,
    ug: &UserGenerator,
    window: &mut Window,
    cx: &mut Context<GeneratorLibraryState>,
) -> CreateDraft {
    let mut draft =
        new_create_draft(panel.clone(), Some(ug.data_type.clone()), ug.category, window, cx);
    // 名字预填（InputState::set_value 不发 Change 事件，无副作用）
    draft
        .name_input
        .update(cx, |s, cx| s.set_value(ug.name.clone(), window, cx));
    draft.locked_base = Some(generator_catalog::spec_of(&ug.base));
    draft.preset = Some(ug.base.clone());
    draft
}

impl CreateDraft {
    /// 当前选中基础生成器的默认配置（`default_of`）；未选中时 `None`。
    fn base_config(&self, cx: &App) -> Option<GeneratorConfig> {
        self.base_state
            .read(cx)
            .selected_value()
            .copied()
            .and_then(|spec| generator_catalog::default_of(spec.name))
    }

    /// 基准配置：编辑模式 = 被编辑项保存的配置（`preset`，保住用户改过的参数值）；
    /// 新建模式 = 基础生成器默认配置。参数区重建 / 校验 / 样例共用。
    fn effective_config(&self, cx: &App) -> Option<GeneratorConfig> {
        self.preset.clone().or_else(|| self.base_config(cx))
    }

    /// 按「基准配置 + 全部参数输入值」拼出最终配置（样例预览与保存共用）。
    fn patched_config(&self, cx: &App) -> Option<GeneratorConfig> {
        let mut config = self.effective_config(cx)?;
        for param in &self.params {
            match param {
                DraftParam::Scalar { key, kind, state } => {
                    let text = state.read(cx).value().to_string();
                    if let Some(patched) = patch_param(&config, key, &text, *kind) {
                        config = patched;
                    }
                }
                DraftParam::Bool { key, value } => {
                    if let Some(patched) = patch_param(
                        &config,
                        key,
                        if *value { "true" } else { "false" },
                        ParamKind::Bool,
                    ) {
                        config = patched;
                    }
                }
                DraftParam::Complex { key, state } => {
                    let text = state.read(cx).value().to_string();
                    if let Ok(value) = parse_complex_param(key, &text) {
                        if let Some(patched) = patch_param_value(&config, key, value) {
                            config = patched;
                        }
                    }
                }
            }
        }
        Some(config)
    }
}

impl GeneratorLibraryState {
    /// 按 base 当前选中重建参数行（身份变化由 `Confirm` 订阅驱动；旧输入不保留——
    /// 换生成器 = 换参数集，沿用旧值反而误导）。
    fn rebuild_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.create.as_mut() else {
            return;
        };
        draft.params.clear();
        draft._subs.clear();
        let Some(config) = draft.effective_config(cx) else {
            draft.preview.clear();
            cx.notify();
            return;
        };
        let spec = generator_catalog::spec_of(&config);
        let fields: Vec<ParamField> = spec.params.iter().copied().collect();
        // 每个参数一个订阅：Change → 写回草稿值 + 刷样例。Bool 用 Switch（不再退化成文本框）。
        for field in fields {
            let key = field.key.to_string();
            match field.kind {
                ParamKind::Complex => {
                    let initial = complex_param_text(&config, field.key);
                    let state = cx.new(|cx| TextareaState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |state, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            state.commit_complex(&key_for_closure, &text, cx);
                            state.refresh_preview(cx);
                        },
                    );
                    draft._subs.push(_sub);
                    draft.params.push(DraftParam::Complex { key, state });
                }
                ParamKind::Bool => {
                    let initial = param_text(&config, field.key) == "true";
                    draft.params.push(DraftParam::Bool {
                        key,
                        value: initial,
                    });
                }
                kind => {
                    let initial = param_text(&config, field.key);
                    let state = cx.new(|cx| InputState::new(window, cx));
                    state.update(cx, |s, cx| s.set_value(initial, window, cx));
                    let key_for_closure = key.clone();
                    let _sub = cx.subscribe_in(
                        &state,
                        window,
                        move |state, emitter, ev: &InputEvent, _window, cx| {
                            if !matches!(ev, InputEvent::Change) {
                                return;
                            }
                            let text = emitter.read(cx).value().to_string();
                            state.commit_scalar(&key_for_closure, &text, kind, cx);
                            state.refresh_preview(cx);
                        },
                    );
                    draft._subs.push(_sub);
                    draft.params.push(DraftParam::Scalar { key, kind, state });
                }
            }
        }
        cx.notify();
    }

    /// 标量参数 Change：校验值合法性（值存在输入框里，`patched_config` 现读现 patch）。
    fn commit_scalar(
        &mut self,
        key: &str,
        text: &str,
        kind: ParamKind,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.create.as_mut() else {
            return;
        };
        let Some(config) = draft.effective_config(cx) else {
            return;
        };
        draft.error = if patch_param(&config, key, text, kind).is_none() {
            Some(format!("参数 {key} 的值不合法"))
        } else {
            None
        };
        cx.notify();
    }

    /// 复杂参数 Change：解析成功清错、失败记错（值存在 Textarea 里，保存时现读）。
    fn commit_complex(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        if let Some(draft) = self.create.as_mut() {
            draft.error = match parse_complex_param(key, text) {
                Ok(_) => None,
                Err(reason) => Some(reason),
            };
        }
        cx.notify();
    }

    /// 样例预览：2 条示意值（固定种子，catch_unwind 兜底空参数区间 panic）。
    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.create.as_mut() else {
            return;
        };
        let Some(config) = draft.patched_config(cx) else {
            draft.preview.clear();
            cx.notify();
            return;
        };
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let locale = draft.locale.clone();
        let mut rng = fake::rand::rngs::StdRng::seed_from_u64(SAMPLE_PREVIEW_SEED);
        let samples: Vec<String> = (0..2)
            .map(|row| {
                catch_unwind(AssertUnwindSafe(|| {
                    crate::generators::generate_cell(&config, &mut rng, row, &locale)
                }))
                .unwrap_or_else(|_| "（参数区间为空，无法采样）".to_string())
            })
            .collect();
        draft.preview = samples.join("，");
        cx.notify();
    }
}

// ---------- 保存与 footer ----------

/// 保存：提交 panel（去重命名）→ 应用写回列 → 关对话框。失败（名空/无 base）就地提示。
fn submit_create(state: &Entity<GeneratorLibraryState>, window: &mut Window, cx: &mut App) {
    let (name, data_type, category, config) = {
        let s = state.read(cx);
        let Some(draft) = s.create.as_ref() else {
            return;
        };
        let name = draft.name_input.read(cx).value().to_string();
        let data_type = draft.locked_type.clone().or_else(|| {
            draft
                .type_state
                .as_ref()
                .and_then(|ts| ts.read(cx).selected_value().cloned())
        });
        let category = draft.locked_category.or_else(|| {
            draft
                .category_state
                .as_ref()
                .and_then(|cs| cs.read(cx).selected_value().copied())
                .and_then(|choice| choice.0)
        });
        (name, data_type, category, draft.patched_config(cx))
    };
    let (name, Some(data_type), category, Some(config)) = (name, data_type, category, config) else {
        state.update(cx, |s, cx| {
            if let Some(d) = s.create.as_mut() {
                d.error = Some("请填写生成器名并选择基础生成器".to_string());
            }
            cx.notify();
        });
        return;
    };
    let panel = state.read(cx).panel.clone();
    let editing = state.read(cx).editing.clone();
    // 编辑 = 更新（改名去重 + 全量替换）；新建 = 新增。成功都「保存即应用」写回当前列。
    let saved = panel.update(cx, |p, cx| match &editing {
        Some(id) => p.update_user_generator(id, &name, data_type, category, config, cx),
        None => p.add_user_generator(&name, data_type, category, config, cx),
    });
    match saved {
        Ok(ug_id) => {
            // 保存即应用（与「保存双向挂组」一致：创建/编辑完直接写回当前列/草稿）
            let panel = state.read(cx).panel.clone();
            let writeback = state.read(cx).writeback.clone();
            apply_picked_generator(cx, window, &panel, &writeback, &ug_id, true);
            state.update(cx, |s, cx| {
                s.mode = LibraryMode::Browse;
                s.create = None;
                s.editing = None;
                cx.notify();
            });
            window.close_dialog(cx);
        }
        Err(reason) => {
            state.update(cx, |s, cx| {
                if let Some(d) = s.create.as_mut() {
                    d.error = Some(reason);
                }
                cx.notify();
            });
        }
    }
}

/// 精调层 footer：取消（回浏览）+ 保存并应用（编辑模式文案「保存修改」）。
fn render_create_footer(
    state: &Entity<GeneratorLibraryState>,
    _window: &mut Window,
    _cx: &mut App,
) -> gpui_kit::AnyElement {
    let state_for_cancel = state.clone();
    let state_for_save = state.clone();
    let editing = state.read(_cx).editing.is_some();
    DialogFooter::new()
        .child(
            Button::new("lib-create-cancel")
                .ghost()
                .label("取消")
                .on_click(move |_event, _window, app| {
                    state_for_cancel.update(app, |s, cx| {
                        s.mode = LibraryMode::Browse;
                        s.create = None;
                        s.editing = None;
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new("lib-create-save")
                .label(if editing { "保存修改" } else { "保存并应用" })
                .on_click(move |_event, window, app| {
                    submit_create(&state_for_save, window, app);
                }),
        )
        .into_any_element()
}

// ---------- 精调层渲染 ----------

/// label + Select 一行（与 `form_line` 同构；Select 常驻下拉，多实例无冲突）。
fn select_line<D: SearchableListDelegate + 'static>(
    theme: &Theme,
    label: &str,
    select: Select<D>,
) -> gpui_kit::AnyElement {
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w_0().child(select))
        .into_any_element()
}

/// 精调层表单：名称 + 类型（锁定/可选）+ 业务组（锁定/可选）+ 基础生成器 + 参数区 + 样例。
fn render_create(
    state: &Entity<GeneratorLibraryState>,
    theme: &Theme,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let s = state.read(cx);
    let Some(draft) = s.create.as_ref() else {
        return div().into_any_element();
    };
    let theme = theme.clone();
    let name_state = draft.name_input.clone();
    let mut body = div().v_flex().gap_2().w_full().h(px(420.));
    body = body.child(form_line(&theme, "生成器名", &name_state));
    // 类型行：锁定 → 只读文本；未锁定 → Select
    body = match &draft.locked_type {
        Some(t) => body.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("数据类型"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(column_type_label(t)),
                ),
        ),
        None => {
            if let Some(type_state) = &draft.type_state {
                body.child(select_line(
                    &theme,
                    "数据类型",
                    Select::new(type_state)
                        .menu_width(px(260.))
                        .menu_max_h(px(220.))
                        .placeholder("选择数据类型…")
                        .accessibility_label("数据类型"),
                ))
            } else {
                body
            }
        }
    };
    // 业务组行：锁定 → 只读文本；未锁定 → Select（含「（不分组）」）
    body = match &draft.locked_category {
        Some(c) => body.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("业务组"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(c.label()),
                ),
        ),
        None => {
            if let Some(category_state) = &draft.category_state {
                body.child(select_line(
                    &theme,
                    "业务组",
                    Select::new(category_state)
                        .menu_width(px(260.))
                        .menu_max_h(px(220.))
                        .placeholder("选择业务组（可不选）")
                        .accessibility_label("业务组"),
                ))
            } else {
                body
            }
        }
    };
    // 基础生成器：编辑模式锁定为只读文本（换 base = 换生成器，删了重建更直接）；
    // 新建模式走 Select（按锁定类型过滤）
    body = match &draft.locked_base {
        Some(spec) => body.child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("基础生成器"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(spec.label),
                ),
        ),
        None => body.child(select_line(
            &theme,
            "基础生成器",
            Select::new(&draft.base_state)
                .menu_width(px(260.))
                .menu_max_h(px(220.))
                .placeholder("选择基础生成器…")
                .accessibility_label("基础生成器"),
        )),
    };
    // 参数区（按 base spec 重建；Int/Float/Text→Input、Bool→Switch、Complex→Textarea）
    let has_params = !draft.params.is_empty();
    // 基础生成器是否已定（新建选完 / 编辑锁定）：已定但零参数（自增 / UUID）如实说明，别误导成「还没选」
    let base_ready = draft.effective_config(cx).is_some();
    body = body.child(
        div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(if has_params {
                        "参数（来自基础生成器）"
                    } else if base_ready {
                        "参数（该生成器没有参数）"
                    } else {
                        "参数（选择基础生成器后出现）"
                    }),
            )
            .when(has_params, |el| {
                el.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .w_full()
                        .children(draft.params.iter().map(|param| {
                            let key = param_key(param).to_string();
                            let label = {
                                let fields = draft
                                    .effective_config(cx)
                                    .map(|config| {
                                        generator_catalog::spec_of(&config)
                                            .params
                                            .iter()
                                            .copied()
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                fields
                                    .iter()
                                    .find(|f| f.key == param_key(param))
                                    .map(|f| f.label.to_string())
                                    .unwrap_or_else(|| param_key(param).to_string())
                            };
                            let state = state.clone();
                            let theme2 = theme.clone();
                            match param {
                                DraftParam::Scalar { state: input_state, .. } => {
                                    form_line(&theme2, &label, input_state).into_any_element()
                                }
                                DraftParam::Complex { state: text_state, .. } => {
                                    let hint = complex_param_hint(&key);
                                    complex_form_line(&theme2, &label, hint, text_state)
                                        .into_any_element()
                                }
                                DraftParam::Bool { value, .. } => {
                                    let value = *value;
                                    let key2 = key.clone();
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_2()
                                        .w_full()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_xs()
                                                .text_color(theme2.colors.muted_foreground)
                                                .child(label),
                                        )
                                        .child(
                                            Switch::new(format!("lib-param-{key2}"))
                                                .checked(value)
                                                .on_click(move |checked, _window, app| {
                                                    let checked = *checked;
                                                    state.update(app, |s, cx| {
                                                        if let Some(d) = s.create.as_mut() {
                                                            for p in &mut d.params {
                                                                if let DraftParam::Bool {
                                                                    key: pk,
                                                                    value: v,
                                                                } = p
                                                                {
                                                                    if *pk == key2 {
                                                                        *v = checked;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        s.refresh_preview(cx);
                                                    });
                                                }),
                                        )
                                        .into_any_element()
                                }
                            }
                        })),
                )
            }),
    );
    // 样例预览
    body = body.child(
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("样例预览"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(draft.preview.clone()),
            ),
    );
    // 错误行
    if let Some(error) = &draft.error {
        body = body.child(
            div().text_xs().text_color(theme.colors.danger).child(error.clone()),
        );
    }
    body.into_any_element()
}

// ---------- 小工具 ----------

/// spec → 产出数据类（`default_of` 理论上对该 spec 必返回 `Some`；`None` 只是防御）。
fn data_type_for_spec(spec: &'static GeneratorSpec) -> ColumnDataType {
    match generator_catalog::default_of(spec.name) {
        Some(config) => crate::generators::data_type_for_generator(&config),
        None => ColumnDataType::Varchar { length: None },
    }
}

/// 搜索匹配：名称 / 中文标签 / 分类标签（大小写不敏感）。
fn spec_matches(spec: &'static GeneratorSpec, query: &str) -> bool {
    let q = query.to_lowercase();
    spec.name.to_lowercase().contains(&q)
        || spec.label.to_lowercase().contains(&q)
        || spec.category.label().to_lowercase().contains(&q)
}
