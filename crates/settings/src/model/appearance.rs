//! 外观设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default)]
    pub theme_mode: AppearanceMode,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// 界面语言（`"zh"` / `"en"`，默认中文）。变更时同步到 `shared::i18n`
    /// （全 UI 文案即时切换），见 `SettingsService::set_locale`。
    #[serde(default = "default_locale")]
    pub locale: String,
    /// 标签栏单标签最大宽度（px，默认 200）。窗口多 / 长连接名时给上限 + 省略，
    /// 关闭按钮（✕）保持完整。装配期应用到 DockSkin（vendor `tab_panel.rs` 读取）。
    #[serde(default = "default_tab_bar_max_width")]
    pub tab_bar_max_width: f32,
    /// 标签栏样式（`"tab"` / `"outline"` / `"pill"` / `"segmented"` / `"underline"`，
    /// 默认 `"tab"`），对应 `TabVariant` 五态；装配期映射后应用到 DockSkin。
    #[serde(default = "default_tab_bar_variant")]
    pub tab_bar_variant: String,
    /// 主题包 id（默认 `"rds"` = 现役 `RDS Light` / `RDS Dark`）。
    ///
    /// 值域见 `crate::theme_pack::BUILTIN`；包 = 一个 Light + 一个 Dark + 一套产品 token，
    /// 落在 `assets/themes/packs/<id>/`。变更由装配层经 `install_theme_pack_sink` 即时重装
    /// （换包必须“主题 + 产品 token”一起换，只换一半会留下上一包的语义色）。
    #[serde(default = "default_theme_pack")]
    pub theme_pack: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme_mode: AppearanceMode::Light,
            font_size: default_font_size(),
            locale: default_locale(),
            tab_bar_max_width: default_tab_bar_max_width(),
            tab_bar_variant: default_tab_bar_variant(),
            theme_pack: default_theme_pack(),
        }
    }
}

fn default_font_size() -> f32 {
    16.0
}

fn default_locale() -> String {
    "zh".to_string()
}

fn default_tab_bar_max_width() -> f32 {
    200.0
}

fn default_tab_bar_variant() -> String {
    "tab".to_string()
}

fn default_theme_pack() -> String {
    crate::theme_pack::DEFAULT_PACK.to_string()
}

/// 编辑器常规设置。
///
/// 消费方在 editor crate（`EditorHostPanel`）：行号开关即时生效（渲染同步到内核
/// `set_line_number`），缩进 / 换行 / 编码是**新文档与无会话文档**的默认值——
/// 已开文档的编码 / 换行 / 缩进仍走「重设 ▾」（文档级优先，见
/// `crates/editor/src/view/host.rs` 的重设菜单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    #[default]
    Light,
    Dark,
    System,
}
