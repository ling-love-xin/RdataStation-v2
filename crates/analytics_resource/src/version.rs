use super::*;
use shared::error::{CoreError, StorageError};
use chrono::Utc;
use serde_json::Value;

impl AnalyticsResourceStore {
    // ==================== 版本历史 ====================

    pub async fn get_resource_versions(
        &self,
        resource_id: &str,
    ) -> Result<Vec<ResourceVersion>, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT id, resource_id, version, snapshot, created_at
            FROM analytics_resource_versions
            WHERE resource_id = ?
            ORDER BY version DESC
            "#,
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_versions".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let versions = stmt.query_map(rusqlite::params![resource_id], |row| {
            let snapshot_str: String = row.get(3)?;
            Ok(ResourceVersion {
                id: row.get(0)?,
                resource_id: row.get(1)?,
                version: row.get(2)?,
                snapshot: serde_json::from_str(&snapshot_str).unwrap_or_else(|e| {
                    tracing::warn!(error = %e, snapshot_str = %snapshot_str, "Failed to parse version snapshot JSON, using null");
                    Value::Null
                }),
                created_at: Self::parse_datetime_sqlite(row.get(4)?)?,
            })
        }).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_resource_versions".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        versions.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resource_versions".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }

    /// 在**已有连接**上写入一条版本快照，返回快照行 id。
    ///
    /// 与 v1 的两点差别：
    /// 1. 用裸 `INSERT` 而非 `INSERT OR IGNORE`：`UNIQUE(resource_id, version)` 冲突意味着
    ///    "同一版本被写了两次"，那是必须暴露的缺陷，不是可以静默吞掉的情况（v1 因此丢版本）。
    /// 2. 返回行 id，供资源行的 `parent_version_id` 指向本次快照（v1 该列写的是资源自身 id，无信息量）。
    pub(crate) fn save_resource_version_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
        version: i32,
        snapshot: &str,
    ) -> Result<String, CoreError> {
        let id = format!("arv_{}", uuid::Uuid::new_v4().simple());

        conn.execute(
            r#"
            INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
            rusqlite::params![&id, resource_id, version, snapshot, Utc::now().to_rfc3339()],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_resource_versions".to_string(),
            operation: "insert".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(id)
    }

    /// 自取连接的便捷版（不经事务的调用方用，如归档服务）；**返回快照行 id**，
    /// 便于调用方把资源的 `parent_version_id` 指过去。
    pub async fn save_resource_version(
        &self,
        resource_id: &str,
        version: i32,
        snapshot: &str,
    ) -> Result<String, CoreError> {
        let conn = self.get_conn().await?;
        Self::save_resource_version_on(conn.inner()?, resource_id, version, snapshot)
    }
}
