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
    ArchiveKind, ChangeReason, KeepVersions, ORIGIN_RESOURCES, PayloadMode, ResourcesChanged,
    TRASH_ID_REF_PREFIX,
};
use crate::payload::PayloadStore;
use crate::{AnalyticsResource, AnalyticsResourceStore};

pub mod archive;
pub mod checkout;
pub mod trash;

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
                    .create_tag(crate::store::rows::CreateTagRequest {
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
