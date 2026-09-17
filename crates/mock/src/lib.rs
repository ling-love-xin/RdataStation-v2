//! RdataStation v2 测试数据生成 crate（mock，M7）
//!
//! 基于数据源元数据（schema_map 列映射）生成测试数据，仅落 DuckDB 分析引擎临时表，
//! 不回传各源数据库（M7 约束）。
//! - `engine`：MockEngine 执行管线
//! - `generator_catalog`：生成器目录（137 变体的分类 / 中文标签 / 参数规格，由脚本穷尽派生）
//! - `generators`：fake crate 驱动的各类数据生成器
//! - `history`：生成历史的后台读写（宿主只提供项目根）
//! - `models`：列定义/依赖/配置/导出模型
//! - `mock_view`：Mock 生成面板（Feature 自持视图，宿主能力由 workbench 注入）
//! - `persistence`：生成任务与模板存储
//! - `schema_map`：源库列类型 → mock 列类型映射（`parse_data_type` 为唯一类型串入口）
//! - `templates`：场景模板
//!
//! 依赖方向：mock → engine → shared；视图层另依赖 gpui-kit（Feature 自持视图，见 `mock_view`）。

pub mod engine;
pub mod error;
pub mod generator_catalog;
pub mod generators;
pub mod history;
pub mod mock_view;
pub mod models;
pub mod persistence;
pub mod schema_map;
pub mod templates;

pub use engine::MockEngine;
pub use engine::TempTableWriteMode;
pub use engine::sanitize_identifier;
pub use error::{MockError, MockResult};
pub use generator_catalog::{
    GeneratorCategory, GeneratorSpec, ParamField, ParamKind, all_specs, default_of, spec_by_name,
    spec_of, specs_in,
};
pub use models::{
    ColumnDataType, ColumnDef, ColumnDependency, ColumnMappingResponse, GeneratorConfig, Locale,
    MockConfig, MockExportFormat, MockExportInput, MockGenerateResult, MockPersistAssetInput,
    MockPersistAssetResult, MockSaveToScratchpadInput, MockScenarioResult, MockScenarioTableResult,
    ReferenceDomain, ScenarioTemplate, TemplateTable,
};
pub use persistence::{
    MockGenerationColumn, MockGenerationDetail, MockGenerationStore, MockGenerationTask,
    MockTemplateColumn, MockUserTemplate,
};
pub use schema_map::{ColumnMapper, parse_data_type};
