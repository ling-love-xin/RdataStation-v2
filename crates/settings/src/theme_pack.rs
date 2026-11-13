//! 主题包（theme pack）的**可选值域**：哪些包能选、默认是哪个。
//!
//! 这里只管“值域”，不碰文件系统与主题装载：
//!
//! - 包资产的解析（`assets/themes/packs/<id>/theme.json` + `product-tokens.json`）
//!   与 `Theme` 装配在装配层 `crates/app/src/theme_pack.rs` / `main.rs`；
//! - 环境变量 [`PACK_ENV`] 是装配层的**临时覆盖**（试色/CI 用），不写进 `settings.json`，
//!   也不受本值域校验（它不落盘）。
//!
//! 为什么值域要和装载分开：登记表（`registry`）是设置项准入的代码侧权威，默认值、
//! 枚举选项、写入校验都从这一处出；而 `settings` crate 不依赖主题资产目录。

/// 试切换用的环境变量名（装配层读取，优先级高于设置值）。
pub const PACK_ENV: &str = "RDS_THEME_PACK";

/// 默认包：走现役 `assets/themes/rds-theme.json` + `product-tokens.json`，
/// 行为与历史上完全一致（不设任何东西时就是它）。
pub const DEFAULT_PACK: &str = "rds";

/// 内置可选包（`(落盘 id, 界面展示名)`）。
///
/// id 会直接当目录名用（`assets/themes/packs/<id>/`），因此**不得含路径分隔符或 `..`**；
/// 新增一档时只改这里 + 把包目录放进去，登记表与页面自动跟随。
///
/// 只放两个：第 3 个包之前必须先把 `ProductTokenSet` 升级成“按包解析”
/// （见 `docs/architecture/theme/palette-options.md` §5）。
pub const BUILTIN: &[(&str, &str)] = &[("rds", "RDS 经典"), ("qinghua", "青花")];

/// 是否内置可选包。
pub fn is_builtin(id: &str) -> bool {
    BUILTIN.iter().any(|(value, _)| *value == id)
}

/// 展示名（未登记的 id 回落到默认包的名字，页面不会显示空标签）。
pub fn label(id: &str) -> &'static str {
    BUILTIN
        .iter()
        .find(|(value, _)| *value == id)
        .map(|(_, label)| *label)
        .unwrap_or("RDS 经典")
}
