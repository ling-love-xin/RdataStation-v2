//! RdataStation v2 资产库 / 分析存档 crate（analytics_resource，M6）
//!
//! 把"值得留存、需被引用、要能复现"的分析产物，从工作区转成**只读、有版本、带来源**的正式存档：
//!
//! - `model`：领域语义类型（存档种类 / 复现强度 / 本体状态 / 归档凭证）
//! - `payload`：本体层（`resources/` 受管文件、只读守卫、内容指纹、历史副本）
//! - `service`：归档服务（归档 / 取回 / 再归档编排 + 变更事件）
//! - `indexer`：索引修复（本体与登记表的三类差异：扫描只报告，修复要人工确认）
//! - `models`：持久层行模型（v1 搬运，逐步并入 `model`）
//! - `resource` / `folder` / `tag` / `version`：索引层（`project.db`）
//! - `resource_view` / `detail_view` / `dialogs`：视图层（面板 / 详情 / 对话框；都不自己取数）
//!
//! 回收站：v1 的 `recycle.rs`（软删除表）已随 P0.8 废弃，统一走项目级
//! `engine::persistence::trash::ProjectTrash`（`origin = "resources"`）。
//!
//! 依赖方向：analytics_resource → engine（persistence::project_db）→ shared。
//! 设计权威：`docs/architecture/analytics_resource/`。

use std::sync::Arc;

use chrono::{DateTime, Utc};

use engine::persistence::project_db::{ProjectSqlitePool, SqlitePoolConnection};
use shared::error::CoreError;

pub mod commands;
pub mod detail_view;
pub mod dialogs;
pub mod dnd;
pub mod filter;
pub mod helpers;
pub mod indexer;
pub mod model;
pub mod models;
pub mod payload;
pub mod present;
pub mod resource_view;
pub mod service;
pub mod ui;

pub mod folder;
pub mod resource;
pub mod tag;
pub mod version;

pub use indexer::{IndexIssue, IndexRepair, IndexScanReport};
pub use model::*;
pub use models::*;
pub use service::ArchiveService;

/// 分析资源存储（SQLite 持久化层）
#[derive(Clone)]
pub struct AnalyticsResourceStore {
    pool: Arc<ProjectSqlitePool>,
}

impl AnalyticsResourceStore {
    pub fn new(pool: Arc<ProjectSqlitePool>) -> Self {
        Self { pool }
    }

    async fn get_conn(&self) -> Result<SqlitePoolConnection, CoreError> {
        self.pool.acquire().await
    }

    pub(crate) fn parse_datetime(s: String) -> Result<DateTime<Utc>, CoreError> {
        helpers::parse_datetime(s)
    }

    pub(crate) fn parse_datetime_sqlite(s: String) -> Result<DateTime<Utc>, rusqlite::Error> {
        helpers::parse_datetime_sqlite(s)
    }

    /// 连接池句柄：仅供 crate 内**测试**使用（故障注入与迁移辅助），故不进生产接口。
    #[cfg(test)]
    pub(crate) fn pool(&self) -> &Arc<ProjectSqlitePool> {
        &self.pool
    }
}

// 持久层回归用例（v1 蓝本搬运，Round 11 时漏声明 mod——导致 560 行用例在 v2 从未被编译）。
#[cfg(test)]
mod tests;
