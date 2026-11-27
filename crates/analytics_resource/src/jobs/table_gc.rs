//! 表快照 GC 作业：项目库 + 全局库各跑一遍。
//!
//! 触发点：刷新任务里按 `GC_INTERVAL` 节流（前台无感），也提供手动入队入口。
//! 回收单位是「快照」不是「表」：只有版本行引用全部消失、候选超过 grace，
//! 才 DROP 物理表；失败只保留候选，下次重试。

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use engine::persistence::project_db::ProjectDatabaseManager;

use crate::store::table_asset::SnapshotInstance;
use crate::table_asset::gc::{self, GcReport};

/// 首次发现无引用后再等多久才真正回收。给崩溃恢复、并发读留窗口。
pub(crate) const GC_GRACE: Duration = Duration::from_secs(24 * 3600);

/// 两次自动 GC 的最小间隔（刷新很频繁，不能每次刷新都全库扫一遍）。
pub(crate) const GC_INTERVAL: Duration = Duration::from_secs(3600);

/// 跑一轮项目 + 全局 GC。任一侧失败只记日志，不影响另一侧。
pub(crate) async fn run_table_gc(project_root: Option<PathBuf>, grace: Duration) -> GcReport {
    let mut total = GcReport::default();
    if let Some(root) = project_root {
        match gc_project(&root, grace).await {
            Ok(report) => total.merge(report),
            Err(error) => {
                tracing::warn!(error = %error, root = %root.display(), "项目表快照 GC 失败")
            }
        }
    }
    match crate::global::GlobalInstanceStore::production().await {
        Ok(store) => match store.gc_table_snapshots(grace).await {
            Ok(report) => total.merge(report),
            Err(error) => tracing::warn!(error = %error, "全局表快照 GC 失败"),
        },
        Err(reason) => tracing::debug!(reason = %reason, "全局库未就绪，本轮跳过全局 GC"),
    }
    if total.dropped > 0 || total.failed > 0 {
        tracing::info!(
            marked = total.marked,
            dropped = total.dropped,
            kept = total.kept,
            failed = total.failed,
            "表快照 GC 完成"
        );
    }
    total
}

async fn gc_project(root: &std::path::Path, grace: Duration) -> Result<GcReport, String> {
    let manager = ProjectDatabaseManager::open(root, super::SQLITE_POOL_SIZE)
        .await
        .map_err(|error| format!("打开项目库失败：{error}"))?;
    let pool = manager.sqlite_pool();
    let instance = SnapshotInstance::Project.as_str();
    let now = Utc::now();

    let marked = {
        let conn = pool.acquire().await.map_err(|e| e.to_string())?;
        let inner = conn.inner().map_err(|e| e.to_string())?;
        gc::clear_referenced_candidates(inner, instance)
            .and_then(|_| gc::mark_candidates(inner, instance, now))
            .map_err(|e| e.to_string())?
    };
    let cutoff =
        now - chrono::Duration::from_std(grace).unwrap_or_else(|_| chrono::Duration::zero());
    let candidates = {
        let conn = pool.acquire().await.map_err(|e| e.to_string())?;
        let inner = conn.inner().map_err(|e| e.to_string())?;
        gc::due_candidates(inner, instance, cutoff).map_err(|e| e.to_string())?
    };

    let Some(duck_handle) = manager.duckdb_conn() else {
        return Err("项目 DuckDB 写连接未就绪".to_string());
    };
    let mut report = GcReport {
        marked,
        ..Default::default()
    };
    for candidate in candidates {
        let unreferenced = {
            let conn = pool.acquire().await.map_err(|e| e.to_string())?;
            let inner = conn.inner().map_err(|e| e.to_string())?;
            gc::still_unreferenced(inner, &candidate.snapshot_id).map_err(|e| e.to_string())?
        };
        if !unreferenced {
            let conn = pool.acquire().await.map_err(|e| e.to_string())?;
            let inner = conn.inner().map_err(|e| e.to_string())?;
            inner
                .execute(
                    "UPDATE table_snapshots SET gc_candidate_at = NULL WHERE snapshot_id = ?1",
                    rusqlite::params![&candidate.snapshot_id],
                )
                .map_err(|e| e.to_string())?;
            report.kept += 1;
            continue;
        }
        {
            let hold = duck_handle.acquire().await.map_err(|e| e.to_string())?;
            let mut guard = hold.lock().await;
            let Some(duck) = guard.as_mut() else {
                tracing::warn!("项目 DuckDB 写连接已释放，快照 GC 本轮跳过");
                report.failed += 1;
                continue;
            };
            let sql = format!(
                "DROP TABLE IF EXISTS {}.{}.{};",
                crate::global::quote_ident(shared::duckdb_root::PROJECT_ALIAS),
                crate::global::quote_ident(&candidate.physical_schema),
                crate::global::quote_ident(&candidate.physical_table),
            );
            if let Err(error) = duck.execute_batch(&sql) {
                tracing::warn!(
                    error = %error,
                    snapshot = %candidate.snapshot_id,
                    "项目快照 GC 删除物理表失败，保留候选下次重试"
                );
                report.failed += 1;
                continue;
            }
        }
        let finished = {
            let conn = pool.acquire().await.map_err(|e| e.to_string())?;
            let inner = conn.inner().map_err(|e| e.to_string())?;
            gc::finish_snapshot(inner, &candidate).map_err(|e| e.to_string())?
        };
        if finished {
            report.dropped += 1;
        } else {
            report.kept += 1;
        }
    }
    Ok(report)
}
