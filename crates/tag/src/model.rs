//! 标签实体与命名工具（rds-tag 的模型层）。
//!
//! 统一模型（规格 §3.1）：`color` / `icon` / `scope` 全模块共用；
//! `key:value` 是标签名约定（§6），不进模型字段。

/// 全局作用域：导航连接标签与分析资源标签的定义（系统 `global.db` 的 `tags` 表）。
pub const SCOPE_GLOBAL: &str = "global";
/// 项目作用域：草稿箱标签的定义（项目库的 `tags` 表）。
pub const SCOPE_PROJECT: &str = "project";

/// 标签实体（统一模型，三模块共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// 跨域唯一 id（前缀 `tag_`，见规格 §3.1）。
    pub id: String,
    /// 规范化后的标签名（`key:value` 约定见 [`parse_key_value`]）。
    pub name: String,
    /// 语义色名（映射主题 token），全模块共用；`None` = 默认档位。
    pub color: Option<String>,
    /// 图标名（IconName），全模块共用；`None` = 无图标。
    pub icon: Option<String>,
    /// 派生作用域：由标签定义所在存储域决定（[`SCOPE_GLOBAL`] / [`SCOPE_PROJECT`]），不可编辑。
    pub scope: String,
    /// 创建时间（RFC3339）。
    pub created_at: String,
    /// 软删时间（`None` = 活跃）。
    pub deleted_at: Option<String>,
}

/// 规范化标签名：去首尾空白；空 / 纯空白返回 `None`。
pub fn normalize_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// 解析 `key:value` 命名约定（规格 §6：存储仍是字符串，解析层识别做显式维度）。
///
/// - 命中返回 `(key, value)`，两者均非空、均不含 `:`；
/// - 不符合约定（无 `:`、`key:` / `:value`、含多个 `:`）返回 `None`，按普通标签处理。
pub fn parse_key_value(name: &str) -> Option<(&str, &str)> {
    let (key, value) = name.split_once(':')?;
    if key.is_empty() || value.is_empty() || key.contains(':') || value.contains(':') {
        return None;
    }
    Some((key, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_rejects_blank() {
        assert_eq!(normalize_name("  prod  ").as_deref(), Some("prod"));
        assert_eq!(normalize_name("prod"), Some("prod".to_string()));
        assert_eq!(normalize_name(""), None);
        assert_eq!(normalize_name("   "), None);
        assert_eq!(normalize_name("\t\n"), None);
    }

    #[test]
    fn key_value_parses_only_single_colon_pairs() {
        // 命中
        assert_eq!(parse_key_value("env:prod"), Some(("env", "prod")));
        assert_eq!(parse_key_value("driver:pg"), Some(("driver", "pg")));
        // 未命中：无冒号 / 空 key / 空 value / 多冒号
        assert_eq!(parse_key_value("prod"), None);
        assert_eq!(parse_key_value(":prod"), None);
        assert_eq!(parse_key_value("env:"), None);
        assert_eq!(parse_key_value("a:b:c"), None);
    }
}
