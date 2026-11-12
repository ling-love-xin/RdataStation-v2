//! RdataStation v2 资产库 / 分析存档 crate（analytics_resource，M6）
//!
//! 把"值得留存、需被引用、要能复现"的分析产物，从工作区转成**只读、有版本、带来源**的正式存档：
//!
//! - `model`：领域语义类型（存档种类 / 本体状态 / 归档凭证 / 业务断言 / 效力 / 履历事件）
//! - `store`：索引层（`project.db` 的登记读写：`resource` / `folder` / `tag` / `version`
//!   / `timeline` / `semantics` / `refs`）
//! - `payload`：本体层（`resources/` 受管文件、只读守卫、内容指纹、历史副本、暂存替换）
//! - `service`：归档服务（归档 / 取回 / 再归档编排 + 变更事件 + 履历埋点）
//! - `indexer`：索引修复（本体与登记表的三类差异：扫描只报告，修复要人工确认）
//! - `resource_view` / `detail_view` / `dialogs`：视图层（面板 / 详情 / 对话框；都不自己取数）
//!
//! 回收站：v1 的 `recycle.rs`（软删除表）已随 P0.8 废弃，统一走项目级
//! `engine::persistence::trash::ProjectTrash`（`origin = "resources"`）。
//!
//! 依赖方向：analytics_resource → engine（persistence::project_db）→ shared；
//! 上游 `workbench` 通过 `resource_host::ResourceHostBackend` 注入宿主能力，
//! **本 crate 不认识草稿箱 / 工作台的内部类型**。
//! 设计权威：`docs/architecture/analytics_resource/`。
//!
//! 公开面约定（2026-11-12 收口）：只有被其它 crate 真实引用的模块才是 `pub mod`；
//! 内部实现细节（本体 / 索引 / 服务 / 呈现 / 时间辅助）一律 `pub(crate)`，
//! 需要对外给的类型由下面的 `pub use` 单点导出。

pub(crate) mod analysis;
pub mod commands;
pub mod detail_view;
pub mod dialogs;
pub mod dnd;
pub mod filter;
pub(crate) mod helpers;
pub(crate) mod indexer;
pub mod jobs;
pub mod model;
pub(crate) mod payload;
pub(crate) mod present;
pub mod preview;
pub mod resource_view;
pub(crate) mod service;
pub(crate) mod store;
pub(crate) mod ui;

pub mod global;
pub(crate) mod view_model;
pub mod resource_host;

pub use indexer::{IndexIssue, IndexRepair, IndexScanReport};
pub use model::*;
pub use service::ArchiveService;
pub use store::rows::{
    AnalyticsFolder, AnalyticsResource, AnalyticsTag, CreateFolderRequest, CreateTagRequest,
    ResourceVersion,
};
pub use store::AnalyticsResourceStore;

// 持久层回归用例（v1 蓝本搬运，Round 11 时漏声明 mod——导致 560 行用例在 v2 从未被编译）。
#[cfg(test)]
mod tests;
