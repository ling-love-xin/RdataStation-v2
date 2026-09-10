use super::*;
use crate::core::error::{CoreError, StorageError};
use chrono::Utc;
use serde_json::Value;

impl AnalyticsResourceStore {
    // ==================== 鐗堟湰鍘嗗彶 ====================

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

    pub async fn save_resource_version(
        &self,
        resource_id: &str,
        version: i32,
        snapshot: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("arv_{}", uuid::Uuid::new_v4().simple());

        conn.inner()?.execute(
            r#"
            INSERT OR IGNORE INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
            rusqlite::params![&id, resource_id, version, snapshot, Utc::now().to_rfc3339()],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_resource_versions".to_string(),
            operation: "insert".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }
}
