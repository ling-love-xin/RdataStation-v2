//! 洞察模块的**视图模型**（M8 Phase 1 落地）。
//!
//! 规划内容（见 `docs/architecture/insight/insight-prototype-design.md` §3、§8）：
//! - `InsightTarget`：当前分析目标（列 / 表 / 多列 / Schema），决定面板渲染哪一路内容
//! - `PanelTab`：五 Tab（列 / 表 / 多列 / 结构 / 历史）
//! - `InsightPanelState`：面板状态（目标、Tab、折叠态、加载与错误态）
//!
//! 边界：本模块只放**视图模型**；算法 DTO（`ColumnInsightFull` / `QualityScore` /
//! `TableQuality` / `SchemaInsightReport` 等）归 `model::types`，两者不混放。
//!
//! 本文件当前为空占位（Phase 1 随视图一并落地）。
