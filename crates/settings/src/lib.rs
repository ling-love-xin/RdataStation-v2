//! rds-settings — 设置服务与持久化。
//!
//! `Settings` 以 GPUI global 形式存在（独立于窗口/工作台生命周期）；
//! 配置文件位于用户配置目录 `%APPDATA%/RdataStation/settings.json`
//! （非 Windows 回退到系统临时目录，保持可运行）。
//!
//! 主题切换即时生效：`Theme::change(mode, window, cx)` + 写回磁盘。

use std::path::PathBuf;
use std::sync::RwLock;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::App;

pub mod commands;
pub mod model;
pub mod product_tokens;
pub mod registry;
pub mod settings_page;
pub mod settings_view;
pub mod ui;

use crate::model::{ConnectionDefaults, NavigatorFilters, Settings};
use crate::registry::{SettingValue, Slot};

/// 进程级连接默认值快照：供**无 `App` 的异步连接路径**（`ConnectionService`）读取。
///
/// 由 `save_settings` / `SettingsService::init` 发布：async 上下文拿不到 GPUI global，
/// 但又需要「建连超时 / LAN 关 TLS」这两个参数。
static CONNECTION_DEFAULTS: RwLock<Option<ConnectionDefaults>> = RwLock::new(None);

/// 上一次落盘失败的原因（`None` = 上一次落盘成功 / 还没写过）。
///
/// 为什么要有这个槽：落盘失败**不影响本进程生效**（值已经进了 global），
/// 但用户必须看得见——否则"改了没存住"是完全静默的（架构 §13 K2）。
static LAST_SAVE_ERROR: RwLock<Option<String>> = RwLock::new(None);

/// 测试用配置目录覆盖（**只在测试构建存在**；生产代码永远不会设置它）。
#[cfg(test)]
static CONFIG_DIR_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 把配置目录指到临时目录（仅测试用）。
#[cfg(test)]
pub(crate) fn set_config_dir_for_tests(dir: PathBuf) {
    if let Ok(mut guard) = CONFIG_DIR_OVERRIDE.write() {
        *guard = Some(dir);
    }
}

/// 读取连接默认值（未发布时回退模型默认）。
pub fn connection_defaults() -> ConnectionDefaults {
    CONNECTION_DEFAULTS
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

/// 发布连接默认值快照（内部使用；`save_settings` / `init` 调用）。
fn publish_connection_defaults(defaults: &ConnectionDefaults) {
    if let Ok(mut guard) = CONNECTION_DEFAULTS.write() {
        *guard = Some(defaults.clone());
    }
}

/// 用户配置目录：`%APPDATA%/RdataStation`。
pub fn config_dir() -> PathBuf {
    // 测试路径注入（仅测试构建）：避免测试碰用户真实配置。
    #[cfg(test)]
    if let Some(dir) = CONFIG_DIR_OVERRIDE.read().ok().and_then(|g| g.clone()) {
        return dir;
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("RdataStation")
    } else {
        // 非 Windows：跟随现有约定放到主目录，保证可写（见 K5：将来改走平台配置目录）。
        std::env::temp_dir().join("RdataStation")
    }
}

/// 配置文件路径。
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// 从指定路径加载设置；文件不存在或解析失败时回退默认值（**不覆盖**坏文件）。
pub fn load_settings_from(path: &std::path::Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

/// 从磁盘加载设置（用户配置目录）。
pub fn load_settings() -> Settings {
    load_settings_from(&settings_path())
}

/// 保存到指定路径：**原子写**（临时文件 + rename）。
///
/// 为什么不是直写：直写时进程在写一半退出，`settings.json` 就会是半截 JSON——
/// 下次启动解析失败即回退默认，等于把用户配置全丢了。
pub fn save_settings_to(path: &std::path::Path, settings: &Settings) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("配置路径没有父目录：{}", path.display()))?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    let result = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, &text))
        .and_then(|()| std::fs::rename(&tmp, path));
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            // 失败的临时文件不留在用户配置目录里。
            let _ = std::fs::remove_file(&tmp);
            Err(format!("未能写入 {}：{e}", path.display()))
        }
    }
}

/// 上一次落盘失败的原因（不清除；下一次成功后自动清空）。
pub fn last_save_error() -> Option<String> {
    LAST_SAVE_ERROR.read().ok().and_then(|g| g.clone())
}

/// 记录落盘结果。
fn set_last_save_error(err: Option<String>) {
    if let Ok(mut guard) = LAST_SAVE_ERROR.write() {
        *guard = err;
    }
}

/// 保存设置到磁盘（幂等）。
///
/// 失败时**不改本进程已生效的值**（已进 global），只记录原因供界面提示
/// （[`last_save_error`]）。
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    // 先发布快照：本进程内的消费方（异步连接路径等）不受落盘结果影响。
    publish_connection_defaults(&settings.connection_defaults);
    let result = save_settings_to(&settings_path(), settings);
    set_last_save_error(result.as_ref().err().cloned());
    result
}

/// 落盘并忽略返回值（失败原因已记进 `LAST_SAVE_ERROR`，由设置页展示）。
///
/// 各 `set_*` 走这里：它们不因落盘失败而回滚内存值，也不向调用方改签名。
fn persist(settings: &Settings) {
    let _ = save_settings(settings);
}

/// 设置服务：加载、读取、修改（含主题即时切换）。
pub struct SettingsService;

/// 主题模式的文本形态（落盘值，与 `registry` 的枚举选项同源）。
fn theme_mode_text(mode: ThemeMode) -> String {
    serde_json::to_value(mode)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// 文本 → 主题模式；未知文本返回 `None`（**不猜**，由调用方拒绝并提示）。
fn theme_mode_from_text(text: &str) -> Option<ThemeMode> {
    serde_json::from_value(serde_json::Value::String(text.to_string())).ok()
}

/// 按 key 读当前值（页面渲染、「非默认值」判定与契约测试共用）。
///
/// 返回 `None`：key 未登记，或该项是复合值（标量类型表达不了）。
pub fn value_by_key(settings: &Settings, key: &str) -> Option<SettingValue> {
    let slot = registry::slot_for(key)?;
    Some(match slot {
        Slot::AppearanceThemeMode => {
            SettingValue::Text(theme_mode_text(settings.appearance.theme_mode))
        }
        Slot::NavigatorSourceShortCode => SettingValue::Bool(settings.navigator.source_short_code),
        Slot::NavigatorShowTags => SettingValue::Bool(settings.navigator.show_tags),
        Slot::NavigatorShowScope => SettingValue::Bool(settings.navigator.show_scope),
        Slot::NavigatorPropertyWidth => {
            SettingValue::Number(settings.navigator.property_panel_width as f64)
        }
        // 复合值（facet 筛选）没有标量形态：它的读写走导航面板自己的入口。
        Slot::NavigatorFilters => return None,
        Slot::ConnectTimeoutMs => {
            SettingValue::Number(settings.connection_defaults.connect_timeout_ms as f64)
        }
        Slot::LanDisableTls => SettingValue::Bool(settings.connection_defaults.lan_disable_tls),
        Slot::ProjectSortMode => SettingValue::Text(settings.projects.sort_mode.clone()),
    })
}

impl SettingsService {
    /// 启动时调用：加载磁盘配置并注册为 global。
    pub fn init(cx: &mut App) {
        if !cx.has_global::<Settings>() {
            let settings = load_settings();
            publish_connection_defaults(&settings.connection_defaults);
            cx.set_global(settings);
        }
    }

    /// 读取当前设置。
    pub fn get(cx: &App) -> Settings {
        cx.global::<Settings>().clone()
    }

    /// 按 key 写值（**唯一写入路径**）：改 global → 落盘 → 按生效方式通知。
    ///
    /// 分发靠 `registry::{slot_for, slot_kind}`（形态不符由契约测试提前拦住）。
    /// 返回 `false` 表示拒绝（key 未登记 / 值形态不符 / 复合值 / 文本无法解析）——
    /// 调用方（页面）据此提示，**不静默吞掉**。
    pub fn apply_by_key(
        key: &str,
        value: SettingValue,
        window: Option<&mut gpui_kit::Window>,
        cx: &mut App,
    ) -> bool {
        let Some(slot) = registry::slot_for(key) else {
            return false;
        };
        match slot {
            Slot::AppearanceThemeMode => {
                let Some(mode) = value.as_text().and_then(theme_mode_from_text) else {
                    return false;
                };
                Self::set_theme_mode(mode, window, cx);
            }
            Slot::NavigatorSourceShortCode => {
                let Some(on) = value.as_bool() else { return false };
                Self::set_source_short_code(on, cx);
            }
            Slot::NavigatorShowTags => {
                let Some(on) = value.as_bool() else { return false };
                Self::set_show_tags(on, cx);
            }
            Slot::NavigatorShowScope => {
                let Some(on) = value.as_bool() else { return false };
                Self::set_show_scope(on, cx);
            }
            Slot::NavigatorPropertyWidth => {
                let Some(rem) = value.as_number() else { return false };
                Self::set_property_panel_width(rem as f32, cx);
            }
            // 复合值没有标量写入路径：拒绝而不是"猜一半"。
            Slot::NavigatorFilters => return false,
            Slot::ConnectTimeoutMs => {
                let Some(ms) = value.as_number() else { return false };
                Self::set_connect_timeout_ms(ms.max(0.) as u64, cx);
            }
            Slot::LanDisableTls => {
                let Some(on) = value.as_bool() else { return false };
                Self::set_lan_disable_tls(on, cx);
            }
            Slot::ProjectSortMode => {
                let Some(text) = value.as_text() else { return false };
                Self::set_project_sort_mode(text, cx);
            }
        }
        true
    }

    /// 读取当前主题模式。
    pub fn theme_mode(cx: &App) -> ThemeMode {
        cx.global::<Settings>().appearance.theme_mode
    }

    /// 设置主题模式：更新 global → 持久化 → `Theme::change` 即时生效。
    pub fn set_theme_mode(mode: ThemeMode, window: Option<&mut gpui_kit::Window>, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.appearance.theme_mode = mode;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        Theme::change(mode, window, cx);
    }

    /// 明暗切换（ToggleThemeMode / 设置面板按钮共用）。
    pub fn toggle_theme_mode(window: Option<&mut gpui_kit::Window>, cx: &mut App) {
        let next = match Self::theme_mode(cx) {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        };
        Self::set_theme_mode(next, window, cx);
    }

    /// 读取项目列表排序方式（`last_opened` / `name` / `created`）。
    pub fn project_sort_mode(cx: &App) -> String {
        cx.global::<Settings>().projects.sort_mode.clone()
    }

    /// 设置并持久化项目列表排序方式（更新 global → 写 settings.json）。
    pub fn set_project_sort_mode(mode: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.projects.sort_mode = mode.to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }

    /// 来源标识是否用短码（`P` / `G` / `GP`）。
    pub fn source_short_code(cx: &App) -> bool {
        cx.global::<Settings>().navigator.source_short_code
    }

    /// 设置并持久化来源标识形式（短码 ⇄ 文字）。
    pub fn set_source_short_code(short_code: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.source_short_code = short_code;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        // 导航面板按需重渲染（设置与面板是不同实体，不通知不会即时刷新）。
        cx.refresh_windows();
    }

    /// 属性面板宽度（rem 倍率）。
    pub fn property_panel_width(cx: &App) -> f32 {
        cx.global::<Settings>().navigator.property_panel_width
    }

    /// 连接行是否显示标签 chip。
    pub fn show_tags(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_tags
    }

    /// 设置并持久化「显示标签」开关。
    pub fn set_show_tags(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_tags = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }

    /// 连接行是否显示归属域列。
    pub fn show_scope(cx: &App) -> bool {
        cx.global::<Settings>().navigator.show_scope
    }

    /// 设置并持久化「显示归属域」开关。
    pub fn set_show_scope(show: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.show_scope = show;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }

    /// 导航 facet 筛选（类型 / 驱动 / 标签 / 归属域）。
    pub fn nav_filters(cx: &App) -> NavigatorFilters {
        cx.global::<Settings>().navigator.filters.clone()
    }

    /// 设置并持久化导航 facet 筛选。
    pub fn set_nav_filters(filters: NavigatorFilters, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.filters = filters;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        cx.refresh_windows();
    }

    /// 建连超时（毫秒）。
    pub fn connect_timeout_ms(cx: &App) -> u64 {
        cx.global::<Settings>()
            .connection_defaults
            .connect_timeout_ms
    }

    /// 设置并持久化建连超时（毫秒）。
    pub fn set_connect_timeout_ms(ms: u64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.connection_defaults.connect_timeout_ms = ms;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }

    /// LAN / 本机直连是否显式关闭 TLS。
    pub fn lan_disable_tls(cx: &App) -> bool {
        cx.global::<Settings>().connection_defaults.lan_disable_tls
    }

    /// 设置并持久化「LAN 直连关闭 TLS」。
    pub fn set_lan_disable_tls(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.connection_defaults.lan_disable_tls = on;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }

    /// 设置并持久化属性面板宽度（rem 倍率）。
    pub fn set_property_panel_width(width: f32, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.navigator.property_panel_width = width;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时目录（每个用例独立，避免并行测试互相干扰）。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_settings_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
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
    /// 走的是公开入口 `save_settings` + 配置目录注入，因此覆盖到真实路径拼装。
    #[test]
    fn last_save_error_tracks_failure_then_success() {
        let dir = temp_dir("slot");
        std::fs::create_dir_all(&dir).expect("建目录");
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, "x").expect("挡住");

        set_config_dir_for_tests(blocker.join("cfg"));
        assert!(save_settings(&Settings::default()).is_err());
        assert!(last_save_error().is_some(), "失败必须留下原因");

        set_config_dir_for_tests(dir.join("cfg"));
        assert!(save_settings(&Settings::default()).is_ok());
        assert!(last_save_error().is_none(), "成功后必须清空错误槽");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
