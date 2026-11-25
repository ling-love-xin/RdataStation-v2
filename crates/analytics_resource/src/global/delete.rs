//! 删除全局实例资产：表 → 登记 → 版本快照 → 分发回执。
//!
//! 口径（用户 2026-11-25 裁定）：删的是**全局这一份**，已经分发到各项目的副本
//! **不受影响**——副本早已落成项目自己的表 / 文件，与全局资产不再有数据依赖。
//! 所以这里只清 `scope = 'global'` 的行、全局 DuckDB 里该资产各版本的 `res_*` 表、
//! 版本快照与分发回执；项目侧一行不动。
//!
//! 顺序：**先删表、后删登记**。表删一半失败时登记还在，重跑一次即可（`DROP ... IF EXISTS`
//! 幂等）；反过来先删登记会留下"无主的全局表"，谁也找不到它们。删除登记放在一个事务里，
//! 三张表（资源 / 版本 / 分发回执）要么都清、要么都不清。

use super::{GlobalInstanceStore, InstanceOpError, sqlite_err};

/// 一次全局资产删除的结果（回执文案用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDeleteOutcome {
    /// 资产名（全局侧同名链的名字）。
    pub name: String,
    /// 删掉的版本数（含已软删的历史行）。
    pub versions: usize,
    /// 删掉的全局表数（版本裁剪后可能少于版本数）。
    pub tables: usize,
    /// 曾分发到过几个项目（回执里说"这些副本还在"）。
    pub projects: usize,
}

impl GlobalInstanceStore {
    /// 删除一个全局资产（按资产名，连同它的整条同名版本链）。
    pub async fn delete_global(&self, name: &str) -> Result<GlobalDeleteOutcome, InstanceOpError> {
        // 1. 同名链全部版本行（**含软删**）：覆盖升级会软删旧登记，但旧表还在库里。
        let rows: Vec<(String, Option<String>)> = self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, analytics_table FROM analytics_resources \
                     WHERE name = ?1 AND scope = 'global'",
                )
                .map_err(sqlite_err("prepare_delete_global"))?;
            let rows = stmt
                .query_map(rusqlite::params![name], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                })
                .map_err(sqlite_err("query_delete_global"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_delete_global"))
        })?;
        if rows.is_empty() {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        }

        // 2. 曾分发到几个项目（在删回执之前数，否则永远是 0）。
        let projects: i64 = self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT COUNT(DISTINCT project_root) FROM analytics_resource_dispatch \
                 WHERE global_name = ?1",
                rusqlite::params![name],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err("count_delete_global_projects"))
        })?;

        // 3. 逐张删全局表。失败即中止：登记还留着，用户可以原样重试。
        let mut tables = 0usize;
        for (_, table) in &rows {
            if let Some(table) = table.as_deref().filter(|t| !t.is_empty()) {
                self.drop_global_table(table).await?;
                tables += 1;
            }
        }

        // 4. 事务内清三张表：资源行（整条同名链）、版本快照、分发回执。
        let versions = rows.len();
        self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_delete_global"))?;
            for (id, _) in &rows {
                tx.execute(
                    "DELETE FROM analytics_resource_versions WHERE resource_id = ?1",
                    rusqlite::params![id],
                )
                .map_err(sqlite_err("delete_global_versions"))?;
            }
            tx.execute(
                "DELETE FROM analytics_resources WHERE name = ?1 AND scope = 'global'",
                rusqlite::params![name],
            )
            .map_err(sqlite_err("delete_global_resources"))?;
            tx.execute(
                "DELETE FROM analytics_resource_dispatch WHERE global_name = ?1",
                rusqlite::params![name],
            )
            .map_err(sqlite_err("delete_global_dispatch"))?;
            tx.commit().map_err(sqlite_err("commit_delete_global"))
        })?;

        Ok(GlobalDeleteOutcome {
            name: name.to_string(),
            versions,
            tables,
            projects: projects.max(0) as usize,
        })
    }
}
