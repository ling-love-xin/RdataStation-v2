//! 表资产快照索引：manifest / snapshot / version / checkout lease。
//!
//! 只读写 `project.db`。物理表在项目 `analytics.duckdb`（后续全局侧在全局库），
//! 本模块记录“哪张物理表承载哪份逻辑内容、属于哪个资产的哪一版”。

use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use shared::error::{CoreError, StorageError};

use crate::table_asset::manifest::TableManifest;

use super::AnalyticsResourceStore;

/// 快照所在实例。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotInstance {
    Project,
    Global,
}

impl SnapshotInstance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }

    fn parse(text: &str) -> rusqlite::Result<Self> {
        match text {
            "project" => Ok(Self::Project),
            "global" => Ok(Self::Global),
            _ => Err(rusqlite::Error::InvalidParameterName(format!(
                "未知表快照实例：{text}"
            ))),
        }
    }
}

/// 快照物理完好性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    Live,
    Missing,
}

impl SnapshotState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Missing => "missing",
        }
    }

    fn parse(text: &str) -> rusqlite::Result<Self> {
        match text {
            "live" => Ok(Self::Live),
            "missing" => Ok(Self::Missing),
            _ => Err(rusqlite::Error::InvalidParameterName(format!(
                "未知表快照状态：{text}"
            ))),
        }
    }
}

/// checkout 租约状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableLeaseState {
    Active,
    Consumed,
    Abandoned,
}

impl TableLeaseState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Consumed => "consumed",
            Self::Abandoned => "abandoned",
        }
    }

    fn parse(text: &str) -> rusqlite::Result<Self> {
        match text {
            "active" => Ok(Self::Active),
            "consumed" => Ok(Self::Consumed),
            "abandoned" => Ok(Self::Abandoned),
            _ => Err(rusqlite::Error::InvalidParameterName(format!(
                "未知表 checkout 状态：{text}"
            ))),
        }
    }
}

/// 一份物理快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSnapshot {
    pub snapshot_id: String,
    pub manifest_id: String,
    pub instance: SnapshotInstance,
    pub physical_schema: String,
    pub physical_table: String,
    pub state: SnapshotState,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
}

/// 新建快照时由调用方决定的部分；`manifest_id` 由 [`NewTableVersion`] 带入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTableSnapshot {
    pub snapshot_id: String,
    pub instance: SnapshotInstance,
    pub physical_schema: String,
    pub physical_table: String,
}

/// 提交一版表资产：manifest + snapshot + 版本索引，三者同一事务。
#[derive(Debug, Clone)]
pub struct NewTableVersion {
    pub asset_id: String,
    pub version: i32,
    pub parent_version: Option<i32>,
    pub manifest: TableManifest,
    pub snapshot: NewTableSnapshot,
}

/// 资产的某一版指向哪份快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAssetVersion {
    pub asset_id: String,
    pub version: i32,
    pub snapshot_id: String,
    pub manifest_id: String,
    pub parent_version: Option<i32>,
    pub created_at: DateTime<Utc>,
}

/// checkout 工作表租约。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCheckoutLease {
    pub lease_id: String,
    pub asset_id: String,
    pub base_version: i32,
    pub work_schema: String,
    pub work_table: String,
    pub state: TableLeaseState,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub consumed_version: Option<i32>,
}

impl AnalyticsResourceStore {
    /// 提交一版表资产：manifest / snapshot / version 同一事务。
    ///
    /// 同 manifest 重复提交（同内容再归档）会复用已有 manifest 行；snapshot 必须
    /// 是新的物理实例，版本 `(asset_id, version)` 冲突则整笔回滚。
    pub async fn commit_table_version(&self, input: &NewTableVersion) -> Result<(), CoreError> {
        if input.version <= 0 {
            return Err(tav_err("commit", "版本号必须从 1 开始"));
        }
        if input.snapshot.physical_schema.trim().is_empty()
            || input.snapshot.physical_table.trim().is_empty()
        {
            return Err(tav_err("commit", "快照缺少物理 schema/table"));
        }
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let now = Utc::now().to_rfc3339();
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| tav_err("begin", error))?;
        let result = (|| -> Result<(), CoreError> {
            Self::ensure_table_manifest_on(inner, &input.manifest, &now)?;
            Self::insert_table_snapshot_on(inner, &input.snapshot, &input.manifest.manifest_id, &now)?;
            inner
                .execute(
                    r#"
                    INSERT INTO table_asset_versions (
                        asset_id, version, snapshot_id, manifest_id, parent_version, created_at
                    ) VALUES (?, ?, ?, ?, ?, ?)
                    "#,
                    rusqlite::params![
                        &input.asset_id,
                        input.version,
                        &input.snapshot.snapshot_id,
                        &input.manifest.manifest_id,
                        input.parent_version,
                        &now,
                    ],
                )
                .map_err(|error| {
                    let text = error.to_string();
                    if text.contains("UNIQUE") || text.contains("unique") {
                        tav_err(
                            "insert_version",
                            format!(
                                "资产 {} 已有版本 {}，不能重复提交",
                                input.asset_id, input.version
                            ),
                        )
                    } else {
                        tav_err("insert_version", error)
                    }
                })?;
            Ok(())
        })();
        match result {
            Ok(()) => inner
                .execute_batch("COMMIT")
                .map_err(|error| tav_err("commit_tx", error)),
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 读取一份 manifest。
    pub async fn table_manifest(
        &self,
        manifest_id: &str,
    ) -> Result<Option<TableManifest>, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                r#"
                SELECT manifest_id, schema_hash, data_fingerprint, row_count, column_count, schema_json
                FROM table_manifests
                WHERE manifest_id = ?
                "#,
                rusqlite::params![manifest_id],
                |row| {
                    Ok(TableManifest {
                        manifest_id: row.get(0)?,
                        schema_hash: row.get(1)?,
                        data_fingerprint: row.get(2)?,
                        row_count: row.get(3)?,
                        column_count: row.get(4)?,
                        schema_json: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| tav_err("select_manifest", error))
    }

    /// 读取一份快照。
    pub async fn table_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<TableSnapshot>, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                r#"
                SELECT snapshot_id, manifest_id, instance, physical_schema, physical_table,
                       state, created_at, verified_at
                FROM table_snapshots
                WHERE snapshot_id = ?
                "#,
                rusqlite::params![snapshot_id],
                map_snapshot,
            )
            .optional()
            .map_err(|error| tav_err("select_snapshot", error))
    }

    /// 某资产的版本链（升序）。
    pub async fn table_asset_versions(
        &self,
        asset_id: &str,
    ) -> Result<Vec<TableAssetVersion>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
                SELECT asset_id, version, snapshot_id, manifest_id, parent_version, created_at
                FROM table_asset_versions
                WHERE asset_id = ?
                ORDER BY version ASC
                "#,
            )
            .map_err(|error| tav_err("prepare_versions", error))?;
        let rows = stmt
            .query_map(rusqlite::params![asset_id], |row| {
                let created_at: String = row.get(5)?;
                Ok(TableAssetVersion {
                    asset_id: row.get(0)?,
                    version: row.get(1)?,
                    snapshot_id: row.get(2)?,
                    manifest_id: row.get(3)?,
                    parent_version: row.get(4)?,
                    created_at: AnalyticsResourceStore::parse_datetime_sqlite(created_at)?,
                })
            })
            .map_err(|error| tav_err("query_versions", error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| tav_err("collect_versions", error))
    }

    /// 某资产某一版。
    pub async fn table_asset_version(
        &self,
        asset_id: &str,
        version: i32,
    ) -> Result<Option<TableAssetVersion>, CoreError> {
        Ok(self
            .table_asset_versions(asset_id)
            .await?
            .into_iter()
            .find(|item| item.version == version))
    }

    /// 创建 checkout 租约；同资产已有 active / 同工作表已被占用时由唯一索引拒绝。
    pub async fn create_table_lease(
        &self,
        asset_id: &str,
        base_version: i32,
        work_schema: &str,
        work_table: &str,
    ) -> Result<TableCheckoutLease, CoreError> {
        let conn = self.get_conn().await?;
        let lease_id = format!("tlease_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        conn.inner()?
            .execute(
                r#"
                INSERT INTO table_checkout_leases (
                    lease_id, asset_id, base_version, work_schema, work_table,
                    state, created_at
                ) VALUES (?, ?, ?, ?, ?, 'active', ?)
                "#,
                rusqlite::params![
                    &lease_id,
                    asset_id,
                    base_version,
                    work_schema,
                    work_table,
                    &now,
                ],
            )
            .map_err(|error| {
                let text = error.to_string();
                if text.contains("UNIQUE") || text.contains("unique") {
                    tav_err(
                        "create_lease",
                        format!(
                            "资产 {asset_id} 已有活跃编辑工作表，或工作表 {work_schema}.{work_table} 已被占用"
                        ),
                    )
                } else {
                    tav_err("create_lease", error)
                }
            })?;
        self.active_table_lease(asset_id)
            .await?
            .ok_or_else(|| tav_err("create_lease", "租约写入后读不到 active 行"))
    }

    /// 某资产当前 active 的编辑租约。
    pub async fn active_table_lease(
        &self,
        asset_id: &str,
    ) -> Result<Option<TableCheckoutLease>, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                r#"
                SELECT lease_id, asset_id, base_version, work_schema, work_table,
                       state, created_at, closed_at, consumed_version
                FROM table_checkout_leases
                WHERE asset_id = ? AND state = 'active'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                rusqlite::params![asset_id],
                map_lease,
            )
            .optional()
            .map_err(|error| tav_err("active_lease", error))
    }

    /// 所有 active 租约（按资产 id 收成映射，供刷新批量回填）。
    pub async fn active_table_leases(
        &self,
    ) -> Result<std::collections::HashMap<String, TableCheckoutLease>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
                SELECT lease_id, asset_id, base_version, work_schema, work_table,
                       state, created_at, closed_at, consumed_version
                FROM table_checkout_leases
                WHERE state = 'active'
                ORDER BY created_at DESC
                "#,
            )
            .map_err(|error| tav_err("prepare_active_leases", error))?;
        let rows = stmt
            .query_map([], map_lease)
            .map_err(|error| tav_err("query_active_leases", error))?;
        let mut leases = std::collections::HashMap::new();
        for row in rows {
            let lease = row.map_err(|error| tav_err("collect_active_leases", error))?;
            leases.entry(lease.asset_id.clone()).or_insert(lease);
        }
        Ok(leases)
    }

    /// 消费租约：checkout 内容已归档成新版本。
    pub async fn consume_table_lease(
        &self,
        lease_id: &str,
        consumed_version: i32,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                r#"
                UPDATE table_checkout_leases
                SET state = 'consumed', closed_at = ?, consumed_version = ?
                WHERE lease_id = ? AND state = 'active'
                "#,
                rusqlite::params![Utc::now().to_rfc3339(), consumed_version, lease_id],
            )
            .map_err(|error| tav_err("consume_lease", error))?;
        if affected == 0 {
            return Err(tav_err("consume_lease", "租约不存在或已经结束"));
        }
        Ok(())
    }

    /// 放弃 checkout：工作表可以留给用户清理，但版本链不再认它。
    pub async fn abandon_table_lease(&self, lease_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                r#"
                UPDATE table_checkout_leases
                SET state = 'abandoned', closed_at = ?
                WHERE lease_id = ? AND state = 'active'
                "#,
                rusqlite::params![Utc::now().to_rfc3339(), lease_id],
            )
            .map_err(|error| tav_err("abandon_lease", error))?;
        if affected == 0 {
            return Err(tav_err("abandon_lease", "租约不存在或已经结束"));
        }
        Ok(())
    }

    /// 某快照被多少个版本引用（GC 的保留判据之一）。
    pub async fn snapshot_version_refs(&self, snapshot_id: &str) -> Result<i64, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                "SELECT COUNT(*) FROM table_asset_versions WHERE snapshot_id = ?",
                rusqlite::params![snapshot_id],
                |row| row.get(0),
            )
            .map_err(|error| tav_err("snapshot_refs", error))
    }

    fn ensure_table_manifest_on(
        conn: &rusqlite::Connection,
        manifest: &TableManifest,
        now: &str,
    ) -> Result<(), CoreError> {
        conn.execute(
            r#"
            INSERT OR IGNORE INTO table_manifests (
                manifest_id, schema_hash, data_fingerprint,
                row_count, column_count, schema_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
            rusqlite::params![
                &manifest.manifest_id,
                &manifest.schema_hash,
                &manifest.data_fingerprint,
                manifest.row_count,
                manifest.column_count,
                &manifest.schema_json,
                now,
            ],
        )
        .map_err(|error| tav_err("insert_manifest", error))?;
        let existing: (String, String, i64, i64) = conn
            .query_row(
                r#"
                SELECT schema_hash, data_fingerprint, row_count, column_count
                FROM table_manifests
                WHERE manifest_id = ?
                "#,
                rusqlite::params![&manifest.manifest_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|error| tav_err("verify_manifest", error))?;
        if existing.0 != manifest.schema_hash
            || existing.1 != manifest.data_fingerprint
            || existing.2 != manifest.row_count
            || existing.3 != manifest.column_count
        {
            return Err(tav_err(
                "insert_manifest",
                format!(
                    "manifest {} 已存在但内容不一致（哈希冲突或写入损坏）",
                    manifest.manifest_id
                ),
            ));
        }
        Ok(())
    }

    fn insert_table_snapshot_on(
        conn: &rusqlite::Connection,
        snapshot: &NewTableSnapshot,
        manifest_id: &str,
        now: &str,
    ) -> Result<(), CoreError> {
        conn.execute(
            r#"
            INSERT OR IGNORE INTO table_snapshots (
                snapshot_id, manifest_id, instance, physical_schema, physical_table,
                state, created_at
            ) VALUES (?, ?, ?, ?, ?, 'live', ?)
            "#,
            rusqlite::params![
                &snapshot.snapshot_id,
                manifest_id,
                snapshot.instance.as_str(),
                &snapshot.physical_schema,
                &snapshot.physical_table,
                now,
            ],
        )
        .map_err(|error| tav_err("insert_snapshot", error))?;
        let existing: (String, String, String, String) = conn
            .query_row(
                r#"
                SELECT manifest_id, instance, physical_schema, physical_table
                FROM table_snapshots
                WHERE snapshot_id = ?
                "#,
                rusqlite::params![&snapshot.snapshot_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|error| tav_err("verify_snapshot", error))?;
        if existing.0 != manifest_id
            || existing.1 != snapshot.instance.as_str()
            || existing.2 != snapshot.physical_schema
            || existing.3 != snapshot.physical_table
        {
            return Err(tav_err(
                "insert_snapshot",
                format!(
                    "快照 {} 已存在但物理定位或 manifest 不一致",
                    snapshot.snapshot_id
                ),
            ));
        }
        Ok(())
    }
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<TableSnapshot> {
    let created_at: String = row.get(6)?;
    let verified_at: Option<String> = row.get(7)?;
    Ok(TableSnapshot {
        snapshot_id: row.get(0)?,
        manifest_id: row.get(1)?,
        instance: SnapshotInstance::parse(row.get::<_, String>(2)?.as_str())?,
        physical_schema: row.get(3)?,
        physical_table: row.get(4)?,
        state: SnapshotState::parse(row.get::<_, String>(5)?.as_str())?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(created_at)?,
        verified_at: verified_at
            .and_then(|text| AnalyticsResourceStore::parse_datetime(text).ok()),
    })
}

fn map_lease(row: &rusqlite::Row<'_>) -> rusqlite::Result<TableCheckoutLease> {
    let created_at: String = row.get(6)?;
    let closed_at: Option<String> = row.get(7)?;
    Ok(TableCheckoutLease {
        lease_id: row.get(0)?,
        asset_id: row.get(1)?,
        base_version: row.get(2)?,
        work_schema: row.get(3)?,
        work_table: row.get(4)?,
        state: TableLeaseState::parse(row.get::<_, String>(5)?.as_str())?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(created_at)?,
        closed_at: closed_at.and_then(|text| AnalyticsResourceStore::parse_datetime(text).ok()),
        consumed_version: row.get(8)?,
    })
}

fn tav_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "table_asset_snapshots".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArchiveBinding, ArchiveKind, NewArchiveInput, PayloadMode};
    use crate::table_asset::manifest::{DataFingerprint, TableColumn, TableSchema};

    async fn seeded_asset(store: &AnalyticsResourceStore) -> String {
        let input = NewArchiveInput {
            resource_type: "analysis".to_string(),
            name: "订单快照".to_string(),
            alias: None,
            kind: ArchiveKind::AnalyticsTable,
            content_hash: "seed".to_string(),
            definition_sql: None,
            row_count: Some(10),
            column_count: Some(2),
            file_rel_path: String::new(),
            payload_mode: PayloadMode::Copy,
            source_path: None,
            file_size: None,
            structured: true,
            binding: ArchiveBinding {
                promoted_from: None,
                source_connection_id: None,
                source_table: Some("res_seed".to_string()),
            },
            scope: "project".to_string(),
        };
        store
            .insert_table_archive_with_event(input, "res_seed", None)
            .await
            .expect("seed asset")
            .id
    }

    async fn bundle(
        asset_id: &str,
        version: i32,
        rows: i64,
        snapshot_suffix: &str,
    ) -> NewTableVersion {
        let schema = TableSchema::new(vec![
            TableColumn::new("id", "BIGINT", false, None, 0),
            TableColumn::new("amount", "DOUBLE", true, None, 1),
        ])
        .expect("schema");
        let data = DataFingerprint::new(rows, 2, "7", "3", Some("1".to_string()), Some("9".to_string()));
        let manifest = TableManifest::build(&schema, &data).expect("manifest");
        NewTableVersion {
            asset_id: asset_id.to_string(),
            version,
            parent_version: (version > 1).then_some(version - 1),
            manifest,
            snapshot: NewTableSnapshot {
                snapshot_id: format!("snap_{snapshot_suffix}"),
                instance: SnapshotInstance::Project,
                physical_schema: "rds_archive".to_string(),
                physical_table: format!("arc_{snapshot_suffix}"),
            },
        }
    }

    #[tokio::test]
    async fn commit_reuses_manifest_and_links_versions() {
        let (store, dir) = crate::tests::create_test_store().await;
        let asset_id = seeded_asset(&store).await;

        store
            .commit_table_version(&bundle(&asset_id, 1, 10, "v1").await)
            .await
            .expect("v1");
        store
            .commit_table_version(&bundle(&asset_id, 2, 10, "v2").await)
            .await
            .expect("v2");

        let versions = store.table_asset_versions(&asset_id).await.expect("versions");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].manifest_id, versions[1].manifest_id, "同内容共享 manifest");
        assert_eq!(versions[1].parent_version, Some(1));

        let manifest = store
            .table_manifest(&versions[0].manifest_id)
            .await
            .expect("manifest")
            .expect("manifest row");
        assert_eq!(manifest.row_count, 10);

        let snapshot = store
            .table_snapshot(&versions[1].snapshot_id)
            .await
            .expect("snapshot")
            .expect("snapshot row");
        assert_eq!(snapshot.manifest_id, manifest.manifest_id);
        assert_eq!(snapshot.state, SnapshotState::Live);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn duplicate_version_rolls_back_without_orphans() {
        let (store, dir) = crate::tests::create_test_store().await;
        let asset_id = seeded_asset(&store).await;
        store
            .commit_table_version(&bundle(&asset_id, 1, 10, "v1").await)
            .await
            .expect("v1");

        let duplicate = bundle(&asset_id, 1, 11, "v2").await;
        let manifest_id = duplicate.manifest.manifest_id.clone();
        let result = store.commit_table_version(&duplicate).await;
        assert!(result.is_err(), "重复版本必须失败");
        assert!(
            store
                .table_manifest(&manifest_id)
                .await
                .expect("manifest")
                .is_none(),
            "回滚后不能留下 manifest"
        );
        assert_eq!(
            store.table_asset_versions(&asset_id).await.expect("versions").len(),
            1
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn lease_is_single_active_and_reusable_after_close() {
        let (store, dir) = crate::tests::create_test_store().await;
        let asset_id = seeded_asset(&store).await;
        store
            .commit_table_version(&bundle(&asset_id, 1, 10, "v1").await)
            .await
            .expect("v1");

        let lease = store
            .create_table_lease(&asset_id, 1, "rds_work", "wrk_asset_lease")
            .await
            .expect("lease");
        assert_eq!(store.active_table_leases().await.expect("active").len(), 1);

        let again = store
            .create_table_lease(&asset_id, 1, "rds_work", "wrk_asset_lease_2")
            .await;
        assert!(again.is_err(), "同资产只能有一条 active 租约");

        store
            .consume_table_lease(&lease.lease_id, 2)
            .await
            .expect("consume");
        assert!(store
            .active_table_lease(&asset_id)
            .await
            .expect("active")
            .is_none());

        let next = store
            .create_table_lease(&asset_id, 2, "rds_work", "wrk_asset_lease_3")
            .await
            .expect("new lease");
        store
            .abandon_table_lease(&next.lease_id)
            .await
            .expect("abandon");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn snapshot_refs_count_versions() {
        let (store, dir) = crate::tests::create_test_store().await;
        let asset_id = seeded_asset(&store).await;
        let first = bundle(&asset_id, 1, 10, "v1").await;
        let snapshot_id = first.snapshot.snapshot_id.clone();
        store.commit_table_version(&first).await.expect("v1");
        assert_eq!(
            store.snapshot_version_refs(&snapshot_id).await.expect("refs"),
            1
        );

        std::fs::remove_dir_all(dir).ok();
    }
}
