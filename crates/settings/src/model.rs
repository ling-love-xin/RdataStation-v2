//! rds-settings — 设置 model。
//!
//! 分节设置项（serde 序列化，缺失字段回退默认值，向前兼容）：
//! - `general`：通用（语言、启动行为）
//! - `appearance`：外观（主题模式 Light/Dark、字号）
//! - `engine`：引擎（SQLite/DuckDB 路径、缓存目录）
//! - `connection_defaults`：连接默认值（默认数据源、超时）
//!
//! 主题模式直接复用 `gpui_kit::component::ThemeMode`（已派生
//! `Serialize/Deserialize/Default`，serde snake_case），与 gpui-kit 0.6
//! 主题系统天然对齐，避免重复枚举。

use gpui_kit::component::ThemeMode;
use serde::{Deserialize, Serialize};

/// 顶层设置。新增字段必须带 `#[serde(default)]`，保证旧配置向前兼容。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub engine: Engine,
    #[serde(default)]
    pub connection_defaults: ConnectionDefaults,
}

/// 通用：语言、启动行为。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct General {
    /// 界面语言，默认 `zh-CN`。
    #[serde(default = "default_language")]
    pub language: String,
    /// 启动时恢复上次工作区。
    #[serde(default = "default_true")]
    pub restore_last_workspace: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            language: default_language(),
            restore_last_workspace: default_true(),
        }
    }
}

/// 外观：主题模式、字号。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default)]
    pub theme_mode: ThemeMode,
    /// 界面基础字号（px），默认 13。
    #[serde(default = "default_font_size")]
    pub font_size: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::Light,
            font_size: default_font_size(),
        }
    }
}

/// 引擎：查询引擎路径与缓存目录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Engine {
    /// 分析引擎目录（DuckDB 工作区等）。
    #[serde(default)]
    pub workspace_dir: String,
    /// 缓存目录。
    #[serde(default)]
    pub cache_dir: String,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            workspace_dir: String::new(),
            cache_dir: String::new(),
        }
    }
}

/// 连接默认值：新建连接时的默认数据源与超时。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDefaults {
    /// 默认数据源类型（duckdb / mysql / postgres / sqlite）。
    #[serde(default = "default_driver")]
    pub default_driver: String,
    /// 查询超时（毫秒）。
    #[serde(default = "default_timeout_ms")]
    pub query_timeout_ms: u64,
}

impl Default for ConnectionDefaults {
    fn default() -> Self {
        Self {
            default_driver: default_driver(),
            query_timeout_ms: default_timeout_ms(),
        }
    }
}

impl gpui_kit::Global for Settings {}

fn default_language() -> String {
    "zh-CN".to_string()
}
fn default_true() -> bool {
    true
}
fn default_font_size() -> f32 {
    13.0
}
fn default_driver() -> String {
    "duckdb".to_string()
}
fn default_timeout_ms() -> u64 {
    15_000
}
