//! 软删 / 复活 / 永久删除 / 孤儿删行。

use super::*;

impl AnalyticsResourceStore {
    /// 软删：标记 `deleted_at`（移入回收站的索引侧动作）。
    ///
    /// 与 `hard_delete_row` 的分工：**回收站里的行软删**（还原要恢复完整记录：别名 /
    /// 指纹 / 来源 / 标签——回收站 manifest 里没有这些），**撤销归档与索引修复硬删**
    /// （那些场景本就该不留痕）。两者都不作用于已软删的行。
    pub async fn soft_delete_archive(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let now = Utc::now().to_rfc3339();
        let affected = conn
            .inner()?
            .execute(
                "UPDATE analytics_resources SET deleted_at = ?, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NULL",
                rusqlite::params![&now, &now, id],
            )
            .map_err(|e| persistence_err("soft_delete", e))?;
        if affected == 0 {
            return Err(persistence_err("soft_delete", "行不存在或已在回收站里"));
        }
        Ok(())
    }

    /// 复活一行（从回收站还原）：清 `deleted_at`；本体路径因重名避让变过时一并更新。
    pub async fn undelete_archive(
        &self,
        id: &str,
        rel_path: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                "UPDATE analytics_resources \
                 SET deleted_at = NULL, file_rel_path = COALESCE(?, file_rel_path), updated_at = ? \
                 WHERE id = ? AND deleted_at IS NOT NULL",
                rusqlite::params![rel_path, Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| persistence_err("undelete", e))?;
        if affected == 0 {
            return Err(persistence_err("undelete", "行不存在或不在回收站里"));
        }
        Ok(())
    }

    /// 按本体路径找**已软删**的行（回收站还原用：区别于“当前存活的同路径行”）。
    pub async fn find_deleted_archive_by_rel_path(
        &self,
        rel_path: &str,
    ) -> Result<Option<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE file_rel_path = ? AND deleted_at IS NOT NULL LIMIT 1"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;

        match stmt.query_row(rusqlite::params![rel_path], map_resource_row) {
            Ok(resource) => Ok(Some(resource)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(persistence_err("select", e)),
        }
    }

    /// 列出全部**已软删**的登记行（回收站第二来源：reference 型没有实体回收站条目，
    /// 回收站清单要把这些行一并列出；按删除时间倒序，与实体条目排序口径一致）。
    pub async fn list_deleted_archives(&self) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let rows = stmt
            .query_map([], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))
    }

    /// 硬删除一行**存活**登记（不写回收站、不留痕）。
    ///
    /// 两个调用方语义不同但动作相同：**撤销归档**（刚发生的那次，本体已移回）与
    /// **索引修复**（删掉“有记录无本体”的孤儿记录）。软删的行（`deleted_at` 非空）由
    /// [`Self::purge_deleted_row`] 负责——那是“回收站永久删除”。
    pub async fn hard_delete_row(&self, id: &str) -> Result<(), CoreError> {
        self.delete_row_with_links(id, false).await
    }

    /// 永久删除一行**软删**登记（回收站的「永久删除」）：本体由 `ProjectTrash::purge` 删，
    /// 这里清掉登记行与它的标签 / 分组归属——留着只会是一条永远看不见、却还占着标签与
    /// 分组归属的幽灵行（v1 的“还原丢归属”就是这种半个状态）。
    pub async fn purge_deleted_row(&self, id: &str) -> Result<(), CoreError> {
        self.delete_row_with_links(id, true).await
    }

    /// 永久删除**全部**软删登记（回收站的「清空」）：返回删掉的行数。
    ///
    /// 与逐条 `purge_deleted_row` 的差别只在批量的写法（回收站里还有条目而登记行被手工删过
    /// 时，这种“幽灵行”只能靠它收尾）。
    pub async fn purge_all_deleted(&self) -> Result<usize, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<usize, CoreError> {
            for table in ["analytics_resource_folder", "analytics_resource_tags"] {
                inner
                    .execute(
                        &format!(
                            "DELETE FROM {table} WHERE resource_id IN \
                             (SELECT id FROM analytics_resources WHERE deleted_at IS NOT NULL)"
                        ),
                        [],
                    )
                    .map_err(|e| persistence_err("delete", e))?;
            }
            inner
                .execute(
                    "DELETE FROM analytics_resources WHERE deleted_at IS NOT NULL",
                    [],
                )
                .map_err(|e| persistence_err("delete", e))
        })();

        match result {
            Ok(count) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| persistence_err("commit", e))?;
                Ok(count)
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 真要删行的那一步（`in_trash` 决定删软删行还是存活行）。
    ///
    /// **关联先清、同一事务**：`analytics_resource_folder` / `analytics_resource_tags` 是
    /// `resource_id` 上的外键且没有 `ON DELETE CASCADE`，而项目库开了 `foreign_keys=ON`
    /// ——不先清关联，`DELETE FROM analytics_resources` 会直接被外键拒掉（表现为“删不掉”）。
    async fn delete_row_with_links(&self, id: &str, in_trash: bool) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<(), CoreError> {
            for table in ["analytics_resource_folder", "analytics_resource_tags"] {
                inner
                    .execute(
                        &format!("DELETE FROM {table} WHERE resource_id = ?"),
                        rusqlite::params![id],
                    )
                    .map_err(|e| persistence_err("delete", e))?;
            }
            let guard = if in_trash {
                "AND deleted_at IS NOT NULL"
            } else {
                "AND deleted_at IS NULL"
            };
            let affected = inner
                .execute(
                    &format!("DELETE FROM analytics_resources WHERE id = ? {guard}"),
                    rusqlite::params![id],
                )
                .map_err(|e| persistence_err("delete", e))?;
            if affected == 0 {
                return Err(persistence_err(
                    "delete",
                    if in_trash {
                        "记录不存在或不在回收站里"
                    } else {
                        "记录不存在或已删除"
                    },
                ));
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| persistence_err("commit", e))?;
                Ok(())
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub async fn remove_orphan_record(&self, id: &str) -> Result<(), CoreError> {
        self.hard_delete_row(id).await
    }
}
