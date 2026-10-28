//! 结果集面板设置 model（struct + 默认值 + 默认函数）。
use super::default_true;
use serde::{Deserialize, Serialize};

/// 带类型导出 / IronCalc 快照）在 B 批接通消费，渲染与文本导出不读本偏好。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultPanel {
    /// 事实档载体：`auto`（默认，按双份估算水位分流）/ `memory`（强制内存档）/
    /// `table`（强制表档；B 批接通后直灌常驻表，A 批未接通时按降级档处理）。
    #[serde(default = "default_result_fact_storage")]
    pub fact_storage: FactStoragePref,
    /// 列头是否**直接显示**字段注释（默认 `false`：注释只在 hover tooltip 里）。
    /// 开启后列名下方第二行小字显示注释（截短 + ellipsis；注释为空不占位）。
    /// 面板工具栏另有 B1 临时开关，覆盖此全局偏好。
    #[serde(default)]
    pub show_column_comments: bool,
    /// 【DBeaver 对齐】列头是否显示类型徽标（123/Aa/◷/✓/01 浅底符号）。
    #[serde(default = "default_true")]
    pub show_type_badge: bool,
    /// 【DBeaver 对齐】键盘导航是否循环选择（到末尾后循环回开头）。
    #[serde(default = "default_true")]
    pub loop_selection: bool,
    /// 【DBeaver 对齐】是否启用 Ctrl+点击多选（修饰键多选）。
    #[serde(default = "default_true")]
    pub select_on_modifier_click: bool,
    /// 【DBeaver 对齐】列宽是否跨会话持久化（关闭则只在当前会话内记忆）。
    #[serde(default = "default_true")]
    pub persist_column_widths: bool,
    /// 【类型样式】数字显示格式（默认原始值；只影响渲染显示，复制 / 导出仍用原始值）。
    #[serde(default)]
    pub number_format: workbench_shell::result_style::NumberFormatPref,
    /// 【类型样式】NULL 显示占位（默认 `NULL` 原文；判定仍按原文）。
    #[serde(default)]
    pub null_placeholder: workbench_shell::result_style::NullPlaceholderPref,
    /// 【类型样式】布尔显示映射（默认 `true` / `false`）。
    #[serde(default)]
    pub bool_display: workbench_shell::result_style::BoolDisplayPref,
    /// 【类型样式】按族对齐覆盖（默认全部跟随类型默认：数字右对齐 / 其余左对齐）。
    #[serde(default)]
    pub align_numeric: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_text: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_temporal: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_bool: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_blob: workbench_shell::result_style::AlignPref,
    /// 【类型样式】按族前景色角色（默认全部不染色；预设色板从主题语义 token 派生）。
    #[serde(default)]
    pub color_numeric: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_text: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_temporal: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_bool: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_blob: workbench_shell::result_style::ColorRolePref,
}

impl Default for ResultPanel {
    fn default() -> Self {
        Self {
            fact_storage: default_result_fact_storage(),
            show_column_comments: false,
            show_type_badge: true,
            loop_selection: true,
            select_on_modifier_click: true,
            persist_column_widths: true,
            // 类型样式默认全继承 / 原始值（与渲染现状逐项一致）
            number_format: workbench_shell::result_style::NumberFormatPref::Raw,
            null_placeholder: workbench_shell::result_style::NullPlaceholderPref::Null,
            bool_display: workbench_shell::result_style::BoolDisplayPref::TrueFalse,
            align_numeric: workbench_shell::result_style::AlignPref::Inherit,
            align_text: workbench_shell::result_style::AlignPref::Inherit,
            align_temporal: workbench_shell::result_style::AlignPref::Inherit,
            align_bool: workbench_shell::result_style::AlignPref::Inherit,
            align_blob: workbench_shell::result_style::AlignPref::Inherit,
            color_numeric: workbench_shell::result_style::ColorRolePref::Inherit,
            color_text: workbench_shell::result_style::ColorRolePref::Inherit,
            color_temporal: workbench_shell::result_style::ColorRolePref::Inherit,
            color_bool: workbench_shell::result_style::ColorRolePref::Inherit,
            color_blob: workbench_shell::result_style::ColorRolePref::Inherit,
        }
    }
}

impl ResultPanel {
    /// 【类型样式】导出为渲染消费用的聚合偏好。
    ///
    /// 消费方（editor host 构造同步 / workbench 设置变更推送）都从这里取，
    /// 不必各自把 13 项字段再拼一遍；偏好与模型字段一一对应。
    pub fn type_style(&self) -> workbench_shell::result_style::TypeStylePrefs {
        workbench_shell::result_style::TypeStylePrefs {
            number_format: self.number_format,
            null_placeholder: self.null_placeholder,
            bool_display: self.bool_display,
            align_numeric: self.align_numeric,
            align_text: self.align_text,
            align_temporal: self.align_temporal,
            align_bool: self.align_bool,
            align_blob: self.align_blob,
            color_numeric: self.color_numeric,
            color_text: self.color_text,
            color_temporal: self.color_temporal,
            color_bool: self.color_bool,
            color_blob: self.color_blob,
        }
    }
}

/// 事实档载体偏好（落盘 `auto` / `memory` / `table`，serde snake_case）。
///
/// 决策层枚举是 `shared::StoragePref`（纯函数入参，不引 serde）；这里独立定义持久化形态，
/// 经 [`FactStoragePref::to_shared`] 映射——设置层不把 serde 序列化约束拖进事实层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FactStoragePref {
    /// 自动：按双份估算水位分流（8 MiB 上界 / 3 MiB 滞回线）。
    #[default]
    Auto,
    /// 强制内存档：即使超水位也驻 Rust 堆。
    Memory,
    /// 强制表档：即使在水位内也走常驻表（B 批接通）。
    Table,
}

impl FactStoragePref {
    /// 映射到决策层 [`shared::StoragePref`]。
    pub fn to_shared(self) -> ::shared::StoragePref {
        match self {
            Self::Auto => ::shared::StoragePref::Auto,
            Self::Memory => ::shared::StoragePref::Memory,
            Self::Table => ::shared::StoragePref::Table,
        }
    }

    /// 全部取值（登记表枚举项、测试穷尽遍历用）。
    pub const ALL: [FactStoragePref; 3] = [
        FactStoragePref::Auto,
        FactStoragePref::Memory,
        FactStoragePref::Table,
    ];

    /// 落盘值文本（登记表枚举落盘值 / `value_by_key` 回读用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Memory => "memory",
            Self::Table => "table",
        }
    }

    /// 解析落盘值；未知值返回 `None`，由写入路径拒绝而不是猜。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == s.trim())
    }
}

fn default_result_fact_storage() -> FactStoragePref {
    FactStoragePref::Auto
}
