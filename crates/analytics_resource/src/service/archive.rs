//! 归档入口与新建 / 并入既有两条路径。

use super::*;
use crate::model::*;

impl ArchiveService {
    /// 归档：首次归档（`existing_resource_id = None`）或再归档（取回后改完再归档）。
    pub async fn archive(&self, req: ArchiveRequest) -> Result<ArchiveOutcome, CoreError> {
        // 指纹在搬运前算：失败即中止，不动任何状态（架构 §6.3 第 1/3 步）。
        if !req.source_path.is_file() {
            return Err(service_err("archive", "源文件不存在或不是普通文件"));
        }
        // 指纹口径按种类分（P4.1 / R4）：
        // - 分析表：**定义 + 结构摘要**（行数不进指纹）——宿主采集好随请求带入；
        // - 文件 / 引用型：本体字节的 sha256。
        // 两者共用同一列（`content_hash`），所以“内容是否变”的判定对两类都是“比这一列”。
        let analysis = req.analysis.clone();
        let content_hash = match analysis.as_ref() {
            Some(facts) => facts.fingerprint(),
            None => self.payload.content_hash(&req.source_path).await?,
        };
        // 体积也在搬运前读：本体随后会被 move 进 `resources/`，读的是同一个文件。
        let file_size = self.payload.file_size(&req.source_path).await?;

        match req.existing_resource_id.clone() {
            None => self.archive_new(req, content_hash, file_size).await,
            Some(resource_id) => {
                self.archive_into_existing(
                    &resource_id,
                    req,
                    content_hash,
                    file_size,
                    analysis.as_ref(),
                )
                .await
            }
        }
    }

    /// 首次归档：copy 模式本体复制 + 写索引行；reference 模式只登记元数据（本体留原位）。
    ///
    /// 冲突检查两模式同一条（`find_archive_by_rel_path`）：reference 的 `file_rel_path`
    /// 存源绝对路径，唯一索引天然防"同一源被归档两次"（一个源一份引用）。
    async fn archive_new(
        &self,
        req: ArchiveRequest,
        content_hash: String,
        file_size: Option<i64>,
    ) -> Result<ArchiveOutcome, CoreError> {
        if let Some(existing) = self.store.find_archive_by_rel_path(&req.rel_path).await? {
            return Err(service_err(
                "archive",
                &format!(
                    "目标路径已被存档「{}」（id={}）占用；请改名或先移除旧存档",
                    existing.name, existing.id
                ),
            ));
        }

        let mode = req.payload_mode;
        // reference 模式的"登记路径"：源绝对路径（本体留原位）。
        // copy 模式：`resources/` 相对路径（搬运前定好，失败即中止）。
        let registered_rel = match mode {
            PayloadMode::Copy => req.rel_path.clone(),
            PayloadMode::Reference => req.source_path.to_string_lossy().replace('\\', "/"),
        };

        if mode == PayloadMode::Copy {
            self.payload
                .copy_in(&req.source_path, &req.rel_path)
                .await?;
        }

        let input = NewArchiveInput {
            resource_type: default_resource_type(req.kind),
            name: req.name.clone(),
            alias: req.alias.clone(),
            kind: req.kind,
            content_hash: content_hash.clone(),
            // 分析表：定义与规模同批登记（其余型为 None，不编造）。
            definition_sql: req.analysis.as_ref().and_then(|f| f.definition_sql.clone()),
            row_count: req.analysis.as_ref().and_then(|f| {
                f.row_count
                    .map(|rows| rows.clamp(0, i32::MAX as i64) as i32)
            }),
            column_count: req.analysis.as_ref().map(|f| f.column_count()),
            file_rel_path: registered_rel,
            payload_mode: mode,
            // reference 模式记录源绝对路径；copy 模式为 None。
            source_path: (mode == PayloadMode::Reference)
                .then(|| req.source_path.to_string_lossy().replace('\\', "/")),
            file_size,
            binding: req.binding.clone(),
            // 作用域为派生只读量：数据住项目库 → project（架构 §4.3）。
            scope: "project".to_string(),
        };

        let resource = match self.store.insert_archive(input).await {
            Ok(resource) => resource,
            Err(error) => {
                // 回滚本体（copy 模式）：宁可回到"文件还在草稿箱"，也不要"副本留了、记录没有"。
                // reference 模式没搬任何东西，无需回滚。
                if mode == PayloadMode::Copy {
                    if let Err(rollback) = self.payload.remove_payload(&req.rel_path).await {
                        tracing::error!(
                            error = %rollback,
                            rel = %req.rel_path,
                            "归档回滚失败：副本留在 resources/，需经索引修复处理"
                        );
                    }
                }
                return Err(error);
            }
        };

        let notes = self
            .apply_labels(&resource.id, &req.tags, req.group_id.as_deref())
            .await;
        self.emit(ChangeReason::Archived, Some(resource.id.clone()));

        Ok(ArchiveOutcome {
            resource_id: resource.id,
            version: resource.version,
            content_hash,
            file_rel_path: req.rel_path,
            created_new_version: true,
            notes,
        })
    }

    /// 再归档：指纹未变即幂等返回；变了才"旧内容留副本 → 写前快照 → 覆盖本体 → 索引 +1"。
    ///
    /// 顺序取舍：版本副本与快照行先落，再覆盖本体。任一步失败都不会丢旧内容
    /// （最坏是留下一份多余的副本/快照行，不产生错误的历史结论）。
    async fn archive_into_existing(
        &self,
        resource_id: &str,
        req: ArchiveRequest,
        content_hash: String,
        file_size: Option<i64>,
        analysis: Option<&crate::analysis::AnalysisFacts>,
    ) -> Result<ArchiveOutcome, CoreError> {
        let current = self.store.get_resource_by_id(resource_id).await?;
        if current.deleted_at.is_some() {
            return Err(service_err("re_archive", "存档已删除，无法再归档"));
        }
        let mode = PayloadMode::from_db_str(&current.payload_mode);
        let rel_path = current
            .file_rel_path
            .clone()
            .ok_or_else(|| service_err("re_archive", "该存档没有本体路径（非文件型或旧行）"))?;

        let current_hash = current.content_hash.clone().unwrap_or_default();
        if current_hash == content_hash {
            // 幂等：内容与当前版本相同 → 不覆盖本体、不增版本，保留调用方的工作副本。
            // 附属项照旧落：指纹没变不代表用户这次填的标签 / 分组不算数。
            let notes = self
                .apply_labels(&current.id, &req.tags, req.group_id.as_deref())
                .await;
            return Ok(ArchiveOutcome {
                resource_id: current.id,
                version: current.version,
                content_hash,
                file_rel_path: rel_path,
                created_new_version: false,
                notes,
            });
        }

        // 旧内容留副本：copy 从 `resources/` 本体复制；reference 从源绝对路径复制。
        // reference 模式的源在外部（可能正是草稿箱工作副本），本体不归我们覆盖。
        let copy_result = match mode {
            PayloadMode::Copy => {
                self.payload
                    .store_version_copy(resource_id, current.version, &rel_path)
                    .await?
            }
            PayloadMode::Reference => {
                let source = self.payload_path_for(&current)?;
                self.payload
                    .store_version_copy_from(resource_id, current.version, &source)
                    .await?
            }
        };
        if copy_result.is_none() {
            tracing::warn!(
                resource_id,
                version = current.version,
                "再归档时旧内容副本未落（本体缺失？）"
            );
        }

        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        let snapshot_id = self
            .store
            .save_resource_version(resource_id, current.version, &snapshot)
            .await?;

        // copy：用工作区源覆盖 `resources/` 副本（源保留——它是工作副本）。
        // reference：不覆盖源（本体在外部，M6 不动它），只更新指纹 / 规模 / 版本。
        if mode == PayloadMode::Copy {
            self.payload
                .replace_payload(&req.source_path, &rel_path)
                .await?;
        }

        let updated = self
            .store
            .update_archive_content(
                resource_id,
                &content_hash,
                &snapshot_id,
                file_size,
                analysis,
            )
            .await?;

        self.prune_copies(resource_id, req.keep_versions).await;

        let notes = self
            .apply_labels(&updated.id, &req.tags, req.group_id.as_deref())
            .await;
        self.emit(ChangeReason::Updated, Some(resource_id.to_string()));

        Ok(ArchiveOutcome {
            resource_id: updated.id,
            version: updated.version,
            content_hash,
            file_rel_path: rel_path,
            created_new_version: true,
            notes,
        })
    }

}
