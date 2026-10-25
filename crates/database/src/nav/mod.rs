//! 导航可插拔底座 · L1 族档案层（静态元模型）。
//!
//! 职责：声明「这一族数据库长什么样」——层级形态、对象类型全集、类型与文件夹 /
//! 属性类别 / 展示变体 / 图标的对应关系。替代此前散落在 `navigator_service` /
//! `nav_view` / `model` 七处身份映射 `match`（方案稿 §2.2 C7/C8/C9/C11/C12/C13/C19）。
//!
//! 分层边界：
//! - 本模块只承载**静态档案与族无关身份字典**，无样式代码、无运行时 I/O；
//! - 运行时能力门控（该连接是否支持某类别内省）归 L2（B2，`capability` 静态规则 +
//!   `metadata_service` 桥接 `MetadataBrowser` 能力位）；方言 SQL 归 L3（B3）；
//!   动作注册归 L4（B4）。
//! - 新增数据库族：在本模块补一份 [`DbFamilyProfile`] 并注册一次，消费方零改动。

pub mod capability;
pub mod family_profile;

pub use capability::{NavOp, OpSet, family_ops, nav_node_object_kind};
pub use family_profile::{
    DbFamilyProfile, FamilyRegistry, FolderProfile, NodeShape, TypeProfile, registry,
};
