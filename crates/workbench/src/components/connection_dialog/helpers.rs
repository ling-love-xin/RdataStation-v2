use super::*;

/// 打开管理器覆盖层（嵌套 Dialog；kind: 0 认证 / 1 网络 / 2 环境）。
/// 组件默认集（`IconName`）不含数据库 / 锁 / 盾牌等图标；应用注册的是全量
/// Lucide 目录（`gpui_kit::assets::AllAssets`），故按资产路径直接加载。
pub(crate) fn lucide(path: &'static str) -> Icon {
    Icon::empty().path(path)
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

// ===== UI 尺寸约束（详见 docs/architecture/theme/ui-constraints.md）=====
//
// 颜色已有硬约束（一律 `theme.colors` token，禁裸色值）；尺寸此前靠原型约定，
// 这里把对话框用到的一组尺度固化为常量：**新代码一律引用常量**，存量代码逐步迁移。

/// 间距阶梯（rem）：xs < sm < md < lg（禁止随手值，如 0.625 / 0.4375）。
pub(crate) const GAP_XS: f32 = 0.25;
pub(crate) const GAP_SM: f32 = 0.375;
pub(crate) const GAP_MD: f32 = 0.5;
pub(crate) const GAP_LG: f32 = 0.75;

/// 行高与控件尺寸（rem）。
pub(crate) const ROW_H: f32 = 1.75;
/// Header 标签列宽（rem）：刚好容纳两字标签（名称 / 备注 / 驱动 / URI），
/// 不让标签与控件之间留下过大的空白（真机反馈：间距过大）。
pub(crate) const LABEL_W: f32 = 1.75;
/// 类型徽标（仅图标）。
pub(crate) const BADGE_W: f32 = 1.75;
pub(crate) const BADGE_H: f32 = 1.5;
/// 作用域分段项高。
pub(crate) const SEG_ITEM_H: f32 = 1.25;
/// Header 定宽控件：驱动下拉 / 项目栏。
pub(crate) const DRIVER_W: f32 = 11.0;
pub(crate) const PROJECT_W: f32 = 17.0;
/// 区域固定高度：Tab 内容 / 暂存列表（超出内部滚动）。
pub(crate) const TAB_BODY_H: f32 = 20.5;
pub(crate) const STAGING_H: f32 = 7.5;

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

/// 驱动能力键 → 中文标签（未收录的键原样展示）。
const CAPABILITY_LABELS: [(&str, &str); 12] = [
    ("tree", "数据库导航"),
    ("health_check", "健康检查"),
    ("transactions", "事务"),
    ("index_analysis", "索引分析"),
    ("sql_autocomplete", "SQL 补全"),
    ("schema_browser", "模式浏览"),
    ("table_editor", "表编辑器"),
    ("analytics", "分析查询"),
    ("federation", "联邦查询"),
    ("export", "数据导出"),
    ("mock", "Mock 生成"),
    ("resource", "资源分析"),
];

/// 解析驱动能力 JSON 数组（`drivers.capabilities`）；非法 / 为空 → 空列表。
pub(crate) fn driver_capabilities(json: Option<&str>) -> Vec<String> {
    json.and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 能力矩阵行：(显示标签, 是否由该驱动声明)。
///
/// 字典内每个键都出一行（声明与否均可视）；驱动声明了字典以外的键时追加在尾部，
/// 保证「驱动新增能力但 UI 未收录标签」时也不丢信息。
pub(crate) fn capability_rows(declared: &[String]) -> Vec<(String, bool)> {
    let mut rows: Vec<(String, bool)> = CAPABILITY_LABELS
        .iter()
        .map(|(key, label)| ((*label).to_string(), declared.iter().any(|d| d == key)))
        .collect();
    for key in declared {
        if !CAPABILITY_LABELS.iter().any(|(k, _)| k == key) {
            rows.push((key.clone(), true));
        }
    }
    rows
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

/// 分组标题行（单列大纲，可折叠）：chevron + 图标 + 标题。
///
/// 与卡片（`sec-card`）的取舍：卡片并行会在宽度不足时换行、卡高不齐、长值被挤；
/// 单列大纲只有一列宽度，标题行同时承担“分组”与“折叠手柄”（真机反馈：卡片式输入框几乎看不见）。
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
        .px(rems(GAP_XS))
        .rounded(rems(GAP_XS))
        .cursor_pointer()
        .hover(|s| s.bg(theme.colors.list_hover))
        .child(
            lucide(if collapsed {
                "icons/chevron-right.svg"
            } else {
                "icons/chevron-down.svg"
            })
            .size(px(14.))
            .text_color(theme.colors.muted_foreground),
        )
        .child(icon.size(px(14.)).text_color(icon_color))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(theme.colors.foreground)
                .child(title.to_string()),
        )
}

/// 大纲分组：标题行（点击折叠）+ 展开时的内容（左缩进与标题对齐）。
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
    let mut out = div().w_full().v_flex().gap(rems(GAP_SM)).child(header);
    if !collapsed {
        out = out.child(div().w_full().pl(rems(GAP_LG)).child(body));
    }
    out
}

/// 表单行（大纲内）：固定标签列 + 弹性控件列（原 `form-grid`）。
pub(crate) fn form_row(theme: &Theme, label: &str, value: impl IntoElement) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(rems(0.75))
        .child(
            div()
                .w(rems(5.75))
                .flex_shrink_0()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w(px(0.)).child(value))
}

/// 只读值行：标签 + **纯文本**值（不用白底白框——底色与卡片同色时会“隐形”）。
pub(crate) fn text_row(theme: &Theme, label: &str, value: &str) -> Div {
    let placeholder = value.trim().is_empty() || value == "-";
    div()
        .h_flex()
        .items_center()
        .gap(rems(0.75))
        .child(
            div()
                .w(rems(5.75))
                .flex_shrink_0()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_xs()
                .text_ellipsis()
                .text_color(if placeholder {
                    theme.colors.muted_foreground
                } else {
                    theme.colors.foreground
                })
                .child(value.to_string()),
        )
}

/// 提示行（大纲内）：弱化小字，缩进对齐控件列。
pub(crate) fn hint_line(theme: &Theme, text: &str) -> Div {
    div()
        .w_full()
        .pl(rems(5.75 + 0.75))
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
                .size(px(12.))
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

/// 地址字段：优先 `type = file`，否则按常见键名（`file_path` / `path` / `file`）。
pub(crate) fn address_field(fields: &[FormField]) -> Option<&FormField> {
    fields
        .iter()
        .find(|f| f.kind == "file")
        .or_else(|| fields.iter().find(|f| matches!(f.key.as_str(), "file_path" | "path" | "file")))
}

/// 地址行标签：取驱动声明字段的标签，未声明时退回内置（文件型 = 地址 / 网络型 = URI）。
pub(crate) fn address_row_label(fields: &[FormField], is_file: bool) -> String {
    address_field(fields)
        .map(|f| f.label.clone())
        .unwrap_or_else(|| address_label(is_file).to_string())
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
        address_field, address_label, address_placeholder, address_row_label, capability_rows,
        driver_auth_types, driver_capabilities, driver_form_fields, driver_short_name,
        enabled_drivers_of_type, field_spec, find_driver_by_value, policy_summary,
        policy_type_from_label, policy_type_label, strip_file_db_noise, tags_from_json, tags_to_json,
        type_badge, type_has_driver, url_template_example,
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
        assert_eq!(address_row_label(&fields, true), "数据库文件");
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

    #[test]
    fn capabilities_come_from_driver_json_with_label_dictionary() {
        // 解析：来自库里的 JSON 数组（非法 / 空 → 空列表，不造默认能力）。
        assert_eq!(driver_capabilities(None).len(), 0);
        assert_eq!(driver_capabilities(Some("not-json")).len(), 0);
        assert_eq!(
            driver_capabilities(Some(r#"["tree","health_check"]"#)),
            vec!["tree".to_string(), "health_check".to_string()]
        );
        // 矩阵：字典内每种能力都出一行（声明与否），驱动自带的未知键追加在尾部。
        let declared = vec!["tree".to_string(), "brand_new".to_string()];
        let rows = capability_rows(&declared);
        assert!(rows.iter().any(|(l, ok)| l == "数据库导航" && *ok), "声明项应命中");
        assert!(
            rows.iter().any(|(l, ok)| l == "Mock 生成" && !*ok),
            "字典内未声明项应标记未声明"
        );
        assert_eq!(rows.last().map(|(l, _)| l.as_str()), Some("brand_new"));
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
