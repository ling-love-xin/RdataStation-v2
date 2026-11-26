//! 全局实例资产删除：移入回收站 / 恢复 / 永久删除。
//!
//! 口径（用户 2026-11-25 裁定）：删的是**全局这一份**，已经分发到各项目的副本
//! **不受影响**——副本早已落成项目自己的表 / 文件，与全局资产不再有数据依赖。
//!
//! 表型资产的回收站与项目表型同构：
//! - 移入 = 只软删**当前活跃行**，全局表与版本快照都留库；
//! - 恢复 = 先校验当前表还在，再复活登记；
//! - 永久删除 = 逐张 `DROP` 同名链各版表，再清版本快照 / 登记 / 分发回执。
//!
//! 回收站条目落在 `<RDS_HOME>/data/trash/global/<asset_id>/<timestamp>/manifest.json`，
//! 不走 Windows 回收站。`delete_global` 暂时保留旧的硬删除语义（兼容现有 UI 调用），
//! 新入口应走 [`GlobalInstanceStore::trash_global`] / [`GlobalInstanceStore::purge_global`]。

use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::{CommonError, CoreError, GlobalInstanceStore, InstanceOpError, sqlite_err};

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

/// 一次「移入全局回收站」的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalTrashOutcome {
    pub name: String,
    pub version: i32,
    pub analytics_table: Option<String>,
    /// 回收站条目目录（manifest 所在）。
    pub trash_dir: PathBuf,
}

/// 一次「从全局回收站恢复」的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRestoreOutcome {
    pub name: String,
    pub version: i32,
    pub analytics_table: Option<String>,
}

/// 回收站 manifest（表型当前只写 `kind = "analysis"`；文件资产后续复用同一结构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GlobalTrashManifest {
    kind: String,
    name: String,
    asset_id: String,
    version: i32,
    analytics_table: Option<String>,
    trashed_at: String,
    versions: Vec<TrashVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrashVersion {
    resource_id: String,
    version: i32,
    analytics_table: Option<String>,
    deleted_at: Option<String>,
}

const TRASH_KIND_ANALYSIS: &str = "analysis";
const MANIFEST_FILE: &str = "manifest.json";

impl GlobalInstanceStore {
    /// 移入全局回收站：活跃 pin 检查 → 只软删当前活跃行 → 写 manifest。
    ///
    /// 历史版本行本来就是覆盖升级留下的软删行，保持原状；全局表一张都不 `DROP`。
    pub async fn trash_global(
        &self,
        name: &str,
    ) -> Result<GlobalTrashOutcome, InstanceOpError> {
        let rows: Vec<(String, i32, Option<String>, Option<String>)> = self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, version, analytics_table, deleted_at FROM analytics_resources \
                     WHERE name = ?1 AND scope = 'global' ORDER BY version DESC",
                )
                .map_err(sqlite_err("prepare_trash_global"))?;
            let rows = stmt
                .query_map(rusqlite::params![name], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i32>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })
                .map_err(sqlite_err("query_trash_global"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_trash_global"))
        })?;
        if rows.is_empty() {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        }
        let active: Vec<_> = rows.iter().filter(|(_, _, _, deleted)| deleted.is_none()).collect();
        if active.is_empty() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!("全局资产「{name}」已经在回收站")),
            )));
        }

        // 活跃 pin 不允许移动：先让用户固化或解除引用，避免回收站里的版本被项目引用。
        let refs = self.active_ref_count(name).map_err(InstanceOpError::Core)?;
        if refs > 0 {
            return Err(InstanceOpError::Referenced {
                name: name.to_string(),
                refs,
            });
        }

        let now = Utc::now().to_rfc3339();
        let current = active[0];
        let trash_dir = trash_root().join(safe_dir_name(&current.0)).join(trash_stamp());
        std::fs::create_dir_all(&trash_dir).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "创建全局回收站目录失败（{}）：{e}",
                trash_dir.display()
            ))))
        })?;

        let manifest = GlobalTrashManifest {
            kind: TRASH_KIND_ANALYSIS.to_string(),
            name: name.to_string(),
            asset_id: current.0.clone(),
            version: current.1,
            analytics_table: current.2.clone(),
            trashed_at: now.clone(),
            versions: rows
                .iter()
                .map(|(id, version, table, deleted)| TrashVersion {
                    resource_id: id.clone(),
                    version: *version,
                    analytics_table: table.clone(),
                    deleted_at: deleted.clone(),
                })
                .collect(),
        };
        let manifest_path = trash_dir.join(MANIFEST_FILE);
        let payload = serde_json::to_string_pretty(&manifest).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "序列化全局回收站 manifest 失败：{e}"
            ))))
        })?;
        std::fs::write(&manifest_path, payload).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "写全局回收站 manifest 失败（{}）：{e}",
                manifest_path.display()
            ))))
        })?;

        // 只软删当前活跃行：历史版本行的 `deleted_at` 是版本链的一部分，不能改。
        let active_ids: Vec<String> = active.iter().map(|row| row.0.clone()).collect();
        self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_trash_global"))?;
            for id in &active_ids {
                tx.execute(
                    "UPDATE analytics_resources SET deleted_at = ?1 \
                     WHERE id = ?2 AND scope = 'global' AND deleted_at IS NULL",
                    rusqlite::params![&now, id],
                )
                .map_err(sqlite_err("soft_delete_trash_global"))?;
            }
            tx.commit().map_err(sqlite_err("commit_trash_global"))
        })?;

        Ok(GlobalTrashOutcome {
            name: name.to_string(),
            version: current.1,
            analytics_table: current.2.clone(),
            trash_dir,
        })
    }

    /// 从全局回收站恢复：先校验当前表还在，再复活当前活跃行。
    pub async fn restore_global(
        &self,
        name: &str,
    ) -> Result<GlobalRestoreOutcome, InstanceOpError> {
        let (manifest, manifest_dir) = find_latest_manifest(name).ok_or_else(|| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "全局回收站中没有「{name}」"
            ))))
        })?;
        let Some(table) = manifest.analytics_table.as_deref().filter(|t| !t.is_empty()) else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局回收站条目「{name}」没有记录分析表，无法自动恢复"
                )),
            )));
        };
        if !self.table_exists(table).await? {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{} 的表（{table}）已经不在了：请从回收站永久删除这条登记",
                    manifest.version
                )),
            )));
        }

        let restored = self.with_sqlite(|conn| {
            conn.execute(
                "UPDATE analytics_resources SET deleted_at = NULL \
                 WHERE id = ?1 AND scope = 'global'",
                rusqlite::params![&manifest.asset_id],
            )
            .map_err(sqlite_err("restore_global_resource"))
        })?;
        if restored == 0 {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局回收站条目「{name}」对应的登记行已不存在，无法恢复"
                )),
            )));
        }
        // 恢复成功，回收站条目即失效；清理失败只告警，登记已经复活。
        if let Err(error) = std::fs::remove_dir_all(&manifest_dir) {
            tracing::warn!(error = %error, dir = %manifest_dir.display(), "清理已恢复的全局回收站条目失败");
        }

        Ok(GlobalRestoreOutcome {
            name: name.to_string(),
            version: manifest.version,
            analytics_table: manifest.analytics_table,
        })
    }

    /// 永久删除一个全局资产：`DROP` 同名链各版表 → 清版本快照 / 登记 / 分发回执。
    ///
    /// 顺序：**先删表、后删登记**。表删一半失败时登记还在，重跑一次即可（`DROP ... IF EXISTS`
    /// 幂等）；反过来先删登记会留下"无主的全局表"，谁也找不到它们。删除登记放在一个事务里，
    /// 三张表（资源 / 版本 / 分发回执）要么都清、要么都不清。
    pub async fn purge_global(&self, name: &str) -> Result<GlobalDeleteOutcome, InstanceOpError> {
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

        // 引用守卫：按版本 pin 的全局表不能整条删除。先让用户固化或解除引用，
        // 而不是删完登记后让项目侧引用变成悬空指针。
        let refs = self.active_ref_count(name).map_err(InstanceOpError::Core)?;
        if refs > 0 {
            return Err(InstanceOpError::Referenced {
                name: name.to_string(),
                refs,
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

        // 5. 回收站条目随永久删除清掉；找不到也视为已清。
        purge_trash_entries(name);

        Ok(GlobalDeleteOutcome {
            name: name.to_string(),
            versions,
            tables,
            projects: projects.max(0) as usize,
        })
    }

    /// 旧入口：当前直接走永久删除，保持既有 UI 行为；新调用应改用
    /// [`Self::trash_global`] / [`Self::purge_global`]。
    pub async fn delete_global(&self, name: &str) -> Result<GlobalDeleteOutcome, InstanceOpError> {
        self.purge_global(name).await
    }
}

fn trash_root() -> PathBuf {
    paths::data_dir().join("trash").join("global")
}

fn trash_stamp() -> String {
    Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string()
}

fn safe_dir_name(asset_id: &str) -> String {
    asset_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect()
}

fn find_latest_manifest(name: &str) -> Option<(GlobalTrashManifest, PathBuf)> {
    let root = trash_root();
    let mut best: Option<(GlobalTrashManifest, PathBuf)> = None;
    for asset_dir in std::fs::read_dir(&root).ok()? {
        let asset_dir = asset_dir.ok()?.path();
        if !asset_dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&asset_dir).ok()? {
            let entry = entry.ok()?.path();
            let manifest_path = entry.join(MANIFEST_FILE);
            if !manifest_path.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<GlobalTrashManifest>(&text) else {
                continue;
            };
            if manifest.kind != TRASH_KIND_ANALYSIS || manifest.name != name {
                continue;
            }
            if best
                .as_ref()
                .map(|(current, _)| manifest.trashed_at > current.trashed_at)
                .unwrap_or(true)
            {
                best = Some((manifest, entry));
            }
        }
    }
    best
}

fn purge_trash_entries(name: &str) {
    let root = trash_root();
    let Ok(assets) = std::fs::read_dir(&root) else {
        return;
    };
    for asset_dir in assets.flatten() {
        let asset_dir = asset_dir.path();
        if !asset_dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&asset_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let entry = entry.path();
            let manifest_path = entry.join(MANIFEST_FILE);
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<GlobalTrashManifest>(&text) else {
                continue;
            };
            if manifest.name == name {
                if let Err(error) = std::fs::remove_dir_all(&entry) {
                    tracing::warn!(error = %error, dir = %entry.display(), "清理全局回收站条目失败");
                }
            }
        }
        // 资产目录空了就顺手删掉，避免回收站里堆空壳。
        if std::fs::read_dir(&asset_dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(&asset_dir);
        }
    }
}
