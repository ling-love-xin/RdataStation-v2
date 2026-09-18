use super::*;
use shared::error::{CoreError, StorageError};
use chrono::Utc;

/// 分组表的错误（与 `tag_err` / `persistence_err` 同形，store 名不同）。
fn folder_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_folders".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 分组行映射（两处查询共用；列序即 `SELECT` 的列序）。
fn map_folder_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalyticsFolder> {
    Ok(AnalyticsFolder {
        id: row.get(0)?,
        name: row.get(1)?,
        scope: row.get(2)?,
        parent_folder_id: row.get(3)?,
        sort_order: row.get(4)?,
        color: row.get(5)?,
        icon: row.get(6)?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(7)?)?,
        updated_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(8)?)?,
        deleted_at: row
            .get(9)
            .ok()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
    })
}

impl AnalyticsResourceStore {
    /// 新建分组。
    ///
    /// 单层分组（架构 D8）：`parent_folder_id` 只是表结构的历史遗留，**一律传 `None`**——
    /// 这里不摆只有一项的选择器，但要拒绝真传进来的父分组（静默忽略会让调用方以为建了子树）。
    pub async fn create_folder(
        &self,
        req: CreateFolderRequest,
    ) -> Result<AnalyticsFolder, CoreError> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(folder_err("insert", "分组名不能为空"));
        }
        if req.parent_folder_id.is_some() {
            return Err(folder_err(
                "insert",
                "分组是单层的（没有子分组）——不能指定父分组",
            ));
        }
        if self.folder_name_taken(name, &req.scope).await? {
            return Err(folder_err("insert", &format!("已经有同名分组「{name}」")));
        }

        let conn = self.get_conn().await?;
        let id = format!("af_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_folders (
                id, name, scope, parent_folder_id, sort_order, color, icon, created_at, updated_at
            ) VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    name,
                    &req.scope,
                    0,
                    req.color,
                    req.icon,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| folder_err("insert", e))?;

        self.get_folder_by_id(&id).await
    }

    /// 名字是否已被（未删的）分组占用。
    async fn folder_name_taken(&self, name: &str, scope: &str) -> Result<bool, CoreError> {
        let conn = self.get_conn().await?;
        let count: i64 = conn
            .inner()?
            .query_row(
                "SELECT COUNT(*) FROM analytics_folders \
                 WHERE name = ? AND scope = ? AND deleted_at IS NULL",
                rusqlite::params![name, scope],
                |row| row.get(0),
            )
            .map_err(|e| folder_err("select", e))?;
        Ok(count > 0)
    }

    /// 改名（只改显示名；`scope` 由分组住哪个库决定，不是可编辑项）。
    ///
    /// 幂等：改成现在这个名字直接返回原行。
    pub async fn rename_folder(&self, id: &str, name: &str) -> Result<AnalyticsFolder, CoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(folder_err("rename", "分组名不能为空"));
        }
        let current = self.get_folder_by_id(id).await?;
        if current.name == name {
            return Ok(current);
        }
        if self.folder_name_taken(name, &current.scope).await? {
            return Err(folder_err("rename", &format!("已经有同名分组「{name}」")));
        }

        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                "UPDATE analytics_folders SET name = ?, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NULL",
                rusqlite::params![name, Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| folder_err("rename", e))?;
        if affected == 0 {
            return Err(folder_err("rename", "分组不存在或已删除"));
        }
        self.get_folder_by_id(id).await
    }

    /// 删除分组：**软删分组行 + 清掉它的成员关联**（返回“回到未分组”的资源数）。
    ///
    /// 两件事同一事务：只软删会让关联留在库里（重建同名分组会把旧成员带回来），
    /// 只清关联又会在列表里留一条空分组。成员不跟着删——分组是组织方式，
    /// 删分组不该删存档（原型 §2.4：成员回到「未分组」）。
    pub async fn delete_folder(&self, id: &str) -> Result<usize, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| folder_err("begin_immediate", e))?;

        let result = (|| -> Result<usize, CoreError> {
            let affected = inner
                .execute(
                    "UPDATE analytics_folders SET deleted_at = ?, updated_at = ? \
                     WHERE id = ? AND deleted_at IS NULL",
                    rusqlite::params![Utc::now().to_rfc3339(), Utc::now().to_rfc3339(), id],
                )
                .map_err(|e| folder_err("delete", e))?;
            if affected == 0 {
                return Err(folder_err("delete", "分组不存在或已删除"));
            }
            let ungrouped = inner
                .execute(
                    "DELETE FROM analytics_resource_folder WHERE folder_id = ?",
                    rusqlite::params![id],
                )
                .map_err(|e| folder_err("delete", e))?;
            Ok(ungrouped)
        })();

        match result {
            Ok(ungrouped) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| folder_err("commit", e))?;
                Ok(ungrouped)
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub async fn get_folder_by_id(&self, id: &str) -> Result<AnalyticsFolder, CoreError> {
        let conn = self.get_conn().await?;

        let mut stmt = conn.inner()?.prepare(
            r#"
            SELECT id, name, scope, parent_folder_id, sort_order, color, icon, created_at, updated_at, deleted_at
            FROM analytics_folders
            WHERE id = ?
            "#,
        ).map_err(|e| folder_err("select", e))?;

        let folder = stmt
            .query_row(rusqlite::params![id], map_folder_row)
            .map_err(|e| folder_err("select", e))?;

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
            .query_map(rusqlite::params_from_iter(params), map_folder_row)
            .map_err(|e| folder_err("select", e))?;

        folders
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| folder_err("select", e))
    }

    /// 单层分组的**移动语义**：一个资源最多属于一个分组（表的主键允许对多，但面板的分区
    /// 渲染会把“同时属于两个分组”的资源画两遍——所以这里先清后插）。
    pub async fn add_resource_to_folder(
        &self,
        resource_id: &str,
        folder_id: &str,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| folder_err("begin_immediate", e))?;

        let result = (|| -> Result<(), CoreError> {
            inner
                .execute(
                    "DELETE FROM analytics_resource_folder WHERE resource_id = ?",
                    rusqlite::params![resource_id],
                )
                .map_err(|e| folder_err("delete", e))?;
            inner
                .execute(
                    "INSERT INTO analytics_resource_folder (resource_id, folder_id, sort_order) \
                     VALUES (?, ?, 0)",
                    rusqlite::params![resource_id, folder_id],
                )
                .map_err(|e| folder_err("insert", e))?;
            Ok(())
        })();

        match result {
            Ok(()) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| folder_err("commit", e))?;
                Ok(())
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 把资源移回「未分组」：清掉它的分组关联（单层 → 全清就是移出）。
    pub async fn clear_resource_folder(&self, resource_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute(
                "DELETE FROM analytics_resource_folder WHERE resource_id = ?",
                rusqlite::params![resource_id],
            )
            .map_err(|e| folder_err("delete", e))?;
        Ok(())
    }

    /// 按资源查分组（`resource_id → folder_id`）。
    ///
    /// 一次查完：面板要给每行分区、详情要给当前分组，逐行查就是 N+1 次往返。
    pub async fn folders_by_resource(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
            SELECT rf.resource_id, rf.folder_id
            FROM analytics_resource_folder rf
            INNER JOIN analytics_folders f ON f.id = rf.folder_id
            INNER JOIN analytics_resources r ON r.id = rf.resource_id
            WHERE f.deleted_at IS NULL AND r.deleted_at IS NULL
            "#,
            )
            .map_err(|e| folder_err("select", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| folder_err("select", e))?;

        let mut grouped = std::collections::HashMap::new();
        for row in rows {
            let (resource_id, folder_id) = row.map_err(|e| folder_err("select", e))?;
            // 极端脏数据（同一资源两条关联）取第一条：单层分组下这是“不存在的状态”。
            grouped.entry(resource_id).or_insert(folder_id);
        }
        Ok(grouped)
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
            .map_err(|e| folder_err("delete", e))?;

        Ok(())
    }
}
