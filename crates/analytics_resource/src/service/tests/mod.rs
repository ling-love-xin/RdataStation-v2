use super::*;
pub(crate) use crate::model::{
    ArchiveBinding, ArchiveKind, ArchiveRequest, ArchiveUndo, CheckoutRequest, PayloadMode,
};
pub(crate) use engine::persistence::ProjectSqlitePool;
pub(crate) use std::sync::Arc;
pub(crate) use tokio::fs;

pub(super) const MIGRATION_SQL: &str =
    include_str!("../../../../store/migrations/project_meta/007_analytics_resources.sql");
pub(super) const ARCHIVE_MIGRATION_SQL: &str =
    include_str!("../../../../store/migrations/project_meta/020_analytics_resource_archive.sql");
pub(super) const PAYLOAD_MODE_MIGRATION_SQL: &str =
    include_str!("../../../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql");
/// 关联表去 tag 外键（标签定义上移全局库，规格 §7.2；跨库无法 FK 约束）。
pub(super) const TAG_LINK_FK_MIGRATION_SQL: &str = include_str!(
    "../../../../store/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
);
/// 资产履历事件表（时间线）：服务层会写事件，测试库不跑这段就会 "no such table"。
/// 时间线（027）+ 业务断言 / 效力（028）合并跑：测试库结构须与生产库一致。
pub(super) const TIMELINE_MIGRATION_SQL: &str = concat!(
    include_str!("../../../../store/migrations/project_meta/027_analytics_resource_timeline.sql"),
    include_str!("../../../../store/migrations/project_meta/028_analytics_resource_semantics.sql"),
    include_str!("../../../../store/migrations/project_meta/029_analytics_resource_refs.sql")
);

/// 测试服务：默认给「保留 n 份」（大多数用例只关心份数）。
///
/// 需要全留 / 只留元数据这类策略的用例走 [`test_service_with`]。
pub(super) async fn test_service(keep_versions: u32) -> (ArchiveService, PathBuf) {
    test_service_with(KeepVersions::Keep(keep_versions)).await
}

pub(super) async fn test_service_with(keep_versions: KeepVersions) -> (ArchiveService, PathBuf) {
    let dir = crate::tests::temp_project_dir("archive");
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
            .execute_batch(TAG_LINK_FK_MIGRATION_SQL)
            .expect("migration 024");
        inner
            .execute_batch(TIMELINE_MIGRATION_SQL)
            .expect("migration 027");
    }
    // 全局标签库注入本测试独立路径（避免共享进程级隔离根 → 跨测试同名标签冲突）。
    let store = AnalyticsResourceStore::new_for_test(pool, dir.join("global.db"));
    (
        ArchiveService::new(&dir, store).with_keep_versions(keep_versions),
        dir,
    )
}

pub(super) fn archive_req(source: &std::path::Path, rel: &str, existing: Option<&str>) -> ArchiveRequest {
    ArchiveRequest {
        source_path: source.to_path_buf(),
        rel_path: rel.to_string(),
        name: "dau_report".to_string(),
        alias: None,
        kind: ArchiveKind::File,
        // 大多数服务用例是文件型归档；分析表那条路见 `archive_analysis_req`。
        payload_mode: PayloadMode::Copy,
        analysis: None,
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

mod archive;
mod checkout;
mod reference;
mod semantics;
mod timeline;
mod trash;
mod versions;

/// 构造 reference 归档请求：本体留原位、只登记元数据。
pub(super) fn reference_req(source: &std::path::Path, rel: &str) -> ArchiveRequest {
    let mut req = archive_req(source, rel, None);
    req.payload_mode = PayloadMode::Reference;
    req
}
