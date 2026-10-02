//! 描述模型：属性页 / 条目 / 支持状态 / 注册表契约。
//!
//! 动态渲染原则：渲染层只遍历 [`PropertyRegistry::pages_for`] 返回的页集合 +
//! [`crate::model::ObjectProperties`]，不按 `PropertyKind` 写分支。
//! 新增对象类型 / 新增属性 = 注册表加条目，渲染零改动。

use crate::model::PropertyKind;

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
    /// 例程序源码
    Source,
    /// 权限（当前不支持 → 置灰）
    Privileges,
    /// 依赖（当前不支持 → 置灰）
    Dependencies,
}

/// 支持状态：驱动 / 类型能力位决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportState {
    /// 可渲染实际内容
    Supported,
    /// 置灰渲染（内容区显示「暂不可用」占位）
    Unsupported,
}

/// 一个属性页的描述。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyPage {
    pub id: PageId,
    pub title: String,
    pub state: SupportState,
}

/// 属性网格中的一个条目（可扩展：editable 预留编辑能力）。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyEntry {
    pub label: String,
    pub value: String,
    pub state: SupportState,
    pub editable: bool,
}

/// 对象类型 → 属性页集合的注册表契约（数据生产方实现）。
pub trait PropertyRegistry {
    /// 返回某类型对象的页集合（顺序即 tab 顺序）。
    fn pages_for(&self, kind: PropertyKind) -> Vec<PropertyPage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认页为 General（tab 化后首次激活页）。
    #[test]
    fn page_id_defaults_to_general() {
        assert_eq!(PageId::default(), PageId::General);
    }

    /// 注册表契约可被 mock 实现消费（渲染方只依赖 trait）。
    #[test]
    fn registry_contract_consumable() {
        struct MockRegistry;
        impl PropertyRegistry for MockRegistry {
            fn pages_for(&self, _kind: PropertyKind) -> Vec<PropertyPage> {
                vec![
                    PropertyPage {
                        id: PageId::General,
                        title: "通用".to_string(),
                        state: SupportState::Supported,
                    },
                    PropertyPage {
                        id: PageId::Privileges,
                        title: "权限".to_string(),
                        state: SupportState::Unsupported,
                    },
                ]
            }
        }

        let pages = MockRegistry.pages_for(PropertyKind::Table);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].id, PageId::General);
        assert_eq!(pages[1].state, SupportState::Unsupported);
    }
}
