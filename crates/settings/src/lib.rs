//! rds-settings — 设置服务与持久化。
//!
//! `Settings` 以 GPUI global 形式存在（独立于窗口/工作台生命周期）；
//! 配置文件位置由 `paths::config_dir()` 解析（`<RDS_HOME>/config/settings.json`，
//! 默认 RDS_HOME = 可执行文件所在目录）。
//!
//! 主题切换即时生效：`Theme::change(mode, window, cx)` + 写回磁盘。

use std::path::PathBuf;
use std::sync::RwLock;

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeMode};

pub mod commands;
pub mod model;
pub mod registry;
pub mod settings_page;

// 产品语义 token 已下沉到 `workbench_shell`（视图共用资产，特性 crate 也要用它取色），
// 此处重导保持 `settings::product_tokens::*` 路径不变。
pub use workbench_shell::product_tokens;

use crate::model::{ConnectionDefaults, LogMinLevel, NavigatorFilters, Settings};
use crate::registry::{SettingValue, Slot};

/// 日志级别变更的副作用出口（**由装配层注册一次**，见 `crates/app/src/main.rs`）。
///
/// 为什么要这个槽：级别改完要让日志系统立刻 reload，而 `settings` 不依赖 `engine`
/// （依赖只向下；反向依赖会把双引擎拖进设置层）。装配层把
/// `engine::logging::reload_log_level` 注册进来，设置层只管"值变了，通知一声"。
static LOG_LEVEL_SINK: RwLock<Option<fn(LogMinLevel)>> = RwLock::new(None);

/// 注册日志级别变更出口（重复注册覆盖前一个；未注册时变更只落盘）。
pub fn install_log_level_sink(sink: fn(LogMinLevel)) {
    if let Ok(mut guard) = LOG_LEVEL_SINK.write() {
        *guard = Some(sink);
    }
}

/// 通知日志级别变更（无出口时静默——例如测试环境）。
fn notify_log_level(level: LogMinLevel) {
    if let Some(sink) = LOG_LEVEL_SINK.read().ok().and_then(|guard| *guard) {
        sink(level);
    }
}

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

/// 全局设置目录：`<RDS_HOME>/config`（默认 RDS_HOME = 可执行文件所在目录，即安装目录）。
///
/// 路径口径全在 [`paths`]：本 crate **不自拼** `APPDATA` / 目录名（见 `docs/architecture/runtime/data-paths.md`）。
/// 测试构建的数据根本身由 `paths` 的 `test-support` 隔离（各成员在 `[dev-dependencies]` 打开），
/// 因此这里不需要任何测试专用分支。
pub fn config_dir() -> PathBuf {
    paths::config_dir()
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
        Slot::ResourcesKeepVersions => {
            SettingValue::Number(settings.resources.keep_versions as f64)
        }
        Slot::ResourcesDefaultSort => SettingValue::Text(settings.resources.default_sort.clone()),
        // 复合值（折叠状态）：读写走资产库面板自己的入口。
        Slot::ResourcesCollapsedGroups => return None,
        // 复合值（默认分组）：同上（入口在分组头右键）。
        Slot::ResourcesDefaultGroup => return None,
        Slot::ConnectTimeoutMs => {
            SettingValue::Number(settings.connection_defaults.connect_timeout_ms as f64)
        }
        Slot::LanDisableTls => SettingValue::Bool(settings.connection_defaults.lan_disable_tls),
        Slot::ProjectSortMode => SettingValue::Text(settings.projects.sort_mode.clone()),
        Slot::LogMinLevel => SettingValue::Text(settings.logging.min_level.as_str().to_string()),
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
                let Some(on) = value.as_bool() else {
                    return false;
                };
                Self::set_source_short_code(on, cx);
            }
            Slot::NavigatorShowTags => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                Self::set_show_tags(on, cx);
            }
            Slot::NavigatorShowScope => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                Self::set_show_scope(on, cx);
            }
            Slot::NavigatorPropertyWidth => {
                let Some(rem) = value.as_number() else {
                    return false;
                };
                Self::set_property_panel_width(rem as f32, cx);
            }
            // 复合值没有标量写入路径：拒绝而不是"猜一半"。
            Slot::NavigatorFilters => return false,
            Slot::ResourcesCollapsedGroups | Slot::ResourcesDefaultGroup => return false,
            Slot::ResourcesKeepVersions => {
                let Some(value) = value.as_number() else {
                    return false;
                };
                // 页面上只给预设档（含 `-1` = 全留），取整后写入。
                Self::set_keep_versions(value.round() as i64, cx);
            }
            Slot::ResourcesDefaultSort => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                Self::set_default_resource_sort(text, cx);
            }
            Slot::ConnectTimeoutMs => {
                let Some(ms) = value.as_number() else {
                    return false;
                };
                Self::set_connect_timeout_ms(ms.max(0.) as u64, cx);
            }
            Slot::LanDisableTls => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                Self::set_lan_disable_tls(on, cx);
            }
            Slot::ProjectSortMode => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                Self::set_project_sort_mode(text, cx);
            }
            Slot::LogMinLevel => {
                let Some(level) = value.as_text().and_then(LogMinLevel::parse) else {
                    return false;
                };
                Self::set_log_min_level(level, cx);
            }
        }
        true
    }

    /// 当前日志最低级别。
    pub fn log_min_level(cx: &App) -> LogMinLevel {
        cx.global::<Settings>().logging.min_level
    }

    /// 设置并持久化日志最低级别，并通知日志系统即时 reload。
    ///
    /// 通知走 [`install_log_level_sink`] 注册的出口（装配层把 `engine` 的
    /// `reload_log_level` 接在那里）；没有出口时只落盘，不报错。
    pub fn set_log_min_level(level: LogMinLevel, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.logging.min_level = level;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_log_level(level);
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

    /// 明暗切换（`ToggleThemeMode` 命令与设置页「主题模式」两条路径共用）。
    ///
    /// 注：`ToggleThemeMode` 目前**未绑键位**（架构 §14 Q2 未拍板：接线还是删除）；
    /// 设置页走的是 `set_theme_mode`（显式指定目标模式），不依赖本函数。
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

    /// 每份存档保留的历史内容副本份数（`-1` = 全留，`0` = 只留元数据）。
    ///
    /// 返回的是**落盘口径的原始数**：转成领域类型的那一步在宿主侧做
    /// （设置层不依赖 M6，见 `analytics_resource::model::KeepVersions::from_setting`）。
    pub fn keep_versions(cx: &App) -> i64 {
        cx.global::<Settings>().resources.keep_versions
    }

    /// 设置并持久化历史内容保留份数。
    pub fn set_keep_versions(value: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.resources.keep_versions = value;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }

    /// 资产库列表的默认排序字段（落盘 key 形如 `name` / `archived_at`）。
    ///
    /// 返回原始文本：**认不认得出这个 key 是 M6 的事**（设置层不认识 `SortField`）。
    pub fn default_resource_sort(cx: &App) -> String {
        cx.global::<Settings>().resources.default_sort.clone()
    }

    /// 设置并持久化资产库列表的默认排序字段。
    pub fn set_default_resource_sort(key: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.resources.default_sort = key.to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }

    /// 某个项目里折叠的分组 key 列表（空 = 没有折叠项）。
    ///
    /// `project_root` 用绝对路径的字符串形式（与 `Shared::project_root` 同一来源）；
    /// 哪些 key 还算数由资产库自己判（面板会丢掉已删分组的标记）。
    pub fn collapsed_groups(project_root: &str, cx: &App) -> Vec<String> {
        cx.global::<Settings>()
            .resources
            .collapsed_groups
            .get(project_root)
            .cloned()
            .unwrap_or_default()
    }

    /// 覆盖某个项目的折叠分组列表（空列表 = 删掉该项目的记录，不留空壳）。
    pub fn set_collapsed_groups(project_root: &str, keys: &[String], cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            if keys.is_empty() {
                settings.resources.collapsed_groups.remove(project_root);
            } else {
                settings
                    .resources
                    .collapsed_groups
                    .insert(project_root.to_string(), keys.to_vec());
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }

    /// 某个项目的默认分组 id（`None` = 没设 / 设的分组已删，即未分组）。
    ///
    /// 与 [`collapsed_groups`](Self::collapsed_groups) 同一形态（按项目根分桶）。
    /// **不在这里校验分组是否还在**：那份名单在项目库里，只有资产库自己知道——
    /// 认不出的 id 由调用方按未分组处理（面板渲染标记 / 归档对话框都这么做）。
    pub fn default_group(project_root: &str, cx: &App) -> Option<String> {
        cx.global::<Settings>()
            .resources
            .default_group
            .get(project_root)
            .cloned()
    }

    /// 设 / 清某个项目的默认分组（`None` = 清掉该项目的记录，不留空壳）。
    pub fn set_default_group(project_root: &str, folder_id: Option<&str>, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            match folder_id {
                Some(id) => {
                    settings
                        .resources
                        .default_group
                        .insert(project_root.to_string(), id.to_string());
                }
                None => {
                    settings.resources.default_group.remove(project_root);
                }
            }
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
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
    /// 走公开入口 `save_settings` + 真实路径拼装；数据根本身由 `paths` 的 `test-support`
    /// 隔离（`docs/architecture/runtime/data-paths.md` §5），这里只需把 `config` **目录**
    /// 暂时挡住——在它的位置放一个同名文件即可。
    #[test]
    fn last_save_error_tracks_failure_then_success() {
        let config = config_dir();
        let _ = std::fs::remove_dir_all(&config);
        std::fs::write(&config, "blocked").expect("在 config 目录位置放一个文件");
        assert!(
            save_settings(&Settings::default()).is_err(),
            "目录位置被文件占住时必须失败"
        );
        assert!(last_save_error().is_some(), "失败必须留下原因");

        std::fs::remove_file(&config).expect("挪开挡住的文件");
        assert!(save_settings(&Settings::default()).is_ok());
        assert!(last_save_error().is_none(), "成功后必须清空错误槽");
    }
}
