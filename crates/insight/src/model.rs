//! 洞察模块的**领域模型**与视图模型。
//!
//! | 子模块 | 内容 |
//! | --- | --- |
//! | [`types`] | 领域类型：列画像（`ColumnInsightFull` / `ColumnStats` / 各类型统计 / `DistributionBin`）、表画像（`TableProfile` / `TableColumnMeta`）、质量（`QualityScore` / `QualityDimension` / `TableQuality` / `ColumnQualityEntry`） |
//!
//! 视图模型（`InsightTarget` / `PanelTab` / `InsightPanelState`）规划在 Phase 1 落地，
//! 与领域类型分开放：领域类型是分析链路的输入输出契约，视图模型是界面的状态。
//!
//! 归属变更（M8 Phase 0 / 0.2）：`types` 自 `engine/src/persistence/insight_types.rs` 迁入。
//! 它们是**洞察领域词汇**，不属数据层；engine 只提供连接与迁移，不再认识「洞察」。

pub mod types;
