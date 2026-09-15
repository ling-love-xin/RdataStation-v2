//! 洞察规则的**作用域模型**与来源描述。
//!
//! 规则分三层，后加载者**整体覆盖**前者的同名规则（`meta.id` 相同即视为同一条，
//! 不做字段级合并、不加命名空间前缀）：
//!
//! | 作用域 | 位置 | 可见范围 | 可写 |
//! | --- | --- | --- | --- |
//! | [`RuleScope::Builtin`] | 应用内嵌（`include_dir!`，随二进制分发，18 条） | 所有项目 | ❌ |
//! | [`RuleScope::Global`] | `{系统目录}/insight-rules/` | 所有项目 | ✅ |
//! | [`RuleScope::Project`] | `{项目}/.RSmeta/insight-rules/` | 当前项目 | ✅ |
//!
//! 设计取舍：**正文以文件为唯一真相源**（可 diff、可进 git、可手工编辑），
//! 库表只承载索引与启停状态（见 `insight_rule_index`，同步器在 `crate::service::indexer`）。
//!
//! `Builtin` 层不可删，但可经索引表的抑制记录**禁用**（`enabled = 0`）——
//! 这样用户能关掉一条不想要的内置规则，而不必伪造一个同名覆盖文件。

use serde::{Deserialize, Serialize};

/// 规则作用域；枚举顺序即覆盖优先级（由低到高）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
pub enum RuleScope {
    /// 应用内嵌规则（`include_dir!`），所有项目可见、只读。
    Builtin,
    /// 用户全局规则，所有项目可见、可写。
    Global,
    /// 项目规则，仅当前项目可见、可写（随项目走，可进 git）。
    Project,
}

impl RuleScope {
    /// 加载顺序：先加载者优先级低，被后加载的同名规则覆盖。
    pub const LOAD_ORDER: [RuleScope; 3] = [
        RuleScope::Builtin,
        RuleScope::Global,
        RuleScope::Project,
    ];

    /// 界面展示用中文名。
    pub fn label(self) -> &'static str {
        match self {
            RuleScope::Builtin => "内置",
            RuleScope::Global => "全局",
            RuleScope::Project => "项目",
        }
    }

    /// 落库取值（与 `insight_rule_index.scope` 的 CHECK 约束一致）。
    ///
    /// 与 [`Self::label`] 分开：存储用稳定英文标识，展示用中文，
    /// 不拿展示文案当键（改文案不应动数据）。
    pub fn as_str(self) -> &'static str {
        match self {
            RuleScope::Builtin => "builtin",
            RuleScope::Global => "global",
            RuleScope::Project => "project",
        }
    }

    /// 从落库取值解析。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "builtin" => Some(RuleScope::Builtin),
            "global" => Some(RuleScope::Global),
            "project" => Some(RuleScope::Project),
            _ => None,
        }
    }

    /// 该作用域是否可写（内置层不可写）。
    pub fn writable(self) -> bool {
        !matches!(self, RuleScope::Builtin)
    }
}

/// 某条规则的实际来源（用于界面展示「这条规则从哪来」）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RuleSource {
    /// 来源作用域。
    pub scope: RuleScope,
    /// 规则文件路径（`Builtin` 为内嵌虚拟路径，仅用于展示）。
    pub path: String,
}

impl RuleSource {
    pub fn new(scope: RuleScope, path: impl Into<String>) -> Self {
        Self {
            scope,
            path: path.into(),
        }
    }
}

/// 加载失败的记录：单条规则解析失败**不连坐**，其余规则照常可用。
///
/// 失败必须**有出口**：规则格式是带 `deny_unknown_fields` 的严格 schema，
/// 解析错误只在日志里 warn 的话，用户在界面上只会看到「规则莫名其妙不见了」。
/// 本结构即该出口——由索引同步器落库（`load_status = 'invalid'` + `load_error`），
/// 界面据此逐条展示错误原文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RuleLoadFailure {
    /// 来源作用域。
    pub scope: RuleScope,
    /// 规则文件路径。
    pub path: String,
    /// 解析错误原文（TOML 报错含行列信息，直接展示给用户）。
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_order_is_low_to_high_priority() {
        // 顺序即优先级：内置最低、项目最高；加载时后者覆盖前者。
        assert_eq!(
            RuleScope::LOAD_ORDER,
            [RuleScope::Builtin, RuleScope::Global, RuleScope::Project]
        );
    }

    #[test]
    fn test_ordering_matches_load_order() {
        assert!(RuleScope::Builtin < RuleScope::Global);
        assert!(RuleScope::Global < RuleScope::Project);
    }

    #[test]
    fn test_builtin_is_read_only() {
        assert!(!RuleScope::Builtin.writable());
        assert!(RuleScope::Global.writable());
        assert!(RuleScope::Project.writable());
    }

    #[test]
    fn test_labels() {
        assert_eq!(RuleScope::Builtin.label(), "内置");
        assert_eq!(RuleScope::Global.label(), "全局");
        assert_eq!(RuleScope::Project.label(), "项目");
    }

    /// 落库取值必须与 `insight_rule_index.scope` 的 CHECK 约束逐字一致，
    /// 否则写入会被 SQLite 拒绝（且只有运行时才发现）。
    #[test]
    fn test_storage_values_match_schema_check() {
        assert_eq!(RuleScope::Builtin.as_str(), "builtin");
        assert_eq!(RuleScope::Global.as_str(), "global");
        assert_eq!(RuleScope::Project.as_str(), "project");
        for scope in RuleScope::LOAD_ORDER {
            assert_eq!(RuleScope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(RuleScope::parse("nonsense"), None);
    }
}
