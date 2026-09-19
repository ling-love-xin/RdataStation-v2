use super::*;

/// 打开管理器覆盖层（嵌套 Dialog；kind: 0 认证 / 1 网络 / 2 环境）。
/// 组件默认集（`IconName`）不含数据库 / 锁 / 盾牌等图标；应用注册的是全量
/// Lucide 目录（`gpui_kit::assets::AllAssets`），故按资产路径直接加载。
pub(crate) fn lucide(path: &'static str) -> Icon {
    Icon::empty().path(path)
}

/// 设置输入框的值（仅在变化时写入，避免无谓的 notify / 渲染）。
pub(crate) fn set_input_value(
    target: &Entity<InputState>,
    value: String,
    window: &mut Window,
    cx: &mut App,
) {
    if target.read(cx).value().as_ref() != value.as_str() {
        target.update(cx, |s, cx| s.set_value(value, window, cx));
    }
}

/// 设置字符串型 Select 的选中值（空字符串 → 清空选中；暂存列表载入用）。
pub(crate) fn set_select_value(
    sel: &Entity<SelectState<SearchableVec<SharedString>>>,
    value: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let v = value.trim().to_string();
    sel.update(cx, |s, cx| {
        if v.is_empty() {
            s.set_selected_index(None, window, cx);
        } else {
            s.set_selected_value(&SharedString::from(v), window, cx);
        }
    });
}

// ===== UI 尺寸约束（数值登记在 `crate::ui`，见 docs/architecture/ui/ui-design-spec.md）=====
//
// 颜色一律 `theme.colors` token，尺寸一律引用 `ui.rs` 常量或 Tailwind 尺度方法；
// 本模块只做别名（保持既有调用点可读），不再自己声明数值。

pub(crate) use crate::ui::{
    DIALOG_BADGE_HEIGHT as BADGE_H, DIALOG_BADGE_WIDTH as BADGE_W,
    DIALOG_BODY_HEIGHT as BODY_H, DIALOG_DRIVER_WIDTH as DRIVER_W,
    DIALOG_FORM_LABEL_WIDTH as LABEL_COL_W, DIALOG_PROJECT_WIDTH as PROJECT_W,
    DIALOG_ROW_HEIGHT as ROW_H, DIALOG_STAGING_HEIGHT as STAGING_H, GAP_LG, GAP_MD, GAP_SM,
};

/// Header 标签列宽（rem）：刚好容纳两字标签（名称 / 备注 / 驱动 / 地址），
/// 不让标签与控件之间留下过大的空白（真机反馈：间距过大）。
pub(crate) const LABEL_W: f32 = 1.75;

/// Header 统一标签列（固定宽度，保证各行标签左对齐，减少视觉磕绊）。
pub(crate) fn header_label(theme: &Theme, text: &'static str) -> Div {
    div()
        .w(rems(LABEL_W))
        .flex_shrink_0()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text)
}

// ===== 标签文本 ↔ JSON（UI 逗号分隔 ⇄ 落库 / 回读）=====

/// 标签文本 → JSON 数组（支持中英文逗号；空列表返回 None，避免写入空串）。
pub(crate) fn tags_to_json(text: &str) -> Option<String> {
    let mut list: Vec<String> = Vec::new();
    for part in text.split([',', '，']) {
        let t = part.trim();
        if !t.is_empty() && !list.iter().any(|x| x == t) {
            list.push(t.to_string());
        }
    }
    if list.is_empty() {
        None
    } else {
        serde_json::to_string(&list).ok()
    }
}

/// JSON 数组 → 标签文本（回读回显；解析失败返回空串）。
pub(crate) fn tags_from_json(json: Option<&str>) -> String {
    json.and_then(|t| serde_json::from_str::<Vec<String>>(t).ok())
        .map(|list| list.join(", "))
        .unwrap_or_default()
}

// ===== 对话框 Tab 定义与可见下标映射（#15：TabBar 迁移后的可测接缝）=====

/// Tab 定义（显示名, **内部 Tab 索引**）。
///
/// 文件型驱动（SQLite / DuckDB）按原型隐藏「网络」Tab——内部索引保持不变，
/// 所以 `active_tab` 与内容分支（0 常规 / 1 网络 / 2 能力 / 3 驱动属性 / 4 高级）不受影响。
pub(crate) fn dialog_tab_defs(is_file_db: bool) -> Vec<(&'static str, usize)> {
    if is_file_db {
        vec![("常规", 0), ("能力", 2), ("驱动属性", 3), ("高级", 4)]
    } else {
        vec![
            ("常规", 0),
            ("网络", 1),
            ("能力", 2),
            ("驱动属性", 3),
            ("高级", 4),
        ]
    }
}

/// 内部 Tab 索引 → TabBar 的**可见下标**（隐藏「网络」后两者不相等，决策 #85）。
pub(crate) fn visible_tab_index(defs: &[(&'static str, usize)], active: usize) -> usize {
    defs.iter().position(|(_, ix)| *ix == active).unwrap_or(0)
}

// ===== 数据库类型 / 驱动展示（Header 去噪 + 条目类型徽标）=====

/// 驱动实现短名：`MySQL (sqlx)` → `sqlx`；无括号（或括号内为空）时返回原值。
///
/// 数据库类型已在左侧栏选定，Header 下拉只需呈现实现差异（sqlx / Official /
/// rusqlite / duckdb-rs），避免每项重复携带类型名。
/// 同一类型下驱动短名唯一（如 MySQL 下 `sqlx` 与 `Official`），跨类型同名不冲突——
/// 下拉选项按当前类型过滤。
pub(crate) fn driver_short_name(name: &str) -> String {
    let trimmed = name.trim();
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let Some(start) = trimmed.find(open) {
            if let Some(rel_end) = trimmed[start + open.len_utf8()..].find(close) {
                let inner =
                    trimmed[start + open.len_utf8()..start + open.len_utf8() + rel_end].trim();
                if !inner.is_empty() {
                    return inner.to_string();
                }
            }
        }
    }
    trimmed.to_string()
}

/// 解析驱动声明的属性默认值（`drivers.driver_properties`：SQL 对象 → **按键排序**的键值列表）。
///
/// 空 / 非法 JSON / 空键 → 空列表（**不造默认值**）：属性页的默认值必须来自驱动声明
/// （`descriptors.rs` 里各驱动只声明该客户端库真认的键），UI 编造一个键就是“写了不生效”
/// 或“写了就连不上”（能力矩阵 §2.1 的「未知参数」列）。
/// 非字符串值按 JSON 文本给出（与落库路径 `connection_service::apply_driver_properties` 同口径）。
pub(crate) fn driver_property_defaults(json: Option<&str>) -> Vec<(String, String)> {
    let Some(map) = json.and_then(|s| {
        serde_json::from_str::<std::collections::BTreeMap<String, serde_json::Value>>(s).ok()
    }) else {
        return Vec::new();
    };
    map.into_iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| {
            let value = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            (k, value)
        })
        .collect()
}

/// 属性行「去向」提示的严重程度（决定颜色：Info = 弱化 / Warn = 警告 / Danger = 危险）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertyNoteLevel {
    Info,
    Warn,
    Danger,
}

/// 属性行下方的「去向」提示：这个键到底会怎样（`None` = 按原样下发且无需说明）。
///
/// 判据全部来自引擎的 [`engine::driver::property_spec`]（依据客户端库源码，唯一真相源）——
/// 这里只把判决翻译成一句人话，不自己判断键的真伪（§15：UI 不造数据）。
pub(crate) fn property_note(
    driver_id: &str,
    key: &str,
) -> Option<(String, PropertyNoteLevel)> {
    match engine::driver::driver_property_verdict(driver_id, key) {
        engine::driver::PropertyVerdict::Delivered {
            param,
            label,
            note,
            caution,
        } => {
            let renamed = param != key.trim();
            if !renamed && note.is_none() && caution.is_none() {
                // 原名直通、没有要说明的 → 不必给提示（不啰嗝）
                return None;
            }
            let mut text = if renamed {
                format!("下发为 {param}")
            } else {
                "会下发".to_string()
            };
            if let Some(l) = label {
                text = format!("{l} · {text}");
            }
            // 副作用（会覆盖别的设置）按警告色；中性说明按弱化色
            match (caution, note) {
                (Some(c), _) => Some((format!("{text}（{c}）"), PropertyNoteLevel::Warn)),
                (None, Some(n)) => Some((format!("{text}（{n}）"), PropertyNoteLevel::Info)),
                (None, None) => Some((text, PropertyNoteLevel::Info)),
            }
        }
        engine::driver::PropertyVerdict::DriverSide {
            applied_as,
            label,
            note,
            caution,
        } => {
            let renamed = applied_as != key.trim();
            let mut text = if renamed {
                format!("由驱动应用为 {applied_as}")
            } else {
                "由驱动应用".to_string()
            };
            if let Some(l) = label {
                text = format!("{l} · {text}");
            }
            match (caution, note) {
                (Some(c), _) => Some((format!("{text}（{c}）"), PropertyNoteLevel::Warn)),
                (None, Some(n)) => Some((format!("{text}（{n}）"), PropertyNoteLevel::Info)),
                (None, None) => Some((text, PropertyNoteLevel::Info)),
            }
        }
        engine::driver::PropertyVerdict::Unknown { effect } => Some(match effect {
            engine::driver::UnknownEffect::Ignored => (
                "当前实现不认这个键，不会被应用（写了不生效）".to_string(),
                PropertyNoteLevel::Warn,
            ),
            engine::driver::UnknownEffect::ConnectionError => (
                "当前实现不认这个键：连接会因未知参数**直接报错**".to_string(),
                PropertyNoteLevel::Danger,
            ),
        }),
        engine::driver::PropertyVerdict::Unclassified => Some((
            "未收录该驱动的属性规格，去向未知（按原样下发）".to_string(),
            PropertyNoteLevel::Info,
        )),
    }
}

/// 指定数据库类型下的启用驱动（保持 drivers 目录顺序）。
pub(crate) fn enabled_drivers_of_type(drivers: &[Driver], type_id: &str) -> Vec<Driver> {
    drivers
        .iter()
        .filter(|d| d.enabled && d.type_id == type_id)
        .cloned()
        .collect()
}

// ===== 驱动能力 / 环境策略（**清单与值来自数据库**，这里只放标签字典）=====
//
// 审计约定（见 connection-dialog-architecture §15）：UI 不自行编造业务数据——
// 能力清单取 `drivers.capabilities`、环境策略取 `environment_policies`；
// 本段只负责「键/类型 → 中文标签」的展示映射（字典），以及 JSON 解析工具。

/// 驱动能力键 → 中文标签的字典**已上收到引擎**（`engine::driver::CAPABILITY_DICTIONARY`）：
/// 那里同时定义「键 → 运行时能力位」与「真机验收状态」，避免界面另存一份键表
/// （本段只留 JSON 解析与行组装）。

/// 能力矩阵行：标签 + 是否由该驱动声明 + 运行时位 + 真机验收状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilityRow {
    pub label: String,
    /// 驱动是否声明了该能力（`drivers.capabilities`）。
    pub declared: bool,
    /// 对应的运行时能力位（无 → 纯界面能力）。
    pub meta_bit: Option<engine::driver::MetaBit>,
    /// 真机验收状态（D10：未验收不算可用，界面如实标注）。
    pub acceptance: engine::driver::Acceptance,
}

/// 解析驱动能力 JSON 数组（`drivers.capabilities`）；非法 / 为空 → 空列表。
pub(crate) fn driver_capabilities(json: Option<&str>) -> Vec<String> {
    json.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 能力矩阵行（**字典唯一来源**：`engine::driver::CAPABILITY_DICTIONARY`）。
///
/// 字典内的**驱动能力**键逐个出一行（声明与否均可视），并带出运行时位与真机验收状态；
/// 驱动声明了字典以外的键时追加在尾部（键名原样，`declared = true`），
/// 保证「驱动新增能力但字典未收录」时也不丢信息。
///
/// **只列 `Scope::Driver` 的键**：应用级功能（导出 / Mock 生成 / 资源分析）与驱动无关，
/// 在这里逐行列出来会被读成「该驱动不支持它」（假信息）——它们走 [`app_level_capabilities`]。
pub(crate) fn capability_rows(declared: &[String]) -> Vec<CapabilityRow> {
    let mut rows: Vec<CapabilityRow> = engine::driver::driver_capability_keys()
        .iter()
        .map(|spec| CapabilityRow {
            label: spec.label.to_string(),
            declared: declared.iter().any(|d| d == spec.key),
            meta_bit: spec.meta_bit,
            acceptance: spec.acceptance,
        })
        .collect();
    for key in declared {
        if engine::driver::capability_spec(key).is_none() {
            rows.push(CapabilityRow {
                label: key.clone(),
                declared: true,
                meta_bit: None,
                acceptance: engine::driver::Acceptance::unverified(),
            });
        }
    }
    rows
}

/// 应用级功能的中文标签（与驱动无关；能力 Tab 用一句说明带过）。
pub(crate) fn app_level_capabilities() -> Vec<String> {
    engine::driver::app_level_capability_keys()
        .iter()
        .map(|s| s.label.to_string())
        .collect()
}

/// 解析驱动声明的认证方法（`drivers.supported_auth_types`，JSON 数组）。
///
/// UI 的「认证方法」下拉即以此为准（数据源：驱动表）；驱动未声明 → 空列表（下拉置灰）。
pub(crate) fn driver_auth_types(json: Option<&str>) -> Vec<String> {
    json.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 环境策略类型 → 中文标签（未收录的类型原样展示）。
pub(crate) fn policy_type_label(policy_type: &str) -> String {
    match policy_type {
        "security" => "安全策略".to_string(),
        "schema" => "模式加载".to_string(),
        "performance" => "性能策略".to_string(),
        "audit" => "审计策略".to_string(),
        "ui" => "界面策略".to_string(),
        other => other.to_string(),
    }
}

/// 中文标签 → 策略类型（环境管理器落库用；未知标签原样返回）。
pub(crate) fn policy_type_from_label(label: &str) -> String {
    match label {
        "安全策略" => "security".to_string(),
        "模式加载" => "schema".to_string(),
        "性能策略" => "performance".to_string(),
        "审计策略" => "audit".to_string(),
        "界面策略" => "ui".to_string(),
        other => other.to_string(),
    }
}

/// 策略配置摘要（只展示库里的值，不造值）：取前 3 个标量项 → `k=v · k=v`。
pub(crate) fn policy_summary(config: Option<&str>) -> String {
    let Some(obj) = config.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else {
        return "—".to_string();
    };
    let Some(map) = obj.as_object() else {
        return "—".to_string();
    };
    let parts: Vec<String> = map
        .iter()
        .filter(|(_, v)| !v.is_object() && !v.is_array())
        .take(3)
        .map(|(k, v)| {
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            format!("{k}={val}")
        })
        .collect();
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" · ")
    }
}

/// 该数据库类型下是否有**可用驱动**（决定类型能否被选中）。
///
/// 类型目录与驱动目录是两层：`data_source_types` 里有条目不代表能用——当前只内置
/// MySQL / PostgreSQL / SQLite / DuckDB 四个驱动，其余类型（MariaDB / Oracle /
/// SQL Server / ClickHouse…）需等驱动插件能力开放。没有可用驱动的类型：
/// 类型树置灰并标注「暂无驱动」，且 `select_type` 拒绝切换（避免“选了类型却存不了”）。
pub(crate) fn type_has_driver(drivers: &[Driver], type_id: &str) -> bool {
    drivers.iter().any(|d| d.enabled && d.type_id == type_id)
}

/// 按下拉显示值定位驱动：驱动 id → 完整名 → 短名（兼容回读、旧草稿与手输值）。
pub(crate) fn find_driver_by_value<'a>(drivers: &'a [Driver], value: &str) -> Option<&'a Driver> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    drivers
        .iter()
        .find(|d| d.id == v)
        .or_else(|| drivers.iter().find(|d| d.name == v))
        .or_else(|| {
            drivers
                .iter()
                .find(|d| driver_short_name(&d.name).eq_ignore_ascii_case(v))
        })
}

/// 类型徽标（emoji 图标 + 类型名）；类型不存在时返回 None。
///
/// `icon` 缺失（旧目录数据）时用通用数据库符号，保证条目与 Header 位置不空。
pub(crate) fn type_badge(types: &[DataSourceType], type_id: &str) -> Option<(String, String)> {
    types.iter().find(|t| t.id == type_id).map(|t| {
        let icon = match &t.icon {
            Some(i) if !i.trim().is_empty() => i.clone(),
            _ => "🗄".to_string(),
        };
        (icon, t.name.clone())
    })
}

/// 分组标题行（单列大纲，可折叠）：chevron + 图标 + 标题 + 底部分隔线。
///
/// 与卡片（`sec-card`）的取舍：卡片并行会在宽度不足时换行、卡高不齐、长值被挤；
/// 单列大纲只有一列宽度，分组用**整幅面板 + 标题栏分隔线**区分（真机反馈：分组分辨不清）。
pub(crate) fn section_header(
    theme: &Theme,
    icon: Icon,
    icon_color: Hsla,
    title: &str,
    collapsed: bool,
) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(rems(GAP_SM))
        .h(rems(ROW_H))
        .border_b_1()
        .border_color(theme.colors.border)
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .child(
            lucide(if collapsed {
                "icons/chevron-right.svg"
            } else {
                "icons/chevron-down.svg"
            })
            .size(rems(crate::ui::ICON_SIZE_SM))
            .text_color(theme.colors.muted_foreground),
        )
        .child(icon.size(rems(crate::ui::ICON_SIZE_SM)).text_color(icon_color))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(theme.colors.foreground)
                .child(title.to_string()),
        )
}

/// 大纲分组：**整幅面板**（浅底 + 圆角，非并排卡片）+ 标题行（点击折叠）+ 内容。
///
/// `on_toggle` 由调用方提供（需要对话框状态与宿主重绘桥），保持 helper 不依赖状态。
pub(crate) fn outline_section(
    theme: &Theme,
    id: &'static str,
    icon: Icon,
    icon_color: Hsla,
    title: &str,
    collapsed: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    body: Div,
) -> Div {
    let header = section_header(theme, icon, icon_color, title, collapsed)
        .id(ElementId::Name(SharedString::from(format!("sec-{id}"))))
        .on_click(move |_, window, cx| on_toggle(window, cx));
    let mut panel = div()
        .w_full()
        .v_flex()
        .gap(rems(GAP_SM))
        .rounded(theme.radius)
        .bg(theme.colors.group_box)
        .py(rems(GAP_SM))
        .px_2()
        .child(header);
    if !collapsed {
        panel = panel.child(div().w_full().pt(rems(GAP_SM)).child(body));
    }
    panel
}

/// 表单行（大纲内）：标签列（贴紧控件）+ 弹性控件列（原 `form-grid`）。
///
/// 标签列 4.25rem + 间距 0.375rem（真机反馈：标签与输入框距离太远）。
pub(crate) fn form_row(theme: &Theme, label: &str, value: impl IntoElement) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(rems(GAP_SM))
        .child(
            div()
                .w(rems(LABEL_COL_W))
                .flex_shrink_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// 提示行（大纲内）：弱化小字，缩进对齐控件列。
pub(crate) fn hint_line(theme: &Theme, text: &str) -> Div {
    div()
        .w_full()
        .pl(rems(LABEL_COL_W + GAP_SM))
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 原型 `reuse-note`：复用说明行（小字 + 图标）。
pub(crate) fn reuse_note(theme: &Theme, text: &str) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(rems(0.375))
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(
            lucide("icons/settings.svg")
                .size(rems(crate::ui::ICON_SIZE_SM))
                .text_color(theme.colors.success),
        )
        .child(text.to_string())
}

/// 地址列标签：文件型 = 「地址」（本地文件路径），网络型 = 「URI」
/// （真机反馈：SQLite 下仍显示 URI 与 mysql 示例，语义与噪声都不对）。
pub(crate) fn address_label(is_file: bool) -> &'static str {
    if is_file {
        "地址"
    } else {
        "URI"
    }
}

/// 地址输入占位：**随当前驱动推导**（占位是 UI 文案字典，不是业务数据；见架构 §15）。
///
/// - 网络型：优先驱动声明的 `url_template`（`{username}` → user 等示例值，端口取声明的默认端口）；
///   无模板时才退回 `{type_id}://主机:端口/数据库`。
/// - 文件型：按类型给文件提示（内置 sqlite / duckdb；插件驱动落地后走通用提示）。
pub(crate) fn address_placeholder(driver: Option<&Driver>, type_id: &str) -> String {
    if let Some(d) = driver {
        if d.is_file {
            return file_path_placeholder(&d.type_id);
        }
        return match d
            .url_template
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            Some(t) => url_template_example(t, d.default_port),
            None => format!("{}://主机:端口/数据库", d.type_id),
        };
    }
    if matches!(type_id, "sqlite" | "duckdb") {
        return file_path_placeholder(type_id);
    }
    "选择数据库类型与驱动后填写连接地址".to_string()
}

/// 文件型地址占位（按类型 id 的文案字典；未知文件型驱动用通用提示）。
fn file_path_placeholder(type_id: &str) -> String {
    match type_id {
        "sqlite" => "选择或新建 .db / .sqlite 文件路径".to_string(),
        "duckdb" => "选择或新建 .duckdb 文件路径（或输入 :memory:）".to_string(),
        _ => "选择或新建数据库文件路径".to_string(),
    }
}

/// `url_template` → 可读示例（占位符换成示例值；端口优先用驱动声明的默认端口）。
pub(crate) fn url_template_example(template: &str, default_port: Option<i32>) -> String {
    let port = default_port
        .map(|p| p.to_string())
        .unwrap_or_else(|| "端口".to_string());
    template
        .replace("{username}", "user")
        .replace("{password}", "password")
        .replace("{host}", "localhost")
        .replace("{port}", &port)
        .replace("{database}", "db")
        .replace("{schema}", "public")
        .replace("{file_path}", "路径")
}

// ===== 驱动声明的连接字段（`drivers.config_schema`）=====
//
// 常规 Tab 的字段集合是「驱动 schema × 连接方式」的函数：
// - 行的**存在性**看 schema 是否声明了该键（如某驱动无 database 字段就不出该行）；
// - 行的**标签 / 占位**取 schema 声明（不再写死「主机 / 端口 / 数据库」）；
// - 未声明的字段不造默认行（见架构 §15）。

/// 驱动声明的连接字段（`config_schema.fields[]` 子集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormField {
    pub key: String,
    pub label: String,
    /// `text` / `number` / `password` / `file` / `select` / `textarea` / `bool`（未声明 → `text`）。
    pub kind: String,
    pub required: bool,
    pub placeholder: Option<String>,
}

/// 解析 `drivers.config_schema` 的 `fields`（非法 / 缺字段 → 空列表；不造默认值）。
pub(crate) fn driver_form_fields(config_schema: &str) -> Vec<FormField> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(config_schema) else {
        return Vec::new();
    };
    let Some(list) = json.get("fields").and_then(|f| f.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|f| {
            let key = f.get("key")?.as_str()?.trim();
            if key.is_empty() {
                return None;
            }
            Some(FormField {
                key: key.to_string(),
                label: f
                    .get("label")
                    .and_then(|x| x.as_str())
                    .unwrap_or(key)
                    .to_string(),
                kind: f
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("text")
                    .to_string(),
                required: f.get("required").and_then(|x| x.as_bool()).unwrap_or(false),
                placeholder: f
                    .get("placeholder")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect()
}

/// 按键查驱动声明字段（未声明 → `None`）。
pub(crate) fn field_spec<'a>(fields: &'a [FormField], key: &str) -> Option<&'a FormField> {
    fields.iter().find(|f| f.key == key)
}

/// 驱动派生数据：渲染期反复用到的三份声明解析结果（表单字段 / 能力 / 认证方法）。
///
/// 只做数据聚合；缓存与失效判别在 [`ConnectionDialogState::driver_derived`]。
/// `key` 取驱动 id + 三段声明原文（声明变了即失效），避免为拿键而先解析 JSON。
///
/// [`ConnectionDialogState::driver_derived`]: crate::components::connection_dialog::ConnectionDialogState::driver_derived
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DriverDerived {
    /// 缓存键：驱动 id + 三段声明原文；无驱动时为空串。
    pub(crate) key: String,
    pub(crate) form_fields: Vec<FormField>,
    pub(crate) capabilities: Vec<String>,
    pub(crate) auth_types: Vec<String>,
}

impl DriverDerived {
    /// 缓存键（只拼接字符串，不解析 JSON）。
    pub(crate) fn cache_key(driver: &Driver) -> String {
        format!(
            "{}|{}|{}|{}",
            driver.id,
            driver.config_schema,
            driver.capabilities.as_deref().unwrap_or(""),
            driver.supported_auth_types.as_deref().unwrap_or("")
        )
    }

    /// 由驱动目录行解析（`None` → 全空，与“未选驱动”语义一致）。
    pub(crate) fn resolve(driver: Option<&Driver>) -> Self {
        let Some(d) = driver else {
            return Self::default();
        };
        Self {
            key: Self::cache_key(d),
            form_fields: driver_form_fields(&d.config_schema),
            capabilities: driver_capabilities(d.capabilities.as_deref()),
            auth_types: driver_auth_types(d.supported_auth_types.as_deref()),
        }
    }
}

/// 地址字段：优先 `type = file`，否则按常见键名（`file_path` / `path` / `file`）。
pub(crate) fn address_field(fields: &[FormField]) -> Option<&FormField> {
    fields
        .iter()
        .find(|f| f.kind == "file")
        .or_else(|| fields.iter().find(|f| matches!(f.key.as_str(), "file_path" | "path" | "file")))
}

// ===== 网络配置字段（`network_configs.config` 的结构化编辑）=====
//
// 背景（审计 #20 / #24）：管理器原先只有一个「数据(JSON)」文本框，用户必须手写
// `SshConfig` / `ProxyConfig` 的 JSON 才能建出可用档案（写错就叫苦不迭）。
// 这里把字段声明、JSON 组装、校验与回填都做成纯函数，渲染层只负责把输入框摆出来——
// 与「驱动 schema → 表单」同一思路（架构 §15：UI 不造数据，只管形状）。

/// 网络配置字段的输入形态（渲染层据此选 Input 与占位文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetFieldKind {
    Text,
    Port,
    Password,
    Bool,
}

/// 单个字段声明：`path` 是写入 config JSON 的路径（多段 → 嵌套对象）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetFieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub path: &'static [&'static str],
    pub kind: NetFieldKind,
    pub required: bool,
    pub placeholder: &'static str,
    /// true = 逗号分隔 → JSON 数组（如 `no_proxy`）。
    pub array: bool,
}

fn net_spec(
    key: &'static str,
    label: &'static str,
    path: &'static [&'static str],
    kind: NetFieldKind,
    required: bool,
    placeholder: &'static str,
) -> NetFieldSpec {
    NetFieldSpec {
        key,
        label,
        path,
        kind,
        required,
        placeholder,
        array: false,
    }
}

/// 某类型需要显示的字段（**空 = 走原始 JSON 文本框**，如协议链）。
///
/// 字段与 `connection::config` 的 serde 模型一一对应：
/// - `proxy` / `socks`：`ProxyConfig`（host / port / auth.username / auth.password / no_proxy）；
/// - `ssh`：`SshConfig`（host / port / username / password 或 key_path / remote_host / remote_port）；
/// - `ssl`：`SslConfig`（verify_server_cert / 三个证书路径）。
pub(crate) fn network_field_specs(type_key: &str) -> Vec<NetFieldSpec> {
    let port = NetFieldKind::Port;
    match type_key.trim().to_ascii_lowercase().as_str() {
        "ssh" | "ssh_tunnel" => vec![
            net_spec("host", "SSH 主机", &["host"], NetFieldKind::Text, true, "jump.example.com"),
            net_spec("port", "SSH 端口", &["port"], port, false, "22"),
            net_spec("username", "SSH 用户名", &["username"], NetFieldKind::Text, true, "deploy"),
            net_spec("password", "SSH 密码", &["password"], NetFieldKind::Password, false, "与私钥二选一"),
            net_spec("key_path", "私钥路径", &["key_path"], NetFieldKind::Text, false, "~/.ssh/id_ed25519"),
            net_spec("remote_host", "目标主机", &["remote_host"], NetFieldKind::Text, true, "数据库主机（隧道出口）"),
            net_spec("remote_port", "目标端口", &["remote_port"], port, true, "5432"),
        ],
        "proxy" | "http" | "http_proxy" => {
            let mut specs = proxy_field_specs("8080");
            specs[1].placeholder = "8080";
            specs
        }
        "socks" | "socks5" | "socks_proxy" => {
            let mut specs = proxy_field_specs("1080");
            specs[1].placeholder = "1080";
            specs
        }
        "ssl" | "tls" => vec![
            net_spec("verify", "校验服务器证书", &["verify_server_cert"], NetFieldKind::Bool, false, "true"),
            net_spec("ca", "CA 证书路径", &["ca_cert_path"], NetFieldKind::Text, false, "/etc/ssl/ca.pem"),
            net_spec("cert", "客户端证书", &["client_cert_path"], NetFieldKind::Text, false, "（可选）"),
            net_spec("key", "客户端私钥", &["client_key_path"], NetFieldKind::Text, false, "（可选）"),
        ],
        _ => Vec::new(),
    }
}

fn proxy_field_specs(default_port: &'static str) -> Vec<NetFieldSpec> {
    vec![
        net_spec("host", "代理主机", &["host"], NetFieldKind::Text, true, "127.0.0.1"),
        net_spec("port", "代理端口", &["port"], NetFieldKind::Port, true, default_port),
        net_spec("username", "代理用户名", &["auth", "username"], NetFieldKind::Text, false, "（可选）"),
        net_spec("password", "代理密码", &["auth", "password"], NetFieldKind::Password, false, "（可选）"),
        NetFieldSpec {
            key: "no_proxy",
            label: "直连主机",
            path: &["no_proxy"],
            kind: NetFieldKind::Text,
            required: false,
            placeholder: "localhost,127.0.0.1",
            array: true,
        },
    ]
}

/// 字段值（key → 原始文本）→ config JSON（校验必填 / 端口 / 布尔）。
///
/// SSH 认证方式：填了私钥路径→ `auth_type=private_key`，否则用密码；两者都空报错。
pub(crate) fn build_network_config_json(
    network_type: &str,
    values: &[(String, String)],
) -> Result<String, String> {
    let specs = network_field_specs(network_type);
    if specs.is_empty() {
        return Err(format!(
            "类型「{}」不支持字段编辑（协议链请选 chain 并直接填 JSON）",
            network_type.trim()
        ));
    }
    let get = |key: &str| -> String {
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };

    for spec in &specs {
        let raw = get(spec.key);
        if spec.required && raw.is_empty() {
            return Err(format!("「{}」为必填项", spec.label));
        }
        if spec.kind == NetFieldKind::Port
            && !raw.is_empty()
            && raw.parse::<u16>().is_err()
        {
            return Err(format!("「{}」需要 1-65535 的端口号", spec.label));
        }
        if spec.kind == NetFieldKind::Bool && !raw.is_empty() && !is_bool_text(&raw) {
            return Err(format!("「{}」需要 true / false", spec.label));
        }
    }

    let mut root = serde_json::Map::new();
    for spec in &specs {
        let raw = get(spec.key);
        if raw.is_empty() {
            continue;
        }
        let value = if spec.array {
            serde_json::Value::Array(
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| serde_json::Value::String(s.to_string()))
                    .collect(),
            )
        } else {
            match spec.kind {
                NetFieldKind::Port => serde_json::Value::Number(
                    raw.parse::<u16>()
                        .map_err(|_| format!("「{}」端口非法", spec.label))?
                        .into(),
                ),
                NetFieldKind::Bool => serde_json::Value::Bool(parse_bool_text(&raw)),
                _ => serde_json::Value::String(raw),
            }
        };
        insert_json_path(&mut root, spec.path, value);
    }

    // SSH 的 `auth` 是内部标签枚举（`auth_type`），配置里必须显式给出。
    if network_type.trim().eq_ignore_ascii_case("ssh")
        || network_type.trim().eq_ignore_ascii_case("ssh_tunnel")
    {
        let key_path = get("key_path");
        let password = get("password");
        if !key_path.is_empty() {
            root.insert(
                "auth_type".into(),
                serde_json::Value::String("private_key".into()),
            );
        } else if !password.is_empty() {
            root.insert(
                "auth_type".into(),
                serde_json::Value::String("password".into()),
            );
        } else {
            return Err("SSH 需要「密码」或「私钥路径」二者之一".into());
        }
    }

    Ok(serde_json::Value::Object(root).to_string())
}

/// config JSON → 字段值（编辑回填；缺失 / 非法 JSON → 全空，不报错）。
pub(crate) fn network_config_values(network_type: &str, config_json: &str) -> Vec<(String, String)> {
    spec_field_values(network_field_specs(network_type), config_json)
}

/// 按字段声明从 JSON 回填文本值（网络 / 认证共用）。
fn spec_field_values(specs: Vec<NetFieldSpec>, json: &str) -> Vec<(String, String)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    specs
        .into_iter()
        .map(|spec| {
            let text = match read_json_path(&value, spec.path) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::Bool(b)) => b.to_string(),
                Some(serde_json::Value::Array(items)) => items
                    .iter()
                    .filter_map(|i| i.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                _ => String::new(),
            };
            (spec.key.to_string(), text)
        })
        .collect()
}

// ===== 认证配置字段（`auth_configs.auth_data` 的结构化编辑）=====
//
// 背景（审计 A5）：认证管理器原先只有「数据(JSON)」文本框，用户必须手写
// `inject_auth_into_url` / `inject_{ssh,proxy}_auth_from_auth_data` 认得的键（含
// camelCase 的 `keyPath` / `certPath` / `keytabPath`）。与网络档案同款：字段声明 +
// JSON 组装 / 校验 / 回填都是纯函数，渲染层只摆输入框。

/// 某认证类型需要显示的字段（**空 = 走原始 JSON 文本框**）。
///
/// `path` 是**写进 `auth_data` 的键**，与后端读取端严格一致：
/// - `password` / `ldap`：`username` / `password`（`inject_auth_into_url` 同一分支）；
/// - `ssh_key`：`keyPath` / `passphrase` / `password`（`inject_ssh_auth_from_auth_data`）；
/// - `proxy_pwd`：`username` / `password`（`inject_proxy_auth_from_auth_data`）；
/// - `pg_class`：`certPath` / `certKeyPath`；`kerberos`：`principal` / `keytabPath`。
pub(crate) fn auth_field_specs(auth_type: &str) -> Vec<NetFieldSpec> {
    match auth_type.trim().to_ascii_lowercase().as_str() {
        "password" | "ldap" => vec![
            net_spec("username", "用户名", &["username"], NetFieldKind::Text, true, "数据库账号"),
            net_spec("password", "密码", &["password"], NetFieldKind::Password, true, "登录密码"),
        ],
        "ssh_key" | "ssh" => vec![
            net_spec("username", "SSH 用户名", &["username"], NetFieldKind::Text, false, "（可选）覆盖档案里的用户名"),
            net_spec("key_path", "私钥路径", &["keyPath"], NetFieldKind::Text, false, "与密码二选一"),
            net_spec("passphrase", "私钥口令", &["passphrase"], NetFieldKind::Password, false, "（可选）随私钥使用"),
            net_spec("password", "SSH 密码", &["password"], NetFieldKind::Password, false, "与私钥二选一"),
        ],
        "proxy_pwd" | "proxy" => vec![
            net_spec("username", "代理用户名", &["username"], NetFieldKind::Text, true, "代理账号"),
            net_spec("password", "代理密码", &["password"], NetFieldKind::Password, true, "代理密码"),
        ],
        "pg_class" => vec![
            net_spec("cert_path", "客户端证书", &["certPath"], NetFieldKind::Text, true, "/etc/ssl/client.crt"),
            net_spec("cert_key_path", "客户端私钥", &["certKeyPath"], NetFieldKind::Text, false, "（可选）"),
        ],
        "kerberos" => vec![
            net_spec("principal", "Principal", &["principal"], NetFieldKind::Text, true, "user@REALM"),
            net_spec("keytab_path", "Keytab 路径", &["keytabPath"], NetFieldKind::Text, false, "（可选）"),
        ],
        _ => Vec::new(),
    }
}

/// 认证字段值 → `auth_data` JSON（校验必填 / SSH 凭据二选一 / 代理凭据成对）。
pub(crate) fn build_auth_config_json(
    auth_type: &str,
    values: &[(String, String)],
) -> Result<String, String> {
    let key = auth_type.trim().to_ascii_lowercase();
    let specs = auth_field_specs(&key);
    if specs.is_empty() {
        return Err(format!("认证类型「{}」不支持字段编辑", auth_type.trim()));
    }
    let get = |k: &str| -> String {
        values
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default()
    };

    for spec in &specs {
        if spec.required && get(spec.key).is_empty() {
            return Err(format!("「{}」为必填项", spec.label));
        }
    }
    let is_ssh = matches!(key.as_str(), "ssh_key" | "ssh");
    let is_proxy = matches!(key.as_str(), "proxy_pwd" | "proxy");
    if is_ssh && get("key_path").is_empty() && get("password").is_empty() {
        return Err("SSH 认证需要「私钥路径」或「密码」二者之一".into());
    }
    if is_proxy && (get("username").is_empty() || get("password").is_empty()) {
        return Err("代理认证需要同时填写用户名与密码".into());
    }

    let mut root = serde_json::Map::new();
    for spec in &specs {
        let raw = get(spec.key);
        if raw.is_empty() {
            continue;
        }
        // 口令只在私钥路径存在时有意义（注入函数只在 keyPath 分支读它）。
        if is_ssh && spec.key == "passphrase" && get("key_path").is_empty() {
            continue;
        }
        insert_json_path(&mut root, spec.path, serde_json::Value::String(raw));
    }
    Ok(serde_json::Value::Object(root).to_string())
}

/// `auth_data` JSON → 字段值（编辑回填；缺失 / 非法 JSON → 全空，不报错）。
pub(crate) fn auth_config_values(auth_type: &str, config_json: &str) -> Vec<(String, String)> {
    spec_field_values(auth_field_specs(auth_type), config_json)
}

// ===== 文件型「新建文件…」的确定性逻辑（纯函数，便于单测）=====

/// 「新建文件…」的默认文件名（按**驱动 id** 判族，与 `is_file_db` 同一来源）。
pub(crate) fn new_db_file_suggested_name(driver_id: &str) -> &'static str {
    if driver_id.eq_ignore_ascii_case("duckdb") {
        "new_database.duckdb"
    } else {
        "new_database.db"
    }
}

// ===== 结果行（#28 分级 + 详情）=====

/// 摘要超过该字符数（或含换行）时，UI 提供「详情」展开（按字符数，避免多字节截断）。
pub(crate) const RESULT_SUMMARY_MAX_CHARS: usize = 80;

/// 摘要是否需要「详情」入口（过长或含换行）。
pub(crate) fn result_needs_detail(summary: &str) -> bool {
    summary.chars().count() > RESULT_SUMMARY_MAX_CHARS || summary.contains('\n')
}

/// 处理「新建文件…」选中的路径：返回 `(结果行文案, 是否成功, 写回地址的值)`。
///
/// **语义（与「打开文件…」严格分离）**：
/// - 路径已存在 → 不创建、不引用、也不碰原文件（避免误损已有库），提示换名或用「打开文件…」；
/// - 路径不存在 → 创建空库文件并写回地址（首次连接由驱动初始化结构）。
///
/// 为何不做“已存在则直接引用”：那会让「新建」看起来就是打开（真机反馈“duckdb 的新建
/// 为什么还是打开功能”）；两个按钮职责必须互斥。
pub(crate) fn create_new_db_file(path: &std::path::Path) -> (String, bool, Option<String>) {
    let value = path.to_string_lossy().to_string();
    if path.exists() {
        return (
            format!("该文件已存在，未创建：{value}；请换一个文件名，或用「打开文件…」引用它"),
            false,
            None,
        );
    }
    match std::fs::File::create(path) {
        Ok(_) => (format!("已新建数据库文件：{value}"), true, Some(value)),
        Err(e) => (format!("新建数据库文件失败：{e}"), false, None),
    }
}

fn is_bool_text(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "true" | "false" | "1" | "0" | "yes" | "no"
    )
}

fn parse_bool_text(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

fn insert_json_path(
    root: &mut serde_json::Map<String, serde_json::Value>,
    path: &[&str],
    value: serde_json::Value,
) {
    match path {
        [] => {}
        [only] => {
            root.insert((*only).to_string(), value);
        }
        [head, rest @ ..] => {
            let entry = root
                .entry((*head).to_string())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(map) = entry {
                insert_json_path(map, rest, value);
            }
        }
    }
}

fn read_json_path<'a>(
    value: &'a serde_json::Value,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    match path {
        [] => Some(value),
        [head, rest @ ..] => read_json_path(value.get(*head)?, rest),
    }
}

/// 文件型（SQLite / DuckDB）连接的落库前清洗：清掉无意义的凭据 / 网络 / TLS 字段。
///
/// 输入对象可能带着上个数据库类型的残留（如从 MySQL 切到 SQLite），这些字段落到库里
/// 就是脏数据（还会让“引用认证配置”在同一连接上产生歧义）；策略覆盖等文件型仍有效的
/// 选项保留。
pub(crate) fn strip_file_db_noise(input: &mut DataSourceSaveInput) {
    input.username = None;
    input.password = None;
    input.auth_config_id = None;
    input.auth_method = None;
    input.network_config_id = None;
    let Some(raw) = input.advanced_options.as_deref() else {
        return;
    };
    let Ok(mut adv) = serde_json::from_str::<serde_json::Value>(raw) else {
        // 非法 JSON 原样保留（解析失败不静默丢数据，上层已有其它校验）。
        return;
    };
    if let Some(obj) = adv.as_object_mut() {
        obj.remove("ssl");
        obj.remove("network_chain");
    }
    input.advanced_options = Some(adv.to_string());
}

/// 重建编辑回读地址（DataSource 无 url 字段，由 host/port/database 重拼）。
///
/// 文件型返回**裸路径**（与地址输入框语义一致，见 `normalize_file_db_path`）。
pub(crate) fn reconstruct_url(ds: &DataSource) -> String {
    if crate::services::data_source_service::is_file_db_driver(&ds.db_type) {
        let raw = ds
            .database
            .clone()
            .or_else(|| ds.host.clone())
            .unwrap_or_default();
        return crate::services::data_source_service::normalize_file_db_path(&ds.db_type, &raw);
    }
    let auth = ds
        .username
        .as_deref()
        .map(|u| format!("{}@", u))
        .unwrap_or_default();
    match (&ds.host, ds.port) {
        (Some(h), Some(p)) => format!(
            "{}://{}{}:{}/{}",
            ds.db_type,
            auth,
            h,
            p,
            ds.database.clone().unwrap_or_default()
        ),
        (Some(h), None) => format!(
            "{}://{}{}/{}",
            ds.db_type,
            auth,
            h,
            ds.database.clone().unwrap_or_default()
        ),
        _ => format!(
            "{}://{}{}",
            ds.db_type,
            ds.database.clone().unwrap_or_default(),
            ""
        ),
    }
}

/// 暂存条目显示用的类型 id：**当前条目（光标位）取正在编辑的表单**，其余取快照。
///
/// 真机反馈：从 MySQL 切到 SQLite 后，表单已是 SQLite、条目还显示 mysql 图标——
/// 因为草稿只在“切换条目 / 保存 / 关闭”时写回，显示不能等写回。
pub(crate) fn staging_display_type_id(draft_type_id: &str, live_type_id: Option<&str>) -> String {
    match live_type_id {
        Some(live) if !live.trim().is_empty() => live.to_string(),
        _ => draft_type_id.to_string(),
    }
}

/// 作用域标签（UI ↔ ConnectionScope）。
pub(crate) fn scope_label(s: &ConnectionScope) -> &'static str {
    match s {
        ConnectionScope::Global => "仅全局",
        ConnectionScope::Project => "仅项目",
        ConnectionScope::GlobalAndProject => "全局+项目",
    }
}

pub(crate) fn scope_from_label(l: &str) -> ConnectionScope {
    match l {
        "仅项目" => ConnectionScope::Project,
        "全局+项目" => ConnectionScope::GlobalAndProject,
        _ => ConnectionScope::Global,
    }
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`super::*` 会把 gpui 的 `test` 宏带入作用域）。
    use super::{
        address_field, address_label, address_placeholder, capability_rows, driver_auth_types,
        driver_capabilities, driver_form_fields, driver_short_name, enabled_drivers_of_type,
        field_spec, find_driver_by_value, policy_summary, policy_type_from_label, policy_type_label,
        staging_display_type_id, strip_file_db_noise, tags_from_json, tags_to_json, type_badge,
        type_has_driver, url_template_example, auth_config_values, auth_field_specs,
        build_auth_config_json, build_network_config_json, conn_display_name, create_new_db_file,
        dialog_tab_defs, network_config_values, network_field_specs, new_db_file_suggested_name,
        property_note, result_needs_detail, saved_result, visible_tab_index, DriverDerived,
        app_level_capabilities,
    };
    use connection::model::DataSourceSaveInput;
    use engine::persistence::driver_store::{DataSourceType, Driver};

    fn driver(id: &str, type_id: &str, name: &str, enabled: bool) -> Driver {
        Driver {
            id: id.into(),
            type_id: type_id.into(),
            name: name.into(),
            driver_kind: "native".into(),
            is_file: false,
            default_port: None,
            url_template: None,
            download_url: None,
            download_checksum: None,
            version: None,
            config_schema: String::new(),
            supported_auth_types: None,
            capabilities: None,
            driver_properties: None,
            enabled,
        }
    }

    fn ds_type(id: &str, name: &str, icon: Option<&str>) -> DataSourceType {
        DataSourceType {
            id: id.into(),
            name: name.into(),
            category: "relational".into(),
            icon: icon.map(|s| s.to_string()),
            enabled: true,
            created_at: String::new(),
        }
    }

    #[test]
    fn driver_derived_parses_declarations_and_keys_on_them() {
        let mut d = driver("mysql_native", "mysql", "MySQL (Official)", true);
        d.config_schema = r#"{"fields":[{"key":"host","label":"主机"}]}"#.to_string();
        d.capabilities = Some(r#"["transactions","ssl"]"#.to_string());
        d.supported_auth_types = Some(r#"["password","ssl"]"#.to_string());

        let derived = DriverDerived::resolve(Some(&d));
        assert_eq!(derived.form_fields.len(), 1);
        assert_eq!(derived.form_fields[0].label, "主机");
        assert_eq!(derived.capabilities, vec!["transactions", "ssl"]);
        assert_eq!(derived.auth_types, vec!["password", "ssl"]);

        // 键只看驱动 id + 三段声明原文：声明未改则键稳定（缓存命中），改了就换键（失效重算）。
        assert_eq!(DriverDerived::cache_key(&d), derived.key);
        let mut changed = d.clone();
        changed.capabilities = Some(r#"["ssl"]"#.to_string());
        assert_ne!(DriverDerived::cache_key(&changed), derived.key);

        // 未选驱动 → 全空（不为“未选”造默认字段）。
        let empty = DriverDerived::resolve(None);
        assert!(empty.key.is_empty());
        assert!(empty.form_fields.is_empty());
        assert!(empty.capabilities.is_empty());
        assert!(empty.auth_types.is_empty());
    }

    #[test]
    fn network_field_specs_cover_supported_types() {
        for t in ["ssh", "SSH", "ssh_tunnel", "proxy", "http", "socks5", "TLS", "ssl"] {
            assert!(!network_field_specs(t).is_empty(), "{t} 应有字段声明");
        }
        assert!(
            network_field_specs("chain").is_empty(),
            "协议链走原始 JSON 文本框（跳太多，不逐项展开）"
        );
    }

    #[test]
    fn network_config_json_matches_engine_models() {
        // 关键回归：组装出的 JSON 必须能被 `connection::config` 的 serde 模型直接反序列化。
        let proxy = build_network_config_json(
            "proxy",
            &[
                ("host".into(), "127.0.0.1".into()),
                ("port".into(), "3128".into()),
                ("username".into(), "u".into()),
                ("password".into(), "p".into()),
                ("no_proxy".into(), "localhost, 127.0.0.1".into()),
            ],
        )
        .expect("build proxy");
        let parsed: connection::config::ProxyConfig =
            serde_json::from_str(&proxy).expect("proxy JSON 可反序列化");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 3128);
        assert_eq!(parsed.auth.as_ref().map(|a| a.username.as_str()), Some("u"));
        assert_eq!(
            parsed.no_proxy,
            vec!["localhost".to_string(), "127.0.0.1".to_string()]
        );

        // SSH 密码认证（端口留空 → 模型默认 22）。
        let ssh = build_network_config_json(
            "SSH",
            &[
                ("host".into(), "jump".into()),
                ("username".into(), "deploy".into()),
                ("password".into(), "pw".into()),
                ("remote_host".into(), "db".into()),
                ("remote_port".into(), "5432".into()),
            ],
        )
        .expect("build ssh");
        let parsed: connection::config::SshConfig =
            serde_json::from_str(&ssh).expect("ssh JSON 可反序列化");
        assert_eq!(parsed.host, "jump");
        assert_eq!(parsed.port, 22);
        assert!(matches!(
            parsed.auth,
            connection::config::SshAuth::Password { .. }
        ));

        // SSH 私钥认证（填了私钥路径 → auth_type 切 private_key）。
        let ssh_key = build_network_config_json(
            "ssh",
            &[
                ("host".into(), "jump".into()),
                ("username".into(), "deploy".into()),
                ("key_path".into(), "/home/u/.ssh/id_ed25519".into()),
                ("remote_host".into(), "db".into()),
                ("remote_port".into(), "5432".into()),
            ],
        )
        .expect("build ssh key");
        let parsed: connection::config::SshConfig =
            serde_json::from_str(&ssh_key).expect("ssh(私钥) JSON 可反序列化");
        assert!(matches!(
            parsed.auth,
            connection::config::SshAuth::PrivateKey { .. }
        ));

        let ssl = build_network_config_json(
            "ssl",
            &[
                ("verify".into(), "false".into()),
                ("ca".into(), "/etc/ssl/ca.pem".into()),
            ],
        )
        .expect("build ssl");
        let parsed: connection::config::SslConfig =
            serde_json::from_str(&ssl).expect("ssl JSON 可反序列化");
        assert!(!parsed.verify_server_cert);
        assert_eq!(parsed.ca_cert_path.as_deref(), Some("/etc/ssl/ca.pem"));
    }

    #[test]
    fn network_config_json_validates_required_ports_and_auth() {
        let err = build_network_config_json("proxy", &[("port".into(), "1080".into())])
            .expect_err("缺主机应报错");
        assert!(err.contains("必填"), "{err}");

        let err = build_network_config_json(
            "proxy",
            &[("host".into(), "h".into()), ("port".into(), "abc".into())],
        )
        .expect_err("端口非法应报错");
        assert!(err.contains("端口"), "{err}");

        let err = build_network_config_json(
            "ssh",
            &[
                ("host".into(), "h".into()),
                ("username".into(), "u".into()),
                ("remote_host".into(), "d".into()),
                ("remote_port".into(), "1".into()),
            ],
        )
        .expect_err("无凭据应报错");
        assert!(err.contains("密码") && err.contains("私钥"), "{err}");

        let err = build_network_config_json("chain", &[])
            .expect_err("协议链不走字段编辑");
        assert!(err.contains("chain"), "{err}");
    }

    #[test]
    fn network_config_values_roundtrip_for_edit() {
        let json = build_network_config_json(
            "proxy",
            &[
                ("host".into(), "p".into()),
                ("port".into(), "3128".into()),
                ("no_proxy".into(), "localhost".into()),
            ],
        )
        .expect("build");
        let values = network_config_values("proxy", &json);
        let get = |k: &str| {
            values
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("host"), "p");
        assert_eq!(get("port"), "3128");
        assert_eq!(get("no_proxy"), "localhost");
        assert_eq!(get("username"), "", "未填字段回填为空");

        // 非法 JSON：不 panic，字段全空（不把垃圾反填进表单）。
        let empty = network_config_values("proxy", "not-json");
        assert!(empty.is_empty());
    }

    #[test]
    fn auth_field_specs_cover_supported_types() {
        for t in [
            "password", "PASSWORD", "ldap", "pg_class", "kerberos", "ssh_key", "ssh",
            "proxy_pwd", "proxy",
        ] {
            assert!(!auth_field_specs(t).is_empty(), "{t} 应有字段声明");
        }
        assert!(
            auth_field_specs("unknown_new_auth").is_empty(),
            "未知类型没有声明 → 回落到原始 JSON 文本框"
        );
    }

    #[test]
    fn auth_config_json_uses_backend_keys() {
        // 键必须与后端读取端一致（`inject_auth_into_url` / `inject_{ssh,proxy}_auth_from_auth_data`）。
        let v = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, s)| ((*k).to_string(), (*s).to_string()))
                .collect()
        };

        let json = build_auth_config_json(
            "password",
            &v(&[("username", "alice"), ("password", "s3cret")]),
        )
        .expect("password 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["username"], "alice");
        assert_eq!(parsed["password"], "s3cret");

        let json = build_auth_config_json(
            "pg_class",
            &v(&[("cert_path", "/c/c.pem"), ("cert_key_path", "/c/k.pem")]),
        )
        .expect("pg_class 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["certPath"], "/c/c.pem", "后端读 certPath（驼峰）");
        assert_eq!(parsed["certKeyPath"], "/c/k.pem");

        let json = build_auth_config_json(
            "kerberos",
            &v(&[("principal", "u@REALM"), ("keytab_path", "/k.keytab")]),
        )
        .expect("kerberos 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["principal"], "u@REALM");
        assert_eq!(parsed["keytabPath"], "/k.keytab");

        // SSH：私钥路径 + 口令；未填字段不写空值
        let json = build_auth_config_json(
            "ssh_key",
            &v(&[("username", "deploy"), ("key_path", "~/.ssh/id"), ("passphrase", "pp")]),
        )
        .expect("ssh 组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["keyPath"], "~/.ssh/id");
        assert_eq!(parsed["passphrase"], "pp");
        assert_eq!(parsed["username"], "deploy");
        assert!(parsed.get("password").is_none(), "未填字段不写空串");

        // 只填口令不填私钥：口令被忽略（注入函数只在 keyPath 分支读它）
        let json = build_auth_config_json(
            "ssh_key",
            &v(&[("password", "ssh-pwd"), ("passphrase", "pp")]),
        )
        .expect("ssh 密码组装");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert!(parsed.get("passphrase").is_none());
        assert_eq!(parsed["password"], "ssh-pwd");
    }

    #[test]
    fn auth_config_json_validation_and_roundtrip() {
        let v = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, s)| ((*k).to_string(), (*s).to_string()))
                .collect()
        };

        // 必填（password 类型需要用户名）
        let err = build_auth_config_json("password", &v(&[("password", "p")]))
            .expect_err("缺用户名应报错");
        assert!(err.contains("用户名"), "{err}");
        // 代理凭据成对
        let err = build_auth_config_json("proxy_pwd", &v(&[("username", "u")]))
            .expect_err("缺密码应报错");
        assert!(err.contains("密码"), "{err}");
        // SSH 凭据二选一
        let err = build_auth_config_json("ssh_key", &v(&[("username", "u")]))
            .expect_err("无凭据应报错");
        assert!(err.contains("私钥") || err.contains("密码"), "{err}");
        // 未声明字段的类型：明确提示走原始 JSON
        let err = build_auth_config_json("oauth2", &[]).expect_err("未知类型应拒绝字段编辑");
        assert!(err.contains("不支持字段编辑"), "{err}");

        // 回填往返（proxy_pwd）
        let json = build_auth_config_json(
            "proxy_pwd",
            &v(&[("username", "u"), ("password", "p")]),
        )
        .expect("build");
        let values = auth_config_values("proxy_pwd", &json);
        let get = |k: &str| {
            values
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, x)| x.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("username"), "u");
        assert_eq!(get("password"), "p");

        // 非法 JSON：不 panic，字段全空
        assert!(auth_config_values("password", "not-json").is_empty());
    }

    #[test]
    fn auth_config_json_is_readable_by_backend_injector() {
        // A5 核心闭环：UI 字段组装出的 `auth_data` 必须能被后端注入函数直接读懂。
        let json = build_auth_config_json(
            "password",
            &[
                ("username".to_string(), "alice".to_string()),
                ("password".to_string(), "pwd".to_string()),
            ],
        )
        .expect("build");
        let url = connection::url_params::inject_auth_into_url(
            "postgres://h:5432/db",
            "password",
            &json,
        )
        .expect("注入凭据");
        assert_eq!(url, "postgres://alice:pwd@h:5432/db");

        // SSH 私钥分支：键名必须是后端读的 `keyPath`（驼峰），否则等于没配。
        let json = build_auth_config_json(
            "ssh_key",
            &[("key_path".to_string(), "~/.ssh/id_ed25519".to_string())],
        )
        .expect("build ssh");
        assert!(json.contains("\"keyPath\""), "{json}");
    }

    #[test]
    fn saved_result_keeps_connection_id_out_of_the_summary() {
        // #32 B 案：界面与提示词只出现名称；连接 ID 只在「详情」里（排障时点开/复制）。
        let line = saved_result("生产 PG", "G_conn_prod");
        assert!(line.summary.contains("生产 PG"), "摘要应带名称");
        assert!(
            !line.summary.contains("G_conn_prod"),
            "摘要不得出现连接 ID（否则用户又把它当主键）"
        );
        assert!(!result_needs_detail(&line.summary), "短摘要本身不触发折叠");
        let detail = line.detail_text().to_string();
        assert!(detail.contains("G_conn_prod"), "详情带 ID，供排障/报障");
        assert!(line.detail.is_some(), "有详情才会出现「详情 / 复制」入口");

        // 空名回退占位，不出现空引号。
        assert!(saved_result("  ", "P_conn_1").summary.contains("未命名连接"));
        assert_eq!(conn_display_name(" x "), "x");
    }

    #[test]
    fn dialog_tabs_hide_network_for_file_db_and_map_visible_index() {
        // #15 迁移到 `TabBar` 后的接缝：文件型隐藏「网络」→ 可见下标与内部索引错开一位，
        // 错了就会“点能力却显示网络内容”。
        let net = dialog_tab_defs(false);
        assert_eq!(net.len(), 5);
        assert_eq!(visible_tab_index(&net, 0), 0);
        assert_eq!(visible_tab_index(&net, 4), 4);

        let file = dialog_tab_defs(true);
        assert_eq!(file.len(), 4, "文件型不显示网络 Tab");
        assert!(!file.iter().any(|(label, _)| *label == "网络"));
        assert_eq!(visible_tab_index(&file, 0), 0);
        assert_eq!(visible_tab_index(&file, 2), 1, "能力在文件型里是第 2 个可见项");
        assert_eq!(visible_tab_index(&file, 4), 3);
        // 隐藏的「网络」被选中（旧草稿 / 切类型后）回退到第一个可见项，不越界。
        assert_eq!(visible_tab_index(&file, 1), 0);

        for (label, ix) in &file {
            assert_eq!(
                file.get(visible_tab_index(&file, *ix)).map(|(l, _)| *l),
                Some(*label),
                "可见下标应能反查回同一个 Tab"
            );
        }
    }

    #[test]
    fn result_detail_needed_only_for_long_or_multiline_summaries() {
        assert!(!result_needs_detail("已保存：G_conn_x"));
        assert!(!result_needs_detail(&"字".repeat(80)), "刚好 80 字不展开");
        assert!(result_needs_detail(&"字".repeat(81)), "超 80 字提供详情");
        assert!(result_needs_detail("第一行\n第二行"), "换行必须可展开");
    }

    /// 属性行的「去向」提示：四种命运各给一句人话，直通的不啰嗝。
    ///
    /// 判据全部在引擎的 `property_spec`（库认的键 + 未知键的效果），这里只验证翻译层。
    #[test]
    fn property_notes_state_what_will_actually_happen() {
        use super::PropertyNoteLevel as L;

        // 直通同名、无说明 → 不给提示（不啰嗝）；带中性说明 → Info
        assert!(property_note("mysql", "collation").is_none());
        assert!(property_note("mysql_native", "stmt_cache_size").is_none());
        let (text, level) = property_note("mysql", "charset").expect("charset 有说明");
        assert!(text.contains("utf8mb4"), "{text}");
        assert_eq!(level, L::Info);

        // 带副作用的键（会覆盖别的设置）：警告级
        let (text, level) = property_note("mysql_native", "require_ssl").expect("TLS 键应有提醒");
        assert!(text.contains("连接安全"), "{text}");
        assert_eq!(level, L::Warn);

        // sqlx：未知键不会被应用 → 警告（写了不生效）
        let (text, level) = property_note("mysql", "connectTimeout").expect("未知键应提示");
        assert!(text.contains("不会被应用"), "{text}");
        assert_eq!(level, L::Warn);

        // native：未知键会报错 → 危险（连接失败）
        let (text, level) =
            property_note("mysql_native", "ssl_mode").expect("未知键应提示");
        assert!(text.contains("报错"), "{text}");
        assert_eq!(level, L::Danger);
        let (_, level) = property_note("postgres_native", "connectTimeout").expect("应提示");
        assert_eq!(level, L::Danger);

        // 文件型：PRAGMA / SET 由驱动侧在开库时应用（“会生效”，不是“不理”）
        let (text, level) = property_note("sqlite", "journalMode").expect("应提示");
        assert!(text.contains("由驱动应用为 journal_mode"), "{text}");
        assert_eq!(level, L::Info);
        assert!(property_note("duckdb", "threads").is_some());
        // 文件型里带副作用的键（会覆盖应用默认）：警告级
        let (text, level) = property_note("sqlite", "foreign_keys").expect("应提示");
        assert!(text.contains("违规写入"), "{text}");
        assert_eq!(level, L::Warn);
        // 文件型里**不在清单**的键：驱动不应用（界面不能说它会生效）
        let (text, level) = property_note("sqlite", "magic").expect("应提示");
        assert!(text.contains("不会被应用"), "{text}");
        assert_eq!(level, L::Warn);

        // 未收录的驱动：不装作知道
        let (text, level) = property_note("some_plugin_driver", "charset").expect("应提示");
        assert!(text.contains("未收录"), "{text}");
        assert_eq!(level, L::Info);
    }

    #[test]
    fn new_db_file_suggested_name_follows_driver() {
        // 建议文件名按驱动 id 判族（与 `is_file_db` 同一来源），否则类型 / 驱动两套事实会打架。
        assert_eq!(new_db_file_suggested_name("duckdb"), "new_database.duckdb");
        assert_eq!(new_db_file_suggested_name("DUCKDB"), "new_database.duckdb");
        assert_eq!(new_db_file_suggested_name("sqlite"), "new_database.db");
        assert_eq!(new_db_file_suggested_name(""), "new_database.db", "未知驱动回退 .db");
    }

    #[test]
    fn create_new_db_file_rejects_existing_and_creates_missing() {
        let dir = std::env::temp_dir().join(format!("rds_newfile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");

        // 不存在：创建**空**文件并写回地址（首次连接由驱动初始化结构）
        let path = dir.join("new.duckdb");
        let (msg, ok, value) = create_new_db_file(&path);
        assert!(ok, "{msg}");
        assert_eq!(
            value.as_deref(),
            Some(path.to_string_lossy().to_string().as_str())
        );
        assert_eq!(std::fs::metadata(&path).expect("meta").len(), 0, "必须是空文件");

        // 已存在：不采用、不覆盖（「新建」与「打开」职责互斥：真机反馈“新建为什么还是打开”）
        std::fs::write(&path, b"keep-me").expect("write");
        let (msg, ok, value) = create_new_db_file(&path);
        assert!(!ok && value.is_none(), "{msg}");
        assert!(msg.contains("已存在"), "{msg}");
        assert_eq!(std::fs::read(&path).expect("read"), b"keep-me", "不得清空原文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tags_text_and_json_roundtrip() {
        // 中英文逗号 + 空白 + 去重；空输入 → None（不写空 JSON）。
        assert_eq!(
            tags_to_json("prod, core，prod  ,  "),
            Some(r#"["prod","core"]"#.to_string())
        );
        assert_eq!(tags_to_json("   "), None);
        assert_eq!(tags_from_json(Some(r#"["prod","core"]"#)), "prod, core");
        assert_eq!(tags_from_json(Some("not-json")), "");
        assert_eq!(tags_from_json(None), "");
    }

    #[test]
    fn driver_form_fields_come_from_config_schema() {
        // 空 / 非法 / 缺 fields：不得造默认字段（调用方自行回退）。
        assert!(driver_form_fields("").is_empty());
        assert!(driver_form_fields("not-json").is_empty());
        assert!(driver_form_fields(r#"{"options":[]}"#).is_empty());
        // 真实种子：SQLite 只声明 file_path（type=file）。
        let sqlite = r#"{"fields":[{"key":"file_path","label":"数据库文件","type":"file","required":true,"placeholder":"选择 .db 或 .sqlite 文件"}]}"#;
        let fields = driver_form_fields(sqlite);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "file_path");
        assert_eq!(fields[0].label, "数据库文件");
        assert_eq!(fields[0].kind, "file");
        assert!(fields[0].required);
        assert_eq!(address_field(&fields).map(|f| f.key.as_str()), Some("file_path"));
        // 真实种子：MySQL 声明 host/port/database/username/password（顺序保留）。
        let mysql = r#"{"fields":[{"key":"host","label":"主机","type":"text","required":true},{"key":"port","label":"端口","type":"number","required":true},{"key":"database","label":"数据库","type":"text"},{"key":"username","label":"用户名","type":"text"},{"key":"password","label":"密码","type":"password"}]}"#;
        let fields = driver_form_fields(mysql);
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[1].kind, "number");
        assert_eq!(fields[4].kind, "password");
        assert_eq!(field_spec(&fields, "database").map(|f| f.label.as_str()), Some("数据库"));
        // 未声明的键 → None（调用方据此不出该行，而不是造默认行）。
        assert!(field_spec(&fields, "file_path").is_none());
        // 缺 key / key 为空 → 丢弃该字段。
        let broken = r#"{"fields":[{"label":"无键"},{"key":"  ","label":"空键"},{"key":"ok"}]}"#;
        let fields = driver_form_fields(broken);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].key, "ok");
        assert_eq!(fields[0].label, "ok", "未声明 label 时回退键名");
    }

    #[test]
    fn short_name_strips_type_prefix() {
        assert_eq!(driver_short_name("MySQL (sqlx)"), "sqlx");
        assert_eq!(driver_short_name("MySQL (Official)"), "Official");
        assert_eq!(driver_short_name("SQLite (rusqlite)"), "rusqlite");
        assert_eq!(driver_short_name("DuckDB（duckdb-rs）"), "duckdb-rs");
        // 无括号 / 空括号：原样返回（不丢信息）。
        assert_eq!(driver_short_name("native"), "native");
        assert_eq!(driver_short_name("MySQL ()"), "MySQL ()");
        assert_eq!(driver_short_name("  sqlx  "), "sqlx");
    }

    #[test]
    fn driver_lookup_matches_id_full_name_and_short() {
        let drivers = vec![
            driver("mysql_native", "mysql", "MySQL (Official)", true),
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("postgres", "postgresql", "PostgreSQL (sqlx)", true),
        ];
        assert_eq!(
            find_driver_by_value(&drivers, "mysql").map(|d| d.id.as_str()),
            Some("mysql")
        );
        assert_eq!(
            find_driver_by_value(&drivers, "MySQL (Official)").map(|d| d.id.as_str()),
            Some("mysql_native")
        );
        assert_eq!(
            find_driver_by_value(&drivers, "Official").map(|d| d.id.as_str()),
            Some("mysql_native")
        );
        assert!(find_driver_by_value(&drivers, "").is_none());
        assert!(find_driver_by_value(&drivers, "oracle").is_none());
    }

    #[test]
    fn enabled_drivers_are_filtered_by_type() {
        let drivers = vec![
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("mysql_native", "mysql", "MySQL (Official)", false),
            driver("postgres", "postgresql", "PostgreSQL (sqlx)", true),
        ];
        let mysql = enabled_drivers_of_type(&drivers, "mysql");
        assert_eq!(mysql.len(), 1, "禁用驱动不应出现在下拉中");
        assert_eq!(mysql[0].id, "mysql");
        assert_eq!(enabled_drivers_of_type(&drivers, "oracle").len(), 0);
    }

    #[test]
    fn type_has_driver_requires_enabled_driver() {
        let drivers = vec![
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("oracle_legacy", "oracle", "Oracle (Legacy)", false),
        ];
        assert!(type_has_driver(&drivers, "mysql"));
        // 只有禁用驱动的类型视为不可用（选型入口置灰 + 拒绝切换）。
        assert!(!type_has_driver(&drivers, "oracle"));
        // 目录里完全没有驱动的类型同样不可用（与种子目录一致：只内置 4 个驱动）。
        assert!(!type_has_driver(&drivers, "clickhouse"));
    }

    /// 能力矩阵只列**驱动能力**：应用级功能（导出 / Mock / 资源）不能当行出现，
    /// 否则界面上会被读成「这个驱动不支持数据导出」。
    #[test]
    fn capability_rows_exclude_app_level_features() {
        let rows = capability_rows(&[]);
        assert!(
            !rows.iter().any(|r| r.label == "数据导出"),
            "应用级功能不该出现在驱动能力矩阵里：{:?}",
            rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>()
        );
        assert!(!rows.iter().any(|r| r.label == "Mock 生成"));
        assert!(!rows.iter().any(|r| r.label == "资源分析"));
        // 驱动能力仍在（能逐行对比）
        assert!(rows.iter().any(|r| r.label == "事务"));
        assert!(rows.iter().any(|r| r.label == "数据库导航"));

        // 三个应用级功能走另一句说明
        let app = app_level_capabilities();
        assert!(app.contains(&"数据导出".to_string()), "{app:?}");
        assert!(app.contains(&"Mock 生成".to_string()));
        assert!(app.contains(&"资源分析".to_string()));
    }

    #[test]
    fn capabilities_come_from_driver_json_with_label_dictionary() {
        // 解析：来自库里的 JSON 数组（非法 / 空 → 空列表，不造默认能力）。
        assert_eq!(driver_capabilities(None).len(), 0);
        assert_eq!(driver_capabilities(Some("not-json")).len(), 0);
        assert_eq!(
            driver_capabilities(Some(r#"["tree","health_check"]"#)),
            vec!["tree".to_string(), "health_check".to_string()]
        );
        // 矩阵：字典（引擎侧唯一一份）内的**驱动能力**逐个出一行（声明与否），
        // 驱动自带的未知键追加在尾部；行里同时带出运行时位与真机验收状态。
        // （应用级功能不在这里：见 `capability_rows_exclude_app_level_features`）
        let declared = vec!["tree".to_string(), "brand_new".to_string()];
        let rows = capability_rows(&declared);
        assert!(
            rows.iter()
                .any(|r| r.label == "数据库导航" && r.declared),
            "声明项应命中"
        );
        assert!(
            rows.iter()
                .any(|r| r.label == "模式浏览" && !r.declared),
            "字典内未声明项应标记未声明"
        );
        assert_eq!(rows.last().map(|r| r.label.as_str()), Some("brand_new"));
        // 未收录的键不假装验收（也不丢信息）
        assert!(!rows.last().expect("尾部行").acceptance.verified);
        // 事务 / 联邦两个键带运行时位；已验收项带用例名（D10 口径）
        let tx = rows
            .iter()
            .find(|r| r.label == "事务")
            .expect("事务行");
        assert_eq!(tx.meta_bit, Some(engine::driver::MetaBit::Transaction));
        assert!(tx.acceptance.verified);
        assert!(!tx.acceptance.evidence.is_empty(), "已验收必须给出可复现用例");
    }

    #[test]
    fn policy_type_label_and_summary_are_db_driven() {
        // 标签字典与反向映射必须成对（管理器按库里的 policy_type 展示、按标签写回）。
        for t in ["security", "schema", "performance", "audit", "ui"] {
            let label = policy_type_label(t);
            assert_eq!(policy_type_from_label(&label), t, "标签与类型应可往返：{t}");
        }
        // 未收录的类型原样展示（不丢失库里的信息）。
        assert_eq!(policy_type_label("custom"), "custom");
        // 摘要只取库里的标量值，最多 3 项；空/非法 → “—”。
        assert_eq!(policy_summary(None), "—");
        assert_eq!(policy_summary(Some("{}")), "—");
        let s = policy_summary(Some(
            r#"{"readonly":true,"rowLimit":1000,"queryTimeout":60,"nested":{"a":1}}"#,
        ));
        assert!(s.contains("readonly=true"), "{s}");
        assert!(s.contains("rowLimit=1000"), "{s}");
        assert!(!s.contains("nested"), "对象值不应进摘要：{s}");
        assert_eq!(s.matches('=').count(), 3, "最多 3 项：{s}");
    }

    #[test]
    fn auth_types_come_from_driver_declaration() {
        // 数据源：drivers.supported_auth_types（JSON 数组）；非法 / 空 → 空列表（UI 置灰）。
        assert_eq!(driver_auth_types(None).len(), 0);
        assert_eq!(driver_auth_types(Some("not-json")).len(), 0);
        assert_eq!(driver_auth_types(Some("[]")).len(), 0);
        assert_eq!(
            driver_auth_types(Some(r#"["password","ssl"]"#)),
            vec!["password".to_string(), "ssl".to_string()]
        );
    }

    #[test]
    fn address_placeholder_and_label_follow_driver() {
        // 标签：文件型 = 地址，网络型 = URI。
        assert_eq!(address_label(true), "地址");
        assert_eq!(address_label(false), "URI");

        // 网络型：占位来自驱动声明的 url_template + 默认端口（不再固定 mysql 示例）。
        let mut mysql = driver("mysql", "mysql", "MySQL (sqlx)", true);
        mysql.url_template = Some(
            "mysql://{username}:{password}@{host}:{port}/{database}".to_string(),
        );
        mysql.default_port = Some(3306);
        assert_eq!(
            address_placeholder(Some(&mysql), "mysql"),
            "mysql://user:password@localhost:3306/db"
        );
        // 无模板/无驱动：退回类型前缀示例与引导文案（不造连接数据）。
        let pg = driver("postgres", "postgresql", "PostgreSQL (sqlx)", true);
        assert_eq!(address_placeholder(Some(&pg), "postgresql"), "postgresql://主机:端口/数据库");
        assert_eq!(address_placeholder(None, "mysql"), "选择数据库类型与驱动后填写连接地址");

        // 文件型：提示是文件路径（选了 SQLite 不会再出现 mysql:// 示例）。
        let mut sqlite = driver("sqlite", "sqlite", "SQLite (rusqlite)", true);
        sqlite.is_file = true;
        assert!(
            address_placeholder(Some(&sqlite), "sqlite").contains(".sqlite"),
            "文件型提示应指向文件路径"
        );
        assert!(address_placeholder(None, "duckdb").contains(":memory:"));

        // 模板示例：未声明默认端口时不做臆造（写「端口」而非具体值）。
        assert_eq!(
            url_template_example("postgres://{host}:{port}/{database}", None),
            "postgres://localhost:端口/db"
        );
        assert_eq!(
            url_template_example("sqlite://{file_path}", None),
            "sqlite://路径"
        );
    }

    #[test]
    fn file_db_input_drops_credentials_and_tls() {
        // 从 MySQL 切到 SQLite：表单残留的凭据 / 网络 / TLS 不落库，策略覆盖保留。
        let mut i = DataSourceSaveInput::new("a", "sqlite", "C:/data/a.db");
        i.username = Some("root".into());
        i.password = Some("pw".into());
        i.auth_method = Some("password".into());
        i.auth_config_id = Some("auth_1".into());
        i.network_config_id = Some("net_1".into());
        i.advanced_options = Some(
            r#"{"ssl":{"mode":"require"},"network_chain":[{"kind":"ssh"}],"policy_overrides":["security"]}"#
                .into(),
        );
        strip_file_db_noise(&mut i);
        assert!(i.username.is_none() && i.password.is_none());
        assert!(i.auth_method.is_none() && i.auth_config_id.is_none());
        assert!(i.network_config_id.is_none());
        let adv: serde_json::Value =
            serde_json::from_str(i.advanced_options.as_deref().expect("adv 保留")).expect("json");
        assert!(adv.get("ssl").is_none() && adv.get("network_chain").is_none());
        assert!(adv.get("policy_overrides").is_some(), "策略覆盖仍然有效");
        // 无高级选项/非法 JSON：不 panic，不静默丢数据。
        let mut empty = DataSourceSaveInput::new("b", "duckdb", ":memory:");
        strip_file_db_noise(&mut empty);
        assert!(empty.advanced_options.is_none());
        let mut bad = DataSourceSaveInput::new("c", "duckdb", ":memory:");
        bad.advanced_options = Some("not-json".into());
        strip_file_db_noise(&mut bad);
        assert_eq!(bad.advanced_options.as_deref(), Some("not-json"));
    }

    #[test]
    fn staging_type_badge_prefers_live_form_for_current_entry() {
        // 当前条目：表单已切类型 → 显示表单类型（不等草稿写回）
        assert_eq!(staging_display_type_id("mysql", Some("sqlite")), "sqlite");
        // 表单未选类型 → 回退草稿快照
        assert_eq!(staging_display_type_id("mysql", Some("  ")), "mysql");
        // 非当前条目（无 live）→ 快照
        assert_eq!(staging_display_type_id("mysql", None), "mysql");
    }

    #[test]
    fn type_badge_uses_icon_with_default_fallback() {
        let types = vec![ds_type("mysql", "MySQL", Some("🐬")), ds_type("oracle", "Oracle", None)];
        assert_eq!(
            type_badge(&types, "mysql"),
            Some(("🐬".to_string(), "MySQL".to_string()))
        );
        assert_eq!(
            type_badge(&types, "oracle"),
            Some(("🗄".to_string(), "Oracle".to_string()))
        );
        assert_eq!(type_badge(&types, "redis"), None);
    }
}
