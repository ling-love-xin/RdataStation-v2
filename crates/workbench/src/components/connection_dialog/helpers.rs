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
                let inner = trimmed[start + open.len_utf8()..start + open.len_utf8() + rel_end].trim();
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

/// 原型 `sec-card`：section 卡片（边框 + 圆角 + 图标标题 + 内容）。
pub(crate) fn sec_card(
    theme: &Theme,
    icon: Icon,
    icon_color: Hsla,
    title: &'static str,
    body: impl IntoElement,
) -> Div {
    div()
        .flex_1()
        .min_w(rems(14.75))
        .border_1()
        .border_color(theme.colors.border)
        .rounded(px(10.))
        .bg(theme.colors.background)
        .py(rems(0.75))
        .px(rems(0.875))
        .child(
            div()
                .h_flex()
                .items_center()
                .gap(rems(0.4375))
                .mb(rems(0.625))
                .child(Icon::new(icon).size(px(14.)).text_color(icon_color))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.foreground)
                        .child(title),
                ),
        )
        .child(body)
}

/// 原型 `form-grid` 行：固定标签列（92px）+ 弹性值列。
pub(crate) fn grid_row(theme: &Theme, label: &'static str, value: impl IntoElement) -> Div {
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
                .child(label),
        )
        .child(div().flex_1().min_w(px(0.)).child(value))
}

/// 原型 `val2`：只读值框（常规 Tab 卡片内的摘要展示）。
pub(crate) fn val_readonly(theme: &Theme, text: &str) -> Div {
    let placeholder = text.is_empty() || text == "-";
    div()
        .border_1()
        .border_color(theme.colors.border)
        .rounded(rems(0.375))
        .bg(theme.colors.background)
        .px(rems(0.5625))
        .py(rems(0.25))
        .text_xs()
        .text_color(if placeholder {
            theme.colors.muted_foreground
        } else {
            theme.colors.foreground
        })
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

/// 重建编辑回读 URL（DataSource 无 url 字段，由 host/port/database 重拼）。
pub(crate) fn reconstruct_url(ds: &DataSource) -> String {
    if matches!(ds.db_type.as_str(), "sqlite" | "duckdb") {
        return format!(
            "{}:///{}",
            ds.db_type,
            ds.database.clone().unwrap_or_default()
        );
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
        capability_rows, driver_auth_types, driver_capabilities, driver_short_name,
        enabled_drivers_of_type, find_driver_by_value, policy_summary, policy_type_from_label,
        policy_type_label, tags_from_json, tags_to_json, type_badge, type_has_driver,
    };
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
