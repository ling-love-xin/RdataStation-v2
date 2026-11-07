//! `promoted_from` 一字段两义：**升级 / 分发**写全局实例 id，**从草稿归档**写来源草稿相对路径。
//!
//! 两条写入面（`resource_host/archive_actions.rs` 的草稿归档分支、
//! `global/register.rs` 的项目登记分支）本来各写各的，读取面
//! （`present/detail.rs` → 详情「来源」区）却一律当「来源草稿」渲染，
//! 于是升级过的行会在详情里显示「来源草稿：global:arg_3f2a…」——把技术 id
//! 当成一个路径标签卖给用户。
//!
//! 这里按**已知前缀**分支解释（不改 schema：加一列要新迁移 + 改列名清单 + 改断言，
//! 而现在两个写入面都只在本 crate 内、前缀就够分辨）。真要分字段时，
//! `global:` 前缀仍是可回填的迁移线索。

/// 写全局实例 id 时的标记（与 `global/register.rs` 两处 `format!("global:{id}")` 同一约定）。
pub const GLOBAL_PREFIX: &str = "global:";

/// `promoted_from` 的两种真实含义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotedFrom {
    /// 从草稿归档：`scratchpad/<rel>`（真·来源草稿的相对路径）。
    Draft(String),
    /// 升级到全局 / 分发落项目时记下的全局实例 id（`global:` 之后那一段）。
    Global(String),
}

impl PromotedFrom {
    /// 按前缀解释一个原始值（空串 / 只有前缀的脏值都归草稿分支，不吞掉内容）。
    pub fn parse(raw: &str) -> Self {
        match raw.strip_prefix(GLOBAL_PREFIX) {
            Some(id) if !id.is_empty() => Self::Global(id.to_string()),
            _ => Self::Draft(raw.to_string()),
        }
    }

    /// 详情「来源」区的行标签：同一个字段，两种来源要两个说法。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Draft(_) => "来源草稿",
            Self::Global(_) => "来源全局实例",
        }
    }

    /// 展示值：全局分支去掉技术前缀只给 id（前缀是库里的约定，不是给人看的）；
    /// 草稿分支原样给相对路径。
    pub fn value(&self) -> &str {
        match self {
            Self::Draft(path) => path,
            Self::Global(id) => id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_prefix_means_an_instance_id() {
        let parsed = PromotedFrom::parse("global:arg_3f2a9c");
        assert_eq!(parsed, PromotedFrom::Global("arg_3f2a9c".to_string()));
        assert_eq!(parsed.label(), "来源全局实例");
        assert_eq!(parsed.value(), "arg_3f2a9c", "前缀不该露给用户");
    }

    #[test]
    fn anything_else_is_a_draft_path() {
        let parsed = PromotedFrom::parse("scratchpad/dau.sql");
        assert_eq!(parsed, PromotedFrom::Draft("scratchpad/dau.sql".to_string()));
        assert_eq!(parsed.label(), "来源草稿");
        assert_eq!(parsed.value(), "scratchpad/dau.sql");
    }

    #[test]
    fn empty_and_bare_prefix_do_not_swallow_content() {
        assert_eq!(PromotedFrom::parse(""), PromotedFrom::Draft(String::new()));
        assert_eq!(
            PromotedFrom::parse("global:"),
            PromotedFrom::Draft("global:".to_string()),
            "只有前缀没有 id 是脏值：当草稿原样显示，不装成全局实例"
        );
    }
}
