//! 索引修复：文件系统（**本体权威**）与登记表（**索引**）之间的三类差异（架构 §7.2）。
//!
//! 硬原则：**不做自动修复**。`scan` 只报告，任何改动都由调用方（界面）在用户确认后调用——
//! 静默导入会让"存档"变成用户没同意过的东西，静默删除更不可接受。
//!
//! 三类差异（与原型 §4.5 的分组一一对应）：
//! 1. `UntrackedFile`  有文件、无记录（手工放入 / 从别处拷入）→ 补登
//! 2. `MissingPayload` 有记录、无本体（被手工删除 / 移动）→ 从回收站还原（待 P0.8）/ 删除记录
//! 3. `ContentChanged` 指纹不匹配（绕过只读改了内容）→ 接受当前内容（生成新版本）/ 从历史还原

use std::collections::HashSet;

use crate::model::{ArchiveBinding, ArchiveKind, NewArchiveInput};
use crate::payload::PayloadStore;
use crate::{AnalyticsResource, AnalyticsResourceStore, CoreError};

/// 一条索引差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexIssue {
    /// 有文件、无记录。
    UntrackedFile { rel_path: String },
    /// 有记录、无本体。
    MissingPayload {
        resource_id: String,
        name: String,
        rel_path: String,
    },
    /// 指纹不匹配：本体被绕过只读改过。
    ContentChanged {
        resource_id: String,
        name: String,
        rel_path: String,
        /// 登记表里的指纹（旧行可能为空 = 无比对基准，此时不会产生本差异）。
        expected_hash: String,
        /// 当前实际指纹。
        actual_hash: String,
    },
}

impl IndexIssue {
    /// 相关存档 id（`UntrackedFile` 无）。
    pub fn resource_id(&self) -> Option<&str> {
        match self {
            Self::UntrackedFile { .. } => None,
            Self::MissingPayload { resource_id, .. } | Self::ContentChanged { resource_id, .. } => {
                Some(resource_id)
            }
        }
    }

    /// 相关本体相对路径。
    pub fn rel_path(&self) -> &str {
        match self {
            Self::UntrackedFile { rel_path }
            | Self::MissingPayload { rel_path, .. }
            | Self::ContentChanged { rel_path, .. } => rel_path,
        }
    }

    /// 界面分组用的一句话说明。
    pub fn summary(&self) -> String {
        match self {
            Self::UntrackedFile { rel_path } => {
                format!("resources/{rel_path}：本体存在但没有登记记录")
            }
            Self::MissingPayload { name, rel_path, .. } => {
                format!("存档「{name}」的本体缺失（期望位置 resources/{rel_path}）")
            }
            Self::ContentChanged { name, rel_path, .. } => {
                format!("存档「{name}」的内容已变（resources/{rel_path}）")
            }
        }
    }
}

/// 扫描报告。
#[derive(Debug, Clone, Default)]
pub struct IndexScanReport {
    pub issues: Vec<IndexIssue>,
}

impl IndexScanReport {
    /// 三项均为 0（界面状态行显示"干净"）。
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn untracked_files(&self) -> Vec<&IndexIssue> {
        self.pick(|issue| matches!(issue, IndexIssue::UntrackedFile { .. }))
    }

    pub fn missing_payloads(&self) -> Vec<&IndexIssue> {
        self.pick(|issue| matches!(issue, IndexIssue::MissingPayload { .. }))
    }

    pub fn content_changes(&self) -> Vec<&IndexIssue> {
        self.pick(|issue| matches!(issue, IndexIssue::ContentChanged { .. }))
    }

    fn pick(&self, predicate: impl Fn(&IndexIssue) -> bool) -> Vec<&IndexIssue> {
        self.issues.iter().filter(|issue| predicate(issue)).collect()
    }
}

/// 索引修复：借用本体层与索引层（不持有，避免与 `ArchiveService` 争所有权）。
pub struct IndexRepair<'a> {
    payload: &'a PayloadStore,
    store: &'a AnalyticsResourceStore,
}

impl<'a> IndexRepair<'a> {
    pub fn new(payload: &'a PayloadStore, store: &'a AnalyticsResourceStore) -> Self {
        Self { payload, store }
    }

    /// 只读扫描：**不改任何状态**（可为 0 成本地反复调用，供面板刷新）。
    pub async fn scan(&self) -> Result<IndexScanReport, CoreError> {
        let files = self.payload.list_files().await?;
        let rows = self.store.list_file_archives().await?;

        let mut issues = Vec::new();
        let mut tracked: HashSet<&str> = HashSet::new();

        for row in &rows {
            let Some(rel_path) = row.file_rel_path.as_deref() else {
                continue;
            };
            tracked.insert(rel_path);

            let path = self.payload.resolve(rel_path)?;
            if !path.is_file() {
                issues.push(IndexIssue::MissingPayload {
                    resource_id: row.id.clone(),
                    name: row.name.clone(),
                    rel_path: rel_path.to_string(),
                });
                continue;
            }

            // 旧行（007 时代搬运过来的）没有指纹，就没有比对基准——不报"内容已变"。
            let Some(expected_hash) = row.content_hash.as_deref() else {
                continue;
            };
            let actual_hash = self.payload.content_hash(&path).await?;
            if actual_hash != expected_hash {
                issues.push(IndexIssue::ContentChanged {
                    resource_id: row.id.clone(),
                    name: row.name.clone(),
                    rel_path: rel_path.to_string(),
                    expected_hash: expected_hash.to_string(),
                    actual_hash,
                });
            }
        }

        for rel_path in files {
            if !tracked.contains(rel_path.as_str()) {
                issues.push(IndexIssue::UntrackedFile { rel_path });
            }
        }

        Ok(IndexScanReport { issues })
    }

    /// **补登**（人工确认后调用）：把已存在的本体登记为存档，指纹现算。
    ///
    /// 不搬动文件——本体本来就在 `resources/` 里；来源绑定留空（无从得知）。
    pub async fn adopt_file(
        &self,
        rel_path: &str,
        name: Option<&str>,
        kind: ArchiveKind,
    ) -> Result<AnalyticsResource, CoreError> {
        let path = self.payload.resolve(rel_path)?;
        if !path.is_file() {
            return Err(repair_err("adopt_file", "该路径下没有本体文件"));
        }
        if let Some(existing) = self.store.find_archive_by_rel_path(rel_path).await? {
            return Err(repair_err(
                "adopt_file",
                &format!("该本体已被存档「{}」登记", existing.name),
            ));
        }

        let content_hash = self.payload.content_hash(&path).await?;
        let display_name = name.map(str::to_string).unwrap_or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.to_string())
        });

        self.store
            .insert_archive(NewArchiveInput {
                resource_type: kind.as_db_str().to_string(),
                name: display_name,
                alias: None,
                kind,
                content_hash,
                file_rel_path: rel_path.to_string(),
                binding: ArchiveBinding::default(),
                scope: "project".to_string(),
            })
            .await
    }

    /// **接受当前内容**（`ContentChanged` 的修复）：指纹换成实际值、版本 +1、重新加只读。
    ///
    /// 诚实说明：这一版的"内容副本"**不可得**——旧内容在外部被覆盖时就已经没了，
    /// 因此该历史版本只有元数据（界面按"副本缺失"呈现，而不是假装能还原）。
    pub async fn accept_current_content(
        &self,
        resource_id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let current = self.store.get_resource_by_id(resource_id).await?;
        let rel_path = current
            .file_rel_path
            .clone()
            .ok_or_else(|| repair_err("accept_current_content", "该存档没有本体路径"))?;
        let path = self.payload.resolve(&rel_path)?;
        if !path.is_file() {
            return Err(repair_err(
                "accept_current_content",
                "本体不存在：应走「有记录、无本体」的处理（还原或删除记录）",
            ));
        }

        // 外部改动往往把只读属性也带走了，这里重新加回。
        if let Err(e) = self.payload.set_readonly(&path, true).await {
            tracing::warn!(error = %e, path = %path.display(), "恢复只读属性失败");
        }

        let actual_hash = self.payload.content_hash(&path).await?;
        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(shared::error::StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        let snapshot_id = self
            .store
            .save_resource_version(resource_id, current.version, &snapshot)
            .await?;

        self.store
            .update_archive_content(resource_id, &actual_hash, &snapshot_id)
            .await
    }

    /// **删除孤儿登记**（`MissingPayload` 的修复）：本体都没了，直接删行，不进回收站。
    pub async fn remove_orphan_record(&self, resource_id: &str) -> Result<(), CoreError> {
        self.store.remove_orphan_record(resource_id).await
    }
}

fn repair_err(operation: &str, reason: &str) -> CoreError {
    CoreError::storage(shared::error::StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArchiveRequest, ReproductionStrength};
    use crate::service::ArchiveService;
    use engine::persistence::ProjectSqlitePool;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::fs;

    const MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/007_analytics_resources.sql");
    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/020_analytics_resource_archive.sql");

    /// 建好临时项目 + 两段迁移 + 归档服务；修复器借用服务里的本体层与索引层。
    async fn test_env() -> (ArchiveService, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rds_indexer_{}", uuid::Uuid::new_v4().simple()));
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
        (ArchiveService::new(&dir, store), dir)
    }

    fn request(source: &std::path::Path, rel: &str) -> ArchiveRequest {
        ArchiveRequest {
            source_path: source.to_path_buf(),
            rel_path: rel.to_string(),
            name: "dau_report".to_string(),
            alias: None,
            kind: ArchiveKind::File,
            binding: ArchiveBinding::default(),
            tags: Vec::new(),
            group_id: None,
            keep_versions: None,
            existing_resource_id: None,
        }
    }

    async fn archive_one(service: &ArchiveService, dir: &std::path::Path, rel: &str) -> String {
        let draft = dir.join(format!("src_{}", rel.replace('/', "_")));
        fs::write(&draft, b"select 1;").await.expect("write draft");
        service
            .archive(request(&draft, rel))
            .await
            .expect("archive")
            .resource_id
    }

    #[tokio::test]
    async fn t201_scan_reports_untracked_file() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());

        assert!(repair.scan().await.expect("scan empty").is_clean());

        // 手工放入一个文件（模拟"从别处拷入 resources/"）。
        let stray = service.payload().resources_dir().join("stray.sql");
        fs::create_dir_all(service.payload().resources_dir())
            .await
            .expect("mkdir resources");
        fs::write(&stray, b"select 9;").await.expect("write stray");

        let report = repair.scan().await.expect("scan");
        assert_eq!(report.untracked_files().len(), 1);
        assert_eq!(report.untracked_files()[0].rel_path(), "stray.sql");
        assert!(report.missing_payloads().is_empty());
        assert!(report.content_changes().is_empty());

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t202_scan_reports_missing_payload() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());
        let id = archive_one(&service, &dir, "gone.sql").await;

        fs::remove_file(service.payload().resources_dir().join("gone.sql"))
            .await
            .expect("delete payload");

        let report = repair.scan().await.expect("scan");
        assert_eq!(report.missing_payloads().len(), 1);
        assert_eq!(report.missing_payloads()[0].resource_id(), Some(id.as_str()));
        assert!(report.untracked_files().is_empty());

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t203_scan_reports_content_changed() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());
        let id = archive_one(&service, &dir, "edited.sql").await;

        let payload = service.payload().resources_dir().join("edited.sql");
        service
            .payload()
            .set_readonly(&payload, false)
            .await
            .expect("clear readonly");
        fs::write(&payload, b"select 2;").await.expect("edit payload");

        let report = repair.scan().await.expect("scan");
        assert_eq!(report.content_changes().len(), 1);
        let issue = report.content_changes()[0];
        assert_eq!(issue.resource_id(), Some(id.as_str()));
        assert!(issue.summary().contains("内容已变"), "{}", issue.summary());

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t204_adopt_file_registers_and_clears_issue() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());

        let stray = service.payload().resources_dir().join("reports").join("dau.sql");
        fs::create_dir_all(stray.parent().expect("parent"))
            .await
            .expect("mkdir");
        fs::write(&stray, b"select 1;").await.expect("write stray");

        let adopted = repair
            .adopt_file("reports/dau.sql", None, ArchiveKind::File)
            .await
            .expect("adopt");

        assert_eq!(adopted.name, "dau", "默认取文件名（去扩展名）");
        assert_eq!(adopted.file_rel_path.as_deref(), Some("reports/dau.sql"));
        assert!(adopted.content_hash.is_some(), "补登应现算指纹");
        assert_eq!(adopted.kind, "file");
        assert_eq!(
            ArchiveKind::from_db_str(&adopted.kind).strength(),
            ReproductionStrength::Strong
        );
        assert!(repair.scan().await.expect("scan").is_clean(), "补登后应无差异");

        // 重复补登应被拒（同一本体只能一个存档占用）。
        assert!(repair
            .adopt_file("reports/dau.sql", None, ArchiveKind::File)
            .await
            .is_err());

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t205_accept_current_content_bumps_version_and_restores_readonly() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());
        let id = archive_one(&service, &dir, "changed.sql").await;

        let payload = service.payload().resources_dir().join("changed.sql");
        service
            .payload()
            .set_readonly(&payload, false)
            .await
            .expect("clear readonly");
        fs::write(&payload, b"select 42;").await.expect("edit payload");
        let actual = service.payload().content_hash(&payload).await.expect("hash");

        let accepted = repair.accept_current_content(&id).await.expect("accept");

        assert_eq!(accepted.version, 2, "接受当前内容 = 生成新版本");
        assert_eq!(accepted.content_hash.as_deref(), Some(actual.as_str()));
        assert!(
            service.payload().is_readonly(&payload),
            "修复后应重新加回只读"
        );
        assert!(
            repair.scan().await.expect("scan").is_clean(),
            "修复后不应再报内容已变"
        );

        let versions = service.store().get_resource_versions(&id).await.expect("versions");
        assert_eq!(versions.len(), 1, "写前快照保留旧指纹");
        assert_eq!(versions[0].version, 1);

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t206_remove_orphan_record_deletes_row() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());
        let id = archive_one(&service, &dir, "orphan.sql").await;

        fs::remove_file(service.payload().resources_dir().join("orphan.sql"))
            .await
            .expect("delete payload");
        assert_eq!(repair.scan().await.expect("scan").missing_payloads().len(), 1);

        repair.remove_orphan_record(&id).await.expect("remove");

        assert!(repair.scan().await.expect("scan").is_clean());
        assert!(
            service.store().get_resource_by_id(&id).await.is_err(),
            "孤儿登记应已删除"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t207_accept_current_content_rejects_missing_payload() {
        let (service, dir) = test_env().await;
        let repair = IndexRepair::new(service.payload(), service.store());
        let id = archive_one(&service, &dir, "lost.sql").await;

        fs::remove_file(service.payload().resources_dir().join("lost.sql"))
            .await
            .expect("delete payload");

        let error = repair
            .accept_current_content(&id)
            .await
            .expect_err("本体不存在时不应接受当前内容");
        assert!(error.to_string().contains("本体不存在"), "{error}");

        let _ = fs::remove_dir_all(&dir).await;
    }
}
