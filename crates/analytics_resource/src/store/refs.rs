//! 资产谱系的读写（迁移 029）。

use chrono::Utc;
use shared::error::{CoreError, StorageError};

use super::AnalyticsResourceStore;

/// 一条谱系关系（时间线展示用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef {
    pub rel_type: String,
    pub from_id: String,
    pub to_id: String,
    pub created_at: chrono::DateTime<Utc>,
}

fn refs_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resource_refs".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

impl AnalyticsResourceStore {
    /// 在**调用方连接**上记一条关系（可并入调用方事务）。
    pub(crate) fn record_ref_on(
        conn: &rusqlite::Connection,
        from_id: &str,
        to_id: &str,
        rel_type: &str,
    ) -> Result<(), CoreError> {
        conn.execute(
            "INSERT INTO analytics_resource_refs (id, from_id, to_id, rel_type, created_at) \
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                format!("arr_{}", uuid::Uuid::new_v4().simple()),
                from_id,
                to_id,
                rel_type,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| refs_err("insert", e))?;
        Ok(())
    }

    /// 查一条存档相关的全部关系（两端都算，时间倒序）。
    pub async fn get_refs(&self, resource_id: &str) -> Result<Vec<ResourceRef>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let mut stmt = inner
            .prepare(
                "SELECT rel_type, from_id, to_id, created_at FROM analytics_resource_refs \
                 WHERE from_id = ? OR to_id = ? ORDER BY created_at DESC",
            )
            .map_err(|e| refs_err("select", e))?;
        let rows = stmt
            .query_map(rusqlite::params![resource_id, resource_id], |row| {
                Ok(ResourceRef {
                    rel_type: row.get(0)?,
                    from_id: row.get(1)?,
                    to_id: row.get(2)?,
                    created_at: Self::parse_datetime_sqlite(row.get(3)?)?,
                })
            })
            .map_err(|e| refs_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| refs_err("select", e))
    }
}
