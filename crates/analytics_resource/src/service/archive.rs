//! 归档入口与新建 / 并入既有两条路径。

use super::*;
use crate::model::*;
use std::path::Path;

/// `file_rel_path` 的登记键：**预检、写入、回执三处唯一的口径来源**。
///
/// - `copy`：`resources/` 下的相对路径（搬运前定好，失败即中止）；
/// - `reference`：规范化后的源绝对路径（本体留原位）。
///
/// 2026-11-12 收口：023 迁移后 reference 把源绝对路径同时写进 `file_rel_path`
/// 与 `source_path`，这是为了复用 020 的局部唯一索引拦住「同一外部源被引用两次」。
/// 不改成 NULL 是刻意的——改了要回填存量行、且 `global/distribute` 以「该列非空」
/// 判脚本档，收益只是概念整洁。现在把一字段两义**封装进这一个函数**：
/// 两列恒等，任何新增代码都只经它算键，不再各写一份字符串处理。
fn registration_key(mode: PayloadMode, rel_path: &str, source_path: &Path) -> String {
    match mode {
        PayloadMode::Copy => rel_path.to_string(),
        PayloadMode::Reference => source_path.to_string_lossy().replace('\\', "/"),
    }
}

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
    /// 冲突检查两模式共用 `registration_key` 算出的**实际入库键**：reference 下它是
    /// 规范化的源绝对路径。旧实现按 `req.rel_path`（相对）查重，而库里存的是绝对路径，
    /// 预检永远查不到，重复引用只能撞 020 唯一索引报一句裸 `insert` 错误。
    async fn archive_new(
        &self,
        req: ArchiveRequest,
        content_hash: String,
        file_size: Option<i64>,
    ) -> Result<ArchiveOutcome, CoreError> {
        let mode = req.payload_mode;
        let registered_rel = registration_key(mode, &req.rel_path, &req.source_path);
        if let Some(existing) = self.store.find_archive_by_rel_path(&registered_rel).await? {
            return Err(service_err(
                "archive",
                &format!(
                    "目标路径已被存档「{}」（id={}）占用；请改名或先移除旧存档",
                    existing.name, existing.id
                ),
            ));
        }

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
            file_rel_path: registered_rel.clone(),
            payload_mode: mode,
            // reference 与登记键同源（两列恒等，避免再出现两种写法）；copy 为 None。
            source_path: (mode == PayloadMode::Reference).then(|| registered_rel.clone()),
            file_size,
            binding: req.binding.clone(),
            // 作用域为派生只读量：数据住项目库 → project（架构 §4.3）。
            scope: "project".to_string(),
        };

        // 登记行与「已归档」履历同一事务：不会出现时间线说归档了、列表里却没有。
        let archived_detail = format!("归档登记：{}", req.name);
        let resource = match self
            .store
            .insert_archive_with_event(input, Some(&archived_detail))
            .await
        {
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
            // 回执给真实入库键：reference 下就是外部源路径，用户看到的与库里一致。
            file_rel_path: registered_rel,
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
            // 旧内容没留下副本就继续覆盖，这一版将永远找不回来——中止，交给索引修复。
            return Err(service_err(
                "re_archive",
                "当前内容未能留成历史副本（本体缺失？），已中止再归档以免丢失旧内容",
            ));
        }

        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 新内容先落同卷暂存：此刻本体仍是旧内容，后面任何失败都不会污染它。
        // reference 不覆盖本体（源在外部），只更新指纹 / 规模 / 版本，故无需暂存。
        let staged = if mode == PayloadMode::Copy {
            Some(
                self.payload
                    .stage_payload(&req.source_path, &rel_path)
                    .await?,
            )
        } else {
            None
        };

        // 快照 + 索引 +1 + 履历事件同一事务：要么三件一起成立，要么一起回滚。
        let committed = self
            .store
            .commit_new_version(
                resource_id,
                current.version,
                &snapshot,
                &content_hash,
                file_size,
                analysis,
                crate::model::TimelineEventKind::Updated,
                Some(&format!(
                    "内容指纹变化，v{} → v{}",
                    current.version,
                    current.version + 1
                )),
            )
            .await;
        let updated = match committed {
            Ok(value) => value,
            Err(error) => {
                if let Some(path) = &staged {
                    self.payload.discard_staged(path).await;
                }
                return Err(error);
            }
        };

        if let Some(path) = &staged {
            if let Err(error) = self.payload.commit_staged(path, &rel_path).await {
                // 索引已提交、本体未落地：暂存文件保留作恢复凭据，时间线留一条醒目异常；
                // indexer 下次扫描会报 ContentChanged，不会静默过去。
                tracing::error!(
                    error = %error,
                    staged = %path.display(),
                    resource_id,
                    "再归档：索引已提交但本体替换失败，暂存文件保留待恢复"
                );
                self.record_timeline(
                    resource_id,
                    Some(updated.version),
                    crate::model::TimelineEventKind::Validated,
                    Some(&format!("新内容未落地，暂存文件保留：{}", path.display())),
                    Some(&content_hash),
                )
                .await;
                return Err(error);
            }
            self.payload.discard_staged(path).await;
        }

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
