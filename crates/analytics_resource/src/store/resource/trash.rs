//! 软删 / 复活 / 永久删除 / 孤儿删行。

use super::*;

/// 需要随资源行一起清掉的关联表（关联列都是 `resource_id`）。
///
/// 2026-11-12 补齐：旧版只清 `folder` / `tags`，漏了 `versions`（**带外键**，
/// 项目库 `foreign_keys=ON` 时会把 `DELETE FROM analytics_resources` 直接拒掉，
/// 表现为「删不掉」）、`recycle_bin`，以及 027–029 的断言 / 效力 / 谱系三张表。
/// `events` 刻意不在此列：履历要留痕（撤销归档本身就是可追溯事件），
/// 只在「清空回收站」时按保留期清理已无资源行的老事件。
const LINKED_TABLES: [&str; 6] = [
    "analytics_resource_folder",
    "analytics_resource_tags",
    "analytics_resource_versions",
    "analytics_recycle_bin",
    "analytics_resource_assertions",
    "analytics_resource_lifecycle",
];

/// 老库 / 未跑全量迁移的测试库可能没有 027–029 的表：跳过而不是让整个删除失败。
fn table_exists(conn: &rusqlite::Connection, name: &str) -> Result<bool, CoreError> {
    match conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![name],
        |_| Ok(()),
    ) {
        Ok(()) => Ok(true),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(error) => Err(persistence_err("table_exists", error)),
    }
}

fn delete_links_for(conn: &rusqlite::Connection, id: &str) -> Result<(), CoreError> {
    for table in LINKED_TABLES {
        if !table_exists(conn, table)? {
            continue;
        }
        conn.execute(
            &format!("DELETE FROM {table} WHERE resource_id = ?1"),
            rusqlite::params![id],
        )
        .map_err(|e| persistence_err("delete_links", e))?;
    }
    delete_refs_for(conn, "from_id = ?1 OR to_id = ?1", rusqlite::params![id])?;
    Ok(())
}

fn delete_links_for_deleted(conn: &rusqlite::Connection) -> Result<(), CoreError> {
    let deleted = "resource_id IN (SELECT id FROM analytics_resources WHERE deleted_at IS NOT NULL)";
    for table in LINKED_TABLES {
        if !table_exists(conn, table)? {
            continue;
        }
        conn.execute(&format!("DELETE FROM {table} WHERE {deleted}"), [])
            .map_err(|e| persistence_err("delete_links", e))?;
    }
    delete_refs_for(
        conn,
        "from_id IN (SELECT id FROM analytics_resources WHERE deleted_at IS NOT NULL) \
         OR to_id IN (SELECT id FROM analytics_resources WHERE deleted_at IS NOT NULL)",
        [],
    )?;
    Ok(())
}

/// 谱系表有两个方向的资源 id，单独清。
fn delete_refs_for<P: rusqlite::Params>(
    conn: &rusqlite::Connection,
    predicate: &str,
    params: P,
) -> Result<(), CoreError> {
    if !table_exists(conn, "analytics_resource_refs")? {
        return Ok(());
    }
    conn.execute(
        &format!("DELETE FROM analytics_resource_refs WHERE {predicate}"),
        params,
    )
    .map_err(|e| persistence_err("delete_refs", e))?;
    Ok(())
}

/// 履历保留期：只在用户主动「清空回收站」时收掉 **已无资源行且超过 365 天** 的孤儿事件。
///
/// 不按时间删存活资源的事件；`datetime()` 解析不了的历史值会得 NULL、条件为假，
/// 宁可留着也不误删。
fn purge_expired_orphan_events(conn: &rusqlite::Connection) -> Result<(), CoreError> {
    if !table_exists(conn, "analytics_resource_events")? {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM analytics_resource_events \
         WHERE datetime(occurred_at) < datetime('now', '-365 days') \
           AND resource_id NOT IN (SELECT id FROM analytics_resources)",
        [],
    )
    .map_err(|e| persistence_err("purge_events", e))?;
    Ok(())
}

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
            delete_links_for_deleted(inner)?;
            let count = inner
                .execute(
                    "DELETE FROM analytics_resources WHERE deleted_at IS NOT NULL",
                    [],
                )
                .map_err(|e| persistence_err("delete", e))?;
            // 清空回收站是用户主动动作：顺手收掉过期孤儿履历，不让 027 无外键的
            // 留痕策略变成只增不减的垃圾堆积。
            purge_expired_orphan_events(inner)?;
            Ok(count)
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
    /// **关联先清、同一事务**：`analytics_resource_folder` / `analytics_resource_tags`
    /// / `analytics_resource_versions` 是 `resource_id` 上的外键且没有 `ON DELETE CASCADE`，
    /// 而项目库开了 `foreign_keys=ON`——不先清关联，`DELETE FROM analytics_resources`
    /// 会直接被外键拒掉（表现为“删不掉”）。完整表清单见 [`LINKED_TABLES`]；
    /// 履历事件刻意保留，只在「清空回收站」时按保留期清理孤儿。
    async fn delete_row_with_links(&self, id: &str, in_trash: bool) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<(), CoreError> {
            delete_links_for(inner, id)?;
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
