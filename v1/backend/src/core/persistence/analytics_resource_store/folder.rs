use super::*;
use crate::core::error::{CoreError, StorageError};
use chrono::Utc;

impl AnalyticsResourceStore {
    pub async fn create_folder(
        &self,
        req: CreateFolderRequest,
    ) -> Result<AnalyticsFolder, CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("af_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_folders (
                id, name, scope, parent_folder_id, sort_order, color, icon, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    &req.name,
                    &req.scope,
                    req.parent_folder_id,
                    0,
                    req.color,
                    req.icon,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_folders".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.get_folder_by_id(&id).await
    }

    pub async fn get_folder_by_id(&self, id: &str) -> Result<AnalyticsFolder, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn.inner()?.prepare(
            r#"
            SELECT id, name, scope, parent_folder_id, sort_order, color, icon, created_at, updated_at, deleted_at
            FROM analytics_folders
            WHERE id = ?
            "#,
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_folders".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        let folder = stmt
            .query_row(rusqlite::params![id], |row| {
                Ok(AnalyticsFolder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    scope: row.get(2)?,
                    parent_folder_id: row.get(3)?,
                    sort_order: row.get(4)?,
                    color: row.get(5)?,
                    icon: row.get(6)?,
                    created_at: Self::parse_datetime_sqlite(row.get(7)?)?,
                    updated_at: Self::parse_datetime_sqlite(row.get(8)?)?,
                    deleted_at: row.get(9).ok().and_then(|s| Self::parse_datetime(s).ok()),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_folders".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(folder)
    }

    pub async fn list_folders(
        &self,
        scope: Option<&str>,
        parent_folder_id: Option<&str>,
    ) -> Result<Vec<AnalyticsFolder>, CoreError> {
        let conn = self.get_conn().await?;

        let mut sql = String::from(
            r#"
            SELECT id, name, scope, parent_folder_id, sort_order, color, icon, created_at, updated_at, deleted_at
            FROM analytics_folders
            WHERE deleted_at IS NULL
            "#,
        );

        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(s) = scope {
            sql.push_str(" AND scope = ?");
            params.push(rusqlite::types::Value::Text(s.to_string()));
        }

        if let Some(p) = parent_folder_id {
            sql.push_str(" AND parent_folder_id = ?");
            params.push(rusqlite::types::Value::Text(p.to_string()));
        } else {
            sql.push_str(" AND parent_folder_id IS NULL");
        }

        sql.push_str(" ORDER BY sort_order ASC, name ASC");

        let mut stmt = conn.inner()?.prepare(&sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_folders".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })?;

        let folders = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok(AnalyticsFolder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    scope: row.get(2)?,
                    parent_folder_id: row.get(3)?,
                    sort_order: row.get(4)?,
                    color: row.get(5)?,
                    icon: row.get(6)?,
                    created_at: Self::parse_datetime_sqlite(row.get(7)?)?,
                    updated_at: Self::parse_datetime_sqlite(row.get(8)?)?,
                    deleted_at: row.get(9).ok().and_then(|s| Self::parse_datetime(s).ok()),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_folders".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        folders.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_folders".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }

    pub async fn add_resource_to_folder(
        &self,
        resource_id: &str,
        folder_id: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;

        conn.inner()?
            .execute(
                r#"
            INSERT OR REPLACE INTO analytics_resource_folder (resource_id, folder_id, sort_order)
            VALUES (?, ?, 0)
            "#,
                rusqlite::params![resource_id, folder_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_folder".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    pub async fn remove_resource_from_folder(
        &self,
        resource_id: &str,
        folder_id: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;

        conn.inner()?
            .execute(
                r#"
            DELETE FROM analytics_resource_folder
            WHERE resource_id = ? AND folder_id = ?
            "#,
                rusqlite::params![resource_id, folder_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_folder".to_string(),
                    operation: "delete".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }
}
