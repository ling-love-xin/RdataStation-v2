//! 全局文件资产解锁：把当前内容退回项目草稿箱，再撤销全局登记。
//!
//! 产品口径（2026-11-26）：全局**文件型**资产不能直接删除/进回收站。要删先解锁退回
//! 草稿箱，再从草稿箱删除、走唯一的项目回收站；表型全局资产仍走逻辑删除 / 永久删除。

use super::*;

/// 全局文件解锁结果（回执文案用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalUnlockOutcome {
    pub name: String,
    pub version: i32,
    /// 退回目标项目草稿箱后的相对路径（相对 `{project}/scratchpad/`，统一 `/`）。
    pub draft_rel: String,
    /// 随解锁清掉的历史副本份数。
    pub removed_versions: usize,
}

impl GlobalInstanceStore {
    /// 把一条**文件型**全局资产解锁退回当前项目草稿箱。
    ///
    /// 前置：无项目引用（pin）。顺序：先复制当前全局本体到草稿箱 → `purge_global`
    /// 清全局表 / 文件 / 历史副本 / 登记链；清失败时把刚复制的草稿删掉，避免“两边都在”。
    pub async fn unlock_global_file_to_project(
        &self,
        name: &str,
        drafts_root: &Path,
    ) -> Result<GlobalUnlockOutcome, InstanceOpError> {
        let active: Option<(i32, Option<String>, String)> = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT version, file_rel_path, kind
                FROM analytics_resources
                WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                ORDER BY version DESC LIMIT 1
                "#,
                rusqlite::params![name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(sqlite_err("read_unlock_global_active"))
        })?;
        let Some((version, Some(rel), kind)) = active else {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        };
        if kind != "file" {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::InvalidArgument {
                    param: "name".to_string(),
                    reason: format!(
                        "全局「{name}」不是文件型资产（表型请用逻辑删除 / 永久删除）"
                    ),
                },
            )));
        }
        let refs = self.active_ref_count(name).map_err(InstanceOpError::Core)?;
        if refs > 0 {
            return Err(InstanceOpError::Referenced {
                name: name.to_string(),
                refs,
            });
        }
        let source = payload::resources_dir(&self.system_dir).join(&rel);
        if !source.is_file() {
            return Err(InstanceOpError::Core(CoreError::common(CommonError::General(
                format!("全局「{name}」的文件本体不在（resources/{rel}），先走索引修复"),
            ))));
        }

        let draft_rel = crate::service::unlock::free_draft_rel(drafts_root, &rel);
        let dest = drafts_root.join(&draft_rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "创建草稿箱目录失败（{}）：{e}",
                    parent.display()
                ))))
            })?;
        }
        std::fs::copy(&source, &dest).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "复制全局文件到草稿箱失败（{}）：{e}",
                dest.display()
            ))))
        })?;
        // 副本继承只读属性时清掉，草稿要是可编辑的。
        if let Ok(meta) = std::fs::metadata(&dest) {
            let mut perms = meta.permissions();
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&dest, perms);
        }

        let removed_versions = payload::asset_versions_dir(
            &self.system_dir,
            &payload::asset_key(name),
        )
        .read_dir()
        .map(|dir| dir.filter(|entry| entry.is_ok()).count())
        .unwrap_or(0);

        if let Err(error) = self.purge_global(name).await {
            let _ = std::fs::remove_file(&dest);
            return Err(error);
        }

        Ok(GlobalUnlockOutcome {
            name: name.to_string(),
            version,
            draft_rel: draft_rel.replace('\\', "/"),
            removed_versions,
        })
    }
}
