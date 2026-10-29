//! 归档服务：本体层（`payload`）与索引层（`AnalyticsResourceStore`）的编排（架构 §6）。
//!
//! 三条不变式：
//! 1. **先本体、后索引，索引失败回滚本体**——本体是权威，索引可重建（架构 §6.3）；
//! 2. **内容指纹是版本的唯一触发条件**——指纹未变则不动本体、不增版本（架构 §5.1）；
//! 3. **事件发/收两端同批落地**——v1 那个"发了没人听"的无载荷事件是反例（架构 §6.2）。
//!
//! 本服务**不依赖任何上游模块**：归档入参是 `PathBuf` + 元数据，取回出参是
//! 调用方给的绝对路径（依赖方向 `scratchpad → analytics_resource`）。

use std::path::PathBuf;

use tokio::sync::broadcast;

use shared::error::{CoreError, StorageError};

use crate::model::{
    ArchiveKind, ArchiveOutcome, ArchiveRequest, ArchiveUndo, ChangeReason, CheckoutOutcome,
    CheckoutRequest, KeepVersions, NewArchiveInput, ORIGIN_RESOURCES, PayloadMode, ResourcesChanged,
    TRASH_ID_REF_PREFIX,
    TrashArchiveEntry,
};
use crate::payload::PayloadStore;
use crate::{AnalyticsResource, AnalyticsResourceStore};

/// 历史内容副本默认保留份数（设置项 `resources.keep_versions`，架构 §5.2）。
pub const DEFAULT_KEEP_VERSIONS: u32 = 5;

/// 事件广播容量：够覆盖面板重绘窗口即可，溢出由订阅方收到 `Lagged` 并整表刷新。
const EVENT_CHANNEL_CAPACITY: usize = 64;

/// 归档服务（按项目实例化；项目态不得放进程单例）。
pub struct ArchiveService {
    payload: PayloadStore,
    store: AnalyticsResourceStore,
    events: broadcast::Sender<ResourcesChanged>,
    keep_versions: KeepVersions,
}

impl ArchiveService {
    pub fn new(project_root: impl Into<PathBuf>, store: AnalyticsResourceStore) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            payload: PayloadStore::new(project_root),
            store,
            events,
            keep_versions: KeepVersions::Keep(DEFAULT_KEEP_VERSIONS),
        }
    }

    /// 覆盖历史内容保留策略（设置项 `resources.keep_versions` → `KeepVersions`）。
    ///
    /// `KeepVersions::All` 表示**不裁剪**（不是"保留 0 份"）。
    pub fn with_keep_versions(mut self, keep_versions: KeepVersions) -> Self {
        self.keep_versions = keep_versions;
        self
    }

    /// 本次操作生效的保留策略：请求里的覆盖优先，否则用装配时给的（设置项）。
    fn effective_keep(&self, request_override: Option<KeepVersions>) -> KeepVersions {
        request_override.unwrap_or(self.keep_versions)
    }

    /// 裁剪历史内容副本（全留 = 不动作）。
    ///
    /// 失败只记日志：历史副本多留几份不影响正确性（架构 §5.2）。
    async fn prune_copies(&self, resource_id: &str, request_override: Option<KeepVersions>) {
        let Some(keep) = self.effective_keep(request_override).limit() else {
            return;
        };
        if let Err(e) = self.payload.prune_version_copies(resource_id, keep).await {
            tracing::warn!(error = %e, resource_id, "裁剪历史内容副本失败");
        }
    }

    /// 归档后把「标签 / 分组」落到位（归档对话框里用户填的那两项）。
    ///
    /// 顺序与失败语义：本体与登记行是**主操作**（此刻已完成，不因附属项失败回滚——
    /// 那会把用户刚归档好的文件再搬回去）；标签与分组是**附属项**（都能在面板上补做），
    /// 失败把原因记进 [`ArchiveOutcome::notes`]，由宿主写进回执——**不静默吞掉**。
    ///
    /// 空值 = 不动：再归档时调用方不给这两项，不能把已有的标签 / 归属清掉。
    async fn apply_labels(
        &self,
        resource_id: &str,
        tags: &[String],
        group_id: Option<&str>,
    ) -> Vec<String> {
        let mut notes = Vec::new();

        if !tags.is_empty() {
            match self.link_tags(resource_id, tags).await {
                Ok(tag_notes) => notes.extend(tag_notes),
                // 连标签表都读不到（库异常）：整批说明一句，而不是逐条重复同一个原因。
                Err(error) => notes.push(format!("标签未打上：{error}")),
            }
        }

        if let Some(folder_id) = group_id {
            if let Err(error) = self
                .store
                .add_resource_to_folder(resource_id, folder_id)
                .await
            {
                notes.push(format!("分组未归入：{error}"));
            }
        }

        notes
    }

    /// 按名字把标签挂到存档上：库里没有的**就地新建**（同名复用，不重复建）。
    ///
    /// 单个标签失败不牵连其余：返回逐条失败说明（空 = 全成功）。名字按 `trim` 后**区分大小写**
    /// 比对——与存储层「同名（未删）拒绝」同口径，不然会出现“看起来建了、其实复用了另一个”。
    ///
    /// 名字 → id 的映射同时充当**本批缓存**：同一批里出现两次同一个名字（对话框会去重，
    /// 但服务层不能靠调用方）时第二次直接复用刚建的那条，而不是再建一次撞唯一索引。
    async fn link_tags(
        &self,
        resource_id: &str,
        names: &[String],
    ) -> Result<Vec<String>, CoreError> {
        let existing = self.store.list_tags(None).await?;
        let mut by_name: std::collections::HashMap<String, String> =
            existing.into_iter().map(|tag| (tag.name, tag.id)).collect();
        let mut notes = Vec::new();

        for name in names {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let tag_id = match by_name.get(name) {
                Some(id) => id.clone(),
                None => match self
                    .store
                    .create_tag(crate::models::CreateTagRequest {
                        name: name.to_string(),
                        color: None,
                        icon: None,
                        // 作用域与存档同口径（数据住项目库 → project，架构 §4.3）。
                        scope: "project".to_string(),
                    })
                    .await
                {
                    Ok(tag) => {
                        by_name.insert(tag.name.clone(), tag.id.clone());
                        tag.id
                    }
                    Err(error) => {
                        notes.push(format!("标签「{name}」未建立：{error}"));
                        continue;
                    }
                },
            };
            if let Err(error) = self.store.add_tag_to_resource(resource_id, &tag_id).await {
                notes.push(format!("标签「{name}」未打上：{error}"));
            }
        }

        Ok(notes)
    }

    /// 订阅变更事件（面板挂载时订阅一次）。
    pub fn subscribe(&self) -> broadcast::Receiver<ResourcesChanged> {
        self.events.subscribe()
    }

    /// 解析存档的本体路径（按归档模式分派）。
    ///
    /// copy：`resources/` 相对路径（`PayloadStore::resolve`，越界守卫生效）；
    /// reference：源**绝对**路径（`PayloadStore::resolve_source`，只校验绝对与存在）。
    ///
    /// 失败即返回错误——本体不在 / 源已删都要明说，不静默降级。
    fn payload_path_for(&self, resource: &AnalyticsResource) -> Result<PathBuf, CoreError> {
        match PayloadMode::from_db_str(&resource.payload_mode) {
            PayloadMode::Copy => {
                let rel = resource.file_rel_path.clone().ok_or_else(|| {
                    service_err("resolve", "该存档没有登记本体路径（非文件型或旧行）")
                })?;
                self.payload.resolve(&rel)
            }
            PayloadMode::Reference => {
                let source = resource.source_path.clone().ok_or_else(|| {
                    service_err("resolve", "引用型存档没有登记源路径（数据异常）")
                })?;
                self.payload.resolve_source(&source)
            }
        }
    }

    pub fn payload(&self) -> &PayloadStore {
        &self.payload
    }

    pub fn store(&self) -> &AnalyticsResourceStore {
        &self.store
    }

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
            PayloadMode::Reference => req
                .source_path
                .to_string_lossy()
                .replace('\\', "/"),
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
            tracing::warn!(resource_id, version = current.version, "再归档时旧内容副本未落（本体缺失？）");
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

    /// 取回（检出）：把本体复制成可写工作副本，本体不动。
    pub async fn checkout(&self, req: CheckoutRequest) -> Result<CheckoutOutcome, CoreError> {
        // 目标不得落在 resources/ 内：那等于绕开只读守卫写本体。
        if req.dest_path.starts_with(self.payload.resources_dir()) {
            return Err(service_err(
                "checkout",
                "取回目标不能位于 resources/ 内（那是归档本体目录）",
            ));
        }

        let resource = self.store.get_resource_by_id(&req.resource_id).await?;
        let rel_path = resource
            .file_rel_path
            .clone()
            .ok_or_else(|| service_err("checkout", "该存档没有本体（非文件型或旧行）"))?;

        match PayloadMode::from_db_str(&resource.payload_mode) {
            // copy：从 `resources/` 副本复制（本体不动、仍只读）。
            PayloadMode::Copy => self.payload.copy_out(&rel_path, &req.dest_path).await?,
            // reference：从源绝对路径复制（本体在源位置，M6 不动它）。
            PayloadMode::Reference => {
                let source = self.payload_path_for(&resource)?;
                self.payload.copy_out_source(&source, &req.dest_path).await?;
            }
        }
        self.emit(ChangeReason::CheckedOut, Some(resource.id.clone()));

        Ok(CheckoutOutcome {
            resource_id: resource.id,
            version: resource.version,
            dest_path: req.dest_path,
        })
    }

    /// 还原到某个历史版本：**用旧内容生成新版本**，不覆盖历史（原型 §4.3）。
    ///
    /// 与 `archive_into_existing` 同构（写前快照 → 覆盖本体 → 索引 +1），只是内容来源从
    /// “工作区文件”换成“历史副本”；两条守卫：
    ///
    /// - 该版本没有内容副本（被保留策略裁掉 / 从未生成）→ 拒绝，并说清“副本没了”；
    /// - 副本内容与当前内容指纹相同 → 幂等返回，不产生无意义的新版本（架构 §5.1）。
    pub async fn restore_version(
        &self,
        resource_id: &str,
        version: i32,
    ) -> Result<ArchiveOutcome, CoreError> {
        let current = self.store.get_resource_by_id(resource_id).await?;
        if current.deleted_at.is_some() {
            return Err(service_err("restore", "存档已删除，无法还原"));
        }
        // reference 模式的本体在源位置，不归 M6 管辖——"还原为旧版本"意味着覆盖源文件，
        // 那可能正是用户工作区里在用的活文件，风险不可控；M6 定位"留存 + 版本"不做覆盖外部源。
        // 出口：取回该版本为草稿（`checkout_version`），需要旧内容时从历史副本拿。
        if PayloadMode::from_db_str(&current.payload_mode) == PayloadMode::Reference {
            return Err(service_err(
                "restore",
                "引用型存档的本体在源位置，不支持覆盖还原；请用「取回该版本为草稿」",
            ));
        }
        let rel_path = current
            .file_rel_path
            .clone()
            .ok_or_else(|| service_err("restore", "该存档没有本体路径（非文件型或旧行）"))?;

        let copy = self
            .payload
            .version_copy_file(resource_id, version)
            .await?
            .ok_or_else(|| {
                service_err(
                    "restore",
                    &format!("v{version} 没有内容副本（可能已被保留策略裁剪），无法还原"),
                )
            })?;
        let copy_hash = self.payload.content_hash(&copy).await?;
        // 还原后的体积就是副本的体积（写回后本体与副本同内容）。
        let file_size = self.payload.file_size(&copy).await?;
        if copy_hash == current.content_hash.clone().unwrap_or_default() {
            // 幂等：要还原的内容就是当前内容，什么都不做（与再归档的指纹守卫同一口径）。
            return Ok(ArchiveOutcome {
                resource_id: current.id,
                version: current.version,
                content_hash: copy_hash,
                file_rel_path: rel_path,
                created_new_version: false,
                notes: Vec::new(),
            });
        }

        // 与再归档同序：先把当前内容留副本、写快照行，再覆盖本体，最后动索引。
        self.payload
            .store_version_copy(resource_id, current.version, &rel_path)
            .await?;
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

        if !self
            .payload
            .restore_version_copy(resource_id, version, &rel_path)
            .await?
        {
            // 副本在“查到”与“写回”之间消失（并发裁剪 / 外部删除）：说清，不静默降级。
            return Err(service_err(
                "restore",
                &format!("v{version} 的内容副本在写回前消失，还原中止"),
            ));
        }

        let updated = self
            .store
            // 版本还原：只换内容与体积；分析表的定义 / 规模保持原位不动
            // （还原旧内容不等于把“这份东西是什么”改回去——那要走再归档）。
            .update_archive_content(resource_id, &copy_hash, &snapshot_id, file_size, None)
            .await?;

        self.prune_copies(resource_id, None).await;

        self.emit(ChangeReason::Restored, Some(resource_id.to_string()));

        Ok(ArchiveOutcome {
            resource_id: updated.id,
            version: updated.version,
            content_hash: copy_hash,
            file_rel_path: rel_path,
            created_new_version: true,
            // 还原不带标签 / 分组：那两项是归档入口的事。
            notes: Vec::new(),
        })
    }

    /// 取回某个**历史版本**为工作副本（版本历史对话框的“取回该版本为草稿”）。
    ///
    /// 与 `checkout` 同一语义，只是源换成历史副本（本体不动）。
    pub async fn checkout_version(
        &self,
        resource_id: &str,
        version: i32,
        dest_path: &std::path::Path,
    ) -> Result<CheckoutOutcome, CoreError> {
        if dest_path.starts_with(self.payload.resources_dir()) {
            return Err(service_err(
                "checkout_version",
                "取回目标不能位于 resources/ 内（那是归档本体目录）",
            ));
        }
        let resource = self.store.get_resource_by_id(resource_id).await?;
        // 本体异常（缺失 / 内容已变）不影响历史副本：能从历史取回就让它取。
        if !self
            .payload
            .copy_version_out(resource_id, version, dest_path)
            .await?
        {
            return Err(service_err(
                "checkout_version",
                &format!("v{version} 没有内容副本（可能已被保留策略裁剪），无法取回"),
            ));
        }
        self.emit(ChangeReason::CheckedOut, Some(resource.id.clone()));

        Ok(CheckoutOutcome {
            resource_id: resource.id,
            version,
            dest_path: dest_path.to_path_buf(),
        })
    }

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
    pub async fn deleted_reference_archives(
        &self,
    ) -> Result<Vec<AnalyticsResource>, CoreError> {
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
        Ok(())
    }

    /// 发事件：无订阅者时 `send` 返回 `Err`，那不是错误（面板可能未打开）。
    fn emit(&self, reason: ChangeReason, resource_id: Option<String>) {
        let _ = self.events.send(ResourcesChanged {
            reason,
            resource_id,
        });
    }
}

/// `kind` → `resource_type` 的默认词表（完整收敛待 P0.7：词表应上提到 `shared` 枚举）。
fn default_resource_type(kind: ArchiveKind) -> String {
    match kind {
        ArchiveKind::File => "file",
        ArchiveKind::Analysis => "analysis",
        ArchiveKind::TableRef => "table_ref",
    }
    .to_string()
}

fn service_err(operation: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests;
