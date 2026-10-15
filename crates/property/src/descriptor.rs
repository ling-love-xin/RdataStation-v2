//! 描述模型：属性页 id（页 → 内容映射键，也是 tab 标识）。
//!
//! 动态渲染原则：渲染层只遍历 [`crate::model::ObjectProperties`] 的
//! `sections` / `properties`，不按 `PropertyKind` 写分支。
//! 新增对象类型 / 新增属性 = provider 加条目，渲染零改动。
//! 页集合与置灰状态由 provider 的 `categories()` 直接产出
//! （见 `crate::provider`），本模块不再维护独立的描述注册表。

/// 属性页 id（页 → 内容映射键，也是 tab 标识）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PageId {
    /// 属性网格
    #[default]
    General,
    /// 列
    Columns,
    /// 索引
    Indexes,
    /// 约束
    Constraints,
    /// DDL（源版 / 合成）
    Ddl,
    /// 表数据预览（分页抓取，见 `crate::model::TableDataPage`）
    Data,
    /// 例程序源码
    Source,
    /// 权限（当前不支持 → 置灰）
    Privileges,
    /// 依赖（当前不支持 → 置灰）
    Dependencies,
    /// 规则 / 策略（PG 表类别，当前不支持 → 置灰）
    Routines,
    /// 分区（PG 表类别，当前不支持 → 置灰）
    Partitions,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认页为 General（tab 化后首次激活页）。
    #[test]
    fn page_id_defaults_to_general() {
        assert_eq!(PageId::default(), PageId::General);
    }
}
