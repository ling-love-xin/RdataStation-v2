use super::*;

// ===== 驱动能力 / 环境策略（**清单与值来自数据库**，这里只放标签字典）=====
//
// 审计约定（见 connection-dialog-architecture §15）：UI 不自行编造业务数据——
// 能力清单取 `drivers.capabilities`、环境策略取 `environment_policies`；
// 本段只负责「键/类型 → 中文标签」的展示映射（字典），以及 JSON 解析工具。

/// 驱动能力键 → 中文标签的字典**已上收到引擎**（`engine::driver::CAPABILITY_DICTIONARY`）：
/// 那里同时定义「键 → 运行时能力位」与「真机验收状态」，避免界面另存一份键表
/// （本段只留 JSON 解析与行组装）。

/// 能力矩阵行（**字典唯一来源**：`engine::driver::CAPABILITY_DICTIONARY`）：
/// 标签 + 是否由该驱动声明 + 运行时位 + 真机验收状态 + 功能是否存在。
///
/// - `declared` 来自驱动声明（`drivers.capabilities`）；**功能未实现**的键
///   （`Stage::NotBuilt`）恒为 `false`：它们谁也不声明，界面也不该显示成「支持」；
/// - `meta_bit` / `acceptance` 直接取字典：运行时位与真机验收状态（`✓` 只给已验收）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilityRow {
    pub label: String,
    /// 驱动是否声明了该能力（`drivers.capabilities`）。
    pub declared: bool,
    /// 功能是否已经存在（未实现的键在界面上要说明白，不要让测试者去找）。
    pub not_built: bool,
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
            not_built: spec.stage == engine::driver::CapabilityStage::NotBuilt,
            meta_bit: spec.meta_bit,
            acceptance: spec.acceptance,
        })
        .collect();
    for key in declared {
        if engine::driver::capability_spec(key).is_none() {
            rows.push(CapabilityRow {
                label: key.clone(),
                declared: true,
                not_built: false,
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
    // 只画内容（chevron / 图标 / 标题）：外层（行高 / 分隔线 / 悬停 / 点击）由
    // `outline_section` 里的语义 `Button` 承担。
    //
    // **必须 `w_full()`**：Button 内层 content（gpui-component `button.rs`）是
    // `size_full + justify_center`（内容默认居中），`outline_section` 上的
    // `.justify_start()` 只作用于 Button 外层，对内容层无效——不占满宽度时
    // 卡片标题会居中显示（真机反馈「连接设置」标题不对齐；原型 `.sec-head`
    // 是 flex 默认左对齐）。占满后内容层居中失效，内部元素自然靠左。
    div()
        .w_full()
        .h_flex()
        .items_center()
        .gap(rems(GAP_SM))
        .child(
            lucide(if collapsed {
                "icons/chevron-right.svg"
            } else {
                "icons/chevron-down.svg"
            })
            .size(rems(crate::ui::ICON_SIZE_SM))
            .text_color(theme.colors.muted_foreground),
        )
        .child(
            icon.size(rems(crate::ui::ICON_SIZE_SM))
                .text_color(icon_color),
        )
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
/// 折叠走组件 `Collapsible`（决策 #108），不自己 `if !collapsed { … }`：
/// - 揭示动效是组件内的 `spring`（`motion_id` 给它稳定 identity），手删内容就变成“啪”地一跳；
/// - 带上 `motion_id` 后内容**保持挂载**（`MotionReveal` 按进度夹高度），所以收起时
///   段落不会重建、输入框不丢焦点/滚动位置。
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
    // 标题行：**语义 `Button`**（ghost）——悬停 / 焦点 / 键盘 Enter·Space / a11y 都由组件给，
    // 不再是 `cursor_pointer` + `on_click` 的 div（决策 #108；上一批 #107 是侧栏两处列表）。
    let trigger = Button::new(ElementId::Name(SharedString::from(format!("sec-{id}"))))
        .ghost()
        .w_full()
        .h(rems(ROW_H))
        .px_0()
        .justify_start()
        // 直角：底部一条通栏分隔线要贴齐面板内宽（面板圆角在外层，标题行在里面）。
        .rounded(ButtonRounded::None)
        .border_b_1()
        .border_color(theme.colors.border)
        .tooltip(if collapsed {
            tr("展开分组")
        } else {
            tr("收起分组")
        })
        .on_click(move |_, window, cx| on_toggle(window, cx))
        .child(section_header(theme, icon, icon_color, title, collapsed));
    let collapsible = Collapsible::new()
        // 稳定 identity：同一 id 才能让揭示动效量到高度变化（组件内的 spring 用它）。
        .motion_id(ElementId::Name(SharedString::from(format!(
            "sec-motion-{id}"
        ))))
        .open(!collapsed)
        // 标题行容器：`.id()` + `.test_support()` 把这一行登记成 kit 观测里的真元素
        // （`TestWindowExt::find` 取快照 / `click_at` 按局部偏移点击，都不靠手算绝对坐标）；
        // 非测试构建下 `test_support()` 是零成本转发（`#[cfg]` 分支返回原元素）。
        // 注：headless 下鼠标事件到不了对话框层（deferred 子树，本仓已两次撞到），
        // 所以对话框侧的用例用 `find` 断言“这是个可见的真元素”，真点击在
        // `tests::header_interaction` 里用不带对话框的窗口钉。
        .child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "conn-sec-{id}"
                ))))
                .test_support()
                .w_full()
                .child(trigger),
        )
        .content(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "conn-sec-body-{id}"
                ))))
                // 测试快照：内容体（收起时**仍挂载**、但被揭示进度夹成不可见——
                // `find(..).visible()` 正好能区分这两种状态）。
                .test_support()
                .w_full()
                .pt(rems(GAP_SM))
                .child(body),
        )
        .w_full();
    // 分组面板底（浅底 + 圆角 + 内距）：包住标题行与内容。
    div()
        .w_full()
        .v_flex()
        .rounded(theme.radius)
        .bg(theme.colors.group_box)
        .py(rems(GAP_SM))
        .px_2()
        .child(collapsible)
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
        .gap_1()
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
    if is_file { tr("地址") } else { tr("URI") }
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
    tr("选择数据库类型与驱动后填写连接地址").to_string()
}

/// 文件型地址占位（按类型 id 的文案字典；未知文件型驱动用通用提示）。
fn file_path_placeholder(type_id: &str) -> String {
    match type_id {
        "sqlite" => tr("选择或新建 .db / .sqlite 文件路径").to_string(),
        "duckdb" => tr("选择或新建 .duckdb 文件路径（或输入 :memory:）").to_string(),
        _ => tr("选择或新建数据库文件路径").to_string(),
    }
}

/// `url_template` → 可读示例（占位符换成示例值；端口优先用驱动声明的默认端口）。
pub(crate) fn url_template_example(template: &str, default_port: Option<i32>) -> String {
    let port = default_port
        .map(|p| p.to_string())
        .unwrap_or_else(|| tr("端口").to_string());
    template
        .replace("{username}", "user")
        .replace("{password}", "password")
        .replace("{host}", "localhost")
        .replace("{port}", &port)
        .replace("{database}", "db")
        .replace("{schema}", "public")
        .replace("{file_path}", tr("路径"))
}
