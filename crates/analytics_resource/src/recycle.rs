use super::*;
use shared::error::{CommonError, CoreError, StorageError};
use chrono::Utc;
use serde_json::Value;

impl AnalyticsResourceStore {
    pub async fn delete_resource(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let now = Utc::now();

        let resource = self.get_resource_by_id(id).await?;

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_recycle_bin (
                id, resource_id, resource_type, resource_name, resource_data, deleted_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    format!("rb_{}", uuid::Uuid::new_v4().simple()),
                    &resource.id,
                    &resource.resource_type,
                    &resource.name,
                    serde_json::to_string(&resource)
                        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_recycle_bin".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        conn.inner()?
            .execute(
                r#"
            UPDATE analytics_resources
            SET deleted_at = ?
            WHERE id = ?
            "#,
                rusqlite::params![now.to_rfc3339(), id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "update".to_string(),
                    reason: e.to_string(),
                })
            })?;

        conn.inner()?
            .execute(
                r#"
            DELETE FROM analytics_resource_folder
            WHERE resource_id = ?
            "#,
                rusqlite::params![id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_folder".to_string(),
                    operation: "delete".to_string(),
                    reason: e.to_string(),
                })
            })?;

        conn.inner()?
            .execute(
                r#"
            DELETE FROM analytics_resource_tags
            WHERE resource_id = ?
            "#,
                rusqlite::params![id],
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

    pub async fn batch_delete_resources(&self, ids: &[String]) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let now = Utc::now();

        conn.inner()?
            .execute("BEGIN TRANSACTION", [])
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "begin_transaction".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let result = (|| -> Result<(), CoreError> {
            for id in ids {
                let resource = {
                    // 复用权威列列表与行映射：v1 在这里抄了第三份（加一列要改多处，且 JSON 策略不一致）。
                    let sql = format!(
                        "SELECT {} FROM analytics_resources WHERE id = ?",
                        crate::resource::RESOURCE_COLUMNS
                    );
                    let mut stmt = conn
                        .inner()?
                        .prepare(&sql)
                        .map_err(|e| CoreError::storage(StorageError::Persistence {
                            store: "analytics_resources".to_string(),
                            operation: "select".to_string(),
                            reason: e.to_string(),
                        }))?;

                    stmt.query_row(rusqlite::params![id], crate::resource::map_resource_row)
                        .map_err(|e| CoreError::storage(StorageError::Persistence {
                            store: "analytics_resources".to_string(),
                            operation: "select".to_string(),
                            reason: e.to_string(),
                        }))?
                };

                conn.inner()?
                    .execute(
                        r#"
                    INSERT INTO analytics_recycle_bin (
                        id, resource_id, resource_type, resource_name, resource_data, deleted_at
                    ) VALUES (?, ?, ?, ?, ?, ?)
                    "#,
                        rusqlite::params![
                            format!("rb_{}", uuid::Uuid::new_v4().simple()),
                            &resource.id,
                            &resource.resource_type,
                            &resource.name,
                            serde_json::to_string(&resource).map_err(|e| CoreError::common(
                                CommonError::General(e.to_string())
                            ))?,
                            now.to_rfc3339(),
                        ],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_recycle_bin".to_string(),
                            operation: "insert".to_string(),
                            reason: e.to_string(),
                        })
                    })?;

                conn.inner()?
                    .execute(
                        r#"
                    UPDATE analytics_resources
                    SET deleted_at = ?
                    WHERE id = ?
                    "#,
                        rusqlite::params![now.to_rfc3339(), id],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_resources".to_string(),
                            operation: "update".to_string(),
                            reason: e.to_string(),
                        })
                    })?;

                conn.inner()?
                    .execute(
                        r#"
                    DELETE FROM analytics_resource_folder
                    WHERE resource_id = ?
                    "#,
                        rusqlite::params![id],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_resource_folder".to_string(),
                            operation: "delete".to_string(),
                            reason: e.to_string(),
                        })
                    })?;

                conn.inner()?
                    .execute(
                        r#"
                    DELETE FROM analytics_resource_tags
                    WHERE resource_id = ?
                    "#,
                        rusqlite::params![id],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_resource_tags".to_string(),
                            operation: "delete".to_string(),
                            reason: e.to_string(),
                        })
                    })?;
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.inner()?.execute("COMMIT", []).map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "analytics_resources".to_string(),
                        operation: "commit".to_string(),
                        reason: e.to_string(),
                    })
                })?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.inner()?.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    pub async fn get_recycle_items(&self) -> Result<Vec<AnalyticsRecycleItem>, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn.inner()?.prepare(
            r#"
            SELECT id, resource_id, resource_type, resource_name, resource_data, deleted_by, deleted_at
            FROM analytics_recycle_bin
            ORDER BY deleted_at DESC
            "#,
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_recycle_bin".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        let items = stmt.query_map([], |row| {
            let data_str: String = row.get(4)?;
            let resource_data: Value = serde_json::from_str(&data_str).unwrap_or_else(|e| {
                tracing::warn!(error = %e, data_str = %data_str, "Failed to parse recycle item data JSON, using null");
                Value::Null
            });

            Ok(AnalyticsRecycleItem {
                id: row.get(0)?,
                resource_id: row.get(1)?,
                resource_type: row.get(2)?,
                resource_name: row.get(3)?,
                resource_data,
                deleted_by: row.get(5)?,
                deleted_at: Self::parse_datetime_sqlite(row.get(6)?)?,
            })
        }).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "analytics_recycle_bin".to_string(),
            operation: "select".to_string(),
            reason: e.to_string(),
        }))?;

        items.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_recycle_bin".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })
    }

    pub async fn restore_from_recycle(
        &self,
        recycle_id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;

        let resource_id = {
            let mut stmt = conn
                .inner()?
                .prepare(
                    r#"
                SELECT resource_data FROM analytics_recycle_bin WHERE id = ?
                "#,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "analytics_recycle_bin".to_string(),
                        operation: "select".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let data_str: String = stmt
                .query_row(rusqlite::params![recycle_id], |row| row.get(0))
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "analytics_recycle_bin".to_string(),
                        operation: "select".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let resource: AnalyticsResource = serde_json::from_str(&data_str)
                .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;

            conn.inner()?
                .execute("BEGIN TRANSACTION", [])
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "analytics_resources".to_string(),
                        operation: "begin_transaction".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let restore_result = (|| -> Result<(), CoreError> {
                conn.inner()?
                    .execute(
                        r#"
                    UPDATE analytics_resources
                    SET deleted_at = NULL
                    WHERE id = ?
                    "#,
                        rusqlite::params![&resource.id],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_resources".to_string(),
                            operation: "update".to_string(),
                            reason: e.to_string(),
                        })
                    })?;

                conn.inner()?
                    .execute(
                        r#"
                    DELETE FROM analytics_recycle_bin WHERE id = ?
                    "#,
                        rusqlite::params![recycle_id],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_recycle_bin".to_string(),
                            operation: "delete".to_string(),
                            reason: e.to_string(),
                        })
                    })?;

                Ok(())
            })();

            match restore_result {
                Ok(()) => {
                    conn.inner()?.execute("COMMIT", []).map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "analytics_resources".to_string(),
                            operation: "commit".to_string(),
                            reason: e.to_string(),
                        })
                    })?;
                }
                Err(e) => {
                    let _ = conn.inner()?.execute("ROLLBACK", []);
                    return Err(e);
                }
            }

            resource.id
        };

        self.get_resource_by_id(&resource_id).await
    }

    pub async fn permanent_delete(&self, recycle_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;

        conn.inner()?
            .execute(
                r#"
            DELETE FROM analytics_recycle_bin WHERE id = ?
            "#,
                rusqlite::params![recycle_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_recycle_bin".to_string(),
                    operation: "delete".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }
}
