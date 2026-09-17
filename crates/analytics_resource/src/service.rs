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
    CheckoutRequest, NewArchiveInput, ResourcesChanged,
};
use crate::payload::PayloadStore;
use crate::AnalyticsResourceStore;

/// 历史内容副本默认保留份数（设置项 `keepVersions`，架构 §5.2）。
pub const DEFAULT_KEEP_VERSIONS: u32 = 5;

/// 事件广播容量：够覆盖面板重绘窗口即可，溢出由订阅方收到 `Lagged` 并整表刷新。
const EVENT_CHANNEL_CAPACITY: usize = 64;

/// 归档服务（按项目实例化；项目态不得放进程单例）。
pub struct ArchiveService {
    payload: PayloadStore,
    store: AnalyticsResourceStore,
    events: broadcast::Sender<ResourcesChanged>,
    keep_versions: u32,
}

impl ArchiveService {
    pub fn new(project_root: impl Into<PathBuf>, store: AnalyticsResourceStore) -> Self {
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            payload: PayloadStore::new(project_root),
            store,
            events,
            keep_versions: DEFAULT_KEEP_VERSIONS,
        }
    }

    /// 覆盖历史内容保留份数（`0` = 只留版本元数据，不留内容副本）。
    pub fn with_keep_versions(mut self, keep_versions: u32) -> Self {
        self.keep_versions = keep_versions;
        self
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

        match req.existing_resource_id.clone() {
            None => self.archive_new(req, content_hash).await,
            Some(resource_id) => self.archive_into_existing(&resource_id, req, content_hash).await,
        }
    }

    /// 首次归档：本体 move + 写索引行；索引失败把本体移回原处。
    async fn archive_new(
        &self,
        req: ArchiveRequest,
        content_hash: String,
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

        self.emit(ChangeReason::Archived, Some(resource.id.clone()));

        Ok(ArchiveOutcome {
            resource_id: resource.id,
            version: resource.version,
            content_hash,
            file_rel_path: req.rel_path,
            created_new_version: true,
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
            return Ok(ArchiveOutcome {
                resource_id: current.id,
                version: current.version,
                content_hash,
                file_rel_path: rel_path,
                created_new_version: false,
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
            .update_archive_content(resource_id, &content_hash, &snapshot_id)
            .await?;

        if let Err(e) = self
            .payload
            .prune_version_copies(resource_id, req.keep_versions.unwrap_or(self.keep_versions))
            .await
        {
            // 裁剪失败只记日志：历史副本多留几份不影响正确性。
            tracing::warn!(error = %e, resource_id, "裁剪历史内容副本失败");
        }

        self.emit(ChangeReason::Updated, Some(resource_id.to_string()));

        Ok(ArchiveOutcome {
            resource_id: updated.id,
            version: updated.version,
            content_hash,
            file_rel_path: rel_path,
            created_new_version: true,
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
        if copy_hash == current.content_hash.clone().unwrap_or_default() {
            // 幂等：要还原的内容就是当前内容，什么都不做（与再归档的指纹守卫同一口径）。
            return Ok(ArchiveOutcome {
                resource_id: current.id,
                version: current.version,
                content_hash: copy_hash,
                file_rel_path: rel_path,
                created_new_version: false,
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
            .update_archive_content(resource_id, &copy_hash, &snapshot_id)
            .await?;

        if let Err(e) = self
            .payload
            .prune_version_copies(resource_id, self.keep_versions)
            .await
        {
            tracing::warn!(error = %e, resource_id, "裁剪历史内容副本失败");
        }

        self.emit(ChangeReason::Restored, Some(resource_id.to_string()));

        Ok(ArchiveOutcome {
            resource_id: updated.id,
            version: updated.version,
            content_hash: copy_hash,
            file_rel_path: rel_path,
            created_new_version: true,
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

    async fn test_service(keep_versions: u32) -> (ArchiveService, PathBuf) {
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
        fs::write(&copy, b"select 2;").await.expect("edit copy");

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
        assert_eq!(
            row.parent_version_id.as_deref(),
            Some(versions[0].id.as_str()),
            "parent_version_id 应指向写前快照行"
        );

        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::Updated);

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
}
