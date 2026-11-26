//! 文件资产解锁：撤销归档并把当前内容退回草稿箱。
//!
//! 产品口径（2026-11-26）：**资产库的文件不能直接删除**。要删先解锁退回草稿箱，
//! 再从草稿箱删除、走唯一的项目回收站。解锁只退**当前版本**，历史副本与版本快照
//! 随解锁一起清掉（确认框已写明）；表型资产不走这里，仍是“逻辑删除 / 永久删除”。

use std::path::{Component, Path, PathBuf};

use super::*;

/// 解锁结果（回执文案用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlockFileOutcome {
    pub resource_id: String,
    pub name: String,
    /// 退回草稿箱后的相对路径（相对 `{project}/scratchpad/`，统一 `/`）。
    pub draft_rel: String,
    /// 随解锁清掉的历史副本份数。
    pub removed_versions: usize,
}

impl ArchiveService {
    /// 把一条**文件型**存档解锁退回草稿箱。
    ///
    /// 顺序：先复制当前本体到草稿箱（不先动资产库）→ 清登记链 → 删资产库本体与历史副本。
    /// 登记链清失败时把刚复制出来的草稿删掉，宁可没解锁，也不要“草稿多一份、资产还在”。
    pub async fn unlock_file_to_drafts(
        &self,
        resource_id: &str,
    ) -> Result<UnlockFileOutcome, CoreError> {
        let resource = self.store.get_resource_by_id(resource_id).await?;
        if resource.deleted_at.is_some() {
            return Err(service_err(
                "unlock",
                &format!("「{}」在回收站里，不能解锁退回草稿箱", resource.name),
            ));
        }
        if ArchiveKind::resolve(&resource.kind, resource.analytics_table.as_deref())
            != ArchiveKind::File
        {
            return Err(service_err(
                "unlock",
                &format!(
                    "「{}」不是文件型资产（表型请用逻辑删除 / 永久删除）",
                    resource.name
                ),
            ));
        }
        let rel = resource.file_rel_path.clone().ok_or_else(|| {
            service_err(
                "unlock",
                &format!("「{}」没有文件本体路径，不能解锁", resource.name),
            )
        })?;
        let source = self.payload.resolve(&rel)?;
        if !source.is_file() {
            return Err(service_err(
                "unlock",
                &format!(
                    "「{}」的本体不在（resources/{rel}）：先走索引修复处理",
                    resource.name
                ),
            ));
        }

        let drafts_root = self.payload.project_root().join("scratchpad");
        let draft_rel = free_draft_rel(&drafts_root, &rel);
        let dest = drafts_root.join(&draft_rel);
        self.payload.copy_out(&rel, &dest).await?;

        if let Err(error) = self
            .store
            .purge_name_chain(&resource.name, "project")
            .await
        {
            // 登记链没清掉 = 资产仍然存在；把刚复制出来的草稿删掉，别留两份。
            let _ = tokio::fs::remove_file(&dest).await;
            return Err(error);
        }

        // 登记已经没了，剩下的是清理资产库自己的副本。失败只记日志：本体留在 resources/
        // 只是可被索引修复重新发现的孤儿，不会让用户丢内容。
        if let Err(error) = self.payload.remove_payload(&rel).await {
            tracing::warn!(error = %error, rel = %rel, "解锁后退回草稿箱：清理资产库本体失败");
        }
        let removed_versions = self
            .payload
            .version_copies(resource_id)
            .await
            .map(|versions| versions.len())
            .unwrap_or(0);
        if let Err(error) = self
            .payload
            .remove_all_version_copies(resource_id)
            .await
        {
            tracing::warn!(error = %error, resource_id = %resource_id, "解锁后退回草稿箱：清理历史副本失败");
        }

        self.emit(ChangeReason::Undone, Some(resource.id.clone()));
        Ok(UnlockFileOutcome {
            resource_id: resource.id,
            name: resource.name,
            draft_rel: draft_rel.replace('\\', "/"),
            removed_versions,
        })
    }
}

/// 草稿箱里找一个不冲突的落点：保留原名 / 原相对目录，撞名加 `-2` / `-3`。
pub(crate) fn free_draft_rel(root: &Path, rel: &str) -> String {
    let rel = Path::new(rel);
    if rel
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return format!(
            "{}.unlocked",
            rel.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("asset")
        );
    }
    let taken = |candidate: &Path| root.join(candidate).exists();
    if !taken(rel) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    let parent = rel.parent().filter(|dir| !dir.as_os_str().is_empty());
    let stem = rel
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("asset");
    let ext = rel.extension().and_then(|ext| ext.to_str());
    for index in 2..1000 {
        let name = match ext {
            Some(ext) => format!("{stem}-{index}.{ext}"),
            None => format!("{stem}-{index}"),
        };
        let candidate: PathBuf = match parent {
            Some(dir) => dir.join(name),
            None => PathBuf::from(name),
        };
        if !taken(&candidate) {
            return candidate.to_string_lossy().replace('\\', "/");
        }
    }
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_draft_rel_keeps_dirs_and_avoids_taken_names() {
        let dir = std::env::temp_dir().join(format!("rds-unlock-rel-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("reports"));
        std::fs::write(dir.join("reports/q.sql"), b"x").unwrap();
        assert_eq!(free_draft_rel(&dir, "reports/q.sql"), "reports/q-2.sql");
        assert_eq!(free_draft_rel(&dir, "reports/new.sql"), "reports/new.sql");
        assert_eq!(free_draft_rel(&dir, "../escape.sql"), "escape.sql.unlocked");
        let _ = std::fs::remove_dir_all(dir);
    }
}
