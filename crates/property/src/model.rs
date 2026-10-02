//! 属性面板领域模型与数据容器。

use crate::descriptor::PageId;
use serde::{Deserialize, Serialize};

/// 属性面板可展示的对象类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyKind {
    Connection,
    Catalog,
    Schema,
    Table,
    View,
    Column,
    /// 存储过程 / 函数
    Routine,
    /// 序列
    Sequence,
    /// 触发器
    Trigger,
}

/// 属性网格中的一行（label / value）。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyRow {
    pub label: String,
    pub value: String,
}

/// 子实体表格（列 / 索引 / 约束 / DDL / 源码）。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 子实体分区（带页 id，供描述模型映射「页 → 内容」）。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertySection {
    pub id: PageId,
    pub label: String,
    pub table: PropertyTable,
}

/// 一个对象的完整属性（供属性面板渲染）。
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectProperties {
    /// 标题（对象名）
    pub title: String,
    /// 对象类型文案（BASE TABLE / VIEW / COLUMN …）
    pub object_type: String,
    /// 来源标签（项目 (P) / 全局 (G) / 共享 (GP)）
    pub source_label: String,
    /// 上部属性网格
    pub properties: Vec<PropertyRow>,
    /// 子实体分区
    pub sections: Vec<PropertySection>,
}

/// 属性面板状态（懒加载一次，按请求 key 失效重载）。
///
/// 由消费方（编辑区右侧面板）持有，本 crate 只管类型——原先住在导航视图里，
/// 但它是**属性面板**的状态，与导航树无关。
#[derive(Default)]
pub struct PropertyState {
    pub loaded_for: Option<String>,
    pub props: Option<ObjectProperties>,
    pub error: Option<String>,
    /// 是否正在后台加载（渲染「加载中…」）。
    pub loading: bool,
    /// 当前激活页（tab 化，默认 General）。
    pub active_page: PageId,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kind 序列化往返（PropertyRef 参与持久化 / Quick Open 传递）。
    #[test]
    fn property_kind_serde_roundtrip() {
        for kind in [
            PropertyKind::Connection,
            PropertyKind::Catalog,
            PropertyKind::Schema,
            PropertyKind::Table,
            PropertyKind::View,
            PropertyKind::Column,
            PropertyKind::Routine,
            PropertyKind::Sequence,
            PropertyKind::Trigger,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize");
            let back: PropertyKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, back, "kind 序列化往返");
        }
    }

    /// 状态默认值：无目标、非加载、激活页为 General。
    #[test]
    fn property_state_defaults() {
        let st = PropertyState::default();
        assert!(st.loaded_for.is_none());
        assert!(st.props.is_none());
        assert!(st.error.is_none());
        assert!(!st.loading);
        assert_eq!(st.active_page, PageId::General);
    }
}
