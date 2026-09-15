//! rds-settings — 设置 model。
//!
//! 分节设置项（serde 序列化，缺失字段回退默认值，向前兼容）：
//! - `appearance`：外观（主题模式）
//! - `connection_defaults`：连接默认值（建连超时、LAN 直连 TLS）
//! - `projects`：项目列表偏好（排序方式）
//! - `navigator`：数据源导航（来源标识、显示开关、属性面板宽度、facet 筛选）
//!
//! 主题模式直接复用 `gpui_kit::component::ThemeMode`（已派生
//! `Serialize/Deserialize/Default`，serde snake_case），与 gpui-kit 0.6
//! 主题系统天然对齐，避免重复枚举。
//!
//! ## 准入（2026-09-16 起）
//!
//! **字段不是"想加就加"**：每个字段都必须在 `registry.rs` 的登记表里有对应条目
//! （消费方 / 默认值 / 生效方式 / 入口 / 是否上页），否则
//! `registry::tests::every_model_leaf_is_registered` 直接失败。被裁掉的 7 个
//! "有字段、有界面行，但没有消费方"的字段见
//! `docs/architecture/settings/settings-architecture.md` §7.2；新增字段的判定标准见 §7.1。

use gpui_kit::component::ThemeMode;
use serde::{Deserialize, Serialize};

/// 顶层设置。新增字段必须带 `#[serde(default)]`，保证旧配置向前兼容。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub connection_defaults: ConnectionDefaults,
    #[serde(default)]
    pub projects: Projects,
    #[serde(default)]
    pub navigator: Navigator,
}

/// 外观：主题模式。
///
/// 界面字号不在这里：字号的权威是主题资产（`theme.font_size`），设置只记"选哪套模式"。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default)]
    pub theme_mode: ThemeMode,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::Light,
        }
    }
}

/// 连接默认值：新建连接时的超时与直连 TLS 策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDefaults {
    /// 建连超时（毫秒）：超过即判定本次尝试失败（会自动重试一次）。默认 15000。
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// 未配置 SSL 档案且目标为 LAN / 本机时，显式关闭 TLS（规避 sqlx 默认 `prefer`
    /// 握手卡顿）。默认开；公网 / 需要 TLS 的场景可关。
    #[serde(default = "default_true")]
    pub lan_disable_tls: bool,
}

impl Default for ConnectionDefaults {
    fn default() -> Self {
        Self {
            connect_timeout_ms: default_connect_timeout_ms(),
            lan_disable_tls: true,
        }
    }
}

impl gpui_kit::Global for Settings {}

/// 项目：项目名册的展示偏好（排序方式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projects {
    /// 项目列表排序方式（`last_opened` / `name` / `created`）。
    #[serde(default = "default_project_sort")]
    pub sort_mode: String,
}

impl Default for Projects {
    fn default() -> Self {
        Self {
            sort_mode: default_project_sort(),
        }
    }
}

/// 数据源导航的 facet 筛选（UI 偏好；`None` = 未启用）。
///
/// 这是**复合值**：在登记表里作为一项（`navigator.filters`）登记，子键不单独登记。
/// 归属域为唯一常驻 chips，其余为「筛选 ▾」弹层；`db_type` 存 `drivers.type_id`，
/// `driver` 存驱动 id，`tag` 存标签文本，`source` 存 `project` / `global` / `shared`。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NavigatorFilters {
    /// 归属域筛选（`project` / `global` / `shared`；`None` = 全部）。
    #[serde(default)]
    pub source: Option<String>,
    /// 数据库类型筛选（`drivers.type_id`）。
    #[serde(default)]
    pub db_type: Option<String>,
    /// 驱动 id 筛选。
    #[serde(default)]
    pub driver: Option<String>,
    /// 标签筛选。
    #[serde(default)]
    pub tag: Option<String>,
}

/// 数据源导航：来源标识展示形式、显示开关与属性面板宽度（UI 偏好，跨项目）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Navigator {
    /// 来源标识用短码（`P` / `G` / `GP`）；false 时用文字（项目 / 全局 / 共享）。
    #[serde(default = "default_true")]
    pub source_short_code: bool,
    /// 属性面板宽度（rem 倍率，随界面缩放；默认 24.5 ≈ 392px）。
    #[serde(default = "default_property_width")]
    pub property_panel_width: f32,
    /// 连接行是否显示标签 chip（默认关；开启后「≤2 chip + `+N`」）。
    #[serde(default)]
    pub show_tags: bool,
    /// 连接行是否显示归属域列（默认开）。
    #[serde(default = "default_true")]
    pub show_scope: bool,
    /// facet 筛选（类型 / 驱动 / 标签 / 归属域）。
    #[serde(default)]
    pub filters: NavigatorFilters,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            source_short_code: true,
            property_panel_width: default_property_width(),
            show_tags: false,
            show_scope: true,
            filters: NavigatorFilters::default(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_connect_timeout_ms() -> u64 {
    15_000
}
fn default_project_sort() -> String {
    "last_opened".to_string()
}
fn default_property_width() -> f32 {
    24.5
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧配置必须能解析：既缺新节（`navigator`），又带着已裁撤的节与字段
    /// （`general` / `engine` / `appearance.font_size` / `connection_defaults.default_driver`）。
    ///
    /// 裁撤是向后兼容安全的：`Settings` 未开 `deny_unknown_fields`，未知内容被忽略。
    #[test]
    fn legacy_config_with_removed_sections_still_loads() {
        let legacy = r#"{
            "general":{"language":"zh-CN","restore_last_workspace":true},
            "appearance":{"theme_mode":"dark","font_size":13.0},
            "engine":{"workspace_dir":"D:/ws","cache_dir":"D:/cache"},
            "connection_defaults":{"default_driver":"duckdb","query_timeout_ms":15000}
        }"#;
        let s: Settings = serde_json::from_str(legacy).expect("旧配置应可解析");

        // 仍在用的字段按文件里的值生效
        assert_eq!(s.appearance.theme_mode, ThemeMode::Dark);
        // 裁撤的节 / 字段回退默认，不留残影
        assert_eq!(s.connection_defaults.connect_timeout_ms, 15_000);
        assert!(s.connection_defaults.lan_disable_tls);
        assert!(s.navigator.source_short_code);
        assert_eq!(s.navigator.property_panel_width, 24.5);
        // v7 新增：标签默认不显、归属域列默认显。
        assert!(!s.navigator.show_tags, "标签默认不显示");
        assert!(s.navigator.show_scope, "归属域列默认显示");
        // v7 facet 筛选默认全空。
        assert!(s.navigator.filters.source.is_none());
        assert!(s.navigator.filters.db_type.is_none());
        assert!(s.navigator.filters.driver.is_none());
        assert!(s.navigator.filters.tag.is_none());
    }

    /// facet 筛选可序列化往返（`None` 不被写成显式 null 之外的异常形态）。
    #[test]
    fn navigator_filters_roundtrip() {
        let mut s = Settings::default();
        s.navigator.filters = NavigatorFilters {
            source: Some("global".into()),
            db_type: Some("postgres".into()),
            driver: None,
            tag: Some("prod".into()),
        };
        let text = serde_json::to_string(&s).expect("序列化");
        let back: Settings = serde_json::from_str(&text).expect("反序列化");
        assert_eq!(back.navigator.filters.source.as_deref(), Some("global"));
        assert_eq!(back.navigator.filters.db_type.as_deref(), Some("postgres"));
        assert!(back.navigator.filters.driver.is_none());
        assert_eq!(back.navigator.filters.tag.as_deref(), Some("prod"));
    }
}
