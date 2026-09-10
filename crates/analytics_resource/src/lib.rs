//! RdataStation v2 资源分析 crate（analytics_resource，M6）
//!
//! 管理分析资源（数据源连接、DuckDB 表等）：
//! - `models`：数据模型定义
//! - `helpers`：辅助函数（时间解析等）
//! - `resource`：资源 CRUD + 分页列表 + 克隆
//! - `folder`：文件夹 CRUD + 资源关联
//! - `tag`：标签 CRUD + 双向关联查询
//! - `recycle`：回收站操作（软删除/恢复/永久删除）
//! - `version`：版本历史管理
//!
//! 依赖方向：analytics_resource → engine（persistence::project_db）→ shared。

use std::sync::Arc;

use chrono::{DateTime, Utc};

use shared::error::CoreError;
use engine::persistence::project_db::{ProjectSqlitePool, SqlitePoolConnection};

pub mod helpers;
pub mod models;

pub mod folder;
pub mod recycle;
pub mod resource;
pub mod tag;
pub mod version;

pub use models::*;

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
}

