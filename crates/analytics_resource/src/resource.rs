use super::*;
use shared::error::{CommonError, CoreError, StorageError};
use chrono::Utc;
use serde_json::Value;

/// 归档资源表的固定列顺序：单行读取、列表、分页三处必须一致。
///
/// v1 三处各写一遍，且 JSON 解析策略不同（列表宽容 / 单行硬报错）——同一份坏数据会
/// "列表里看得到、点进去报错"。现统一走 [`map_resource_row`]：降级为 `null` 并记 warn，
/// 一行坏数据不应该毁掉整个列表。
const RESOURCE_COLUMNS: &str = "\
    id, resource_type, name, alias, config, scope, row_count, column_count, file_size, \
    version, parent_version_id, parent_resource_id, source_query, created_at, updated_at, \
    created_by, deleted_at";

/// 每页上限：`page_size` 夹紧到 [1, MAX]。
///
/// v1 的 `total_pages` 在 `page_size == 0` 时整数除零 panic；`page_size < 0` 会让 SQLite 的
/// `LIMIT -N` 变成"无上限"（整表返回）。服务层直连调用没有 IPC 兜底，必须自己夹紧。
const MAX_PAGE_SIZE: i32 = 500;

/// 行 → 领域对象（列顺序与 [`RESOURCE_COLUMNS`] 严格一致）。
fn map_resource_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalyticsResource> {
    let config_str: String = row.get(4)?;
    let config: Value = serde_json::from_str(&config_str).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "资源 config JSON 解析失败，降级为 null");
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
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(13)?)?,
        updated_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(14)?)?,
        created_by: row.get(15)?,
        deleted_at: row
            .get(16)
            .ok()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
    })
}

/// 统一的持久层错误构造（v1 每处手写一遍四元组）。
fn persistence_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 分页参数夹紧：页码最小 1，页大小落在 [1, MAX_PAGE_SIZE]。
fn normalize_pagination(page: i32, page_size: i32) -> (i32, i32) {
    (page.max(1), page_size.clamp(1, MAX_PAGE_SIZE))
}

/// LIKE 通配符转义（配合 SQL 的 `ESCAPE '\'`）：v1 未转义，输入 `%` 等于全表匹配。
fn escape_like(term: &str) -> String {
    let mut escaped = String::with_capacity(term.len() + 2);
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

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

    /// 更新资源：读旧行 → 存写前快照 → 递增版本，三步在同一写事务内完成。
    ///
    /// 为何用 `BEGIN IMMEDIATE`：三步之间存在"读版本号 → 写版本号"的竞态。延迟事务不加写锁，
    /// 两个并发更新会读到同一版本号，而后到的快照被 v1 的 `INSERT OR IGNORE` 静默丢弃
    /// （`UNIQUE(resource_id, version)`）——表现为"版本号涨了，历史里却没有"。
    /// 取写锁后，真并发写者会明确报 `SQLITE_BUSY`，而不是静默丢版本。
    ///
    /// 注：池尚未设 `busy_timeout`（engine 侧待修，见开发方案 P0.2），因此本路径**不重试**
    /// BUSY——宁可报错也不静默丢数据；超时补齐后再评估是否需要退避重试。
    pub async fn update_resource(
        &self,
        id: &str,
        req: CreateResourceRequest,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let now = Utc::now();

        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<AnalyticsResource, CoreError> {
            let current = Self::get_resource_by_id_on(inner, id)?;

            let snapshot = serde_json::to_string(&current)
                .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
            let snapshot_id =
                Self::save_resource_version_on(inner, id, current.version, &snapshot)?;

            let affected = inner
                .execute(
                    r#"
            UPDATE analytics_resources
            SET name = ?, alias = ?, config = ?, scope = ?, row_count = ?, column_count = ?, file_size = ?,
                version = version + 1, parent_version_id = ?, updated_at = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
                    rusqlite::params![
                        &req.name,
                        &req.alias,
                        serde_json::to_string(&req.config)
                            .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                        &req.scope,
                        req.row_count,
                        req.column_count,
                        req.file_size,
                        // 指向本次写入的快照行（v1 传的是资源自身 id，该列恒等于 id、无信息量）。
                        Some(&snapshot_id),
                        now.to_rfc3339(),
                        id,
                    ],
                )
                .map_err(|e| persistence_err("update", e))?;

            // v1 不检查影响行数：更新一个已软删除的资源会"静默成功"并返回旧数据。
            if affected == 0 {
                return Err(persistence_err("update", "资源不存在或已删除"));
            }

            Self::get_resource_by_id_on(inner, id)
        })();

        match &result {
            Ok(_) => inner
                .execute_batch("COMMIT")
                .map_err(|e| persistence_err("commit", e))?,
            Err(_) => {
                // 回滚失败只记日志：要向上返回的是原始错误。
                if let Err(e) = inner.execute_batch("ROLLBACK") {
                    tracing::warn!(error = %e, "回滚资源更新事务失败");
                }
            }
        }

        result
    }

    pub async fn get_resource_by_id(&self, id: &str) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        Self::get_resource_by_id_on(conn.inner()?, id)
    }

    /// 单连接版本的读取：供"读 → 写"必须在同一事务内完成的路径复用。
    ///
    /// v1 的 `update_resource` 先占一条连接，调 `get_resource_by_id` 又占一条，存快照再占一条
    /// （池只有 3 条）——一次逻辑操作最坏同时持两条连接，并发时会互相等对方归还。
    pub(crate) fn get_resource_by_id_on(
        conn: &rusqlite::Connection,
        id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let sql = format!("SELECT {RESOURCE_COLUMNS} FROM analytics_resources WHERE id = ?");
        let mut stmt = conn.prepare(&sql).map_err(|e| persistence_err("select", e))?;
        let resource = stmt
            .query_row(rusqlite::params![id], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        Ok(resource)
    }

    pub async fn list_resources(
        &self,
        scope: Option<&str>,
        resource_type: Option<&str>,
        folder_id: Option<&str>,
    ) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;

        let mut sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources WHERE deleted_at IS NULL"
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

        let resources = stmt.query_map(rusqlite::params_from_iter(params), map_resource_row)
            .map_err(|e| persistence_err("select", e))?;

        resources
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))
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
        let default_name = format!("{}（副本）", original.name);
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

        // 边界夹紧：page ≥ 1、page_size ∈ [1, MAX_PAGE_SIZE]（否则 total_pages 会除零 panic）。
        let (page, page_size) = normalize_pagination(page, page_size);

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
            where_clauses.push(r#"(name LIKE ? ESCAPE '\' OR alias LIKE ? ESCAPE '\')"#.to_string());
            let search_pattern = format!("%{}%", escape_like(search_term));
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
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources {where_sql} \
             ORDER BY {sort_field} {sort_dir} LIMIT ? OFFSET ?"
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

        let resources = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), map_resource_row)
            .map_err(|e| persistence_err("select", e))?;

        let items: Vec<AnalyticsResource> = resources
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))?;

        // page_size 已在入口夹紧到 ≥ 1，此除式安全。
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
