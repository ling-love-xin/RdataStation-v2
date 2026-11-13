pub(crate) use super::*;
pub(crate) use engine::persistence::ProjectSqlitePool;
pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::Arc;

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
/// 资产履历事件表（时间线）：服务层会写事件，测试库不跑这段就会 "no such table"。
/// 时间线（027）+ 业务断言 / 效力（028）合并跑：测试库结构须与生产库一致。
pub(super) const TIMELINE_MIGRATION_SQL: &str = concat!(
    include_str!("../../../store/migrations/project_meta/027_analytics_resource_timeline.sql"),
    include_str!("../../../store/migrations/project_meta/028_analytics_resource_semantics.sql"),
    include_str!("../../../store/migrations/project_meta/029_analytics_resource_refs.sql"),
    include_str!("../../../store/migrations/project_meta/030_analytics_resource_checkouts.sql")
);

/// 测试库跑**四段**迁移：007（表结构）+ 020（分析存档新列）+ 023（copy/reference 两档）+
/// 024（关联表去 tag 外键）。
///
/// 跑齐是刻意的：只跑 007 的话，测试库的表结构与生产库不一致，行映射一旦引用新列
/// 就会在测试里报 "no such column"（是缺陷，不是噪声）。
/// 测试项目目录：创建前顺手清掉上一轮遗留的 `rds_*` 目录。
///
/// 根因：断言失败 / panic / 早返回会跳过用例末尾的 `cleanup(dir)`，历史上 `.rds/tmp`
/// 堆到 500+ 个目录，Windows 并发链接时临时文件争用报 `LNK1104`。
/// 只删 1 小时前的目录，避免误伤并行测试正在使用的那一份。
pub(crate) fn temp_project_dir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&root) {
        let stale_before = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(3600))
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let is_dir = path.is_dir();
            // 目录只认本 crate 的测试项目；文件只认它自己造的两种临时前缀
            // （reference 测试的外部源、全局分发的临时 parquet）。
            let matches = if is_dir {
                name.starts_with("rds_")
            } else {
                name.starts_with("ref_src_") || name.starts_with("rds_dist_")
            };
            if !matches {
                continue;
            }
            let is_stale = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map(|modified| modified < stale_before)
                .unwrap_or(false);
            if is_stale {
                let result = if is_dir {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                let _ = result;
            }
        }
    }
    let dir = root.join(format!("rds_{tag}_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).expect("create temp project dir");
    dir
}

pub(super) async fn create_test_store() -> (AnalyticsResourceStore, PathBuf) {
    let dir = temp_project_dir("test");
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
        inner
            .execute_batch(TIMELINE_MIGRATION_SQL)
            .expect("run migration 027");
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
