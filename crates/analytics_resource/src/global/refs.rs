//! 全局引用登记：项目引用版本 → 全局版本；以及项目引用资产的建/追加与固化。
//!
//! 保留与固化都按版本（用户 2026-11-25 锁定）：B 的每个引用版本单独一行，
//! 只有 `released_at IS NULL` 的登记才阻止全局裁剪/删除。pin/release 只做登记账本；
//! `reference_to_project` 写项目引用行但不复制数据；`solidify_ref_version` 才复制数据。

use super::*;
use crate::store::AnalyticsResourceStore;
use chrono::Utc;
use std::path::Path;

/// 一条项目引用登记（项目库里的引用资产版本 → 全局名 + 版本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRefPin {
    pub project_root: String,
    pub project_resource_id: String,
    pub project_version: i32,
    pub global_name: String,
    pub global_version: i32,
}

/// 项目里新建/更新一条全局引用资产的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRefOutcome {
    pub resource_id: String,
    pub name: String,
    pub alias: Option<String>,
    pub version: i32,
    pub global_name: String,
    pub global_version: i32,
}

impl GlobalInstanceStore {
    /// 把全局表引用到项目（不落数据）：项目侧写一条 `table_ref` 资产，
    /// `origin_ref` 指全局行 id + 版本；同一个引用资产按版本追加。
    ///
    /// v1 只支持表型全局资产：`file_rel_path` 非空的脚本文件拒绝引用。
    pub async fn reference_to_project(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        global_name: &str,
        global_version: i32,
        alias: Option<&str>,
    ) -> Result<GlobalRefOutcome, InstanceOpError> {
        let row = self
            .read_global_row(global_name, Some(global_version))?
            .ok_or_else(|| InstanceOpError::NotFound {
                name: global_name.to_string(),
            })?;
        let (global_id, version, _kind, hash, rel_path, _definition, rows, cols, _size, table) =
            row;
        if rel_path.is_some() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General("当前只支持引用表型全局资产；文件型引用后置".to_string()),
            )));
        }
        let table = table.unwrap_or_else(|| global_name.to_string());
        if !self.table_exists(&table).await? {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{global_name}」v{version} 的内容表（{table}）不存在，无法引用"
                )),
            )));
        }

        let origin_ref = crate::model::global_ref(&global_id, version);
        let alias = alias.map(str::trim).filter(|value| !value.is_empty());
        let pool = project.1.clone();
        let conn = pool.acquire().await.map_err(InstanceOpError::Core)?;
        let inner = conn.inner().map_err(InstanceOpError::Core)?;
        let now = Utc::now().to_rfc3339();

        // 同一个全局名只维护一条引用资产；已有则追加版本，不另起同名引用。
        let existing: Option<String> = inner
            .query_row(
                "SELECT id FROM analytics_resources \
                 WHERE scope = 'project' AND kind = 'table_ref' AND origin_kind = 'global' \
                   AND name = ?1 AND deleted_at IS NULL \
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![global_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| InstanceOpError::Core(sqlite_err("find_project_ref")(e)))?;

        if let Some(id) = existing {
            drop(conn);
            self.append_ref_version(
                project,
                &id,
                alias,
                &origin_ref,
                hash.as_deref(),
                rows,
                cols,
                &now,
            )
            .await?;
            let store = AnalyticsResourceStore::new(project.1.clone());
            let current = store
                .get_resource_by_id(&id)
                .await
                .map_err(InstanceOpError::Core)?;
            self.pin_global_ref(&GlobalRefPin {
                project_root: normalize_root(project_root),
                project_resource_id: id.clone(),
                project_version: current.version,
                global_name: global_name.to_string(),
                global_version: version,
            })
            .map_err(InstanceOpError::Core)?;
            return Ok(GlobalRefOutcome {
                resource_id: id,
                name: global_name.to_string(),
                alias: current.alias,
                version: current.version,
                global_name: global_name.to_string(),
                global_version: version,
            });
        }

        let id = format!("arp_ref_{}", uuid::Uuid::new_v4().simple());
        inner
            .execute(
                r#"
                INSERT INTO analytics_resources (
                    id, resource_type, name, alias, config, scope, row_count, column_count,
                    file_size, version, parent_version_id, parent_resource_id, source_query,
                    created_at, updated_at, created_by, deleted_at,
                    kind, content_hash, file_rel_path, readonly, promoted_from,
                    source_connection_id, source_table, definition_sql, archived_at,
                    payload_mode, source_path, origin_kind, origin_ref, analytics_table,
                    structured
                ) VALUES (
                    ?1, 'table', ?2, ?3, 'null', 'project', ?4, ?5, NULL,
                    1, NULL, NULL, NULL, ?6, ?6, NULL, NULL,
                    'table_ref', ?7, NULL, 1, NULL,
                    NULL, NULL, NULL, ?6,
                    'reference', NULL, 'global', ?8, NULL, NULL
                )
                "#,
                rusqlite::params![
                    &id,
                    global_name,
                    alias,
                    rows,
                    cols,
                    &now,
                    hash.as_deref(),
                    &origin_ref
                ],
            )
            .map_err(|e| InstanceOpError::Core(sqlite_err("insert_project_ref")(e)))?;
        drop(conn);

        self.pin_global_ref(&GlobalRefPin {
            project_root: normalize_root(project_root),
            project_resource_id: id.clone(),
            project_version: 1,
            global_name: global_name.to_string(),
            global_version: version,
        })
        .map_err(InstanceOpError::Core)?;

        Ok(GlobalRefOutcome {
            resource_id: id,
            name: global_name.to_string(),
            alias: alias.map(ToString::to_string),
            version: 1,
            global_name: global_name.to_string(),
            global_version: version,
        })
    }

    /// 固化一个项目引用版本：复制指定全局版本到项目本地表，并释放该版本 pin。
    ///
    /// 原引用资产保留；新本地资产由 `distribute_version` 正常登记（独立版本链）。
    pub async fn solidify_ref_version(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        project_resource_id: &str,
        project_version: i32,
        target: &str,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        if target.trim().is_empty() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General("固化目标名不能为空".to_string()),
            )));
        }
        let (global_name, global_version, alias) = self
            .read_project_ref_pin(project, project_resource_id, project_version)
            .await?;
        let folder = self
            .read_project_ref_folder(project, project_resource_id)
            .await?;
        let tags = self
            .read_project_ref_tags(project, project_resource_id)
            .await?;
        let outcome = self
            .distribute_version(project, project_root, &global_name, global_version, target)
            .await?;

        // 固化出来的独立本地资产复制引用资产的别名/分组/标签；原引用仍在。
        let store = AnalyticsResourceStore::new(project.1.clone());
        if let Some(alias) = alias.as_deref() {
            store
                .set_alias(&outcome.resource_id, Some(alias))
                .await
                .map_err(InstanceOpError::Core)?;
        }
        if let Some(folder) = folder.as_deref() {
            store
                .add_resource_to_folder(&outcome.resource_id, folder)
                .await
                .map_err(InstanceOpError::Core)?;
        }
        for tag in &tags {
            store
                .add_tag_to_resource(&outcome.resource_id, tag)
                .await
                .map_err(InstanceOpError::Core)?;
        }

        self.release_global_ref(project_root, project_resource_id, project_version)
            .map_err(InstanceOpError::Core)?;
        Ok(outcome)
    }

    async fn append_ref_version(
        &self,
        project: &ProjectDuckConn,
        id: &str,
        alias: Option<&str>,
        origin_ref: &str,
        hash: Option<&str>,
        rows: Option<i32>,
        cols: Option<i32>,
        now: &str,
    ) -> Result<(), InstanceOpError> {
        let store = AnalyticsResourceStore::new(project.1.clone());
        let current = store
            .get_resource_by_id(id)
            .await
            .map_err(InstanceOpError::Core)?;
        let snapshot = serde_json::to_string(&current).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "序列化引用版本快照失败：{e}"
            ))))
        })?;
        store
            .save_resource_version(id, current.version, &snapshot)
            .await
            .map_err(InstanceOpError::Core)?;

        let conn = project.1.acquire().await.map_err(InstanceOpError::Core)?;
        let inner = conn.inner().map_err(InstanceOpError::Core)?;
        let affected = inner
            .execute(
                "UPDATE analytics_resources \
                 SET alias = ?2, origin_ref = ?3, content_hash = ?4, row_count = ?5, \
                     column_count = ?6, version = version + 1, updated_at = ?7 \
                 WHERE id = ?1 AND deleted_at IS NULL",
                rusqlite::params![id, alias, origin_ref, hash, rows, cols, now],
            )
            .map_err(|e| InstanceOpError::Core(sqlite_err("append_project_ref")(e)))?;
        if affected == 0 {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General("引用资产不存在或已删除，无法追加版本".to_string()),
            )));
        }
        Ok(())
    }

    async fn read_project_ref_pin(
        &self,
        project: &ProjectDuckConn,
        id: &str,
        version: i32,
    ) -> Result<(String, i32, Option<String>), InstanceOpError> {
        let conn = project.1.acquire().await.map_err(InstanceOpError::Core)?;
        let inner = conn.inner().map_err(InstanceOpError::Core)?;
        let (name, current_version, current_alias): (String, i32, Option<String>) = inner
            .query_row(
                "SELECT name, version, alias FROM analytics_resources \
                 WHERE id = ?1 AND scope = 'project' AND kind = 'table_ref' \
                   AND origin_kind = 'global' AND deleted_at IS NULL",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|e| InstanceOpError::Core(sqlite_err("read_project_ref")(e)))?
            .ok_or_else(|| InstanceOpError::NotFound {
                name: id.to_string(),
            })?;

        let (origin_ref, alias): (String, Option<String>) = if current_version == version {
            inner
                .query_row(
                    "SELECT origin_ref, alias FROM analytics_resources WHERE id = ?1",
                    rusqlite::params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| InstanceOpError::Core(sqlite_err("read_project_ref_origin")(e)))?
        } else {
            let snapshot: String = inner
                .query_row(
                    "SELECT snapshot FROM analytics_resource_versions \
                     WHERE resource_id = ?1 AND version = ?2",
                    rusqlite::params![id, version],
                    |row| row.get(0),
                )
                .map_err(|e| InstanceOpError::Core(sqlite_err("read_project_ref_snapshot")(e)))?;
            let value: serde_json::Value = serde_json::from_str(&snapshot).map_err(|e| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "解析引用版本快照失败：{e}"
                ))))
            })?;
            let origin = value
                .get("origin_ref")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(
                        "引用版本快照缺少 origin_ref".to_string(),
                    )))
                })?
                .to_string();
            let alias = value
                .get("alias")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            (origin, alias)
        };
        let value: serde_json::Value = serde_json::from_str(&origin_ref).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "解析引用 origin_ref 失败：{e}"
            ))))
        })?;
        let global_version = value
            .get("version")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(
                    "引用 origin_ref 缺少 version".to_string(),
                )))
            })? as i32;
        Ok((name, global_version, alias.or(current_alias)))
    }

    async fn read_project_ref_folder(
        &self,
        project: &ProjectDuckConn,
        id: &str,
    ) -> Result<Option<String>, InstanceOpError> {
        let conn = project.1.acquire().await.map_err(InstanceOpError::Core)?;
        let inner = conn.inner().map_err(InstanceOpError::Core)?;
        inner
            .query_row(
                "SELECT folder_id FROM analytics_resource_folder WHERE resource_id = ?1 LIMIT 1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| InstanceOpError::Core(sqlite_err("read_project_ref_folder")(e)))
    }

    async fn read_project_ref_tags(
        &self,
        project: &ProjectDuckConn,
        id: &str,
    ) -> Result<Vec<String>, InstanceOpError> {
        let conn = project.1.acquire().await.map_err(InstanceOpError::Core)?;
        let inner = conn.inner().map_err(InstanceOpError::Core)?;
        let mut stmt = inner
            .prepare("SELECT tag_id FROM analytics_resource_tags WHERE resource_id = ?1")
            .map_err(|e| InstanceOpError::Core(sqlite_err("prepare_project_ref_tags")(e)))?;
        let rows = stmt
            .query_map(rusqlite::params![id], |row| row.get::<_, String>(0))
            .map_err(|e| InstanceOpError::Core(sqlite_err("query_project_ref_tags")(e)))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| InstanceOpError::Core(sqlite_err("collect_project_ref_tags")(e)))
    }

    /// 登记/更新一个项目引用版本 pin。
    ///
    /// 同一 `(project_root, project_resource_id, project_version)` 重复 pin 时
    /// 覆盖全局版本并重新激活（等价于项目里把这一版改指到另一个全局版本）。
    pub fn pin_global_ref(&self, pin: &GlobalRefPin) -> Result<(), CoreError> {
        self.with_sqlite(|conn| {
            conn.execute(
                r#"
                INSERT INTO analytics_global_refs
                    (id, project_root, project_resource_id, project_version,
                     global_name, global_version, created_at, released_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
                ON CONFLICT(project_root, project_resource_id, project_version)
                DO UPDATE SET
                    global_name = excluded.global_name,
                    global_version = excluded.global_version,
                    released_at = NULL
                "#,
                rusqlite::params![
                    format!("agr_{}", uuid::Uuid::new_v4().simple()),
                    &pin.project_root,
                    &pin.project_resource_id,
                    pin.project_version,
                    &pin.global_name,
                    pin.global_version,
                    Utc::now().to_rfc3339(),
                ],
            )
            .map(|_| ())
            .map_err(sqlite_err("pin_global_ref"))
        })
    }

    /// 释放一个项目引用版本的 pin（固化选中版本 / 删除引用版本时调用）。
    pub fn release_global_ref(
        &self,
        project_root: &Path,
        project_resource_id: &str,
        project_version: i32,
    ) -> Result<usize, CoreError> {
        let root = normalize_root(project_root);
        self.with_sqlite(|conn| {
            conn.execute(
                "UPDATE analytics_global_refs SET released_at = ?1 \
                 WHERE project_root = ?2 AND project_resource_id = ?3 \
                   AND project_version = ?4 AND released_at IS NULL",
                rusqlite::params![
                    Utc::now().to_rfc3339(),
                    root,
                    project_resource_id,
                    project_version
                ],
            )
            .map_err(sqlite_err("release_global_ref"))
        })
    }

    /// 释放某个引用资产的全部 pin（删除引用资产时调用）。
    pub fn release_global_refs_for_resource(
        &self,
        project_root: &Path,
        project_resource_id: &str,
    ) -> Result<usize, CoreError> {
        let root = normalize_root(project_root);
        self.with_sqlite(|conn| {
            conn.execute(
                "UPDATE analytics_global_refs SET released_at = ?1 \
                 WHERE project_root = ?2 AND project_resource_id = ?3 \
                   AND released_at IS NULL",
                rusqlite::params![Utc::now().to_rfc3339(), root, project_resource_id],
            )
            .map_err(sqlite_err("release_global_refs_for_resource"))
        })
    }

    /// 项目被删除/回收时释放该项目全部 pin（双库无共同事务，按补偿清理）。
    pub fn release_global_refs_for_project(&self, project_root: &Path) -> Result<usize, CoreError> {
        let root = normalize_root(project_root);
        self.with_sqlite(|conn| {
            conn.execute(
                "UPDATE analytics_global_refs SET released_at = ?1 \
                 WHERE project_root = ?2 AND released_at IS NULL",
                rusqlite::params![Utc::now().to_rfc3339(), root],
            )
            .map_err(sqlite_err("release_global_refs_for_project"))
        })
    }

    /// 某个全局名还有多少活跃引用版本。
    pub fn active_ref_count(&self, global_name: &str) -> Result<usize, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM analytics_global_refs \
                 WHERE global_name = ?1 AND released_at IS NULL",
                rusqlite::params![global_name],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count.max(0) as usize)
            .map_err(sqlite_err("active_ref_count"))
        })
    }

    /// 某个全局版本的活跃引用数。
    pub fn active_ref_count_for_version(
        &self,
        global_name: &str,
        global_version: i32,
    ) -> Result<usize, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM analytics_global_refs \
                 WHERE global_name = ?1 AND global_version = ?2 AND released_at IS NULL",
                rusqlite::params![global_name, global_version],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count.max(0) as usize)
            .map_err(sqlite_err("active_ref_count_for_version"))
        })
    }

    /// 某个全局名下被 pin 住的版本集合（裁剪时跳过这些版本）。
    pub fn active_ref_versions(&self, global_name: &str) -> Result<Vec<i32>, CoreError> {
        self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT global_version FROM analytics_global_refs \
                     WHERE global_name = ?1 AND released_at IS NULL \
                     ORDER BY global_version",
                )
                .map_err(sqlite_err("prepare_active_ref_versions"))?;
            let rows = stmt
                .query_map(rusqlite::params![global_name], |row| row.get::<_, i32>(0))
                .map_err(sqlite_err("query_active_ref_versions"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_active_ref_versions"))
        })
    }
}

fn normalize_root(root: &Path) -> String {
    root.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_root_uses_forward_slashes() {
        assert_eq!(normalize_root(Path::new(r"D:\RDS\proj")), "D:/RDS/proj");
    }
}
