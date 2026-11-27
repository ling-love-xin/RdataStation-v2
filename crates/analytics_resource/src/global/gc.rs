//! 全局表快照 GC：全局库侧执行器。
//!
//! 分两段：先标记 / 撤销候选（SQLite），再把过 grace 且仍无版本引用的快照
//! `DROP` 并删元数据。DROP 用全局连接；失败只保留下次重试，绝不先删元数据。

use std::time::Duration;

use chrono::Utc;

use super::{GlobalInstanceStore, quote_ident, sqlite_err};
use crate::store::table_asset::SnapshotInstance;
use crate::table_asset::gc::{self, GcReport};

impl GlobalInstanceStore {
    /// 回收全局侧无引用的表快照（`grace` = 首次发现无引用后至少保留多久）。
    pub async fn gc_table_snapshots(&self, grace: Duration) -> Result<GcReport, super::CoreError> {
        let instance = SnapshotInstance::Global.as_str();
        let now = Utc::now();
        let marked = self.with_sqlite(|conn| {
            gc::clear_referenced_candidates(conn, instance)
                .and_then(|_| gc::mark_candidates(conn, instance, now))
                .map_err(sqlite_err("mark_global_gc_candidates"))
        })?;
        let cutoff =
            now - chrono::Duration::from_std(grace).unwrap_or_else(|_| chrono::Duration::zero());
        let candidates = self.with_sqlite(|conn| {
            gc::due_candidates(conn, instance, cutoff)
                .map_err(sqlite_err("query_global_gc_candidates"))
        })?;

        let mut report = GcReport {
            marked,
            ..Default::default()
        };
        for candidate in candidates {
            let unreferenced = self.with_sqlite(|conn| {
                gc::still_unreferenced(conn, &candidate.snapshot_id)
                    .map_err(sqlite_err("check_global_gc_reference"))
            })?;
            if !unreferenced {
                self.with_sqlite(|conn| {
                    conn.execute(
                        "UPDATE table_snapshots SET gc_candidate_at = NULL WHERE snapshot_id = ?1",
                        rusqlite::params![&candidate.snapshot_id],
                    )
                    .map(|_| ())
                    .map_err(sqlite_err("clear_global_gc_candidate"))
                })?;
                report.kept += 1;
                continue;
            }
            let dropped = self
                .with_duckdb(|conn| {
                    conn.execute_batch(&format!(
                        "DROP TABLE IF EXISTS {}.{}.{};",
                        quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                        quote_ident(&candidate.physical_schema),
                        quote_ident(&candidate.physical_table),
                    ))
                    .map_err(|error| {
                        super::duckdb_err("gc_drop_global_snapshot", &error.to_string())
                    })
                })
                .await;
            if let Err(error) = dropped {
                tracing::warn!(
                    error = %error,
                    snapshot = %candidate.snapshot_id,
                    "全局快照 GC 删除物理表失败，保留候选下次重试"
                );
                report.failed += 1;
                continue;
            }
            let finished = self.with_sqlite(|conn| {
                gc::finish_snapshot(conn, &candidate)
                    .map_err(sqlite_err("finish_global_gc_snapshot"))
            })?;
            if finished {
                report.dropped += 1;
            } else {
                tracing::warn!(
                    snapshot = %candidate.snapshot_id,
                    "全局快照 DROP 后重新出现版本引用，保留 snapshot 行"
                );
                report.kept += 1;
            }
        }
        Ok(report)
    }
}
