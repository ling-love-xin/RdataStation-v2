//! 归档入口与新建 / 并入既有两条路径。

use super::*;
use crate::model::*;
use crate::payload::manifest::FileManifest;
use crate::store::file_asset::NewFileAssetVersion;
use std::path::{Path, PathBuf};

/// 一次文件归档预先落进 CAS 的产物。
///
/// manifest 已经先落盘（不可达对象可由 GC 清），登记事务成功后才成为版本索引的权威。
struct PreparedFile {
    content_hash: String,
    file_size: Option<i64>,
    manifest_id: String,
    meta_hash: Option<String>,
}

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
        // 文件资产（copy 粒度）一律先入 CAS：content_hash 取字节 sha256，
        // manifest 同时收 FileMeta，后续取回 / 还原都按这一版清单物化。
        let prepared = if req.kind == ArchiveKind::File && req.payload_mode == PayloadMode::Copy {
            Some(self.prepare_file_cas(&req).await?)
        } else {
            None
        };
        let content_hash = match prepared.as_ref() {
            Some(file) => file.content_hash.clone(),
            None if req.kind == ArchiveKind::File => {
                self.payload.content_hash(&req.source_path).await?
            }
            None => match analysis.as_ref() {
                Some(facts) => facts.fingerprint(),
                None => self.payload.content_hash(&req.source_path).await?,
            },
        };
        // 体积也在搬运前读：本体随后会被 move 进 `resources/`，读的是同一个文件。
        let file_size = match prepared.as_ref() {
            Some(file) => file.file_size,
            None => self.payload.file_size(&req.source_path).await?,
        };

        match req.existing_resource_id.clone() {
            None => self.archive_new(req, content_hash, file_size, prepared).await,
            Some(resource_id) => {
                self.archive_into_existing(
                    &resource_id,
                    req,
                    content_hash,
                    file_size,
                    analysis.as_ref(),
                    prepared,
                )
                .await
            }
        }
    }

    /// 把源文件与它的 FileMeta 落进 CAS，生成这一版 manifest。
    async fn prepare_file_cas(&self, req: &ArchiveRequest) -> Result<PreparedFile, CoreError> {
        let cas = self.payload.cas();
        let object = cas.put_file(&req.source_path).await?;
        let meta = self.read_source_file_meta(req).await?;
        let meta_hash = match meta.as_ref() {
            Some(meta) => Some(cas.put_file_meta(meta).await?.hash),
            None => None,
        };
        let file_name = req
            .source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&req.name)
            .to_string();
        let manifest = FileManifest::single_file(
            file_name,
            object.hash.clone(),
            object.size,
            meta_hash.clone(),
        );
        let manifest_id = cas.put_manifest(&manifest).await?;
        Ok(PreparedFile {
            content_hash: object.hash,
            file_size: Some(i64::try_from(object.size).unwrap_or(i64::MAX)),
            manifest_id,
            meta_hash,
        })
    }

    /// 读源文件的完整 `FileMeta`：草稿走 scratchpad 的集中 sidecar，本地文件走同目录 sidecar。
    ///
    /// 这里只按字符串约定定位，不依赖 scratchpad crate；依赖方向保持
    /// `scratchpad → analytics_resource`，M6 不反向认识草稿箱内部类型。
    async fn read_source_file_meta(
        &self,
        req: &ArchiveRequest,
    ) -> Result<Option<file_meta::FileMeta>, CoreError> {
        let Some(path) = source_meta_path(self.payload.project_root(), req) else {
            return Ok(None);
        };
        if !path.is_file() {
            return Ok(None);
        }
        let path_for_read = path.clone();
        let meta = tokio::task::spawn_blocking(move || {
            file_meta::read_meta_at(&path_for_read)
                .map_err(|error| service_err("archive_meta", &error.to_string()))
        })
        .await
        .map_err(|error| {
            service_err(
                "archive_meta",
                &format!("同步读取任务失败：{error}"),
            )
        })??;
        Ok(Some(meta))
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
        prepared: Option<PreparedFile>,
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
        // 显示名在存活资产里唯一（身份仍是 id）：同名并存会让行选择、链归属和
        // 「按名字找资产」的入口都产生歧义。先于本体落盘检查，失败不留半份文件。
        if !req.name.trim().is_empty()
            && let Some(existing) = self
                .store
                .alive_by_name(req.name.trim())
                .await?
                .into_iter()
                .next()
        {
            return Err(service_err(
                "archive",
                &format!(
                    "显示名「{}」已被资产（id={}）占用；请换一个名字",
                    existing.name, existing.id
                ),
            ));
        }

        if let Some(file) = prepared.as_ref() {
            // 文件资产：按 manifest 把 CAS 内容物化到 resources/，不再走 copy_in 复制草稿。
            let cas = self.payload.cas();
            let manifest = cas.read_manifest(&file.manifest_id).await?;
            let dest = self.payload.resolve(&req.rel_path)?;
            cas.materialize_content(&manifest, &dest).await?;
            if let Err(error) = self.payload.set_readonly(&dest, true).await {
                tracing::warn!(error = %error, path = %dest.display(), "归档后设置只读失败（应用层守卫仍生效）");
            }
        } else if mode == PayloadMode::Copy {
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
            // 「能不能进分析库」只有一处判据：归档作业真的探到了结构。
            // 探测失败会降级成普通文件（`analysis = None`），这里自然落 `false`。
            structured: req.analysis.is_some(),
            binding: req.binding.clone(),
            // 作用域为派生只读量：数据住项目库 → project（架构 §4.3）。
            scope: "project".to_string(),
        };

        // 登记行与「已归档」履历同一事务：不会出现时间线说归档了、列表里却没有。
        // 文件资产还把 v1 的 manifest / content / meta hash 同事务写进版本索引。
        let archived_detail = format!("归档登记：{}", req.name);
        let file_version = prepared.as_ref().map(|file| NewFileAssetVersion {
            manifest_id: file.manifest_id.clone(),
            content_hash: file.content_hash.clone(),
            meta_hash: file.meta_hash.clone(),
            parent_version: None,
        });
        let resource = match self
            .store
            .insert_archive_with_event_and_file(input, file_version, Some(&archived_detail))
            .await
        {
            Ok(resource) => resource,
            Err(error) => {
                // 回滚本体（copy / CAS 物化）：宁可回到"文件还在草稿箱"，也不要"副本留了、记录没有"。
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

        let mut notes = self
            .apply_labels(&resource.id, &req.tags, req.group_id.as_deref())
            .await;
        // 归档消费草稿：只有来自 scratchpad 的源才删；本地文件归档不动用户原文件。
        // 登记事务已提交，删除失败不回滚归档，只把事实写进回执。
        if prepared.is_some() && is_scratchpad_source(&req) {
            if let Err(error) = tokio::fs::remove_file(&req.source_path).await {
                tracing::warn!(
                    error = %error,
                    path = %req.source_path.display(),
                    "归档后清理草稿失败，已保留待用户处理"
                );
                notes.push(format!(
                    "归档已完成，但草稿未删除：{}（可稍后手动清理）",
                    req.source_path.display()
                ));
            }
        }
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
        prepared: Option<PreparedFile>,
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
        let current_meta_hash = if prepared.is_some() {
            self.store
                .file_asset_version(resource_id, current.version)
                .await?
                .and_then(|version| version.meta_hash)
        } else {
            None
        };
        let same_snapshot = match prepared.as_ref() {
            Some(file) => current_hash == file.content_hash && current_meta_hash == file.meta_hash,
            None => current_hash == content_hash,
        };
        if same_snapshot {
            // 幂等：内容（以及文件资产的元数据）与当前版本相同 → 不覆盖本体、不增版本，
            // 保留调用方的工作副本。附属项照旧落：快照没变不代表这次填的标签 / 分组不算数。
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

        // 取回租约是再归档的 base 凭据：base 落后于当前版本，说明期间别的路径已经发布了
        // 新版本，禁止静默覆盖。旧数据无 lease 时先兼容放行（M4 再收紧）。
        if prepared.is_some() {
            if let Some(lease) = self.store.active_lease(resource_id).await? {
                if lease.base_version != current.version {
                    return Err(service_err(
                        "re_archive",
                        &format!(
                            "取回基线已过期：工作副本基于 v{}，当前资产已是 v{}；请重新取回或显式 fork",
                            lease.base_version, current.version
                        ),
                    ));
                }
            } else {
                tracing::warn!(
                    resource_id,
                    "再归档没有活跃取回租约，按旧数据兼容模式继续（M4 收紧）"
                );
            }
        }

        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 新内容先落同卷暂存：此刻本体仍是旧内容，后面任何失败都不会污染它。
        // 文件资产从 CAS manifest 物化到临时文件，再交给 payload 的暂存/原子替换；
        // 只有元数据变化（content 未变）时不碰本体。
        let content_changed = match prepared.as_ref() {
            Some(file) => current_hash != file.content_hash,
            None => true,
        };
        let staged = if content_changed {
            match prepared.as_ref() {
                Some(file) => {
                    let cas = self.payload.cas();
                    let manifest = cas.read_manifest(&file.manifest_id).await?;
                    let temp = self.payload.project_root().join(".RSmeta/cas/staging").join(format!(
                        "rearchive-{}.tmp",
                        uuid::Uuid::new_v4().simple()
                    ));
                    cas.materialize_content(&manifest, &temp).await?;
                    let staged = self.payload.stage_payload(&temp, &rel_path).await;
                    if let Err(error) = tokio::fs::remove_file(&temp).await {
                        tracing::warn!(error = %error, path = %temp.display(), "清理再归档临时物化文件失败");
                    }
                    Some(staged?)
                }
                None if mode == PayloadMode::Copy => Some(
                    self.payload
                        .stage_payload(&req.source_path, &rel_path)
                        .await?,
                ),
                None => None,
            }
        } else {
            None
        };

        // 快照 + 版本索引 + 履历事件同一事务：要么一起成立，要么一起回滚。
        let committed = match prepared.as_ref() {
            Some(file) => {
                self.store
                    .commit_new_file_version(
                        resource_id,
                        current.version,
                        &snapshot,
                        NewFileAssetVersion {
                            manifest_id: file.manifest_id.clone(),
                            content_hash: file.content_hash.clone(),
                            meta_hash: file.meta_hash.clone(),
                            parent_version: Some(current.version),
                        },
                        file_size,
                        analysis,
                        crate::model::TimelineEventKind::Updated,
                        Some(&format!(
                            "内容/元数据变化，v{} → v{}",
                            current.version,
                            current.version + 1
                        )),
                    )
                    .await
            }
            None => {
                self.store
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
                    .await
            }
        };
        let updated = match committed {
            Ok(value) => value,
            Err(error) => {
                if let Some(path) = &staged {
                    self.payload.discard_staged(path).await;
                }
                return Err(error);
            }
        };

        // 再归档成功后消费租约：版本索引已记新版本，租约失败只影响后续再归档的
        // base 校验，不回滚已成立的新版本。
        if prepared.is_some() {
            match self.store.active_lease(resource_id).await {
                Ok(Some(lease)) => {
                    if let Err(error) = self.store.consume_lease(&lease.lease_id, updated.version).await {
                        tracing::error!(error = %error, lease_id = %lease.lease_id, "再归档后消费取回租约失败");
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::error!(error = %error, resource_id, "再归档后查询取回租约失败");
                }
            }
        }

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

        let mut notes = self
            .apply_labels(&updated.id, &req.tags, req.group_id.as_deref())
            .await;
        // 再归档同样消费草稿工作副本；本地文件 / reference 不动源。
        if prepared.is_some() && is_scratchpad_source(&req) {
            if let Err(error) = tokio::fs::remove_file(&req.source_path).await {
                tracing::warn!(
                    error = %error,
                    path = %req.source_path.display(),
                    "再归档后清理草稿失败，已保留待用户处理"
                );
                notes.push(format!(
                    "再归档已完成，但草稿未删除：{}（可稍后手动清理）",
                    req.source_path.display()
                ));
            }
        }
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

/// 源文件是否来自草稿箱（只有这类归档才消费源文件）。
fn is_scratchpad_source(req: &ArchiveRequest) -> bool {
    req.binding
        .promoted_from
        .as_deref()
        .is_some_and(|from| from.starts_with("scratchpad/"))
}

/// 源文件完整 `FileMeta` 的 sidecar 位置。
///
/// - 草稿：`{project}/.RSmeta/scratchpad/meta/{相对目录}/{文件名}.meta.json`
///   （与 `scratchpad` 的集中 sidecar 口径一致，但不反向依赖该 crate）；
/// - 本地文件：同目录 `.{文件名}.meta.json`（`rds-file-meta` 默认口径）。
fn source_meta_path(project_root: &Path, req: &ArchiveRequest) -> Option<PathBuf> {
    if let Some(rel) = req
        .binding
        .promoted_from
        .as_deref()
        .and_then(|from| from.strip_prefix("scratchpad/"))
    {
        let relative = Path::new(rel);
        let mut sidecar = project_root
            .join(".RSmeta")
            .join("scratchpad")
            .join("meta");
        if let Some(parent) = relative.parent() {
            if !parent.as_os_str().is_empty() {
                sidecar = sidecar.join(parent);
            }
        }
        let name = relative.file_name()?.to_string_lossy();
        return Some(sidecar.join(format!("{name}.meta.json")));
    }
    Some(file_meta::meta_path(&req.source_path))
}
