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
    CheckoutRequest, KeepVersions, NewArchiveInput, ORIGIN_RESOURCES, ResourcesChanged,
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
            if let Err(error) = self.store.add_resource_to_folder(resource_id, folder_id).await {
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
    async fn link_tags(&self, resource_id: &str, names: &[String]) -> Result<Vec<String>, CoreError> {
        let existing = self.store.list_tags(None).await?;
        let mut by_name: std::collections::HashMap<String, String> = existing
            .into_iter()
            .map(|tag| (tag.name, tag.id))
            .collect();
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
        let content_hash = self.payload.content_hash(&req.source_path).await?;
        // 体积也在搬运前读：本体随后会被 move 进 `resources/`，读的是同一个文件。
        let file_size = self.payload.file_size(&req.source_path).await?;

        match req.existing_resource_id.clone() {
            None => self.archive_new(req, content_hash, file_size).await,
            Some(resource_id) => {
                self.archive_into_existing(&resource_id, req, content_hash, file_size)
                    .await
            }
        }
    }

    /// 首次归档：本体 move + 写索引行；索引失败把本体移回原处。
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

        let payload_path = self
            .payload
            .archive_in(&req.source_path, &req.rel_path)
            .await?;

        let input = NewArchiveInput {
            resource_type: default_resource_type(req.kind),
            name: req.name.clone(),
            alias: req.alias.clone(),
            kind: req.kind,
            content_hash: content_hash.clone(),
            file_rel_path: req.rel_path.clone(),
            file_size,
            binding: req.binding.clone(),
            // 作用域为派生只读量：数据住项目库 → project（架构 §4.3）。
            scope: "project".to_string(),
        };

        let resource = match self.store.insert_archive(input).await {
            Ok(resource) => resource,
            Err(error) => {
                // 回滚本体：宁可回到"文件还在草稿箱"，也不要"文件没了、列表里也没有"。
                if let Err(rollback) = self
                    .payload
                    .move_payload_out(&req.rel_path, &req.source_path)
                    .await
                {
                    tracing::error!(
                        error = %rollback,
                        rel = %req.rel_path,
                        path = %payload_path.display(),
                        "归档回滚失败：本体留在 resources/，需经索引修复处理"
                    );
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
    ) -> Result<ArchiveOutcome, CoreError> {
        let current = self.store.get_resource_by_id(resource_id).await?;
        if current.deleted_at.is_some() {
            return Err(service_err("re_archive", "存档已删除，无法再归档"));
        }
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

        self.payload
            .replace_payload(&req.source_path, &rel_path)
            .await?;

        let updated = self
            .store
            .update_archive_content(resource_id, &content_hash, &snapshot_id, file_size)
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

        self.payload.copy_out(&rel_path, &req.dest_path).await?;
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
            .update_archive_content(resource_id, &copy_hash, &snapshot_id, file_size)
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

        let restored = trash.restore(trash_id, &self.payload.resources_dir()).await?;
        // 避让改过名时，登记行的本体路径要跟着改，否则索引与本体又对不上。
        let new_rel = restored
            .path
            .strip_prefix(self.payload.resources_dir())
            .ok()
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .filter(|rel| rel != &entry.manifest.original_rel_path);
        if let Err(error) = self.store.undelete_archive(&record.id, new_rel.as_deref()).await {
            // 回滚：把本体再移回收站（条目 id 变了，但宁可多一条可读的回收站条目，
            // 也不要“本体回了原位、记录却还在回收站里”的错位）。
            if let Err(rollback) = trash
                .move_to_trash(&restored.path, ORIGIN_RESOURCES, &entry.manifest.original_rel_path)
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

    /// 撤销一次归档（原型 §4.1：归档是“把文件从工作区搬走”的不可逆动作，必须给一个立即反悔的窗口）。
    ///
    /// **只管刚发生的那一次**，三条都不静默降级：
    ///
    /// - 版本 > 1（即已经再归档过）→ 拒绝：撤销会连带丢掉新内容；
    /// - 原位置已被占用 → 拒绝：撤销必须**精确**还原，覆盖别人不叫撤销；
    /// - 顺序与归档同构（本体先行、索引后动、失败回滚）：掉索引失败就把本体搬回去，
    ///   宁可回到"文件还在 resources/ 且记录还在"，也不要"文件没了、记录也没了"。
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
        if undo.source_path.exists() {
            return Err(service_err(
                "undo",
                &format!(
                    "原位置已有文件（{}）：撤销要精确还原，请先移走它",
                    undo.source_path.display()
                ),
            ));
        }

        self.payload
            .move_payload_out(&rel_path, &undo.source_path)
            .await?;
        if let Err(error) = self.store.hard_delete_row(&undo.resource_id).await {
            // 回滚本体：与 `archive_new` 的"索引失败把本体移回去"同一条纪律。
            if let Err(rollback) = self.payload.archive_in(&undo.source_path, &rel_path).await {
                tracing::error!(
                    error = %rollback,
                    rel = %rel_path,
                    "撤销回滚失败：本体留在原位置且记录仍在，需经索引修复处理"
                );
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
mod tests {
    use super::*;
    use crate::model::{ArchiveBinding, ArchiveKind};
    use engine::persistence::ProjectSqlitePool;
    use std::sync::Arc;
    use tokio::fs;

    const MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/007_analytics_resources.sql");
    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/020_analytics_resource_archive.sql");

    /// 测试服务：默认给「保留 n 份」（大多数用例只关心份数）。
    ///
    /// 需要全留 / 只留元数据这类策略的用例走 [`test_service_with`]。
    async fn test_service(keep_versions: u32) -> (ArchiveService, PathBuf) {
        test_service_with(KeepVersions::Keep(keep_versions)).await
    }

    async fn test_service_with(keep_versions: KeepVersions) -> (ArchiveService, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "rds_archive_{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).await.expect("create temp project");
        let pool = Arc::new(
            ProjectSqlitePool::new(dir.join("project.db"), 2)
                .await
                .expect("create pool"),
        );
        {
            let conn = pool.acquire().await.expect("acquire");
            let inner = conn.inner().expect("inner");
            inner.execute_batch(MIGRATION_SQL).expect("migration 007");
            inner
                .execute_batch(ARCHIVE_MIGRATION_SQL)
                .expect("migration 020");
        }
        let store = AnalyticsResourceStore::new(pool);
        (
            ArchiveService::new(&dir, store).with_keep_versions(keep_versions),
            dir,
        )
    }

    fn archive_req(source: &std::path::Path, rel: &str, existing: Option<&str>) -> ArchiveRequest {
        ArchiveRequest {
            source_path: source.to_path_buf(),
            rel_path: rel.to_string(),
            name: "dau_report".to_string(),
            alias: None,
            kind: ArchiveKind::File,
            binding: ArchiveBinding {
                promoted_from: Some("scratchpad/dau.sql".to_string()),
                source_connection_id: Some("conn_1".to_string()),
                source_table: None,
            },
            tags: Vec::new(),
            group_id: None,
            keep_versions: None,
            existing_resource_id: existing.map(str::to_string),
        }
    }

    async fn write_draft(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).await.expect("write draft");
        path
    }

    #[tokio::test]
    async fn t101_archive_first_time_moves_payload_and_writes_registry() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;

        let outcome = service
            .archive(archive_req(&draft, "reports/dau.sql", None))
            .await
            .expect("archive");

        assert!(outcome.created_new_version);
        assert_eq!(outcome.version, 1);
        assert!(!draft.exists(), "归档是移动");

        let payload = service.payload().resources_dir().join("reports").join("dau.sql");
        assert!(payload.is_file());
        assert!(service.payload().is_readonly(&payload), "本体应只读");

        let row = service
            .store()
            .get_resource_by_id(&outcome.resource_id)
            .await
            .expect("row");
        assert_eq!(row.kind, "file");
        assert_eq!(row.content_hash.as_deref(), Some(outcome.content_hash.as_str()));
        assert_eq!(row.file_rel_path.as_deref(), Some("reports/dau.sql"));
        assert_eq!(row.readonly, 1);
        assert!(row.archived_at.is_some(), "归档时刻应记录");
        assert_eq!(row.file_size, Some(9), "归档时登记本体字节数（「大小」排序与行的尾巴都读它）");
        assert_eq!(row.promoted_from.as_deref(), Some("scratchpad/dau.sql"));
        assert_eq!(row.source_connection_id.as_deref(), Some("conn_1"));

        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::Archived);
        assert_eq!(event.resource_id.as_deref(), Some(outcome.resource_id.as_str()));

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t102_archive_refuses_occupied_target_and_keeps_source() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let first = write_draft(&dir, "a.sql", b"select 1;").await;
        service
            .archive(archive_req(&first, "same.sql", None))
            .await
            .expect("first archive");

        let second = write_draft(&dir, "b.sql", b"select 2;").await;
        let error = service
            .archive(archive_req(&second, "same.sql", None))
            .await
            .expect_err("conflict must fail");

        assert!(error.to_string().contains("已被存档"), "错误应指明占用者：{error}");
        assert!(second.is_file(), "失败时源文件必须保留");

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t103_archive_rolls_back_payload_when_index_write_fails() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;

        // 故障注入：用触发器让"写索引"必然失败（比构造数据冲突更贴近真实失败路径）。
        {
            let conn = service.store().pool().acquire().await.expect("acquire");
            conn.inner()
                .expect("inner")
                .execute_batch(
                    "CREATE TRIGGER test_block_archive BEFORE INSERT ON analytics_resources \
                     BEGIN SELECT RAISE(ABORT, 'blocked for test'); END;",
                )
                .expect("create trigger");
        }

        let draft = write_draft(&dir, "draft.sql", b"select 1;").await;
        let error = service
            .archive(archive_req(&draft, "taken.sql", None))
            .await
            .expect_err("index write must fail");

        assert!(error.to_string().contains("blocked for test"));
        assert!(
            draft.is_file(),
            "索引失败必须把本体移回原处（错误：{error}）"
        );
        assert!(
            !service.payload().resources_dir().join("taken.sql").exists(),
            "回滚后本体不应留在 resources/"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t104_checkout_copies_payload_out_writable() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let outcome = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");
        let _ = events.recv().await;

        let dest = dir.join("scratchpad").join("dau（工作副本）.sql");
        let checkout = service
            .checkout(CheckoutRequest {
                resource_id: outcome.resource_id.clone(),
                dest_path: dest.clone(),
            })
            .await
            .expect("checkout");

        assert_eq!(checkout.version, 1);
        assert!(dest.is_file());
        assert!(!service.payload().is_readonly(&dest), "工作副本必须可写");
        assert!(
            service
                .payload()
                .resources_dir()
                .join("dau.sql")
                .is_file(),
            "取回不得移动本体"
        );
        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::CheckedOut);

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t105_rearchive_with_same_content_is_idempotent() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");

        // 取回原样副本 → 内容未变 → 不应产生新版本。
        let copy = dir.join("dau（工作副本）.sql");
        service
            .checkout(CheckoutRequest {
                resource_id: first.resource_id.clone(),
                dest_path: copy.clone(),
            })
            .await
            .expect("checkout");

        let again = service
            .archive(archive_req(&copy, "dau.sql", Some(&first.resource_id)))
            .await
            .expect("re-archive");

        assert!(!again.created_new_version, "内容未变不应产生新版本");
        assert_eq!(again.version, 1);
        assert!(copy.is_file(), "幂等路径不删用户的工作副本");

        let versions = service
            .store()
            .get_resource_versions(&first.resource_id)
            .await
            .expect("versions");
        assert!(versions.is_empty(), "不应有历史版本行");

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t106_rearchive_changed_content_bumps_version_and_keeps_copy() {
        let (service, dir) = test_service(1).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");
        assert_eq!(
            events.recv().await.expect("archived event").reason,
            ChangeReason::Archived
        );

        // 取回 → 改动 → 再归档。
        let copy = dir.join("dau（工作副本）.sql");
        service
            .checkout(CheckoutRequest {
                resource_id: first.resource_id.clone(),
                dest_path: copy.clone(),
            })
            .await
            .expect("checkout");
        assert_eq!(
            events.recv().await.expect("checked out event").reason,
            ChangeReason::CheckedOut
        );
        // 特意改成长度不同的内容：体积是否跟着换，看的是这个数。
        fs::write(&copy, b"select 22;").await.expect("edit copy");

        let second = service
            .archive(archive_req(&copy, "dau.sql", Some(&first.resource_id)))
            .await
            .expect("re-archive");

        assert!(second.created_new_version);
        assert_eq!(second.version, 2, "内容变了版本 +1");
        assert!(!copy.exists(), "再归档同样是移动");

        let payload = service.payload().resources_dir().join("dau.sql");
        assert_eq!(
            service.payload().content_hash(&payload).await.expect("hash"),
            second.content_hash,
            "本体应已被新内容覆盖"
        );
        assert!(service.payload().is_readonly(&payload), "替换后仍只读");

        let versions = service
            .store()
            .get_resource_versions(&first.resource_id)
            .await
            .expect("versions");
        assert_eq!(versions.len(), 1, "写前快照：只保留历史版本 v1");
        assert_eq!(versions[0].version, 1);

        let row = service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .expect("row");
        assert_eq!(row.version, 2);
        assert_eq!(row.file_size, Some(10), "体积跟着新内容一起换（不与指纹脱节）");
        assert_eq!(
            row.parent_version_id.as_deref(),
            Some(versions[0].id.as_str()),
            "parent_version_id 应指向写前快照行"
        );

        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::Updated);

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 保留策略的两端：`All`（设置项 `-1`）一份不裁，`MetadataOnly`（`0`）副本全清——
    /// 两端都不动**版本行**（元数据永远保留，架构 §5.2）。
    #[tokio::test]
    async fn t130_keep_policy_ends_are_respected() {
        for (policy, expected_copies) in [
            (KeepVersions::All, vec![2, 1]),
            (KeepVersions::MetadataOnly, Vec::new()),
        ] {
            let (service, dir) = test_service_with(policy).await;
            let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
            let first = service
                .archive(archive_req(&draft, "dau.sql", None))
                .await
                .expect("archive");

            // 连着改两次内容：每次再归档都会把上一版留成一份内容副本。
            for (name, body) in [("dau2.sql", &b"select 22;"[..]), ("dau3.sql", &b"select 333;"[..])] {
                let next = write_draft(&dir, name, body).await;
                service
                    .archive(archive_req(&next, "dau.sql", Some(&first.resource_id)))
                    .await
                    .expect("re-archive");
            }

            assert_eq!(
                service
                    .payload()
                    .version_copies(&first.resource_id)
                    .await
                    .expect("copies"),
                expected_copies,
                "{policy:?} 的副本保留结果"
            );
            assert_eq!(
                service
                    .store()
                    .get_resource_versions(&first.resource_id)
                    .await
                    .expect("version rows")
                    .len(),
                2,
                "裁剪只动副本：版本行两个（v1 / v2 写前快照）应全在"
            );
            let _ = fs::remove_dir_all(&dir).await;
        }
    }

    /// 归档时填的**标签与分组真的落上**（之前 `ArchiveRequest` 的这两项没人消费，填了等于白填）：
    /// 标签按名字找 / 找不到就建（同名复用），分组就是移动语义；不存在的分组只给回执说明、
    /// **不回滚归档本身**（文件已经归档好了，分组是能在面板上补做的附属项）。
    #[tokio::test]
    async fn t131_archive_applies_tags_and_group() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let folder = service
            .store()
            .create_folder(crate::models::CreateFolderRequest {
                name: "月报".to_string(),
                scope: "project".to_string(),
                parent_folder_id: None,
                color: None,
                icon: None,
            })
            .await
            .expect("create folder");

        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let mut req = archive_req(&draft, "dau.sql", None);
        req.alias = Some("日报".to_string());
        req.tags = vec!["报表".to_string(), "月度".to_string(), "报表".to_string()];
        req.group_id = Some(folder.id.clone());

        let outcome = service.archive(req).await.expect("archive");
        assert!(outcome.notes.is_empty(), "全落上时不该有说明：{:?}", outcome.notes);

        let row = service
            .store()
            .get_resource_by_id(&outcome.resource_id)
            .await
            .expect("row");
        assert_eq!(row.alias.as_deref(), Some("日报"), "别名应写进登记行");

        let tags = service
            .store()
            .get_tags_for_resource(&outcome.resource_id)
            .await
            .expect("tags");
        let mut names: Vec<&str> = tags.iter().map(|tag| tag.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["报表", "月度"], "两个标签都建好并挂上（重复名只算一次）");

        assert_eq!(
            service
                .store()
                .folders_by_resource()
                .await
                .expect("folders"),
            std::collections::HashMap::from([(outcome.resource_id.clone(), folder.id.clone())]),
            "分组归属应落到关联表"
        );

        // 再归档另一份：同名标签**复用**，不另建一个。
        let second = write_draft(&dir, "dau2.sql", b"select 2;").await;
        let mut req = archive_req(&second, "dau2.sql", None);
        req.tags = vec!["报表".to_string()];
        service.archive(req).await.expect("archive 2");
        assert_eq!(
            service.store().list_tags(None).await.expect("tags").len(),
            2,
            "同名标签复用，不是每个存档建一个"
        );

        // 分组不存在（悬空 id）：归档仍然成功，但要在回执里说清。
        let third = write_draft(&dir, "dau3.sql", b"select 3;").await;
        let mut req = archive_req(&third, "dau3.sql", None);
        req.group_id = Some("af_不存在".to_string());
        let outcome = service.archive(req).await.expect("archive 3");
        assert_eq!(outcome.notes.len(), 1, "应有一条说明：{:?}", outcome.notes);
        assert!(
            outcome.notes[0].starts_with("分组未归入："),
            "说明要说清是哪一项没落上：{:?}",
            outcome.notes[0]
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t107_checkout_rejects_target_inside_resources() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let outcome = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");

        let inside = service.payload().resources_dir().join("escape.sql");
        assert!(
            service
                .checkout(CheckoutRequest {
                    resource_id: outcome.resource_id,
                    dest_path: inside,
                })
                .await
                .is_err(),
            "取回目标落在 resources/ 内必须被拒（那等于绕开只读守卫）"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t120_undo_moves_payload_back_and_drops_the_row() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "undo.sql", b"select 1;").await;
        let outcome = service
            .archive(archive_req(&draft, "reports/undo.sql", None))
            .await
            .expect("archive");
        assert!(!draft.exists(), "归档后本体离开原位");

        let undo = ArchiveUndo {
            resource_id: outcome.resource_id.clone(),
            name: "dau_report".to_string(),
            source_path: draft.clone(),
        };
        service.undo_archive(&undo).await.expect("undo");

        assert!(draft.is_file(), "撤销后本体回到原位");
        assert!(
            !service
                .payload()
                .resolve("reports/undo.sql")
                .expect("resolve")
                .exists(),
            "本体不再留在 resources/"
        );
        assert!(
            service
                .store()
                .get_resource_by_id(&outcome.resource_id)
                .await
                .is_err(),
            "登记行已硬删除"
        );
        // 发/收两端同批：先 Archived，再 Undone。
        let first = events.recv().await.expect("archived 事件");
        assert_eq!(first.reason, ChangeReason::Archived);
        let second = events.recv().await.expect("undone 事件");
        assert_eq!(second.reason, ChangeReason::Undone);

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t121_undo_refuses_on_occupied_origin_or_newer_version() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "occupied.sql", b"select 1;").await;
        let outcome = service
            .archive(archive_req(&draft, "occupied.sql", None))
            .await
            .expect("archive");
        let undo = ArchiveUndo {
            resource_id: outcome.resource_id.clone(),
            name: "dau_report".to_string(),
            source_path: draft.clone(),
        };

        // ① 原位置又被占：拒绝，且**两边都不动**（撤销不覆盖别人）。
        fs::write(&draft, b"other").await.expect("write again");
        assert!(
            service.undo_archive(&undo).await.is_err(),
            "原位置已有文件要拒绝"
        );
        assert!(
            service
                .payload()
                .resolve("occupied.sql")
                .expect("resolve")
                .is_file(),
            "被拒后本体仍在 resources/"
        );

        // ② 已经再归档过（v2）：撤销窗口已过（否则会连带丢掉新内容）。
        fs::write(&draft, b"select 2;").await.expect("rewrite");
        service
            .archive(archive_req(&draft, "occupied.sql", Some(&outcome.resource_id)))
            .await
            .expect("re-archive");
        assert!(
            service.undo_archive(&undo).await.is_err(),
            "已有新版本不给撤销"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t122_restore_version_brings_old_content_forward() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");
        let _ = events.recv().await.expect("archived event");

        // v2：内容变了 → v1 进历史（版本行 + 内容副本）。
        let second_draft = write_draft(&dir, "dau2.sql", b"select 2;").await;
        let second = service
            .archive(archive_req(
                &second_draft,
                "dau.sql",
                Some(&first.resource_id),
            ))
            .await
            .expect("re-archive");
        assert_eq!(second.version, 2);
        let _ = events.recv().await.expect("updated event");

        // 从历史取回 v1（不改本体）：内容是 v1 的字节，且工作副本可写。
        let copy = dir.join("dau（v1 工作副本）.sql");
        let checked = service
            .checkout_version(&first.resource_id, 1, &copy)
            .await
            .expect("checkout version");
        assert_eq!(checked.version, 1);
        assert_eq!(fs::read(&copy).await.expect("read copy"), b"select 1;");
        assert!(!service.payload().is_readonly(&copy), "工作副本不应只读");
        let _ = events.recv().await.expect("checked out event");

        // 还原到 v1：生成 v3（不覆盖历史），本体内容回到 v1 的字节。
        let restored = service
            .restore_version(&first.resource_id, 1)
            .await
            .expect("restore");
        assert!(restored.created_new_version);
        assert_eq!(restored.version, 3, "还原 = 生成新版本，不是原地回滚");
        let payload = service.payload().resources_dir().join("dau.sql");
        assert_eq!(fs::read(&payload).await.expect("read"), b"select 1;");
        assert!(service.payload().is_readonly(&payload), "还原后的本体仍只读");

        // 体积也回到 v1 的字节数（9 而不再是 v2 的）：「大小」一列不能停在还原前的值上。
        let row = service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .expect("row");
        assert_eq!(row.file_size, Some(9), "体积随本体一起回到历史值");

        // 历史：v1、v2 都有行（写前快照语义），且两份副本都还在（还原不消耗副本）。
        let versions = service
            .store()
            .get_resource_versions(&first.resource_id)
            .await
            .expect("versions");
        let numbers: Vec<i32> = versions.iter().map(|v| v.version).collect();
        assert_eq!(numbers, vec![2, 1]);
        assert_eq!(
            service
                .payload()
                .version_copies(&first.resource_id)
                .await
                .expect("copies"),
            vec![2, 1]
        );

        assert_eq!(
            events.recv().await.expect("restored event").reason,
            ChangeReason::Restored
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t123_restore_version_requires_copy_and_skips_identical_content() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");
        let second_draft = write_draft(&dir, "dau2.sql", b"select 2;").await;
        service
            .archive(archive_req(
                &second_draft,
                "dau.sql",
                Some(&first.resource_id),
            ))
            .await
            .expect("re-archive");

        // 副本被裁掉（或手工清理）：拒绝并说清原因，不动任何状态。
        assert!(
            service
                .payload()
                .delete_version_copy(&first.resource_id, 1)
                .await
                .expect("delete copy")
        );
        let error = service
            .restore_version(&first.resource_id, 1)
            .await
            .expect_err("no copy");
        assert!(
            error.to_string().contains("没有内容副本"),
            "原因要指向副本不在：{error}"
        );

        // 内容与当前一致的版本：幂等（不为“还原到自己”造一个无意义的新版本）。
        service
            .payload()
            .store_version_copy(&first.resource_id, 2, "dau.sql")
            .await
            .expect("copy v2");
        let outcome = service
            .restore_version(&first.resource_id, 2)
            .await
            .expect("idempotent restore");
        assert!(!outcome.created_new_version);
        assert_eq!(outcome.version, 2, "指纹未变不得递增版本");

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 移入回收站：本体离开 `resources/` 进入项目级回收站，索引行**软删**（列表与
    /// “按路径查存活行”都看不到，但记录本身还在，还原才能恢复别名 / 标签 / 指纹）。
    /// 永久删除与清空：都**只动本模块的**条目（回收站是共用的，删别人的东西不是清空）；
    /// 登记行与它的标签 / 分组归属一并清掉（外键没有 `ON DELETE CASCADE`，不清就删不掉）。
    #[tokio::test]
    async fn t128_purge_and_empty_stay_within_our_origin() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let first_draft = write_draft(&dir, "a.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&first_draft, "a.sql", None))
            .await
            .expect("archive a");
        let second_draft = write_draft(&dir, "b.sql", b"select 2;").await;
        let second = service
            .archive(archive_req(&second_draft, "b.sql", None))
            .await
            .expect("archive b");

        // 给第一条挂标签与分组：删除时必须连关联一起清（外键无 CASCADE，项目库开了 foreign_keys）。
        let tag = service
            .store()
            .create_tag(crate::models::CreateTagRequest {
                name: "important".to_string(),
                scope: "project".to_string(),
                color: None,
                icon: None,
            })
            .await
            .expect("tag");
        service
            .store()
            .add_tag_to_resource(&first.resource_id, &tag.id)
            .await
            .expect("link tag");

        let first_entry = service
            .move_to_trash(&[first.resource_id.clone()])
            .await
            .expect("trash a");
        let second_entry = service
            .move_to_trash(&[second.resource_id.clone()])
            .await
            .expect("trash b");

        // 别人的条目：永久删除必须被拒（只验证守卫，不真删）。
        let foreign_path = dir.join("draft.sql");
        fs::write(&foreign_path, b"select 3;").await.expect("write");
        let foreign = service
            .payload()
            .trash()
            .move_to_trash(&foreign_path, "scratchpad", "draft.sql")
            .await
            .expect("foreign entry");
        let error = service
            .purge_archive(&foreign.manifest.id)
            .await
            .expect_err("cross-module purge must fail");
        assert!(error.to_string().contains("scratchpad"), "错误要指明归属：{error}");

        // 单条永久删除：本体与登记行都不在了。
        service
            .purge_archive(&first_entry[0].trash_id)
            .await
            .expect("purge a");
        assert!(service
            .store()
            .find_deleted_archive_by_rel_path("a.sql")
            .await
            .expect("find a")
            .is_none());
        assert!(
            service
                .store()
                .get_resource_by_id(&first.resource_id)
                .await
                .is_err(),
            "登记行删了就该查不到（它已经不在回收站里等还原了）"
        );
        assert!(service
            .store()
            .get_tags_for_resource(&first.resource_id)
            .await
            .expect("tags")
            .is_empty());

        // 清空：只删本模块的两条（含刚删过的），别人的原样留着。
        let purged = service.empty_trash().await.expect("empty");
        assert_eq!(purged, 1, "a 已单条删过，剩 b 一条");
        let left = service.payload().trash().list().await.expect("list");
        assert_eq!(left.len(), 1, "草稿箱的条目必须原样留着");
        assert_eq!(left[0].manifest.origin, "scratchpad");
        assert!(service
            .store()
            .find_deleted_archive_by_rel_path("b.sql")
            .await
            .expect("find b")
            .is_none());
        assert!(second_entry.len() == 1);

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 由原相对路径还原（索引修复的「从回收站还原」）：命中就把本体搬回来并复活登记行；
    /// 回收站里没有对应条目时**不静默**，把用户指向仅剩的出口（删记录）。
    #[tokio::test]
    async fn t129_restore_by_rel_path_finds_the_entry_or_says_why_not() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "a.sql", b"select 1;").await;
        let archived = service
            .archive(archive_req(&draft, "reports/a.sql", None))
            .await
            .expect("archive");
        service
            .move_to_trash(&[archived.resource_id.clone()])
            .await
            .expect("trash");

        let revived = service
            .restore_archive_by_rel_path("reports/a.sql")
            .await
            .expect("restore by rel path");
        assert_eq!(revived.id, archived.resource_id);
        assert!(service
            .payload()
            .resources_dir()
            .join("reports")
            .join("a.sql")
            .is_file());

        let error = service
            .restore_archive_by_rel_path("reports/a.sql")
            .await
            .expect_err("条目已经从回收站里出去了");
        assert!(
            error.to_string().contains("只能删掉这条登记记录"),
            "错误要指向仅剩的出口：{error}"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t124_move_to_trash_soft_deletes_row_and_keeps_payload_in_trash() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let archived = service
            .archive(archive_req(&draft, "reports/dau.sql", None))
            .await
            .expect("archive");
        let _ = events.recv().await.expect("archived event");

        let entries = service
            .move_to_trash(&[archived.resource_id.clone()])
            .await
            .expect("move to trash");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].resource_id, archived.resource_id);
        assert_eq!(entries[0].name, "dau_report");

        // 本体：离开 resources/；回收站条目带 origin 与原相对路径（还原要靠它回原位）。
        let payload = service
            .payload()
            .resources_dir()
            .join("reports")
            .join("dau.sql");
        assert!(!payload.exists(), "移入回收站是移动，本体不能留在 resources/");
        let entry = service
            .payload()
            .trash()
            .get(&entries[0].trash_id)
            .await
            .expect("trash entry");
        assert_eq!(entry.manifest.origin, ORIGIN_RESOURCES);
        assert_eq!(entry.manifest.original_rel_path, "reports/dau.sql");

        // 索引：存活列表与按路径查找都看不见，但软删行仍在（deleted_at 非空）。
        assert!(
            service
                .store()
                .list_resources(None, None, None)
                .await
                .expect("list")
                .is_empty()
        );
        assert!(
            service
                .store()
                .find_archive_by_rel_path("reports/dau.sql")
                .await
                .expect("find")
                .is_none()
        );
        let row = service
            .store()
            .get_resource_by_id(&archived.resource_id)
            .await
            .expect("row");
        assert!(row.deleted_at.is_some(), "回收站里的行必须保留 deleted_at");

        assert_eq!(
            events.recv().await.expect("trashed event").reason,
            ChangeReason::Trashed
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 从回收站还原：本体按原相对路径回原位、软删行复活；**跨模块条目一律拒绝**
    /// （origin 校验——别的模块的本体不属于我们，误还原会把别人的数据搬到 resources/）。
    #[tokio::test]
    async fn t125_restore_from_trash_revives_row_and_rejects_foreign_origin() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let archived = service
            .archive(archive_req(&draft, "reports/dau.sql", None))
            .await
            .expect("archive");
        let _ = events.recv().await.expect("archived event");
        let trashed = service
            .move_to_trash(&[archived.resource_id.clone()])
            .await
            .expect("move to trash");
        let _ = events.recv().await.expect("trashed event");

        let revived = service
            .restore_archive_from_trash(&trashed[0].trash_id)
            .await
            .expect("restore");
        assert_eq!(revived.id, archived.resource_id);
        assert!(revived.deleted_at.is_none(), "还原必须清掉 deleted_at");
        assert_eq!(
            revived.file_rel_path.as_deref(),
            Some("reports/dau.sql"),
            "无同名冲突时登记行路径不应变"
        );
        assert!(
            service
                .payload()
                .resources_dir()
                .join("reports")
                .join("dau.sql")
                .is_file(),
            "本体应回到原相对路径"
        );
        assert_eq!(
            service
                .store()
                .list_resources(None, None, None)
                .await
                .expect("list")
                .len(),
            1,
            "还原后应重新出现在存活列表里"
        );
        assert_eq!(
            events.recv().await.expect("untrashed event").reason,
            ChangeReason::Untrashed
        );

        // 跨模块条目（模拟草稿箱的）：拒绝，且条目原样留在回收站里等它的主人。
        let foreign_path = dir.join("draft.sql");
        fs::write(&foreign_path, b"select 2;").await.expect("write");
        let foreign = service
            .payload()
            .trash()
            .move_to_trash(&foreign_path, "scratchpad", "draft.sql")
            .await
            .expect("foreign trash entry");
        let error = service
            .restore_archive_from_trash(&foreign.manifest.id)
            .await
            .expect_err("cross-module restore must fail");
        assert!(
            error.to_string().contains("scratchpad"),
            "错误要指明条目属于谁：{error}"
        );
        assert!(
            service
                .payload()
                .trash()
                .get(&foreign.manifest.id)
                .await
                .is_ok(),
            "被拒的条目必须留在回收站里"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 两条不静默降级的边界：**同名不覆盖**（回收站层避让改名，登记行的本体路径
    /// 跟着改），以及**本体缺失的行拒绝移入**（指向索引修复这个正规出口）。
    #[tokio::test]
    async fn t126_restore_avoids_name_conflict_and_trash_refuses_missing_payload() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let archived = service
            .archive(archive_req(&draft, "reports/dau.sql", None))
            .await
            .expect("archive");
        let trashed = service
            .move_to_trash(&[archived.resource_id.clone()])
            .await
            .expect("move to trash");

        // 原位被占用（模拟并存档 / 外部写入）：还原必须让路，不能覆盖。
        let occupied = service
            .payload()
            .resources_dir()
            .join("reports")
            .join("dau.sql");
        fs::write(&occupied, b"someone else").await.expect("occupy");

        let revived = service
            .restore_archive_from_trash(&trashed[0].trash_id)
            .await
            .expect("restore");
        assert_eq!(fs::read(&occupied).await.expect("read"), b"someone else");
        assert_eq!(
            revived.file_rel_path.as_deref(),
            Some("reports/dau_1.sql"),
            "避让改名后登记行必须跟着改，否则索引与本体又对不上"
        );
        assert!(
            service
                .payload()
                .resources_dir()
                .join("reports")
                .join("dau_1.sql")
                .is_file(),
            "避让后的本体应落在新路径"
        );

        // 本体缺失的行：拒绝移入（它的出口是索引修复，不是回收站）。
        let orphan_draft = write_draft(&dir, "orphan.sql", b"select 3;").await;
        let orphan = service
            .archive(archive_req(&orphan_draft, "orphan.sql", None))
            .await
            .expect("archive orphan");
        fs::remove_file(service.payload().resources_dir().join("orphan.sql"))
            .await
            .expect("remove payload behind the index");
        let error = service
            .move_to_trash(&[orphan.resource_id.clone()])
            .await
            .expect_err("missing payload must be refused");
        assert!(
            error.to_string().contains("本体不在"),
            "错误要指出本体缺失：{error}"
        );
        assert!(
            service
                .store()
                .get_resource_by_id(&orphan.resource_id)
                .await
                .expect("row")
                .deleted_at
                .is_none(),
            "被拒的行不能变成软删态"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 批量移入回收站中途失败：**已进回收站的不回退**（见 `move_to_trash` 的取舍），
    /// 但错误里必须说清“已移了几条”——否则用户只看到一句失败，列表却已经变了。
    #[tokio::test]
    async fn t127_batch_trash_reports_partial_progress() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let first_draft = write_draft(&dir, "a.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&first_draft, "a.sql", None))
            .await
            .expect("archive a");
        let second_draft = write_draft(&dir, "b.sql", b"select 2;").await;
        let second = service
            .archive(archive_req(&second_draft, "b.sql", None))
            .await
            .expect("archive b");
        fs::remove_file(service.payload().resources_dir().join("b.sql"))
            .await
            .expect("remove b payload behind the index");

        let error = service
            .move_to_trash(&[first.resource_id.clone(), second.resource_id.clone()])
            .await
            .expect_err("batch with a missing payload must fail");
        assert!(
            error.to_string().contains("已移入 1 项"),
            "错误要交代部分成功：{error}"
        );

        // 前一条真的进了回收站（本体走了、行软删），后一条一点没动。
        assert!(!service.payload().resources_dir().join("a.sql").exists());
        assert_eq!(service.payload().trash().list().await.expect("list").len(), 1);
        assert!(
            service
                .store()
                .get_resource_by_id(&first.resource_id)
                .await
                .expect("a")
                .deleted_at
                .is_some()
        );
        assert!(
            service
                .store()
                .get_resource_by_id(&second.resource_id)
                .await
                .expect("b")
                .deleted_at
                .is_none(),
            "失败的那条不能被顺手软删"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }
}
