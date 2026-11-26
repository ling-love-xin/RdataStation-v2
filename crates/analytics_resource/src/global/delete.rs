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

use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::payload;
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

/// 回收站 manifest（表型与文件本体共用同一结构；没有 payload 的旧条目靠 `serde(default)` 兼容）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GlobalTrashManifest {
    kind: String,
    name: String,
    asset_id: String,
    version: i32,
    analytics_table: Option<String>,
    /// 文件本体相对路径（`resources/<rel>`）；表型旧条目没有这一项。
    #[serde(default)]
    file_rel_path: Option<String>,
    /// 回收站内的 payload 根（相对 `trash_dir`）；文件本体 / 历史版本移入时写。
    #[serde(default)]
    payload_dir: Option<String>,
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

/// 全局回收站中的一个条目（宿主列表 / 对话框用，不暴露 manifest 文件结构）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalTrashEntry {
    /// 资产名（恢复 / 永久删除都按它找最新 manifest）。
    pub name: String,
    pub version: i32,
    pub analytics_table: Option<String>,
    pub file_rel_path: Option<String>,
    pub trashed_at: chrono::DateTime<Utc>,
    /// 同名链里进回收站时的版本数。
    pub version_count: usize,
    /// 条目目录（`trash/global/<asset_id>/<timestamp>`），恢复后不存在。
    pub trash_dir: PathBuf,
    /// 是否带文件本体 payload（表型旧条目为 false）。
    pub has_file_payload: bool,
}

impl GlobalInstanceStore {
    /// 列出全局回收站条目（按移入时间倒序）。
    ///
    /// 同一条资产多次移入只列最新一次（名称是恢复 / 永久删除的键，旧条目留着也没法单独操作）；
    /// 文件本体是否在回收站由 `payload_dir` 判断，不再去查 `resources/`（那里已经没有它）。
    pub fn list_global_trash(&self) -> Result<Vec<GlobalTrashEntry>, CoreError> {
        let root = self.global_trash_root();
        let mut latest: std::collections::HashMap<String, GlobalTrashEntry> =
            std::collections::HashMap::new();
        let Ok(asset_dirs) = std::fs::read_dir(&root) else {
            return Ok(Vec::new());
        };
        for asset_dir in asset_dirs.flatten() {
            let asset_path = asset_dir.path();
            if !asset_path.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&asset_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let dir = entry.path();
                let manifest_path = dir.join(MANIFEST_FILE);
                let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                    continue;
                };
                let Ok(manifest) = serde_json::from_str::<GlobalTrashManifest>(&text) else {
                    continue;
                };
                if manifest.kind != TRASH_KIND_ANALYSIS {
                    continue;
                }
                let Ok(trashed_at) = chrono::DateTime::parse_from_rfc3339(&manifest.trashed_at)
                else {
                    continue;
                };
                let item = GlobalTrashEntry {
                    name: manifest.name.clone(),
                    version: manifest.version,
                    analytics_table: manifest.analytics_table,
                    file_rel_path: manifest.file_rel_path,
                    trashed_at: trashed_at.with_timezone(&Utc),
                    version_count: manifest.versions.len(),
                    trash_dir: dir,
                    has_file_payload: manifest.payload_dir.is_some(),
                };
                match latest.get(&item.name) {
                    Some(existing) if existing.trashed_at >= item.trashed_at => {}
                    _ => {
                        latest.insert(item.name.clone(), item);
                    }
                }
            }
        }
        let mut items: Vec<_> = latest.into_values().collect();
        items.sort_by(|a, b| b.trashed_at.cmp(&a.trashed_at));
        Ok(items)
    }

    /// 清空全局回收站：逐条永久删除；仍被项目 pin 的条目跳过并计入 `skipped`。
    ///
    /// 不提供“整目录删除”的捷径：每条都要走 `purge_global` 的 pin 守卫与三表事务，
    /// 否则会把仍被引用的登记一起抹掉。
    pub async fn empty_global_trash(&self) -> Result<(usize, usize), InstanceOpError> {
        let mut purged = 0usize;
        let mut skipped = 0usize;
        for entry in self.list_global_trash()? {
            match self.purge_global(&entry.name).await {
                Ok(_) => purged += 1,
                Err(InstanceOpError::Referenced { .. }) => skipped += 1,
                Err(InstanceOpError::NotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok((purged, skipped))
    }

    fn global_trash_root(&self) -> PathBuf {
        // 回收站根与全局库根同源（`RDS_HOME/data/trash/global`）。测试注入的 sqlite
        // 路径不保证在 `data/system` 下，按目录推会推错，所以仍走统一的 `paths`。
        trash_root()
    }
}

impl GlobalInstanceStore {
    /// 移入全局回收站：活跃 pin 检查 → 只软删当前活跃行 → 写 manifest。
    ///
    /// 历史版本行本来就是覆盖升级留下的软删行，保持原状；全局表一张都不 `DROP`。
    pub async fn trash_global(
        &self,
        name: &str,
    ) -> Result<GlobalTrashOutcome, InstanceOpError> {
        let rows: Vec<(
            String,
            i32,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        )> = self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, version, analytics_table, deleted_at, file_rel_path, kind \
                         FROM analytics_resources \
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
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
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
        let active: Vec<_> = rows
            .iter()
            .filter(|(_, _, _, deleted, _, _)| deleted.is_none())
            .collect();
        if active.is_empty() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!("全局资产「{name}」已经在回收站")),
            )));
        }
        // 文件型资产不能直接删除/进回收站：先解锁退回项目草稿箱，再从草稿箱删除。
        if active[0].5 == "file" && active[0].4.is_some() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::InvalidArgument {
                    param: "name".to_string(),
                    reason: format!(
                        "全局「{name}」是文件型资产，不能直接删除：请先执行「解除锁定并退回草稿箱」"
                    ),
                },
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

        // 文件本体与历史副本移入回收站：表型没有 payload，这一步自然跳过。
        let current_file = current.4.clone();
        let mut payload_dir = None;
        if let Some(rel) = current_file.as_deref() {
            let source = payload::resources_dir(&self.system_dir).join(rel);
            if source.is_file() {
                let payload_root = trash_dir.join("payload");
                let dest = payload_root.join("resources").join(rel);
                move_path(&source, &dest).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "移动全局文件本体到回收站失败（{}）：{e}",
                        source.display()
                    ))))
                })?;
                payload_dir = Some("payload".to_string());
            }
        }
        let key = payload::asset_key(name);
        let versions_src = payload::asset_versions_dir(&self.system_dir, &key);
        if versions_src.is_dir() {
            let payload_root = trash_dir.join("payload");
            let dest = payload_root.join("versions").join(&key);
            move_path(&versions_src, &dest).map_err(|e| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "移动全局文件历史副本到回收站失败（{}）：{e}",
                    versions_src.display()
                    ))))
            })?;
            payload_dir = Some("payload".to_string());
        }

        let manifest = GlobalTrashManifest {
            kind: TRASH_KIND_ANALYSIS.to_string(),
            name: name.to_string(),
            asset_id: current.0.clone(),
            version: current.1,
            analytics_table: current.2.clone(),
            file_rel_path: current_file,
            payload_dir,
            trashed_at: now.clone(),
            versions: rows
                .iter()
                .map(|(id, version, table, deleted, _, _)| TrashVersion {
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
        // 表型：先校验当前表还在；文件本体：先把 payload 移回原位。
        if let Some(table) = manifest
            .analytics_table
            .as_deref()
            .filter(|table| !table.is_empty())
        {
            if !self.table_exists(table).await? {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::General(format!(
                        "全局「{name}」v{} 的表（{table}）已经不在了：请从回收站永久删除这条登记",
                        manifest.version
                    )),
                )));
            }
        } else if manifest.file_rel_path.is_none() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局回收站条目「{name}」既没有分析表也没有文件本体，无法恢复"
                )),
            )));
        }

        if manifest.payload_dir.is_some() {
            let payload_root = manifest_dir.join("payload");
            if let Some(rel) = manifest.file_rel_path.as_deref() {
                let source = payload_root.join("resources").join(rel);
                if source.is_file() {
                    let dest = payload::resources_dir(&self.system_dir).join(rel);
                    if dest.exists() {
                        return Err(InstanceOpError::Core(CoreError::common(
                            CommonError::General(format!(
                                "恢复全局「{name}」失败：本体目标已存在（{}）",
                                dest.display()
                            )),
                        )));
                    }
                    move_path(&source, &dest).map_err(|e| {
                        InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                            "恢复全局文件本体失败（{}）：{e}",
                            dest.display()
                        ))))
                    })?;
                }
            }
            let key = payload::asset_key(name);
            let source = payload_root.join("versions").join(&key);
            if source.is_dir() {
                let dest = payload::versions_dir(&self.system_dir).join(&key);
                if dest.exists() {
                    return Err(InstanceOpError::Core(CoreError::common(
                        CommonError::General(format!(
                            "恢复全局「{name}」失败：历史版本目标已存在（{}）",
                            dest.display()
                        )),
                    )));
                }
                move_path(&source, &dest).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "恢复全局文件历史副本失败（{}）：{e}",
                        dest.display()
                    ))))
                })?;
            }
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

        // 4. 文件本体与历史副本：永久删除时一并清掉（回收站里的 payload 由第 5 步清）。
        let file_rel: Option<String> = self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT file_rel_path FROM analytics_resources \
                 WHERE name = ?1 AND scope = 'global' AND file_rel_path IS NOT NULL \
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![name],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_err("read_purge_file"))
        })?;
        if let Some(rel) = file_rel {
            let path = payload::resources_dir(&self.system_dir).join(&rel);
            if path.is_file() {
                std::fs::remove_file(&path).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "永久删除全局文件本体失败（{}）：{e}",
                        path.display()
                    ))))
                })?;
            }
            let versions = payload::versions_dir(&self.system_dir).join(payload::asset_key(name));
            if versions.is_dir() {
                std::fs::remove_dir_all(&versions).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "永久删除全局文件历史副本失败（{}）：{e}",
                        versions.display()
                    ))))
                })?;
            }
        }

        // 5. 事务内清三张表：资源行（整条同名链）、版本快照、分发回执。
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
}

fn trash_root() -> PathBuf {
    paths::data_dir().join("trash").join("global")
}

/// 同卷 `rename`；跨卷退回复制 + 删源。目录与文件都走这一条。
fn move_path(source: &Path, dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(source, dest) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            if source.is_dir() {
                copy_dir(source, dest)?;
                std::fs::remove_dir_all(source)?;
                Ok(())
            } else {
                std::fs::copy(source, dest).map_err(|copy_error| {
                    std::io::Error::new(
                        copy_error.kind(),
                        format!("{rename_error}; {copy_error}"),
                    )
                })?;
                std::fs::remove_file(source)?;
                Ok(())
            }
        }
    }
}

fn copy_dir(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
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
