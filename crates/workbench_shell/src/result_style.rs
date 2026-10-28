//! 结果集「类型样式」偏好模型（2026-10-28 拍板：按类型族 / 全局统一 / 预设色板）。
//!
//! 放本 crate 的原因：`settings`（持久化 + 设置页）与 `result_panel`（渲染消费）
//! 都要用这套枚举与聚合类型，放进外壳层打破双向依赖（与 [`product_tokens`] 同例）。
//! 本模块是**纯数据 + parse**，不含渲染决策（对齐/颜色的"继承默认"怎么落到渲染，
//! 由 result_panel 的 `cell_style` / `format_cell_text` 消费时解析）。

use serde::{Deserialize, Serialize};

/// 结果集列的类型族（用户口径六族：数字 / 文本 / 时间 / 布尔 / NULL / BLOB）。
///
/// `ColumnKind`（result_panel）→ 本族的映射在 result_panel 侧做；这里只定义族本身
/// 与设置项（NULL 是**值级**不是列级，不在本族内——NULL 的显示偏好单独成项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeFamily {
    /// 数字族：Integer / Float / Decimal
    Numeric,
    /// 文本族：Text / Json / Uuid / Unknown（未知类型按文本渲染）
    Text,
    /// 时间族：Timestamp / Date / Time
    Temporal,
    /// 布尔族：Boolean
    Boolean,
    /// BLOB 族：Binary
    Blob,
}

impl TypeFamily {
    /// 设置页与日志用的显示名（不带族字样的短名）。
    pub const ALL: [TypeFamily; 5] = [
        TypeFamily::Numeric,
        TypeFamily::Text,
        TypeFamily::Temporal,
        TypeFamily::Boolean,
        TypeFamily::Blob,
    ];

    /// 落盘 / 设置页键后缀（`align_numeric` / `color_blob` …）。
    pub fn key_suffix(self) -> &'static str {
        match self {
            TypeFamily::Numeric => "numeric",
            TypeFamily::Text => "text",
            TypeFamily::Temporal => "temporal",
            TypeFamily::Boolean => "bool",
            TypeFamily::Blob => "blob",
        }
    }

    /// 设置页显示名（分组行标签）。
    pub fn label(self) -> &'static str {
        match self {
            TypeFamily::Numeric => "数字",
            TypeFamily::Text => "文本",
            TypeFamily::Temporal => "时间",
            TypeFamily::Boolean => "布尔",
            TypeFamily::Blob => "BLOB",
        }
    }
}

/// 数字显示格式（DBeaver「数值格式」同向；只影响渲染显示，复制 / 导出仍用原始值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormatPref {
    /// 原始值：驱动 / 引擎给的字符串原样显示（默认）。
    #[default]
    Raw,
    /// 千分位：`1234567.89` → `1,234,567.89`（整数部分三位一组）。
    Grouped,
    /// 两位小数：数值统一显示两位小数（`1234.5` → `1234.50`）。
    Fixed2,
    /// 科学计数：`123456.789` → `1.234568e5`（与 Rust `{:e}` 一致）。
    Scientific,
}

impl NumberFormatPref {
    /// 落盘 / 设置页值（`raw` / `grouped` / `fixed2` / `scientific`）。
    pub fn as_str(self) -> &'static str {
        match self {
            NumberFormatPref::Raw => "raw",
            NumberFormatPref::Grouped => "grouped",
            NumberFormatPref::Fixed2 => "fixed2",
            NumberFormatPref::Scientific => "scientific",
        }
    }

    /// 未知落盘值回退默认（设置文件手改坏时不拒绝整个文件）。
    pub fn parse(text: &str) -> Self {
        match text {
            "grouped" => NumberFormatPref::Grouped,
            "fixed2" => NumberFormatPref::Fixed2,
            "scientific" => NumberFormatPref::Scientific,
            _ => NumberFormatPref::Raw,
        }
    }
}

/// NULL 的显示占位（渲染时替换 `NULL` 原文；判定仍按原文 `== "NULL"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NullPlaceholderPref {
    /// 显示 `NULL`（默认，与 SQL 语义一致）。
    #[default]
    Null,
    /// 显示 `∅`（紧凑）。
    EmptySet,
    /// 显示 `—`（表格里最安静的占位）。
    Dash,
}

impl NullPlaceholderPref {
    pub fn as_str(self) -> &'static str {
        match self {
            NullPlaceholderPref::Null => "null",
            NullPlaceholderPref::EmptySet => "emptyset",
            NullPlaceholderPref::Dash => "dash",
        }
    }

    /// 渲染用的实际占位文本。
    pub fn rendered(self) -> &'static str {
        match self {
            NullPlaceholderPref::Null => "NULL",
            NullPlaceholderPref::EmptySet => "∅",
            NullPlaceholderPref::Dash => "—",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "emptyset" => NullPlaceholderPref::EmptySet,
            "dash" => NullPlaceholderPref::Dash,
            _ => NullPlaceholderPref::Null,
        }
    }
}

/// 布尔值的显示映射（驱动给的 `true` / `false` 原文怎么呈现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoolDisplayPref {
    /// `true` / `false`（默认，与 SQL 语义一致）。
    #[default]
    TrueFalse,
    /// `是` / `否`（中文界面口径）。
    Cn,
    /// `1` / `0`（数字口径，与某些驱动返回一致）。
    Digits,
}

impl BoolDisplayPref {
    pub fn as_str(self) -> &'static str {
        match self {
            BoolDisplayPref::TrueFalse => "true_false",
            BoolDisplayPref::Cn => "cn",
            BoolDisplayPref::Digits => "digits",
        }
    }

    /// 渲染用映射（`true` → 元组第一位）。
    pub fn rendered(self) -> (&'static str, &'static str) {
        match self {
            BoolDisplayPref::TrueFalse => ("true", "false"),
            BoolDisplayPref::Cn => ("是", "否"),
            BoolDisplayPref::Digits => ("1", "0"),
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "cn" => BoolDisplayPref::Cn,
            "digits" => BoolDisplayPref::Digits,
            _ => BoolDisplayPref::TrueFalse,
        }
    }
}

/// 按族对齐覆盖（`inherit` = 跟随类型默认：数字右对齐、其余左对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlignPref {
    /// 跟随类型默认（数字右对齐 / 其余左对齐）。
    #[default]
    Inherit,
    Left,
    Center,
    Right,
}

impl AlignPref {
    pub fn as_str(self) -> &'static str {
        match self {
            AlignPref::Inherit => "inherit",
            AlignPref::Left => "left",
            AlignPref::Center => "center",
            AlignPref::Right => "right",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "left" => AlignPref::Left,
            "center" => AlignPref::Center,
            "right" => AlignPref::Right,
            _ => AlignPref::Inherit,
        }
    }
}

/// 按族前景色角色（预设色板：从主题语义 token 派生，零裸色值约束不变；
/// `inherit` = 不染色，跟随普通前景）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorRolePref {
    /// 不染色（默认）。
    #[default]
    Inherit,
    /// 信息蓝（`theme.colors.info`；数字族的推荐角色）。
    Info,
    /// 成功绿（`theme.colors.success`；布尔族的推荐角色）。
    Success,
    /// 警告橙（`theme.colors.warning`；BLOB 的推荐角色）。
    Warning,
    /// 危险红（`theme.colors.danger`）。
    Danger,
    /// 主品牌（`theme.colors.primary`）。
    Primary,
    /// 弱化灰（`theme.colors.muted_foreground`）。
    Muted,
}

impl ColorRolePref {
    pub fn as_str(self) -> &'static str {
        match self {
            ColorRolePref::Inherit => "inherit",
            ColorRolePref::Info => "info",
            ColorRolePref::Success => "success",
            ColorRolePref::Warning => "warning",
            ColorRolePref::Danger => "danger",
            ColorRolePref::Primary => "primary",
            ColorRolePref::Muted => "muted",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "info" => ColorRolePref::Info,
            "success" => ColorRolePref::Success,
            "warning" => ColorRolePref::Warning,
            "danger" => ColorRolePref::Danger,
            "primary" => ColorRolePref::Primary,
            "muted" => ColorRolePref::Muted,
            _ => ColorRolePref::Inherit,
        }
    }
}

/// 结果集「类型样式」偏好的聚合（落盘在 `settings.json` 的 `result_panel` 节下）。
///
/// 全局统一生效（用户拍板：先做全局，不做 per-结果集覆盖）；每个族两项
/// （对齐覆盖 + 前景色角色），加三项全局（数字格式 / NULL 占位 / 布尔映射）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TypeStylePrefs {
    /// 数字显示格式（默认原始值）。
    #[serde(default)]
    pub number_format: NumberFormatPref,
    /// NULL 显示占位（默认 `NULL` 原文）。
    #[serde(default)]
    pub null_placeholder: NullPlaceholderPref,
    /// 布尔显示映射（默认 `true` / `false`）。
    #[serde(default)]
    pub bool_display: BoolDisplayPref,
    /// 按族对齐覆盖（默认全部继承类型默认）。
    #[serde(default)]
    pub align_numeric: AlignPref,
    #[serde(default)]
    pub align_text: AlignPref,
    #[serde(default)]
    pub align_temporal: AlignPref,
    #[serde(default)]
    pub align_bool: AlignPref,
    #[serde(default)]
    pub align_blob: AlignPref,
    /// 按族前景色角色（默认全部不染色）。
    #[serde(default)]
    pub color_numeric: ColorRolePref,
    #[serde(default)]
    pub color_text: ColorRolePref,
    #[serde(default)]
    pub color_temporal: ColorRolePref,
    #[serde(default)]
    pub color_bool: ColorRolePref,
    #[serde(default)]
    pub color_blob: ColorRolePref,
}

impl TypeStylePrefs {
    /// 某族的对齐覆盖。
    pub fn align_for(&self, family: TypeFamily) -> AlignPref {
        match family {
            TypeFamily::Numeric => self.align_numeric,
            TypeFamily::Text => self.align_text,
            TypeFamily::Temporal => self.align_temporal,
            TypeFamily::Boolean => self.align_bool,
            TypeFamily::Blob => self.align_blob,
        }
    }

    /// 某族的前景色角色。
    pub fn color_for(&self, family: TypeFamily) -> ColorRolePref {
        match family {
            TypeFamily::Numeric => self.color_numeric,
            TypeFamily::Text => self.color_text,
            TypeFamily::Temporal => self.color_temporal,
            TypeFamily::Boolean => self.color_bool,
            TypeFamily::Blob => self.color_blob,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_unknown_values_fall_back_to_defaults() {
        assert_eq!(NumberFormatPref::parse("grouped"), NumberFormatPref::Grouped);
        assert_eq!(NumberFormatPref::parse("???"), NumberFormatPref::Raw);
        assert_eq!(NullPlaceholderPref::parse("dash"), NullPlaceholderPref::Dash);
        assert_eq!(NullPlaceholderPref::parse(""), NullPlaceholderPref::Null);
        assert_eq!(BoolDisplayPref::parse("cn"), BoolDisplayPref::Cn);
        assert_eq!(BoolDisplayPref::parse("x"), BoolDisplayPref::TrueFalse);
        assert_eq!(AlignPref::parse("right"), AlignPref::Right);
        assert_eq!(AlignPref::parse("0"), AlignPref::Inherit);
        assert_eq!(ColorRolePref::parse("info"), ColorRolePref::Info);
        assert_eq!(ColorRolePref::parse("red"), ColorRolePref::Inherit);
    }

    #[test]
    fn default_prefs_are_all_inherit_and_raw() {
        let prefs = TypeStylePrefs::default();
        assert_eq!(prefs.number_format, NumberFormatPref::Raw);
        assert_eq!(prefs.null_placeholder, NullPlaceholderPref::Null);
        assert_eq!(prefs.bool_display, BoolDisplayPref::TrueFalse);
        for family in TypeFamily::ALL {
            assert_eq!(prefs.align_for(family), AlignPref::Inherit);
            assert_eq!(prefs.color_for(family), ColorRolePref::Inherit);
        }
    }

    #[test]
    fn serde_roundtrip_keeps_explicit_values() {
        let prefs = TypeStylePrefs {
            number_format: NumberFormatPref::Grouped,
            align_numeric: AlignPref::Right,
            color_temporal: ColorRolePref::Info,
            ..Default::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: TypeStylePrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, prefs);
    }
}
