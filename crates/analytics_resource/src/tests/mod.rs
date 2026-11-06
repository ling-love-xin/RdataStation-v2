pub(crate) use super::*;
pub(crate) use engine::persistence::ProjectSqlitePool;
pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::Arc;
pub(crate) use uuid::Uuid;

pub(super) const MIGRATION_SQL: &str =
    include_str!("../../../store/migrations/project_meta/007_analytics_resources.sql");

pub(super) const ARCHIVE_MIGRATION_SQL: &str =
    include_str!("../../../store/migrations/project_meta/020_analytics_resource_archive.sql");

pub(super) const PAYLOAD_MODE_MIGRATION_SQL: &str =
    include_str!("../../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql");
/// 关联表去 tag 外键（标签定义上移全局库，规格 §7.2；跨库无法 FK 约束）。
pub(super) const TAG_LINK_FK_MIGRATION_SQL: &str = include_str!(
    "../../../store/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
);

/// 测试库跑**四段**迁移：007（表结构）+ 020（分析存档新列）+ 023（copy/reference 两档）+
/// 024（关联表去 tag 外键）。
///
/// 跑齐是刻意的：只跑 007 的话，测试库的表结构与生产库不一致，行映射一旦引用新列
/// 就会在测试里报 "no such column"（是缺陷，不是噪声）。
pub(super) async fn create_test_store() -> (AnalyticsResourceStore, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rds_test_{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&dir).expect("create temp dir");
    let db_path = dir.join("project.db");
    let pool = Arc::new(
        ProjectSqlitePool::new(db_path.clone(), 2)
            .await
            .expect("create pool"),
    );
    {
        let conn = pool.acquire().await.expect("acquire connection");
        let inner = conn.inner().expect("get connection");
        inner
            .execute_batch(MIGRATION_SQL)
            .expect("run migration 007");
        inner
            .execute_batch(ARCHIVE_MIGRATION_SQL)
            .expect("run migration 020");
        inner
            .execute_batch(PAYLOAD_MODE_MIGRATION_SQL)
            .expect("run migration 023");
        inner
            .execute_batch(TAG_LINK_FK_MIGRATION_SQL)
            .expect("run migration 024");
    }
    // 全局标签库注入本测试独立路径（避免共享进程级隔离根 → 跨测试同名标签冲突）。
    let store = AnalyticsResourceStore::new_for_test(pool, dir.join("global.db"));
    (store, dir)
}

fn cleanup(dir: PathBuf) {
    let _ = fs::remove_dir_all(&dir);
}

mod folders;
mod records;
mod tags;
