//! rds-settings — 设置服务与持久化。
//!
//! `Settings` 以 GPUI global 形式存在（独立于窗口/工作台生命周期）；
//! 配置文件位于用户配置目录 `%APPDATA%/RdataStation/settings.json`
//! （非 Windows 回退到系统临时目录，保持可运行）。
//!
//! 主题切换即时生效：`Theme::change(mode, window, cx)` + 写回磁盘。

use std::path::PathBuf;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::App;

pub mod commands;
pub mod model;
pub mod settings_view;

use crate::model::Settings;

/// 用户配置目录：`%APPDATA%/RdataStation`。
pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("RdataStation")
    } else {
        // 非 Windows：跟随现有约定放到主目录，保证可写。
        std::env::temp_dir().join("RdataStation")
    }
}

/// 配置文件路径。
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// 从磁盘加载设置；文件不存在或解析失败时回退默认值（不覆盖坏文件）。
pub fn load_settings() -> Settings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

/// 保存设置到磁盘（幂等，失败静默——设置仍在本进程生效）。
pub fn save_settings(settings: &Settings) {
    let dir = config_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(text) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(settings_path(), text);
        }
    }
}

/// 设置服务：加载、读取、修改（含主题即时切换）。
pub struct SettingsService;

impl SettingsService {
    /// 启动时调用：加载磁盘配置并注册为 global。
    pub fn init(cx: &mut App) {
        if !cx.has_global::<Settings>() {
            cx.set_global(load_settings());
        }
    }

    /// 读取当前设置。
    pub fn get(cx: &App) -> Settings {
        cx.global::<Settings>().clone()
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
        save_settings(&settings);
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
        save_settings(&settings);
    }
}
