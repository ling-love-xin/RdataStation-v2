//! 表快照 GC 元数据助手（项目库 / 全局库共用）。
//!
//! 只做 SQLite 侧的候选标记与结算；真正 `DROP` 物理表由调用方用自己的
//! DuckDB 连接执行——两侧连接所有权不同，混在一起会绕开单写者约束。
//!
//! 状态机：无引用 → 首次 GC 记 `gc_candidate_at` → 超过 grace 且仍无引用 → 允许 DROP →
//! DROP 成功后删 snapshot 行、再删无引用的 manifest。任何一步失败都保留候选，下次重试。

use chrono::{DateTime, Utc};
use rusqlite::Connection;

/// 一份等待回收的快照（物理定位 + manifest 归属）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcCandidate {
    pub snapshot_id: String,
    pub manifest_id: String,
    pub physical_schema: String,
    pub physical_table: String,
}

/// 一次 GC 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GcReport {
    /// 新标记为候选的快照数。
    pub marked: usize,
    /// DROP 并删掉元数据的快照数。
    pub dropped: usize,
    /// 仍被引用 / 未到 grace，保留的数。
    pub kept: usize,
    /// DROP 或结算失败，保留下次重试的数。
    pub failed: usize,
}

impl GcReport {
    pub fn merge(&mut self, other: GcReport) {
        self.marked += other.marked;
        self.dropped += other.dropped;
        self.kept += other.kept;
        self.failed += other.failed;
    }
}

/// 首次发现无引用快照时写候选时刻；已有候选或仍被引用的不动。
pub fn mark_candidates(
    conn: &Connection,
    instance: &str,
    now: DateTime<Utc>,
) -> rusqlite::Result<usize> {
    conn.execute(
        r#"
        UPDATE table_snapshots
        SET gc_candidate_at = ?1
        WHERE instance = ?2
          AND gc_candidate_at IS NULL
          AND snapshot_id NOT IN (SELECT snapshot_id FROM table_asset_versions)
        "#,
        rusqlite::params![now.to_rfc3339(), instance],
    )
}

/// 快照重新出现引用（恢复 / fork / 新版本复用）时撤销候选。
pub fn clear_referenced_candidates(conn: &Connection, instance: &str) -> rusqlite::Result<usize> {
    conn.execute(
        r#"
        UPDATE table_snapshots
        SET gc_candidate_at = NULL
        WHERE instance = ?1
          AND gc_candidate_at IS NOT NULL
          AND snapshot_id IN (SELECT snapshot_id FROM table_asset_versions)
        "#,
        rusqlite::params![instance],
    )
}

/// 已过 grace 的候选（候选时刻 <= cutoff）。
pub fn due_candidates(
    conn: &Connection,
    instance: &str,
    cutoff: DateTime<Utc>,
) -> rusqlite::Result<Vec<GcCandidate>> {
    let mut stmt = conn.prepare(
        r#"
            SELECT snapshot_id, manifest_id, physical_schema, physical_table
            FROM table_snapshots
            WHERE instance = ?1
              AND gc_candidate_at IS NOT NULL
              AND gc_candidate_at <= ?2
            ORDER BY gc_candidate_at ASC
            "#,
    )?;
    let rows = stmt.query_map(rusqlite::params![instance, cutoff.to_rfc3339()], |row| {
        Ok(GcCandidate {
            snapshot_id: row.get(0)?,
            manifest_id: row.get(1)?,
            physical_schema: row.get(2)?,
            physical_table: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// DROP 前的最后一道检查：版本行仍引用就绝不删。
pub fn still_unreferenced(conn: &Connection, snapshot_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM table_asset_versions WHERE snapshot_id = ?1",
        rusqlite::params![snapshot_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count == 0)
}

/// DROP 成功后删 snapshot 行；再用 `manifest_id` 清掉无任何快照 / 版本引用的 manifest。
///
/// 返回 `true` = snapshot 行已删除；`false` = 期间又有版本引用，保留。
pub fn finish_snapshot(conn: &Connection, candidate: &GcCandidate) -> rusqlite::Result<bool> {
    let removed = conn.execute(
        "DELETE FROM table_snapshots WHERE snapshot_id = ?1 \
         AND snapshot_id NOT IN (SELECT snapshot_id FROM table_asset_versions)",
        rusqlite::params![&candidate.snapshot_id],
    )?;
    if removed == 0 {
        return Ok(false);
    }
    conn.execute(
        "DELETE FROM table_manifests WHERE manifest_id = ?1 \
         AND NOT EXISTS (SELECT 1 FROM table_asset_versions WHERE manifest_id = ?1) \
         AND NOT EXISTS (SELECT 1 FROM table_snapshots WHERE manifest_id = ?1)",
        rusqlite::params![&candidate.manifest_id],
    )?;
    Ok(true)
}
