//! 文件 CAS GC 作业：项目 CAS + 全局 CAS 各跑一遍。
//!
//! 表快照由 `jobs::table_gc` 回收；本作业只处理 `payload::cas::CasStore` 里的
//! manifest / blob / staging。保留集来自版本索引（`file_asset_versions`）：
//! 仍被任一版本行引用的 manifest 连同其内容对象一定保留，孤儿超过 grace 才删。

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use engine::persistence::project_db::ProjectDatabaseManager;

use crate::payload::cas::{CasGcReport, CasStore};

/// 跑一轮项目 + 全局文件 CAS GC。任一侧失败只记日志，不影响另一侧。
pub(crate) async fn run_file_gc(
    project_root: Option<PathBuf>,
    grace: Duration,
) -> CasGcReport {
    let mut total = CasGcReport::default();
    if let Some(root) = project_root {
        match gc_project(&root, grace).await {
            Ok(report) => total.merge(report),
            Err(error) => {
                tracing::warn!(error = %error, root = %root.display(), "项目文件 CAS GC 失败")
            }
        }
    }
    match crate::global::GlobalInstanceStore::production().await {
        Ok(store) => match store.gc_file_cas(grace) {
            Ok(report) => total.merge(report),
            Err(error) => tracing::warn!(error = %error, "全局文件 CAS GC 失败"),
        },
        Err(reason) => tracing::debug!(reason = %reason, "全局库未就绪，本轮跳过全局文件 CAS GC"),
    }
    if total.manifests_removed > 0 || total.blobs_removed > 0 || total.staging_removed > 0 {
        tracing::info!(
            manifests = total.manifests_removed,
            blobs = total.blobs_removed,
            staging = total.staging_removed,
            bytes = total.bytes_freed,
            "文件 CAS GC 完成"
        );
    }
    total
}

async fn gc_project(root: &std::path::Path, grace: Duration) -> Result<CasGcReport, String> {
    let manager = ProjectDatabaseManager::open(root, super::SQLITE_POOL_SIZE)
        .await
        .map_err(|error| format!("打开项目库失败：{error}"))?;
    let pool = manager.sqlite_pool();
    let keep: HashSet<String> = {
        let conn = pool.acquire().await.map_err(|error| error.to_string())?;
        let inner = conn.inner().map_err(|error| error.to_string())?;
        let mut stmt = inner
            .prepare("SELECT manifest_id FROM file_asset_versions")
            .map_err(|error| format!("准备读取文件版本索引失败：{error}"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("读取文件版本索引失败：{error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("汇总文件版本索引失败：{error}"))?
            .into_iter()
            .collect()
    };
    Ok(CasStore::new(root).gc(&keep, grace))
}
