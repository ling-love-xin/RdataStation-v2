//! rds-settings — 设置 model。
//!
//! 按域垂直切片：每域一个子文件（struct + 默认值 + 默认函数），`Settings` 聚合
//! 与跨域枚举（`AppearanceMode` / `LogMinLevel` / `FactStoragePref`）留在本 mod.rs。
//! 新增域 = 新建 `model/<域>.rs` + 本文件登记字段 + re-export，旧域零接触。
//!
//! ## 准入
//!
//! **字段不是"想加就加"**：每个字段都必须在 `registry.rs` 登记表里有对应条目，
//! 否则 `registry::tests::every_model_leaf_is_registered` 直接失败。
//!
pub mod appearance;
pub mod connection;
pub mod editor;
pub mod insight;
pub mod layout;
pub mod logging;
pub mod navigator;
pub mod notifications;
pub mod projects;
pub mod resources;
pub mod result_panel;
pub mod scratchpad;
pub mod startup;

use serde::{Deserialize, Serialize};

pub use appearance::{Appearance, AppearanceMode};
pub use connection::ConnectionDefaults;
pub use editor::Editor;
pub use insight::Insight;
pub use layout::Layout;
pub use logging::{Logging, LogMinLevel};
pub use navigator::{default_type_short_map, Navigator, NavigatorFilters, ObjectMaskRule, SavedView};
pub use notifications::Notifications;
pub use projects::Projects;
pub use resources::Resources;
pub use result_panel::{FactStoragePref, ResultPanel};
pub use scratchpad::{Scratchpad, ToolbarButtonCfg};
pub use startup::Startup;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub appearance: Appearance,
    /// 编辑器常规设置（行号 / 新文档缩进 / 换行 / 编码默认值；文档级「重设 ▾」优先）
    #[serde(default)]
    pub editor: Editor,
    #[serde(default)]
    pub connection_defaults: ConnectionDefaults,
    #[serde(default)]
    pub projects: Projects,
    #[serde(default)]
    pub navigator: Navigator,
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub logging: Logging,
    #[serde(default)]
    pub startup: Startup,
    #[serde(default)]
    pub scratchpad: Scratchpad,
    #[serde(default)]
    pub insight: Insight,
    /// 结果集面板偏好（事实档载体分流；A 批仅注册偏好，分流纯函数在 `shared::fact`，
    /// 触发物化路径 B 批接通消费）。
    #[serde(default)]
    pub result_panel: ResultPanel,
    /// 布局设置（侧边栏宽度、布局锁定等）。
    #[serde(default)]
    pub layout: Layout,
    /// 通知设置（自动消失时间、批量合并）。
    #[serde(default)]
    pub notifications: Notifications,
}

impl gpui_kit::Global for Settings {}

/// 布尔字段共享默认值（多个域的 `#[serde(default = "default_true")]` 引用）。
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧配置必须能解析：既缺新节（`navigator`），又带着已裁撤的节与字段
    /// （`general` / `engine` / `connection_defaults.default_driver`）。
    ///
    /// `appearance.font_size` 是**复活字段**（v7 曾裁撤，2026-10-11 按「字号缩放
    /// 进设置」决策复活）：旧配置里的 `13.0` 现在会直接生效（不再被忽略）。
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
        assert_eq!(s.appearance.theme_mode, AppearanceMode::Dark);
        // 复活字段：旧配置的数字字号直接生效（不再是"被忽略"）。
        assert_eq!(s.appearance.font_size, 13.0);
        // 裁撤的节 / 字段回退默认，不留残影
        assert_eq!(s.connection_defaults.connect_timeout_ms, 15_000);
        assert!(s.connection_defaults.lan_disable_tls);
        assert!(s.navigator.source_short_code);
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

    /// 已保存视图：序列化往返 + 空对象默认回退（旧配置无此节时不报错）。
    #[test]
    fn saved_views_roundtrip() {
        let mut s = Settings::default();
        s.navigator.saved_views.insert(
            "生产库".into(),
            NavigatorFilters {
                source: Some("project".into()),
                db_type: Some("postgres".into()),
                driver: None,
                tag: Some("prod".into()),
            },
        );
        let text = serde_json::to_string(&s).expect("序列化");
        let back: Settings = serde_json::from_str(&text).expect("反序列化");
        let view = back
            .navigator
            .saved_views
            .get("生产库")
            .expect("视图名是键");
        assert_eq!(view.source.as_deref(), Some("project"));
        assert_eq!(view.tag.as_deref(), Some("prod"));

        // 旧配置（无 saved_views 节）回退空 map，不 panic。
        let legacy: Settings = serde_json::from_str("{}").expect("旧配置可读");
        assert!(legacy.navigator.saved_views.is_empty());
    }

    /// 事实档载体默认 `auto`；旧配置（无 `result_panel` 节）回退默认，不报错。
    #[test]
    fn fact_storage_defaults_to_auto() {
        assert_eq!(Settings::default().result_panel.fact_storage, FactStoragePref::Auto);

        let legacy: Settings = serde_json::from_str("{}").expect("旧配置可读");
        assert_eq!(legacy.result_panel.fact_storage, FactStoragePref::Auto);
    }

    /// 三档序列化往返；落盘文本为 snake_case（auto / memory / table）。
    #[test]
    fn fact_storage_roundtrip_three_tiers() {
        for pref in FactStoragePref::ALL {
            let mut settings = Settings::default();
            settings.result_panel.fact_storage = pref;
            let text = serde_json::to_string(&settings).expect("序列化");
            let back: Settings = serde_json::from_str(&text).expect("反序列化");
            assert_eq!(back.result_panel.fact_storage, pref);
            assert!(text.contains(&format!(r#""fact_storage":"{}""#, pref.as_str())));
        }
    }

    /// 三档映射到决策层 `shared::StoragePref`；未知落盘值拒绝解析，不猜。
    #[test]
    fn fact_storage_maps_to_shared_and_rejects_unknown() {
        assert_eq!(FactStoragePref::Auto.to_shared(), ::shared::StoragePref::Auto);
        assert_eq!(FactStoragePref::Memory.to_shared(), ::shared::StoragePref::Memory);
        assert_eq!(FactStoragePref::Table.to_shared(), ::shared::StoragePref::Table);

        assert!(FactStoragePref::parse("disk").is_none());
        assert!(FactStoragePref::parse("").is_none());
        for pref in FactStoragePref::ALL {
            assert_eq!(FactStoragePref::parse(pref.as_str()), Some(pref));
        }
    }
}
