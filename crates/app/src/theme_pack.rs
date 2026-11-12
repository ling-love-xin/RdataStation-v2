//! 主题包（theme pack）选择与加载。
//!
//! 「包」= 一个 Light + 一个 Dark + 一套产品 token，落在 `assets/themes/packs/<id>/`：
//!
//! ```text
//! assets/themes/packs/<id>/theme.json           # themes: [Light, Dark]（140 键）
//! assets/themes/packs/<id>/product-tokens.json  # light / dark 各 13 个产品角色
//! ```
//!
//! 为什么放在子目录而不是直接放 `assets/themes/`：`ThemeRegistry::reload` 与
//! `load_theme_assets` 都只扫**第一层** `*.json`，子目录天然不注册 —— 候选包因此不会
//! 出现在主题列表里，也不会影响默认行为。
//!
//! 选择方式是**两层**：设置页「外观 → 主题方案」（落盘 `appearance.theme_pack`）是常态；
//! 环境变量 `RDS_THEME_PACK=<id>` 是试色/CI 的临时覆盖（不落盘、优先于设置值）。
//! 两者都解析出 `rds` / `default` / 空时，完全走现役 `rds-theme.json` +
//! `product-tokens.json`，默认行为零变化。
//!
//! 换包必须「主题 + 产品 token 一起换」：`product-tokens.json` 只按明暗模式解析、没有
//! 主题维度，只换主题文件会让活动栏底色 / Quick Open 组头 / 搜索命中底 / 结果集四槽
//! 残留默认珊瑚包的色值（静默不一致）。

use std::path::{Path, PathBuf};

use gpui_kit::component::{ThemeConfig, ThemeMode, ThemeSet};
use settings::theme_pack::PACK_ENV;

/// 已解析的包标识（`id` 与目录）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePack {
    pub id: String,
    pub dir: PathBuf,
}

/// 解析环境变量的取值。
///
/// 空 / `rds` / `default` → `None`（走现役主题）；其余按包 id 处理。
pub fn parse_pack_id(raw: Option<&str>) -> Option<String> {
    let id = raw?.trim();
    if id.is_empty() || id.eq_ignore_ascii_case("rds") || id.eq_ignore_ascii_case("default") {
        return None;
    }
    // 包 id 直接用作目录名：挡掉路径分隔与 `..`，避免指到 themes 目录之外。
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return None;
    }
    Some(id.to_string())
}

/// 当前生效的包（**环境变量优先于落盘设置**，环境变量只用于试色/CI，不落盘）。
///
/// - `RDS_THEME_PACK=<id>` 且 `<id>` 合法 → 用它（覆盖设置页选定的方案）；
/// - 否则用 `saved_id`（设置页 `appearance.theme_pack` 的落盘值）；
/// - `rds` / `default` / 空 → `None`（走现役 `RDS Light` / `RDS Dark`）。
pub fn selected(themes_dir: &Path, saved_id: &str) -> Option<ThemePack> {
    let env_id = std::env::var(PACK_ENV).ok();
    let id = parse_pack_id(env_id.as_deref()).or_else(|| parse_pack_id(Some(saved_id)))?;
    let dir = themes_dir.join("packs").join(&id);
    Some(ThemePack { id, dir })
}

impl ThemePack {
    pub fn theme_file(&self) -> PathBuf {
        self.dir.join("theme.json")
    }

    pub fn product_tokens_file(&self) -> PathBuf {
        self.dir.join("product-tokens.json")
    }

    /// 读出包内明暗两套配置（按 `mode` 取，不依赖主题名，改名也不影响切换）。
    pub fn load_pair(&self) -> Result<(ThemeConfig, ThemeConfig), String> {
        let path = self.theme_file();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
        load_pair_from_str(&content)
    }
}

/// 从 JSON 文本取一对明暗配置（`load_pair` 的可测内核）。
pub fn load_pair_from_str(content: &str) -> Result<(ThemeConfig, ThemeConfig), String> {
    let set: ThemeSet =
        serde_json::from_str(content).map_err(|e| format!("主题包解析失败: {e}"))?;
    let light = set
        .themes
        .iter()
        .find(|t| t.mode == ThemeMode::Light)
        .cloned()
        .ok_or_else(|| "主题包缺 Light 配置".to_string())?;
    let dark = set
        .themes
        .iter()
        .find(|t| t.mode == ThemeMode::Dark)
        .cloned()
        .ok_or_else(|| "主题包缺 Dark 配置".to_string())?;
    Ok((light, dark))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packs_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes/packs")
    }

    #[test]
    fn pack_id_parsing_defaults_to_none() {
        assert_eq!(parse_pack_id(None), None);
        assert_eq!(parse_pack_id(Some("")), None);
        assert_eq!(parse_pack_id(Some("   ")), None);
        assert_eq!(parse_pack_id(Some("rds")), None);
        assert_eq!(parse_pack_id(Some("RDS")), None);
        assert_eq!(parse_pack_id(Some("default")), None);
        assert_eq!(parse_pack_id(Some(" qinghua ")), Some("qinghua".to_string()));
        // 挡掉目录穿越：包 id 会直接当目录名用。
        assert_eq!(parse_pack_id(Some("../x")), None);
        assert_eq!(parse_pack_id(Some("a/b")), None);
    }

    /// 每个已出厂的包都必须能读出 Light + Dark，且字体档位 / 键数 / 产品角色齐全。
    #[test]
    fn every_shipped_pack_loads() {
        let root = packs_root();
        let mut packs = 0;
        let entries = std::fs::read_dir(&root)
            .unwrap_or_else(|e| panic!("读不到 {}: {e}", root.display()));
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let id = dir.file_name().unwrap().to_string_lossy().to_string();
            let pack = ThemePack { id: id.clone(), dir: dir.clone() };
            let (light, dark) = pack
                .load_pair()
                .unwrap_or_else(|e| panic!("包 {id} 载入失败: {e}"));
            assert_eq!(light.mode, ThemeMode::Light, "包 {id} 第一套不是 Light");
            assert_eq!(dark.mode, ThemeMode::Dark, "包 {id} 第二套不是 Dark");
            assert_eq!(light.font_size, Some(16.0), "包 {id} Light 字号不对");
            assert_eq!(dark.font_size, Some(16.0), "包 {id} Dark 字号不对");
            assert!(light.name.starts_with("RDS "), "包 {id} 命名缺 RDS 前缀");

            // 键数：140 色 token（与现役资产同口径）。
            for mode in ["light", "dark"] {
                let raw = std::fs::read_to_string(pack.theme_file()).unwrap();
                let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
                let themes = v["themes"].as_array().unwrap();
                let me = themes
                    .iter()
                    .find(|t| t["mode"] == mode)
                    .unwrap_or_else(|| panic!("包 {id} 缺 {mode}"));
                let colors = me["colors"].as_object().unwrap();
                assert_eq!(colors.len(), 140, "包 {id} {mode} 色 token 不是 140");
            }

            let pt = pack.product_tokens_file();
            assert!(pt.exists(), "包 {id} 缺 product-tokens.json");
            let pt_raw = std::fs::read_to_string(&pt).unwrap();
            let file: settings::product_tokens::ProductTokenFile =
                serde_json::from_str(&pt_raw).unwrap_or_else(|e| panic!("包 {id} 产品 token 解析失败: {e}"));
            for (mode, map) in [("light", &file.light), ("dark", &file.dark)] {
                let defined = [
                    map.activity_bar_background.is_some(),
                    map.activity_bar_active_border.is_some(),
                    map.activity_bar_icon_active.is_some(),
                    map.activity_bar_icon_inactive.is_some(),
                    map.title_bar_slot_background.is_some(),
                    map.quick_open_group_header.is_some(),
                    map.search_match_background.is_some(),
                    map.staged_drop.is_some(),
                    map.result_odd_row_background.is_some(),
                    map.result_cell_modified_background.is_some(),
                    map.result_cell_error_background.is_some(),
                    map.result_local_row_background.is_some(),
                    map.result_header_border.is_some(),
                ];
                assert_eq!(
                    defined.iter().filter(|x| **x).count(),
                    13,
                    "包 {id} {mode} 产品角色不是 13 个"
                );
            }
            packs += 1;
        }
        assert!(packs >= 8, "出厂包数量 {packs}，期望 ≥ 8");
    }

    /// 默认路径不受影响：落盘值是默认包 + 未设环境变量时，`selected` 必须返回 `None`。
    /// （环境变量在进程内是全局的，这里只在变量确实未设置时断言，避免测试互相干扰。）
    #[test]
    fn selected_is_none_without_env() {
        if std::env::var(PACK_ENV).is_ok() {
            return;
        }
        let dir = Path::new("/tmp/themes");
        assert_eq!(selected(dir, "rds"), None);
        assert_eq!(selected(dir, ""), None);
        // 设置页选定的内置包：拼出包目录（不看文件是否存在）。
        let pack = selected(dir, "qinghua").expect("选中青花时应有包");
        assert_eq!(pack.id, "qinghua");
        assert!(pack.dir.ends_with(Path::new("packs").join("qinghua")));
    }
}
