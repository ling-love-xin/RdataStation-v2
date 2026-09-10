use super::*;
use crate::core::error::{CommonError, CoreError, StorageError};
use chrono::Utc;
use serde_json::Value;

impl AnalyticsResourceStore {
    pub async fn create_resource(
        &self,
        req: CreateResourceRequest,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                version, parent_version_id, parent_resource_id, source_query, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    &req.resource_type,
                    &req.name,
                    &req.alias,
                    serde_json::to_string(&req.config)
                        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                    &req.scope,
                    req.row_count,
                    req.column_count,
                    req.file_size,
                    1,
                    None::<String>,
                    req.parent_resource_id,
                    req.source_query,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.get_resource_by_id(&id).await
    }

    pub async fn update_resource(
        &self,
        id: &str,
        req: CreateResourceRequest,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let now = Utc::now();

        let current = self.get_resource_by_id(id).await?;

        let snapshot = serde_json::to_string(&current)
            .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
        self.save_resource_version(id, current.version, &snapshot)
            .await?;

        conn.inner()?.execute(
            r#"
            UPDATE analytics_resources
            SET name = ?, alias = ?, config = ?, scope = ?, row_count = ?, column_count = ?, file_size = ?,
                version = version + 1, parent_version_id = ?, updated_at = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
            rusqlite::params![
                &req.name,
                &req.alias,
                serde_json::to_string(&req.config).map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                &req.scope,
                req.row_count,
                req.column_count,
                req.file_size,
                Some(&current.id),
                now.to_rfc3339(),
                id,
            ],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_resources".to_string(),
            operation: "update".to_string(),
            reason: e.to_string(),
        }))?;

        self.get_resource_by_id(id).await
    }

    pub async fn get_resource_by_id(&self, id: &str) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn.inner()?.prepare(
            r#"
            SELECT id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                   version, parent_version_id, parent_resource_id, source_query, created_at, updated_at,
                   created_by, deleted_at
            FROM analytics_resources
            WHERE id = ?
            "#,
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_resources".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        let resource = stmt
            .query_row(rusqlite::params![id], |row| {
                let config_str: String = row.get(4)?;
                let config: Value = serde_json::from_str(&config_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("config JSON parse failed: {}", e),
                        )),
                    )
                })?;

                Ok(AnalyticsResource {
                    id: row.get(0)?,
                    resource_type: row.get(1)?,
                    name: row.get(2)?,
                    alias: row.get(3)?,
                    config,
                    scope: row.get(5)?,
                    row_count: row.get::<_, Option<i64>>(6)?.map(|v| v as i32),
                    column_count: row.get(7)?,
                    file_size: row.get::<_, Option<i64>>(8)?.map(|v| v as i32),
                    version: row.get(9)?,
                    parent_version_id: row.get(10)?,
                    parent_resource_id: row.get(11)?,
                    source_query: row.get(12)?,
                    created_at: Self::parse_datetime_sqlite(row.get(13)?)?,
                    updated_at: Self::parse_datetime_sqlite(row.get(14)?)?,
                    created_by: row.get(15)?,
                    deleted_at: row.get(16).ok().and_then(|s| Self::parse_datetime(s).ok()),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(resource)
    }

    pub async fn list_resources(
        &self,
        scope: Option<&str>,
        resource_type: Option<&str>,
        folder_id: Option<&str>,
    ) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;

        let mut sql = String::from(
            r#"
            SELECT id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                   version, parent_version_id, parent_resource_id, source_query, created_at, updated_at,
                   created_by, deleted_at
            FROM analytics_resources
            WHERE deleted_at IS NULL
            "#,
        );

        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(s) = scope {
            sql.push_str(" AND scope = ?");
            params.push(rusqlite::types::Value::Text(s.to_string()));
        }

        if let Some(t) = resource_type {
            sql.push_str(" AND resource_type = ?");
            params.push(rusqlite::types::Value::Text(t.to_string()));
        }

        if let Some(f) = folder_id {
            sql.push_str(
                r#"
                AND id IN (
                    SELECT resource_id FROM analytics_resource_folder WHERE folder_id = ?
                )
                "#,
            );
            params.push(rusqlite::types::Value::Text(f.to_string()));
        }

        sql.push_str(" ORDER BY created_at DESC");

        let mut stmt = conn.inner()?.prepare(&sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })?;

        let resources = stmt.query_map(rusqlite::params_from_iter(params), |row| {
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
                row_count: row.get::<_, Option<i64>>(6)?.map(|v| v as i32),
                column_count: row.get(7)?,
                file_size: row.get::<_, Option<i64>>(8)?.map(|v| v as i32),
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
            store: "analytics_resources".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        resources.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }

    pub async fn clone_resource(
        &self,
        id: &str,
        new_name: Option<&str>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;

        let original = self.get_resource_by_id(id).await?;

        let cloned_id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();
        let default_name = format!("{} (鍓湰)", original.name);
        let cloned_name = new_name.unwrap_or(&default_name);

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                version, parent_version_id, parent_resource_id, source_query, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &cloned_id,
                    &original.resource_type,
                    cloned_name,
                    original.alias,
                    serde_json::to_string(&original.config)
                        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                    &original.scope,
                    original.row_count,
                    original.column_count,
                    original.file_size,
                    1,
                    None::<String>,
                    Some(&original.id),
                    original.source_query,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.get_resource_by_id(&cloned_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_resources_paginated(
        &self,
        scope: Option<&str>,
        resource_type: Option<&str>,
        folder_id: Option<&str>,
        search: Option<&str>,
        page: i32,
        page_size: i32,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<ListResourcesOutput, CoreError> {
        let conn = self.get_conn().await?;

        let mut where_clauses = vec!["deleted_at IS NULL".to_string()];
        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(s) = scope {
            where_clauses.push("scope = ?".to_string());
            params.push(rusqlite::types::Value::Text(s.to_string()));
        }

        if let Some(t) = resource_type {
            where_clauses.push("resource_type = ?".to_string());
            params.push(rusqlite::types::Value::Text(t.to_string()));
        }

        if let Some(f) = folder_id {
            where_clauses.push(
                "id IN (SELECT resource_id FROM analytics_resource_folder WHERE folder_id = ?)"
                    .to_string(),
            );
            params.push(rusqlite::types::Value::Text(f.to_string()));
        }

        if let Some(search_term) = search {
            where_clauses.push("(name LIKE ? OR alias LIKE ?)".to_string());
            let search_pattern = format!("%{}%", search_term);
            params.push(rusqlite::types::Value::Text(search_pattern.clone()));
            params.push(rusqlite::types::Value::Text(search_pattern));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM analytics_resources {}", where_sql);

        let total: i32 = conn
            .inner()?
            .query_row(
                &count_sql,
                rusqlite::params_from_iter(params.iter()),
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "count".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let sort_field = match sort_by {
            Some("name") => "name",
            Some("created_at") => "created_at",
            Some("updated_at") => "updated_at",
            Some("row_count") => "row_count",
            Some("file_size") => "file_size",
            _ => "created_at",
        };

        let sort_dir = match sort_order {
            Some("desc") => "DESC",
            _ => "ASC",
        };

        let offset = (page - 1) * page_size;

        let sql = format!(
            r#"
            SELECT id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                   version, parent_version_id, parent_resource_id, source_query, created_at, updated_at,
                   created_by, deleted_at
            FROM analytics_resources
            {}
            ORDER BY {} {}
            LIMIT ? OFFSET ?
            "#,
            where_sql, sort_field, sort_dir
        );

        params.push(rusqlite::types::Value::Integer(page_size as i64));
        params.push(rusqlite::types::Value::Integer(offset as i64));

        let mut stmt = conn.inner()?.prepare(&sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })?;

        let resources = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
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
                row_count: row.get::<_, Option<i64>>(6)?.map(|v| v as i32),
                column_count: row.get(7)?,
                file_size: row.get::<_, Option<i64>>(8)?.map(|v| v as i32),
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
            store: "analytics_resources".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        let items: Vec<AnalyticsResource> =
            resources.collect::<Result<Vec<_>, _>>().map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let total_pages = if total == 0 {
            1
        } else {
            (total + page_size - 1) / page_size
        };

        Ok(ListResourcesOutput {
            items,
            total,
            page,
            page_size,
            total_pages,
        })
    }
}
