use super::*;
use shared::error::{CoreError, StorageError};
use chrono::Utc;

/// 标签表的错误（与 `resource.rs` 的 `persistence_err` 同形，store 名不同）。
fn tag_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_tags".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 标签行映射（四处查询共用；列序即 `SELECT` 的列序）。
fn map_tag_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalyticsTag> {
    Ok(AnalyticsTag {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        icon: row.get(3)?,
        scope: row.get(4)?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(5)?)?,
        deleted_at: row
            .get(6)
            .ok()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
    })
}

impl AnalyticsResourceStore {
    /// 新建标签。
    ///
    /// **同名（未删）拒绝**：库里本就有 `(name, scope) WHERE deleted_at IS NULL` 的部分唯一
    /// 索引兑底，但那条约束报的是英文 SQLite 原话——先查一次，给一句人能读、能直接纠正的话。
    pub async fn create_tag(&self, req: CreateTagRequest) -> Result<AnalyticsTag, CoreError> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(tag_err("insert", "标签名不能为空"));
        }
        if self.tag_name_taken(name, &req.scope).await? {
            return Err(tag_err("insert", &format!("已经有同名标签「{name}」")));
        }

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
                    name,
                    req.color,
                    req.icon,
                    &req.scope,
                    now.to_rfc3339()
                ],
            )
            .map_err(|e| tag_err("insert", e))?;

        self.get_tag_by_id(&id).await
    }

    /// 名字是否已被（未删的）标签占用。
    async fn tag_name_taken(&self, name: &str, scope: &str) -> Result<bool, CoreError> {
        let conn = self.get_conn().await?;
        let count: i64 = conn
            .inner()?
            .query_row(
                "SELECT COUNT(*) FROM analytics_tags \
                 WHERE name = ? AND scope = ? AND deleted_at IS NULL",
                rusqlite::params![name, scope],
                |row| row.get(0),
            )
            .map_err(|e| tag_err("select", e))?;
        Ok(count > 0)
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
            .query_row(rusqlite::params![id], map_tag_row)
            .map_err(|e| tag_err("select", e))?;

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
            .query_map(rusqlite::params_from_iter(params), map_tag_row)
            .map_err(|e| tag_err("select", e))?;

        tags.collect::<Result<Vec<_>, _>>()
            .map_err(|e| tag_err("select", e))
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

    // ==================== 标签双向查询 ====================

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
            .query_map(rusqlite::params![resource_id], map_tag_row)
            .map_err(|e| tag_err("select", e))?;

        tags.collect::<Result<Vec<_>, _>>()
            .map_err(|e| tag_err("select", e))
    }

    pub async fn get_resources_by_tag(
        &self,
        tag_id: &str,
    ) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;

        // 列顺序与行映射复用权威定义（`RESOURCE_COLUMNS` + `map_resource_row`）。
        let sql = format!(
            "SELECT {} FROM analytics_resources r \
             INNER JOIN analytics_resource_tags rt ON r.id = rt.resource_id \
             WHERE rt.tag_id = ? AND r.deleted_at IS NULL ORDER BY r.created_at DESC",
            crate::resource::qualified_resource_columns("r")
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resource_tags".to_string(),
                    operation: "select".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let resources = stmt
            .query_map(rusqlite::params![tag_id], crate::resource::map_resource_row)
            .map_err(|e| CoreError::storage(StorageError::Persistence {
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

    /// 改名（**补 v1 缺失的能力**）：只改显示名，`scope` 不动——作用域由标签住哪个库决定，
    /// 不是可编辑项（架构 §4.3 的“scope 派生”同一条原则）。
    ///
    /// 幂等：改成现在这个名字直接返回原行（不报“同名”——那是用户自己的名字）。
    pub async fn rename_tag(&self, id: &str, name: &str) -> Result<AnalyticsTag, CoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(tag_err("rename", "标签名不能为空"));
        }
        let current = self.get_tag_by_id(id).await?;
        if current.name == name {
            return Ok(current);
        }
        if self.tag_name_taken(name, &current.scope).await? {
            return Err(tag_err("rename", &format!("已经有同名标签「{name}」")));
        }

        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                "UPDATE analytics_tags SET name = ? WHERE id = ? AND deleted_at IS NULL",
                rusqlite::params![name, id],
            )
            .map_err(|e| tag_err("rename", e))?;
        if affected == 0 {
            return Err(tag_err("rename", "标签不存在或已删除"));
        }
        self.get_tag_by_id(id).await
    }

    /// 删除标签：**软删标签行 + 清掉它的全部资源关联**（返回解除的关联数）。
    ///
    /// 两件事必须在同一事务里：只软删行会让关联留在库里（“以后重建同名标签”会把旧归属带回来），
    /// 只清关联又会在标签列表里留一条空标签——都是半个状态。
    pub async fn delete_tag(&self, id: &str) -> Result<usize, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| tag_err("begin_immediate", e))?;

        let result = (|| -> Result<usize, CoreError> {
            let affected = inner
                .execute(
                    "UPDATE analytics_tags SET deleted_at = ? \
                     WHERE id = ? AND deleted_at IS NULL",
                    rusqlite::params![Utc::now().to_rfc3339(), id],
                )
                .map_err(|e| tag_err("delete", e))?;
            if affected == 0 {
                return Err(tag_err("delete", "标签不存在或已删除"));
            }
            let unlinked = inner
                .execute(
                    "DELETE FROM analytics_resource_tags WHERE tag_id = ?",
                    rusqlite::params![id],
                )
                .map_err(|e| tag_err("delete", e))?;
            Ok(unlinked)
        })();

        match result {
            Ok(unlinked) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| tag_err("commit", e))?;
                Ok(unlinked)
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 一次查完全部资源的标签（`resource_id → 标签`，按标签名升序）。
    ///
    /// 详情与筛选都要按行给标签；逐行 `get_tags_for_resource` 会把一次刷新变成 N+1 次查询
    /// （`version_counts` 同理，见 `version.rs`）。
    pub async fn tags_by_resource(
        &self,
    ) -> Result<std::collections::HashMap<String, Vec<AnalyticsTag>>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT rt.resource_id, t.id, t.name, t.color, t.icon, t.scope, t.created_at, t.deleted_at
            FROM analytics_resource_tags rt
            INNER JOIN analytics_tags t ON t.id = rt.tag_id
            INNER JOIN analytics_resources r ON r.id = rt.resource_id
            WHERE t.deleted_at IS NULL AND r.deleted_at IS NULL
            ORDER BY rt.resource_id ASC, t.name ASC
            "#,
            )
            .map_err(|e| tag_err("select", e))?;

        let rows = stmt
            .query_map([], |row| {
                let resource_id: String = row.get(0)?;
                // 列 1.. 与 `map_tag_row` 同形，但多了一列前缀（resource_id）。
                Ok((resource_id, map_tag_row_offset(row, 1)?))
            })
            .map_err(|e| tag_err("select", e))?;

        let mut grouped: std::collections::HashMap<String, Vec<AnalyticsTag>> =
            std::collections::HashMap::new();
        for row in rows {
            let (resource_id, tag) = row.map_err(|e| tag_err("select", e))?;
            grouped.entry(resource_id).or_default().push(tag);
        }
        Ok(grouped)
    }

    /// 每个标签被多少条**存活**存档用着（筛选菜单里的计数）。
    pub async fn tag_usage_counts(
        &self,
    ) -> Result<std::collections::HashMap<String, i64>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT rt.tag_id, COUNT(*)
            FROM analytics_resource_tags rt
            INNER JOIN analytics_resources r ON r.id = rt.resource_id
            WHERE r.deleted_at IS NULL
            GROUP BY rt.tag_id
            "#,
            )
            .map_err(|e| tag_err("select", e))?;

        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|e| tag_err("select", e))?;

        let mut counts = std::collections::HashMap::new();
        for row in rows {
            let (tag_id, count) = row.map_err(|e| tag_err("select", e))?;
            counts.insert(tag_id, count);
        }
        Ok(counts)
    }
}

/// 带列偏移的标签行映射（`tags_by_resource` 的 `SELECT` 前面多一列 `resource_id`）。
fn map_tag_row_offset(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<AnalyticsTag> {
    Ok(AnalyticsTag {
        id: row.get(offset)?,
        name: row.get(offset + 1)?,
        color: row.get(offset + 2)?,
        icon: row.get(offset + 3)?,
        scope: row.get(offset + 4)?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(offset + 5)?)?,
        deleted_at: row
            .get(offset + 6)
            .ok()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
    })
}
