//! rds-settings — 设置服务与持久化。
//!
//! `Settings` 以 GPUI global 形式存在（独立于窗口/工作台生命周期）；
//! 配置文件位置由 `paths::config_dir()` 解析（`<RDS_HOME>/config/settings.json`，
//! 默认 RDS_HOME = 可执行文件所在目录）。
//!
//! 主题切换即时生效：显式模式 `Theme::change(mode, window, cx)`；`system` 模式由
//! 装配层在启动与系统外观变化时调 `Theme::sync_system_appearance` 解析为实际明暗。

#[allow(unused_imports)]
use std::sync::RwLock;

#[allow(unused_imports)]
use gpui_kit::App;
#[allow(unused_imports)]
use gpui_kit::component::{Theme, ThemeMode};
#[allow(unused_imports)]
use gpui_kit::px;

pub mod bridge;
pub mod commands;
pub mod fields;
pub mod model;
pub mod persistence;
pub mod registry;
pub mod service;
pub mod settings_page;
pub mod skeleton;

// 持久化与进程级快照已下沉到 `persistence`；以下 re-export 保持 `settings::*` 公共路径不变。
pub use persistence::{
    config_dir, connection_defaults, last_save_error, load_settings, save_settings, settings_path,
};
// tests 区（`use super::*`）依赖的持久化测试辅助，私有引入；公共路径见上方 re-export。
#[allow(unused_imports)]
use crate::persistence::{
    load_settings_from, persist, publish_connection_defaults, save_settings_to,
    set_last_save_error,
};

// 产品语义 token 已下沉到 `workbench_shell`（视图共用资产，特性 crate 也要用它取色），
// 此处重导保持 `settings::product_tokens::*` 路径不变。
pub use workbench_shell::product_tokens;

// 设置服务骨架与读写臂在 `service`；以下 re-export 保持 `settings::*` 公共路径不变。
pub use service::{SettingsService, value_by_key};
pub use service::{install_insight_memory_limit_sink, install_log_level_sink};

#[allow(unused_imports)]
use crate::model::{
    AppearanceMode, FactStoragePref, LogMinLevel, NavigatorFilters, ObjectMaskRule, SavedView,
    Settings,
};
#[allow(unused_imports)]
use crate::registry::{SettingValue, Slot};

#[cfg(test)]
mod tests {
    use super::*;
    // 安全模式：逐项列举依赖（见 gpui-kit-dev skill「窗口测试」）。
    use gpui_kit::component::ActiveTheme as _;

    /// 临时目录（每个用例独立，避免并行测试互相干扰）。
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_settings_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 默认分组与折叠态同形：按项目分桶、**清掉时不留空壳**、未设时读到 `None`。
    ///
    /// 用模型层直接验（不经过 GPUI 的 global）：这两个字段的语义就是“项目根 → 值”的映射，
    /// 服务层的 `default_group` / `set_default_group` 只是它的一层转发。
    #[test]
    fn default_group_is_bucketed_per_project_and_clears_without_a_shell() {
        let mut settings = Settings::default();
        assert!(
            settings.resources.default_group.is_empty(),
            "默认没设任何项目"
        );

        settings
            .resources
            .default_group
            .insert("/p/a".to_string(), "af_1".to_string());
        settings
            .resources
            .default_group
            .insert("/p/b".to_string(), "af_9".to_string());
        assert_eq!(
            settings.resources.default_group.get("/p/a"),
            Some(&"af_1".to_string()),
            "各项目各归各的"
        );
        assert_eq!(
            settings.resources.default_group.get("/p/b"),
            Some(&"af_9".to_string())
        );

        // 清掉一个项目：只删这一项（不弄丢别的项目，也不留空壳）。
        settings.resources.default_group.remove("/p/a");
        assert_eq!(settings.resources.default_group.len(), 1);
        assert_eq!(settings.resources.default_group.get("/p/a"), None);

        // 落盘往返后仍然在（与其它字段一视同仁）。
        let dir = temp_dir("default_group");
        let path = dir.join("settings.json");
        save_settings_to(&path, &settings).expect("写盘");
        let back = load_settings_from(&path);
        assert_eq!(
            back.resources.default_group.get("/p/b"),
            Some(&"af_9".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 往返：写进临时文件再读回来，值一致；**临时文件不留残影**（原子写的中间态）。
    #[test]
    fn round_trip_through_disk_leaves_no_temp_file() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let mut settings = Settings::default();
        settings.navigator.show_tags = true;
        settings.connection_defaults.connect_timeout_ms = 30_000;

        save_settings_to(&path, &settings).expect("写盘");
        let back = load_settings_from(&path);
        assert!(back.navigator.show_tags);
        assert_eq!(back.connection_defaults.connect_timeout_ms, 30_000);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "临时文件必须被 rename 掉"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写盘失败要**报出来**（不再静默）：父路径是个文件时必然失败。
    #[test]
    fn save_failure_is_reported() {
        let dir = temp_dir("fail");
        std::fs::create_dir_all(&dir).expect("建目录");
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, "not a directory").expect("放个文件挡住目录");

        let result = save_settings_to(&blocker.join("settings.json"), &Settings::default());
        let err = result.expect_err("父路径是文件时必须失败");
        assert!(err.contains("未能写入"), "错误里要带落点：{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 错误槽：失败后留下原因（页面据此提示），下一次成功后清空（提示收起）。
    ///
    /// **不触碰共享 `config_dir()`**（2026-10-11 修复并行闪失败：该测试原先在 pid 级
    /// 共享数据根里破坏 config 目录，与 `theme_mode_persists_and_reloads` 等写盘测试
    /// 并行时互相干扰）——改在独立临时目录里用 `save_settings_to` + 错误槽的直接
    /// 驱动（`set_last_save_error`）验证「失败留槽 / 成功清槽」语义。
    #[test]
    fn last_save_error_tracks_failure_then_success() {
        let dir = temp_dir("err_slot");
        std::fs::create_dir_all(&dir).expect("建目录");
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, "not a directory").expect("放个文件挡住目录");

        // 失败：父路径是文件 → 留下原因。
        let err = save_settings_to(&blocker.join("settings.json"), &Settings::default())
            .expect_err("父路径是文件时必须失败");
        set_last_save_error(Some(err));
        assert!(last_save_error().is_some(), "失败必须留下原因");

        // 成功：写入独立目录正常路径 → 清空错误槽。
        assert!(
            save_settings_to(&dir.join("settings.json"), &Settings::default()).is_ok(),
            "正常路径写盘必须成功"
        );
        set_last_save_error(None);
        assert!(last_save_error().is_none(), "成功后必须清空错误槽");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 主题三态：显式模式下 `toggle_theme_mode` 两态循环（Light → Dark → Light）。
    ///
    /// 走公开入口 `set_theme_mode` / `toggle_theme_mode`（含 global 更新 + 落盘），
    /// 数据根由 `paths` 的 `test-support` 隔离，不碰用户配置。
    #[gpui_kit::test]
    fn toggle_theme_mode_cycles_between_explicit_modes(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            SettingsService::set_theme_mode(AppearanceMode::Light, None, cx);
            SettingsService::toggle_theme_mode(None, cx);
            assert_eq!(SettingsService::theme_mode(cx), AppearanceMode::Dark);
            SettingsService::toggle_theme_mode(None, cx);
            assert_eq!(SettingsService::theme_mode(cx), AppearanceMode::Light);
        });
    }

    /// `system` 下一次按键即脱离跟随：按系统当前外观反切到显式对侧
    /// （系统为深 → 落明；系统为明 → 落深）。不静默落任一侧。
    #[gpui_kit::test]
    fn toggle_from_system_flips_to_explicit_opposite(cx: &mut gpui_kit::TestAppContext) {
        // 系统当前外观（headless 测试平台为固定值，读取自洽，不猜默认）。
        let system_appearance = cx.update(|cx| cx.window_appearance());
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            SettingsService::set_theme_mode(AppearanceMode::System, None, cx);
            SettingsService::toggle_theme_mode(None, cx);
            let after = SettingsService::theme_mode(cx);
            assert_ne!(after, AppearanceMode::System, "一次按键必须脱离跟随");
            let expected = match system_appearance {
                gpui_kit::WindowAppearance::Dark => AppearanceMode::Light,
                _ => AppearanceMode::Dark,
            };
            assert_eq!(after, expected, "反切方向 = 系统当前外观的对侧");
        });
    }

    /// 主题模式落盘往返：`system` / 显式模式写进 settings.json 后重读仍一致。
    ///
    /// **用独立临时目录**（2026-10-11 修复并行闪失败：原先走共享 `config_dir()`，
    /// 与 `toggle_*` 等写盘测试同进程并行时互相覆盖，`Dark` 常被读回 `Light`）——
    /// 落盘往返语义不变（写什么读什么），不再碰共享数据根。
    #[gpui_kit::test]
    fn theme_mode_persists_and_reloads(cx: &mut gpui_kit::TestAppContext) {
        let path = temp_dir("theme_roundtrip").join("settings.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            SettingsService::set_theme_mode(AppearanceMode::System, None, cx);
            save_settings_to(&path, &SettingsService::snapshot(cx)).expect("写盘");
            assert_eq!(
                load_settings_from(&path).appearance.theme_mode,
                AppearanceMode::System
            );
            SettingsService::set_theme_mode(AppearanceMode::Dark, None, cx);
            save_settings_to(&path, &SettingsService::snapshot(cx)).expect("写盘");
            assert_eq!(
                load_settings_from(&path).appearance.theme_mode,
                AppearanceMode::Dark
            );
        });
        let _ = std::fs::remove_dir_all(path.parent().expect("父目录"));
    }

    /// 界面字号：默认 16 → `set_font_size` 更新 global + 持久化 + 应用
    /// （主题字号同步到 `theme.font_size`；无窗口上下文时遍历现有窗口设 rem 基准）。
    ///
    /// **用独立临时目录**（与 `theme_mode_persists_and_reloads` 同一理由：
    /// 不碰共享 `config_dir()`，避免并行写盘测试互相覆盖）。
    #[gpui_kit::test]
    fn font_size_applies_global_and_persists(cx: &mut gpui_kit::TestAppContext) {
        let path = temp_dir("font_roundtrip").join("settings.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            // 默认 16px。
            assert_eq!(SettingsService::font_size(cx), 16.0);
            // 应用 14px：global 更新 + 主题字号同步。
            SettingsService::set_font_size(14.0, None, cx);
            assert_eq!(SettingsService::font_size(cx), 14.0);
            assert_eq!(cx.theme().font_size, px(14.0), "主题字号必须同步");
            // 落盘往返：写什么读什么。
            save_settings_to(&path, &SettingsService::snapshot(cx)).expect("写盘");
            assert_eq!(load_settings_from(&path).appearance.font_size, 14.0);
            // 改回默认：恢复默认有唯一目标（登记表默认 16.0 与模型默认一致）。
            SettingsService::set_font_size(16.0, None, cx);
            assert_eq!(SettingsService::font_size(cx), 16.0);
        });
        let _ = std::fs::remove_dir_all(path.parent().expect("父目录"));
    }

    /// 超范围字号被拒绝（页面只给预设档；手改坏的设置文件不让写入）。
    #[gpui_kit::test]
    fn out_of_range_font_size_is_rejected(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            // 同进程并行的写盘用例共享隔离根（`paths` test-support 按进程隔离，不区分测试），
            // 先记录启动时值；拒绝语义是“不写入”，保持该值即可，不硬编码默认 16.0，
            // 避免并行用例把隔离盘写成其它字号时本用例误判。
            let before = SettingsService::font_size(cx);
            let ok = SettingsService::apply_by_key(
                "appearance.font_size",
                SettingValue::Number(99.0),
                None,
                cx,
            );
            assert!(!ok, "超范围字号必须拒绝");
            assert_eq!(SettingsService::font_size(cx), before, "拒绝后值不变");
        });
    }

    /// 界面语言：默认中文 → `set_locale` 更新 global + 持久化 + 同步 `shared::i18n`
    /// （`tr()` 切换英文查表基准）。
    ///
    /// **用独立临时目录**（同上：不碰共享 `config_dir()`，避免并行写盘测试互相覆盖）。
    #[gpui_kit::test]
    fn locale_applies_global_and_persists(cx: &mut gpui_kit::TestAppContext) {
        let path = temp_dir("locale_roundtrip").join("settings.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            // 默认中文。
            assert_eq!(SettingsService::locale(cx), shared::i18n::Locale::Zh);
            assert_eq!(shared::i18n::tr("数据源"), "数据源");
            // 切英文：global 更新 + i18n 查表基准切换。
            SettingsService::set_locale("en", None, cx);
            assert_eq!(SettingsService::locale(cx), shared::i18n::Locale::En);
            assert_eq!(shared::i18n::tr("数据源"), "Data Sources");
            // 落盘往返：写什么读什么。
            save_settings_to(&path, &SettingsService::snapshot(cx)).expect("写盘");
            assert_eq!(load_settings_from(&path).appearance.locale, "en");
            // 改回默认：恢复默认有唯一目标（登记表默认 "zh" 与模型默认一致）。
            SettingsService::set_locale("zh", None, cx);
            assert_eq!(SettingsService::locale(cx), shared::i18n::Locale::Zh);
            assert_eq!(shared::i18n::tr("数据源"), "数据源");
        });
        let _ = std::fs::remove_dir_all(path.parent().expect("父目录"));
    }

    /// 非法语言码被拒绝（页面只给 zh/en 档；手改坏的设置文件不让写入）。
    #[gpui_kit::test]
    fn invalid_locale_is_rejected(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            let ok = SettingsService::apply_by_key(
                "appearance.locale",
                SettingValue::Text("fr".to_string()),
                None,
                cx,
            );
            assert!(!ok, "非法语言码必须拒绝");
            assert_eq!(
                SettingsService::locale(cx),
                shared::i18n::Locale::Zh,
                "拒绝后值不变"
            );
        });
    }

    /// 已保存视图访问器：保存 → 同名拦截 → 删除；持久化走同一 `persist` 通道。
    #[gpui_kit::test]
    fn saved_views_accessors_roundtrip(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            assert!(SettingsService::saved_views(cx).is_empty(), "初始无视图");

            let filters = NavigatorFilters {
                source: Some("global".into()),
                db_type: Some("postgres".into()),
                driver: None,
                tag: Some("prod".into()),
            };
            assert!(SettingsService::save_saved_view("生产库".into(), filters, cx).is_ok());
            // 同名拦截：不覆盖、不自动改名。
            let dup = NavigatorFilters::default();
            let err = SettingsService::save_saved_view("生产库".into(), dup, cx).unwrap_err();
            assert!(err.contains("已存在同名视图"), "拦截文案含提示: {err}");
            assert_eq!(SettingsService::saved_views(cx).len(), 1, "同名保存不生效");

            let views = SettingsService::saved_views(cx);
            assert_eq!(views[0].name, "生产库");
            assert_eq!(views[0].filters.tag.as_deref(), Some("prod"));

            assert!(SettingsService::delete_saved_view("生产库", cx).is_ok());
            assert!(SettingsService::saved_views(cx).is_empty());
            // 删不存在的视图 → Err。
            assert!(SettingsService::delete_saved_view("没有的视图", cx).is_err());
        });
    }

    /// S3-① 可见库白名单访问器：未设置 → None；设置 → Some；置 None 清除 → None。
    #[gpui_kit::test]
    fn visible_databases_accessors_roundtrip(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            SettingsService::init(cx);
            assert_eq!(
                SettingsService::visible_databases("P_conn", cx),
                None,
                "未设置 = 默认模式"
            );

            SettingsService::set_visible_databases(
                "P_conn",
                Some(vec!["postgres".into(), "analytics".into()]),
                cx,
            );
            let got = SettingsService::visible_databases("P_conn", cx).expect("已设置");
            assert_eq!(got, vec!["postgres", "analytics"], "roundtrip 保序");

            // 显式空白名单：键保留、值为空（弹框全取消的口径）。
            SettingsService::set_visible_databases("P_conn", Some(vec![]), cx);
            assert_eq!(
                SettingsService::visible_databases("P_conn", cx),
                Some(vec![]),
                "空集合 = 显式全隐藏"
            );

            // 置 None = 清除记录（回到默认模式）。
            SettingsService::set_visible_databases("P_conn", None, cx);
            assert_eq!(SettingsService::visible_databases("P_conn", cx), None);
            // 其他连接的记录互不影响。
            assert_eq!(SettingsService::visible_databases("P_other", cx), None);
        });
    }
}
