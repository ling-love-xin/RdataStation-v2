//! 业务断言与效力的读写（迁移 028）。
//!
//! 与 027 同一纪律：断言 / 效力变更与对应的履历事件**同事务**。时间线要能回答
//! 「口径什么时候改过」，就不能出现"断言已改、履历没记"或反过来。

use chrono::Utc;
use rusqlite::OptionalExtension;
use shared::error::{CoreError, StorageError};

use super::AnalyticsResourceStore;
use crate::model::{ArchiveAssertion, Lifecycle, TimelineEventKind};

fn sem_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resource_assertions".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

impl AnalyticsResourceStore {
    /// 在**调用方连接**上写入 / 更新断言，返回口径指纹（可并入调用方事务）。
    pub(crate) fn upsert_assertion_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
        assertion: &ArchiveAssertion,
    ) -> Result<String, CoreError> {
        let hash = assertion.semantic_hash();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO analytics_resource_assertions (
                resource_id, question, metric_key, metric_name, definition, assumptions,
                time_window, conclusion, owner, confirmed_by, confirmed_at,
                semantic_hash, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(resource_id) DO UPDATE SET
                question = excluded.question,
                metric_key = excluded.metric_key,
                metric_name = excluded.metric_name,
                definition = excluded.definition,
                assumptions = excluded.assumptions,
                time_window = excluded.time_window,
                conclusion = excluded.conclusion,
                owner = excluded.owner,
                confirmed_by = excluded.confirmed_by,
                confirmed_at = excluded.confirmed_at,
                semantic_hash = excluded.semantic_hash,
                updated_at = excluded.updated_at
            "#,
            rusqlite::params![
                resource_id,
                assertion.question,
                assertion.metric_key,
                assertion.metric_name,
                assertion.definition,
                assertion.assumptions,
                assertion.time_window,
                assertion.conclusion,
                assertion.owner,
                assertion.confirmed_by,
                assertion.confirmed_at,
                hash,
                now,
            ],
        )
        .map_err(|e| sem_err("upsert", e))?;
        Ok(hash)
    }

    fn current_semantic_hash_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
    ) -> Result<Option<String>, CoreError> {
        conn.query_row(
            "SELECT semantic_hash FROM analytics_resource_assertions WHERE resource_id = ?",
            rusqlite::params![resource_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| sem_err("select", e))
    }

    /// 设置 / 更新业务断言：断言落库 + 口径事件**同事务**。
    ///
    /// 首次记录也写 `SemanticChanged`（口径从「无」到「有」确实改变了结论的可解释性），
    /// detail 用文案区分「已记录」与「已变更」。描述性字段（问题 / 结论 / 责任人）改动
    /// 不改变 `semantic_hash`，因此不会在时间线上刷出无意义的口径变更。
    pub async fn set_assertion_with_event(
        &self,
        resource_id: &str,
        version: i32,
        assertion: &ArchiveAssertion,
    ) -> Result<String, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| sem_err("begin_immediate", e))?;
        let result = (|| -> Result<String, CoreError> {
            let previous = Self::current_semantic_hash_on(inner, resource_id)?;
            let hash = Self::upsert_assertion_on(inner, resource_id, assertion)?;
            if previous.as_deref() != Some(hash.as_str()) {
                let detail = if previous.is_some() {
                    format!("口径变更：{}", assertion.summary())
                } else {
                    format!("业务断言已记录：{}", assertion.summary())
                };
                Self::record_event_on(
                    inner,
                    resource_id,
                    Some(version),
                    TimelineEventKind::SemanticChanged,
                    Some(&detail),
                    Some(&hash),
                )?;
            }
            Ok(hash)
        })();
        match result {
            Ok(hash) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| sem_err("commit", e))?;
                Ok(hash)
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 读一条业务断言（`None` = 从未填写）。
    pub async fn get_assertion(
        &self,
        resource_id: &str,
    ) -> Result<Option<ArchiveAssertion>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .query_row(
                "SELECT question, metric_key, metric_name, definition, assumptions, \
                        time_window, conclusion, owner, confirmed_by, confirmed_at \
                 FROM analytics_resource_assertions WHERE resource_id = ?",
                rusqlite::params![resource_id],
                |row| {
                    Ok(ArchiveAssertion {
                        question: row.get(0)?,
                        metric_key: row.get(1)?,
                        metric_name: row.get(2)?,
                        definition: row.get(3)?,
                        assumptions: row.get(4)?,
                        time_window: row.get(5)?,
                        conclusion: row.get(6)?,
                        owner: row.get(7)?,
                        confirmed_by: row.get(8)?,
                        confirmed_at: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(|e| sem_err("select", e))
    }

    /// 在**调用方连接**上设置效力状态（可并入调用方事务）。
    pub(crate) fn set_lifecycle_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
        state: Lifecycle,
        superseded_by: Option<&str>,
    ) -> Result<(), CoreError> {
        conn.execute(
            r#"
            INSERT INTO analytics_resource_lifecycle (
                resource_id, state, valid_until, superseded_by, updated_at
            ) VALUES (?, ?, NULL, ?, ?)
            ON CONFLICT(resource_id) DO UPDATE SET
                state = excluded.state,
                superseded_by = excluded.superseded_by,
                updated_at = excluded.updated_at
            "#,
            rusqlite::params![
                resource_id,
                state.as_db_str(),
                superseded_by,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|e| sem_err("upsert_lifecycle", e))?;
        Ok(())
    }

    /// 设置效力状态：状态落库 + 效力事件**同事务**（`Draft` 是默认态，不产生事件）。
    pub async fn set_lifecycle_with_event(
        &self,
        resource_id: &str,
        version: i32,
        state: Lifecycle,
        superseded_by: Option<&str>,
        detail: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| sem_err("begin_immediate", e))?;
        let result = (|| -> Result<(), CoreError> {
            let previous = Self::current_lifecycle_on(inner, resource_id)?;
            Self::set_lifecycle_on(inner, resource_id, state, superseded_by)?;
            // 「被取代」同时落一条谱系：影响面分析（谁引用了旧结论）靠它。
            if state == Lifecycle::Superseded {
                if let Some(target) = superseded_by {
                    Self::record_ref_on(inner, resource_id, target, "supersedes")?;
                }
            }
            if let Some(kind) = state.event_kind() {
                Self::record_event_on(inner, resource_id, Some(version), kind, detail, None)?;
            }
            // 从「正式 / 废弃 / 被取代」退回候选也是一次效力变更，不能无痕——
            // 否则追溯时会以为这条资产一直没被确认过。
            if state == Lifecycle::Draft && previous != Lifecycle::Draft {
                Self::record_event_on(
                    inner,
                    resource_id,
                    Some(version),
                    TimelineEventKind::Validated,
                    Some("效力退回候选：不再作为正式依据"),
                    None,
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| sem_err("commit", e))?;
                Ok(())
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 读效力状态（没有记录 = `Draft`，老库天然兼容）。
    pub async fn get_lifecycle(&self, resource_id: &str) -> Result<Lifecycle, CoreError> {
        let conn = self.get_conn().await?;
        let state = conn
            .inner()?
            .query_row(
                "SELECT state FROM analytics_resource_lifecycle WHERE resource_id = ?",
                rusqlite::params![resource_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| sem_err("select_lifecycle", e))?;
        Ok(state
            .as_deref()
            .map(Lifecycle::from_db_str)
            .unwrap_or_default())
    }

    /// 一次查完全部业务断言（刷新用；逐行查会把一次刷新变成 N+1）。
    pub async fn all_assertions(
        &self,
    ) -> Result<std::collections::HashMap<String, ArchiveAssertion>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let mut stmt = inner
            .prepare(
                "SELECT resource_id, question, metric_key, metric_name, definition, assumptions, \
                        time_window, conclusion, owner, confirmed_by, confirmed_at \
                 FROM analytics_resource_assertions",
            )
            .map_err(|e| sem_err("select_all", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ArchiveAssertion {
                        question: row.get(1)?,
                        metric_key: row.get(2)?,
                        metric_name: row.get(3)?,
                        definition: row.get(4)?,
                        assumptions: row.get(5)?,
                        time_window: row.get(6)?,
                        conclusion: row.get(7)?,
                        owner: row.get(8)?,
                        confirmed_by: row.get(9)?,
                        confirmed_at: row.get(10)?,
                    },
                ))
            })
            .map_err(|e| sem_err("select_all", e))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, assertion) = row.map_err(|e| sem_err("select_all", e))?;
            out.insert(id, assertion);
        }
        Ok(out)
    }

    /// 一次查完全部效力状态（同上）。
    pub async fn all_lifecycles(
        &self,
    ) -> Result<std::collections::HashMap<String, Lifecycle>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let mut stmt = inner
            .prepare("SELECT resource_id, state FROM analytics_resource_lifecycle")
            .map_err(|e| sem_err("select_all_lifecycle", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| sem_err("select_all_lifecycle", e))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, state) = row.map_err(|e| sem_err("select_all_lifecycle", e))?;
            out.insert(id, Lifecycle::from_db_str(&state));
        }
        Ok(out)
    }

    /// 读当前效力（事务内用；没有记录 = 候选）。
    fn current_lifecycle_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
    ) -> Result<Lifecycle, CoreError> {
        let state = conn
            .query_row(
                "SELECT state FROM analytics_resource_lifecycle WHERE resource_id = ?",
                rusqlite::params![resource_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| sem_err("select_lifecycle", e))?;
        Ok(state
            .as_deref()
            .map(Lifecycle::from_db_str)
            .unwrap_or_default())
    }

    /// 口径时间窗更新到「未过期」后，撤掉旧的过期提示。
    ///
    /// 只清本模块自动生成的那类（detail 以「业务时间窗」开头），不动人工写入的其它
    /// `Validated` 事件，也不动效力退回候选那条记录。
    pub async fn clear_window_validations(&self, resource_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute(
                "DELETE FROM analytics_resource_events \
                 WHERE resource_id = ? AND event_type = 'validated' \
                   AND detail LIKE '业务时间窗%'",
                rusqlite::params![resource_id],
            )
            .map_err(|e| sem_err("clear_window_validations", e))?;
        Ok(())
    }

    /// 主动校验发现异常时记一条 `Validated` 事件；**同一条说明只记一次**。
    ///
    /// 刷新是高频动作，不能每次都往时间线刷一条重复告警。用一条
    /// `INSERT … SELECT … WHERE NOT EXISTS` 在库里做去重，省掉一次往返也避免竞态。
    pub async fn record_validation_if_absent(
        &self,
        resource_id: &str,
        version: i32,
        detail: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute(
                "INSERT INTO analytics_resource_events \
                 (id, resource_id, version, event_type, detail, content_hash, occurred_at) \
                 SELECT ?, ?, ?, 'validated', ?, NULL, ? \
                 WHERE NOT EXISTS ( \
                     SELECT 1 FROM analytics_resource_events \
                     WHERE resource_id = ? AND event_type = 'validated' AND detail = ? \
                 )",
                rusqlite::params![
                    format!("are_{}", uuid::Uuid::new_v4().simple()),
                    resource_id,
                    version,
                    detail,
                    Utc::now().to_rfc3339(),
                    resource_id,
                    detail,
                ],
            )
            .map_err(|e| sem_err("record_validation", e))?;
        Ok(())
    }
}
