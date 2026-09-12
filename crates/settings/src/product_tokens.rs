//! 产品语义 token（对齐 `rds-theme` skill §「产品语义 token 注册与消费」）。
//!
//! 为什么需要独立设施：gpui-kit 0.6 的语义 token 面（`SemanticThemeTokens` / `ColorTokens`）
//! 是**固定角色集**（background / surface / primary / …… 共 18 个），无法承载本产品的特有
//! 角色（活动栏背景、标题栏挖空槽、Quick Open 分组头、搜索命中底…）。按 rds-theme 约定，
//! 这类角色落在独立资产 `assets/themes/product-tokens.json`，由本模块加载、按明暗模式解析，
//! 并以 GPUI global 提供给视图消费，保证**视图代码零裸色值**。
//!
//! 缺失角色时按语义最接近的标准字段回退（保底，不 panic）；正式配色由资产文件给出。

use std::path::Path;

use gpui_kit::component::{try_parse_color, ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Global, Hsla};
use serde::Deserialize;

/// 资产文件结构：明 / 暗两套产品角色（hex 字符串）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProductTokenFile {
    pub light: TokenMap,
    pub dark: TokenMap,
}

/// 产品角色原始值（未解析的 hex 字符串；`None` 表示资产未定义该角色）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TokenMap {
    #[serde(rename = "activity_bar.background")]
    pub activity_bar_background: Option<String>,
    #[serde(rename = "activity_bar.active_border")]
    pub activity_bar_active_border: Option<String>,
    #[serde(rename = "activity_bar.icon.active")]
    pub activity_bar_icon_active: Option<String>,
    #[serde(rename = "activity_bar.icon.inactive")]
    pub activity_bar_icon_inactive: Option<String>,
    #[serde(rename = "title_bar.slot.background")]
    pub title_bar_slot_background: Option<String>,
    #[serde(rename = "quick_open.group.header")]
    pub quick_open_group_header: Option<String>,
    #[serde(rename = "search.match.background")]
    pub search_match_background: Option<String>,
}

/// 解析后的产品角色（缺失为 `None`，消费方经访问器回退标准字段）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductTokens {
    pub activity_bar_background: Option<Hsla>,
    pub activity_bar_active_border: Option<Hsla>,
    pub activity_bar_icon_active: Option<Hsla>,
    pub activity_bar_icon_inactive: Option<Hsla>,
    pub title_bar_slot_background: Option<Hsla>,
    pub quick_open_group_header: Option<Hsla>,
    pub search_match_background: Option<Hsla>,
}

impl ProductTokens {
    fn from_map(map: &TokenMap) -> Self {
        let parse = |v: &Option<String>| v.as_deref().and_then(|s| try_parse_color(s).ok());
        Self {
            activity_bar_background: parse(&map.activity_bar_background),
            activity_bar_active_border: parse(&map.activity_bar_active_border),
            activity_bar_icon_active: parse(&map.activity_bar_icon_active),
            activity_bar_icon_inactive: parse(&map.activity_bar_icon_inactive),
            title_bar_slot_background: parse(&map.title_bar_slot_background),
            quick_open_group_header: parse(&map.quick_open_group_header),
            search_match_background: parse(&map.search_match_background),
        }
    }

    // ===== 访问器：产品角色缺失时回退到语义最接近的标准字段 =====

    /// 活动栏背景。
    pub fn activity_bar_background(&self, theme: &Theme) -> Hsla {
        self.activity_bar_background.unwrap_or(theme.colors.secondary)
    }

    /// 活动栏激活项侧条。
    pub fn activity_bar_active_border(&self, theme: &Theme) -> Hsla {
        self.activity_bar_active_border
            .unwrap_or(theme.colors.primary)
    }

    /// 活动栏激活图标色。
    pub fn activity_bar_icon_active(&self, theme: &Theme) -> Hsla {
        self.activity_bar_icon_active
            .unwrap_or(theme.colors.foreground)
    }

    /// 活动栏未激活图标色。
    pub fn activity_bar_icon_inactive(&self, theme: &Theme) -> Hsla {
        self.activity_bar_icon_inactive
            .unwrap_or(theme.colors.muted_foreground)
    }

    /// 标题栏项目挖空槽背景。
    pub fn title_bar_slot_background(&self, theme: &Theme) -> Hsla {
        self.title_bar_slot_background.unwrap_or(theme.colors.sidebar)
    }

    /// Quick Open 分组头背景。
    pub fn quick_open_group_header(&self, theme: &Theme) -> Hsla {
        self.quick_open_group_header
            .unwrap_or(theme.colors.secondary)
    }

    /// 搜索命中文本背景。
    pub fn search_match_background(&self, theme: &Theme) -> Hsla {
        self.search_match_background.unwrap_or(theme.colors.list_active)
    }
}

/// 明暗两套解析结果（GPUI global）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductTokenSet {
    pub light: ProductTokens,
    pub dark: ProductTokens,
}

impl Global for ProductTokenSet {}

/// 读取当前模式的产品角色。
pub fn get(cx: &App) -> ProductTokens {
    let set = cx.global::<ProductTokenSet>();
    match cx.theme().mode {
        ThemeMode::Dark => set.dark,
        _ => set.light,
    }
}

/// 从 JSON 文本安装产品角色（覆盖已有 global）。
pub fn apply_from_str(content: &str, cx: &mut App) -> Result<(), String> {
    let file: ProductTokenFile =
        serde_json::from_str(content).map_err(|e| format!("产品 token 解析失败: {e}"))?;
    cx.set_global(ProductTokenSet {
        light: ProductTokens::from_map(&file.light),
        dark: ProductTokens::from_map(&file.dark),
    });
    Ok(())
}

/// 从文件安装产品角色；文件缺失时安装空集（消费方全走标准字段回退）。
pub fn apply_from_path(path: &Path, cx: &mut App) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(content) => apply_from_str(&content, cx),
        Err(e) => {
            // 资产缺失不是致命错误：安装空集，视图回退标准字段。
            if !cx.has_global::<ProductTokenSet>() {
                cx.set_global(ProductTokenSet::default());
            }
            Err(format!("产品 token 读取失败 {}: {e}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_roles_and_tolerates_missing() {
        let json = r##"{
            "light": { "activity_bar.background": "#ECECEC", "search.match.background": "#FFF3C4" },
            "dark":  { "activity_bar.background": "#333333" }
        }"##;
        let file: ProductTokenFile = serde_json::from_str(json).expect("解析");
        let light = ProductTokens::from_map(&file.light);
        assert!(light.activity_bar_background.is_some());
        assert!(light.search_match_background.is_some());
        // 未定义角色为 None（消费方回退标准字段）。
        assert!(light.activity_bar_active_border.is_none());
        let dark = ProductTokens::from_map(&file.dark);
        assert!(dark.activity_bar_background.is_some());
        assert!(dark.search_match_background.is_none());
    }
}
