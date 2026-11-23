pub(crate) use super::*;
pub(crate) use engine::persistence::ProjectSqlitePool;
pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::Arc;

// 项目库建表 SQL 统一从 `crate::store::schema_sql` 取：此前这里手抄了一份迁移清单，
// 加一个迁移要改六处（漏一处只在某个用例里以 no such column 冒出来），见该模块注释。

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
            .execute_batch(crate::store::schema_sql::PROJECT_MIGRATIONS_SQL)
            .expect("run analytics_resources migrations");
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
mod table_asset;
mod tags;
