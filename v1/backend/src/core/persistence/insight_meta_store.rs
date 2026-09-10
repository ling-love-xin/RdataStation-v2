//! 洞察元数据存储（SQLite 侧）
//!
//! 管理洞察快照的执行记录和版本追踪。
//! 数据存储在项目的 `project.db` 中。
//!
//! 与 DuckDB 侧的 `insight_store.rs` 配合使用：
//! SQLite 存储轻量级元数据（执行时间、行数等），
//! DuckDB 存储重量级 JSON 数据（完整的洞察快照）。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::core::error::{CoreError, StorageError};
use crate::core::persistence::project_db::ProjectSqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InsightSnapshotMeta {
    pub id: String,
    pub entity_type: String,
    pub entity_name: String,
    pub entity_source: Option<String>,
    pub snapshot_id: String,
    pub row_count: Option<i32>,
    pub elapsed_ms: Option<i32>,
    pub version_id: String,
    pub parent_version_id: Option<String>,
    pub checksum: String,
    pub created_at: Option<String>,
}

pub struct InsightMetaStore {
    sqlite_pool: Arc<ProjectSqlitePool>,
}

impl InsightMetaStore {
    pub fn new(sqlite_pool: Arc<ProjectSqlitePool>) -> Self {
        Self { sqlite_pool }
    }

    /// 保存洞察快照元数据
    #[allow(clippy::too_many_arguments)]
    pub async fn save_meta(
        &self,
        entity_type: &str,
        entity_name: &str,
        entity_source: Option<&str>,
        snapshot_id: &str,
        row_count: Option<i32>,
        elapsed_ms: Option<i32>,
        version_id: &str,
        parent_version_id: Option<&str>,
        checksum: &str,
    ) -> Result<String, CoreError> {
        let id = Uuid::new_v4().to_string();
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?.execute(
            "INSERT INTO insight_snapshots (id, entity_type, entity_name, entity_source, snapshot_id, row_count, elapsed_ms, version_id, parent_version_id, checksum)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                &id,
                entity_type,
                entity_name,
                &entity_source.map(|s| s.to_string()),
                snapshot_id,
                row_count,
                elapsed_ms,
                version_id,
                &parent_version_id.map(|s| s.to_string()),
                checksum,
            ],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "save_insight_meta".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(id)
    }

    /// 获取指定实体的最新快照元数据
    pub async fn get_latest_meta(
        &self,
        entity_type: &str,
        entity_name: &str,
    ) -> Result<Option<InsightSnapshotMeta>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let result = conn.inner()?.query_row(
            "SELECT id, entity_type, entity_name, entity_source, snapshot_id, row_count, elapsed_ms, version_id, parent_version_id, checksum, created_at
             FROM insight_snapshots WHERE entity_type = ?1 AND entity_name = ?2 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![entity_type, entity_name],
            |row| {
                Ok(InsightSnapshotMeta {
                    id: row.get(0)?,
                    entity_type: row.get(1)?,
                    entity_name: row.get(2)?,
                    entity_source: row.get(3)?,
                    snapshot_id: row.get(4)?,
                    row_count: row.get(5)?,
                    elapsed_ms: row.get(6)?,
                    version_id: row.get(7)?,
                    parent_version_id: row.get(8)?,
                    checksum: row.get(9)?,
                    created_at: row.get(10)?,
                })
            },
        );

        match result {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_latest_insight_meta".to_string(),
                reason: e.to_string(),
            })),
        }
    }

    /// 获取指定实体的所有历史版本元数据（从新到旧）
    pub async fn get_history_meta(
        &self,
        entity_type: &str,
        entity_name: &str,
        limit: Option<usize>,
    ) -> Result<Vec<InsightSnapshotMeta>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let limit_val = limit.unwrap_or(20) as i32;

        let mut stmt = conn.inner()?.prepare(
            "SELECT id, entity_type, entity_name, entity_source, snapshot_id, row_count, elapsed_ms, version_id, parent_version_id, checksum, created_at
             FROM insight_snapshots WHERE entity_type = ?1 AND entity_name = ?2 ORDER BY created_at DESC LIMIT ?3"
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "prepare_history_meta".to_string(),
            reason: e.to_string(),
        }))?;

        let entries: Vec<InsightSnapshotMeta> = stmt
            .query_map(
                rusqlite::params![entity_type, entity_name, limit_val],
                |row| {
                    Ok(InsightSnapshotMeta {
                        id: row.get(0)?,
                        entity_type: row.get(1)?,
                        entity_name: row.get(2)?,
                        entity_source: row.get(3)?,
                        snapshot_id: row.get(4)?,
                        row_count: row.get(5)?,
                        elapsed_ms: row.get(6)?,
                        version_id: row.get(7)?,
                        parent_version_id: row.get(8)?,
                        checksum: row.get(9)?,
                        created_at: row.get(10)?,
                    })
                },
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_history_meta".to_string(),
                    reason: e.to_string(),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(entries)
    }

    /// 按版本 ID 获取元数据
    pub async fn get_meta_by_version(
        &self,
        version_id: &str,
    ) -> Result<Option<InsightSnapshotMeta>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let result = conn.inner()?.query_row(
            "SELECT id, entity_type, entity_name, entity_source, snapshot_id, row_count, elapsed_ms, version_id, parent_version_id, checksum, created_at
             FROM insight_snapshots WHERE version_id = ?1",
            rusqlite::params![version_id],
            |row| {
                Ok(InsightSnapshotMeta {
                    id: row.get(0)?,
                    entity_type: row.get(1)?,
                    entity_name: row.get(2)?,
                    entity_source: row.get(3)?,
                    snapshot_id: row.get(4)?,
                    row_count: row.get(5)?,
                    elapsed_ms: row.get(6)?,
                    version_id: row.get(7)?,
                    parent_version_id: row.get(8)?,
                    checksum: row.get(9)?,
                    created_at: row.get(10)?,
                })
            },
        );

        match result {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_meta_by_version".to_string(),
                reason: e.to_string(),
            })),
        }
    }

    /// 删除指定洞察的元数据记录
    pub async fn delete_meta(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?
            .execute(
                "DELETE FROM insight_snapshots WHERE id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "delete_insight_meta".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    /// 统计指定类型洞察的总数
    pub async fn count_by_type(&self, entity_type: &str) -> Result<i32, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let count: i32 = conn
            .inner()?
            .query_row(
                "SELECT COUNT(*) FROM insight_snapshots WHERE entity_type = ?1",
                rusqlite::params![entity_type],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "count_insight_meta".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(count)
    }

    /// 按 snapshot_id 删除元数据记录
    pub async fn delete_meta_by_snapshot_id(&self, snapshot_id: &str) -> Result<usize, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let deleted = conn
            .inner()?
            .execute(
                "DELETE FROM insight_snapshots WHERE snapshot_id = ?1",
                rusqlite::params![snapshot_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "delete_meta_by_snapshot_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(deleted)
    }

    /// 统计某列的版本总数
    pub async fn count_versions_for_column(&self, column_name: &str) -> Result<usize, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let count: i64 = conn.inner()?.query_row(
            "SELECT COUNT(*) FROM insight_snapshots WHERE entity_type = 'column' AND entity_name = ?1",
            rusqlite::params![column_name],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "count_versions_for_column".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(count as usize)
    }

    /// 按天数清理过期元数据
    ///
    /// 返回清理的条目数
    pub async fn cleanup_older_than(&self, days: i32) -> Result<usize, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let deleted = conn
            .inner()?
            .execute(
                "DELETE FROM insight_snapshots
             WHERE created_at < datetime('now', ?1 || ' days')",
                rusqlite::params![format!("-{}", days)],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "cleanup_older_than".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(deleted)
    }
}
