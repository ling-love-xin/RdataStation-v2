//! 全局引用登记：项目引用版本 → 全局版本。
//!
//! 保留与固化都按版本（用户 2026-11-25 锁定）：B 的每个引用版本单独一行，
//! 只有 `released_at IS NULL` 的登记才阻止全局裁剪/删除。本模块只做登记账本，
//! 不复制数据、不碰项目资源行。

use super::*;
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

impl GlobalInstanceStore {
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
