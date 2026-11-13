//! 回收站：移入、还原、清除与撤销归档。

use super::*;
use crate::model::*;

impl ArchiveService {
    /// 把一个或多个存档移入项目级回收站（原型 §4.7：`Delete` / 右键 / 后续的危险区）。
    ///
    /// 顺序与归档同构：**先本体、后索引**（本体 move 进 `.RSmeta/trash` → 登记行软删）；
    /// 索引失败把本体**从回收站搬回原位**。软删而不是删行：还原要恢复完整记录（别名 /
    /// 指纹 / 来源 / 标签），回收站 manifest 里没有这些——让行留在表里、只是不再出现在
    /// 任何查询里（所有列表都过滤 `deleted_at IS NULL`），比在磁盘上再存一份 JSON 快照
    /// 少一处可失同步的状态。
    ///
    /// 本体缺失的行没有可移的东西：拒绝并指向索引修复（它才是“本体不在”的正规出口）。
    ///
    /// 不做预回滚：中途失败时已移的条目**留在回收站里**（已成功的部分不回退）——
    /// 部分成功与部分回退相比，前者至少状态可读（哪些进了回收站就是哪些）。
    pub async fn move_to_trash(
        &self,
        resource_ids: &[String],
    ) -> Result<Vec<TrashArchiveEntry>, CoreError> {
        let mut out = Vec::with_capacity(resource_ids.len());
        for id in resource_ids {
            match self.trash_one(id).await {
                Ok(entry) => {
                    self.emit(ChangeReason::Trashed, Some(entry.resource_id.clone()));
                    out.push(entry);
                }
                // 第一条就失败：什么都没动，原样报。
                Err(error) if out.is_empty() => return Err(error),
                // 中途失败：已进回收站的**不回退**（见上文），但必须把“已移了几条”说清楚，
                // 否则用户只看到一句失败，列表却已经变了。
                Err(error) => {
                    return Err(service_err(
                        "trash",
                        &format!("{error}；本批已移入 {} 项，其余未处理", out.len()),
                    ));
                }
            }
        }
        Ok(out)
    }

    /// 单条移入回收站（批量的单元；守卫与顺序见 `move_to_trash`）。
    async fn trash_one(&self, id: &str) -> Result<TrashArchiveEntry, CoreError> {
        let resource = self.store.get_resource_by_id(id).await?;
        if resource.deleted_at.is_some() {
            return Err(service_err(
                "trash",
                &format!("「{}」已经在回收站里", resource.name),
            ));
        }
        let mode = PayloadMode::from_db_str(&resource.payload_mode);

        // reference 型：本体在源位置（用户工作区 / 外部路径），M6 不能未经同意移动用户文件；
        // 移入回收站 = 只软删登记行（血缘标签随登记行留着，还原时一并复活）。
        // 还原 = 复活登记行；永久删除 = 仅删登记行 + 版本元数据（§12.1.1 ③）。
        if mode == PayloadMode::Reference {
            self.store.soft_delete_archive(&resource.id).await?;
            return Ok(TrashArchiveEntry {
                resource_id: resource.id.clone(),
                name: resource.name,
                trash_id: format!("{TRASH_ID_REF_PREFIX}{}", resource.id),
            });
        }

        let rel_path = resource.file_rel_path.clone().ok_or_else(|| {
            service_err(
                "trash",
                &format!("「{}」没有本体路径，不能移入回收站", resource.name),
            )
        })?;
        let path = self.payload.resolve(&rel_path)?;
        if !path.is_file() {
            return Err(service_err(
                "trash",
                &format!(
                    "「{}」的本体不在（resources/{rel_path}）：先走索引修复处理",
                    resource.name
                ),
            ));
        }

        let entry = self
            .payload
            .trash()
            .move_to_trash(&path, ORIGIN_RESOURCES, &rel_path)
            .await?;
        if let Err(error) = self.store.soft_delete_archive(&resource.id).await {
            // 回滚本体：宁可回到“文件在原位、记录也在”，也不要“文件没了、记录还在列表里”。
            if let Err(rollback) = self
                .payload
                .trash()
                .restore(&entry.manifest.id, &self.payload.resources_dir())
                .await
            {
                tracing::error!(
                    error = %rollback,
                    rel = %rel_path,
                    trash_id = %entry.manifest.id,
                    "回收站回滚失败：本体留在回收站而记录仍是存活态，需人工处理"
                );
            }
            return Err(error);
        }

        Ok(TrashArchiveEntry {
            resource_id: resource.id,
            name: resource.name,
            trash_id: entry.manifest.id,
        })
    }

    /// 从项目级回收站还原一条存档（原型 §4.7 的还原）。
    ///
    /// 三条守卫都不静默降级：**只认本模块的条目**（`origin` 校验——跨模块还原会拿到
    /// 不属于自己的本体）、**登记行必须还在**（软删态；行被索引修复清掉过就只能走补登）、
    /// **同名不覆盖**（回收站层自动避让，并报告改名后的路径——登记行跟着改）。
    pub async fn restore_archive_from_trash(
        &self,
        trash_id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        // reference 条目：没有实体 manifest，本体一直在源位置，还原只是复活登记行。
        if let Some(resource_id) = trash_id.strip_prefix(TRASH_ID_REF_PREFIX) {
            let record = self.store.get_resource_by_id(resource_id).await?;
            if record.deleted_at.is_none() {
                return Err(service_err(
                    "untrash",
                    &format!("「{}」不在回收站里", record.name),
                ));
            }
            self.store.undelete_archive(resource_id, None).await?;
            self.emit(ChangeReason::Untrashed, Some(record.id.clone()));
            return self.store.get_resource_by_id(resource_id).await;
        }

        let trash = self.payload.trash();
        let entry = trash.get(trash_id).await?;
        if entry.manifest.origin != ORIGIN_RESOURCES {
            return Err(service_err(
                "untrash",
                &format!(
                    "该条目属于模块「{}」，请在它的界面里还原",
                    entry.manifest.origin
                ),
            ));
        }
        let Some(record) = self
            .store
            .find_deleted_archive_by_rel_path(&entry.manifest.original_rel_path)
            .await?
        else {
            return Err(service_err(
                "untrash",
                &format!(
                    "找不到「{}」的登记记录（可能已被索引修复清掉）：请用「重建索引」补登",
                    entry.manifest.name
                ),
            ));
        };

        let restored = trash
            .restore(trash_id, &self.payload.resources_dir())
            .await?;
        // 避让改过名时，登记行的本体路径要跟着改，否则索引与本体又对不上。
        let new_rel = restored
            .path
            .strip_prefix(self.payload.resources_dir())
            .ok()
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .filter(|rel| rel != &entry.manifest.original_rel_path);
        if let Err(error) = self
            .store
            .undelete_archive(&record.id, new_rel.as_deref())
            .await
        {
            // 回滚：把本体再移回收站（条目 id 变了，但宁可多一条可读的回收站条目，
            // 也不要“本体回了原位、记录却还在回收站里”的错位）。
            if let Err(rollback) = trash
                .move_to_trash(
                    &restored.path,
                    ORIGIN_RESOURCES,
                    &entry.manifest.original_rel_path,
                )
                .await
            {
                tracing::error!(
                    error = %rollback,
                    path = %restored.path.display(),
                    "还原回滚失败：本体已回原位而登记行仍是软删态，需人工处理"
                );
            }
            return Err(error);
        }

        self.emit(ChangeReason::Untrashed, Some(record.id.clone()));
        self.store.get_resource_by_id(&record.id).await
    }

    /// 由**原相对路径**还原（索引修复的「从回收站还原」走这条：那一组行只有登记记录，
    /// 手上没有回收站条目 id）。
    ///
    /// 找不到条目时**不静默降级**：这个存档的本体既不在原位、也不在回收站里——
    /// 剩下的唯一出口是删记录，所以错误直接把用户指过去。
    pub async fn restore_archive_by_rel_path(
        &self,
        rel_path: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let entry = self
            .payload
            .trash()
            .list()
            .await?
            .into_iter()
            .find(|entry| {
                entry.manifest.origin == ORIGIN_RESOURCES
                    && entry.manifest.original_rel_path == rel_path
            });
        let Some(entry) = entry else {
            return Err(service_err(
                "untrash",
                &format!(
                    "回收站里没有 resources/{rel_path} 的条目：本体已不可恢复，只能删掉这条登记记录"
                ),
            ));
        };
        self.restore_archive_from_trash(&entry.manifest.id).await
    }

    /// 永久删除一条回收站条目（本体 + 登记行，**不可恢复**）。
    ///
    /// 顺序：先删本体（不可逆的那一步），再清登记行——登记行没清掉只是一条看不见的幽灵行，
    /// 下次「清空」还会收它；反过来（先清行、本体留下）才是真的收不了场。
    pub async fn purge_archive(&self, trash_id: &str) -> Result<(), CoreError> {
        // reference 条目：本体留在源位置不受影响，永久删除只清登记行 + 标签 / 分组归属
        // + 版本元数据（§12.1.1 ③）；没有实体 manifest 可删。
        if let Some(resource_id) = trash_id.strip_prefix(TRASH_ID_REF_PREFIX) {
            let row = self.store.get_resource_by_id(resource_id).await?;
            if row.deleted_at.is_none() {
                return Err(service_err(
                    "purge",
                    &format!("「{}」不在回收站里", row.name),
                ));
            }
            self.store.purge_deleted_row(resource_id).await?;
            return Ok(());
        }

        let trash = self.payload.trash();
        let entry = trash.get(trash_id).await?;
        if entry.manifest.origin != ORIGIN_RESOURCES {
            return Err(service_err(
                "purge",
                &format!(
                    "该条目属于模块「{}」，请在它的界面里处理（跨模块永久删除不是这里的事）",
                    entry.manifest.origin
                ),
            ));
        }
        trash.purge(trash_id).await?;
        if let Some(row) = self
            .store
            .find_deleted_archive_by_rel_path(&entry.manifest.original_rel_path)
            .await?
        {
            self.store.purge_deleted_row(&row.id).await?;
        }
        Ok(())
    }

    /// 清空**本模块的**回收站（返回删掉的条目数）。
    ///
    /// 不走 `ProjectTrash::empty`（整仓）：回收站是项目级的，把草稿箱的东西一起删掉不是"清空"。
    /// 登记行那边用 `purge_all_deleted` 收尾：它还兼顾“条目被手工删了、行还留着”的幽灵行——
    /// 清空的语义是“资产库的回收站彻底空了”，不是“删掉我在界面上看见的那几条”。
    pub async fn empty_trash(&self) -> Result<usize, CoreError> {
        let trash = self.payload.trash();
        let entries: Vec<_> = trash
            .list()
            .await?
            .into_iter()
            .filter(|entry| entry.manifest.origin == ORIGIN_RESOURCES)
            .collect();
        for entry in &entries {
            trash.purge(&entry.manifest.id).await?;
        }
        self.store.purge_all_deleted().await?;
        Ok(entries.len())
    }

    /// 列出软删的 reference 存档（回收站第二来源；§12.1.1 ③）。
    ///
    /// copy 软删行都有实体 `ProjectTrash` 条目，回收站清单由实体条目承载；reference 没有
    /// 本体可搬，回收站清单要把这些软删登记行一并列出。
    pub async fn deleted_reference_archives(&self) -> Result<Vec<AnalyticsResource>, CoreError> {
        Ok(self
            .store
            .list_deleted_archives()
            .await?
            .into_iter()
            .filter(|r| PayloadMode::from_db_str(&r.payload_mode) == PayloadMode::Reference)
            .collect())
    }

    /// 撤销一次归档（原型 §4.1：归档的立即反悔窗口）。
    ///
    /// **只管刚发生的那一次**，两条都不静默降级：
    ///
    /// - 版本 > 1（即已经再归档过）→ 拒绝：撤销会连带丢掉新内容；
    /// - 顺序与归档同构（本体先行、索引后动、失败回滚）：
    ///   - copy 模式：源文件本来就在原处（归档是复制、草稿保留为工作副本），撤销 = 删掉副本 + 删登记行；
    ///   - reference 模式：没搬过本体，撤销 = 只删登记行（源文件本来就在原位）。
    pub async fn undo_archive(&self, undo: &ArchiveUndo) -> Result<(), CoreError> {
        let resource = self.store.get_resource_by_id(&undo.resource_id).await?;
        if ArchiveKind::from_db_str(&resource.kind) != ArchiveKind::File {
            return Err(service_err("undo", "只有文件型存档支持撤销"));
        }
        let rel_path = resource
            .file_rel_path
            .clone()
            .ok_or_else(|| service_err("undo", "该存档没有登记本体路径，无法撤销"))?;
        if resource.version > 1 {
            return Err(service_err(
                "undo",
                &format!(
                    "已有新版本（v{}），撤销窗口已过；如需回退请走版本历史",
                    resource.version
                ),
            ));
        }
        let mode = PayloadMode::from_db_str(&resource.payload_mode);

        // copy：删掉副本即可（源文件本来就在原处——归档是复制，不是搬走）。
        if mode == PayloadMode::Copy {
            self.payload.remove_payload(&rel_path).await?;
        }
        if let Err(error) = self.store.hard_delete_row(&undo.resource_id).await {
            // 回滚本体（copy 模式）：删行失败 → 把副本放回去（与 archive_new 的回滚同纪律）。
            if mode == PayloadMode::Copy {
                if let Err(rollback) = self
                    .payload
                    .copy_in(undo.source_path.as_path(), &rel_path)
                    .await
                {
                    tracing::error!(
                        error = %rollback,
                        rel = %rel_path,
                        "撤销回滚失败：副本未恢复，需经索引修复处理"
                    );
                }
            }
            return Err(error);
        }

        self.emit(ChangeReason::Undone, Some(undo.resource_id.clone()));
        // 撤销会硬删资源行，但履历必须留下：这条正是「它曾经存在过」的证据。
        self.record_timeline(
            &undo.resource_id,
            Some(resource.version),
            crate::model::TimelineEventKind::Undone,
            Some("撤销归档（登记行已删除）"),
            None,
        )
        .await;
        Ok(())
    }

}
