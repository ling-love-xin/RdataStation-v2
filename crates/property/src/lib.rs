//! rds-property — 属性面板模块（独立 crate，自 M4 抽取，2026-10-03）。
//!
//! 描述驱动：对象类型（[`PropertyKind`]）→ 属性页集合（[`PropertyRegistry`]）→ 动态渲染。
//! 数据生产者（`database::property_panel::load_properties`）与渲染方（`workbench::EditorPanel`）
//! 都只消费本 crate 的模型与契约，互不依赖。
//!
//! 设计规格：`docs/architecture/property/property-module-spec.md`。

pub mod descriptor;
pub mod model;

pub use descriptor::{PageId, PropertyEntry, PropertyPage, PropertyRegistry, SupportState};
pub use model::{
    ObjectProperties, PropertyKind, PropertyRow, PropertySection, PropertyState, PropertyTable,
    PropertyValueKind, TableDataColumn, TableDataPage,
};
