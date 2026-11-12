use super::*;
use crate::model::{ArchiveRequest, ReproductionStrength};
use crate::service::ArchiveService;
use engine::persistence::ProjectSqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

const MIGRATION_SQL: &str =
    include_str!("../../../store/migrations/project_meta/007_analytics_resources.sql");
const ARCHIVE_MIGRATION_SQL: &str =
    include_str!("../../../store/migrations/project_meta/020_analytics_resource_archive.sql");
const PAYLOAD_MODE_MIGRATION_SQL: &str =
    include_str!("../../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql");
const TIMELINE_MIGRATION_SQL: &str = concat!(
    include_str!("../../../store/migrations/project_meta/027_analytics_resource_timeline.sql"),
    include_str!("../../../store/migrations/project_meta/028_analytics_resource_semantics.sql"),
    include_str!("../../../store/migrations/project_meta/029_analytics_resource_refs.sql")
);

/// 建好临时项目 + 三段迁移 + 归档服务；修复器借用服务里的本体层与索引层。
async fn test_env() -> (ArchiveService, PathBuf) {
    let dir = crate::tests::temp_project_dir("indexer");
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
        inner
            .execute_batch(PAYLOAD_MODE_MIGRATION_SQL)
            .expect("migration 023");
        inner
            .execute_batch(TIMELINE_MIGRATION_SQL)
            .expect("migration 027");
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
        payload_mode: crate::model::PayloadMode::Copy,
        analysis: None,
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
    assert_eq!(
        report.missing_payloads()[0].resource_id(),
        Some(id.as_str())
    );
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
    fs::write(&payload, b"select 2;")
        .await
        .expect("edit payload");

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

    let stray = service
        .payload()
        .resources_dir()
        .join("reports")
        .join("dau.sql");
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
    assert_eq!(
        adopted.file_size,
        Some(9),
        "补登也登记体积（否则「大小」一列永远是空的）"
    );
    assert_eq!(adopted.kind, "file");
    assert_eq!(
        ArchiveKind::from_db_str(&adopted.kind).strength(),
        ReproductionStrength::Strong
    );
    assert!(
        repair.scan().await.expect("scan").is_clean(),
        "补登后应无差异"
    );

    // 重复补登应被拒（同一本体只能一个存档占用）。
    assert!(
        repair
            .adopt_file("reports/dau.sql", None, ArchiveKind::File)
            .await
            .is_err()
    );

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
    fs::write(&payload, b"select 42;")
        .await
        .expect("edit payload");
    let actual = service
        .payload()
        .content_hash(&payload)
        .await
        .expect("hash");

    let accepted = repair.accept_current_content(&id).await.expect("accept");

    assert_eq!(accepted.version, 2, "接受当前内容 = 生成新版本");
    assert_eq!(accepted.content_hash.as_deref(), Some(actual.as_str()));
    assert_eq!(
        accepted.file_size,
        Some(10),
        "接受的是当前文件的实际体积（外部改动往往也改了长度）"
    );
    assert!(
        service.payload().is_readonly(&payload),
        "修复后应重新加回只读"
    );
    assert!(
        repair.scan().await.expect("scan").is_clean(),
        "修复后不应再报内容已变"
    );

    let versions = service
        .store()
        .get_resource_versions(&id)
        .await
        .expect("versions");
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
    assert_eq!(
        repair.scan().await.expect("scan").missing_payloads().len(),
        1
    );

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
