//! RdataStation v2 测试数据生成 crate（mock，M7）
//!
//! 基于数据源元数据（schema_map 列映射）生成测试数据，仅落 DuckDB 分析引擎临时表，
//! 不回传各源数据库（M7 约束）。
//! - `engine`：MockEngine 执行管线
//! - `generators`：fake crate 驱动的各类数据生成器
//! - `models`：列定义/依赖/配置/导出模型
//! - `persistence`：生成任务与模板存储
//! - `schema_map`：源库列类型 → mock 列类型映射
//! - `templates`：场景模板
//!
//! 依赖方向：mock → engine → shared。

pub mod engine;
pub mod error;
pub mod generators;
pub mod models;
pub mod persistence;
pub mod schema_map;
pub mod templates;

pub use engine::MockEngine;
pub use error::{MockError, MockResult};
pub use models::{
    ColumnDataType, ColumnDef, ColumnDependency, ColumnMappingResponse, DependencyConfig,
    DependencyType, GeneratorConfig, ImportSchemaInput, Locale, MockConfig, MockExportFormat,
    MockExportInput, MockGenerateResult, MockPersistAssetInput, MockPersistAssetResult,
    MockSaveToScratchpadInput, MockScenarioResult, MockScenarioTableResult, ScenarioTemplate,
    TemplateTable,
};
pub use persistence::{
    MockGenerationColumn, MockGenerationDetail, MockGenerationStore, MockGenerationTask,
    MockTemplateColumn, MockUserTemplate,
};
pub use schema_map::ColumnMapper;

