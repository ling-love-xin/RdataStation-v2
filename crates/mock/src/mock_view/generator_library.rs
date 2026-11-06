//! 双视角生成器库对话框 + 分组「＋」自定义生成器精调层（§9）。
//!
//! # 结构与入口
//!
//! - 打开：列编辑对话框的左栏（内嵌浏览区）；自定义生成器走 [`open_create_dialog`] 独立窗口；
//! - 浏览：顶部搜索框 + `TabBar` 双视角（业务分类 / 数据类型）+ 分组行（`Button` 展开/收起 +
//!   行尾「＋」`Button`，键盘可达）+ 展开的生成器项（内建在前、自定义在后，点击写回列并关对话框）；
//! - 精调层走**独立窗口**（`open_create_dialog`）：点「＋」开一个 560px 对话框，
//!   保存成功后关窗口。精调层三个下拉用 `select::{Select, SelectState}` + `SearchableVec`——
//!   **不用** `Button`+`dropdown_menu`：vendor `dropdown_button.rs:207` 触发器 id 硬编码 `"popup"`，
//!   同帧只允许一个 DropdownButton，而这里一屏要三个下拉。
//!
//! # 数据模型
//!
//! 自定义生成器 = **命名变体**（已有生成器 + 参数 + 名称），**不是**新 `GeneratorConfig` 变体
//! （`spec_of` / `generate_cell` 编译期穷尽 match，运行时新算法不成立）。保存时把参数 patch 回
//! 基础生成器的默认配置，产出 [`UserGenerator`]（硬属性 `data_type` + 可选 `category`，双向挂组）。

use std::collections::HashSet;

use super::*;
use fake::rand::SeedableRng as _;
use gpui_kit::component::Theme;
use gpui_kit::component::searchable_list::{
    SearchableListDelegate, SearchableListItem, SearchableVec,
};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};

use crate::mock_view::panel::MockPanel;
use crate::mock_view::pure::{
    column_type_label, complex_param_hint, complex_param_text, param_text, parse_complex_param,
    patch_param, patch_param_value,
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

/// 生成器选择的写回目标：列编辑对话框里正在编辑的**那一列表配置**。
///
/// 此前还有 `Panel`（直接写面板列）——那是字段行那个独立「生成器库」窗口的写回目标。
/// 入口合并进列编辑后，选生成器一律先落在表配置、由「应用」落地：两处口径统一，
/// 也不再留下「点一下就把表改了，反悔只能再选回去」的差别。
#[derive(Clone)]
pub(crate) struct LibraryWriteback {
    /// 列编辑视图（弱引用：选生成器的过程中这个 tab 可能已经被关掉）
    pub(crate) detail: gpui::WeakEntity<MockDetailView>,
    pub(crate) column_id: u64,
}

/// 生成器浏览 / 精调的状态（实体由 [`create_library_state`] 创建；builder 每帧从它读，写入走事件路径）。
pub(crate) struct GeneratorLibraryState {
    pub(crate) panel: Entity<MockPanel>,
    pub(crate) writeback: LibraryWriteback,
    /// 内嵌模式（列编辑对话框内联浏览区）：无对话框壳，选中即时写回表配置并留在宿主；
    /// 独立的精调层窗口（自定义生成器）为 false。
    pub(crate) embedded: bool,
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
    /// 精调层表配置（`mode=Create` 时非空）
    pub(crate) create: Option<CreateDraft>,
    /// 编辑中的自定义生成器 id（`Some` = 精调层是编辑而非新建；保存走 `update_user_generator`）
    pub(crate) editing: Option<String>,
    _search_sub: Option<Subscription>,
}

/// 精调层表配置（分组「＋」进入时构建，保存成功后丢弃）。
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
    pub(crate) category_state: Option<Entity<SelectState<SearchableVec<CategoryChoice>>>>,
    /// 基础生成器下拉（技术组进入时按锁定类型过滤；项是 `&'static GeneratorSpec`；
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
        DraftParam::Scalar { key, .. }
        | DraftParam::Bool { key, .. }
        | DraftParam::Complex { key, .. } => key,
    }
}

// ---------- 打开入口 ----------

/// 创建生成器状态实体（浏览 + 搜索 + 精调层共用）。
///
/// `embedded=true` 供列编辑对话框内联（`detail.rs`）：无对话框壳，写回表配置后留在列编辑；
/// `embedded=false` 只用于 [`open_create_dialog`] 的独立精调层窗口。
///
/// 列名输入框不在这里：内嵌时列编辑对话框自己有，独立窗口是建生成器（与被编辑的列无关）。
pub(crate) fn create_library_state(
    panel: Entity<MockPanel>,
    writeback: LibraryWriteback,
    embedded: bool,
    // 当前这一列用的生成器（只用于「打开时就展开它所在组」）；独立精调窗口传 None。
    // 不能在这里读 `writeback.detail`：列编辑对话框打开时正持有该视图的 update，
    // 同栈再 `read` 会 panic（cannot read … while it is already being updated）。
    initial_generator: Option<GeneratorConfig>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<GeneratorLibraryState> {
    let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索定位生成器…"));
    // 打开时就把「当前这一列用的生成器」所在分组展开：15 个组全收起时，用户得先猜
    // 它属于哪一类再展开，等于每次都要重新找一遍自己正在用的东西。
    // （不做滚动定位：`ScrollHandle` 只有顶部/底部滚动，没有按元素滚动，靠搜索框补）
    let (mut expanded, mut expanded_types) = (HashSet::new(), HashSet::new());
    if let Some(generator) = initial_generator {
        let spec = generator_catalog::spec_of(&generator);
        expanded.insert(spec.category.label().to_string());
        expanded_types.insert(data_type_key(&data_type_for_spec(spec)).to_string());
    }
    let state = cx.new(|_cx| GeneratorLibraryState {
        panel: panel.clone(),
        writeback,
        embedded,
        mode: LibraryMode::Browse,
        view: LibraryView::Business,
        query: String::new(),
        expanded,
        expanded_types,
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
            move |s, emitter, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    s.query = emitter.read(cx).value().to_string();
                    cx.notify();
                } else if matches!(ev, InputEvent::PressEnter { .. }) {
                    // 搜索定位：Enter 直接采纳当前视角下第一个命中项（写回表配置/面板，
                    // 与点击列表项同一入口 `apply_picked_generator`；空搜索词不采纳）
                    if s.query.trim().is_empty() {
                        return;
                    }
                    let panel = s.panel.clone();
                    let writeback = s.writeback.clone();
                    let first = first_match_for(&s.view, &s.query, &panel.read(cx));
                    if let Some((picked, is_user)) = first {
                        apply_picked_generator(cx, window, &panel, &writeback, &picked, is_user);
                    }
                }
            },
        );
        s._search_sub = Some(_search_sub);
    });
    state
}

/// 当前视角 + 搜索词下「第一个命中」的生成器（搜索框 Enter 采纳用）。
///
/// 与列表渲染同一套顺序：业务视角按 `GeneratorCategory::ALL` 分类序、内建在前自定义在后；
/// 技术视角按 `COLUMN_TYPES` 类型序，内建只算**方法级**（`is_method_spec`），自定义项始终参与。
/// 空搜索词返回 `None`（没有「当前项」可采纳）。
pub(crate) fn first_match_for(
    view: &LibraryView,
    query: &str,
    panel: &MockPanel,
) -> Option<(String, bool)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let ug_matches = |ug: &UserGenerator| {
        ug.name.to_lowercase().contains(&q)
            || crate::mock_view::pure::column_type_label(&ug.data_type)
                .to_lowercase()
                .contains(&q)
    };
    let mut hit: Option<(String, bool)> = None;
    match view {
        LibraryView::Business => {
            for category in GeneratorCategory::ALL {
                if let Some(spec) = generator_catalog::specs_in(category)
                    .into_iter()
                    .find(|spec| spec_matches(spec, query))
                {
                    hit = Some((spec.name.to_string(), false));
                    break;
                }
                if let Some(ug) = panel
                    .user_generators_in_category(category)
                    .into_iter()
                    .find(|ug| ug_matches(ug))
                {
                    hit = Some((ug.id.clone(), true));
                    break;
                }
            }
        }
        LibraryView::Tech => {
            for data_type in COLUMN_TYPES {
                if let Some(spec) = generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .filter(|spec| generator_catalog::is_method_spec(spec))
                    .filter(|spec| data_type_for_spec(spec) == data_type)
                    .find(|spec| spec_matches(spec, query))
                {
                    hit = Some((spec.name.to_string(), false));
                    break;
                }
                if let Some(ug) = panel
                    .user_generators_in_type(&data_type)
                    .into_iter()
                    .find(|ug| ug_matches(ug))
                {
                    hit = Some((ug.id.clone(), true));
                    break;
                }
            }
        }
    }
    hit
}

/// 精调层（自定义生成器）以什么姿态打开：新建（可锁一个维度）/ 编辑（两个维度都锁）。
pub(crate) enum CreateSeed {
    /// 新建：业务组「＋」→ 锁业务组、类型可自选；技术组「＋」→ 锁类型、业务组可自选
    New {
        locked_type: Option<ColumnDataType>,
        locked_category: Option<GeneratorCategory>,
    },
    /// 编辑：被编辑项的 类型 / 业务组 / 基础生成器 全部锁定（换基础 = 换个生成器）
    Edit { ug: UserGenerator },
}

/// 打开「自定义生成器」独立对话框（列编辑内嵌浏览区的分组「＋」/ 自定义项「编辑」）。
///
/// 为什么不在内嵌的那个 state 上切 `mode`：内嵌左栏只有 280px，精调层表单
/// （名字 / 类型 / 业务组 / 基础生成器 / 参数 / 样例）塞进去会挤成上下两段，
/// 而列编辑对话框本身是 840px 的两栏——在它里面再叠一层表单会把右栏挤掉。
/// 单独开一个 560px 窗口：表单完整，列编辑那边也不用让位。
pub(crate) fn open_create_dialog(
    panel: Entity<MockPanel>,
    writeback: LibraryWriteback,
    seed: CreateSeed,
    window: &mut Window,
    cx: &mut App,
) {
    let state = create_library_state(panel, writeback, false, None, window, cx);
    state.update(cx, |s, cx| {
        s.mode = LibraryMode::Create;
        match &seed {
            CreateSeed::New {
                locked_type,
                locked_category,
            } => {
                s.create = Some(new_create_draft(
                    s.panel.clone(),
                    locked_type.clone(),
                    *locked_category,
                    window,
                    cx,
                ));
            }
            CreateSeed::Edit { ug } => {
                s.editing = Some(ug.id.clone());
                s.create = Some(new_edit_draft(s.panel.clone(), ug, window, cx));
            }
        }
        s.rebuild_params(window, cx);
        s.refresh_preview(cx);
        cx.notify();
    });

    window.open_dialog(cx, move |dialog, window, cx| {
        let theme = cx.theme().clone();
        let editing = state.read(cx).editing.is_some();
        dialog
            .title(if editing {
                "编辑自定义生成器"
            } else {
                "新建自定义生成器"
            })
            .width(ui::GENERATOR_DIALOG_WIDTH)
            .child(render_create(&state, &theme, window, cx))
            .footer(render_create_footer(&state, window, cx))
    });
}

// ---------- 浏览模式 ----------

/// 把「被点中的那一项」解析成（可写回的生成器配置，自定义项 id）。
///
/// 键的规范是：内建 = `spec.name`，自定义 = `ug.id`。这里再兜一层显示名
/// （`spec.label` / `ug.name`）：列表曾经把中文显示名当键传下去，目录按 name 查不到，
/// 于是**静默什么都不做**——用户看到的就是「点左边列表，右边一点反应都没有」。
/// 兜底让这类错误退化成「仍然生效」，而不是无声失败。
///
/// 第二个返回值是**用户真正点中的自定义项 id**（内建为 `None`）：`def.generator` 只存
/// 解包后的基础配置，同一 base 的多个自定义项在它上面完全一样，只有这个 id 能说清
/// 「左栏亮的是哪一项 / 右栏该显示哪个名字」。
pub(crate) fn picked_entry(
    panel: &MockPanel,
    picked: &str,
    is_user: bool,
) -> Option<(GeneratorConfig, Option<String>)> {
    if is_user {
        panel
            .user_generators
            .iter()
            .find(|ug| ug.id == picked)
            .or_else(|| panel.user_generators.iter().find(|ug| ug.name == picked))
            .map(|ug| (ug.base.clone(), Some(ug.id.clone())))
    } else {
        generator_catalog::default_of(picked)
            .or_else(|| {
                generator_catalog::all_specs()
                    .iter()
                    .copied()
                    .find(|spec| spec.label == picked)
                    .and_then(|spec| generator_catalog::default_of(spec.name))
            })
            .map(|config| (config, None))
    }
}

/// 只要可写回的配置（不需要自定义项身份时用；测试与外部调用沿用这个入口）。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn picked_config(
    panel: &MockPanel,
    picked: &str,
    is_user: bool,
) -> Option<GeneratorConfig> {
    picked_entry(panel, picked, is_user).map(|(config, _)| config)
}

fn apply_picked_generator(
    app: &mut App,
    window: &mut Window,
    panel: &Entity<MockPanel>,
    writeback: &LibraryWriteback,
    picked: &str,
    is_user: bool,
) {
    // 内建取规格默认配置；自定义解包 base（与 `apply_user_generator` 同源），
    // 同时带出「点的是哪一项」的自定义 id。
    // 解析放在独立作用域里，避免 `panel.read` 的借用活到 `update` 那一步
    let entry = {
        let panel_ref = panel.read(app);
        picked_entry(&panel_ref, picked, is_user)
    };
    let Some((config, picked_user)) = entry else {
        return;
    };
    let _ = writeback.detail.update(app, |v, cx| {
        v.apply_generator_config_to_draft(
            writeback.column_id,
            config,
            picked_user.clone(),
            window,
            cx,
        );
    });
    // 记一笔历史：自定义项按 `ug.id` 记，历史 chips 才认得回来（内建那两个写回通道
    // `set_generator` / `set_generator_config` 自己会记 `spec.name`）。
    // 用解析出来的真实 id：键可能是显示名兜底命中的。
    if let Some(user_id) = picked_user {
        panel.update(app, |panel, _cx| panel.remember_generator(&user_id, true));
    }
}

/// 顶部搜索定位输入框（横跨对话框全宽；词过滤左栏分组项，见 [`render_browse_pane`]）。
pub(crate) fn render_search_box(
    state: &Entity<GeneratorLibraryState>,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let search = state.read(cx).search_input.clone();
    div()
        .id("mock-lib-search")
        .test_support()
        .w_full()
        .child(Input::new(&search))
        .into_any_element()
}

/// 「智能推荐」按钮：按列名 + 类型重推这一列的生成器（与手选并列的一条捷径）。
///
/// 位置由调用方决定：列编辑对话框把它放在行 2 的「智能推荐」块里（与「历史生成器」
/// 并排），不再塞在库列表顶部——它作用的是「这一列怎么造」，不是「库里翻哪一项」。
pub(crate) fn render_smart_button(
    state: &Entity<GeneratorLibraryState>,
    _theme: &Theme,
    _window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let writeback = state.read(cx).writeback.clone();
    let Some(detail) = writeback.detail.upgrade() else {
        return div().into_any_element();
    };
    let column_id = writeback.column_id;
    div()
        // 外层块自己的 id（不与按钮同 id：同帧两个相同 ElementId 会撞 a11y 校验）。
        // 宽度按内容收（`flex_none`）：行 2 是横向单行，`w_full` 会把右侧「历史」挤成 0 宽
        .id("mock-lib-smart-block")
        .test_support()
        .flex_none()
        .child(
            button("mock-lib-smart", "智能推荐", ActionKind::Quiet)
                .xsmall()
                .tooltip("按列名与类型重推：回到推荐的生成器、参数与空值率")
                .tooltip_show_delay(TOOLTIP_DELAY)
                .on_click(move |_event, window, app| {
                    let _ = detail.update(app, |view, cx| {
                        view.smart_reset(column_id, window, cx);
                    });
                }),
        )
        .into_any_element()
}

/// TabBar 双视角 + 分组列表（业务视角：15 分类的**生成器**；技术视角：13 数据类下的
/// **方法**——两边不是同一批东西的两种排法，`is_method_spec` 决定技术视角只露方法）。
///
/// 只负责「库本体」：智能推荐与历史生成器已上移到列编辑对话框的行 2，这里不再渲染，
/// 列表超高时由本块的 `flex_1().min_h_0()` 内部滚动。
/// 当前列编辑表配置用的生成器（左栏选中态用）+ 用户点中的自定义项 id。
///
/// 二者要一起读：`def.generator` 只说明「用的哪个算法」，同一 base 的多个自定义项在它上面
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
                            .pl(px(20.))
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
                            .pl(px(20.))
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
fn render_type_group_row(
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

fn render_item_row(
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

// ---------- 精调层（创建自定义生成器） ----------

/// 新建精调层表配置：锁定一个维度（类型或业务组），另一维可自选，基础生成器按维度过滤。
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
    category_items.extend(
        GeneratorCategory::ALL
            .iter()
            .copied()
            .map(|c| CategoryChoice(Some(c))),
    );
    let category_state = locked_category.as_ref().map(|_| {
        let delegate = SearchableVec::new(category_items);
        cx.new(|cx| SelectState::new(delegate, None, window, cx))
    });
    // 基础生成器：候选 = 类型兼容的**方法级**生成器（技术组进入按锁定类型过滤，业务组
    // 进入不锁类型 = 全部方法）。业务生成器（姓名/地址等）不是「基础方法」，不进精调层
    // 兜底列表——要微调业务生成器直接在库列表里选中即可，不必再包一层命名变体（#1 定稿：
    // 技术视角只留方法，精调层基础候选与之一致）
    let base_items: Vec<&'static GeneratorSpec> = generator_catalog::all_specs()
        .iter()
        .copied()
        .filter(|spec| generator_catalog::is_method_spec(spec))
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
    // 基础生成器身份变化 → 重建参数行 + 刷样例（订阅一次，表配置生命周期内常驻；
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

/// 编辑精调层表配置：复用创建表配置（类型 / 业务组锁定 = 被编辑项的值），再覆写为编辑态——
/// 基础生成器锁定（编辑不改 base：换 base = 换个生成器，删了重建更直接）、参数区 / 样例以
/// 被编辑项**保存过的配置**为基准（`preset`，用户改过的参数值在编辑重开时不丢）、名字预填。
fn new_edit_draft(
    panel: Entity<MockPanel>,
    ug: &UserGenerator,
    window: &mut Window,
    cx: &mut Context<GeneratorLibraryState>,
) -> CreateDraft {
    let mut draft = new_create_draft(
        panel.clone(),
        Some(ug.data_type.clone()),
        ug.category,
        window,
        cx,
    );
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
        // 每个参数一个订阅：Change → 写回表配置值 + 刷样例。Bool 用 Switch（不再退化成文本框）。
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
    fn commit_scalar(&mut self, key: &str, text: &str, kind: ParamKind, cx: &mut Context<Self>) {
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
    let (name, Some(data_type), category, Some(config)) = (name, data_type, category, config)
    else {
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
            // 保存即应用（与「保存双向挂组」一致：创建/编辑完直接写回当前列/表配置）
            let panel = state.read(cx).panel.clone();
            let writeback = state.read(cx).writeback.clone();
            let embedded = state.read(cx).embedded;
            apply_picked_generator(cx, window, &panel, &writeback, &ug_id, true);
            state.update(cx, |s, cx| {
                s.mode = LibraryMode::Browse;
                s.create = None;
                s.editing = None;
                cx.notify();
            });
            // 内嵌模式（列编辑内联）：写回表配置后留在列编辑；弹对话框模式才关
            if !embedded {
                window.close_dialog(cx);
            }
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
            button("lib-create-cancel", "取消", ActionKind::Soft).on_click(
                move |_event, _window, app| {
                    state_for_cancel.update(app, |s, cx| {
                        s.mode = LibraryMode::Browse;
                        s.create = None;
                        s.editing = None;
                        cx.notify();
                    });
                },
            ),
        )
        .child(
            // 对话框的主行动：进来就是为了存下这一个生成器
            button(
                "lib-create-save",
                if editing {
                    "保存修改"
                } else {
                    "保存并应用"
                },
                ActionKind::Main,
            )
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
    kv(theme, label, div().flex_1().min_w_0().child(select)).into_any_element()
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
    // 类型行：锁定 → 只读文本；未锁定 → Select（用户定稿：字段名「产出类型」）
    body = match &draft.locked_type {
        Some(t) => body.child(
            div().h_flex().items_center().gap_2().w_full().child(kv(
                &theme,
                "产出类型",
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(column_type_label(t)),
            )),
        ),
        None => {
            if let Some(type_state) = &draft.type_state {
                body.child(select_line(
                    &theme,
                    "产出类型",
                    Select::new(type_state)
                        .menu_width(px(260.))
                        .menu_max_h(px(220.))
                        .placeholder("选择产出类型…")
                        .accessibility_label("产出类型"),
                ))
            } else {
                body
            }
        }
    };
    // 业务组行：锁定 → 只读文本；未锁定 → Select（含「（不分组）」）
    body = match &draft.locked_category {
        Some(c) => body.child(
            div().h_flex().items_center().gap_2().w_full().child(kv(
                &theme,
                "业务组",
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground)
                    .child(c.label()),
            )),
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
                .child(kv(
                    &theme,
                    "基础生成器",
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(spec.label),
                )),
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
            .child(section(
                &theme,
                if has_params {
                    "参数（来自基础生成器）"
                } else if base_ready {
                    "参数（该生成器没有参数）"
                } else {
                    "参数（选择基础生成器后出现）"
                },
            ))
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
                                DraftParam::Scalar {
                                    state: input_state, ..
                                } => form_line(&theme2, &label, input_state).into_any_element(),
                                DraftParam::Complex {
                                    state: text_state, ..
                                } => {
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
    body = body.child(kv(
        &theme,
        "样例预览",
        div()
            .text_xs()
            .text_color(theme.colors.foreground)
            .child(draft.preview.clone()),
    ));
    // 错误行
    if let Some(error) = &draft.error {
        body = body.child(error_line(&theme, error.clone()));
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

/// 数据视角的唯一键：`column_type_label` 会撞名（`Text` / `Uuid` → `VARCHAR`，
/// `DateTime` / `Timestamp` → `TIMESTAMP`），而 ElementId 与 `expanded_types`
/// 都要求唯一——重复 id 会让 GPUI 的 a11y 节点校验直接 panic（切「数据类型」闪退）。
pub(crate) fn data_type_key(data_type: &ColumnDataType) -> &'static str {
    match data_type {
        ColumnDataType::Integer => "integer",
        ColumnDataType::BigInt => "big_int",
        ColumnDataType::Float => "float",
        ColumnDataType::Double => "double",
        ColumnDataType::Decimal { .. } => "decimal",
        ColumnDataType::Boolean => "boolean",
        ColumnDataType::Varchar { .. } => "varchar",
        ColumnDataType::Text => "text",
        ColumnDataType::Date => "date",
        ColumnDataType::DateTime => "date_time",
        ColumnDataType::Timestamp => "timestamp",
        ColumnDataType::Uuid => "uuid",
        ColumnDataType::Blob => "blob",
    }
}

/// 数据视角分组标题：可读且**唯一**（括号内保留 DuckDB 类型，便于和产出对齐）。
///
/// 直接用 `column_type_label` 会让三个组都叫 `VARCHAR`、两个组都叫 `TIMESTAMP`，
/// 用户分不清这是不同的数据类还是重复渲染。
/// 前缀「产出」：明确这是"产出这个类型的配方"，不是在选数据类型本身。
pub(crate) fn data_type_title(data_type: &ColumnDataType) -> String {
    let cn = match data_type {
        ColumnDataType::Integer => "整数",
        ColumnDataType::BigInt => "长整数",
        ColumnDataType::Float => "浮点",
        ColumnDataType::Double => "双精度",
        ColumnDataType::Decimal { .. } => "定点数",
        ColumnDataType::Boolean => "布尔",
        ColumnDataType::Varchar { .. } => "变长文本",
        ColumnDataType::Text => "长文本",
        ColumnDataType::Date => "日期",
        ColumnDataType::DateTime => "日期时间",
        ColumnDataType::Timestamp => "时间戳",
        ColumnDataType::Uuid => "UUID",
        ColumnDataType::Blob => "二进制",
    };
    format!("产出{cn}（{}）", column_type_label(data_type))
}

/// 搜索匹配：名称 / 中文标签 / 分类标签（大小写不敏感）。
fn spec_matches(spec: &'static GeneratorSpec, query: &str) -> bool {
    let q = query.to_lowercase();
    spec.name.to_lowercase().contains(&q)
        || spec.label.to_lowercase().contains(&q)
        || spec.category.label().to_lowercase().contains(&q)
}
