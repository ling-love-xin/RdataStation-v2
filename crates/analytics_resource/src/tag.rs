use super::*;
use shared::error::{CoreError, StorageError};
use chrono::Utc;
use std::sync::atomic::Ordering;
// `TagTable` 的定义方法是 `TagDefinitions` trait 提供的（本地 `mod tag` 遮蔽外部 crate，
// 故用 `::tag::` 绝对路径 + 显式导入 trait）。
use ::tag::TagDefinitions;

/// 标签关联表的错误（定义侧错误由 `tag::TagTable` 自带 `tags` 域错误透传）。
fn tag_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resource_tags".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 全局 `tags` 定义 → 分析资源行模型（`created_at` 双格式解析：全局表存 RFC3339）。
fn tag_to_analytics(tag: &::tag::Tag) -> AnalyticsTag {
    AnalyticsTag {
        id: tag.id.clone(),
        name: tag.name.clone(),
        color: tag.color.clone(),
        icon: tag.icon.clone(),
        scope: tag.scope.clone(),
        created_at: AnalyticsResourceStore::parse_datetime(tag.created_at.clone())
            .unwrap_or_else(|_| Utc::now()),
        deleted_at: tag
            .deleted_at
            .as_deref()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s.to_string()).ok()),
    }
}

impl AnalyticsResourceStore {
    /// 存量标签迁移（规格 §10.2 / §7.2）：项目库 `analytics_tags` 行 → 全局 `tags` 表。
    ///
    /// 幂等（按 id 判重）：全局已有该 id 跳过；全局已有**同名活跃**标签时按规格 D3
    /// 「同名复用」把项目库关联重映射到全局行，不重复建定义。软删行原样迁入（保留标记）。
    /// 并发双跑安全：重映射与插入都是幂等语句。
    pub async fn ensure_tags_migrated(&self) -> Result<(), CoreError> {
        if self.tags_migrated.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.migrate_analytics_tags().await?;
        self.tags_migrated.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn migrate_analytics_tags(&self) -> Result<(), CoreError> {
        // 1. 读项目库存量行（含软删；created_at 双格式）。
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT id, name, color, icon, scope, created_at, deleted_at
            FROM analytics_tags
            ORDER BY created_at ASC
            "#,
            )
            .map_err(|e| tag_err("migrate_select", e))?;
        let rows = stmt
            .query_map([], map_tag_row)
            .map_err(|e| tag_err("migrate_select", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| tag_err("migrate_select", e))?;
        drop(stmt);
        drop(conn);

        // 2. 在全局库内完成判重与「同名 → 重映射 / 异名 → 插入」的决策（同步段，不跨 await 持锁）。
        let (to_insert, to_remap) = self.with_global_tags(|global| {
            let mut to_insert: Vec<AnalyticsTag> = Vec::new();
            let mut to_remap: Vec<(String, String)> = Vec::new();
            for row in rows {
                if global.get_tag(&row.id).is_ok() {
                    continue; // 已迁移（幂等）
                }
                match global.get_tag_by_name(&row.name)? {
                    Some(existing) => to_remap.push((row.id, existing.id)),
                    None => to_insert.push(row),
                }
            }
            Ok((to_insert, to_remap))
        })?;

        // 3. 锁外执行：重映射写项目库关联，插入写全局定义。
        for (old_id, new_id) in to_remap {
            self.remap_tag_links(&old_id, &new_id).await?;
        }
        for row in to_insert {
            let created = row.created_at.to_rfc3339();
            let deleted = row.deleted_at.map(|d| d.to_rfc3339());
            let inserted = self.with_global_tags(|global| {
                global.insert_migrated(
                    &row.id,
                    &row.name,
                    row.color.as_deref(),
                    row.icon.as_deref(),
                    &created,
                    deleted.as_deref(),
                )
            });
            // 并发窗口内全局出现同名活跃 → 降级为关联重映射（幂等，无半状态）。
            if inserted.is_err() {
                if let Some(existing) = self.with_global_tags(|g| g.get_tag_by_name(&row.name))? {
                    self.remap_tag_links(&row.id, &existing.id).await?;
                } else {
                    return inserted;
                }
            }
        }
        Ok(())
    }

    /// 把项目库关联从旧标签 id 改挂到全局同名标签 id（`INSERT OR REPLACE` 防撞
    /// `(resource_id, tag_id)` 主键，再清旧行；两步幂等）。
    async fn remap_tag_links(&self, old_tag_id: &str, new_tag_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute(
                "INSERT OR REPLACE INTO analytics_resource_tags (resource_id, tag_id) \
                 SELECT resource_id, ?1 FROM analytics_resource_tags WHERE tag_id = ?2",
                rusqlite::params![new_tag_id, old_tag_id],
            )
            .map_err(|e| tag_err("remap_insert", e))?;
        inner
            .execute(
                "DELETE FROM analytics_resource_tags WHERE tag_id = ?1",
                rusqlite::params![old_tag_id],
            )
            .map_err(|e| tag_err("remap_delete", e))?;
        Ok(())
    }

    /// 新建标签。
    ///
    /// 定义写**全局库**（规格 §3.2 / §7.2：分析资源标签定义上移全局表）。`req.scope`
    /// 字段已废弃——作用域由存储域派生（恒为 global），不再参与落库。
    pub async fn create_tag(&self, req: CreateTagRequest) -> Result<AnalyticsTag, CoreError> {
        let created = self.with_global_tags(|t| {
            t.create_tag(&req.name, req.color.as_deref(), req.icon.as_deref())
        })?;
        Ok(tag_to_analytics(&created))
    }

    pub async fn get_tag_by_id(&self, id: &str) -> Result<AnalyticsTag, CoreError> {
        let tag = self.with_global_tags(|t| t.get_tag(id))?;
        Ok(tag_to_analytics(&tag))
    }

    /// 活跃标签列表。`scope` 参数保留仅为调用方兼容（分析资源标签恒为 global，忽略之）。
    pub async fn list_tags(&self, _scope: Option<&str>) -> Result<Vec<AnalyticsTag>, CoreError> {
        self.ensure_tags_migrated().await?;
        let tags = self.with_global_tags(|t| t.list_tags())?;
        Ok(tags.iter().map(tag_to_analytics).collect())
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

    /// 存档的活跃标签。定义在全局库、关联在项目库（sqlite 无跨库 join，规格 §7.2 D1）：
    /// 先在关联表取 id 集合，再批量 `IN` 查全局定义。
    pub async fn get_tags_for_resource(
        &self,
        resource_id: &str,
    ) -> Result<Vec<AnalyticsTag>, CoreError> {
        self.ensure_tags_migrated().await?;
        let conn = self.get_conn().await?;

        let mut stmt = conn
            .inner()?
            .prepare(
                "SELECT tag_id FROM analytics_resource_tags WHERE resource_id = ?1",
            )
            .map_err(|e| tag_err("select", e))?;
        let ids = stmt
            .query_map(rusqlite::params![resource_id], |row| row.get::<_, String>(0))
            .map_err(|e| tag_err("select", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| tag_err("select", e))?;
        drop(stmt);
        drop(conn);

        let tags = self.with_global_tags(|t| t.get_tags_by_ids(&ids))?;
        let mut result: Vec<AnalyticsTag> = tags.iter().map(tag_to_analytics).collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
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
    /// 不是可编辑项（架构 §4.3 的“scope 派生”同一条原则）。同名检查与幂等由全局存储处理。
    pub async fn rename_tag(&self, id: &str, name: &str) -> Result<AnalyticsTag, CoreError> {
        let renamed = self.with_global_tags(|t| t.rename_tag(id, name))?;
        Ok(tag_to_analytics(&renamed))
    }

    /// 删除标签：**只软删全局定义行，不清关联**（规格 §5 决策 D2——软删后同名重建产生
    /// 新 id，旧关联查询侧被 `deleted_at IS NULL` 过滤，不会带回；不需要跨库级联清关联）。
    pub async fn delete_tag(&self, id: &str) -> Result<(), CoreError> {
        self.with_global_tags(|t| t.delete_tag(id))
    }

    /// 一次查完全部资源的标签（`resource_id → 标签`，按标签名升序）。
    ///
    /// 详情与筛选都要按行给标签；逐行 `get_tags_for_resource` 会把一次刷新变成 N+1 次查询
    /// （`version_counts` 同理，见 `version.rs`）。跨库：关联对 + 批量 `IN` 查全局定义。
    pub async fn tags_by_resource(
        &self,
    ) -> Result<std::collections::HashMap<String, Vec<AnalyticsTag>>, CoreError> {
        self.ensure_tags_migrated().await?;
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT rt.resource_id, rt.tag_id
            FROM analytics_resource_tags rt
            INNER JOIN analytics_resources r ON r.id = rt.resource_id
            WHERE r.deleted_at IS NULL
            "#,
            )
            .map_err(|e| tag_err("select", e))?;
        let pairs = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| tag_err("select", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| tag_err("select", e))?;
        drop(stmt);
        drop(conn);

        // 去重收集 tag id，一次批量解析定义（活跃过滤在 `get_tags_by_ids`）。
        let mut ids: Vec<String> = pairs.iter().map(|(_, tag_id)| tag_id.clone()).collect();
        ids.sort_unstable();
        ids.dedup();
        let tags = self.with_global_tags(|t| t.get_tags_by_ids(&ids))?;
        let by_id: std::collections::HashMap<String, AnalyticsTag> = tags
            .iter()
            .map(|t| (t.id.clone(), tag_to_analytics(t)))
            .collect();

        let mut grouped: std::collections::HashMap<String, Vec<AnalyticsTag>> =
            std::collections::HashMap::new();
        for (resource_id, tag_id) in pairs {
            if let Some(tag) = by_id.get(&tag_id) {
                grouped.entry(resource_id).or_default().push(tag.clone());
            }
        }
        for values in grouped.values_mut() {
            values.sort_by(|a, b| a.name.cmp(&b.name));
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

/// 项目库存量行映射（仅迁移读取 `analytics_tags` 用；列序即 `SELECT` 的列序）。
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
