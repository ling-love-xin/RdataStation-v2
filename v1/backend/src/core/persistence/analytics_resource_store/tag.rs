use super::*;
use crate::core::error::{CoreError, StorageError};
use chrono::Utc;
use serde_json::Value;

impl AnalyticsResourceStore {
    pub async fn create_tag(&self, req: CreateTagRequest) -> Result<AnalyticsTag, CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("at_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_tags (id, name, color, icon, scope, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    &req.name,
                    req.color,
                    req.icon,
                    &req.scope,
                    now.to_rfc3339()
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_tags".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.get_tag_by_id(&id).await
    }

    pub async fn get_tag_by_id(&self, id: &str) -> Result<AnalyticsTag, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT id, name, color, icon, scope, created_at, deleted_at
            FROM analytics_tags
            WHERE id = ?
            "#,
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let tag = stmt
            .query_row(rusqlite::params![id], |row| {
                Ok(AnalyticsTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    icon: row.get(3)?,
                    scope: row.get(4)?,
                    created_at: Self::parse_datetime_sqlite(row.get(5)?)?,
                    deleted_at: row.get(6).ok().and_then(|s| Self::parse_datetime(s).ok()),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(tag)
    }

    pub async fn list_tags(&self, scope: Option<&str>) -> Result<Vec<AnalyticsTag>, CoreError> {
        let conn = self.get_conn().await?;

        let mut sql = String::from(
            r#"
            SELECT id, name, color, icon, scope, created_at, deleted_at
            FROM analytics_tags
            WHERE deleted_at IS NULL
            "#,
        );

        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(s) = scope {
            sql.push_str(" AND scope = ?");
            params.push(rusqlite::types::Value::Text(s.to_string()));
        }

        sql.push_str(" ORDER BY name ASC");

        let mut stmt = conn.inner()?.prepare(&sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_tags".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })?;

        let tags = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok(AnalyticsTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    icon: row.get(3)?,
                    scope: row.get(4)?,
                    created_at: Self::parse_datetime_sqlite(row.get(5)?)?,
                    deleted_at: row.get(6).ok().and_then(|s| Self::parse_datetime(s).ok()),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        tags.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_tags".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }

    pub async fn add_tag_to_resource(
        &self,
        resource_id: &str,
        tag_id: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;

        conn.inner()?
            .execute(
                r#"
            INSERT OR REPLACE INTO analytics_resource_tags (resource_id, tag_id)
            VALUES (?, ?)
            "#,
                rusqlite::params![resource_id, tag_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_tags".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    pub async fn remove_tag_from_resource(
        &self,
        resource_id: &str,
        tag_id: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;

        conn.inner()?
            .execute(
                r#"
            DELETE FROM analytics_resource_tags
            WHERE resource_id = ? AND tag_id = ?
            "#,
                rusqlite::params![resource_id, tag_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_tags".to_string(),
                    operation: "delete".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    // ==================== 鏍囩鍙屽悜鏌ヨ ====================

    pub async fn get_tags_for_resource(
        &self,
        resource_id: &str,
    ) -> Result<Vec<AnalyticsTag>, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT t.id, t.name, t.color, t.icon, t.scope, t.created_at, t.deleted_at
            FROM analytics_tags t
            INNER JOIN analytics_resource_tags rt ON t.id = rt.tag_id
            WHERE rt.resource_id = ? AND t.deleted_at IS NULL
            ORDER BY t.name ASC
            "#,
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let tags = stmt
            .query_map(rusqlite::params![resource_id], |row| {
                Ok(AnalyticsTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    icon: row.get(3)?,
                    scope: row.get(4)?,
                    created_at: Self::parse_datetime_sqlite(row.get(5)?)?,
                    deleted_at: row.get(6).ok().and_then(|s| Self::parse_datetime(s).ok()),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        tags.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resource_tags".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }

    pub async fn get_resources_by_tag(
        &self,
        tag_id: &str,
    ) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT r.id, r.resource_type, r.name, r.alias, r.config, r.scope,
                   r.row_count, r.column_count, r.file_size, r.version,
                   r.parent_version_id, r.parent_resource_id, r.source_query,
                   r.created_at, r.updated_at, r.created_by, r.deleted_at
            FROM analytics_resources r
            INNER JOIN analytics_resource_tags rt ON r.id = rt.resource_id
            WHERE rt.tag_id = ? AND r.deleted_at IS NULL
            ORDER BY r.created_at DESC
            "#,
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let resources = stmt.query_map(rusqlite::params![tag_id], |row| {
            let config_str: String = row.get(4)?;
            let config: Value = serde_json::from_str(&config_str).unwrap_or_else(|e| {
                tracing::warn!(error = %e, config_str = %config_str, "Failed to parse resource config JSON, using null");
                Value::Null
            });

            Ok(AnalyticsResource {
                id: row.get(0)?,
                resource_type: row.get(1)?,
                name: row.get(2)?,
                alias: row.get(3)?,
                config,
                scope: row.get(5)?,
                row_count: row.get(6)?,
                column_count: row.get(7)?,
                file_size: row.get(8)?,
                version: row.get(9)?,
                parent_version_id: row.get(10)?,
                parent_resource_id: row.get(11)?,
                source_query: row.get(12)?,
                created_at: Self::parse_datetime_sqlite(row.get(13)?)?,
                updated_at: Self::parse_datetime_sqlite(row.get(14)?)?,
                created_by: row.get(15)?,
                deleted_at: row.get(16).ok().and_then(|s| Self::parse_datetime(s).ok()),
            })
        }).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_resource_tags".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        resources.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resource_tags".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }
}
