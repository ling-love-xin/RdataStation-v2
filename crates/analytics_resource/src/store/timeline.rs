//! 时间线事件的读写（迁移 027）。
//!
//! `record_event_on` 接受调用方连接，是为了让「索引更新 + 版本快照 + 事件」能落在同一个
//! 事务里：时间线一旦与索引各写各的，就会出现「记录说归档了、行却不在」这种自相矛盾的履历。

use chrono::Utc;
use shared::error::{CoreError, StorageError};

use super::AnalyticsResourceStore;
use crate::model::{TimelineEvent, TimelineEventKind};

fn events_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resource_events".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

impl AnalyticsResourceStore {
    /// 在**调用方连接**上写一条事件（可并入调用方事务）。
    ///
    /// `detail` 只放人能读懂的一句说明，不塞结构化 JSON——时间线是给人看的履历，
    /// 结构化事实归断言表 / 版本快照。
    pub(crate) fn record_event_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
        version: Option<i32>,
        kind: TimelineEventKind,
        detail: Option<&str>,
        content_hash: Option<&str>,
    ) -> Result<(), CoreError> {
        let id = format!("are_{}", uuid::Uuid::new_v4().simple());
        conn.execute(
            "INSERT INTO analytics_resource_events \
             (id, resource_id, version, event_type, detail, content_hash, occurred_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                id,
                resource_id,
                version,
                kind.as_db_str(),
                detail,
                content_hash,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| events_err("insert", e))?;
        Ok(())
    }

    /// 自取连接的便捷版（服务层非事务路径用）。
    ///
    /// 事件写入失败**不回滚主动作**：履历少一条可以事后补，把一次成功的归档整个撤销
    /// 反而更糟；但必须打日志，不能静默。
    pub async fn record_event(
        &self,
        resource_id: &str,
        version: Option<i32>,
        kind: TimelineEventKind,
        detail: Option<&str>,
        content_hash: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        Self::record_event_on(inner, resource_id, version, kind, detail, content_hash)
    }

    /// 一条存档的全部履历事件（时间倒序，同秒按 id 兜底稳定排序）。
    pub async fn get_resource_events(
        &self,
        resource_id: &str,
    ) -> Result<Vec<TimelineEvent>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let mut stmt = inner
            .prepare(
                "SELECT id, resource_id, version, event_type, detail, content_hash, occurred_at \
                 FROM analytics_resource_events WHERE resource_id = ? \
                 ORDER BY occurred_at DESC, id DESC",
            )
            .map_err(|e| events_err("select", e))?;
        let rows = stmt
            .query_map(rusqlite::params![resource_id], |row| {
                Ok(TimelineEvent {
                    id: row.get(0)?,
                    resource_id: row.get(1)?,
                    version: row.get(2)?,
                    kind: TimelineEventKind::from_db_str(&row.get::<_, String>(3)?),
                    detail: row.get(4)?,
                    content_hash: row.get(5)?,
                    occurred_at: Self::parse_datetime_sqlite(row.get(6)?)?,
                })
            })
            .map_err(|e| events_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| events_err("select", e))
    }
}
