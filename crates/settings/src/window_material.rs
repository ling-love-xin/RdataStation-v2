//! 窗口材质（Windows 11 的 Mica / Mica Alt）——值域、外壳 alpha 与平台背景外观映射的
//! **唯一来源**。
//!
//! 职责边界：设置层管「值」（值域 / 校验 / 落盘 / 重放外壳 alpha），装配层管「装到窗口」
//! （`Window::set_background_appearance`，Windows 侧落到 DWM 的
//! `DWMWA_SYSTEMBACKDROP_TYPE`）。材质**不是**主题体系：它只调几个 token 的 alpha，
//! 让系统背景从窗口本身透出来，不改任何一套配色。
//!
//! 为什么只暴露三档（关闭 / 云母 / 云母 Alt）：`Transparent` 是无背景的纯透明，
//! `Blurred` 在 Windows 上回退到 `SetWindowCompositionAttribute` 老路径（亚克力），
//! 观感与稳定性都不如 Mica；两档留在映射里但不进值域，将来做实验档位只需加值域。

use gpui_kit::WindowBackgroundAppearance;

/// 默认档：关闭（不改窗口背景外观，行为与引入本功能前完全一致）。
pub const DEFAULT_CODE: &str = "off";

/// 值域（设置页选项与写入校验的唯一来源，`registry` 直接引用）。
///
/// 名字带 `MATERIALS`：`theme_pack::BUILTIN`（主题包值域）是本 crate 的同名先例，
/// 两个不同语义的常量同名会让「改一个、另一个不生效」查不出来（`ui_guard` 拦的就是这个）。
pub const BUILTIN_MATERIALS: &[(&str, &str)] = &[
    ("off", "关闭"),
    ("mica", "云母 Mica"),
    ("mica_alt", "云母 Alt"),
];

/// 窗口材质三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMaterial {
    /// 不透明（现状）。
    Off,
    /// Windows 11 的 Mica（`DWMSBT_MAINWINDOW`）。
    Mica,
    /// Windows 11 的 Mica Alt（`DWMSBT_TABBEDWINDOW`，带页签语义的更强着色）。
    MicaAlt,
}

impl WindowMaterial {
    /// 落盘值（`settings.json` 里存的就是这个字符串）。
    pub fn code(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Mica => "mica",
            Self::MicaAlt => "mica_alt",
        }
    }

    /// 从落盘值解析；未登记的值返回 `None`（写入路径据此拒绝，不替用户猜一个档位）。
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "off" => Some(Self::Off),
            "mica" => Some(Self::Mica),
            "mica_alt" => Some(Self::MicaAlt),
            _ => None,
        }
    }

    /// 是否启用系统背景（关闭时外壳 alpha 必须复原为 1.0，否则上一次的透明会残留）。
    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// 外壳面板的不透明度；`None` = 不干预（保持主题原色）。
    ///
    /// 越低越透：`Mica` 是本仓主场景（普通窗口），`Mica Alt` 自带更强着色，
    /// 给高一点的 alpha 让两者观感有差别而不是"两个都一样"。
    pub fn shell_alpha(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::Mica => Some(0.78),
            Self::MicaAlt => Some(0.86),
        }
    }

    /// 平台窗口背景外观。
    ///
    /// 非 Windows 一律 `Opaque`：macOS 的 vibrancy 是另一条路径（`Blurred`），
    /// 本次不启用——宁可"设置在这台机器上无效"，也不要出现没验证过的观感。
    pub fn background_appearance(self) -> WindowBackgroundAppearance {
        if !cfg!(windows) {
            return WindowBackgroundAppearance::Opaque;
        }
        match self {
            Self::Off => WindowBackgroundAppearance::Opaque,
            Self::Mica => WindowBackgroundAppearance::MicaBackdrop,
            Self::MicaAlt => WindowBackgroundAppearance::MicaAltBackdrop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 值域封闭：每个登记值都能解析回自己，未登记值一律 `None`（含路径穿越）。
    #[test]
    fn value_domain_is_closed() {
        assert_eq!(DEFAULT_CODE, "off", "默认档必须是关闭");
        assert!(
            BUILTIN_MATERIALS.iter().any(|(code, _)| *code == DEFAULT_CODE),
            "默认值必须在值域内（否则「恢复默认」落到档位之外）"
        );
        for (code, label) in BUILTIN_MATERIALS {
            assert!(!label.is_empty(), "`{code}` 缺显示名");
            let parsed = WindowMaterial::from_code(code).expect("登记值必须可解析");
            assert_eq!(parsed.code(), *code, "`{code}` 落盘值往返不一致");
        }
        assert_eq!(WindowMaterial::from_code("no-such"), None);
        assert_eq!(WindowMaterial::from_code("../x"), None);
        assert_eq!(WindowMaterial::from_code(""), None);
    }

    /// 关闭档不干预 alpha；启用档必须落在 (0, 1) 内（=1 等于没透，=0 等于看不见）。
    #[test]
    fn shell_alpha_is_bounded_and_off_is_none() {
        assert_eq!(WindowMaterial::Off.shell_alpha(), None);
        assert!(!WindowMaterial::Off.is_enabled());
        for material in [WindowMaterial::Mica, WindowMaterial::MicaAlt] {
            let alpha = material.shell_alpha().expect("启用档必须给 alpha");
            assert!(alpha > 0.0 && alpha < 1.0, "{material:?} 的 alpha 越界: {alpha}");
            assert!(material.is_enabled());
        }
    }

    /// 关闭档恒为不透明背景（与引入本功能前的行为一致，跨平台都要成立）。
    #[test]
    fn off_is_always_opaque() {
        assert_eq!(
            WindowMaterial::Off.background_appearance(),
            WindowBackgroundAppearance::Opaque
        );
    }

    /// Windows 上启用档落到 DWM 的两种 backdrop；非 Windows 必须降级为不透明。
    #[test]
    fn enabled_materials_map_to_backdrops_only_on_windows() {
        if cfg!(windows) {
            assert_eq!(
                WindowMaterial::Mica.background_appearance(),
                WindowBackgroundAppearance::MicaBackdrop
            );
            assert_eq!(
                WindowMaterial::MicaAlt.background_appearance(),
                WindowBackgroundAppearance::MicaAltBackdrop
            );
        } else {
            for material in [WindowMaterial::Mica, WindowMaterial::MicaAlt] {
                assert_eq!(
                    material.background_appearance(),
                    WindowBackgroundAppearance::Opaque,
                    "非 Windows 必须静默降级（不出现未验证的观感）"
                );
            }
        }
    }
}
